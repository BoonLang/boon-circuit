#![cfg(feature = "build")]

use boon_app_package::{
    AppManifest, BUNDLE_MANIFEST_FILE, BrowserAppConfig, BuildRequest, BundleManifest,
    LoadedAppBundle, MAX_CAPABILITY_GRANTS_PER_PROFILE, MAX_CAPABILITY_PROFILES, NamespaceProfile,
    RunMode, build_app_package,
};
use boon_plan::ProgramRole;
use boon_runtime::ProgramCapabilityProfile;
use std::fs;
use std::path::{Path, PathBuf};

fn fixture_manifest() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata/triple/app.toml")
}

fn browser_wasm(temp: &tempfile::TempDir) -> PathBuf {
    let path = temp.path().join("browser.wasm");
    let bytes = wat::parse_str("(module (func (export \"ping\")))").unwrap();
    fs::write(&path, bytes).unwrap();
    path
}

fn build_fixture(temp: &tempfile::TempDir, output_name: &str) -> PathBuf {
    let output = temp.path().join(output_name);
    let wasm = browser_wasm(temp);
    build_app_package(BuildRequest {
        manifest_path: &fixture_manifest(),
        output_dir: &output,
        run_mode: RunMode::Deterministic,
        namespace_profile: NamespaceProfile::Deterministic,
        browser_wasm: &wasm,
        source_revision: "fixture-revision-1",
        force: false,
    })
    .unwrap();
    output
}

fn read_bundle_manifest(output: &Path) -> BundleManifest {
    let bytes = fs::read(output.join(BUNDLE_MANIFEST_FILE)).unwrap();
    ciborium::from_reader(bytes.as_slice()).unwrap()
}

fn write_bundle_manifest(output: &Path, manifest: &BundleManifest) {
    let mut bytes = Vec::new();
    ciborium::into_writer(manifest, &mut bytes).unwrap();
    fs::write(output.join(BUNDLE_MANIFEST_FILE), bytes).unwrap();
}

fn assert_bundle_manifest_rejected(
    output: &Path,
    manifest: &BundleManifest,
    expected_message: &str,
) {
    write_bundle_manifest(output, manifest);
    let error = LoadedAppBundle::load(output)
        .err()
        .expect("tampered bundle manifest must fail");
    assert!(
        error.to_string().contains(expected_message),
        "unexpected package error: {error}"
    );
}

fn encode_browser_config_unchecked(config: &BrowserAppConfig) -> Vec<u8> {
    let mut bytes = Vec::new();
    ciborium::into_writer(config, &mut bytes).unwrap();
    bytes
}

#[test]
fn strict_manifest_rejects_unknown_fields_and_protocol_mismatch() {
    let original = fs::read_to_string(fixture_manifest()).unwrap();
    let unknown = original.replacen("format = 1", "format = 1\nunknown = true", 1);
    assert!(toml::from_str::<AppManifest>(&unknown).is_err());

    let mismatch = original.replacen(
        "capability_profile_id = \"public-webgpu-v1\"\nprotocol_version = 1",
        "capability_profile_id = \"public-webgpu-v1\"\nprotocol_version = 2",
        1,
    );
    let parsed = toml::from_str::<AppManifest>(&mismatch).unwrap();
    let error = parsed.validate().unwrap_err();
    assert!(error.to_string().contains("protocol version"));

    let legacy_document = original
        .replacen("[programs.client]", "[programs.document]", 1)
        .replacen(
            "[programs.client.namespaces]",
            "[programs.document.namespaces]",
            1,
        );
    assert!(toml::from_str::<AppManifest>(&legacy_document).is_err());
}

