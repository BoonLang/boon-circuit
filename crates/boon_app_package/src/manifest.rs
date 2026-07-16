use crate::{APP_MANIFEST_FORMAT, MAX_MANIFEST_BYTES, PackageError};
use boon_plan::{ProgramRole, TargetProfile};
use boon_runtime::ProgramCapabilityProfile;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path};

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunMode {
    Deterministic,
    Live,
}

impl RunMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Deterministic => "deterministic",
            Self::Live => "live",
        }
    }
}

impl std::str::FromStr for RunMode {
    type Err = PackageError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "deterministic" => Ok(Self::Deterministic),
            "live" => Ok(Self::Live),
            _ => Err(PackageError::new(format!(
                "unknown run mode `{value}`; expected deterministic or live"
            ))),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NamespaceProfile {
    Deterministic,
    Staging,
    Production,
}

impl NamespaceProfile {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Deterministic => "deterministic",
            Self::Staging => "staging",
            Self::Production => "production",
        }
    }
}

impl std::str::FromStr for NamespaceProfile {
    type Err = PackageError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "deterministic" => Ok(Self::Deterministic),
            "staging" => Ok(Self::Staging),
            "production" => Ok(Self::Production),
            _ => Err(PackageError::new(format!(
                "unknown namespace profile `{value}`; expected deterministic, staging, or production"
            ))),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppManifest {
    pub format: u32,
    pub package: PackageIdentityManifest,
    pub programs: ProgramPairManifest,
    #[serde(default)]
    pub capability_profiles: BTreeMap<String, CapabilityProfileManifest>,
    pub browser: BrowserManifest,
    pub http: HttpManifest,
    #[serde(default)]
    pub assets: Vec<PackageFileManifest>,
    #[serde(default)]
    pub fixtures: Vec<PackageFileManifest>,
    #[serde(default)]
    pub migrations: Vec<PackageFileManifest>,
    #[serde(default)]
    pub scenarios: Vec<PackageFileManifest>,
    #[serde(default)]
    pub budgets: Vec<PackageFileManifest>,
    #[serde(default)]
    pub environment: Vec<EnvironmentVariableManifest>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackageIdentityManifest {
    pub id: String,
    pub version: String,
    pub deployment_domain: String,
    pub protocol_version: u32,
    pub modes: Vec<RunMode>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProgramPairManifest {
    pub document: ProgramManifest,
    pub server: ProgramManifest,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProgramManifest {
    pub role: ProgramRole,
    pub entry: String,
    pub sources: Vec<String>,
    pub artifact: String,
    pub target_profile: TargetProfile,
    pub capability_profile: ProgramCapabilityProfile,
    pub capability_profile_id: String,
    pub protocol_version: u32,
    pub namespaces: ProgramNamespaces,
}

impl ProgramManifest {
    pub fn namespace(&self, profile: NamespaceProfile) -> &str {
        match profile {
            NamespaceProfile::Deterministic => &self.namespaces.deterministic,
            NamespaceProfile::Staging => &self.namespaces.staging,
            NamespaceProfile::Production => &self.namespaces.production,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProgramNamespaces {
    pub deterministic: String,
    pub staging: String,
    pub production: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapabilityProfileManifest {
    pub id: String,
    pub role: ProgramRole,
    #[serde(default)]
    pub grants: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserManifest {
    pub title: String,
    pub canvas_id: String,
    pub wasm_output_name: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HttpManifest {
    #[serde(default)]
    pub program_path_prefixes: Vec<String>,
    pub health_path: String,
    pub readiness_path: String,
    pub spa_fallback: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackageFileManifest {
    pub source: String,
    pub target: String,
    #[serde(default)]
    pub cache: StaticCachePolicy,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StaticCachePolicy {
    #[default]
    Revalidate,
    Immutable,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnvironmentVariableManifest {
    pub name: String,
    pub kind: EnvironmentValueKind,
    #[serde(default)]
    pub required_modes: Vec<RunMode>,
    #[serde(default)]
    pub allowed_values: Vec<String>,
    #[serde(default)]
    pub default: Option<String>,
    #[serde(default)]
    pub redaction: EnvironmentRedaction,
    pub description: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EnvironmentValueKind {
    Text,
    Bool,
    U16,
    Origin,
    CidrList,
    SecretRef,
    Choice,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EnvironmentRedaction {
    #[default]
    Public,
    Sensitive,
    Reference,
}

impl AppManifest {
    pub fn from_path(path: &Path) -> Result<Self, PackageError> {
        let metadata = fs::symlink_metadata(path)
            .map_err(|error| PackageError::context("read package manifest metadata", error))?;
        if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
            return Err(PackageError::new(
                "package manifest must be a regular non-symlink file",
            ));
        }
        if metadata.len() as usize > MAX_MANIFEST_BYTES {
            return Err(PackageError::new(format!(
                "package manifest exceeds {MAX_MANIFEST_BYTES} bytes"
            )));
        }
        let text = fs::read_to_string(path)
            .map_err(|error| PackageError::context("read package manifest", error))?;
        let manifest: Self = toml::from_str(&text)?;
        manifest.validate()?;
        Ok(manifest)
    }

    pub fn validate(&self) -> Result<(), PackageError> {
        if self.format != APP_MANIFEST_FORMAT {
            return Err(PackageError::new(format!(
                "unsupported app manifest format {}; expected {APP_MANIFEST_FORMAT}",
                self.format
            )));
        }
        validate_identifier("package id", &self.package.id, true)?;
        validate_identifier("package version", &self.package.version, true)?;
        validate_identifier("deployment domain", &self.package.deployment_domain, true)?;
        if self.package.protocol_version == 0 {
            return Err(PackageError::new(
                "protocol_version must be greater than zero",
            ));
        }
        let modes = self.package.modes.iter().copied().collect::<BTreeSet<_>>();
        if modes.is_empty() || modes.len() != self.package.modes.len() {
            return Err(PackageError::new(
                "package modes must be non-empty and contain no duplicates",
            ));
        }
        self.validate_program("document", &self.programs.document, ProgramRole::Document)?;
        self.validate_program("server", &self.programs.server, ProgramRole::Server)?;
        if self.programs.document.artifact == self.programs.server.artifact {
            return Err(PackageError::new(
                "document and server artifact paths must differ",
            ));
        }
        for profile in NamespaceProfile::all() {
            if self.programs.document.namespace(profile) == self.programs.server.namespace(profile)
            {
                return Err(PackageError::new(format!(
                    "document and server state namespaces must differ for {}",
                    profile.as_str()
                )));
            }
        }
        self.validate_capability_profiles()?;
        validate_identifier("browser canvas_id", &self.browser.canvas_id, false)?;
        validate_identifier(
            "browser wasm_output_name",
            &self.browser.wasm_output_name,
            false,
        )?;
        if self.browser.title.trim().is_empty() || self.browser.title.len() > 256 {
            return Err(PackageError::new(
                "browser title must be non-empty and at most 256 bytes",
            ));
        }
        self.validate_http()?;
        self.validate_files()?;
        self.validate_environment()?;
        Ok(())
    }

    pub fn supports_mode(&self, mode: RunMode) -> bool {
        self.package.modes.contains(&mode)
    }

    fn validate_program(
        &self,
        label: &str,
        program: &ProgramManifest,
        expected_role: ProgramRole,
    ) -> Result<(), PackageError> {
        if program.role != expected_role {
            return Err(PackageError::new(format!(
                "{label} program declares role {}, expected {}",
                program.role.as_str(),
                expected_role.as_str()
            )));
        }
        let expected_capability = match expected_role {
            ProgramRole::Document => ProgramCapabilityProfile::PublicDocument,
            ProgramRole::Server => ProgramCapabilityProfile::TrustedServer,
        };
        if program.capability_profile != expected_capability {
            return Err(PackageError::new(format!(
                "{label} program requires capability profile {}, found {}",
                expected_capability.name(),
                program.capability_profile.name()
            )));
        }
        if program.target_profile != TargetProfile::SoftwareBounded {
            return Err(PackageError::new(format!(
                "{label} program target profile must be software_bounded"
            )));
        }
        if program.protocol_version != self.package.protocol_version {
            return Err(PackageError::new(format!(
                "{label} protocol version {} does not match package protocol version {}",
                program.protocol_version, self.package.protocol_version
            )));
        }
        validate_relative_path(&format!("{label} entry"), &program.entry)?;
        validate_relative_path(&format!("{label} artifact"), &program.artifact)?;
        if program.sources.is_empty() {
            return Err(PackageError::new(format!(
                "{label} program must declare source files"
            )));
        }
        let mut sources = BTreeSet::new();
        for source in &program.sources {
            validate_relative_path(&format!("{label} source"), source)?;
            if !sources.insert(source) {
                return Err(PackageError::new(format!(
                    "{label} program repeats source `{source}`"
                )));
            }
        }
        if !sources.contains(&program.entry) {
            return Err(PackageError::new(format!(
                "{label} entry `{}` is absent from its sources",
                program.entry
            )));
        }
        validate_identifier(
            &format!("{label} capability_profile_id"),
            &program.capability_profile_id,
            false,
        )?;
        for (profile, namespace) in [
            ("deterministic", &program.namespaces.deterministic),
            ("staging", &program.namespaces.staging),
            ("production", &program.namespaces.production),
        ] {
            validate_identifier(&format!("{label} {profile} namespace"), namespace, false)?;
        }
        Ok(())
    }

    fn validate_capability_profiles(&self) -> Result<(), PackageError> {
        for (key, profile) in &self.capability_profiles {
            validate_identifier("capability profile key", key, false)?;
            validate_identifier("capability profile id", &profile.id, false)?;
            if key != &profile.id {
                return Err(PackageError::new(format!(
                    "capability profile map key `{key}` differs from id `{}`",
                    profile.id
                )));
            }
            let mut grants = BTreeSet::new();
            for grant in &profile.grants {
                validate_identifier("capability grant", grant, true)?;
                if !grants.insert(grant) {
                    return Err(PackageError::new(format!(
                        "capability profile `{key}` repeats grant `{grant}`"
                    )));
                }
            }
        }
        for (label, program) in [
            ("document", &self.programs.document),
            ("server", &self.programs.server),
        ] {
            let profile = self
                .capability_profiles
                .get(&program.capability_profile_id)
                .ok_or_else(|| {
                    PackageError::new(format!(
                        "{label} references unknown capability profile `{}`",
                        program.capability_profile_id
                    ))
                })?;
            if profile.role != program.role {
                return Err(PackageError::new(format!(
                    "{label} capability profile role does not match its program role"
                )));
            }
        }
        Ok(())
    }

    fn validate_http(&self) -> Result<(), PackageError> {
        validate_http_path("health_path", &self.http.health_path)?;
        validate_http_path("readiness_path", &self.http.readiness_path)?;
        if self.http.health_path == self.http.readiness_path {
            return Err(PackageError::new(
                "health_path and readiness_path must differ",
            ));
        }
        let mut prefixes = BTreeSet::new();
        for prefix in &self.http.program_path_prefixes {
            validate_identifier("program path prefix", prefix, false)?;
            if !prefixes.insert(prefix) {
                return Err(PackageError::new(format!(
                    "program path prefix `{prefix}` is duplicated"
                )));
            }
        }
        Ok(())
    }

    fn validate_files(&self) -> Result<(), PackageError> {
        let mut targets = BTreeSet::new();
        for (kind, files) in [
            ("asset", &self.assets),
            ("fixture", &self.fixtures),
            ("migration", &self.migrations),
            ("scenario", &self.scenarios),
            ("budget", &self.budgets),
        ] {
            for file in files {
                validate_relative_path(&format!("{kind} source"), &file.source)?;
                validate_relative_path(&format!("{kind} target"), &file.target)?;
                if !targets.insert(file.target.as_str()) {
                    return Err(PackageError::new(format!(
                        "package target `{}` is duplicated",
                        file.target
                    )));
                }
            }
        }
        Ok(())
    }

    fn validate_environment(&self) -> Result<(), PackageError> {
        let supported_modes = self.package.modes.iter().copied().collect::<BTreeSet<_>>();
        let mut names = BTreeSet::new();
        for variable in &self.environment {
            validate_environment_name(&variable.name)?;
            if !names.insert(variable.name.as_str()) {
                return Err(PackageError::new(format!(
                    "environment variable `{}` is duplicated",
                    variable.name
                )));
            }
            if variable.description.trim().is_empty() || variable.description.len() > 1024 {
                return Err(PackageError::new(format!(
                    "environment variable `{}` needs a bounded description",
                    variable.name
                )));
            }
            let required = variable
                .required_modes
                .iter()
                .copied()
                .collect::<BTreeSet<_>>();
            if required.len() != variable.required_modes.len()
                || !required.is_subset(&supported_modes)
            {
                return Err(PackageError::new(format!(
                    "environment variable `{}` has duplicate or unsupported required modes",
                    variable.name
                )));
            }
            match variable.kind {
                EnvironmentValueKind::Choice if variable.allowed_values.is_empty() => {
                    return Err(PackageError::new(format!(
                        "choice environment variable `{}` needs allowed_values",
                        variable.name
                    )));
                }
                EnvironmentValueKind::Choice => {}
                _ if !variable.allowed_values.is_empty() => {
                    return Err(PackageError::new(format!(
                        "only choice environment variables may declare allowed_values (`{}`)",
                        variable.name
                    )));
                }
                _ => {}
            }
            if variable.redaction != EnvironmentRedaction::Public && variable.default.is_some() {
                return Err(PackageError::new(format!(
                    "redacted environment variable `{}` may not have a manifest default",
                    variable.name
                )));
            }
            if let Some(default) = &variable.default {
                validate_scalar_shape(variable, default)?;
            }
        }
        Ok(())
    }
}

impl NamespaceProfile {
    const fn all() -> [Self; 3] {
        [Self::Deterministic, Self::Staging, Self::Production]
    }
}

pub fn validate_scalar_shape(
    variable: &EnvironmentVariableManifest,
    value: &str,
) -> Result<(), PackageError> {
    if value.len() > 64 * 1024 {
        return Err(PackageError::new(format!(
            "environment variable `{}` exceeds 65536 bytes",
            variable.name
        )));
    }
    match variable.kind {
        EnvironmentValueKind::Text => {}
        EnvironmentValueKind::Bool => {
            if !matches!(value, "true" | "false") {
                return Err(PackageError::new(format!(
                    "environment variable `{}` must be true or false",
                    variable.name
                )));
            }
        }
        EnvironmentValueKind::U16 => {
            value.parse::<u16>().map_err(|_| {
                PackageError::new(format!(
                    "environment variable `{}` must be an unsigned 16-bit integer",
                    variable.name
                ))
            })?;
        }
        EnvironmentValueKind::Origin => {
            if value.contains(['\n', '\r']) {
                return Err(PackageError::new(format!(
                    "environment variable `{}` contains a line break",
                    variable.name
                )));
            }
        }
        EnvironmentValueKind::CidrList => {}
        EnvironmentValueKind::SecretRef => {
            if value.is_empty()
                || !value
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"._:/-".contains(&byte))
            {
                return Err(PackageError::new(format!(
                    "environment variable `{}` is not a canonical secret reference",
                    variable.name
                )));
            }
        }
        EnvironmentValueKind::Choice => {
            if !variable
                .allowed_values
                .iter()
                .any(|allowed| allowed == value)
            {
                return Err(PackageError::new(format!(
                    "environment variable `{}` is not one of its allowed values",
                    variable.name
                )));
            }
        }
    }
    Ok(())
}

pub(crate) fn validate_relative_path(label: &str, value: &str) -> Result<(), PackageError> {
    if value.is_empty() || value.trim() != value || value.contains(['\0', '\\']) {
        return Err(PackageError::new(format!(
            "{label} `{value}` is not a canonical relative path"
        )));
    }
    let path = Path::new(value);
    if path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, Component::RootDir | Component::Prefix(_)))
    {
        return Err(PackageError::new(format!(
            "{label} `{value}` must be relative"
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

fn validate_environment_name(value: &str) -> Result<(), PackageError> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
        || value.as_bytes()[0].is_ascii_digit()
    {
        return Err(PackageError::new(format!(
            "environment variable name `{value}` is not canonical"
        )));
    }
    Ok(())
}

fn validate_http_path(label: &str, value: &str) -> Result<(), PackageError> {
    if !value.starts_with('/')
        || value.len() > 256
        || value.contains(['?', '#', '\0', '\\'])
        || value.split('/').any(|segment| segment == "..")
    {
        return Err(PackageError::new(format!(
            "{label} `{value}` is not a canonical absolute path"
        )));
    }
    Ok(())
}
