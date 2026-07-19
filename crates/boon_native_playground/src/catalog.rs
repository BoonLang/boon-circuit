use crate::protocol::{
    ApplicationIdentity, AssetBlob, CatalogItem, MigrationBundle, MigrationStage,
    MigrationTestDriver, ProgramSource, SourceUnit, TestStep,
};
use boon_plan::ProgramRole;
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
    pub programs: Vec<ProgramSource>,
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
                .launch()
                .ok_or_else(|| {
                    format!(
                        "migration launch stage `{}` is absent after validation",
                        migration.launch_stage
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
        let base_application = built_in_application_identity(entry);
        let programs = load_program_sources(entry, &base_application, &units)?;
        let application = programs
            .iter()
            .find(|source| source.role == ProgramRole::Client)
            .map_or(base_application, |source| source.application.clone());
        let test_steps = match migration.as_ref().map(|migration| migration.test_driver) {
            Some(MigrationTestDriver::Migration) => Vec::new(),
            Some(MigrationTestDriver::Example) | None => ordinary_test_steps(&entry.scenario)?,
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
            application,
            units,
            test_steps,
            assets,
            migration,
            programs,
        })
    }
}

fn load_program_sources(
    entry: &ExampleManifestEntry,
    base_application: &ApplicationIdentity,
    primary_units: &[SourceUnit],
) -> RuntimeResult<Vec<ProgramSource>> {
    if entry.programs.is_empty() {
        return Ok(Vec::new());
    }
    let distributed = entry.programs.len() == 3;
    let sources = entry
        .programs
        .iter()
        .map(|program| {
            let units = boon_compiler::compiler_source_units_for_manifest_source(
                &program.source,
                &program.source_files,
            )?
            .into_iter()
            .map(|unit| {
                Ok(SourceUnit {
                    path: workspace_relative_source_path(&unit.path)?,
                    source: unit.source,
                })
            })
            .collect::<RuntimeResult<Vec<_>>>()?;
            let state_namespace = program.state_namespace.clone().unwrap_or_else(|| {
                if distributed {
                    format!(
                        "{}:{}",
                        base_application.state_namespace,
                        program.role.as_str()
                    )
                } else {
                    base_application.state_namespace.clone()
                }
            });
            Ok(ProgramSource {
                role: program.role,
                entry_path: program.source.clone(),
                units,
                application: ApplicationIdentity::new(
                    base_application.package_id.clone(),
                    state_namespace,
                    base_application.deployment_domain.clone(),
                ),
            })
        })
        .collect::<RuntimeResult<Vec<_>>>()?;
    let primary_units = primary_units
        .iter()
        .map(|unit| {
            Ok(SourceUnit {
                path: workspace_relative_source_path(&unit.path)?,
                source: unit.source.clone(),
            })
        })
        .collect::<RuntimeResult<Vec<_>>>()?;
    if let Some(client) = sources
        .iter()
        .find(|source| source.role == ProgramRole::Client)
        && client.units != primary_units
    {
        return Err(format!(
            "example `{}` primary source differs from its declared client program",
            entry.id
        )
        .into());
    }
    Ok(sources)
}

fn workspace_relative_source_path(path: &str) -> RuntimeResult<String> {
    let path = Path::new(path);
    let relative = if path.is_absolute() {
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .ok_or("native playground manifest directory has no workspace parent")?;
        path.strip_prefix(workspace).map_err(|_| {
            format!(
                "program source path `{}` is outside workspace `{}`",
                path.display(),
                workspace.display()
            )
        })?
    } else {
        path
    };
    relative
        .to_str()
        .map(str::to_owned)
        .ok_or_else(|| format!("program source path `{}` is not UTF-8", path.display()).into())
}

fn load_migration_bundle(
    sequence: boon_runtime::MigrationSequence,
    scenario: boon_runtime::MigrationScenario,
) -> RuntimeResult<MigrationBundle> {
    let launch_stage = sequence.launch_stage().to_owned();
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
        launch_stage,
        test_driver: sequence.test_driver,
        scenario_path: sequence.scenario,
        stages,
        scenario,
    })
}

pub(crate) fn ordinary_test_steps(path: &str) -> RuntimeResult<Vec<TestStep>> {
    boon_runtime::parse_scenario(Path::new(path))?
        .steps
        .into_iter()
        .filter_map(|step| match (step.user_action_kind, step.source_event) {
            (Some(action_kind), Some(event)) => Some(Ok({
                let pointer_x = payload_field_text(&event.payload, "pointer_x");
                let pointer_y = payload_field_text(&event.payload, "pointer_y");
                let pointer_width = payload_field_text(&event.payload, "pointer_width");
                let pointer_height = payload_field_text(&event.payload, "pointer_height");
                TestStep {
                    id: step.id,
                    source_path: event.source,
                    action_kind: Some(action_kind),
                    target_text: event.target_text,
                    text: step.user_action_text,
                    key: step.user_action_key,
                    address: event.payload.address,
                    target_occurrence: event.target_occurrence.map(|value| value as u64),
                    pointer_x,
                    pointer_y,
                    pointer_width,
                    pointer_height,
                    expectations: step.expectations,
                }
            })),
            (None, None) if !step.expectations.is_empty() => Some(Ok(TestStep {
                id: step.id,
                source_path: String::new(),
                action_kind: None,
                target_text: None,
                text: None,
                key: None,
                address: None,
                target_occurrence: None,
                pointer_x: None,
                pointer_y: None,
                pointer_width: None,
                pointer_height: None,
                expectations: step.expectations,
            })),
            (None, None) => None,
            (None, Some(_)) | (Some(_), None) => None,
        })
        .collect()
}

fn built_in_application_identity(entry: &ExampleManifestEntry) -> ApplicationIdentity {
    let explicit = entry.application_identity();
    ApplicationIdentity::new(
        explicit.as_ref().map_or_else(
            || format!("dev.boon.example.{}", entry.id),
            |value| value.package_id.clone(),
        ),
        explicit
            .as_ref()
            .and_then(|value| value.state_namespace.clone())
            .unwrap_or_else(|| format!("builtin:example:{}", entry.id)),
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
        "vcd" => "application/vnd.boon.waveform.vcd",
        "fst" => "application/vnd.boon.waveform.fst",
        "ghw" => "application/vnd.boon.waveform.ghw",
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
