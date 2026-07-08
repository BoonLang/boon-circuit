// Included by `../tests.rs`; kept in the parent test module for private invariant access.

#[test]
fn root_read_key_aliases_match_store_local_without_nested_leaf_collision() {
    let keys = root_read_keys_for_path("store.group.value");
    let fields = keys
        .iter()
        .filter_map(|key| match key {
            GenericReadKey::Root { field } => Some(field.as_str()),
            _ => None,
        })
        .collect::<BTreeSet<_>>();
    assert_eq!(fields, BTreeSet::from(["store.group.value", "group.value"]));
    assert!(keys.contains(&GenericReadKey::RootChild {
        root: "store.group".to_owned(),
        path: "value".to_owned(),
    }));
    assert!(
        keys.iter()
            .any(|key| root_read_key_matches_path(key, "store.group.value"))
    );
    assert!(root_read_key_matches_path(
        &GenericReadKey::Root {
            field: "group.value".to_owned(),
        },
        "store.group.value"
    ));
    assert!(
        !root_read_key_matches_path(
            &GenericReadKey::Root {
                field: "value".to_owned(),
            },
            "store.group.value"
        ),
        "a nested store root must not publish the bare leaf alias because it can collide with a real top-level store root"
    );

    let nested_cursor = root_read_keys_for_path("store.page_ref.cursor")
        .into_iter()
        .collect::<BTreeSet<_>>();
    assert!(
        !nested_cursor.contains(&GenericReadKey::Root {
            field: "cursor".to_owned()
        }),
        "store.page_ref.cursor must not collide with store.cursor dependents"
    );
    assert!(
        !root_read_key_matches_path(
            &GenericReadKey::Root {
                field: "cursor".to_owned(),
            },
            "store.page_ref.cursor"
        ),
        "matching must follow the same no-nested-leaf dependency contract"
    );
    assert!(root_read_key_matches_path(
        &GenericReadKey::Root {
            field: "value".to_owned(),
        },
        "store.value"
    ));

    let deduped = root_read_keys_for_path("store.value")
        .into_iter()
        .collect::<BTreeSet<_>>();
    assert_eq!(
        deduped.len(),
        root_read_keys_for_path("store.value").len(),
        "store-local and leaf aliases that are equal must not be emitted twice"
    );
}


#[test]
fn text_match_patterns_rejoin_pathlike_punctuation_without_spaces() {
    let tokens = |parts: &[&str]| {
        parts
            .iter()
            .map(|part| (*part).to_owned())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        text_match_pattern_value(&tokens(&["simple_test", ".", "ghw"])),
        "simple_test.ghw"
    );
    assert_eq!(
        text_match_pattern_value(&tokens(&["-", "simple", ".", "vcd"])),
        "- simple.vcd"
    );
    assert_eq!(
        text_match_pattern_value(&tokens(&["simple_tb", ".", "s", "group"])),
        "simple_tb.s group"
    );
    assert_eq!(
        text_match_pattern_value(&tokens(&["https", ":", "/", "/", "kavik", ".", "cz", "/"])),
        "https://kavik.cz/"
    );
    assert_eq!(
        text_match_pattern_value(&tokens(&["data_bus", "[", "7", ":", "0", "]"])),
        "data_bus[7:0]"
    );
}


#[test]
fn runtime_field_slot_collision_diagnostics_preserve_readable_labels() {
    let diagnostics = field_slot_collision_diagnostics_from_names([
        "field_collision_1469".to_owned(),
        "field_collision_2806".to_owned(),
        "title".to_owned(),
    ]);

    assert_eq!(diagnostics.len(), 1);
    assert_eq!(
        diagnostics[0].field_id,
        stable_runtime_field_id("field_collision_1469")
    );
    assert_eq!(
        diagnostics[0].labels,
        vec![
            "field_collision_1469".to_owned(),
            "field_collision_2806".to_owned()
        ]
    );
}


#[test]
fn scenario_delta_expectations_reject_missing_semantic_or_render_patch() {
    let semantic_step = ScenarioStep {
        id: "missing-semantic".to_owned(),
        expect_semantic_delta_contains: vec!["ListInsert".to_owned()],
        ..ScenarioStep::default()
    };
    let render_step = ScenarioStep {
        id: "missing-render".to_owned(),
        expect_render_delta_contains: vec!["BindSource".to_owned()],
        ..ScenarioStep::default()
    };
    let deltas = [field_delta(
        Some(1),
        Some(1),
        "completed",
        ProtocolValue::Bool(true),
    )];
    let patches = [patch(
        "InvalidateDocument",
        RenderTarget::Static(Cow::Borrowed("document")),
        ProtocolValue::CheckedProperty(true),
    )];

    assert!(assert_delta_expectations(&semantic_step, &deltas, &patches).is_err());
    assert!(assert_delta_expectations(&render_step, &deltas, &patches).is_err());
}


#[test]
fn scalar_match_const_uses_default_arm_for_unmatched_values() {
    let equations = ScalarEquationPlan {
        source_paths: Vec::new(),
        branches: vec![ScalarUpdateBranch {
            target: "store.active_signal".to_owned(),
            source: "store.elements.select_data".to_owned(),
            guard: None,
            expression: ScalarUpdateExpression::MatchConst {
                input: "store.active_signal".to_owned(),
                arms: vec![
                    UpdateMatchArm {
                        pattern: "data_bus".to_owned(),
                        output: "none".to_owned(),
                    },
                    UpdateMatchArm {
                        pattern: "__".to_owned(),
                        output: "data_bus".to_owned(),
                    },
                ],
            },
        }],
    };
    let value = equations
        .eval_text(
            "store.active_signal",
            "store.elements.select_data",
            None,
            None,
            None,
            &BTreeMap::new(),
            None,
            None,
            None,
            None,
            |path| (path == "store.active_signal").then(|| "temperature".to_owned()),
            |_| None,
        )
        .unwrap();
    assert_eq!(value, Some(Cow::Owned("data_bus".to_owned())));
}
