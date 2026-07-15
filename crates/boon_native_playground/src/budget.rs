use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BudgetUnit {
    Microseconds,
    Bytes,
    Count,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BudgetLimit {
    pub unit: BudgetUnit,
    pub at_most: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BudgetContract {
    limits: BTreeMap<String, BudgetLimit>,
}

impl BudgetContract {
    pub fn parse(source: &str) -> Result<Self, String> {
        let document = toml::from_str::<toml::Table>(source)
            .map_err(|error| format!("invalid TOML: {error}"))?;
        let mut limits = BTreeMap::new();
        for (section, value) in document {
            let table = value
                .as_table()
                .ok_or_else(|| format!("budget section `{section}` must be a table"))?;
            let unit = match section.as_str() {
                "latency_ms" => BudgetUnit::Microseconds,
                "bytes" => BudgetUnit::Bytes,
                "count" => BudgetUnit::Count,
                _ => return Err(format!("unknown budget section `{section}`")),
            };
            for (name, value) in table {
                let metric = metric_id(name)?;
                let at_most = limit_value(unit, value, &metric)?;
                if limits
                    .insert(metric.clone(), BudgetLimit { unit, at_most })
                    .is_some()
                {
                    return Err(format!("budget metric `{metric}` is duplicated"));
                }
            }
        }
        if limits.is_empty() {
            return Err("budget contract contains no limits".to_owned());
        }
        Ok(Self { limits })
    }

    pub fn limit(&self, metric: &str) -> Result<BudgetLimit, String> {
        self.limits
            .get(metric)
            .copied()
            .ok_or_else(|| format!("budget contract has no metric `{metric}`"))
    }

    #[cfg(test)]
    fn metric_count(&self) -> usize {
        self.limits.len()
    }
}

fn metric_id(name: &str) -> Result<String, String> {
    if name.is_empty()
        || name.starts_with('_')
        || name.ends_with('_')
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
    {
        return Err(format!("invalid budget metric `{name}`"));
    }
    Ok(name.replace('_', "-"))
}

fn limit_value(unit: BudgetUnit, value: &toml::Value, metric: &str) -> Result<u64, String> {
    match unit {
        BudgetUnit::Microseconds => {
            let milliseconds = match value {
                toml::Value::Integer(value) => *value as f64,
                toml::Value::Float(value) => *value,
                _ => {
                    return Err(format!(
                        "latency budget `{metric}` must be a millisecond number"
                    ));
                }
            };
            if !milliseconds.is_finite() || milliseconds < 0.0 {
                return Err(format!(
                    "latency budget `{metric}` must be finite and non-negative"
                ));
            }
            let microseconds = milliseconds * 1_000.0;
            if microseconds > u64::MAX as f64
                || (microseconds - microseconds.round()).abs() > 0.000_001
            {
                return Err(format!(
                    "latency budget `{metric}` must resolve to whole microseconds"
                ));
            }
            Ok(microseconds.round() as u64)
        }
        BudgetUnit::Bytes | BudgetUnit::Count => value
            .as_integer()
            .and_then(|value| u64::try_from(value).ok())
            .ok_or_else(|| format!("budget `{metric}` must be a non-negative integer")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn persons_contract_is_typed_and_exact() {
        let contract =
            BudgetContract::parse(include_str!("../../../examples/persons_pro.budget.toml"))
                .unwrap();
        assert_eq!(contract.metric_count(), 9);
        assert_eq!(
            contract.limit("keystroke-to-editor-visible-p95").unwrap(),
            BudgetLimit {
                unit: BudgetUnit::Microseconds,
                at_most: 16_700,
            }
        );
        assert_eq!(
            contract.limit("trusted-parent-rebuilds-per-edit").unwrap(),
            BudgetLimit {
                unit: BudgetUnit::Count,
                at_most: 0,
            }
        );
    }

    #[test]
    fn malformed_or_ambiguous_budget_data_is_rejected() {
        assert!(BudgetContract::parse("").is_err());
        assert!(BudgetContract::parse("[seconds]\nframe = 1").is_err());
        assert!(BudgetContract::parse("[count]\nBad_Name = 1").is_err());
        assert!(BudgetContract::parse("[count]\nitems = -1").is_err());
        assert!(BudgetContract::parse("[latency_ms]\nframe = 0.0001").is_err());
    }
}
