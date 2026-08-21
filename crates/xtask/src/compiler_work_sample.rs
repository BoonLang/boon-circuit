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
        workspace.join("third_party/mimalloc-3.5.0"),
    ] {
        if input.exists() {
            newest_build_input(&input, &mut newest)?;
        }
    }
    for entry in fs::read_dir(workspace.join("crates"))? {
        let path = entry?.path();
        if path.file_name().is_some_and(|name| name == "xtask") {
            continue;
        }
        newest_build_input(&path, &mut newest)?;
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
    pub(crate) source_units_reused: usize,
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
    pub(crate) owner_statements: u64,
    pub(crate) owner_expressions: u64,
    pub(crate) owner_local_constraints: u64,
    pub(crate) owner_interface_imports: u64,
    pub(crate) owner_interface_plan_direct_owners: u64,
    pub(crate) owner_interface_plan_required_owners: u64,
    pub(crate) owner_interface_plan_provider_sccs: u64,
    pub(crate) owner_interface_plan_result_transfers: u64,
    pub(crate) owner_interface_plan_transfer_nodes: u64,
    pub(crate) owner_interface_plan_transfer_edges: u64,
    pub(crate) owner_calls: u64,
    pub(crate) owner_unification_steps: u64,
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
pub(crate) struct KernelResidualModuleWorkSample {
    pub(crate) owner: u32,
    pub(crate) operations: u32,
    pub(crate) frames: u32,
    pub(crate) linked_operations: u64,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct KernelCompileWorkSample {
    pub(crate) definition_modules: u64,
    pub(crate) principal_expressions: u64,
    pub(crate) residual_type_modules: u64,
    pub(crate) residual_module_operations: u64,
    pub(crate) residual_module_terms: u64,
    pub(crate) residual_frames: u64,
    pub(crate) linked_operations: u64,
    pub(crate) scheduled_work_items: u64,
    pub(crate) acyclic_residual_frames: u64,
    pub(crate) dominant_module_owner: u64,
    pub(crate) dominant_module_operations: u64,
    pub(crate) dominant_module_frames: u64,
    pub(crate) dominant_module_linked_operations: u64,
    pub(crate) residual_module_ranking: [KernelResidualModuleWorkSample; 16],
    pub(crate) linked_terms: u64,
    pub(crate) acyclic_initial_operations: u64,
    pub(crate) compiled_call_sites: u64,
    pub(crate) invocation_frames: u64,
    pub(crate) reused_invocation_frames: u64,
    pub(crate) direct_result_summaries: u64,
    pub(crate) summary_definition_nodes: u64,
    pub(crate) summary_constant_folded_nodes: u64,
    pub(crate) summary_selector_fused_records: u64,
    pub(crate) summary_deduplicated_nodes: u64,
    pub(crate) summary_pruned_nodes: u64,
    pub(crate) summary_pruned_inputs: u64,
    pub(crate) summary_invoke_nodes: u64,
    pub(crate) principal_result_reuses: u64,
    pub(crate) principal_expression_reuses: u64,
    pub(crate) pruned_invocation_expressions: u64,
    pub(crate) specialization_plans: u64,
    pub(crate) reused_specialization_plans: u64,
    pub(crate) max_call_depth: u64,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct KernelSummaryDefinitionWorkSample {
    pub(crate) definition: u32,
    pub(crate) program_evaluations: u64,
    pub(crate) node_evaluations: u64,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct FrozenTypeStoreLayoutSample {
    pub(crate) term_rows: u64,
    pub(crate) term_capacity: u64,
    pub(crate) name_rows: u64,
    pub(crate) name_capacity: u64,
    pub(crate) name_bytes: u64,
    pub(crate) name_byte_capacity: u64,
    pub(crate) child_rows: u64,
    pub(crate) child_capacity: u64,
    pub(crate) variant_rows: u64,
    pub(crate) variant_capacity: u64,
    pub(crate) object_shape_rows: u64,
    pub(crate) object_shape_capacity: u64,
    pub(crate) object_field_rows: u64,
    pub(crate) object_field_capacity: u64,
    pub(crate) semantic_order_rows: u64,
    pub(crate) semantic_order_capacity: u64,
    pub(crate) payload_len_bytes: u64,
    pub(crate) payload_capacity_bytes: u64,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct KernelSolveWorkSample {
    pub(crate) variables: u64,
    pub(crate) scheduled_work_items: u64,
    pub(crate) operations: u64,
    pub(crate) activations: u64,
    pub(crate) unify_activations: u64,
    pub(crate) publish_activations: u64,
    pub(crate) projection_activations: u64,
    pub(crate) select_activations: u64,
    pub(crate) record_activations: u64,
    pub(crate) summary_node_evaluations: u64,
    pub(crate) summary_definition_ranking: [KernelSummaryDefinitionWorkSample; 16],
    pub(crate) summary_call_activations: u64,
    pub(crate) mutations: u64,
    pub(crate) union_operations: u64,
    pub(crate) rich_output_flow_exports: u64,
    pub(crate) term_intern_requests: u64,
    pub(crate) term_intern_hits: u64,
    pub(crate) term_intern_requests_by_kind: [u64; 8],
    pub(crate) term_intern_hits_by_kind: [u64; 8],
    pub(crate) structural_widen_requests: u64,
    pub(crate) structural_widen_hits: u64,
    pub(crate) dynamic_dependency_edges: u64,
    pub(crate) frozen_type_store: FrozenTypeStoreLayoutSample,
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
    pub(crate) kernel_compile: KernelCompileWorkSample,
    pub(crate) kernel_solve: KernelSolveWorkSample,
}

impl WorkSample {
    pub(crate) fn has_complete_frontend_work(self) -> bool {
        let legacy_typecheck_work = self.typecheck.inference_invocations > 0
            && self.typecheck.inference_expression_visits > 0
            && self.typecheck.diagnostic_flow_install_attempts > 0
            && self.typecheck.diagnostic_replay_requests > 0
            && self.typecheck.inference_calls_are_accounted()
            && self.typecheck.diagnostic_replay_is_accounted();
        let owner_typecheck_work = self.typecheck.owner_statements > 0
            && self.typecheck.owner_expressions > 0
            && self.typecheck.owner_local_constraints > 0
            && self.typecheck.owner_unification_steps > 0;
        let kernel_work = self.kernel_compile.definition_modules > 0
            && self.kernel_compile.linked_operations > 0
            && self.kernel_solve.operations > 0
            && self.kernel_solve.activations > 0
            && self.kernel_solve.rich_output_flow_exports > 0;
        self.source_units > 0
            && self
                .parse
                .source_units_attempted
                .checked_add(self.parse.source_units_reused)
                == Some(self.source_units)
            && self.parse.source_units_parsed == self.parse.source_units_attempted
            && self.parse.source_bytes_inspected > 0
            && self.parse.token_inspections > 0
            && self.parse.statement_visits > 0
            && self.parse.expression_visits > 0
            && self.parse.validation_visits > 0
            && (legacy_typecheck_work || owner_typecheck_work)
            && kernel_work
    }

    pub(crate) fn has_cold_complete_frontend_work(self) -> bool {
        self.has_complete_frontend_work()
            && self.parse.source_units_attempted == self.source_units
            && self.parse.source_units_parsed == self.source_units
            && self.parse.source_units_reused == 0
    }
}
