// Included by `../tests.rs`; kept in the parent test module for private verifier-helper access.

#[test]
fn scenario_integrity_rejects_target_text_only_selector() {
    let mut action = BTreeMap::new();
    action.insert("kind".to_owned(), toml::Value::String("click".to_owned()));
    action.insert(
        "target_text".to_owned(),
        toml::Value::String("Duplicate".to_owned()),
    );
    let step = boon_runtime::ScenarioStep {
        id: "target-text-only".to_owned(),
        user_action: Some(action),
        ..Default::default()
    };

    assert!(!scenario_user_action_target_text_is_disambiguated(&step));
}


#[test]
fn scenario_integrity_accepts_target_text_with_control_selector() {
    let mut action = BTreeMap::new();
    action.insert("kind".to_owned(), toml::Value::String("click".to_owned()));
    action.insert("target".to_owned(), toml::Value::String("row".to_owned()));
    action.insert(
        "target_text".to_owned(),
        toml::Value::String("Duplicate".to_owned()),
    );
    let step = boon_runtime::ScenarioStep {
        id: "target-text-with-target".to_owned(),
        user_action: Some(action),
        ..Default::default()
    };

    assert!(scenario_user_action_target_text_is_disambiguated(&step));
}


#[test]
fn scenario_integrity_detects_authored_raw_coordinates() {
    let mut action = BTreeMap::new();
    action.insert("kind".to_owned(), toml::Value::String("click".to_owned()));
    action.insert(
        "target".to_owned(),
        toml::Value::String("canvas".to_owned()),
    );
    action.insert("pointer_x".to_owned(), toml::Value::String("42".to_owned()));
    let step = boon_runtime::ScenarioStep {
        id: "raw-coordinate".to_owned(),
        user_action: Some(action),
        ..Default::default()
    };

    assert_eq!(
        scenario_user_action_raw_coordinate_fields(&step),
        vec!["pointer_x".to_owned()]
    );
}


#[test]
fn scenario_integrity_requires_public_source_intent_for_actions() {
    let mut action = BTreeMap::new();
    action.insert("kind".to_owned(), toml::Value::String("click".to_owned()));
    action.insert(
        "target".to_owned(),
        toml::Value::String("button".to_owned()),
    );
    let missing = boon_runtime::ScenarioStep {
        id: "missing-source-intent".to_owned(),
        user_action: Some(action.clone()),
        ..Default::default()
    };
    assert!(!scenario_input_action_has_expected_source_intent(&missing));

    let mut expected = BTreeMap::new();
    expected.insert(
        "source".to_owned(),
        toml::Value::String("store.elements.button".to_owned()),
    );
    let present = boon_runtime::ScenarioStep {
        id: "has-source-intent".to_owned(),
        user_action: Some(action),
        expected_source_event: Some(expected),
        ..Default::default()
    };
    assert!(scenario_input_action_has_expected_source_intent(&present));
}


#[test]
fn scenario_integrity_rejects_row_action_without_identity_or_selector() {
    let mut action = BTreeMap::new();
    action.insert("kind".to_owned(), toml::Value::String("click".to_owned()));
    action.insert(
        "target_text".to_owned(),
        toml::Value::String("Duplicate todo".to_owned()),
    );
    let mut expected = BTreeMap::new();
    expected.insert(
        "source".to_owned(),
        toml::Value::String("todo.sources.todo_checkbox.click".to_owned()),
    );
    expected.insert(
        "target_text".to_owned(),
        toml::Value::String("Duplicate todo".to_owned()),
    );
    let step = boon_runtime::ScenarioStep {
        id: "row-without-identity-or-selector".to_owned(),
        user_action: Some(action),
        expected_source_event: Some(expected),
        ..Default::default()
    };

    assert!(!scenario_row_action_has_public_identity_or_justified_selector(&step));
}


#[test]
fn scenario_integrity_accepts_row_action_with_public_identity() {
    let mut action = BTreeMap::new();
    action.insert("kind".to_owned(), toml::Value::String("click".to_owned()));
    action.insert(
        "target_text".to_owned(),
        toml::Value::String("Duplicate todo".to_owned()),
    );
    action.insert("target_key".to_owned(), toml::Value::Integer(42));
    action.insert("target_generation".to_owned(), toml::Value::Integer(1));
    let mut expected = BTreeMap::new();
    expected.insert(
        "source".to_owned(),
        toml::Value::String("todo.sources.todo_checkbox.click".to_owned()),
    );
    let step = boon_runtime::ScenarioStep {
        id: "row-with-public-identity".to_owned(),
        user_action: Some(action),
        expected_source_event: Some(expected),
        ..Default::default()
    };

    assert!(scenario_row_action_has_public_identity_or_justified_selector(&step));
}


#[test]
fn scenario_integrity_accepts_row_action_with_justified_selector() {
    let mut action = BTreeMap::new();
    action.insert("kind".to_owned(), toml::Value::String("click".to_owned()));
    action.insert(
        "target".to_owned(),
        toml::Value::String("todo checkbox".to_owned()),
    );
    action.insert(
        "target_text".to_owned(),
        toml::Value::String("Duplicate todo".to_owned()),
    );
    let mut expected = BTreeMap::new();
    expected.insert(
        "source".to_owned(),
        toml::Value::String("todo.sources.todo_checkbox.click".to_owned()),
    );
    let step = boon_runtime::ScenarioStep {
        id: "row-with-selector".to_owned(),
        user_action: Some(action),
        expected_source_event: Some(expected),
        ..Default::default()
    };

    assert!(scenario_row_action_has_public_identity_or_justified_selector(&step));
}


#[test]
fn scenario_integrity_accepts_documented_non_source_action_exemption() {
    let mut action = BTreeMap::new();
    action.insert(
        "kind".to_owned(),
        toml::Value::String("pointer_hover".to_owned()),
    );
    action.insert(
        "target".to_owned(),
        toml::Value::String("delete button".to_owned()),
    );
    let step = boon_runtime::ScenarioStep {
        id: "hover-without-source-event".to_owned(),
        user_action: Some(action),
        source_intent_exemption: Some(
            "hover only exposes an affordance; click routes the source event".to_owned(),
        ),
        ..Default::default()
    };

    assert!(scenario_input_action_has_expected_source_intent(&step));
}


#[test]
fn scenario_integrity_provenance_allows_generated_or_phased_refs() {
    let generated = boon_runtime::ScenarioRefProvenance {
        id: "vertical-wheel-scroll".to_owned(),
        phases: vec!["scroll_focus".to_owned()],
        provenance: "native generated scroll probe".to_owned(),
        generated_probe: true,
    };
    let phased = boon_runtime::ScenarioRefProvenance {
        id: "zoom-in".to_owned(),
        phases: vec!["input".to_owned(), "scroll_focus".to_owned()],
        provenance: "same authored step feeds both phases".to_owned(),
        generated_probe: false,
    };
    let unphased = boon_runtime::ScenarioRefProvenance {
        id: "ambiguous".to_owned(),
        phases: vec!["input".to_owned()],
        provenance: "single phase is not enough for duplicate refs".to_owned(),
        generated_probe: false,
    };

    assert!(scenario_ref_provenance_allows_duplicate(&generated));
    assert!(scenario_ref_provenance_allows_duplicate(&phased));
    assert!(!scenario_ref_provenance_allows_duplicate(&unphased));
}
