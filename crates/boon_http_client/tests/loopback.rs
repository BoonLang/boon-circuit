use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::http::header::{AUTHORIZATION, COOKIE, HeaderName, HeaderValue};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use boon_http_client::{
    CancellationToken, ClientConfig, ConfigError, EndpointCapability, EndpointName, ExecuteError,
    Header, HttpClient, HttpMethod, HttpRequest, LimitKind, LocalHttpTestPermit, RedirectPolicy,
    RequestTimeouts, RequestViolation, TimeoutKind,
};
use tokio::net::TcpListener;
use tokio::task::JoinHandle;

struct RunningServer {
    address: SocketAddr,
    task: JoinHandle<()>,
}

impl RunningServer {
    async fn start(app: Router) -> Self {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        Self { address, task }
    }
}

impl Drop for RunningServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

fn endpoint(name: &str, address: SocketAddr) -> EndpointCapability {
    EndpointCapability::local_http_for_tests(
        EndpointName::new(name).unwrap(),
        address,
        "/api/",
        LocalHttpTestPermit::explicitly_enable_for_tests(),
    )
    .unwrap()
}

fn request(endpoint: &str, path: &str) -> HttpRequest {
    let mut request = HttpRequest::new(EndpointName::new(endpoint).unwrap(), HttpMethod::Get);
    request.path_segments.push(path.to_owned());
    request
}

fn standard_app() -> Router {
    Router::new()
        .route("/api/success", get(success))
        .route("/api/large", get(large))
        .route("/api/many-headers", get(many_headers))
        .route("/api/slow", get(slow))
        .route(
            "/api/redirect-success",
            get(|| async { Redirect::temporary("/api/success") }),
        )
        .route(
            "/api/redirect-outside",
            get(|| async { Redirect::temporary("/private") }),
        )
        .route(
            "/api/redirect-external",
            get(|| async { Redirect::temporary("http://169.254.169.254/latest/meta-data/") }),
        )
        .route("/private", get(|| async { "must not be reached" }))
}

async fn success() -> Response {
    let mut response = Response::new(Body::from("success-body"));
    response.headers_mut().append(
        HeaderName::from_static("x-order"),
        HeaderValue::from_static("z"),
    );
    response.headers_mut().append(
        HeaderName::from_static("x-alpha"),
        HeaderValue::from_static("middle"),
    );
    response.headers_mut().append(
        HeaderName::from_static("x-order"),
        HeaderValue::from_static("a"),
    );
    response
}

async fn large() -> Response {
    Response::new(Body::from(vec![b'x'; 128]))
}

async fn many_headers() -> Response {
    let mut response = Response::new(Body::from("headers"));
    response.headers_mut().append(
        HeaderName::from_static("x-first"),
        HeaderValue::from_static("one"),
    );
    response.headers_mut().append(
        HeaderName::from_static("x-second"),
        HeaderValue::from_static("two"),
    );
    response
}

async fn slow() -> &'static str {
    tokio::time::sleep(Duration::from_millis(300)).await;
    "eventually"
}

#[tokio::test]
async fn loopback_success_is_structural_and_deterministic() {
    let server = RunningServer::start(standard_app()).await;
    let mut config = ClientConfig::new(vec![endpoint("primary", server.address)]);
    config.redirects = RedirectPolicy::Follow { max_hops: 3 };
    let client = HttpClient::new(config).unwrap();

    let response = client
        .execute(
            request("primary", "redirect-success"),
            &CancellationToken::new(),
        )
        .await
        .unwrap();

    assert_eq!(response.status, 200);
    assert_eq!(response.body, b"success-body");
    assert_eq!(response.final_endpoint.as_str(), "primary");
    assert_eq!(response.redirects_followed, 1);
    assert!(response.headers.windows(2).all(|headers| {
        (&headers[0].name, &headers[0].value) <= (&headers[1].name, &headers[1].value)
    }));
    let order_values: Vec<_> = response
        .headers
        .iter()
        .filter(|header| header.name == "x-order")
        .map(|header| header.value.as_slice())
        .collect();
    assert_eq!(order_values, [b"a".as_slice(), b"z".as_slice()]);
}

