// Included by `../tests.rs`; kept in the parent test module for private verifier-helper access.

#[test]
fn stale_path_ledger_rejects_product_forbidden_proof() {
    let dir = PathBuf::from(format!(
        "target/tmp/xtask-stale-path-ledger-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("create stale-path test dir");
    let linked_report = dir.join("preview-loop.json");
    let base_report = dir.join("cells-visible-click.json");
    let ledger = dir.join("ledger.json");
    let output = dir.join("out.json");
    write_json(
        &linked_report,
        &json!({
            "product_proof_built_pre_present": true
        }),
    )
    .expect("write linked report");
    write_json(
        &base_report,
        &json!({
            "preview_loop_report": linked_report
        }),
    )
    .expect("write base report");
    write_json(
        &ledger,
        &json!({
            "schema_version": 1,
            "rows": [{
                "id": "reject-product-forbidden-proof",
                "mode": "product-forbidden",
                "current_owner": "test",
                "typed_replacement": "post-present proof queue",
                "report_path": base_report,
                "linked_report_path_pointer": "/preview_loop_report",
                "symbol_or_field": "product_proof_built_pre_present",
                "json_pointer": "/product_proof_built_pre_present",
                "expected": false,
                "positive_gate": "test positive gate",
                "negative_gate": "pre-present proof coupling must be false",
                "removal_condition": "test removal"
            }]
        }),
    )
    .expect("write ledger");

    let result = verify_native_gpu_stale_path_ledger(&[
        "verify-native-gpu-stale-path-ledger".to_owned(),
        "--ledger".to_owned(),
        ledger.display().to_string(),
        "--report".to_owned(),
        output.display().to_string(),
    ]);

    assert!(
        result.is_err(),
        "stale path ledger must reject product-forbidden pre-present proof rows"
    );
    let report = read_json(&output).expect("read failing stale-path report");
    assert_eq!(
        report.get("status").and_then(serde_json::Value::as_str),
        Some("fail")
    );
    assert!(
        report
            .get("blockers")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|blockers| blockers.iter().any(|blocker| blocker
                .as_str()
                .is_some_and(|text| text.contains("product_proof_built_pre_present")))),
        "failing report should name the stale pre-present proof field: {report}"
    );
}


#[test]
fn stale_path_ledger_accepts_removed_product_forbidden_field() {
    let dir = PathBuf::from(format!(
        "target/tmp/xtask-stale-path-ledger-removed-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("create stale-path removed test dir");
    let base_report = dir.join("cells-visible-click.json");
    let ledger = dir.join("ledger.json");
    let output = dir.join("out.json");
    write_json(
        &base_report,
        &json!({
            "status": "pass"
        }),
    )
    .expect("write base report");
    write_json(
        &ledger,
        &json!({
            "schema_version": 1,
            "rows": [{
                "id": "removed-product-forbidden-proof",
                "mode": "product-forbidden",
                "current_owner": "test",
                "typed_replacement": "post-present proof queue",
                "report_path": base_report,
                "linked_report_path_pointer": "/preview_loop_report",
                "symbol_or_field": "product_proof_built_pre_present",
                "json_pointer": "/product_proof_built_pre_present",
                "expected": false,
                "positive_gate": "test positive gate",
                "negative_gate": "pre-present proof coupling must be false or absent",
                "removal_condition": "test removal"
            }]
        }),
    )
    .expect("write ledger");

    verify_native_gpu_stale_path_ledger(&[
        "verify-native-gpu-stale-path-ledger".to_owned(),
        "--ledger".to_owned(),
        ledger.display().to_string(),
        "--report".to_owned(),
        output.display().to_string(),
    ])
    .expect("removed product-forbidden stale path should pass");
    let report = read_json(&output).expect("read passing stale-path report");
    assert_eq!(
        report.get("status").and_then(serde_json::Value::as_str),
        Some("pass")
    );
    assert_eq!(
        report
            .get("product_forbidden_absent_pass_count")
            .and_then(serde_json::Value::as_u64),
        Some(1)
    );
    assert_eq!(
        report
            .pointer("/row_results/0/absent_counts_as_pass")
            .and_then(serde_json::Value::as_bool),
        Some(true)
    );
}


#[test]
fn stale_path_ledger_rejects_non_product_forbidden_modes() {
    let dir = PathBuf::from(format!(
        "target/tmp/xtask-stale-path-ledger-mode-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("create stale-path mode test dir");
    let base_report = dir.join("cells-visible-click.json");
    let ledger = dir.join("ledger.json");
    let output = dir.join("out.json");
    write_json(
        &base_report,
        &json!({
            "status": "pass",
            "some_counter": 0
        }),
    )
    .expect("write base report");
    write_json(
        &ledger,
        &json!({
            "schema_version": 1,
            "rows": [{
                "id": "obsolete-diagnostic-mode",
                "mode": "diagnostic-only",
                "current_owner": "test",
                "typed_replacement": "product forbidden row",
                "report_path": base_report,
                "symbol_or_field": "some_counter",
                "json_pointer": "/some_counter",
                "expected": 0,
                "positive_gate": "test positive gate",
                "negative_gate": "test negative gate",
                "removal_condition": "test removal"
            }]
        }),
    )
    .expect("write ledger");

    let result = verify_native_gpu_stale_path_ledger(&[
        "verify-native-gpu-stale-path-ledger".to_owned(),
        "--ledger".to_owned(),
        ledger.display().to_string(),
        "--report".to_owned(),
        output.display().to_string(),
    ]);

    assert!(
        result.is_err(),
        "stale path ledger must reject obsolete non-product modes"
    );
    let report = read_json(&output).expect("read failing stale-path mode report");
    assert_eq!(
        report.get("status").and_then(serde_json::Value::as_str),
        Some("fail")
    );
    assert!(
        report
            .get("blockers")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|blockers| blockers.iter().any(|blocker| blocker
                .as_str()
                .is_some_and(|text| text.contains("product-forbidden")))),
        "failing report should name the only accepted mode: {report}"
    );
}


#[test]
fn native_idle_wake_target_helpers_accept_wrapped_press_intents() {
    let layout = json!({
        "layout_proof": {
            "source_intent_assertions": [
                {"node": "decrement", "intent": "decrement_button", "source_path": "store.sources.decrement_button"},
                {"node": "decrement", "intent": "press", "source_path": "store.sources.decrement_button.press"},
                {"node": "decrement", "intent": "target", "source_path": "-"},
                {"node": "increment", "intent": "increment_button", "source_path": "store.sources.increment_button"},
                {"node": "increment", "intent": "press", "source_path": "store.sources.increment_button.press"},
                {"node": "increment", "intent": "target", "source_path": "+"}
            ],
            "hit_target_assertions": [
                {"id": "hit:decrement", "node": "decrement", "bounds": {"x": 306.0, "y": 166.0, "width": 96.0, "height": 40.0}},
                {"id": "hit:increment", "node": "increment", "bounds": {"x": 518.0, "y": 166.0, "width": 96.0, "height": 40.0}}
            ]
        }
    });
    let scenario_path = PathBuf::from("target/tmp/xtask-native-idle-wake-counter-test.scn");
    std::fs::create_dir_all(
        scenario_path
            .parent()
            .expect("scenario temp path should have a parent"),
    )
    .expect("create scenario temp dir");
    std::fs::write(
        &scenario_path,
        r#"
name = "counter-test"
source = "examples/counter.bn"

[[step]]
id = "press-increment"
expected_source_event = { source = "store.sources.increment_button.press", target_text = "+" }
"#,
    )
    .expect("write scenario");

    let target = native_preview_driver_target_from_scenario(&layout, &scenario_path)
        .or_else(|| native_preview_driver_target("counter", &layout))
        .expect("wrapped Counter layout should expose a source-bound hit target");
    assert_eq!(
        target.get("node").and_then(serde_json::Value::as_str),
        Some("increment")
    );
}
