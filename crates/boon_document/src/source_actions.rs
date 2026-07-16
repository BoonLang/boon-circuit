use std::collections::BTreeMap;

use boon_document_model::Rect;
use sha2::{Digest, Sha256};

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct SourceActionId {
    pub source_path: String,
    pub intent: String,
}

#[derive(Clone, Debug, Default)]
pub struct SourceActionCoverage {
    counts: BTreeMap<SourceActionId, u32>,
}

impl SourceActionCoverage {
    pub fn collect(
        actions: impl IntoIterator<Item = (String, String, Rect)>,
        width: u32,
        height: u32,
    ) -> Result<Self, String> {
        let mut grouped = BTreeMap::<SourceActionId, Vec<Rect>>::new();
        for (source_path, intent, bounds) in actions {
            if !matches!(
                intent.as_str(),
                "change" | "click" | "double_click" | "drag" | "key_down" | "press"
            ) {
                continue;
            }
            let id = SourceActionId {
                source_path,
                intent,
            };
            if !valid_bounds(bounds, width, height) {
                return Err(format!(
                    "action `{} [{}]` has invalid bounds ({}, {}, {}, {}) in {width}x{height}",
                    id.source_path, id.intent, bounds.x, bounds.y, bounds.width, bounds.height
                ));
            }
            grouped.entry(id).or_default().push(bounds);
        }
        if grouped.is_empty() {
            return Err("layout rendered no visible public controls".to_owned());
        }
        Ok(Self {
            counts: grouped
                .into_iter()
                .map(|(id, bounds)| (id, bounds.len().try_into().unwrap_or(u32::MAX)))
                .collect(),
        })
    }

    pub fn counts(&self) -> &BTreeMap<SourceActionId, u32> {
        &self.counts
    }

    pub fn merge_max(&mut self, other: &Self) {
        for (id, count) in &other.counts {
            self.counts
                .entry(id.clone())
                .and_modify(|current| *current = (*current).max(*count))
                .or_insert(*count);
        }
    }

    pub fn restricted_to(&self, expected: &Self) -> Self {
        Self {
            counts: self
                .counts
                .iter()
                .filter(|(id, _)| expected.counts.contains_key(*id))
                .map(|(id, count)| (id.clone(), *count))
                .collect(),
        }
    }

    pub fn mismatches(&self, observed: &Self) -> Vec<String> {
        self.counts
            .iter()
            .filter(|(id, count)| observed.counts.get(*id).copied().unwrap_or(0) != **count)
            .map(|(id, count)| {
                format!(
                    "{} [{}] expected x{count}, observed x{}",
                    id.source_path,
                    id.intent,
                    observed.counts.get(id).copied().unwrap_or(0)
                )
            })
            .collect()
    }

    pub fn total(&self) -> u32 {
        self.counts.values().copied().fold(0, u32::saturating_add)
    }

    pub fn digest(&self) -> String {
        let mut hasher = Sha256::new();
        for (id, count) in &self.counts {
            for value in [&id.source_path, &id.intent] {
                hasher.update((value.len() as u64).to_le_bytes());
                hasher.update(value.as_bytes());
            }
            hasher.update(count.to_le_bytes());
        }
        format!("{:x}", hasher.finalize())
    }
}

fn valid_bounds(bounds: Rect, width: u32, height: u32) -> bool {
    bounds.x.is_finite()
        && bounds.y.is_finite()
        && bounds.width.is_finite()
        && bounds.height.is_finite()
        && bounds.width > 0.0
        && bounds.height > 0.0
        && bounds.x >= -0.5
        && bounds.y >= -0.5
        && bounds.x + bounds.width <= width as f32 + 0.5
        && bounds.y + bounds.height <= height as f32 + 0.5
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coverage_filters_lifecycle_routes_and_preserves_multiplicity() {
        let rect = Rect {
            x: 1.0,
            y: 2.0,
            width: 20.0,
            height: 10.0,
        };
        let coverage = SourceActionCoverage::collect(
            [
                ("publish".to_owned(), "press".to_owned(), rect),
                ("publish".to_owned(), "press".to_owned(), rect),
                ("program".to_owned(), "compiled".to_owned(), rect),
            ],
            100,
            100,
        )
        .unwrap();
        assert_eq!(coverage.total(), 2);
    }
}