#[tokio::test]
async fn request_and_response_body_limits_fail_closed() {
    let server = RunningServer::start(standard_app()).await;
    let mut config = ClientConfig::new(vec![endpoint("primary", server.address)]);
    config.limits.max_request_body_bytes = 4;
    config.limits.max_response_body_bytes = 16;
    let client = HttpClient::new(config).unwrap();

    let response_error = client
        .execute(request("primary", "large"), &CancellationToken::new())
        .await
        .unwrap_err();
    assert_eq!(
        response_error,
        ExecuteError::LimitExceeded {
            kind: LimitKind::ResponseBodyBytes,
            limit: 16,
        }
    );

    let mut oversized = HttpRequest::new(EndpointName::new("primary").unwrap(), HttpMethod::Post);
    oversized.path_segments.push("success".to_owned());
    oversized.body = vec![0; 5];
    let request_error = client
        .execute(oversized, &CancellationToken::new())
        .await
        .unwrap_err();
    assert_eq!(
        request_error,
        ExecuteError::LimitExceeded {
            kind: LimitKind::RequestBodyBytes,
            limit: 4,
        }
    );
}

#[tokio::test]
async fn request_and_response_header_limits_fail_closed() {
    let server = RunningServer::start(standard_app()).await;
    let mut config = ClientConfig::new(vec![endpoint("primary", server.address)]);
    config.limits.max_request_headers = 1;
    config.limits.max_response_headers = 1;
    let client = HttpClient::new(config).unwrap();

    let response_error = client
        .execute(
            request("primary", "many-headers"),
            &CancellationToken::new(),
        )
        .await
        .unwrap_err();
    assert_eq!(
        response_error,
        ExecuteError::LimitExceeded {
            kind: LimitKind::ResponseHeaderCount,
            limit: 1,
        }
    );

    let mut outbound = request("primary", "success");
    outbound.headers = vec![Header::new("x-one", "1"), Header::new("x-two", "2")];
    let request_error = client
        .execute(outbound, &CancellationToken::new())
        .await
        .unwrap_err();
    assert_eq!(
        request_error,
        ExecuteError::LimitExceeded {
            kind: LimitKind::RequestHeaderCount,
            limit: 1,
        }
    );
}

#[tokio::test]
async fn overall_timeout_and_cancellation_abort_slow_requests() {
    let server = RunningServer::start(standard_app()).await;
    let mut timeout_config = ClientConfig::new(vec![endpoint("slow", server.address)]);
    timeout_config.timeouts.connect = Duration::from_millis(50);
    timeout_config.timeouts.overall = Duration::from_millis(80);
    let timeout_client = HttpClient::new(timeout_config).unwrap();

    let timeout_error = timeout_client
        .execute(request("slow", "slow"), &CancellationToken::new())
        .await
        .unwrap_err();
    assert_eq!(timeout_error, ExecuteError::OverallTimeout);

    let mut cancellation_config = ClientConfig::new(vec![endpoint("slow", server.address)]);
    cancellation_config.timeouts.connect = Duration::from_millis(100);
    cancellation_config.timeouts.overall = Duration::from_secs(2);
    let cancellation_client = HttpClient::new(cancellation_config).unwrap();
    let cancellation = CancellationToken::new();
    let trigger = cancellation.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(30)).await;
        trigger.cancel();
    });

    let cancellation_error = cancellation_client
        .execute(request("slow", "slow"), &cancellation)
        .await
        .unwrap_err();
    assert_eq!(cancellation_error, ExecuteError::Cancelled);
}

#[tokio::test]
async fn request_timeouts_may_tighten_but_not_expand_host_limits() {
    let server = RunningServer::start(standard_app()).await;
    let mut config = ClientConfig::new(vec![endpoint("slow", server.address)]);
    config.timeouts.connect = Duration::from_millis(100);
    config.timeouts.overall = Duration::from_millis(500);
    let client = HttpClient::new(config).unwrap();

    let timeout_error = client
        .execute_with_timeouts(
            request("slow", "slow"),
            &CancellationToken::new(),
            RequestTimeouts {
                connect: Duration::from_millis(20),
                overall: Duration::from_millis(25),
            },
        )
        .await
        .unwrap_err();
    assert_eq!(timeout_error, ExecuteError::OverallTimeout);

    let connect_limit_error = client
        .execute_with_timeouts(
            request("slow", "success"),
            &CancellationToken::new(),
            RequestTimeouts {
                connect: Duration::from_millis(101),
                overall: Duration::from_millis(500),
            },
        )
        .await
        .unwrap_err();
    assert_eq!(
        connect_limit_error,
        ExecuteError::TimeoutLimitExceeded {
            kind: TimeoutKind::Connect,
            limit_ms: 100,
        }
    );

    let overall_limit_error = client
        .execute_with_timeouts(
            request("slow", "success"),
            &CancellationToken::new(),
            RequestTimeouts {
                connect: Duration::from_millis(100),
                overall: Duration::from_millis(501),
            },
        )
        .await
        .unwrap_err();
    assert_eq!(
        overall_limit_error,
        ExecuteError::TimeoutLimitExceeded {
            kind: TimeoutKind::Overall,
            limit_ms: 500,
        }
    );
}

