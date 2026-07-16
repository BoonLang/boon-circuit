use crate::program_bundle::{ProgramSource, compile_program_bundle};
use crate::protocol::{
    ApplicationIdentity, AssetBlob, CatalogItem, MigrationBundle, MigrationStage,
    MigrationTestDriver, SourceUnit, TestStep,
};
use boon_plan::ProgramRole;
use boon_runtime::{ExampleManifestEntry, PairedProgramLifecycle, ProgramBundle, RuntimeResult};
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
    program_bundle: Option<ProgramBundle>,
}

impl LoadedExample {
    pub(crate) fn program_bundle(&self) -> Option<&ProgramBundle> {
        self.program_bundle.as_ref()
    }

    pub(crate) fn start_paired_programs(&self) -> RuntimeResult<Option<PairedProgramLifecycle>> {
        self.program_bundle
            .as_ref()
            .map(ProgramBundle::start_paired)
            .transpose()
    }
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
        let program_bundle = load_program_bundle(entry, &base_application, &units)?;
        let application = program_bundle
            .as_ref()
            .and_then(|bundle| bundle.artifact(ProgramRole::Document))
            .map_or(base_application, |artifact| artifact.application().clone());
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
            program_bundle,
        })
    }
}

fn load_program_bundle(
    entry: &ExampleManifestEntry,
    base_application: &ApplicationIdentity,
    primary_units: &[SourceUnit],
) -> RuntimeResult<Option<ProgramBundle>> {
    if entry.programs.is_empty() {
        return Ok(None);
    }
    let paired = entry.programs.len() > 1;
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
                if paired {
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
    if let Some(document) = sources
        .iter()
        .find(|source| source.role == ProgramRole::Document)
        && document.units != primary_units
    {
        return Err(format!(
            "example `{}` primary source differs from its declared document program",
            entry.id
        )
        .into());
    }
    compile_program_bundle(sources).map(Some)
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
        assert!(example.program_bundle().is_none());
        assert!(!example.units.is_empty());
        assert!(example.units.iter().all(|unit| !unit.path.is_empty()));
        assert!(example.units.iter().all(|unit| !unit.source.is_empty()));
        assert!(!example.test_steps.is_empty());
        assert!(
            example
                .test_steps
                .iter()
                .all(|step| !step.expectations.is_empty())
        );
        assert!(example.test_steps[0].action_kind.is_none());
        assert!(matches!(
            example.test_steps[0].expectations.as_slice(),
            [boon_runtime::ScenarioExpectation::RootText { name, value }]
                if name == "store.count" && value == "0"
        ));
        let first_action = example
            .test_steps
            .iter()
            .find(|step| step.action_kind.is_some())
            .expect("counter action step");
        assert!(matches!(
            first_action.expectations.as_slice(),
            [boon_runtime::ScenarioExpectation::RootText { name, value }]
                if name == "store.count" && value == "1"
        ));
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
        assert_eq!(migration_bundle.launch_stage, "v1");
        assert_eq!(migration_bundle.test_driver, MigrationTestDriver::Migration);
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

        let persons = catalog.open("persons_pro").expect("Persons.pro sources");
        assert_eq!(persons.label, "Persons.pro");
        assert_eq!(persons.application.package_id, "pro.persons.workspace");
        assert_eq!(persons.application.state_namespace, "local-first-v1");
        let persons_migration = persons
            .migration
            .as_ref()
            .expect("Persons.pro source-controlled migration sequence");
        assert_eq!(persons_migration.initial_stage, "v1");
        assert_eq!(persons_migration.launch_stage, "v3");
        assert_eq!(persons_migration.test_driver, MigrationTestDriver::Example);
        assert_eq!(
            persons.units,
            persons_migration
                .launch()
                .expect("Persons.pro launch stage")
                .units
        );
        assert!(
            persons
                .units
                .last()
                .is_some_and(|unit| unit.path.ends_with("persons_pro/RUN.bn"))
        );
        let persons_step_ids = persons
            .test_steps
            .iter()
            .map(|step| step.id.as_str())
            .collect::<BTreeSet<_>>();
        assert_eq!(persons_step_ids.len(), persons.test_steps.len());
        for expected in [
            "fresh-anonymous-workspace-and-starter-preview",
            "diagnostic-focuses-source-location",
            "passkey-cancel-preserves-anonymous",
            "passkey-registration-failure-preserves-anonymous",
            "first-passkey-protects",
            "duplicate-credential-is-rejected",
            "second-passkey-same-account",
            "authentication-cancellation-preserves-sign-out",
            "authentication-failure-preserves-sign-out",
            "passkey-sign-in-restores-access",
            "phone-preview-width",
            "restore-auto-preview-width",
        ] {
            assert!(persons_step_ids.contains(expected), "missing {expected}");
        }

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

    #[test]
    fn catalog_program_declarations_build_the_unrelated_runtime_owned_pair() {
        const SHARED: &str =
            "crates/boon_native_playground/testdata/paired_fixture/Shared/PairContract.bn";
        const CLIENT: &str = "crates/boon_native_playground/testdata/paired_fixture/Client/RUN.bn";
        const SERVER: &str = "crates/boon_native_playground/testdata/paired_fixture/Server/RUN.bn";

        let catalog = Catalog::load().expect("catalog");
        let mut entry = catalog
            .entries
            .iter()
            .find(|entry| entry.programs.len() == 2)
            .expect("paired catalog declaration")
            .clone();
        entry.id = "paired-fixture".to_owned();
        entry.source = CLIENT.to_owned();
        entry.source_files = vec![SHARED.to_owned(), CLIENT.to_owned()];
        let document_declaration = entry
            .programs
            .iter_mut()
            .find(|program| program.role == ProgramRole::Document)
            .expect("document declaration");
        document_declaration.source = CLIENT.to_owned();
        document_declaration.source_files = vec![SHARED.to_owned(), CLIENT.to_owned()];
        document_declaration.state_namespace = Some("fixture-client".to_owned());
        let server_declaration = entry
            .programs
            .iter_mut()
            .find(|program| program.role == ProgramRole::Server)
            .expect("server declaration");
        server_declaration.source = SERVER.to_owned();
        server_declaration.source_files = vec![SHARED.to_owned(), SERVER.to_owned()];
        server_declaration.state_namespace = Some("fixture-server".to_owned());

        let primary_units = boon_runtime::source_units_for_entry(&entry)
            .expect("fixture document source units")
            .into_iter()
            .map(|unit| SourceUnit {
                path: unit.path,
                source: unit.source,
            })
            .collect::<Vec<_>>();
        let application = ApplicationIdentity::new("dev.boon.paired-fixture", "base", "test");
        let bundle = load_program_bundle(&entry, &application, &primary_units)
            .expect("load fixture program declarations")
            .expect("paired fixture bundle");
        assert_eq!(bundle.artifacts().len(), 2);

        let document = bundle
            .artifact(ProgramRole::Document)
            .expect("fixture document artifact");
        let server = bundle
            .artifact(ProgramRole::Server)
            .expect("fixture server artifact");
        assert_eq!(document.role(), ProgramRole::Document);
        assert_eq!(server.role(), ProgramRole::Server);
        assert_eq!(
            document.capability_profile(),
            boon_runtime::ProgramCapabilityProfile::PublicDocument
        );
        assert_eq!(
            server.capability_profile(),
            boon_runtime::ProgramCapabilityProfile::TrustedServer
        );
        assert_eq!(document.application().state_namespace, "fixture-client");
        assert_eq!(server.application().state_namespace, "fixture-server");
        assert_ne!(document.application(), server.application());
        assert!(!document.source_digest().is_empty());
        assert!(!server.source_digest().is_empty());
        let document_application = document.application().clone();

        let mut loaded = catalog.open("counter").expect("catalog fixture host");
        loaded.application = document_application.clone();
        loaded.program_bundle = Some(bundle);
        let mut lifecycle = loaded
            .start_paired_programs()
            .expect("start fixture programs")
            .expect("fixture pair");
        assert_eq!(lifecycle.session_count(), 2);
        assert_eq!(
            lifecycle
                .artifact(ProgramRole::Document)
                .expect("document session")
                .application(),
            &document_application
        );
        assert_eq!(
            lifecycle
                .root_value_current(ProgramRole::Server, "store.server_count")
                .expect("initial server count"),
            boon_runtime::Value::integer(0).unwrap()
        );
        lifecycle
            .dispatch(
                ProgramRole::Server,
                "store.request_received",
                None,
                boon_runtime::SourcePayload::default(),
            )
            .expect("server source turn");
        assert_eq!(
            lifecycle
                .root_value_current(ProgramRole::Server, "store.server_count")
                .expect("updated server count"),
            boon_runtime::Value::integer(1).unwrap()
        );
        let response = lifecycle
            .output_value_current(ProgramRole::Server, "api_response")
            .expect("server output");
        assert!(matches!(response, boon_runtime::Value::Record(fields)
            if fields.get("count") == Some(&boon_runtime::Value::integer(1).unwrap())));
    }

    #[test]
    fn persons_semantic_memory_is_the_exact_authority_contract() {
        let persons = Catalog::load()
            .expect("catalog")
            .open("persons_pro")
            .expect("Persons.pro sources");
        let migration = persons
            .migration
            .as_ref()
            .expect("Persons.pro migration sequence");
        let plan = crate::compile::compile_migration_stage(
            &persons.application,
            migration,
            &migration.launch_stage,
        )
        .expect("Persons.pro launch plan");

        let scalar_paths = plan
            .persistence
            .memory
            .iter()
            .map(|memory| memory.semantic_path.as_str())
            .collect::<BTreeSet<_>>();
        assert_eq!(
            scalar_paths,
            [
                "store.account_id",
                "store.active_view",
                "store.draft_revision",
                "store.last_valid_draft_artifact_id",
                "store.last_valid_draft_revision",
                "store.mode",
                "store.passkey_workflow_state",
                "store.preview_surface",
                "store.preview_width_mode",
                "store.publish_candidate_revision",
                "store.publish_candidate_source",
                "store.publish_request_sequence",
                "store.publish_settled_sequence",
                "store.published_artifact_id",
                "store.published_capability_profile",
                "store.published_compiler",
                "store.published_plan_digest",
                "store.published_revision",
                "store.published_source",
                "store.published_source_digest",
                "store.published_target",
                "store.signed_out",
                "store.source_draft",
                "store.workspace_grant_id",
                "store.workspace_grant_state",
                "store.workspace_id",
            ]
            .into_iter()
            .collect()
        );

        let list_paths = plan
            .persistence
            .lists
            .iter()
            .map(|list| list.semantic_path.as_str())
            .collect::<BTreeSet<_>>();
        assert_eq!(
            list_paths,
            ["store.credential_descriptors", "store.published_revisions"]
                .into_iter()
                .collect()
        );
        let row_fields = plan
            .persistence
            .lists
            .iter()
            .flat_map(|list| list.row_fields.iter())
            .map(|field| field.semantic_path.as_str())
            .collect::<BTreeSet<_>>();
        assert_eq!(
            row_fields,
            [
                "store.credential_descriptors.credential_id",
                "store.credential_descriptors.label",
                "store.published_revisions.artifact_id",
                "store.published_revisions.capability_profile",
                "store.published_revisions.compiler",
                "store.published_revisions.draft_revision",
                "store.published_revisions.plan_digest",
                "store.published_revisions.request",
                "store.published_revisions.source",
                "store.published_revisions.source_digest",
                "store.published_revisions.target",
            ]
            .into_iter()
            .collect()
        );
        for forbidden in [
            "store.draft_compile_column",
            "store.draft_compile_diagnostic",
            "store.draft_compile_line",
            "store.draft_compile_path",
            "store.passkey_message",
            "store.publish_diagnostic",
            "store.publish_state",
        ] {
            assert!(!scalar_paths.contains(forbidden));
        }
        assert_eq!(plan.persistence.effect_outbox.len(), 2);
    }
}
