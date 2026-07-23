use crate::{
    ArtifactDescriptor, BundleFileDescriptor, BundleFileKind, CapabilityProfileDescriptor,
    MAX_ARTIFACT_BYTES, MAX_MANIFEST_BYTES, MAX_PACKAGE_FILE_BYTES, MAX_PACKAGE_FILES,
    PackageError, sha256_bytes,
};
use boon_persistence::{ContentArtifact, ContentArtifactId};
use boon_plan::{ProgramRole, TargetProfile};
use boon_runtime::{ProgramArtifact, ProgramCapabilityProfile};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::io::Cursor;

pub const BROWSER_APP_CONFIG_FORMAT: u32 = 3;
pub const MAX_BROWSER_APP_CONFIG_BYTES: usize = MAX_MANIFEST_BYTES;
pub const MAX_BROWSER_ARTIFACT_PATH_BYTES: usize = 2 * 1024;
pub const MAX_BROWSER_PACKAGE_ASSETS: usize = MAX_PACKAGE_FILES;
pub const MAX_BROWSER_PACKAGE_ASSET_FETCH_PATH_BYTES: usize = MAX_BROWSER_ARTIFACT_PATH_BYTES;
pub const MAX_BROWSER_PACKAGE_ASSET_URL_BYTES: usize =
    "asset://".len() + 256 + MAX_BROWSER_PACKAGE_ASSET_FETCH_PATH_BYTES;
pub const MAX_BROWSER_PACKAGE_ASSET_MEDIA_TYPE_BYTES: usize = 256;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserPackageAssetDescriptor {
    pub url: String,
    pub fetch_path: String,
    pub bytes_sha256: String,
    pub bytes_len: usize,
    pub media_type: String,
}

impl BrowserPackageAssetDescriptor {
    pub fn from_bundle_file(
        package_id: &str,
        file: &BundleFileDescriptor,
    ) -> Result<Self, PackageError> {
        if file.kind != BundleFileKind::Asset || !file.public {
            return Err(PackageError::new(
                "browser package assets must come from public Asset bundle files",
            ));
        }
        let descriptor = Self {
            url: format!("asset://{package_id}/{}", file.path),
            fetch_path: format!("/{}", file.path),
            bytes_sha256: file.bytes_sha256.clone(),
            bytes_len: file.bytes_len,
            media_type: package_asset_media_type(&file.path).to_owned(),
        };
        descriptor.validate_for_package(package_id)?;
        Ok(descriptor)
    }

    pub fn validate_for_package(&self, package_id: &str) -> Result<(), PackageError> {
        validate_identifier("browser package asset owner", package_id, true)?;
        validate_browser_fetch_path("browser package asset fetch path", &self.fetch_path)?;
        if self.url.len() > MAX_BROWSER_PACKAGE_ASSET_URL_BYTES {
            return Err(PackageError::new(format!(
                "browser package asset URL exceeds {MAX_BROWSER_PACKAGE_ASSET_URL_BYTES} bytes"
            )));
        }
        let expected_url = format!("asset://{package_id}{}", self.fetch_path);
        if self.url != expected_url {
            return Err(PackageError::new(
                "browser package asset URL is non-canonical or belongs to another package",
            ));
        }
        validate_sha256("browser package asset bytes_sha256", &self.bytes_sha256)?;
        if self.bytes_len > MAX_PACKAGE_FILE_BYTES {
            return Err(PackageError::new(format!(
                "browser package asset size exceeds {MAX_PACKAGE_FILE_BYTES} bytes"
            )));
        }
        if self.media_type.is_empty()
            || self.media_type.len() > MAX_BROWSER_PACKAGE_ASSET_MEDIA_TYPE_BYTES
            || self.media_type.trim() != self.media_type
        {
            return Err(PackageError::new(
                "browser package asset media type must be non-empty, trimmed, and bounded",
            ));
        }
        let target = self
            .fetch_path
            .strip_prefix('/')
            .expect("validated browser fetch paths start with a slash");
        if self.media_type != package_asset_media_type(target) {
            return Err(PackageError::new(
                "browser package asset media type differs from its deterministic target type",
            ));
        }
        Ok(())
    }

