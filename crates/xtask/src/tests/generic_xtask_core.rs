// Included by `../tests.rs`; kept in the parent test module for private verifier-helper access.

#[test]
fn advertised_xtask_commands_are_unique() {
    let mut seen = BTreeSet::new();
    for command in XTASK_COMMANDS {
        assert!(
            seen.insert(command.0),
            "duplicate xtask command `{}`",
            command.0
        );
    }
}


#[test]
fn product_render_graph_cells_sample_count_reads_json_sidecar_ref() {
    let dir = std::env::temp_dir().join(format!(
        "boon-xtask-product-graph-sidecar-{}-{}",
        std::process::id(),
        monotonic_now_ns().unwrap_or(0)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let sidecar = dir.join("click_samples.json");
    write_json(
        &sidecar,
        &json!([
            {
                "product_frame_commit": {
                    "render_graph": {
                        "encode_time_ms": 0.25
                    }
                }
            },
            {
                "product_frame_commit": {
                    "render_graph": {
                        "encode_time_ms": 0.5
                    }
                }
            }
        ]),
    )
    .unwrap();
    let report = json!({
        "click_samples": {
            "sidecar": true,
            "kind": "json-sidecar-ref",
            "json_pointer_replaced": "/click_samples",
            "path": sidecar.display().to_string(),
            "sha256": cached_sha256_file(&sidecar).unwrap(),
            "byte_len": fs::metadata(&sidecar).unwrap().len()
        }
    });
    assert_eq!(
        native_product_render_graph_cells_encode_time_sample_count(&report),
        2
    );
    std::fs::remove_dir_all(&dir).unwrap();
}


#[test]
fn product_status_uses_top_level_status_only() {
    let report = json!({
        "status": "pass",
        "plan_executor_status": "fail"
    });
    assert!(report_status_pass(&report));

    let failed_product = json!({
        "status": "fail",
        "plan_executor_status": "pass"
    });
    assert!(!report_status_pass(&failed_product));
    assert!(report_status_pass(&json!({"status": "pass"})));
}
