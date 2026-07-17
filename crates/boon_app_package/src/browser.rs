use crate::{
    APP_MANIFEST_FORMAT, ArtifactDescriptor, MAX_ARTIFACT_BYTES, PackageError, sha256_bytes,
};
use boon_persistence::{ContentArtifact, ContentArtifactId};
use boon_plan::{ProgramRole, TargetProfile};
use boon_runtime::{ProgramArtifact, ProgramCapabilityProfile};
use serde::{Deserialize, Serialize};
use std::io::Cursor;

pub const BROWSER_APP_CONFIG_FORMAT: u32 = APP_MANIFEST_FORMAT;
pub const MAX_BROWSER_APP_CONFIG_BYTES: usize = 16 * 1024;
pub const MAX_BROWSER_ARTIFACT_PATH_BYTES: usize = 2 * 1024;

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
    pub canvas_id: String,
}

impl BrowserAppConfig {
    pub fn for_client(
        package_id: &str,
        protocol_version: u32,
        canvas_id: &str,
        client: &ArtifactDescriptor,
    ) -> Result<Self, PackageError> {
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
            canvas_id: canvas_id.to_owned(),
        };
        if client.role != ProgramRole::Client {
            return Err(PackageError::new(
                "browser bootstrap artifact must have the client role",
            ));
        }
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
        config.validate()?;
        Ok(config)
    }

    pub fn encode(&self) -> Result<Vec<u8>, PackageError> {
        self.validate()?;
        let mut bytes = Vec::new();
        ciborium::into_writer(self, &mut bytes)
            .map_err(|error| PackageError::context("encode browser app config", error))?;
        if bytes.len() > MAX_BROWSER_APP_CONFIG_BYTES {
            return Err(PackageError::new(format!(
                "browser app config exceeds {MAX_BROWSER_APP_CONFIG_BYTES} bytes"
            )));
        }
        Ok(bytes)
    }

    pub fn validate(&self) -> Result<(), PackageError> {
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
    if value.len() > MAX_BROWSER_ARTIFACT_PATH_BYTES
        || !value.starts_with('/')
        || value.starts_with("//")
        || value.contains(['\\', '\0', '?', '#', '%'])
        || value.chars().any(char::is_control)
    {
        return Err(PackageError::new(
            "browser client artifact path must be a bounded canonical same-origin path",
        ));
    }
    let relative = &value[1..];
    if relative.is_empty()
        || relative
            .split('/')
            .any(|component| component.is_empty() || matches!(component, "." | ".."))
    {
        return Err(PackageError::new(
            "browser client artifact path contains a non-canonical component",
        ));
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
