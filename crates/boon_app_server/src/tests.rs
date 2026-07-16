use super::*;
use boon_app_package::{
    ArtifactDescriptor, BrowserManifest, EnvironmentVariableManifest, HttpManifest,
    NamespaceProfile,
};
use boon_plan::{ProgramRole, TargetProfile};
use boon_runtime::ProgramCapabilityProfile;
use boon_server_host::{CancellationReason, ServerConfig, WebSocketFrame, bind};
use futures::{SinkExt, StreamExt};

fn digest() -> String {
    "11".repeat(32)
}

fn server_descriptor(namespace: &str) -> ArtifactDescriptor {
    ArtifactDescriptor {
        role: ProgramRole::Server,
        path: "artifacts/server.boon".to_owned(),
        revision: 1,
        content_artifact_id: digest(),
        content_media_type: "application/vnd.boon.machine-plan+cbor;version=1".to_owned(),
        bytes_sha256: digest(),
        bytes_len: 16,
        source_bundle_sha256: digest(),
        source_digest: digest(),
        plan_digest: digest(),
        compiler_id: "boon-compiler/0.1.0".to_owned(),
        target_profile: TargetProfile::SoftwareBounded,
        capability_profile: ProgramCapabilityProfile::TrustedServer,
        capability_profile_id: "server-v1".to_owned(),
        state_namespace: namespace.to_owned(),
        protocol_version: 1,
    }
}

fn manifest(environment: Vec<EnvironmentVariableManifest>) -> BundleManifest {
    BundleManifest {
        format: 1,
        package_id: "dev.boon.fixture.server".to_owned(),
        package_version: "1.0.0".to_owned(),
        deployment_domain: "fixture.local".to_owned(),
        source_revision: "test-revision".to_owned(),
        run_mode: RunMode::Deterministic,
        namespace_profile: NamespaceProfile::Deterministic,
        protocol_version: 1,
        artifacts: vec![server_descriptor("server-test-v1")],
        files: Vec::new(),
        browser: BrowserManifest {
            title: "Fixture".to_owned(),
            canvas_id: "canvas".to_owned(),
            wasm_output_name: "host".to_owned(),
        },
        http: HttpManifest {
            program_path_prefixes: vec!["api".to_owned(), "ws".to_owned()],
            health_path: "/api/health".to_owned(),
            readiness_path: "/api/readiness".to_owned(),
            spa_fallback: true,
        },
        environment,
    }
}

fn variable(
    name: &str,
    kind: EnvironmentValueKind,
    redaction: EnvironmentRedaction,
) -> EnvironmentVariableManifest {
    EnvironmentVariableManifest {
        name: name.to_owned(),
        kind,
        required_modes: vec![RunMode::Deterministic],
        allowed_values: Vec::new(),
        default: None,
        redaction,
        description: format!("Configuration for {name}."),
    }
}

fn base_environment() -> BTreeMap<String, String> {
    BTreeMap::from([
        ("BOON_RUN_MODE".to_owned(), "deterministic".to_owned()),
        (
            "BOON_PUBLIC_ORIGIN".to_owned(),
            "http://127.0.0.1:8080".to_owned(),
        ),
        ("BOON_DATA_DIR".to_owned(), "/tmp/boon-test".to_owned()),
        (
            "BOON_STATE_NAMESPACE".to_owned(),
            "server-test-v1".to_owned(),
        ),
    ])
}

#[test]
fn environment_fails_closed_and_redacts_sensitive_values() {
    let environment = vec![
        variable(
            "BOON_RUN_MODE",
            EnvironmentValueKind::Choice,
            EnvironmentRedaction::Public,
        ),
        variable(
            "BOON_PUBLIC_ORIGIN",
            EnvironmentValueKind::Origin,
            EnvironmentRedaction::Public,
        ),
        variable(
            "BOON_DATA_DIR",
            EnvironmentValueKind::Text,
            EnvironmentRedaction::Public,
        ),
        variable(
            "BOON_STATE_NAMESPACE",
            EnvironmentValueKind::Text,
            EnvironmentRedaction::Public,
        ),
        variable(
            "SIGNING_SECRET_REF",
            EnvironmentValueKind::SecretRef,
            EnvironmentRedaction::Reference,
        ),
    ];
    let mut manifest = manifest(environment);
    manifest.environment[0].allowed_values = vec!["deterministic".to_owned(), "live".to_owned()];

    let mut missing = base_environment();
    missing.remove("BOON_RUN_MODE");
    assert!(
        RuntimeConfig::from_source(&manifest, &missing)
            .err()
            .unwrap()
            .to_string()
            .contains("BOON_RUN_MODE")
    );

    let mut wrong_namespace = base_environment();
    wrong_namespace.insert("BOON_STATE_NAMESPACE".to_owned(), "other".to_owned());
    assert!(
        RuntimeConfig::from_source(&manifest, &wrong_namespace)
            .err()
            .unwrap()
            .to_string()
            .contains("differs")
    );

    let mut invalid_origin = base_environment();
    invalid_origin.insert(
        "BOON_PUBLIC_ORIGIN".to_owned(),
        "http://example.com".to_owned(),
    );
    assert!(
        RuntimeConfig::from_source(&manifest, &invalid_origin)
            .err()
            .unwrap()
            .to_string()
            .contains("loopback")
    );

    let mut valid = base_environment();
    valid.insert(
        "SIGNING_SECRET_REF".to_owned(),
        "vault:fixture/signing".to_owned(),
    );
    let config = RuntimeConfig::from_source(&manifest, &valid).unwrap();
    assert_eq!(
        config.redacted_snapshot()["SIGNING_SECRET_REF"],
        "[secret-reference]"
    );
    assert_eq!(
        config.value("SIGNING_SECRET_REF"),
        Some("vault:fixture/signing")
    );
}

