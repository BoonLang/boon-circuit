use crate::report::{ActionClass, PROTOCOL, REQUIRED_FIXTURES};
use serde::Deserialize;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Component, Path};

pub const MAX_MANIFEST_BYTES: u64 = 256 * 1024;

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FixtureManifest {
    pub format_version: u16,
    pub protocol: String,
    pub target_profile: String,
    pub build_profile: String,
    pub allocator_scope: String,
    pub fixtures: Vec<FixtureDefinition>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FixtureDefinition {
    pub id: String,
    pub source: String,
    pub expected_authoritative_rows: u64,
    pub semantics: String,
    pub actions: Vec<ActionDefinition>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActionDefinition {
    pub id: String,
    pub class: ActionClass,
    pub kind: ActionKind,
    pub target: String,
    pub warmup_turns: u64,
    pub measured_turns: u64,
    #[serde(default)]
    pub read_after: Option<String>,
    #[serde(default)]
    pub row_key: Option<u64>,
    #[serde(default)]
    pub row_generation: Option<u64>,
    #[serde(default)]
    pub payload_address: Option<String>,
    #[serde(default)]
    pub payload_text: Vec<String>,
    #[serde(default)]
    pub semantic_rows_per_turn: u64,
    #[serde(default)]
    pub require_no_full_scan: bool,
    #[serde(default)]
    pub require_no_interaction_index_rebuild: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum ActionKind {
    RootRead,
    RootSource,
    RowSource,
    CursorPage,
}

impl FixtureManifest {
    pub fn load(path: &Path) -> Result<(Self, Vec<u8>), String> {
        let metadata =
            fs::metadata(path).map_err(|error| format!("{}: {error}", path.display()))?;
        if metadata.len() == 0 || metadata.len() > MAX_MANIFEST_BYTES {
            return Err(format!(
                "{} is {} bytes; expected 1..={MAX_MANIFEST_BYTES}",
                path.display(),
                metadata.len()
            ));
        }
        let bytes = fs::read(path).map_err(|error| format!("{}: {error}", path.display()))?;
        let manifest = toml::from_slice::<Self>(&bytes)
            .map_err(|error| format!("{}: {error}", path.display()))?;
        manifest.validate()?;
        Ok((manifest, bytes))
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.format_version != 1 {
            return Err(format!(
                "fixture manifest format {} is not 1",
                self.format_version
            ));
        }
        require_exact(&self.protocol, PROTOCOL, "fixture protocol")?;
        require_exact(
            &self.target_profile,
            "software_default",
            "fixture target profile",
        )?;
        require_exact(&self.build_profile, "release", "fixture build profile")?;
        require_bounded(&self.allocator_scope, 256, "allocator scope")?;
        let actual = self
            .fixtures
            .iter()
            .map(|fixture| fixture.id.as_str())
            .collect::<BTreeSet<_>>();
        let expected = REQUIRED_FIXTURES.iter().copied().collect::<BTreeSet<_>>();
        if actual != expected {
            return Err(format!(
                "fixture manifest IDs differ: actual={actual:?}, expected={expected:?}"
            ));
        }
        for fixture in &self.fixtures {
            fixture.validate()?;
        }
        Ok(())
    }
}

impl FixtureDefinition {
    fn validate(&self) -> Result<(), String> {
        require_bounded(&self.id, 96, "fixture id")?;
        safe_relative(&self.source)?;
        require_exact(
            &self.semantics,
            "legacy-current-runtime",
            "fixture semantics",
        )?;
        if self.actions.is_empty() || self.actions.len() > 16 {
            return Err(format!("fixture {} must define 1..=16 actions", self.id));
        }
        let mut actions = BTreeSet::new();
        for action in &self.actions {
            if !actions.insert(action.id.as_str()) {
                return Err(format!("fixture {} repeats action {}", self.id, action.id));
            }
            action.validate(&self.id)?;
        }
        Ok(())
    }
}

impl ActionDefinition {
    fn validate(&self, fixture: &str) -> Result<(), String> {
        require_bounded(&self.id, 96, "action id")?;
        require_bounded(&self.target, 240, "action target")?;
        if self.measured_turns == 0 || self.measured_turns > 10_000 {
            return Err(format!(
                "fixture {fixture} action {} measured_turns must be 1..=10000",
                self.id
            ));
        }
        if self.warmup_turns > 10_000 {
            return Err(format!(
                "fixture {fixture} action {} warmup_turns exceeds 10000",
                self.id
            ));
        }
        if self.payload_text.len() > 8 {
            return Err(format!(
                "fixture {fixture} action {} has more than eight alternating payloads",
                self.id
            ));
        }
        for value in &self.payload_text {
            require_bounded(value, 4096, "action payload text")?;
        }
        if let Some(address) = &self.payload_address {
            require_bounded(address, 256, "action payload address")?;
        }
        if let Some(read_after) = &self.read_after {
            require_bounded(read_after, 240, "action read_after target")?;
        }
        match self.kind {
            ActionKind::RootRead => {
                if self.read_after.is_some()
                    || self.row_key.is_some()
                    || self.row_generation.is_some()
                    || self.payload_address.is_some()
                    || !self.payload_text.is_empty()
                {
                    return Err(format!(
                        "fixture {fixture} root-read action {} has source-only fields",
                        self.id
                    ));
                }
            }
            ActionKind::RootSource => {
                if self.row_key.is_some() || self.row_generation.is_some() {
                    return Err(format!(
                        "fixture {fixture} root-source action {} has a row target",
                        self.id
                    ));
                }
            }
            ActionKind::RowSource => {
                if self.row_key.is_none() || self.row_generation.is_none() {
                    return Err(format!(
                        "fixture {fixture} row-source action {} lacks key/generation",
                        self.id
                    ));
                }
            }
            ActionKind::CursorPage => {
                if self.read_after.is_none()
                    || self.row_key.is_some()
                    || self.row_generation.is_some()
                    || self.payload_address.is_some()
                    || !self.payload_text.is_empty()
                {
                    return Err(format!(
                        "fixture {fixture} cursor-page action {} requires only read_after",
                        self.id
                    ));
                }
            }
        }
        Ok(())
    }
}

fn safe_relative(value: &str) -> Result<(), String> {
    let path = Path::new(value);
    if path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        Err(format!(
            "fixture path `{value}` is not a safe relative path"
        ))
    } else {
        Ok(())
    }
}

fn require_exact(actual: &str, expected: &str, label: &str) -> Result<(), String> {
    if actual == expected {
        Ok(())
    } else {
        Err(format!("{label} is `{actual}`; expected `{expected}`"))
    }
}

fn require_bounded(value: &str, maximum: usize, label: &str) -> Result<(), String> {
    if value.is_empty() || value.len() > maximum {
        Err(format!(
            "{label} length {} is outside 1..={maximum}",
            value.len()
        ))
    } else {
        Ok(())
    }
}
