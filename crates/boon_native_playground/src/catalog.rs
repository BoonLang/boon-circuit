use crate::protocol::{CatalogItem, SourceUnit, TestStep};
use boon_runtime::{ExampleManifestEntry, RuntimeResult};
use std::collections::BTreeSet;
use std::path::Path;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LoadedExample {
    pub id: String,
    pub label: String,
    pub units: Vec<SourceUnit>,
    pub test_steps: Vec<TestStep>,
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
        let units = boon_runtime::source_units_for_entry(entry)?
            .into_iter()
            .map(|unit| SourceUnit {
                path: unit.path,
                source: unit.source,
            })
            .collect();
        let test_steps = boon_runtime::parse_scenario(Path::new(&entry.scenario))?
            .steps
            .into_iter()
            .filter_map(|step| {
                step.source_event.map(|event| {
                    (
                        step.user_action_kind,
                        step.user_action_text,
                        step.user_action_key,
                        event,
                    )
                })
            })
            .map(|(action_kind, action_text, action_key, event)| TestStep {
                source_path: event.source,
                action_kind,
                target_text: event.target_text,
                text: action_text,
                key: action_key,
                address: event.payload.address,
                target_occurrence: event.target_occurrence.map(|value| value as u64),
            })
            .collect();
        Ok(LoadedExample {
            id: entry.id.clone(),
            label: entry.label.clone(),
            units,
            test_steps,
        })
    }
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
    }
}
