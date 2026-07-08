// Included by `../todomvc.rs`.

// test: compiled_artifact_emission_is_deterministic_and_schema_valid
#[test]
fn compiled_artifact_emission_is_deterministic_and_schema_valid() {
    let temp_root = TestTempRoot::new("compiled-artifact-test");
    let artifact = temp_root.join("todomvc.boonc");
    let report = temp_root.join("todomvc-compile-report.json");
    let first = emit_compiled_artifact(
        Path::new("../../examples/todomvc.bn"),
        &artifact,
        Some(&report),
    )
    .unwrap();
    verify_report_schema(&report).unwrap();
    let first_hash = first["compiled_artifact"]["sha256"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(first_hash, sha256_file(&artifact).unwrap());

    let second =
        emit_compiled_artifact(Path::new("../../examples/todomvc.bn"), &artifact, None).unwrap();
    assert_eq!(
        second["compiled_artifact"]["sha256"].as_str(),
        Some(first_hash.as_str())
    );

    let artifact_json: JsonValue =
        serde_json::from_slice(&std::fs::read(&artifact).unwrap()).unwrap();
    assert_eq!(artifact_json["format"], json!("boonc-json-v1"));
    assert_eq!(
        artifact_json["parser_ast_required_for_execution"],
        json!(false)
    );
    assert!(artifact_json["semantic_index"].is_object());
    assert!(artifact_json["compiled_schedule"]["source_route_op_streams"].is_object());
    assert!(artifact_json["storage_layout"].is_object());
    assert_eq!(artifact_json["runtime_plan"]["ast_free"], json!(true));
    assert_eq!(
        artifact_json["runtime_plan"]["source_free_runtime_instantiation_ready"],
        json!(true)
    );
    assert_eq!(
        artifact_json["typed_ir_required_for_mvp_loader"],
        json!(false)
    );
    assert_eq!(
        artifact_json["runtime_plan"]["runtime_instantiation_blocked_by"],
        json!([])
    );
    assert!(artifact_json["runtime_plan"]["runtime_symbols"]["paths"].is_array());
    assert!(artifact_json["runtime_plan"]["scalar_equations"]["branches"].is_array());
    assert!(artifact_json["runtime_plan"]["list_equations"]["operations"].is_array());
    assert!(artifact_json["runtime_plan"]["source_routes"]["route_slots"].is_array());
    assert_eq!(
        artifact_json["runtime_plan"]["included_runtime_owned_sections"]["runtime_storage_initialization_plan"],
        json!(true)
    );
    assert_eq!(
        artifact_json["runtime_plan"]["included_runtime_owned_sections"]["document_lowering_runtime_tables"],
        json!(true)
    );
    assert_eq!(
        artifact_json["runtime_plan"]["included_runtime_owned_sections"]["generic_derived_partial_ast_free_plan"],
        json!(true)
    );
    assert_eq!(
        artifact_json["runtime_plan"]["storage_initialization"]["storage_runtime_ast_free"],
        json!(true)
    );
    assert_eq!(
        artifact_json["runtime_plan"]["document_lowering"]["document_lowering_runtime_ast_free"],
        json!(true)
    );
    assert_eq!(
        artifact_json["runtime_plan"]["document_lowering"]["format"],
        json!("boonc-document-lowering-runtime-tables-json-v1")
    );
    assert_eq!(
        artifact_json["runtime_plan"]["generic_derived"]["format"],
        json!("boonc-runtime-generic-derived-partial-json-v1")
    );
    assert_eq!(
        artifact_json["runtime_plan"]["generic_derived"]["generic_derived_runtime_ast_free"],
        json!(true)
    );
    assert!(
        artifact_json["runtime_plan"]["generic_derived"]["root_fields"]
            .as_array()
            .is_some_and(|fields| !fields.is_empty())
    );
    assert!(
        artifact_json["runtime_plan"]["generic_derived"]["indexed_fields"]
            .as_array()
            .is_some_and(|fields| !fields.is_empty())
    );
    assert!(
        artifact_json["runtime_plan"]["generic_derived"]["indexed_fields"]
            .as_array()
            .is_some_and(|fields| fields.iter().all(|field| field
                .get("startup_recompute")
                .and_then(|value| value.as_bool())
                .is_some())),
        "indexed generic-derived runtime fields must carry explicit startup_recompute flags"
    );
    assert!(artifact_json["runtime_plan"]["generic_derived"]["unsupported_reasons"].is_object());
    assert!(
        artifact_json["runtime_plan"]["document_lowering"]["root_summary_paths"]
            .as_array()
            .is_some_and(|paths| !paths.is_empty())
    );
    assert!(
        artifact_json["runtime_plan"]["document_lowering"]["list_summary_fields"]
            .as_array()
            .is_some()
    );
    assert!(
        artifact_json["runtime_plan"]["document_lowering"]["projection_storage_resolutions"]
            .is_object()
    );
    assert!(
        artifact_json["runtime_plan"]["storage_initialization"]["root_slots"]
            .as_array()
            .is_some_and(|slots| !slots.is_empty())
    );
    assert!(
        artifact_json["runtime_plan"]["storage_initialization"]["list_slots"]
            .as_array()
            .is_some_and(|slots| !slots.is_empty())
    );
    assert!(
        artifact_json["runtime_plan"]["excluded_parser_ast_sections"]
            .as_array()
            .is_some_and(|sections| !sections.is_empty())
    );
}

// test: compiled_artifact_decodes_document_lowering_runtime_tables_without_ast
#[test]
fn compiled_artifact_decodes_document_lowering_runtime_tables_without_ast() {
    let temp_root = TestTempRoot::new("compiled-artifact-document-lowering-decode-test");

    for_core_compiled_artifacts(&temp_root, |example, fixture| {
        let decoded = fixture.artifact.runtime_document_lowering_tables().unwrap();

        assert_eq!(
            decoded.root_summary_paths, fixture.compiled.document_lowering.root_summary_paths,
            "decoded root summary paths differ for {example}"
        );
        assert_eq!(
            decoded.list_summary_fields, fixture.compiled.document_lowering.list_summary_fields,
            "decoded list summary fields differ for {example}"
        );
        assert_eq!(
            decoded.dynamic_list_view_lists,
            fixture.compiled.document_lowering.dynamic_list_view_lists,
            "decoded dynamic list-view set differs for {example}"
        );
        assert_eq!(
            decoded.projection_storage_resolutions,
            fixture
                .compiled
                .document_lowering
                .projection_storage_resolutions,
            "decoded projection storage resolutions differ for {example}"
        );
        assert_eq!(
            decoded.render_slot_table_hash,
            fixture.compiled.document_lowering.render_slot_table_hash,
            "decoded render slot table hash differs for {example}"
        );
        assert_eq!(
            decoded.render_slot_count, fixture.compiled.document_lowering.render_slot_count,
            "decoded render slot count differs for {example}"
        );
    });

    let artifact_path = temp_root.join("todomvc.boonc");
    let artifact = CompiledArtifact::load_from_path(&artifact_path).unwrap();
    let decoded = artifact.runtime_document_lowering_tables().unwrap();
    let mut corrupt = artifact.body.clone();
    corrupt["runtime_plan"]["document_lowering"]["render_slot_count"] =
        json!(decoded.render_slots.len() + 1);
    let error =
        RuntimeDocumentLoweringTables::from_artifact(&corrupt["runtime_plan"]["document_lowering"])
            .unwrap_err()
            .to_string();
    assert!(
        error.contains("render_slot_count"),
        "wrong render slot count should fail decoding, got {error}"
    );
    corrupt = artifact.body.clone();
    corrupt["runtime_plan"]["document_lowering"]["render_patch_lowering"]["root_patch_target_kind"] =
        json!("wrong");
    let error =
        RuntimeDocumentLoweringTables::from_artifact(&corrupt["runtime_plan"]["document_lowering"])
            .unwrap_err()
            .to_string();
    assert!(
        error.contains("root_patch_target_kind"),
        "wrong render patch lowering constant should fail decoding, got {error}"
    );
}

// test: compiled_artifact_decodes_runtime_symbols_and_equation_tables_without_ast
#[test]
fn compiled_artifact_decodes_runtime_symbols_and_equation_tables_without_ast() {
    let temp_root = TestTempRoot::new("compiled-artifact-non-route-tables-test");

    for_core_compiled_artifacts(&temp_root, |example, fixture| {
        let decoded = fixture.artifact.runtime_non_route_tables().unwrap();

        assert_eq!(
            decoded
                .symbols
                .paths
                .iter()
                .map(|path| path.as_ref())
                .collect::<Vec<_>>(),
            fixture
                .compiled
                .symbols
                .paths
                .iter()
                .map(|path| path.as_ref())
                .collect::<Vec<_>>(),
            "decoded runtime symbols differ for {example}"
        );
        assert_eq!(
            scalar_equation_plan_artifact(&decoded.scalar_equations),
            scalar_equation_plan_artifact(&fixture.compiled.scalar_equations),
            "decoded scalar equations differ for {example}"
        );
        assert_eq!(
            derived_equation_plan_artifact(&decoded.derived_equations),
            derived_equation_plan_artifact(&fixture.compiled.derived_equations),
            "decoded derived text transforms differ for {example}"
        );
        assert_eq!(
            list_equation_plan_artifact(&decoded.list_equations),
            list_equation_plan_artifact(&fixture.compiled.list_equations),
            "decoded list equations differ for {example}"
        );
        assert_eq!(
            list_projection_plan_artifact(&decoded.list_projections),
            list_projection_plan_artifact(&fixture.compiled.list_projections),
            "decoded list projections differ for {example}"
        );
        assert_eq!(
            list_source_binding_plan_artifact(&decoded.list_source_bindings),
            list_source_binding_plan_artifact(&fixture.compiled.list_source_bindings),
            "decoded list source bindings differ for {example}"
        );
    });

    let artifact_path = temp_root.join("todomvc.boonc");
    let artifact = CompiledArtifact::load_from_path(&artifact_path).unwrap();
    let decoded = artifact.runtime_non_route_tables().unwrap();
    let mut corrupt = artifact.body.clone();
    corrupt["runtime_symbol_count"] = json!(decoded.symbols.len() + 1);
    let error = RuntimeNonRouteTables::from_artifact_body(&corrupt)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("runtime_symbols"),
        "wrong symbol count should fail decoding, got {error}"
    );

    corrupt = artifact.body.clone();
    corrupt["runtime_plan"]["scalar_equations"]["branches"][0]["expression"]["kind"] =
        json!("not_a_scalar_expression");
    let error = RuntimeNonRouteTables::from_artifact_body(&corrupt)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("not_a_scalar_expression"),
        "wrong scalar expression kind should fail decoding, got {error}"
    );

    corrupt = artifact.body.clone();
    corrupt["runtime_plan"]["list_equations"]["operations"][0]["kind"]["kind"] =
        json!("not_a_list_operation");
    let error = RuntimeNonRouteTables::from_artifact_body(&corrupt)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("not_a_list_operation"),
        "wrong list operation kind should fail decoding, got {error}"
    );
}

