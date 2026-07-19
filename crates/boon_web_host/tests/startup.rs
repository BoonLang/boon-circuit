use boon_app_package::{
    BROWSER_APP_CONFIG_FORMAT, BrowserAppConfig, CapabilityProfileDescriptor,
    MAX_BROWSER_APP_CONFIG_BYTES, sha256_bytes,
};
use boon_plan::ProgramRole;
use boon_runtime::{
    ApplicationIdentity, ProgramArtifact, ProgramCapabilityProfile, ProgramCompileRequest,
    RuntimeSourceUnit, compile_distributed_program_bundle, compile_program_artifact,
};
use boon_web_host::{BrowserAppStartup, WebHostError, decode_browser_app_config};

const CLIENT_SOURCE: &str = r#"
store: [
    increment: SOURCE
    count: Session/store.count
]

scene: Scene/Element/text(
    element: [events: [press: store.increment]]
    style: [width: Fill]
    text: TEXT { Browser }
)
"#;

const SESSION_SOURCE: &str = r#"
store: [
    increment: Client/store.increment
    count:
        0 |> HOLD count {
            increment |> THEN { count + 1 }
        }
]
"#;

const SERVER_SOURCE: &str = "store: [ready: True]";

fn client_fixture() -> (BrowserAppConfig, Vec<u8>) {
    let package_id = "dev.boon.browser-startup-test";
    let bundle = compile_distributed_program_bundle(&[
        compile_request(package_id, ProgramRole::Client, CLIENT_SOURCE),
        compile_request(package_id, ProgramRole::Session, SESSION_SOURCE),
        compile_request(package_id, ProgramRole::Server, SERVER_SOURCE),
    ])
    .unwrap();
    let artifact = bundle.artifact(ProgramRole::Client).unwrap();
    let content = artifact.to_content_artifact();
    let config = browser_config(package_id, artifact, &content.bytes, content.media_type);
    (config, content.bytes)
}

fn browser_config(
    package_id: &str,
    artifact: &ProgramArtifact,
    artifact_bytes: &[u8],
    artifact_media_type: String,
) -> BrowserAppConfig {
    let client_capability_profile = CapabilityProfileDescriptor {
        id: "public-webgpu-v1".to_owned(),
        role: ProgramRole::Client,
        grants: Vec::new(),
    };
    BrowserAppConfig {
        format: BROWSER_APP_CONFIG_FORMAT,
        package_id: package_id.to_owned(),
        protocol_version: 1,
        client_artifact_path: "/artifacts/client.boon".to_owned(),
        client_artifact_id: artifact.id_text(),
        client_artifact_sha256: sha256_bytes(artifact_bytes),
        client_artifact_revision: artifact.revision(),
        client_artifact_media_type: artifact_media_type,
        client_artifact_bytes_len: artifact_bytes.len(),
        client_capability_profile_id: client_capability_profile.id.clone(),
        client_capability_profile,
        canvas_id: "boon-canvas".to_owned(),
    }
}

fn compile_request(package_id: &str, role: ProgramRole, source: &str) -> ProgramCompileRequest {
    ProgramCompileRequest {
        revision: 7,
        role,
        entry_path: "RUN.bn".to_owned(),
        units: vec![RuntimeSourceUnit {
            path: "RUN.bn".to_owned(),
            source: source.to_owned(),
        }],
        application: ApplicationIdentity::new(
            package_id,
            format!("browser-test-{}", role.as_str()),
            "local",
        ),
        capability_profile: match role {
            ProgramRole::Client => ProgramCapabilityProfile::PublicClient,
            ProgramRole::Session => ProgramCapabilityProfile::TrustedSession,
            ProgramRole::Server => ProgramCapabilityProfile::TrustedServer,
        },
    }
}

#[test]
fn native_startup_decoder_mounts_the_verified_distributed_client() {
    let (config, artifact_bytes) = client_fixture();
    let encoded = config.encode().unwrap();
    let decoded = decode_browser_app_config(&encoded).unwrap();
    let startup = BrowserAppStartup::from_artifact_bytes(decoded, artifact_bytes).unwrap();

    assert_eq!(startup.config(), &config);
    assert!(startup.runtime().document_frame().is_some());
}

#[test]
fn startup_decoder_is_bounded_and_rejects_artifact_tampering() {
    let error = decode_browser_app_config(&vec![0; MAX_BROWSER_APP_CONFIG_BYTES + 1]).unwrap_err();
    assert!(matches!(error, WebHostError::InvalidInput { .. }));

    let (config, mut artifact_bytes) = client_fixture();
    artifact_bytes[0] ^= 1;
    let error = BrowserAppStartup::from_artifact_bytes(config, artifact_bytes)
        .err()
        .expect("tampered startup artifact must fail");
    assert!(matches!(error, WebHostError::InvalidInput { .. }));
    assert!(error.to_string().contains("digest differs"));
}

#[test]
fn startup_rejects_a_verified_but_non_distributed_client_artifact() {
    let package_id = "dev.boon.browser-local-client-rejected";
    let artifact = compile_program_artifact(&compile_request(
        package_id,
        ProgramRole::Client,
        "scene: Scene/Element/text(element: [], style: [], text: TEXT { Local })",
    ))
    .unwrap();
    let content = artifact.to_content_artifact();
    let config = browser_config(package_id, &artifact, &content.bytes, content.media_type);

    let error = BrowserAppStartup::from_artifact_bytes(config, content.bytes)
        .err()
        .expect("browser startup must not retain a local-runtime fallback");
    assert!(matches!(error, WebHostError::InvalidInput { .. }));
    assert!(error.to_string().contains("no distributed graph endpoint"));
}
