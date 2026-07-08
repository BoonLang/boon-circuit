// Included by `../tests.rs`; kept in the parent test module for private invariant access.

#[test]
fn playground_surface_schema_requires_visible_manual_test_controls() {
    let mut report = playground_surface_fixture();
    verify_playground_surface_report(&report, Path::new("memory:playground")).unwrap();

    report["playground_surface"]["code_editor"] = json!(false);
    assert!(
        verify_playground_surface_report(&report, Path::new("memory:playground"))
            .unwrap_err()
            .to_string()
            .contains("code_editor")
    );

    let mut zero_bounds = playground_surface_fixture();
    zero_bounds["playground_surface_visible_bounds"]["semantic_delta_log"]["elements"][0]["bounds"]
        ["width"] = json!(0.0);
    assert!(
        verify_playground_surface_report(&zero_bounds, Path::new("memory:playground"))
            .unwrap_err()
            .to_string()
            .contains("semantic_delta_log")
    );
}

fn playground_surface_fixture() -> JsonValue {
    let mut surface = serde_json::Map::new();
    let mut bounds = serde_json::Map::new();
    for key in [
        "example_selector",
        "code_editor",
        "run_reset_step_controls",
        "render_preview",
        "semantic_delta_log",
        "selected_value_inspector",
        "dependency_explanation_panel",
    ] {
        surface.insert(key.to_owned(), json!(true));
        bounds.insert(
            key.to_owned(),
            json!({
                "pass": true,
                "elements": [{
                    "element_id": format!("{key}_fixture"),
                    "visible": true,
                    "bounds": {"x": 1.0, "y": 1.0, "width": 10.0, "height": 10.0}
                }]
            }),
        );
    }
    json!({
        "playground_surface": surface,
        "playground_surface_visible_bounds": bounds
    })
}


#[test]
fn scalar_match_value_reports_bad_nested_output_instead_of_skip() {
    let equations = ScalarEquationPlan {
        source_paths: vec!["store.elements.keyboard_capture".to_owned()],
        branches: vec![ScalarUpdateBranch {
            target: "store.zoom_step".to_owned(),
            source: "store.elements.keyboard_capture".to_owned(),
            guard: None,
            expression: ScalarUpdateExpression::MatchValueConst {
                input: "elements.keyboard_capture.key".to_owned(),
                arms: vec![
                    UpdateValueMatchArm {
                        pattern: "W".to_owned(),
                        output: UpdateValueExpression::NumberInfix {
                            left: "store.zoom_step".to_owned(),
                            op: "*".to_owned(),
                            right: "2".to_owned(),
                        },
                    },
                    UpdateValueMatchArm {
                        pattern: "__".to_owned(),
                        output: UpdateValueExpression::Const {
                            value: "SKIP".to_owned(),
                        },
                    },
                ],
            },
        }],
    };
    let error = equations
        .eval_text(
            "store.zoom_step",
            "store.elements.keyboard_capture",
            Some("W"),
            None,
            None,
            &BTreeMap::new(),
            None,
            None,
            None,
            None,
            |path| (path == "store.zoom_step").then(|| "1".to_owned()),
            |_| None,
        )
        .expect_err("unsupported nested update operator should not silently skip");
    let error = error.to_string();
    assert!(
        error.contains("cannot evaluate update value numeric infix `store.zoom_step * 2`"),
        "unexpected runtime error: {error}"
    );
}