    pub fn verify_bytes(&self, bytes: &[u8]) -> Result<(), PackageError> {
        if bytes.len() != self.bytes_len {
            return Err(PackageError::new(
                "browser package asset size differs from bootstrap metadata",
            ));
        }
        if sha256_bytes(bytes) != self.bytes_sha256 {
            return Err(PackageError::new(
                "browser package asset digest differs from bootstrap metadata",
            ));
        }
        Ok(())
    }
}

/// Closed bootstrap metadata consumed by the generic browser host.
///
/// The browser receives only the public Client artifact. Session and Server
/// artifacts remain behind their respective hosts.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserAppConfig {
    pub format: u32,
    pub package_id: String,
    pub protocol_version: u32,
    pub client_artifact_path: String,
    pub client_artifact_id: String,
    pub client_artifact_sha256: String,
    pub client_artifact_revision: u64,
    pub client_artifact_media_type: String,
    pub client_artifact_bytes_len: usize,
    pub client_capability_profile_id: String,
    pub client_capability_profile: CapabilityProfileDescriptor,
    pub package_assets: Vec<BrowserPackageAssetDescriptor>,
    pub canvas_id: String,
}

impl BrowserAppConfig {
    pub fn for_client(
        package_id: &str,
        protocol_version: u32,
        canvas_id: &str,
        client: &ArtifactDescriptor,
        client_capability_profile: &CapabilityProfileDescriptor,
        mut package_assets: Vec<BrowserPackageAssetDescriptor>,
    ) -> Result<Self, PackageError> {
        if client.role != ProgramRole::Client {
            return Err(PackageError::new(
                "browser bootstrap artifact must have the client role",
            ));
        }
        if client_capability_profile.role != ProgramRole::Client {
            return Err(PackageError::new(
                "browser bootstrap capability profile must have the client role",
            ));
        }
        if client.capability_profile_id != client_capability_profile.id {
            return Err(PackageError::new(
                "browser bootstrap artifact and capability profile differ",
            ));
        }
        package_assets.sort_by(|left, right| left.url.cmp(&right.url));
        let config = Self {
            format: BROWSER_APP_CONFIG_FORMAT,
            package_id: package_id.to_owned(),
            protocol_version,
            client_artifact_path: format!("/{}", client.path),
            client_artifact_id: client.content_artifact_id.clone(),
            client_artifact_sha256: client.bytes_sha256.clone(),
            client_artifact_revision: client.revision,
            client_artifact_media_type: client.content_media_type.clone(),
            client_artifact_bytes_len: client.bytes_len,
            client_capability_profile_id: client.capability_profile_id.clone(),
            client_capability_profile: client_capability_profile.clone(),
            package_assets,
            canvas_id: canvas_id.to_owned(),
        };
        config.validate()?;
        Ok(config)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, PackageError> {
        if bytes.is_empty() || bytes.len() > MAX_BROWSER_APP_CONFIG_BYTES {
            return Err(PackageError::new(format!(
                "browser app config size is outside 1..={MAX_BROWSER_APP_CONFIG_BYTES} bytes"
            )));
        }
        let mut reader = Cursor::new(bytes);
        let config: Self = ciborium::from_reader(&mut reader)
            .map_err(|error| PackageError::context("decode browser app config", error))?;
        if reader.position() != bytes.len() as u64 {
            return Err(PackageError::new(
                "browser app config contains trailing CBOR data",
            ));
        }
        config.validate_fields()?;
        Ok(config)
    }

    pub fn encode(&self) -> Result<Vec<u8>, PackageError> {
        self.validate_fields()?;
        let mut bytes = Vec::new();
        ciborium::into_writer(self, &mut bytes)
            .map_err(|error| PackageError::context("encode browser app config", error))?;
        validate_browser_app_config_size(bytes.len())?;
        Ok(bytes)
    }

    pub fn validate(&self) -> Result<(), PackageError> {
        self.validate_fields()?;
        let mut bytes = Vec::new();
        ciborium::into_writer(self, &mut bytes)
            .map_err(|error| PackageError::context("measure browser app config", error))?;
        validate_browser_app_config_size(bytes.len())
    }

    fn validate_fields(&self) -> Result<(), PackageError> {
        if self.format != BROWSER_APP_CONFIG_FORMAT {
            return Err(PackageError::new(format!(
                "unsupported browser app config format {}; expected {BROWSER_APP_CONFIG_FORMAT}",
                self.format
            )));
        }
        validate_identifier("browser package_id", &self.package_id, true)?;
        validate_identifier("browser canvas_id", &self.canvas_id, false)?;
        if self.protocol_version == 0 {
            return Err(PackageError::new(
                "browser protocol_version must be greater than zero",
            ));
        }
        validate_browser_artifact_path(&self.client_artifact_path)?;
        validate_sha256("browser client_artifact_id", &self.client_artifact_id)?;
        validate_sha256(
            "browser client_artifact_sha256",
            &self.client_artifact_sha256,
        )?;
        if self.client_artifact_revision == 0 {
            return Err(PackageError::new(
                "browser client artifact revision must be non-zero",
            ));
        }
        if self.client_artifact_media_type.is_empty()
            || self.client_artifact_media_type.len() > 256
            || self.client_artifact_media_type.trim() != self.client_artifact_media_type
        {
            return Err(PackageError::new(
                "browser client artifact media type must be non-empty, trimmed, and bounded",
            ));
        }
        if self.client_artifact_bytes_len == 0
            || self.client_artifact_bytes_len > MAX_ARTIFACT_BYTES
        {
            return Err(PackageError::new(format!(
                "browser client artifact size is outside 1..={MAX_ARTIFACT_BYTES} bytes"
            )));
        }
        self.client_capability_profile.validate()?;
        if self.client_capability_profile.role != ProgramRole::Client {
            return Err(PackageError::new(
                "browser app config contains a non-Client capability profile",
            ));
        }
        if self.client_capability_profile_id != self.client_capability_profile.id {
            return Err(PackageError::new(
                "browser client capability profile reference is missing or mismatched",
            ));
        }
        if self.package_assets.len() > MAX_BROWSER_PACKAGE_ASSETS {
            return Err(PackageError::new(format!(
                "browser package asset count exceeds {MAX_BROWSER_PACKAGE_ASSETS}"
            )));
        }
        let mut previous_url = None;
        let mut fetch_paths = BTreeSet::new();
        for asset in &self.package_assets {
            asset.validate_for_package(&self.package_id)?;
            if previous_url.is_some_and(|previous| previous >= asset.url.as_str()) {
                return Err(PackageError::new(
                    "browser package assets must be strictly sorted and unique by URL",
                ));
            }
            if !fetch_paths.insert(asset.fetch_path.as_str()) {
                return Err(PackageError::new(
                    "browser package assets repeat a same-origin fetch path",
                ));
            }
            previous_url = Some(asset.url.as_str());
        }
        Ok(())
    }

    pub fn decode_client_artifact(&self, bytes: Vec<u8>) -> Result<ProgramArtifact, PackageError> {
        self.validate()?;
        if bytes.len() != self.client_artifact_bytes_len {
            return Err(PackageError::new(
                "browser client artifact size differs from bootstrap metadata",
            ));
        }
        if sha256_bytes(&bytes) != self.client_artifact_sha256 {
            return Err(PackageError::new(
                "browser client artifact digest differs from bootstrap metadata",
            ));
        }
        let id =
            ContentArtifactId::from_hex(&self.client_artifact_id).map_err(PackageError::new)?;
        let content = ContentArtifact {
            id,
            media_type: self.client_artifact_media_type.clone(),
            bytes,
        };
        let artifact = ProgramArtifact::from_content_artifact(
            self.client_artifact_revision,
            ProgramCapabilityProfile::PublicClient,
            content,
        )
        .map_err(|error| PackageError::context("decode browser client artifact", error))?;
        for (label, matches) in [
            ("role", artifact.role() == ProgramRole::Client),
            (
                "capability profile",
                artifact.capability_profile() == ProgramCapabilityProfile::PublicClient,
            ),
            (
                "target profile",
                artifact.plan().target_profile == TargetProfile::SoftwareBounded,
            ),
            (
                "package identity",
                artifact.application().package_id == self.package_id,
            ),
            (
                "content identity",
                artifact.id_text() == self.client_artifact_id,
            ),
        ] {
            if !matches {
                return Err(PackageError::new(format!(
                    "browser client artifact {label} differs from bootstrap metadata"
                )));
            }
        }
        Ok(artifact)
    }
}

fn validate_browser_artifact_path(value: &str) -> Result<(), PackageError> {
    validate_browser_path(
        "browser client artifact path",
        value,
        MAX_BROWSER_ARTIFACT_PATH_BYTES,
        false,
    )
}

fn validate_browser_fetch_path(label: &str, value: &str) -> Result<(), PackageError> {
    validate_browser_path(
        label,
        value,
        MAX_BROWSER_PACKAGE_ASSET_FETCH_PATH_BYTES,
        true,
    )
}

fn validate_browser_path(
    label: &str,
    value: &str,
    max_bytes: usize,
    require_uri_path_bytes: bool,
) -> Result<(), PackageError> {
    if value.len() > max_bytes
        || !value.starts_with('/')
        || value.starts_with("//")
        || value.contains(['\\', '\0', '?', '#', '%'])
        || value.chars().any(char::is_control)
        || (require_uri_path_bytes && !value.bytes().all(is_canonical_path_byte))
    {
        return Err(PackageError::new(format!(
            "{label} must be a bounded canonical same-origin path"
        )));
    }
    let relative = &value[1..];
    if relative.is_empty()
        || relative
            .split('/')
            .any(|component| component.is_empty() || matches!(component, "." | ".."))
    {
        return Err(PackageError::new(format!(
            "{label} contains a non-canonical component"
        )));
    }
    Ok(())
}

fn is_canonical_path_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric()
        || matches!(
            byte,
            b'/' | b'-'
                | b'.'
                | b'_'
                | b'~'
                | b'!'
                | b'$'
                | b'&'
                | b'\''
                | b'('
                | b')'
                | b'*'
                | b'+'
                | b','
                | b';'
                | b'='
                | b':'
                | b'@'
        )
}

