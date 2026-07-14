use crate::protocol::{
    ApplicationIdentity, AssetBlob, CatalogItem, MigrationBundle, MigrationStage, SourceUnit,
    TestStep,
};
use boon_runtime::{ExampleManifestEntry, RuntimeResult};
use std::collections::BTreeSet;
use std::path::Path;
use std::{fs, path::PathBuf};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LoadedExample {
    pub id: String,
    pub label: String,
    pub application: ApplicationIdentity,
    pub units: Vec<SourceUnit>,
    pub test_steps: Vec<TestStep>,
    pub assets: Vec<AssetBlob>,
    pub migration: Option<MigrationBundle>,
}

pub struct Catalog {
    entries: Vec<ExampleManifestEntry>,
}

impl Catalog {
    pub fn load() -> RuntimeResult<Self> {
        let entries = boon_runtime::example_manifest_entries()?;
        if entries.is_empty() {
            return Err("example manifest is empty".into());
        }
        let mut ids = BTreeSet::new();
        for entry in &entries {
            if entry.id.trim().is_empty() {
                return Err("example manifest contains an empty id".into());
            }
            if entry.label.trim().is_empty() {
                return Err(format!("example `{}` has an empty label", entry.id).into());
            }
            if !ids.insert(entry.id.as_str()) {
                return Err(
                    format!("example manifest contains duplicate id `{}`", entry.id).into(),
                );
            }
        }
        Ok(Self { entries })
    }

    pub fn items(&self) -> Vec<CatalogItem> {
        self.entries
            .iter()
            .map(|entry| CatalogItem {
                id: entry.id.clone(),
                label: entry.label.clone(),
                custom: false,
            })
            .collect()
    }

    pub fn initial_id<'a>(&'a self, requested: Option<&str>) -> RuntimeResult<&'a str> {
        if let Some(requested) = requested {
            return self
                .entries
                .iter()
                .find(|entry| entry.id == requested)
                .map(|entry| entry.id.as_str())
                .ok_or_else(|| format!("example manifest has no entry `{requested}`").into());
        }
        self.entries
            .iter()
            .find(|entry| entry.id == "counter")
            .or_else(|| self.entries.first())
            .map(|entry| entry.id.as_str())
            .ok_or_else(|| "example manifest is empty".into())
    }

    pub fn open(&self, id: &str) -> RuntimeResult<LoadedExample> {
        let entry = self
            .entries
            .iter()
            .find(|entry| entry.id == id)
            .ok_or_else(|| format!("example manifest has no entry `{id}`"))?;
        let migration = boon_runtime::migration_sequence_for_entry(entry)?
            .map(|(sequence, scenario)| load_migration_bundle(sequence, scenario))
            .transpose()?;
        let units = match migration.as_ref() {
            Some(migration) => migration
                .initial()
                .ok_or_else(|| {
                    format!(
                        "migration initial stage `{}` is absent after validation",
                        migration.initial_stage
                    )
                })?
                .units
                .clone(),
            None => boon_runtime::source_units_for_entry(entry)?
                .into_iter()
                .map(|unit| SourceUnit {
                    path: unit.path,
                    source: unit.source,
                })
                .collect(),
        };
        let test_steps = if migration.is_some() {
            Vec::new()
        } else {
            ordinary_test_steps(&entry.scenario)?
        };
        let mut assets = entry
            .asset_files
            .iter()
            .map(|path| load_asset(&entry.id, path))
            .collect::<RuntimeResult<Vec<_>>>()?;
        for directory in &entry.asset_directories {
            assets.extend(load_asset_directory(&entry.id, directory)?);
        }
        assets.sort_by(|left, right| left.url.cmp(&right.url));
        Ok(LoadedExample {
            id: entry.id.clone(),
            label: entry.label.clone(),
            application: built_in_application_identity(entry),
            units,
            test_steps,
            assets,
            migration,
        })
    }
}

fn load_migration_bundle(
    sequence: boon_runtime::MigrationSequence,
    scenario: boon_runtime::MigrationScenario,
) -> RuntimeResult<MigrationBundle> {
    let stages = sequence
        .stages
        .iter()
        .map(|stage| {
            let units = boon_compiler::compiler_source_units_for_manifest_source(
                &stage.source,
                &stage.source_files,
            )?
            .into_iter()
            .map(|unit| SourceUnit {
                path: unit.path,
                source: unit.source,
            })
            .collect();
            Ok(MigrationStage {
                id: stage.id.clone(),
                label: stage.label.clone(),
                schema_version: stage.schema_version,
                source: stage.source.clone(),
                source_files: stage.source_files.clone(),
                units,
            })
        })
        .collect::<RuntimeResult<Vec<_>>>()?;
    Ok(MigrationBundle {
        initial_stage: sequence.initial_stage,
        scenario_path: sequence.scenario,
        stages,
        scenario,
    })
}

fn ordinary_test_steps(path: &str) -> RuntimeResult<Vec<TestStep>> {
    Ok(boon_runtime::parse_scenario(Path::new(path))?
        .steps
        .into_iter()
        .filter_map(|step| {
            step.user_action_kind
                .zip(step.source_event)
                .map(|(action_kind, event)| {
                    (
                        Some(action_kind),
                        step.user_action_text,
                        step.user_action_key,
                        event,
                    )
                })
        })
        .map(|(action_kind, action_text, action_key, event)| {
            let pointer_x = payload_field_text(&event.payload, "pointer_x");
            let pointer_y = payload_field_text(&event.payload, "pointer_y");
            let pointer_width = payload_field_text(&event.payload, "pointer_width");
            let pointer_height = payload_field_text(&event.payload, "pointer_height");
            TestStep {
                source_path: event.source,
                action_kind,
                target_text: event.target_text,
                text: action_text,
                key: action_key,
                address: event.payload.address,
                target_occurrence: event.target_occurrence.map(|value| value as u64),
                pointer_x,
                pointer_y,
                pointer_width,
                pointer_height,
            }
        })
        .collect())
}

