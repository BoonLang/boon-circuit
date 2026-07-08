// Included by `../counter.rs`.

// test: bytecode_report_compiles_counter_scalar_routes_with_interpreter_parity
#[test]
fn bytecode_report_compiles_counter_scalar_routes_with_interpreter_parity() {
    let report = verify_expression_bytecode_report(
        Path::new("../../examples/counter.bn"),
        Path::new("../../examples/counter.scn"),
        None,
    )
    .unwrap();
    assert_eq!(report["status"], json!("pass"));
    let bytecode = &report["expression_bytecode"];
    assert_eq!(
        bytecode["execution_surface"],
        "scalar_source_route_expressions"
    );
    assert_eq!(bytecode["interpreter_oracle"], "ScalarEquationPlan");
    assert_eq!(bytecode["candidate_expression_count"], json!(3));
    assert_eq!(bytecode["compiled_expression_count"], json!(3));
    assert_eq!(bytecode["fallback_count"], json!(0));
    assert_eq!(bytecode["deopt_count"], json!(0));
    assert_eq!(bytecode["parity_passed"], json!(true));
    assert_eq!(bytecode["hot_path_ready"], json!(true));
    assert_eq!(bytecode["op_histogram"]["number_infix"], json!(2));
    assert_eq!(bytecode["op_histogram"]["const_text"], json!(1));
}

// test: generated_kernel_report_proves_counter_scalar_subset_against_bytecode_and_interpreter
#[test]
fn generated_kernel_report_proves_counter_scalar_subset_against_bytecode_and_interpreter() {
    let parsed = parse_source(
        "examples/counter.bn",
        include_str!("../../../../../examples/counter.bn"),
    )
    .unwrap();
    let ir = lower(&parsed).unwrap();
    let compiled = CompiledProgram::from_ir(&ir).unwrap();
    let scenario = parse_scenario(Path::new("../../examples/counter.scn")).unwrap();
    let report = scalar_generated_kernel_report(&compiled, &scenario).unwrap();

    assert_eq!(report["kernel_kind"], json!("generated_rust_enum_kernel"));
    assert_eq!(report["candidate_expression_count"], json!(3));
    assert_eq!(report["generated_kernel_count"], json!(3));
    assert_eq!(report["fallback_count"], json!(0));
    assert_eq!(report["deopt_count"], json!(0));
    assert_eq!(report["parity_passed"], json!(true));
    assert_eq!(report["hot_path_ready"], json!(true));
    assert_eq!(
        report["promotion_decision"],
        json!("not_promoted_without_release_metric")
    );
    assert_eq!(report["bytecode_op_histogram"]["number_infix"], json!(2));
    assert_eq!(report["bytecode_op_histogram"]["const_text"], json!(1));
    assert_eq!(
        report["generated_kernel_histogram"]["generated_number_infix_enum"],
        json!(2)
    );
    assert_eq!(
        report["generated_kernel_histogram"]["generated_const_text_borrow"],
        json!(1)
    );
    assert_eq!(report["generated_number_op_histogram"]["add"], json!(1));
    assert_eq!(
        report["generated_number_op_histogram"]["subtract"],
        json!(1)
    );
    assert_eq!(report["generated_static_borrow_count"], json!(1));
    assert_eq!(report["generated_dynamic_string_count"], json!(2));
}

