use boon_app_package::{
    BROWSER_APP_CONFIG_FORMAT, BrowserAppConfig, MAX_BROWSER_APP_CONFIG_BYTES, sha256_bytes,
};
use boon_plan::ProgramRole;
use boon_runtime::{
    ApplicationIdentity, ProgramCapabilityProfile, ProgramCompileRequest, RuntimeSourceUnit,
    compile_program_artifact,
};
use boon_web_host::{BrowserAppStartup, WebHostError, decode_browser_app_config};

fn client_fixture() -> (BrowserAppConfig, Vec<u8>) {
    let package_id = "dev.boon.browser-startup-test";
    let artifact = compile_program_artifact(&ProgramCompileRequest {
        revision: 7,
        role: ProgramRole::Client,
        entry_path: "RUN.bn".to_owned(),
        units: vec![RuntimeSourceUnit {
            path: "RUN.bn".to_owned(),
            source:
                "document: Document/new(root: Element/label(element: [], label: TEXT { Browser }))"
                    .to_owned(),
        }],
        application: ApplicationIdentity::new(package_id, "browser-test", "local"),
        capability_profile: ProgramCapabilityProfile::PublicClient,
    })
    .unwrap();
    let content = artifact.to_content_artifact();
    let config = BrowserAppConfig {
        format: BROWSER_APP_CONFIG_FORMAT,
        package_id: package_id.to_owned(),
        protocol_version: 1,
        client_artifact_path: "/artifacts/client.boon".to_owned(),
        client_artifact_id: artifact.id_text(),
        client_artifact_sha256: sha256_bytes(&content.bytes),
        client_artifact_revision: artifact.revision(),
        client_artifact_media_type: content.media_type,
        client_artifact_bytes_len: content.bytes.len(),
        canvas_id: "boon-canvas".to_owned(),
    };
    (config, content.bytes)
}

#[test]
fn native_startup_decoder_mounts_the_verified_client_session() {
    let (config, artifact_bytes) = client_fixture();
    let encoded = config.encode().unwrap();
    let decoded = decode_browser_app_config(&encoded).unwrap();
    let startup = BrowserAppStartup::from_artifact_bytes(decoded, artifact_bytes).unwrap();

    assert_eq!(startup.config(), &config);
    assert_eq!(startup.session().artifact().role(), ProgramRole::Client);
    assert!(startup.session().frame().is_some());
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
