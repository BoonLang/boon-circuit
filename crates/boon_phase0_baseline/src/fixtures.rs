//! Executable Phase 0 replacement seams.
//!
//! These checks deliberately distinguish current behavior from target
//! semantics. Supported analogues travel from source through the ordinary
//! compiler and executor. Future syntax travels through the same compiler and
//! must fail closed until its owning phase replaces the matching seam.

use boon_compiler::{
    CompiledMachinePlanFromSource, ProgramRole, TargetProfile,
    compile_source_path_to_machine_plan_for_role,
};
use boon_plan::{ListId, SourceId};
use boon_plan_executor::{MachineInstance, SessionOptions, SourceEvent, SourcePayload, Value};
use std::path::{Path, PathBuf};
use std::sync::Arc;

const FIXTURE_ROOT: &str = "testdata/phase0/fixtures";

pub fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("baseline crate lives under the workspace crates directory")
        .to_path_buf()
}

pub fn fixture_path(name: &str) -> PathBuf {
    workspace_root().join(FIXTURE_ROOT).join(name)
}

fn compile(name: &str, target: TargetProfile) -> Result<CompiledMachinePlanFromSource, String> {
    compile_source_path_to_machine_plan_for_role(&fixture_path(name), target, ProgramRole::Server)
        .map_err(|error| error.to_string())
}

fn compile_for_role(
    name: &str,
    target: TargetProfile,
    role: ProgramRole,
) -> Result<CompiledMachinePlanFromSource, String> {
    compile_source_path_to_machine_plan_for_role(&fixture_path(name), target, role)
        .map_err(|error| error.to_string())
}

fn machine(compiled: CompiledMachinePlanFromSource) -> Result<MachineInstance, String> {
    MachineInstance::new_shared(Arc::new(compiled.plan), SessionOptions::default())
        .map_err(|error| error.to_string())
}

fn source_id(machine: &MachineInstance, path: &str) -> Result<SourceId, String> {
    machine
        .plan()
        .source_routes
        .iter()
        .find(|route| route.path == path)
        .map(|route| route.source_id)
        .ok_or_else(|| format!("MachinePlan has no source route `{path}`"))
}

fn list_id(compiled: &CompiledMachinePlanFromSource, label: &str) -> Result<ListId, String> {
    compiled
        .plan
        .debug_map
        .list_slots
        .iter()
        .find(|entry| entry.label == label)
        .and_then(|entry| entry.id.strip_prefix("list:"))
        .and_then(|id| id.parse::<usize>().ok())
        .map(ListId)
        .ok_or_else(|| format!("MachinePlan has no debug list `{label}`"))
}

fn number(value: &Value) -> Result<f64, String> {
    let Value::Number(value) = value else {
        return Err(format!("expected Number, received {value:?}"));
    };
    Ok(value.get())
}

fn tagged<'a>(
    value: &'a Value,
    expected: &str,
) -> Result<&'a std::collections::BTreeMap<String, Value>, String> {
    let Value::Tag { tag, fields } = value else {
        return Err(format!("expected tag `{expected}`, received {value:?}"));
    };
    if tag != expected {
        return Err(format!("expected tag `{expected}`, received {value:?}"));
    }
    Ok(fields)
}

fn require_compile_rejection(
    name: &str,
    target: TargetProfile,
    needle: &str,
) -> Result<(), String> {
    let error = compile(name, target)
        .expect_err("future Phase 0 fixture unexpectedly produced a runnable MachinePlan");
    if !error.contains(needle) {
        return Err(format!(
            "{name} failed without the bounded `{needle}` diagnostic seam: {error}"
        ));
    }
    Ok(())
}

pub fn direct_and_wrapped_out_execute() -> Result<(), String> {
    for name in ["direct_out.bn", "wrapped_out.bn", "nested_ownership.bn"] {
        let mut machine = machine(compile(name, TargetProfile::SoftwareDefault)?)?;
        let result = machine
            .root_value_current("result")
            .map_err(|error| error.to_string())?;
        if number(&result)? != 12.0 {
            return Err(format!("{name} produced {result:?}; expected Number(12)"));
        }
    }
    Ok(())
}