// test: compiled_artifact_inspection_does_not_reparse_source_and_reports_runtime_load
#[test]
fn compiled_artifact_inspection_does_not_reparse_source_and_reports_runtime_load() {
    let temp_root = TestTempRoot::new("compiled-artifact-load-test");
    let source = temp_root.join("counter.bn");
    std::fs::copy("../../examples/counter.bn", &source).unwrap();
    let artifact = temp_root.join("counter.boonc");
    let compile_report = temp_root.join("counter-compile-report.json");
    emit_compiled_artifact(&source, &artifact, Some(&compile_report)).unwrap();
    std::fs::remove_file(&source).unwrap();

    let load_report = temp_root.join("counter-load-report.json");
    let loaded = inspect_compiled_artifact_report(&artifact, Some(&load_report)).unwrap();
    verify_report_schema(&load_report).unwrap();
    assert_eq!(loaded["inspection_result"]["artifact_valid"], json!(true));
    assert_eq!(
        loaded["inspection_result"]["runtime_instantiated_from_artifact"],
        json!(true)
    );
    assert_eq!(
        loaded["inspection_result"]["runtime_engine"],
        json!("plan_executor")
    );
    assert_eq!(
        loaded["inspection_result"]["plan_executor_runtime_from_artifact"],
        json!(true)
    );
    assert_eq!(
        loaded["inspection_result"]["plan_executor_provenance"]["generic_fallback_enabled"],
        json!(false)
    );
    assert_eq!(
        loaded["inspection_result"]["source_free_runtime_load_available"],
        json!(true)
    );
    assert_eq!(
        loaded["inspection_result"]["source_reparse_required_for_current_runtime"],
        json!(false)
    );
    assert_eq!(
        loaded["inspection_result"]["typed_ir_required_for_mvp_loader"],
        json!(false)
    );
    assert_eq!(
        loaded["inspection_result"]["runtime_plan_present"],
        json!(true)
    );
    assert_eq!(
        loaded["inspection_result"]["runtime_plan_generic_derived_deserialized_from_artifact"],
        json!(true)
    );
    assert!(
        loaded["inspection_result"]["runtime_plan_generic_derived_deserialized_counts"]
            .as_object()
            .is_some_and(|counts| counts
                .get("root_supported_count")
                .and_then(JsonValue::as_u64)
                .is_some())
    );
    assert_eq!(
        loaded["inspection_result"]["runtime_plan_storage_deserialized_from_artifact"],
        json!(true)
    );
    assert!(
        loaded["inspection_result"]["runtime_plan_storage_deserialized_counts"]
            .as_object()
            .is_some_and(|counts| counts
                .get("list_slot_count")
                .and_then(JsonValue::as_u64)
                .is_some())
    );
    assert_eq!(
        loaded["inspection_result"]["runtime_plan_document_lowering_deserialized_from_artifact"],
        json!(true)
    );
    assert!(
        loaded["inspection_result"]["runtime_plan_document_lowering_deserialized_counts"]
            .as_object()
            .is_some_and(|counts| counts
                .get("render_slot_count")
                .and_then(JsonValue::as_u64)
                .is_some())
    );
    assert_eq!(
        loaded["inspection_result"]["runtime_plan_non_route_tables_deserialized_from_artifact"],
        json!(true)
    );
    assert!(
        loaded["inspection_result"]["runtime_plan_non_route_tables_deserialized_counts"]
            .as_object()
            .is_some_and(|counts| counts
                .get("runtime_symbol_count")
                .and_then(JsonValue::as_u64)
                .is_some())
    );
    assert_eq!(
        loaded["inspection_result"]["runtime_plan_source_routes_deserialized_from_artifact"],
        json!(true)
    );
    assert!(
        loaded["inspection_result"]["runtime_plan_source_routes_deserialized_counts"]
            .as_object()
            .is_some_and(|counts| counts
                .get("route_count")
                .and_then(JsonValue::as_u64)
                .is_some())
    );
    assert!(
        !loaded["inspection_result"]["missing_runtime_plan_sections"]
            .as_array()
            .unwrap()
            .iter()
            .any(|section| section.as_str() == Some("runtime_plan"))
    );
    assert!(
        !loaded["inspection_result"]["missing_runtime_plan_sections"]
            .as_array()
            .unwrap()
            .iter()
            .any(|section| section.as_str() == Some("generic_derived_ast_free_plan"))
    );
    assert!(
        !loaded["inspection_result"]["missing_runtime_plan_sections"]
            .as_array()
            .unwrap()
            .iter()
            .any(|section| section.as_str() == Some("runtime_storage_initialization_plan"))
    );
    assert!(
        !loaded["inspection_result"]["missing_runtime_plan_sections"]
            .as_array()
            .unwrap()
            .iter()
            .any(|section| section.as_str() == Some("document_lowering_runtime_tables"))
    );
    assert_eq!(
        loaded["inspection_result"]["source_reparse_attempted"],
        json!(false)
    );
    assert_eq!(
        loaded["inspection_result"]["source_file_access"],
        json!("not_attempted")
    );
    assert_eq!(
        loaded["inspection_result"]["scenario_execution_available"],
        json!(false)
    );
    assert_eq!(
        loaded["compiled_artifact"]["sha256"].as_str(),
        Some(sha256_file(&artifact).unwrap().as_str())
    );
}

