// Included by `../tests.rs`; kept in the parent test module for private schema helper access.

#[test]
fn common_report_shape_binds_existing_command_binary_hash() {
    let command_path = temp_report_path("command-binary-ok");
    fs::write(&command_path, b"test command binary").unwrap();
    assert!(command_path.is_file());
    let mut report = base_report();
    report["command_argv"] = json!([command_path.display().to_string()]);
    report["binary_hash"] = json!(sha256_file(&command_path).unwrap());

    verify_common_report_shape(&report, &temp_report_path("current-binary-common")).unwrap();
    assert!(schema_accepts(report, "current-binary-hash"));
    let _ = fs::remove_file(command_path);
}

#[test]
fn common_report_shape_accepts_native_scoped_verifier_identity_with_stale_binary_hash() {
    let command_path = temp_report_path("native-command-binary-scoped-ok");
    fs::write(&command_path, b"test native command binary").unwrap();
    assert!(command_path.is_file());
    let args = vec![
        command_path.display().to_string(),
        "verify-native-gpu-preview-e2e".to_owned(),
        "--example".to_owned(),
        "cells".to_owned(),
        "--report".to_owned(),
        "ignored-report-path.json".to_owned(),
    ];
    let mut report = base_report();
    report["command"] = json!("verify-native-gpu-preview-e2e");
    report["command_argv"] = json!(args);
    report["binary_path"] = json!(command_path.display().to_string());
    report["binary_hash"] =
        json!("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
    report["verifier_identity"] = verifier_identity_for_command_args(
        "verify-native-gpu-preview-e2e",
        "proof",
        report
            .get("command_argv")
            .and_then(JsonValue::as_array)
            .unwrap()
            .iter()
            .map(|arg| arg.as_str().unwrap().to_owned())
            .collect::<Vec<_>>()
            .as_slice(),
    )
    .unwrap();

    verify_common_report_shape(&report, &temp_report_path("native-scoped-common")).unwrap();
    assert!(schema_accepts(report, "native-scoped-stale-binary"));
    let _ = fs::remove_file(command_path);
}


#[test]
fn common_report_shape_rejects_stale_native_scoped_verifier_identity() {
    let command_path = temp_report_path("native-command-binary-scoped-stale");
    fs::write(&command_path, b"test native command binary").unwrap();
    assert!(command_path.is_file());
    let args = vec![
        command_path.display().to_string(),
        "verify-native-gpu-preview-e2e".to_owned(),
        "--example".to_owned(),
        "cells".to_owned(),
    ];
    let stale_identity_args = vec![
        command_path.display().to_string(),
        "verify-native-gpu-preview-e2e".to_owned(),
        "--example".to_owned(),
        "todomvc".to_owned(),
    ];
    let mut report = base_report();
    report["command"] = json!("verify-native-gpu-preview-e2e");
    report["command_argv"] = json!(args);
    report["binary_path"] = json!(command_path.display().to_string());
    report["binary_hash"] = json!(sha256_file(&command_path).unwrap());
    report["verifier_identity"] = verifier_identity_for_command_args(
        "verify-native-gpu-preview-e2e",
        "proof",
        &stale_identity_args,
    )
    .unwrap();

    assert!(
        verify_common_report_shape(&report, &temp_report_path("native-scoped-stale-common"))
            .is_err()
    );
    assert!(!schema_accepts(report, "native-scoped-stale-identity"));
    let _ = fs::remove_file(command_path);
}


#[test]
fn bytecode_report_requires_parity_and_explicit_readiness() {
    let source_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/counter.bn")
        .canonicalize()
        .unwrap();
    let scenario_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/counter.scn")
        .canonicalize()
        .unwrap();
    let mut report = json!({
        "status": "pass",
        "report_version": 1,
        "command": "verify-bytecode",
        "command_argv": ["cargo", "xtask", "verify-bytecode", "counter"],
        "measurement_mode": "proof",
        "exit_status": 0,
        "generated_at_utc": SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
            .to_string(),
        "git_commit": "test",
        "binary_hash": "test",
        "source_path": source_path.display().to_string(),
        "source_hash": sha256_file(&source_path).unwrap(),
        "scenario_path": scenario_path.display().to_string(),
        "scenario_hash": sha256_file(&scenario_path).unwrap(),
        "program_hash": "program",
        "budget_hash": "n/a",
        "graph_node_count": 1,
        "semantic_index": {},
        "compiled_schedule": {},
        "per_step_pass_fail": [{
            "id": "expression-bytecode-interpreter-parity",
            "pass": true
        }],
        "artifact_sha256s": [],
        "expression_bytecode": {
            "version": 1,
            "execution_surface": "scalar_source_route_expressions",
            "interpreter_oracle": "ScalarEquationPlan",
            "candidate_expression_count": 3,
            "compiled_expression_count": 3,
            "parity_sample_count": 3,
            "parity_passed": true,
            "fallback_count": 0,
            "fallback_reasons": [],
            "deopt_count": 0,
            "deopt_reasons": [],
            "op_histogram": {"number_infix": 2, "const_text": 1},
            "warm_path_allocation_count": 2,
            "hot_path_ready": true,
            "samples": [{"pass": true}]
        }
    });
    assert!(schema_accepts(report.clone(), "bytecode-valid"));

    report["expression_bytecode"]["parity_passed"] = json!(false);
    assert!(!schema_accepts(report.clone(), "bytecode-fake-parity"));

    report["expression_bytecode"]["parity_passed"] = json!(true);
    report["expression_bytecode"]["fallback_count"] = json!(1);
    report["expression_bytecode"]["fallback_reasons"] = json!(["unsupported"]);
    assert!(!schema_accepts(
        report.clone(),
        "bytecode-hot-ready-with-fallback"
    ));

    report["expression_bytecode"]["hot_path_ready"] = json!(false);
    assert!(schema_accepts(report, "bytecode-proof-only-fallback"));
}