pub fn exact_arithmetic_current_binary64_executes() -> Result<(), String> {
    let mut machine = machine(compile(
        "exact_arithmetic_current.bn",
        TargetProfile::SoftwareDefault,
    )?)?;
    let large = machine
        .root_value_current("large_integer")
        .map_err(|error| error.to_string())?;
    let sum = machine
        .root_value_current("decimal_sum")
        .map_err(|error| error.to_string())?;
    let fraction = machine
        .root_value_current("rational_analogue")
        .map_err(|error| error.to_string())?;
    if number(&large)? != 9_007_199_254_740_992.0 {
        return Err(format!("legacy large-integer baseline changed: {large:?}"));
    }
    if number(&sum)?.to_bits() != (0.1_f64 + 0.2_f64).to_bits() {
        return Err(format!("legacy decimal-sum baseline changed: {sum:?}"));
    }
    if number(&fraction)?.to_bits() != (1.0_f64 / 3.0_f64).to_bits() {
        return Err(format!(
            "legacy rational analogue baseline changed: {fraction:?}"
        ));
    }
    require_compile_rejection(
        "future_exact_integer.bn",
        TargetProfile::SoftwareDefault,
        "cannot be represented exactly",
    )
}

pub fn tags_presence_and_fault_current_analogue_executes() -> Result<(), String> {
    let mut machine = machine(compile(
        "tags_presence_fault_current.bn",
        TargetProfile::SoftwareDefault,
    )?)?;
    let present = machine
        .root_value_current("present")
        .map_err(|error| error.to_string())?;
    let present = tagged(&present, "Present")?;
    if present.get("value") != Some(&Value::integer(7).map_err(|error| error.to_string())?) {
        return Err(format!("Present payload changed: {present:?}"));
    }
    let ordinary_fault = machine
        .root_value_current("ordinary_fault")
        .map_err(|error| error.to_string())?;
    if ordinary_fault != Value::tag("InvalidNumber") {
        return Err(format!("ordinary fault Tag changed: {ordinary_fault:?}"));
    }
    let current_absence = machine
        .root_value_current("current_absence")
        .map_err(|error| error.to_string())?;
    if current_absence != Value::tag("Null") {
        return Err(format!(
            "current visible Null Tag changed: {current_absence:?}"
        ));
    }
    Ok(())
}

pub fn bits_map_and_set_future_syntax_fail_closed() -> Result<(), String> {
    require_compile_rejection("future_bits.bn", TargetProfile::SoftwareDefault, "BITS")?;
    require_compile_rejection("future_map.bn", TargetProfile::SoftwareDefault, "MAP")?;
    require_compile_rejection("future_set.bn", TargetProfile::SoftwareDefault, "SET")
}

pub fn typed_views_execute() -> Result<(), String> {
    let mut machine = machine(compile(
        "typed_views_current.bn",
        TargetProfile::SoftwareDefault,
    )?)?;
    let result = machine
        .root_value_current("selected_sum")
        .map_err(|error| error.to_string())?;
    if number(&result)? != 4.0 {
        return Err(format!("typed view selected_sum changed: {result:?}"));
    }
    let page = machine
        .root_value_current("page")
        .map_err(|error| error.to_string())?;
    let page = tagged(&page, "Page")?;
    let Some(Value::List(items)) = page.get("items") else {
        return Err(format!("typed cursor page has no items: {page:?}"));
    };
    if items.len() != 2 || !page.contains_key("next") {
        return Err(format!("typed cursor page changed: {page:?}"));
    }
    Ok(())
}

pub fn proof_erasure_current_path_and_where_rejection_execute() -> Result<(), String> {
    let compiled = compile("proof_erasure_current.bn", TargetProfile::SoftwareDefault)?;
    if compiled.ir.executable.statements.is_empty() {
        return Err("no-WHERE source produced an empty ErasedProgram".to_owned());
    }
    let mut machine = machine(compiled)?;
    let result = machine
        .root_value_current("result")
        .map_err(|error| error.to_string())?;
    if number(&result)? != 42.0 {
        return Err(format!("current proof-free path produced {result:?}"));
    }
    require_compile_rejection("future_where.bn", TargetProfile::SoftwareDefault, "WHERE")
}