// test: compiled_artifact_rejects_non_ast_free_runtime_plan
#[test]
fn compiled_artifact_rejects_non_ast_free_runtime_plan() {
    let temp_root = TestTempRoot::new("compiled-artifact-runtime-plan-test");
    let artifact = temp_root.join("counter.boonc");
    emit_compiled_artifact(Path::new("../../examples/counter.bn"), &artifact, None).unwrap();
    let mut artifact_json: JsonValue =
        serde_json::from_slice(&std::fs::read(&artifact).unwrap()).unwrap();
    artifact_json["runtime_plan"]["ast_free"] = json!(false);
    write_json(&artifact, &artifact_json).unwrap();
    let error = inspect_compiled_artifact_report(&artifact, None).unwrap_err();
    assert!(
        error.to_string().contains("runtime_plan must be AST-free"),
        "unexpected runtime_plan validation error: {error}"
    );
}

// test: compiled_artifact_rejects_non_ast_free_document_lowering_plan
#[test]
fn compiled_artifact_rejects_non_ast_free_document_lowering_plan() {
    let temp_root = TestTempRoot::new("compiled-artifact-document-lowering-test");
    let artifact = temp_root.join("counter.boonc");
    emit_compiled_artifact(Path::new("../../examples/counter.bn"), &artifact, None).unwrap();
    let mut artifact_json: JsonValue =
        serde_json::from_slice(&std::fs::read(&artifact).unwrap()).unwrap();
    artifact_json["runtime_plan"]["document_lowering"]["document_lowering_runtime_ast_free"] =
        json!(false);
    write_json(&artifact, &artifact_json).unwrap();
    let error = inspect_compiled_artifact_report(&artifact, None).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("runtime_plan document_lowering must be AST-free"),
        "unexpected document lowering validation error: {error}"
    );
}

// test: runtime_live_cache_source_compile_uses_compiler_runtime_ir_facade
#[test]
fn runtime_live_cache_source_compile_uses_compiler_runtime_ir_facade() {
    let source = format!("{}\n", include_str!("../../../../../examples/counter.bn"));
    let (_plan, profile) =
        cached_runtime_plan_from_source_profiled("runtime-live-cache-compiler-facade.bn", &source)
            .unwrap();

    assert_eq!(profile["owner"], "boon_compiler");
    assert_eq!(profile["surface"], "runtime-ir");
    assert!(
        profile["runtime_program_build_ms"].as_f64().is_some(),
        "runtime should report only the remaining local runtime program build after compiler-owned parse/lower/verify: {profile}"
    );
}