#[test]
fn manifest_requires_distinct_artifact_paths_and_state_namespaces() {
    let original = fs::read_to_string(fixture_manifest()).unwrap();
    let duplicate_artifact = original.replacen(
        "artifact = \"artifacts/session.boon\"",
        "artifact = \"artifacts/client.boon\"",
        1,
    );
    let parsed = toml::from_str::<AppManifest>(&duplicate_artifact).unwrap();
    let error = parsed.validate().unwrap_err();
    assert!(
        error
            .to_string()
            .contains("artifact paths must be distinct")
    );

    let duplicate_namespace = original.replacen(
        "deterministic = \"fixture-session-deterministic-v1\"",
        "deterministic = \"fixture-client-deterministic-v1\"",
        1,
    );
    let parsed = toml::from_str::<AppManifest>(&duplicate_namespace).unwrap();
    let error = parsed.validate().unwrap_err();
    assert!(
        error
            .to_string()
            .contains("state namespaces must be distinct")
    );
}

#[test]
fn manifest_bounds_and_deduplicates_capability_grants() {
    let mut manifest = AppManifest::from_path(&fixture_manifest()).unwrap();
    {
        let client = manifest
            .capability_profiles
            .get_mut("public-webgpu-v1")
            .unwrap();
        client.grants.push(client.grants[0].clone());
    }
    let error = manifest.validate().unwrap_err();
    assert!(error.to_string().contains("repeats grant"));

    manifest
        .capability_profiles
        .get_mut("public-webgpu-v1")
        .unwrap()
        .grants = (0..=MAX_CAPABILITY_GRANTS_PER_PROFILE)
        .map(|index| format!("browser.grant-{index}"))
        .collect();
    let error = manifest.validate().unwrap_err();
    assert!(error.to_string().contains("exceeds 64 grants"));

    let mut profile = manifest
        .capability_profiles
        .get("public-webgpu-v1")
        .unwrap()
        .clone();
    profile.grants.clear();
    manifest.capability_profiles.clear();
    for index in 0..=MAX_CAPABILITY_PROFILES {
        profile.id = format!("bounded-profile-{index}");
        manifest
            .capability_profiles
            .insert(profile.id.clone(), profile.clone());
    }
    let error = manifest.validate().unwrap_err();
    assert!(error.to_string().contains("profile count is outside"));
}

#[test]
fn build_rejects_source_escape_even_when_the_external_file_exists() {
    let temp = tempfile::tempdir().unwrap();
    let scratch_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("target/package-tests");
    fs::create_dir_all(&scratch_root).unwrap();
    let manifest_dir = tempfile::tempdir_in(scratch_root).unwrap();
    let source = fs::read_to_string(fixture_manifest()).unwrap().replacen(
        "Shared/Contract.bn",
        "../../../../../../../../../../etc/hosts",
        1,
    );
    let manifest = manifest_dir.path().join("app.toml");
    fs::write(&manifest, source).unwrap();
    let wasm = browser_wasm(&temp);
    let error = build_app_package(BuildRequest {
        manifest_path: &manifest,
        output_dir: &temp.path().join("out"),
        run_mode: RunMode::Deterministic,
        namespace_profile: NamespaceProfile::Deterministic,
        browser_wasm: &wasm,
        source_revision: "fixture-revision-1",
        force: false,
    })
    .err()
    .expect("workspace escape must fail");
    assert!(error.to_string().contains("escapes the Cargo workspace"));
}