#[test]
fn data_directory_lock_fails_closed_for_a_second_writer() {
    let temp = tempfile::tempdir().unwrap();
    let first = DataDirectoryLock::acquire(temp.path()).unwrap();
    assert!(DataDirectoryLock::acquire(temp.path()).is_err());
    drop(first);
    DataDirectoryLock::acquire(temp.path()).unwrap();
}

struct MockProgram;

#[async_trait]
impl ServerProgram for MockProgram {
    async fn on_http(
        &mut self,
        _request: HttpRequest,
        _cancellation: CallCancellation,
    ) -> HttpResponse {
        HttpResponse::new(200, "delegated")
    }

    async fn on_websocket(
        &mut self,
        event: WebSocketEvent,
        _cancellation: CallCancellation,
    ) -> Vec<WebSocketAction> {
        match event {
            WebSocketEvent::Open(_) => vec![WebSocketAction::Accept],
            WebSocketEvent::Text(text) => {
                vec![WebSocketAction::Reply(WebSocketFrame::Text(text))]
            }
            _ => Vec::new(),
        }
    }

    async fn on_http_cancelled(&mut self, _reason: CancellationReason) {}
}

fn static_assets() -> StaticAssets {
    StaticAssets {
        assets: BTreeMap::from([
            (
                "index.html".to_owned(),
                StaticAsset {
                    body: Arc::from(b"<main>fixture</main>".as_slice()),
                    content_type: "text/html; charset=utf-8",
                    etag: "\"index\"".to_owned(),
                    cache: StaticCachePolicy::Revalidate,
                },
            ),
            (
                "app.123.js".to_owned(),
                StaticAsset {
                    body: Arc::from(b"export const ready=true;".as_slice()),
                    content_type: "text/javascript; charset=utf-8",
                    etag: "\"asset\"".to_owned(),
                    cache: StaticCachePolicy::Immutable,
                },
            ),
        ]),
        spa_fallback: true,
    }
}

#[tokio::test]
async fn static_cache_spa_lifecycle_and_program_routes_share_one_host() {
    let manifest = manifest(Vec::new());
    let lifecycle = LifecycleState::new();
    let program =
        ProductionProgram::new(MockProgram, static_assets(), lifecycle.clone(), &manifest);
    let mut server_config = ServerConfig::default();
    server_config
        .request_header_allowlist
        .insert("if-none-match".to_owned());
    let running = bind("127.0.0.1:0".parse().unwrap(), server_config, program)
        .await
        .unwrap();
    lifecycle.mark_ready();
    let base = format!("http://{}", running.local_addr());
    let client = reqwest::Client::new();

    let spa = client
        .get(format!("{base}/station/one"))
        .send()
        .await
        .unwrap();
    assert_eq!(spa.status(), 200);
    assert_eq!(spa.headers()["cache-control"], "no-cache");
    assert_eq!(spa.text().await.unwrap(), "<main>fixture</main>");

    let asset = client
        .get(format!("{base}/app.123.js"))
        .send()
        .await
        .unwrap();
    assert_eq!(asset.status(), 200);
    assert_eq!(
        asset.headers()["cache-control"],
        "public, max-age=31536000, immutable"
    );
    let not_modified = client
        .get(format!("{base}/app.123.js"))
        .header("if-none-match", "\"asset\"")
        .send()
        .await
        .unwrap();
    assert_eq!(not_modified.status(), 304);

    let delegated = client
        .get(format!("{base}/api/value"))
        .send()
        .await
        .unwrap();
    assert_eq!(delegated.text().await.unwrap(), "delegated");

    let ready = client
        .get(format!("{base}/api/readiness"))
        .send()
        .await
        .unwrap();
    assert_eq!(ready.status(), 200);
    assert!(ready.text().await.unwrap().contains("\"status\":\"ready\""));

    let (mut socket, _) =
        tokio_tungstenite::connect_async(format!("ws://{}/ws", running.local_addr()))
            .await
            .unwrap();
    socket
        .send(tokio_tungstenite::tungstenite::Message::Text("echo".into()))
        .await
        .unwrap();
    let echoed = socket.next().await.unwrap().unwrap();
    assert_eq!(echoed.into_text().unwrap(), "echo");
    socket.close(None).await.unwrap();

    running.shutdown().await.unwrap();
    assert!(lifecycle.is_shutting_down());
}

#[test]
fn content_type_mapping_is_closed_and_deterministic() {
    assert_eq!(content_type("host.wasm"), "application/wasm");
    assert_eq!(content_type("logo.svg"), "image/svg+xml");
    assert_eq!(content_type("unknown.bin"), "application/octet-stream");
}