// test: compiled_artifact_decodes_source_routes_and_action_table_without_ast
#[test]
fn compiled_artifact_decodes_source_routes_and_action_table_without_ast() {
    let temp_root = TestTempRoot::new("compiled-artifact-source-routes-test");

    for_core_compiled_artifacts(&temp_root, |example, fixture| {
        let decoded = fixture.artifact.runtime_source_routes().unwrap();
        assert_eq!(
            source_route_plan_artifact(&decoded),
            source_route_plan_artifact(&fixture.compiled.source_routes),
            "decoded source routes differ for {example}"
        );
    });

    let artifact_path = temp_root.join("todomvc.boonc");
    let artifact = CompiledArtifact::load_from_path(&artifact_path).unwrap();
    let route_slots = artifact.body["runtime_plan"]["source_routes"]["route_slots"]
        .as_array()
        .unwrap();
    let route_zero_source_id = route_slots[0]["source_id"].as_u64().unwrap() as usize;
    let route_zero_next = (route_zero_source_id + 1) % route_slots.len();

    let mut corrupt = artifact.body.clone();
    corrupt["runtime_plan"]["source_routes"]["route_slots"][0]["route_id"] = json!(999);
    let error = SourceRoutePlan::from_artifact_body(&corrupt)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("route_id"),
        "wrong route id should fail decoding, got {error}"
    );

    corrupt = artifact.body.clone();
    corrupt["runtime_plan"]["source_routes"]["id_slots"][route_zero_source_id] =
        json!(route_zero_next);
    let error = SourceRoutePlan::from_artifact_body(&corrupt)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("id_slots"),
        "wrong SourceId slot should fail decoding, got {error}"
    );

    corrupt = artifact.body.clone();
    corrupt["runtime_plan"]["source_routes"]["label_slots"][0]["source"] = json!("zzzz.not.sorted");
    let error = SourceRoutePlan::from_artifact_body(&corrupt)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("label_slots"),
        "wrong label slot should fail decoding, got {error}"
    );

    corrupt = artifact.body.clone();
    corrupt["runtime_plan"]["source_routes"]["action_table"][0]["source_id"] = json!(1);
    let error = SourceRoutePlan::from_artifact_body(&corrupt)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("action_table"),
        "wrong action table source id should fail decoding, got {error}"
    );

    corrupt = artifact.body.clone();
    corrupt["runtime_plan"]["source_routes"]["action_table"][route_zero_source_id]["actions"][0] =
        json!({"kind": "list_remove", "list": "todos"});
    let error = SourceRoutePlan::from_artifact_body(&corrupt)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("action_table"),
        "action table mismatch should fail decoding, got {error}"
    );

    corrupt = artifact.body.clone();
    corrupt["runtime_plan"]["source_routes"]["route_slots"][0]["root_scalar_targets"] = json!([]);
    let error = SourceRoutePlan::from_artifact_body(&corrupt)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("actions do not match"),
        "route action rebuild mismatch should fail decoding, got {error}"
    );

    corrupt = artifact.body.clone();
    corrupt["runtime_plan"]["source_routes"]["route_slots"][0]["actions"][0]["kind"] =
        json!("not_a_source_action");
    let error = SourceRoutePlan::from_artifact_body(&corrupt)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("not_a_source_action"),
        "bad source action kind should fail decoding, got {error}"
    );

    corrupt = artifact.body.clone();
    corrupt["runtime_plan"]["source_routes"]["route_slots"][0]["payload_fields"][0] =
        json!("PointerWidth");
    let error = SourceRoutePlan::from_artifact_body(&corrupt)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("payload field"),
        "bad payload field should fail decoding, got {error}"
    );

    let indexed_text_route = route_slots
        .iter()
        .position(|route| {
            route["actions"]
                .as_array()
                .unwrap()
                .iter()
                .any(|action| action["kind"] == "indexed_text")
        })
        .expect("TodoMVC artifact should contain an indexed text route");
    let indexed_text_action = route_slots[indexed_text_route]["actions"]
        .as_array()
        .unwrap()
        .iter()
        .position(|action| action["kind"] == "indexed_text")
        .unwrap();
    corrupt = artifact.body.clone();
    corrupt["runtime_plan"]["source_routes"]["route_slots"][indexed_text_route]["actions"]
        [indexed_text_action]["text_action"] = json!("not_an_indexed_text_action");
    let error = SourceRoutePlan::from_artifact_body(&corrupt)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("not_an_indexed_text_action"),
        "bad indexed text action should fail decoding, got {error}"
    );

    let indexed_bool_route = route_slots
        .iter()
        .position(|route| {
            route["actions"]
                .as_array()
                .unwrap()
                .iter()
                .any(|action| action["kind"] == "indexed_bool")
        })
        .expect("TodoMVC artifact should contain an indexed bool route");
    let indexed_bool_action = route_slots[indexed_bool_route]["actions"]
        .as_array()
        .unwrap()
        .iter()
        .position(|action| action["kind"] == "indexed_bool")
        .unwrap();
    corrupt = artifact.body.clone();
    corrupt["runtime_plan"]["source_routes"]["route_slots"][indexed_bool_route]["actions"]
        [indexed_bool_action]["bool_action"] = json!("not_an_indexed_bool_action");
    let error = SourceRoutePlan::from_artifact_body(&corrupt)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("not_an_indexed_bool_action"),
        "bad indexed bool action should fail decoding, got {error}"
    );

    corrupt = artifact.body.clone();
    corrupt["runtime_plan"]["source_routes"]["route_slots"][0]["root_scalar_targets"][0]["expression"]
        ["kind"] = json!("not_a_scalar_expression");
    let error = SourceRoutePlan::from_artifact_body(&corrupt)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("not_a_scalar_expression"),
        "bad scalar route expression should fail decoding, got {error}"
    );

    let list_remove_route = route_slots
        .iter()
        .position(|route| {
            route["list_remove_targets"]
                .as_array()
                .unwrap()
                .first()
                .is_some()
        })
        .expect("TodoMVC artifact should contain a list-remove route");
    corrupt = artifact.body.clone();
    corrupt["runtime_plan"]["source_routes"]["route_slots"][list_remove_route]["list_remove_targets"]
        [0]["predicate"]["kind"] = json!("not_a_list_predicate");
    let error = SourceRoutePlan::from_artifact_body(&corrupt)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("not_a_list_predicate"),
        "bad list-remove predicate should fail decoding, got {error}"
    );
}

// test: compiled_artifact_runs_single_file_scenario_after_source_is_deleted
#[test]
fn compiled_artifact_runs_single_file_scenario_after_source_is_deleted() {
    let temp_root = TestTempRoot::new("compiled-artifact-source-deleted");
    let source = temp_root.join("todomvc.bn");
    std::fs::copy("../../examples/todomvc.bn", &source).unwrap();
    let scenario = Path::new("../../examples/todomvc.scn");
    let artifact_path = temp_root.join("todomvc.boonc");

    emit_compiled_artifact(&source, &artifact_path, None).unwrap();
    let source_output =
        run_plan_scenario_events(&source, scenario, TargetProfile::SoftwareDefault, None).unwrap();
    std::fs::remove_file(&source).unwrap();
    let artifact_output = run_compiled_artifact_scenario(&artifact_path, scenario).unwrap();

    assert_eq!(
        source_output.report["semantic_deltas"],
        serde_json::to_value(&artifact_output.semantic_deltas).unwrap(),
        "deleted-source artifact scenario semantic deltas must match source PlanExecutor"
    );
    assert_eq!(
        source_output.state_summary, artifact_output.state_summary,
        "deleted-source artifact scenario final state must match source PlanExecutor"
    );
}

