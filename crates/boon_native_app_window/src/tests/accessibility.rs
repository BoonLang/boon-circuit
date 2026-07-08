// Included by `../tests.rs`; kept in the parent test module for private app-window helper access.

#[test]
fn accessibility_action_requests_lower_without_leaking_accesskit_types() {
    let requests = native_accessibility_action_requests_from_accesskit(vec![
        accesskit::ActionRequest {
            action: accesskit::Action::Focus,
            target_tree: accesskit::TreeId::ROOT,
            target_node: accesskit::NodeId(41),
            data: None,
        },
        accesskit::ActionRequest {
            action: accesskit::Action::SetValue,
            target_tree: accesskit::TreeId::ROOT,
            target_node: accesskit::NodeId(42),
            data: Some(accesskit::ActionData::Value("hello".into())),
        },
        accesskit::ActionRequest {
            action: accesskit::Action::ScrollLeft,
            target_tree: accesskit::TreeId::ROOT,
            target_node: accesskit::NodeId(43),
            data: None,
        },
    ]);

    assert_eq!(requests.len(), 3);
    assert_eq!(requests[0].target_node_id, 41);
    assert_eq!(requests[0].action, NativeAccessibilityAction::Focus);
    assert_eq!(requests[1].action, NativeAccessibilityAction::SetValue);
    assert_eq!(requests[1].value.as_deref(), Some("hello"));
    assert_eq!(
        requests[2].action,
        NativeAccessibilityAction::Other("ScrollLeft".to_owned())
    );
}