fn package_asset_media_type(path: &str) -> &'static str {
    match path.rsplit('.').next().unwrap_or_default() {
        "html" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "json" => "application/json; charset=utf-8",
        "cbor" => "application/cbor",
        "wasm" => "application/wasm",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "avif" => "image/avif",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "vcd" => "text/x-vcd; charset=utf-8",
        "mp3" => "audio/mpeg",
        "ogg" => "audio/ogg",
        "wav" => "audio/wav",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        _ => "application/octet-stream",
    }
}

fn validate_browser_app_config_size(bytes_len: usize) -> Result<(), PackageError> {
    if bytes_len == 0 || bytes_len > MAX_BROWSER_APP_CONFIG_BYTES {
        return Err(PackageError::new(format!(
            "browser app config size is outside 1..={MAX_BROWSER_APP_CONFIG_BYTES} bytes"
        )));
    }
    Ok(())
}

fn validate_identifier(label: &str, value: &str, dots: bool) -> Result<(), PackageError> {
    if value.is_empty()
        || value.len() > 256
        || value.trim() != value
        || !value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(byte, b'-' | b'_')
                || (dots && matches!(byte, b'.' | b':' | b'/'))
        })
    {
        return Err(PackageError::new(format!(
            "{label} `{value}` is not a canonical identifier"
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