#[tokio::test]
async fn absolute_inputs_and_unlisted_redirects_are_rejected() {
    let server = RunningServer::start(standard_app()).await;
    let mut config = ClientConfig::new(vec![endpoint("primary", server.address)]);
    config.redirects = RedirectPolicy::Follow { max_hops: 2 };
    let client = HttpClient::new(config).unwrap();

    for path in ["redirect-outside", "redirect-external"] {
        let error = client
            .execute(request("primary", path), &CancellationToken::new())
            .await
            .unwrap_err();
        assert_eq!(error, ExecuteError::RedirectDestinationNotAllowed);
    }

    let mut absolute_path = request("primary", "ignored");
    absolute_path.path_segments = vec!["http://169.254.169.254/".to_owned()];
    let error = client
        .execute(absolute_path, &CancellationToken::new())
        .await
        .unwrap_err();
    assert_eq!(
        error,
        ExecuteError::InvalidRequest(RequestViolation::InvalidPathSegment)
    );

    assert!(matches!(
        EndpointCapability::https(
            EndpointName::new("metadata").unwrap(),
            "https://169.254.169.254/latest/"
        ),
        Err(ConfigError::AddressNotAllowed)
    ));
}

#[tokio::test]
async fn dns_cannot_rebind_a_production_endpoint_to_loopback() {
    let server = RunningServer::start(standard_app()).await;
    let capability = EndpointCapability::https(
        EndpointName::new("production").unwrap(),
        format!("https://localhost:{}/api/", server.address.port()),
    )
    .unwrap();
    let client = HttpClient::new(ClientConfig::new(vec![capability])).unwrap();

    let error = client
        .execute(request("production", "success"), &CancellationToken::new())
        .await
        .unwrap_err();
    assert_eq!(
        error,
        ExecuteError::AddressPolicyDenied {
            endpoint: EndpointName::new("production").unwrap(),
        }
    );
}

#[tokio::test]
async fn cross_capability_redirect_revalidates_and_drops_caller_headers() {
    let receiver = RunningServer::start(Router::new().route(
        "/api/capture",
        get(|headers: HeaderMap| async move {
            let leaked = headers.contains_key(AUTHORIZATION)
                || headers.contains_key(COOKIE)
                || headers.contains_key("x-api-key");
            if leaked {
                (StatusCode::INTERNAL_SERVER_ERROR, "leaked").into_response()
            } else {
                (StatusCode::OK, "clean").into_response()
            }
        }),
    ))
    .await;
    let receiver_url = format!("http://{}/api/capture", receiver.address);
    let sender = RunningServer::start(
        Router::new().route(
            "/api/to-receiver",
            get(move || {
                let receiver_url = receiver_url.clone();
                async move { Redirect::temporary(&receiver_url) }
            })
            .merge(post({
                let receiver_url = format!("http://{}/api/capture", receiver.address);
                move || {
                    let receiver_url = receiver_url.clone();
                    async move { Redirect::temporary(&receiver_url) }
                }
            })),
        ),
    )
    .await;
    let mut config = ClientConfig::new(vec![
        endpoint("sender", sender.address),
        endpoint("receiver", receiver.address),
    ]);
    config.redirects = RedirectPolicy::Follow { max_hops: 2 };
    let client = HttpClient::new(config).unwrap();
    let mut outbound = request("sender", "to-receiver");
    outbound.headers = vec![
        Header::new("authorization", "Bearer test-secret"),
        Header::new("cookie", "session=test-secret"),
        Header::new("x-api-key", "test-secret"),
    ];

    let response = client
        .execute(outbound, &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(response.status, 200);
    assert_eq!(response.body, b"clean");
    assert_eq!(response.final_endpoint.as_str(), "receiver");
    assert_eq!(response.redirects_followed, 1);

    let mut body_request = HttpRequest::new(EndpointName::new("sender").unwrap(), HttpMethod::Post);
    body_request.path_segments.push("to-receiver".to_owned());
    body_request.body = b"secret-in-body".to_vec();
    let error = client
        .execute(body_request, &CancellationToken::new())
        .await
        .unwrap_err();
    assert_eq!(error, ExecuteError::RedirectCredentialLeakPrevented);
}

#[test]
fn local_http_test_permit_cannot_authorize_non_loopback() {
    let result = EndpointCapability::local_http_for_tests(
        EndpointName::new("invalid").unwrap(),
        SocketAddr::new(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)), 8080),
        "/api/",
        LocalHttpTestPermit::explicitly_enable_for_tests(),
    );
    assert!(matches!(result, Err(ConfigError::AddressNotAllowed)));
}
