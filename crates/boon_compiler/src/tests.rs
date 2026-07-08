use super::*;

#[test]
fn compiler_facade_produces_stable_counter_plan() {
    let source = include_str!("../../../examples/counter.bn");
    let parsed =
        boon_parser::parse_source("examples/counter.bn".to_owned(), source.to_owned()).unwrap();
    let ir = boon_ir::lower(&parsed).unwrap();

    let facade_plan = compile_typed_program(&ir, TargetProfile::SoftwareDefault).unwrap();
    let repeated_plan = compile_typed_program(&ir, TargetProfile::SoftwareDefault).unwrap();

    assert_eq!(
        boon_plan::plan_sha256(&facade_plan).unwrap(),
        boon_plan::plan_sha256(&repeated_plan).unwrap()
    );
}

#[test]
fn compiler_facade_loads_source_path_to_machine_plan() {
    let compiled = compile_source_path_to_machine_plan(
        Path::new("../../examples/bytes_length_plan_ops.bn"),
        TargetProfile::SoftwareDefault,
    )
    .unwrap();

    assert_eq!(compiled.parsed.files.len(), 1);
    assert_eq!(
        compiled.plan.capability_summary.cpu_plan_executor_complete,
        true
    );
    assert_eq!(compiled.load_pipeline_profile["owner"], "boon_compiler");
}

#[test]
fn compiler_facade_owns_compiled_source_report_context() {
    let compiled = compile_source_path_to_machine_plan(
        Path::new("../../examples/bytes_length_plan_ops.bn"),
        TargetProfile::SoftwareDefault,
    )
    .unwrap();
    let context = compiled.report_context();

    assert_eq!(context.program_kind, "generic");
    assert_eq!(context.program_file_count, 1);
    assert_eq!(context.source_files.len(), 1);
    assert_eq!(context.source_units.len(), 1);
    assert_eq!(context.source_units[0].path, context.source_files[0]);
    assert!(context.source_units[0].source.contains("Bytes/length"));
    assert_eq!(context.program_hash, context.source_hash);
    assert_eq!(context.graph_node_count, compiled.ir.graph_node_count);
    assert_eq!(context.load_pipeline_profile["owner"], "boon_compiler");
}

#[test]
fn compiler_facade_owns_manifest_source_units_for_multifile_examples() {
    let units = compiler_source_units_for_path(Path::new("../../examples/cells.bn")).unwrap();

    assert!(units.len() > 1);
    assert!(
        units
            .iter()
            .any(|unit| unit.path.ends_with("examples/cells/model.bn"))
    );
    assert!(
        units
            .iter()
            .any(|unit| unit.path.ends_with("examples/cells.bn"))
    );

    let source = compiler_source_text_for_path(Path::new("../../examples/cells.bn")).unwrap();
    assert!(source.contains("cells_app()"));
}

#[test]
fn compiler_facade_owns_manifest_source_units_from_entry_fields() {
    let source_files = vec![
        "examples/cells/defaults.bn".to_owned(),
        "examples/cells/formula.bn".to_owned(),
        "examples/cells/cell.bn".to_owned(),
        "examples/cells/model.bn".to_owned(),
        "examples/cells/columns.bn".to_owned(),
        "examples/cells/store.bn".to_owned(),
        "examples/cells/view.bn".to_owned(),
        "examples/cells.bn".to_owned(),
    ];
    let units =
        compiler_source_units_for_manifest_source("examples/cells.bn", &source_files).unwrap();

    assert_eq!(units.len(), source_files.len());
    assert_eq!(
        compiler_source_text_for_manifest_source("examples/cells.bn")
            .unwrap()
            .contains("cells_app()"),
        true
    );
}

#[test]
fn compiler_facade_loads_runtime_ir_from_source_units() {
    let units = vec![CompilerSourceUnit {
        path: "examples/counter.bn".to_owned(),
        source: include_str!("../../../examples/counter.bn").to_owned(),
    }];
    let compiled = compile_source_units_to_runtime_ir("examples/counter.bn", &units).unwrap();

    assert_eq!(compiled.parsed.files.len(), 1);
    assert!(compiled.ir.expression_count > 0);
    assert_eq!(compiled.load_pipeline_profile["owner"], "boon_compiler");
    assert_eq!(compiled.load_pipeline_profile["surface"], "runtime-ir");
}

#[test]
fn compiler_facade_loads_full_ir_from_source_units() {
    let units = vec![CompilerSourceUnit {
        path: "examples/counter.bn".to_owned(),
        source: include_str!("../../../examples/counter.bn").to_owned(),
    }];
    let compiled = compile_source_units_to_full_ir("examples/counter.bn", &units).unwrap();

    assert_eq!(compiled.parsed.files.len(), 1);
    assert!(compiled.ir.expression_count > 0);
    assert_eq!(compiled.load_pipeline_profile["owner"], "boon_compiler");
    assert_eq!(compiled.load_pipeline_profile["surface"], "full-ir");
}

#[test]
fn compiler_facade_loads_machine_plan_from_source_units() {
    let units = vec![CompilerSourceUnit {
        path: "examples/counter.bn".to_owned(),
        source: include_str!("../../../examples/counter.bn").to_owned(),
    }];
    let compiled = compile_source_units_to_machine_plan(
        "examples/counter.bn",
        &units,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();

    assert_eq!(compiled.parsed.files.len(), 1);
    assert!(compiled.ir.expression_count > 0);
    assert_eq!(compiled.load_pipeline_profile["owner"], "boon_compiler");
    assert_eq!(compiled.load_pipeline_profile["surface"], "machine-plan");
    assert!(!compiled.plan.regions.is_empty());
    assert!(!compiled.plan.source_routes.is_empty());
}

#[test]
fn compiler_facade_owns_scenario_file_decode() {
    #[derive(Debug, Deserialize)]
    struct ScenarioLite {
        name: String,
        step: Vec<ScenarioStepLite>,
    }

    #[derive(Debug, Deserialize)]
    struct ScenarioStepLite {
        id: String,
    }

    let scenario: ScenarioLite =
        parse_scenario_file(Path::new("../../examples/counter.scn")).unwrap();

    assert_eq!(scenario.name, "generic");
    assert!(
        scenario
            .step
            .iter()
            .any(|step| step.id == "press-increment")
    );
}

#[test]
fn compiler_facade_lowers_parsed_program_to_runtime_ir() {
    let source = include_str!("../../../examples/counter.bn");
    let parsed =
        boon_parser::parse_source("examples/counter.bn".to_owned(), source.to_owned()).unwrap();
    let compiled = compile_parsed_program_to_runtime_ir(parsed).unwrap();

    assert!(compiled.ir.expression_count > 0);
    assert_eq!(compiled.load_pipeline_profile["owner"], "boon_compiler");
    assert_eq!(compiled.load_pipeline_profile["surface"], "runtime-ir");
    assert_eq!(compiled.load_pipeline_profile["parse_ms"], 0.0);
}
