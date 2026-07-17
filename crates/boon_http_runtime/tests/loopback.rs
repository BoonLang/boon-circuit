use axum::Router;
use axum::body::Body;
use axum::http::{HeaderValue, Request, Response, StatusCode};
use axum::routing::get;
use boon_http_client::{
    ClientConfig, EndpointCapability, EndpointName, HttpClient, LocalHttpTestPermit, Timeouts,
};
use boon_http_runtime::{OutboundHttpEffectAdapter, apply_completion, apply_submission};
use boon_plan::{ApplicationIdentity, FiniteReal};
use boon_runtime::{
    ProgramCapabilityProfile, ProgramCompileRequest, ProgramSession, RuntimeSourceUnit,
    SourcePayload, Value, compile_program_artifact,
};
use std::collections::BTreeMap;
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::Duration;
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

fn number(value: i64) -> Value {
    Value::Number(FiniteReal::from_i64_exact(value).unwrap())
}

fn request_payload(endpoint: &str, cancellation: &str) -> SourcePayload {
    SourcePayload {
        fields: BTreeMap::from([
            ("endpoint".to_owned(), Value::Text(endpoint.to_owned())),
            ("method".to_owned(), Value::Text("Get".to_owned())),
            (
                "path_segments".to_owned(),
                Value::List(vec![
                    Value::Text("v1".to_owned()),
                    Value::Text("items".to_owned()),
                ]),
            ),
            (
                "query".to_owned(),
                Value::List(vec![Value::Record(BTreeMap::from([
                    ("name".to_owned(), Value::Text("limit".to_owned())),
                    ("value".to_owned(), Value::Text("10".to_owned())),
                ]))]),
            ),
            (
                "headers".to_owned(),
                Value::List(vec![Value::Record(BTreeMap::from([
                    ("name".to_owned(), Value::Text("accept".to_owned())),
                    (
                        "value".to_owned(),
                        Value::Bytes(b"application/json".to_vec()),
                    ),
                ]))]),
            ),
            ("body".to_owned(), Value::Bytes(Vec::new())),
            ("connect_timeout_ms".to_owned(), number(500)),
            ("overall_timeout_ms".to_owned(), number(2_000)),
            (
                "cancellation".to_owned(),
                Value::Text(cancellation.to_owned()),
            ),
        ]),
        ..SourcePayload::default()
    }
}

fn program() -> ProgramSession {
    let artifact = compile_program_artifact(&ProgramCompileRequest {
        revision: 1,
        entry_path: "outbound_http_effect.bn".to_owned(),
        units: vec![RuntimeSourceUnit {
            path: "outbound_http_effect.bn".to_owned(),
            source: include_str!("../../../examples/outbound_http_effect.bn").to_owned(),
        }],
        application: ApplicationIdentity::new("dev.boon.outbound-http-loopback", "test", "local"),
        role: boon_plan::ProgramRole::Server,
        capability_profile: ProgramCapabilityProfile::TrustedServer,
    })
    .unwrap();
    ProgramSession::start(artifact).unwrap()
}

fn adapter(address: SocketAddr) -> OutboundHttpEffectAdapter {
    let endpoint = EndpointCapability::local_http_for_tests(
        EndpointName::new("catalog").unwrap(),
        address,
        "/api/",
        LocalHttpTestPermit::explicitly_enable_for_tests(),
    )
    .unwrap();
    let mut config = ClientConfig::new(vec![endpoint]);
    config.timeouts = Timeouts {
        connect: Duration::from_secs(1),
        overall: Duration::from_secs(3),
    };
    OutboundHttpEffectAdapter::new(HttpClient::new(config).unwrap(), 4).unwrap()
}

