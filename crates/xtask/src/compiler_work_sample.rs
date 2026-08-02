use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::report_v2::ToolResult;

pub(crate) fn require_current_prebuilt_producer(
    workspace: &Path,
    producer: &Path,
) -> ToolResult<()> {
    let producer_modified = producer
        .metadata()
        .map_err(|error| {
            format!(
                "prebuilt release compiler producer is missing at {}; build it once with `cargo build --locked --release --jobs 2 -p boon_cli --bin boon_cli`: {error}",
                producer.display(),
            )
        })?
        .modified()?;
    let mut newest = None::<(SystemTime, PathBuf)>;
    for input in [
        workspace.join("Cargo.toml"),
        workspace.join("Cargo.lock"),
        workspace.join("rust-toolchain.toml"),
        workspace.join(".cargo"),
        workspace.join("crates"),
    ] {
        if input.exists() {
            newest_build_input(&input, &mut newest)?;
        }
    }
    if let Some((modified, input)) = newest.filter(|(modified, _)| *modified > producer_modified) {
        let _ = modified;
        return Err(format!(
            "prebuilt release compiler producer {} is older than build input {}; rebuild it once with `cargo build --locked --release --jobs 2 -p boon_cli --bin boon_cli`",
            producer.display(),
            input.display(),
        )
        .into());
    }
    Ok(())
}

fn newest_build_input(path: &Path, newest: &mut Option<(SystemTime, PathBuf)>) -> ToolResult<()> {
    let metadata = fs::metadata(path)?;
    if metadata.is_dir() {
        for entry in fs::read_dir(path)? {
            newest_build_input(&entry?.path(), newest)?;
        }
        return Ok(());
    }
    if !metadata.is_file() {
        return Ok(());
    }
    let modified = metadata.modified()?;
    if newest
        .as_ref()
        .is_none_or(|(current, _)| modified > *current)
    {
        *newest = Some((modified, path.to_path_buf()));
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ParserWorkSample {
    pub(crate) source_units_attempted: usize,
    pub(crate) source_units_parsed: usize,
    pub(crate) source_bytes_inspected: usize,
    pub(crate) token_inspections: usize,
    pub(crate) symbol_inspections: usize,
    pub(crate) statement_visits: usize,
    pub(crate) expression_visits: usize,
    pub(crate) nodes_rebased: usize,
    pub(crate) validation_visits: usize,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TypeCheckWorkSample {
    pub(crate) inference_invocations: u64,
    pub(crate) inference_rounds: u64,
    pub(crate) inference_expression_visits: u64,
    pub(crate) inference_declaration_visits: u64,
    pub(crate) inference_callable_visits: u64,
    pub(crate) inference_call_visits: u64,
    pub(crate) inference_call_changed_visits: u64,
    pub(crate) inference_call_noop_visits: u64,
    pub(crate) inference_call_seed_enqueues: u64,
    pub(crate) inference_call_input_enqueues: u64,
    pub(crate) inference_call_output_enqueues: u64,
    pub(crate) inference_call_callee_enqueues: u64,
    pub(crate) inference_call_selector_enqueues: u64,
    pub(crate) inference_call_output_scope_enqueues: u64,
    pub(crate) inference_call_output_origin_skips: u64,
    pub(crate) inference_selector_visits: u64,
    pub(crate) inference_pattern_visits: u64,
    pub(crate) context_scheme_worklist_invocations: u64,
    pub(crate) context_scheme_worklist_visits: u64,
    pub(crate) context_scheme_worklist_changes: u64,
    pub(crate) wrapper_scheme_worklist_invocations: u64,
    pub(crate) wrapper_scheme_worklist_visits: u64,
    pub(crate) wrapper_scheme_changed_owners: u64,
    pub(crate) wrapper_scheme_parameter_changes: u64,
    pub(crate) wrapper_scheme_result_changes: u64,
    pub(crate) checked_flow_cache_hits: u64,
    pub(crate) checked_flow_cache_misses: u64,
    pub(crate) checked_flow_cache_invalidations: u64,
    pub(crate) checked_flow_cache_reverse_invalidation_traversals: u64,
    pub(crate) checked_flow_cache_full_resets: u64,
    pub(crate) checked_flow_cache_rejected_invalid_ids: u64,
    pub(crate) checked_flow_indexed_read_hits: u64,
    pub(crate) checked_flow_indexed_read_missing: u64,
    pub(crate) checked_flow_indexed_read_rejected: u64,
    pub(crate) checked_flow_indexed_out_hits: u64,
    pub(crate) checked_flow_indexed_out_missing: u64,
    pub(crate) diagnostic_flow_install_attempts: u64,
    pub(crate) diagnostic_flow_duplicate_ids: u64,
    pub(crate) diagnostic_flow_out_of_range_ids: u64,
    pub(crate) diagnostic_flow_missing_parser_ids: u64,
    pub(crate) diagnostic_replay_requests: u64,
    pub(crate) diagnostic_replay_hits: u64,
    pub(crate) diagnostic_replay_misses: u64,
    pub(crate) diagnostic_replay_unique_expressions: u64,
}

impl TypeCheckWorkSample {
    pub(crate) fn inference_calls_are_accounted(self) -> bool {
        self.inference_call_changed_visits
            .checked_add(self.inference_call_noop_visits)
            == Some(self.inference_call_visits)
    }

    pub(crate) fn diagnostic_replay_is_accounted(self) -> bool {
        self.diagnostic_replay_hits
            .checked_add(self.diagnostic_replay_misses)
            == Some(self.diagnostic_replay_requests)
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct WorkSample {
    pub(crate) source_units: usize,
    pub(crate) parsed_expressions: usize,
    pub(crate) checked_expressions: usize,
    pub(crate) checked_calls: usize,
    pub(crate) semantic_graph_nodes: usize,
    pub(crate) cancellation_checkpoints: usize,
    pub(crate) parse: ParserWorkSample,
    pub(crate) typecheck: TypeCheckWorkSample,
}

impl WorkSample {
    pub(crate) fn has_complete_frontend_work(self) -> bool {
        self.source_units > 0
            && self.parse.source_units_attempted == self.source_units
            && self.parse.source_units_parsed == self.source_units
            && self.parse.source_bytes_inspected > 0
            && self.parse.token_inspections > 0
            && self.parse.statement_visits > 0
            && self.parse.expression_visits > 0
            && self.parse.validation_visits > 0
            && self.typecheck.inference_invocations > 0
            && self.typecheck.inference_expression_visits > 0
            && self.typecheck.diagnostic_flow_install_attempts > 0
            && self.typecheck.diagnostic_replay_requests > 0
            && self.typecheck.inference_calls_are_accounted()
            && self.typecheck.diagnostic_replay_is_accounted()
    }
}