pub fn effect_cancellation_rejects_late_publication() -> Result<(), String> {
    let compiled = compile_for_role(
        "effect_cancellation_current.bn",
        TargetProfile::SoftwareDefault,
        ProgramRole::Server,
    )?;
    let mut machine = machine(compiled)?;
    let start = source_id(&machine, "store.start")?;
    let route = machine
        .source_route_token(start, &[])
        .map_err(|error| error.to_string())?;
    let turn = machine
        .apply(SourceEvent {
            sequence: 1,
            route,
            source: start,
            target: None,
            payload: SourcePayload::default(),
        })
        .map_err(|error| error.to_string())?;
    let [invocation] = turn.transient_effects.as_slice() else {
        return Err(format!(
            "effect fixture emitted {} effects; expected one",
            turn.transient_effects.len()
        ));
    };
    let call_id = invocation.call_id;
    if !machine
        .cancel_transient_effect(call_id)
        .map_err(|error| error.to_string())?
    {
        return Err("current effect could not be cancelled".to_owned());
    }
    if machine
        .complete_transient_effect(call_id, Value::tag("Ignored"))
        .is_ok()
    {
        return Err("late completion published after cancellation".to_owned());
    }
    Ok(())
}

pub fn stale_route_is_rejected_before_current_route_executes() -> Result<(), String> {
    let compiled = compile("stale_routing_current.bn", TargetProfile::SoftwareDefault)?;
    let plan = Arc::new(compiled.plan);
    let mut old = MachineInstance::new_shared(
        Arc::clone(&plan),
        SessionOptions {
            program_revision: 1,
            ..SessionOptions::default()
        },
    )
    .map_err(|error| error.to_string())?;
    let mut current = MachineInstance::new_shared(
        Arc::clone(&plan),
        SessionOptions {
            program_revision: 2,
            ..SessionOptions::default()
        },
    )
    .map_err(|error| error.to_string())?;
    let pulse = source_id(&old, "store.pulse")?;
    let stale_route = old
        .source_route_token(pulse, &[])
        .map_err(|error| error.to_string())?;
    let stale = current.apply(SourceEvent {
        sequence: 1,
        route: stale_route,
        source: pulse,
        target: None,
        payload: SourcePayload::default(),
    });
    let error = stale
        .expect_err("revision-1 route unexpectedly entered revision-2 runtime")
        .to_string();
    if !error.contains("stale") {
        return Err(format!(
            "stale route used an unexpected diagnostic: {error}"
        ));
    }
    let route = current
        .source_route_token(pulse, &[])
        .map_err(|error| error.to_string())?;
    current
        .apply(SourceEvent {
            sequence: 1,
            route,
            source: pulse,
            target: None,
            payload: SourcePayload::default(),
        })
        .map_err(|error| error.to_string())?;
    let count = current
        .root_value_current("store.count")
        .map_err(|error| error.to_string())?;
    if number(&count)? != 1.0 {
        return Err(format!(
            "current route did not execute exactly once: {count:?}"
        ));
    }
    // Ensure the old instance remains independently usable, rather than being
    // mutated as a side effect of the rejection check.
    let old_count = old
        .root_value_current("store.count")
        .map_err(|error| error.to_string())?;
    if number(&old_count)? != 0.0 {
        return Err(format!(
            "stale routing mutated the old runtime: {old_count:?}"
        ));
    }
    Ok(())
}

pub fn retained_visible_window_executes_headlessly() -> Result<(), String> {
    let compiled = compile_for_role(
        "visible_window_current.bn",
        TargetProfile::SoftwareDefault,
        ProgramRole::Client,
    )?;
    let cells = list_id(&compiled, "store.cells")?;
    let mut machine = machine(compiled)?;
    let (logical_len, window) = machine
        .list_row_snapshots_window_current(cells, 100..108)
        .map_err(|error| error.to_string())?;
    if logical_len != 2_600 || window.len() != 8 {
        return Err(format!(
            "retained window returned logical_len={logical_len}, rows={}",
            window.len()
        ));
    }
    if machine
        .list_row_snapshots_window_current(cells, 2_700..2_708)
        .map_err(|error| error.to_string())?
        .1
        .is_empty()
    {
        Ok(())
    } else {
        Err("window beyond the logical end returned rows".to_owned())
    }
}