#[tokio::test]
async fn compiled_boon_effect_uses_real_loopback_and_typed_correlated_completion() {
    let observed = Arc::new(Mutex::new(Vec::<String>::new()));
    let handler_observed = Arc::clone(&observed);
    let server = RunningServer::start(Router::new().route(
        "/api/v1/items",
        get(move |request: Request<Body>| {
            let handler_observed = Arc::clone(&handler_observed);
            async move {
                let uri = request.uri().to_string();
                let accept = request
                    .headers()
                    .get("accept")
                    .and_then(|value| value.to_str().ok())
                    .unwrap_or("missing")
                    .to_owned();
                handler_observed
                    .lock()
                    .unwrap()
                    .push(format!("{uri}|{accept}"));
                let mut response = Response::new(Body::from(r#"{"items":[1]}"#));
                *response.status_mut() = StatusCode::MULTI_STATUS;
                response
                    .headers_mut()
                    .insert("x-result", HeaderValue::from_static("catalog-response"));
                response
            }
        }),
    ))
    .await;
    let mut program = program();
    let mut adapter = adapter(server.address);

    let dispatched = program
        .dispatch(
            "store.request",
            None,
            request_payload("catalog", "Independent"),
        )
        .unwrap();
    assert!(dispatched.runtime_turn.outbox_changes.is_empty());
    let [invocation] = dispatched.runtime_turn.transient_effects.as_slice() else {
        panic!("Boon turn must emit one transient outbound HTTP call");
    };
    assert_eq!(invocation.effect_id, adapter.effect_id());
    let submission = adapter.submit(invocation.clone()).unwrap();
    assert!(submission.cancelled_calls.is_empty());
    assert!(
        apply_submission(&mut program, submission)
            .unwrap()
            .is_none()
    );

    let completion = adapter.next_completion().await.unwrap();
    let Value::Record(outcome) = &completion.outcome else {
        panic!("HTTP completion must be a typed variant record");
    };
    assert_eq!(outcome["$tag"], Value::Text("HttpSucceeded".to_owned()));
    assert_eq!(outcome["status"], number(207));
    assert!(matches!(&outcome["body"], Value::Bytes(bytes) if bytes == br#"{"items":[1]}"#));
    let completion_turn = apply_completion(&mut program, completion).unwrap();
    assert!(completion_turn.outbox_changes.is_empty());
    assert_eq!(program.pending_transient_effect_count(), 0);
    assert_eq!(
        program.output_value_current("last_status").unwrap(),
        number(207)
    );
    assert_eq!(
        observed.lock().unwrap().as_slice(),
        ["/api/v1/items?limit=10|application/json"]
    );
}

#[tokio::test]
async fn transport_failure_is_typed_bounded_and_does_not_echo_request_secrets() {
    let server = RunningServer::start(Router::new()).await;
    let mut program = program();
    let mut adapter = adapter(server.address);
    let mut payload = request_payload("not-configured", "Independent");
    payload.fields.insert(
        "headers".to_owned(),
        Value::List(vec![Value::Record(BTreeMap::from([
            ("name".to_owned(), Value::Text("authorization".to_owned())),
            (
                "value".to_owned(),
                Value::Bytes(b"Bearer super-secret-token".to_vec()),
            ),
        ]))]),
    );
    payload.fields.insert(
        "body".to_owned(),
        Value::Bytes(b"super-secret-body".to_vec()),
    );

    let dispatched = program.dispatch("store.request", None, payload).unwrap();
    assert!(dispatched.runtime_turn.outbox_changes.is_empty());
    let [invocation] = dispatched.runtime_turn.transient_effects.as_slice() else {
        panic!("Boon turn must emit one transient outbound HTTP call");
    };
    let submission = adapter.submit(invocation.clone()).unwrap();
    apply_submission(&mut program, submission).unwrap();

    let completion = adapter.next_completion().await.unwrap();
    let Value::Record(outcome) = &completion.outcome else {
        panic!("HTTP failure must be a typed variant record");
    };
    assert_eq!(outcome["$tag"], Value::Text("HttpFailed".to_owned()));
    assert_eq!(
        outcome["endpoint"],
        Value::Text("not-configured".to_owned())
    );
    assert_eq!(outcome["code"], Value::Text("unknown_endpoint".to_owned()));
    let Value::Text(diagnostic) = &outcome["diagnostic"] else {
        panic!("HTTP failure diagnostic must be text");
    };
    assert!(diagnostic.len() <= 1024);
    assert!(!diagnostic.contains("super-secret"));

    let completion_turn = apply_completion(&mut program, completion).unwrap();
    assert!(completion_turn.outbox_changes.is_empty());
    assert_eq!(program.pending_transient_effect_count(), 0);
    assert_eq!(
        program.output_value_current("last_status").unwrap(),
        number(0)
    );
}

#[tokio::test]
async fn cancel_previous_removes_runtime_ownership_and_only_latest_completes() {
    let server = RunningServer::start(Router::new().route(
        "/api/v1/items",
        get(|| async {
            tokio::time::sleep(Duration::from_millis(100)).await;
            (StatusCode::OK, "latest")
        }),
    ))
    .await;
    let mut program = program();
    let mut adapter = adapter(server.address);

    let first = program
        .dispatch(
            "store.request",
            None,
            request_payload("catalog", "CancelPrevious"),
        )
        .unwrap()
        .runtime_turn
        .transient_effects
        .remove(0);
    let first_call = first.call_id;
    apply_submission(&mut program, adapter.submit(first).unwrap()).unwrap();

    let second = program
        .dispatch(
            "store.request",
            None,
            request_payload("catalog", "CancelPrevious"),
        )
        .unwrap()
        .runtime_turn
        .transient_effects
        .remove(0);
    let second_call = second.call_id;
    let submission = adapter.submit(second).unwrap();
    assert_eq!(submission.cancelled_calls, [first_call]);
    apply_submission(&mut program, submission).unwrap();
    assert_eq!(program.pending_transient_effect_count(), 1);

    let completion = adapter.next_completion().await.unwrap();
    assert_eq!(completion.call_id, second_call);
    apply_completion(&mut program, completion).unwrap();
    assert_eq!(program.pending_transient_effect_count(), 0);
    assert_eq!(
        program.output_value_current("last_status").unwrap(),
        number(200)
    );
}