fn built_in_application_identity(entry: &ExampleManifestEntry) -> ApplicationIdentity {
    let explicit = entry.application_identity();
    ApplicationIdentity::new(
        explicit.as_ref().map_or_else(
            || format!("dev.boon.example.{}", entry.id),
            |value| value.package_id.clone(),
        ),
        format!("builtin:example:{}", entry.id),
        explicit.map_or_else(|| "builtin".to_owned(), |value| value.deployment_domain),
    )
}

fn load_asset_directory(example_id: &str, directory: &str) -> RuntimeResult<Vec<AssetBlob>> {
    let mut paths = Vec::new();
    collect_asset_paths(&resolve_repo_path(directory), &mut paths)?;
    paths.sort();
    paths
        .into_iter()
        .map(|path| load_asset(example_id, &path.to_string_lossy()))
        .collect()
}

fn collect_asset_paths(directory: &Path, paths: &mut Vec<PathBuf>) -> RuntimeResult<()> {
    for entry in fs::read_dir(directory)? {
        let path = entry?.path();
        if path.is_dir() {
            collect_asset_paths(&path, paths)?;
        } else {
            paths.push(path);
        }
    }
    Ok(())
}

fn load_asset(example_id: &str, path: &str) -> RuntimeResult<AssetBlob> {
    let filesystem_path = resolve_repo_path(path);
    let bytes = fs::read(&filesystem_path)?;
    let relative = path
        .split_once("/assets/")
        .map(|(_, relative)| relative)
        .or_else(|| Path::new(path).file_name().and_then(|name| name.to_str()))
        .ok_or_else(|| format!("asset path `{path}` has no file name"))?;
    let media_type = match Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        extension => {
            return Err(format!("unsupported asset extension `{extension}`: {path}").into());
        }
    };
    Ok(AssetBlob {
        url: format!("asset://{example_id}/{relative}"),
        media_type: media_type.to_owned(),
        sha256: boon_runtime::sha256_bytes(&bytes),
        bytes,
    })
}

fn resolve_repo_path(path: &str) -> PathBuf {
    let candidate = PathBuf::from(path);
    if candidate.exists() {
        candidate
    } else {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(path)
    }
}

fn payload_field_text(payload: &boon_runtime::SourcePayload, name: &str) -> Option<String> {
    payload.fields.get(name).and_then(|value| match value {
        boon_runtime::Value::Text(value) => Some(value.clone()),
        boon_runtime::Value::Number(value) => Some(value.to_string()),
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_catalog_and_project_sources_through_runtime_api() {
        let catalog = Catalog::load().expect("catalog");
        let id = catalog.initial_id(Some("counter")).expect("counter id");
        let example = catalog.open(id).expect("counter sources");
        assert_eq!(example.id, "counter");
        assert!(!example.units.is_empty());
        assert!(example.units.iter().all(|unit| !unit.path.is_empty()));
        assert!(example.units.iter().all(|unit| !unit.source.is_empty()));
        assert!(!example.test_steps.is_empty());
        assert_eq!(
            example.application.state_namespace,
            "builtin:example:counter"
        );

        let counter_again = catalog.open(id).expect("counter sources again");
        assert_eq!(counter_again.application, example.application);

        let migration = catalog
            .open("counter_migration")
            .expect("counter migration sources");
        assert_eq!(
            migration.application.package_id,
            "dev.boon.example.counter-migration"
        );
        assert_eq!(
            migration.application.state_namespace,
            "builtin:example:counter_migration"
        );
        let migration_bundle = migration
            .migration
            .as_ref()
            .expect("typed migration metadata");
        assert_eq!(migration_bundle.initial_stage, "v1");
        assert_eq!(migration_bundle.stages.len(), 3);
        assert!(!migration_bundle.scenario.steps.is_empty());
        assert_eq!(
            migration.units,
            migration_bundle.initial().expect("initial stage").units,
            "opening a migration example must use its declared initial stage"
        );
        assert!(migration.test_steps.is_empty());

        let novywave = catalog.open("novywave").expect("NovyWave sources");
        assert!(
            novywave
                .units
                .iter()
                .any(|unit| unit.path.ends_with("View/NovyView.bn"))
        );
        assert!(
            novywave
                .units
                .last()
                .is_some_and(|unit| unit.path.ends_with("novywave/RUN.bn"))
        );

        let items = catalog.items();
        for (id, label) in [
            ("minimal", "Minimal"),
            ("hello_world", "Hello World"),
            ("counter_latest", "Counter without HOLD"),
            ("fibonacci", "Fibonacci"),
            ("interval_latest", "Interval without HOLD"),
            ("interval_hold", "Interval"),
            ("flow_operators", "LATEST, THEN, WHEN, WHILE"),
            ("layers", "Layers"),
            ("pages", "Pages"),
        ] {
            assert!(
                items
                    .iter()
                    .any(|item| item.id == id && item.label == label),
                "missing built-in catalog entry {id:?}"
            );
        }
    }
}