pub fn scalar_and_row_storage_current_analogue_executes() -> Result<(), String> {
    let compiled = compile(
        "packed_scalar_row_current.bn",
        TargetProfile::SoftwareDefault,
    )?;
    if compiled.plan.storage_layout.scalar_slots.is_empty()
        || compiled.plan.storage_layout.list_slots.is_empty()
    {
        return Err("current plan omitted scalar or row storage".to_owned());
    }
    let mut machine = machine(compiled)?;
    let increment = source_id(&machine, "store.increment")?;
    let route = machine
        .source_route_token(increment, &[])
        .map_err(|error| error.to_string())?;
    machine
        .apply(SourceEvent {
            sequence: 1,
            route,
            source: increment,
            target: None,
            payload: SourcePayload::default(),
        })
        .map_err(|error| error.to_string())?;
    let count = machine
        .root_value_current("store.count")
        .map_err(|error| error.to_string())?;
    let row_total = machine
        .root_value_current("store.row_total")
        .map_err(|error| error.to_string())?;
    if number(&count)? != 1.0 || number(&row_total)? != 5.0 {
        return Err(format!(
            "current scalar/row analogue changed: count={count:?}, row_total={row_total:?}"
        ));
    }
    Ok(())
}

pub fn bounded_software_profile_executes_and_future_hardware_fails_closed() -> Result<(), String> {
    let compiled = compile(
        "bounded_hardware_current.bn",
        TargetProfile::SoftwareBounded,
    )?;
    if compiled.plan.target_profile != TargetProfile::SoftwareBounded {
        return Err("bounded analogue compiled for the wrong target profile".to_owned());
    }
    let mut machine = machine(compiled)?;
    let result = machine
        .root_value_current("result")
        .map_err(|error| error.to_string())?;
    if number(&result)? != 3.0 {
        return Err(format!("bounded software analogue produced {result:?}"));
    }
    require_compile_rejection(
        "future_hardware_bits.bn",
        TargetProfile::FpgaTodomvc,
        "BITS",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixture_direct_and_wrapped_out_execute_from_source() {
        direct_and_wrapped_out_execute().unwrap();
    }

    #[test]
    fn fixture_exact_arithmetic_records_current_binary64() {
        exact_arithmetic_current_binary64_executes().unwrap();
    }

    #[test]
    fn fixture_tags_presence_fault_records_current_runtime_algebra() {
        tags_presence_and_fault_current_analogue_executes().unwrap();
    }

    #[test]
    fn fixture_bits_map_set_future_syntax_fails_closed() {
        bits_map_and_set_future_syntax_fail_closed().unwrap();
    }

    #[test]
    fn fixture_typed_views_execute_from_source() {
        typed_views_execute().unwrap();
    }

    #[test]
    fn fixture_proof_erasure_pairs_execution_with_where_rejection() {
        proof_erasure_current_path_and_where_rejection_execute().unwrap();
    }

    #[test]
    fn fixture_effect_cancellation_has_no_late_publication() {
        effect_cancellation_rejects_late_publication().unwrap();
    }

    #[test]
    fn fixture_stale_routing_rejects_old_revision() {
        stale_route_is_rejected_before_current_route_executes().unwrap();
    }

    #[test]
    fn fixture_visible_window_is_retained_and_bounded() {
        retained_visible_window_executes_headlessly().unwrap();
    }

    #[test]
    fn fixture_packed_scalar_row_records_current_tree_analogue() {
        scalar_and_row_storage_current_analogue_executes().unwrap();
    }

    #[test]
    fn fixture_bounded_hardware_records_current_profile_and_future_rejection() {
        bounded_software_profile_executes_and_future_hardware_fails_closed().unwrap();
    }
}