#[test]
fn unrelated_triple_builds_and_loads_with_exact_roles_profiles_and_identity() {
    let temp = tempfile::tempdir().unwrap();
    let output = build_fixture(&temp, "bundle");
    let loaded = LoadedAppBundle::load(&output).unwrap();
    assert_eq!(
        loaded.manifest().package_id,
        "dev.boon.fixture.triple-notes"
    );
    assert_eq!(loaded.manifest().artifacts.len(), 3);
    assert_eq!(loaded.manifest().capability_profiles.len(), 3);
    assert_eq!(
        loaded
            .manifest()
            .capability_profiles
            .iter()
            .map(|profile| profile.id.as_str())
            .collect::<Vec<_>>(),
        vec![
            "public-webgpu-v1",
            "trusted-server-v1",
            "trusted-session-v1"
        ]
    );
    assert!(
        loaded
            .manifest()
            .capability_profile("unused-client-v1")
            .is_none(),
        "unselected declarations must not become ambient bundle grants"
    );
    let client_profile = loaded
        .manifest()
        .capability_profile("public-webgpu-v1")
        .unwrap();
    assert_eq!(
        client_profile.grants,
        vec![
            "browser.same-origin-http",
            "browser.same-origin-websocket",
            "browser.webgpu"
        ]
    );
    assert_eq!(loaded.client_artifact().role(), ProgramRole::Client);
    assert_eq!(loaded.session_artifact().role(), ProgramRole::Session);
    assert_eq!(loaded.server_artifact().role(), ProgramRole::Server);
    assert_eq!(
        loaded.client_artifact().capability_profile(),
        ProgramCapabilityProfile::PublicClient
    );
    assert_eq!(
        loaded.session_artifact().capability_profile(),
        ProgramCapabilityProfile::TrustedSession
    );
    assert_eq!(
        loaded.server_artifact().capability_profile(),
        ProgramCapabilityProfile::TrustedServer
    );
    assert_eq!(
        loaded.client_artifact().application().state_namespace,
        "fixture-client-deterministic-v1"
    );
    assert_eq!(
        loaded.session_artifact().application().state_namespace,
        "fixture-session-deterministic-v1"
    );
    assert_eq!(
        loaded.server_artifact().application().state_namespace,
        "fixture-server-deterministic-v1"
    );
    assert!(output.join("boon_web_host.js").is_file());
    assert!(output.join("boon_web_host_bg.wasm").is_file());
    assert!(output.join("index.html").is_file());

    let browser_config_bytes = fs::read(output.join("boon-app.cbor")).unwrap();
    let browser_config = BrowserAppConfig::decode(&browser_config_bytes).unwrap();
    assert_eq!(browser_config.package_id, loaded.manifest().package_id);
    assert_eq!(
        browser_config.protocol_version,
        loaded.manifest().protocol_version
    );
    assert_eq!(
        browser_config.canvas_id,
        loaded.manifest().browser.canvas_id
    );
    assert_eq!(
        browser_config.client_capability_profile_id,
        "public-webgpu-v1"
    );
    assert_eq!(browser_config.client_capability_profile, *client_profile);
    for private_role in [ProgramRole::Session, ProgramRole::Server] {
        let private_profile = loaded
            .manifest()
            .capability_profiles
            .iter()
            .find(|profile| profile.role == private_role)
            .unwrap();
        assert!(private_profile.grants.iter().all(|grant| {
            !browser_config
                .client_capability_profile
                .grants
                .contains(grant)
        }));
    }
    let client_path = browser_config
        .client_artifact_path
        .strip_prefix('/')
        .unwrap();
    let browser_client = browser_config
        .decode_client_artifact(fs::read(output.join(client_path)).unwrap())
        .unwrap();
    assert_eq!(browser_client, loaded.client_artifact().clone());
}

#[test]
fn bundle_capability_profile_tampering_and_omission_fail_closed() {
    let temp = tempfile::tempdir().unwrap();
    let output = build_fixture(&temp, "bundle");
    let original = read_bundle_manifest(&output);

    let mut omitted = original.clone();
    omitted
        .capability_profiles
        .retain(|profile| profile.role != ProgramRole::Session);
    assert_bundle_manifest_rejected(&output, &omitted, "exactly one selected");

    let mut missing_reference = original.clone();
    missing_reference
        .artifacts
        .iter_mut()
        .find(|artifact| artifact.role == ProgramRole::Client)
        .unwrap()
        .capability_profile_id = "omitted-client-v1".to_owned();
    assert_bundle_manifest_rejected(&output, &missing_reference, "references omitted");

    let mut wrong_role = original.clone();
    wrong_role
        .capability_profiles
        .iter_mut()
        .find(|profile| profile.role == ProgramRole::Client)
        .unwrap()
        .role = ProgramRole::Session;
    assert_bundle_manifest_rejected(&output, &wrong_role, "repeats session");

    let mut noncanonical_profiles = original.clone();
    noncanonical_profiles.capability_profiles.reverse();
    assert_bundle_manifest_rejected(&output, &noncanonical_profiles, "strictly sorted by id");

    let mut duplicate_grant = original;
    let client = duplicate_grant
        .capability_profiles
        .iter_mut()
        .find(|profile| profile.role == ProgramRole::Client)
        .unwrap();
    client.grants.insert(1, client.grants[0].clone());
    assert_bundle_manifest_rejected(&output, &duplicate_grant, "strictly sorted and unique");
}

