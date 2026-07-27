use boon_contract::{CanonicalSourceBundleV1, SourceBundleDigestV1, SourceBundleUnit};
use serde::Deserialize;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Component, Path};

pub const DATASET_FIXTURE_SCHEMA: &str = "boon.dataset-fixture-manifest.v1";
pub const DATASET_IDENTITY: &str = "SourceBundleDigestV1";
pub const DEFAULT_DATASET_MANIFEST: &str = "docs/architecture/phase0/dataset_fixtures.toml";
pub const MAX_DATASET_MANIFEST_BYTES: u64 = 256 * 1024;
pub const REQUIRED_DATASET_CATEGORIES: &[DatasetCategory] = &[
    DatasetCategory::Correctness,
    DatasetCategory::Performance,
    DatasetCategory::Product,
    DatasetCategory::Hardware,
];

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct DatasetFixtureManifestV1 {
    pub format_version: u16,
    pub schema: String,
    pub identity: String,
    pub fixtures: Vec<DatasetFixtureV1>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct DatasetFixtureV1 {
    pub id: String,
    pub category: DatasetCategory,
    pub behavior: DatasetBehavior,
    pub purpose: String,
    pub entrypoint: String,
    pub units: Vec<String>,
    pub source_bundle_digest_v1: SourceBundleDigestV1,
    pub owner_phase: String,
    pub owner_plan: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(rename_all = "kebab-case")]
pub enum DatasetCategory {
    Correctness,
    Performance,
    Product,
    Hardware,
}

impl DatasetCategory {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Correctness => "correctness",
            Self::Performance => "performance",
            Self::Product => "product",
            Self::Hardware => "hardware",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum DatasetBehavior {
    CompileExecuteCurrent,
    CompileRejectFuture,
    MeasuredCurrent,
    ProductRegressionCurrent,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedDatasetFixture {
    pub id: String,
    pub category: DatasetCategory,
    pub source_bundle_digest_v1: SourceBundleDigestV1,
    pub unit_count: usize,
}

impl DatasetFixtureManifestV1 {
    pub fn load(path: &Path) -> Result<Self, String> {
        let metadata =
            fs::metadata(path).map_err(|error| format!("{}: {error}", path.display()))?;
        if metadata.len() == 0 || metadata.len() > MAX_DATASET_MANIFEST_BYTES {
            return Err(format!(
                "{} is {} bytes; expected 1..={MAX_DATASET_MANIFEST_BYTES}",
                path.display(),
                metadata.len()
            ));
        }
        let bytes = fs::read(path).map_err(|error| format!("{}: {error}", path.display()))?;
        let manifest = toml::from_slice::<Self>(&bytes)
            .map_err(|error| format!("{}: {error}", path.display()))?;
        manifest.validate()?;
        Ok(manifest)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.format_version != 1 {
            return Err(format!(
                "dataset fixture manifest format {} is not 1",
                self.format_version
            ));
        }
        require_exact(&self.schema, DATASET_FIXTURE_SCHEMA, "dataset schema")?;
        require_exact(&self.identity, DATASET_IDENTITY, "dataset identity")?;
        if self.fixtures.is_empty() || self.fixtures.len() > 256 {
            return Err("dataset manifest must contain 1..=256 fixtures".to_owned());
        }
        let mut ids = BTreeSet::new();
        let mut categories = BTreeSet::new();
        let mut previous = None;
        for fixture in &self.fixtures {
            fixture.validate()?;
            if !ids.insert(fixture.id.as_str()) {
                return Err(format!("dataset fixture ID `{}` is duplicated", fixture.id));
            }
            if previous.is_some_and(|previous: &str| previous >= fixture.id.as_str()) {
                return Err("dataset fixtures are not canonically ordered by stable ID".to_owned());
            }
            previous = Some(&fixture.id);
            categories.insert(fixture.category);
        }
        let expected = REQUIRED_DATASET_CATEGORIES
            .iter()
            .copied()
            .collect::<BTreeSet<_>>();
        if categories != expected {
            return Err(format!(
                "dataset categories differ: actual={categories:?}, expected={expected:?}"
            ));
        }
        Ok(())
    }

    pub fn verify_workspace(
        &self,
        workspace: &Path,
    ) -> Result<Vec<VerifiedDatasetFixture>, String> {
        self.validate()?;
        let mut verified = Vec::with_capacity(self.fixtures.len());
        let mut errors = Vec::new();
        for fixture in &self.fixtures {
            let owner_plan = workspace.join(&fixture.owner_plan);
            if !owner_plan.is_file() {
                errors.push(format!(
                    "dataset fixture {} owner plan {} is not a file",
                    fixture.id,
                    owner_plan.display()
                ));
                continue;
            }
            match fixture.verify_workspace(workspace) {
                Ok(fixture) => verified.push(fixture),
                Err(error) => errors.push(error),
            }
        }
        if errors.is_empty() {
            Ok(verified)
        } else {
            Err(errors.join("; "))
        }
    }
}

impl DatasetFixtureV1 {
    fn validate(&self) -> Result<(), String> {
        let expected_prefix = format!("{}.", self.category.as_str());
        if !self.id.starts_with(&expected_prefix)
            || !self.id.ends_with(".v1")
            || self.id.len() > 96
            || !self.id.bytes().all(|byte| {
                byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'-')
            })
        {
            return Err(format!(
                "dataset fixture ID `{}` is not a canonical stable {}*.v1 ID",
                self.id, expected_prefix
            ));
        }
        require_bounded(&self.purpose, 256, "dataset fixture purpose")?;
        safe_relative(&self.entrypoint)?;
        safe_relative(&self.owner_plan)?;
        require_bounded(&self.owner_phase, 96, "dataset owner phase")?;
        if self.units.is_empty() || self.units.len() > 64 {
            return Err(format!(
                "dataset fixture {} must have 1..=64 units",
                self.id
            ));
        }
        let mut previous = None;
        let mut paths = BTreeSet::new();
        for unit in &self.units {
            safe_relative(unit)?;
            if previous.is_some_and(|previous: &str| previous >= unit.as_str()) {
                return Err(format!(
                    "dataset fixture {} units are not canonically ordered",
                    self.id
                ));
            }
            if !paths.insert(unit.as_str()) {
                return Err(format!("dataset fixture {} repeats unit {unit}", self.id));
            }
            previous = Some(unit);
        }
        if !paths.contains(self.entrypoint.as_str()) {
            return Err(format!(
                "dataset fixture {} entrypoint is absent from units",
                self.id
            ));
        }
        Ok(())
    }

    fn verify_workspace(&self, workspace: &Path) -> Result<VerifiedDatasetFixture, String> {
        let sources = self
            .units
            .iter()
            .map(|path| {
                let absolute = workspace.join(path);
                fs::read_to_string(&absolute)
                    .map(|source| (path.as_str(), source))
                    .map_err(|error| format!("{}: {error}", absolute.display()))
            })
            .collect::<Result<Vec<_>, String>>()?;
        let canonical = CanonicalSourceBundleV1::new(
            &self.entrypoint,
            sources
                .iter()
                .map(|(path, source)| SourceBundleUnit::new(path, source)),
        )
        .map_err(|error| format!("dataset fixture {}: {error}", self.id))?;
        if canonical.digest() != self.source_bundle_digest_v1 {
            return Err(format!(
                "dataset fixture {} digest is {}; expected {}",
                self.id,
                canonical.digest(),
                self.source_bundle_digest_v1
            ));
        }
        Ok(VerifiedDatasetFixture {
            id: self.id.clone(),
            category: self.category,
            source_bundle_digest_v1: canonical.digest(),
            unit_count: canonical.units().len(),
        })
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
            "dataset path `{value}` is not a safe relative path"
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
    if value.is_empty() || value.len() > maximum || value.trim() != value {
        Err(format!(
            "{label} length {} or surrounding whitespace is invalid; expected 1..={maximum}",
            value.len()
        ))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn workspace() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .unwrap()
            .to_path_buf()
    }

    #[test]
    fn dataset_fixture_manifest_has_current_canonical_identities() {
        let workspace = workspace();
        let manifest =
            DatasetFixtureManifestV1::load(&workspace.join(DEFAULT_DATASET_MANIFEST)).unwrap();
        let verified = manifest.verify_workspace(&workspace).unwrap();
        assert_eq!(verified.len(), manifest.fixtures.len());
        assert_eq!(
            verified
                .iter()
                .map(|fixture| fixture.category)
                .collect::<BTreeSet<_>>(),
            REQUIRED_DATASET_CATEGORIES
                .iter()
                .copied()
                .collect::<BTreeSet<_>>()
        );
    }

    #[test]
    fn dataset_fixture_manifest_rejects_a_forged_source_digest() {
        let workspace = workspace();
        let mut manifest =
            DatasetFixtureManifestV1::load(&workspace.join(DEFAULT_DATASET_MANIFEST)).unwrap();
        manifest.fixtures[0].source_bundle_digest_v1 = "00".repeat(32).parse().unwrap();
        assert!(manifest.verify_workspace(&workspace).is_err());
    }
}