// test: runtime_full_static_cache_source_compile_uses_compiler_full_ir_facade
#[test]
fn runtime_full_static_cache_source_compile_uses_compiler_full_ir_facade() {
    let source = format!("{}\n", include_str!("../../../../../examples/counter.bn"));
    let units = vec![RuntimeSourceUnit {
        path: "runtime-full-cache-compiler-facade.bn".to_owned(),
        source,
    }];
    let (_plan, profile) =
        cached_full_runtime_plan_from_project_profiled("runtime-full-cache", &units).unwrap();

    assert_eq!(profile["owner"], "boon_compiler");
    assert_eq!(profile["surface"], "full-ir");
    assert!(
        profile["runtime_program_build_ms"].as_f64().is_some(),
        "runtime full/static cache should report only the remaining local runtime program build after compiler-owned full parse/lower/verify: {profile}"
    );
}

// test: runtime_parsed_program_lowering_uses_compiler_runtime_ir_facade
#[test]
fn runtime_parsed_program_lowering_uses_compiler_runtime_ir_facade() {
    let source = include_str!("../../../../../examples/counter.bn");
    let parsed = parse_source(
        "runtime-parsed-program-compiler-facade.bn".to_owned(),
        source.to_owned(),
    )
    .unwrap();
    let ir = lower_for_runtime(&parsed).unwrap();
    let program_metadata = compiler_typed_program_report_metadata_from_ir(&ir);

    assert!(
        program_metadata.expression_count > 0,
        "runtime parsed-program lowering should still produce usable runtime IR"
    );
    assert!(
        program_metadata.static_schedule_verified,
        "compiler-owned parsed-program lowering should preserve verification"
    );
}

// test: list_index_report_counters_include_task_0301_fields
#[test]
fn list_index_report_counters_include_task_0301_fields() {
    let report = RuntimeListScanCounters {
        rows_scanned: 10,
        row_occurrences_scanned: 3,
        order_slots_refreshed: 2,
        summary_fields_scanned: 4,
        dirty_entries_deduplicated: 1,
        route_candidates_visited: 5,
        text_lookup_index_hits: 1,
        text_lookup_index_misses: 0,
        text_lookup_index_candidates: 2,
        numeric_lookup_index_hits: 1,
        numeric_lookup_index_misses: 0,
        numeric_lookup_index_candidates: 2,
        list_find_rows_scanned: 1,
        filter_text_contains_rows_scanned: 2,
        filter_field_rows_scanned: 3,
        move_field_rows_scanned: 4,
        join_field_rows_scanned: 5,
        map_join_field_fusions: 1,
        map_join_field_rows_fused: 2,
        list_view_direct_rows: 3,
        list_view_row_ref_materializations_avoided: 3,
        retain_rows_scanned: 6,
    }
    .to_report();

    for field in [
        "rows_scanned",
        "row_occurrences_scanned",
        "order_slots_refreshed",
        "summary_fields_scanned",
        "dirty_entries_deduplicated",
        "route_candidates_visited",
        "text_lookup_index_hits",
        "text_lookup_index_misses",
        "text_lookup_index_candidates",
        "numeric_lookup_index_hits",
        "numeric_lookup_index_misses",
        "numeric_lookup_index_candidates",
        "list_find_rows_scanned",
        "filter_text_contains_rows_scanned",
        "filter_field_rows_scanned",
        "move_field_rows_scanned",
        "join_field_rows_scanned",
        "map_join_field_fusions",
        "map_join_field_rows_fused",
        "list_view_direct_rows",
        "list_view_row_ref_materializations_avoided",
        "retain_rows_scanned",
    ] {
        assert!(report.get(field).is_some(), "missing counter `{field}`");
    }
}

// test: live_runtime_default_source_constructor_uses_plan_executor_for_document_programs
#[test]
fn live_runtime_default_source_constructor_uses_plan_executor_for_document_programs() {
    let runtime = LiveRuntime::from_source(
        "examples/counter.bn",
        include_str!("../../../../../examples/counter.bn"),
    )
    .expect("Counter should initialize through the default LiveRuntime constructor");

    assert_eq!(
        runtime.engine_provenance_report()["engine"],
        "plan_executor"
    );
    assert_eq!(
        runtime.engine_provenance_report()["generic_fallback_enabled"],
        false
    );
}