#[test]
fn artifact_tampering_fails_before_program_start() {
    let temp = tempfile::tempdir().unwrap();
    let output = build_fixture(&temp, "bundle");
    let server_path = output.join("artifacts/server.boon");
    let mut bytes = fs::read(&server_path).unwrap();
    bytes[0] ^= 0x01;
    fs::write(server_path, bytes).unwrap();
    let error = LoadedAppBundle::load(&output)
        .err()
        .expect("tampered artifact must fail");
    assert!(error.to_string().contains("digest differs"));
}

#[test]
fn repeated_builds_are_byte_reproducible() {
    let temp = tempfile::tempdir().unwrap();
    let first = build_fixture(&temp, "first");
    let second = build_fixture(&temp, "second");
    let first_manifest = fs::read(first.join("bundle.cbor")).unwrap();
    let second_manifest = fs::read(second.join("bundle.cbor")).unwrap();
    assert_eq!(first_manifest, second_manifest);
    let manifest: boon_app_package::BundleManifest =
        ciborium::from_reader(first_manifest.as_slice()).unwrap();
    for file in manifest.files {
        assert_eq!(
            fs::read(first.join(&file.path)).unwrap(),
            fs::read(second.join(&file.path)).unwrap()
        );
    }
}

#[test]
fn browser_bootstrap_rejects_tampered_and_trailing_input() {
    let temp = tempfile::tempdir().unwrap();
    let output = build_fixture(&temp, "bundle");
    let config_bytes = fs::read(output.join("boon-app.cbor")).unwrap();
    let config = BrowserAppConfig::decode(&config_bytes).unwrap();
    let client_path = config.client_artifact_path.strip_prefix('/').unwrap();
    let mut artifact = fs::read(output.join(client_path)).unwrap();
    artifact[0] ^= 1;
    assert!(
        config
            .decode_client_artifact(artifact)
            .unwrap_err()
            .to_string()
            .contains("digest differs")
    );

    let mut trailing = config_bytes.clone();
    trailing.push(0);
    assert!(
        BrowserAppConfig::decode(&trailing)
            .unwrap_err()
            .to_string()
            .contains("trailing CBOR data")
    );

    let bundle = read_bundle_manifest(&output);
    for private_role in [ProgramRole::Session, ProgramRole::Server] {
        let mut private_profile = config.clone();
        private_profile.client_capability_profile = bundle
            .capability_profiles
            .iter()
            .find(|profile| profile.role == private_role)
            .unwrap()
            .clone();
        private_profile.client_capability_profile_id =
            private_profile.client_capability_profile.id.clone();
        let error = BrowserAppConfig::decode(&encode_browser_config_unchecked(&private_profile))
            .unwrap_err();
        assert!(error.to_string().contains("non-Client"));
    }

    let mut mismatched = config.clone();
    mismatched.client_capability_profile_id = "different-client-v1".to_owned();
    let error =
        BrowserAppConfig::decode(&encode_browser_config_unchecked(&mismatched)).unwrap_err();
    assert!(error.to_string().contains("missing or mismatched"));

    let mut noncanonical = config.clone();
    noncanonical.client_capability_profile.grants.swap(0, 1);
    let error =
        BrowserAppConfig::decode(&encode_browser_config_unchecked(&noncanonical)).unwrap_err();
    assert!(error.to_string().contains("strictly sorted and unique"));

    let mut value: ciborium::Value = ciborium::from_reader(config_bytes.as_slice()).unwrap();
    let ciborium::Value::Map(fields) = &mut value else {
        panic!("browser config must encode as a CBOR map");
    };
    fields.retain(|(key, _)| {
        !matches!(key, ciborium::Value::Text(name) if name == "client_capability_profile")
    });
    let mut omitted_profile = Vec::new();
    ciborium::into_writer(&value, &mut omitted_profile).unwrap();
    assert!(BrowserAppConfig::decode(&omitted_profile).is_err());
}
