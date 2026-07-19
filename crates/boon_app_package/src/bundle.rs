#[cfg(feature = "build")]
use crate::CapabilityProfileManifest;
use crate::manifest::validate_identifier;
use crate::{
    BUNDLE_FORMAT, BrowserManifest, EnvironmentVariableManifest, HttpManifest, MAX_ARTIFACT_BYTES,
    MAX_CAPABILITY_GRANTS_PER_PROFILE, MAX_MANIFEST_BYTES, MAX_PACKAGE_FILE_BYTES,
    MAX_PACKAGE_FILES, NamespaceProfile, PackageError, RunMode, StaticCachePolicy,
    validate_relative_path,
};
use boon_persistence::{ContentArtifact, ContentArtifactId};
use boon_plan::{ApplicationIdentity, ProgramRole, TargetProfile};
use boon_runtime::{ProgramArtifact, ProgramCapabilityProfile};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Component, Path, PathBuf};

pub const BUNDLE_MANIFEST_FILE: &str = "bundle.cbor";

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BundleManifest {
    pub format: u32,
    pub package_id: String,
    pub package_version: String,
    pub deployment_domain: String,
    pub source_revision: String,
    pub run_mode: RunMode,
    pub namespace_profile: NamespaceProfile,
    pub protocol_version: u32,
    pub capability_profiles: Vec<CapabilityProfileDescriptor>,
    pub artifacts: Vec<ArtifactDescriptor>,
    pub files: Vec<BundleFileDescriptor>,
    pub browser: BrowserManifest,
    pub http: HttpManifest,
    pub environment: Vec<EnvironmentVariableManifest>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactDescriptor {
    pub role: ProgramRole,
    pub path: String,
    pub revision: u64,
    pub content_artifact_id: String,
    pub content_media_type: String,
    pub bytes_sha256: String,
    pub bytes_len: usize,
    pub source_bundle_sha256: String,
    pub source_digest: String,
    pub plan_digest: String,
    pub compiler_id: String,
    pub target_profile: TargetProfile,
    pub capability_profile: ProgramCapabilityProfile,
    pub capability_profile_id: String,
    pub state_namespace: String,
    pub protocol_version: u32,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapabilityProfileDescriptor {
    pub id: String,
    pub role: ProgramRole,
    pub grants: Vec<String>,
}

impl CapabilityProfileDescriptor {
    #[cfg(feature = "build")]
    pub(crate) fn canonical_from_manifest(profile: &CapabilityProfileManifest) -> Self {
        let mut grants = profile.grants.clone();
        grants.sort();
        Self {
            id: profile.id.clone(),
            role: profile.role,
            grants,
        }
    }

    pub fn validate(&self) -> Result<(), PackageError> {
        validate_identifier("capability profile id", &self.id, false)?;
        if self.grants.len() > MAX_CAPABILITY_GRANTS_PER_PROFILE {
            return Err(PackageError::new(format!(
                "capability profile `{}` exceeds {MAX_CAPABILITY_GRANTS_PER_PROFILE} grants",
                self.id
            )));
        }
        let mut previous = None;
        for grant in &self.grants {
            validate_identifier("capability grant", grant, true)?;
            if previous.is_some_and(|previous| previous >= grant.as_str()) {
                return Err(PackageError::new(format!(
                    "capability profile `{}` grants must be strictly sorted and unique",
                    self.id
                )));
            }
            previous = Some(grant.as_str());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BundleFileDescriptor {
    pub path: String,
    pub kind: BundleFileKind,
    pub bytes_sha256: String,
    pub bytes_len: usize,
    pub public: bool,
    pub cache: StaticCachePolicy,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BundleFileKind {
    ProgramArtifact,
    BrowserHost,
    Asset,
    Fixture,
    Migration,
    Scenario,
    Budget,
    PackageManifest,
}

impl BundleManifest {
    pub fn validate(&self) -> Result<(), PackageError> {
        if self.format != BUNDLE_FORMAT {
            return Err(PackageError::new(format!(
                "unsupported bundle format {}; expected {BUNDLE_FORMAT}",
                self.format
            )));
        }
        for (label, value) in [
            ("package_id", self.package_id.as_str()),
            ("package_version", self.package_version.as_str()),
            ("deployment_domain", self.deployment_domain.as_str()),
            ("source_revision", self.source_revision.as_str()),
        ] {
            if value.is_empty() || value.trim() != value || value.len() > 512 {
                return Err(PackageError::new(format!(
                    "bundle {label} must be non-empty, trimmed, and bounded"
                )));
            }
        }
        if self.protocol_version == 0 {
            return Err(PackageError::new(
                "bundle protocol_version must be greater than zero",
            ));
        }
        if self.capability_profiles.len() != 3 {
            return Err(PackageError::new(
                "bundle must contain exactly one selected capability profile per artifact",
            ));
        }
        let mut profile_ids = BTreeSet::new();
        let mut profile_roles = BTreeSet::new();
        let mut previous_profile_id = None;
        for profile in &self.capability_profiles {
            profile.validate()?;
            if previous_profile_id.is_some_and(|previous| previous >= profile.id.as_str()) {
                return Err(PackageError::new(
                    "bundle capability profiles must be strictly sorted by id",
                ));
            }
            previous_profile_id = Some(profile.id.as_str());
            if !profile_ids.insert(profile.id.as_str()) {
                return Err(PackageError::new(format!(
                    "bundle repeats capability profile `{}`",
                    profile.id
                )));
            }
            if !profile_roles.insert(profile.role.as_str()) {
                return Err(PackageError::new(format!(
                    "bundle repeats {} capability profile",
                    profile.role.as_str()
                )));
            }
        }
        if profile_roles != BTreeSet::from(["client", "session", "server"]) {
            return Err(PackageError::new(
                "bundle capability profiles are not the required client/session/server triple",
            ));
        }
        if self.artifacts.len() != 3 {
            return Err(PackageError::new(
                "bundle must contain exactly one client, one session, and one server artifact",
            ));
        }
        let mut roles = BTreeSet::new();
        let mut artifact_paths = BTreeSet::new();
        let mut referenced_profile_ids = BTreeSet::new();
        for artifact in &self.artifacts {
            if !roles.insert(artifact.role.as_str()) {
                return Err(PackageError::new(format!(
                    "bundle repeats {} artifact",
                    artifact.role.as_str()
                )));
            }
            validate_bundle_path("artifact path", &artifact.path)?;
            if !artifact_paths.insert(artifact.path.as_str()) {
                return Err(PackageError::new(format!(
                    "bundle repeats artifact path `{}`",
                    artifact.path
                )));
            }
            if artifact.revision == 0 {
                return Err(PackageError::new("artifact revision must be non-zero"));
            }
            validate_sha256("content_artifact_id", &artifact.content_artifact_id)?;
            validate_sha256("artifact bytes_sha256", &artifact.bytes_sha256)?;
            validate_sha256("source_bundle_sha256", &artifact.source_bundle_sha256)?;
            validate_sha256("source_digest", &artifact.source_digest)?;
            validate_sha256("plan_digest", &artifact.plan_digest)?;
            validate_identifier(
                "artifact capability_profile_id",
                &artifact.capability_profile_id,
                false,
            )?;
            if artifact.bytes_len == 0 || artifact.bytes_len > MAX_ARTIFACT_BYTES {
                return Err(PackageError::new(format!(
                    "{} artifact size is outside 1..={MAX_ARTIFACT_BYTES}",
                    artifact.role.as_str()
                )));
            }
            if artifact.content_media_type.is_empty()
                || artifact.content_media_type.len() > 256
                || artifact.compiler_id.is_empty()
                || artifact.compiler_id.len() > 256
                || artifact.state_namespace.is_empty()
                || artifact.state_namespace.len() > 256
            {
                return Err(PackageError::new(format!(
                    "{} artifact metadata is empty or unbounded",
                    artifact.role.as_str()
                )));
            }
            if artifact.target_profile != TargetProfile::SoftwareBounded {
                return Err(PackageError::new(format!(
                    "{} artifact target profile is not software_bounded",
                    artifact.role.as_str()
                )));
            }
            let expected_capability = match artifact.role {
                ProgramRole::Client => ProgramCapabilityProfile::PublicClient,
                ProgramRole::Session => ProgramCapabilityProfile::TrustedSession,
                ProgramRole::Server => ProgramCapabilityProfile::TrustedServer,
            };
            if artifact.capability_profile != expected_capability {
                return Err(PackageError::new(format!(
                    "{} artifact has incompatible capability profile {}",
                    artifact.role.as_str(),
                    artifact.capability_profile.name()
                )));
            }
            let selected_profile = self
                .capability_profile(&artifact.capability_profile_id)
                .ok_or_else(|| {
                    PackageError::new(format!(
                        "{} artifact references omitted capability profile `{}`",
                        artifact.role.as_str(),
                        artifact.capability_profile_id
                    ))
                })?;
            if selected_profile.role != artifact.role {
                return Err(PackageError::new(format!(
                    "{} artifact capability profile `{}` has role {}",
                    artifact.role.as_str(),
                    selected_profile.id,
                    selected_profile.role.as_str()
                )));
            }
            if !referenced_profile_ids.insert(selected_profile.id.as_str()) {
                return Err(PackageError::new(format!(
                    "bundle capability profile `{}` is referenced by more than one artifact",
                    selected_profile.id
                )));
            }
            if artifact.protocol_version != self.protocol_version {
                return Err(PackageError::new(format!(
                    "{} artifact protocol version differs from bundle",
                    artifact.role.as_str()
                )));
            }
        }
        if roles != BTreeSet::from(["client", "session", "server"]) {
            return Err(PackageError::new(
                "bundle artifact roles are not the required client/session/server triple",
            ));
        }
        if referenced_profile_ids != profile_ids {
            return Err(PackageError::new(
                "bundle contains a capability profile not selected by exactly one artifact",
            ));
        }
        if self.files.is_empty() || self.files.len() > MAX_PACKAGE_FILES {
            return Err(PackageError::new(format!(
                "bundle file count is outside 1..={MAX_PACKAGE_FILES}"
            )));
        }
        let mut file_paths = BTreeSet::new();
        for file in &self.files {
            validate_bundle_path("bundle file path", &file.path)?;
            if !file_paths.insert(file.path.as_str()) {
                return Err(PackageError::new(format!(
                    "bundle repeats file `{}`",
                    file.path
                )));
            }
            validate_sha256("bundle file bytes_sha256", &file.bytes_sha256)?;
            if file.bytes_len > MAX_PACKAGE_FILE_BYTES {
                return Err(PackageError::new(format!(
                    "bundle file `{}` exceeds {MAX_PACKAGE_FILE_BYTES} bytes",
                    file.path
                )));
            }
            if file.public
                && matches!(
                    file.kind,
                    BundleFileKind::Migration
                        | BundleFileKind::Fixture
                        | BundleFileKind::Scenario
                        | BundleFileKind::Budget
                        | BundleFileKind::PackageManifest
                )
            {
                return Err(PackageError::new(format!(
                    "non-browser package file `{}` may not be public",
                    file.path
                )));
            }
        }
        if !artifact_paths.is_subset(&file_paths) {
            return Err(PackageError::new(
                "artifact descriptors are absent from the closed file inventory",
            ));
        }
        let namespaces = self
            .artifacts
            .iter()
            .map(|artifact| artifact.state_namespace.as_str())
            .collect::<BTreeSet<_>>();
        if namespaces.len() != 3 {
            return Err(PackageError::new(
                "client, session, and server bundle namespaces must be distinct",
            ));
        }
        Ok(())
    }

    pub fn artifact(&self, role: ProgramRole) -> Option<&ArtifactDescriptor> {
        self.artifacts.iter().find(|artifact| artifact.role == role)
    }

    pub fn capability_profile(&self, id: &str) -> Option<&CapabilityProfileDescriptor> {
        self.capability_profiles
            .iter()
            .find(|profile| profile.id == id)
    }

    pub fn public_files(&self) -> impl Iterator<Item = &BundleFileDescriptor> {
        self.files.iter().filter(|file| file.public)
    }
}

pub struct LoadedAppBundle {
    root: PathBuf,
    manifest: BundleManifest,
    client: ProgramArtifact,
    session: ProgramArtifact,
    server: ProgramArtifact,
}

impl LoadedAppBundle {
    pub fn load(root: &Path) -> Result<Self, PackageError> {
        let root = fs::canonicalize(root)
            .map_err(|error| PackageError::context("canonicalize bundle root", error))?;
        if !fs::metadata(&root)?.is_dir() {
            return Err(PackageError::new("bundle root is not a directory"));
        }
        let manifest_path = root.join(BUNDLE_MANIFEST_FILE);
        let manifest_metadata = fs::symlink_metadata(&manifest_path)
            .map_err(|error| PackageError::context("read bundle manifest metadata", error))?;
        if !manifest_metadata.file_type().is_file()
            || manifest_metadata.file_type().is_symlink()
            || manifest_metadata.len() as usize > MAX_MANIFEST_BYTES
        {
            return Err(PackageError::new(
                "bundle manifest must be a bounded regular non-symlink file",
            ));
        }
        let bytes = fs::read(&manifest_path)
            .map_err(|error| PackageError::context("read bundle manifest", error))?;
        let manifest: BundleManifest = ciborium::from_reader(bytes.as_slice())
            .map_err(|error| PackageError::context("decode bundle manifest", error))?;
        manifest.validate()?;
        for descriptor in &manifest.files {
            verify_file(&root, descriptor)?;
        }
        let client = load_artifact(&root, &manifest, ProgramRole::Client)?;
        let session = load_artifact(&root, &manifest, ProgramRole::Session)?;
        let server = load_artifact(&root, &manifest, ProgramRole::Server)?;
        Ok(Self {
            root,
            manifest,
            client,
            session,
            server,
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn manifest(&self) -> &BundleManifest {
        &self.manifest
    }

    pub fn client_artifact(&self) -> &ProgramArtifact {
        &self.client
    }

    pub fn session_artifact(&self) -> &ProgramArtifact {
        &self.session
    }

    pub fn server_artifact(&self) -> &ProgramArtifact {
        &self.server
    }

    pub fn read_file(&self, descriptor: &BundleFileDescriptor) -> Result<Vec<u8>, PackageError> {
        read_regular_bounded_file(&self.root, &descriptor.path, descriptor.bytes_len)
    }
}

fn load_artifact(
    root: &Path,
    manifest: &BundleManifest,
    role: ProgramRole,
) -> Result<ProgramArtifact, PackageError> {
    let descriptor = manifest
        .artifact(role)
        .ok_or_else(|| PackageError::new(format!("bundle has no {} artifact", role.as_str())))?;
    let bytes = read_regular_bounded_file(root, &descriptor.path, descriptor.bytes_len)?;
    if bytes.len() != descriptor.bytes_len || sha256_bytes(&bytes) != descriptor.bytes_sha256 {
        return Err(PackageError::new(format!(
            "{} artifact bytes do not match bundle metadata",
            role.as_str()
        )));
    }
    let id =
        ContentArtifactId::from_hex(&descriptor.content_artifact_id).map_err(PackageError::new)?;
    let content = ContentArtifact {
        id,
        media_type: descriptor.content_media_type.clone(),
        bytes,
    };
    let artifact = ProgramArtifact::from_content_artifact(
        descriptor.revision,
        descriptor.capability_profile,
        content,
    )
    .map_err(|error| PackageError::context("decode trusted program artifact", error))?;
    let expected_identity = ApplicationIdentity::new(
        &manifest.package_id,
        &descriptor.state_namespace,
        &manifest.deployment_domain,
    );
    for (label, matches) in [
        ("role", artifact.role() == descriptor.role),
        (
            "capability profile",
            artifact.capability_profile() == descriptor.capability_profile,
        ),
        (
            "target profile",
            artifact.plan().target_profile == descriptor.target_profile,
        ),
        (
            "application identity",
            artifact.application() == &expected_identity,
        ),
        (
            "content identity",
            artifact.id_text() == descriptor.content_artifact_id,
        ),
        (
            "source digest",
            artifact.source_digest() == descriptor.source_digest,
        ),
        (
            "plan digest",
            artifact.plan_digest() == descriptor.plan_digest,
        ),
        (
            "compiler identity",
            artifact.compiler_id() == descriptor.compiler_id,
        ),
    ] {
        if !matches {
            return Err(PackageError::new(format!(
                "{} artifact {label} differs from trusted bundle metadata",
                role.as_str()
            )));
        }
    }
    Ok(artifact)
}

fn verify_file(root: &Path, descriptor: &BundleFileDescriptor) -> Result<(), PackageError> {
    let bytes = read_regular_bounded_file(root, &descriptor.path, descriptor.bytes_len)?;
    if bytes.len() != descriptor.bytes_len {
        return Err(PackageError::new(format!(
            "bundle file `{}` size differs from manifest",
            descriptor.path
        )));
    }
    if sha256_bytes(&bytes) != descriptor.bytes_sha256 {
        return Err(PackageError::new(format!(
            "bundle file `{}` digest differs from manifest",
            descriptor.path
        )));
    }
    Ok(())
}

fn read_regular_bounded_file(
    root: &Path,
    relative: &str,
    expected_len: usize,
) -> Result<Vec<u8>, PackageError> {
    validate_bundle_path("bundle file", relative)?;
    if expected_len > MAX_PACKAGE_FILE_BYTES {
        return Err(PackageError::new(
            "bundle file exceeds the package byte limit",
        ));
    }
    let path = root.join(relative);
    let metadata = fs::symlink_metadata(&path)
        .map_err(|error| PackageError::context(&format!("read `{relative}` metadata"), error))?;
    if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
        return Err(PackageError::new(format!(
            "bundle file `{relative}` is not a regular non-symlink file"
        )));
    }
    if metadata.len() as usize > MAX_PACKAGE_FILE_BYTES {
        return Err(PackageError::new(format!(
            "bundle file `{relative}` exceeds the package byte limit"
        )));
    }
    let canonical = fs::canonicalize(&path)
        .map_err(|error| PackageError::context(&format!("canonicalize `{relative}`"), error))?;
    if !canonical.starts_with(root) {
        return Err(PackageError::new(format!(
            "bundle file `{relative}` escapes the bundle root"
        )));
    }
    fs::read(canonical).map_err(Into::into)
}

pub fn sha256_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

fn validate_bundle_path(label: &str, value: &str) -> Result<(), PackageError> {
    validate_relative_path(label, value)?;
    if Path::new(value)
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(PackageError::new(format!(
            "{label} `{value}` contains a non-canonical component"
        )));
    }
    Ok(())
}

fn validate_sha256(label: &str, value: &str) -> Result<(), PackageError> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    {
        return Err(PackageError::new(format!(
            "{label} is not a lowercase SHA-256 digest"
        )));
    }
    Ok(())
}
