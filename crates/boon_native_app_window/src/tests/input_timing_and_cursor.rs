// Included by `../tests.rs`; kept in the parent test module for private app-window helper access.

#[test]
fn unaccounted_host_input_frame_is_not_pre_present_drop_eligible() {
    let mut state = NativeRenderLoopState::new(NativeRenderLoopMode::DemandDriven);
    state.note_accepted_host_input(5, 20.0, false, None);
    state.note_present_completed(28.0);

    assert!(state.current_frame_carries_unaccounted_host_input(5));
    assert!(!state.current_frame_carries_unaccounted_host_input(4));

    assert!(state.take_frame_accepted_input_timing(Some(1)).is_some());
    assert!(!state.current_frame_carries_unaccounted_host_input(5));
}


#[test]
fn accessibility_action_requests_route_to_semantic_source_dispatch() {
    let root_id = boon_host::SemanticId("semantic:counter:root".to_owned());
    let increment_id = boon_host::SemanticId("semantic:counter:increment".to_owned());
    let mut scene = boon_host::SemanticScene {
        root: Some(root_id.clone()),
        focused: Some(increment_id.clone()),
        ..boon_host::SemanticScene::default()
    };
    scene.nodes.insert(
        root_id.clone(),
        boon_host::SemanticNode {
            id: root_id.clone(),
            node: boon_host::DocumentNodeId("counter:root".to_owned()),
            role: boon_host::SemanticRole::Application,
            name: Some("Counter".to_owned()),
            description: None,
            value: None,
            state: boon_host::SemanticState::default(),
            actions: boon_host::SemanticActions::default(),
            relations: boon_host::SemanticRelations {
                children: vec![increment_id.clone()],
                ..boon_host::SemanticRelations::default()
            },
            bounds: None,
            language: None,
            heading_level: None,
            href: None,
            source_binding_id: None,
            source_path: None,
            source_intent: None,
        },
    );
    scene.nodes.insert(
        increment_id.clone(),
        boon_host::SemanticNode {
            id: increment_id.clone(),
            node: boon_host::DocumentNodeId("counter:increment".to_owned()),
            role: boon_host::SemanticRole::Button,
            name: Some("Increment".to_owned()),
            description: None,
            value: None,
            state: boon_host::SemanticState {
                focused: true,
                ..boon_host::SemanticState::default()
            },
            actions: boon_host::SemanticActions {
                focus: true,
                press: true,
                set_text: false,
                increment: false,
                decrement: false,
            },
            relations: boon_host::SemanticRelations {
                parent: Some(root_id),
                ..boon_host::SemanticRelations::default()
            },
            bounds: None,
            language: None,
            heading_level: None,
            href: None,
            source_binding_id: Some(boon_host::SourceBindingId(
                "source:store.increment".to_owned(),
            )),
            source_path: Some("store.increment".to_owned()),
            source_intent: Some("press".to_owned()),
        },
    );
    let snapshot = accesskit_tree_update_from_semantic_scene(&scene, "boon-native", "test-version");
    let increment_node_id = snapshot
        .semantic_node_ids
        .iter()
        .find(|mapping| mapping.semantic_id == increment_id.0)
        .expect("increment semantic node should map to AccessKit")
        .accesskit_node_id;
    let requests =
        native_accessibility_action_requests_from_accesskit(vec![accesskit::ActionRequest {
            action: accesskit::Action::Click,
            target_tree: accesskit::TreeId::ROOT,
            target_node: accesskit::NodeId(increment_node_id),
            data: None,
        }]);

    let dispatches = native_accessibility_source_dispatches_from_requests(&scene, &requests);

    assert_eq!(dispatches.len(), 1);
    assert_eq!(dispatches[0].semantic_id, increment_id);
    assert_eq!(dispatches[0].source_path, "store.increment");
    assert_eq!(dispatches[0].source_intent.as_deref(), Some("press"));
    assert_eq!(dispatches[0].text, None);
}


#[test]
fn low_latency_present_mode_prefers_nonblocking_present_modes() {
    let mut capabilities = wgpu::SurfaceCapabilities {
        formats: vec![wgpu::TextureFormat::Bgra8UnormSrgb],
        present_modes: vec![
            wgpu::PresentMode::Fifo,
            wgpu::PresentMode::Immediate,
            wgpu::PresentMode::Mailbox,
        ],
        alpha_modes: vec![wgpu::CompositeAlphaMode::Opaque],
        usages: wgpu::TextureUsages::RENDER_ATTACHMENT,
    };

    assert_eq!(
        low_latency_present_mode(&capabilities),
        wgpu::PresentMode::Immediate
    );

    capabilities.present_modes = vec![
        wgpu::PresentMode::Fifo,
        wgpu::PresentMode::AutoNoVsync,
        wgpu::PresentMode::Mailbox,
    ];
    assert_eq!(
        low_latency_present_mode(&capabilities),
        wgpu::PresentMode::AutoNoVsync
    );

    capabilities.present_modes = vec![wgpu::PresentMode::Fifo, wgpu::PresentMode::Mailbox];
    assert_eq!(
        low_latency_present_mode(&capabilities),
        wgpu::PresentMode::Mailbox
    );

    capabilities.present_modes = vec![wgpu::PresentMode::Fifo, wgpu::PresentMode::AutoNoVsync];
    assert_eq!(
        low_latency_present_mode(&capabilities),
        wgpu::PresentMode::AutoNoVsync
    );

    capabilities.present_modes = vec![wgpu::PresentMode::Fifo, wgpu::PresentMode::Immediate];
    assert_eq!(
        low_latency_present_mode(&capabilities),
        wgpu::PresentMode::Immediate
    );

    capabilities.present_modes = vec![wgpu::PresentMode::Fifo];
    assert_eq!(
        low_latency_present_mode(&capabilities),
        wgpu::PresentMode::Fifo
    );
}


#[test]
fn configured_present_mode_honors_supported_override() {
    let capabilities = wgpu::SurfaceCapabilities {
        formats: vec![wgpu::TextureFormat::Bgra8UnormSrgb],
        present_modes: vec![
            wgpu::PresentMode::Fifo,
            wgpu::PresentMode::Immediate,
            wgpu::PresentMode::Mailbox,
        ],
        alpha_modes: vec![wgpu::CompositeAlphaMode::Opaque],
        usages: wgpu::TextureUsages::RENDER_ATTACHMENT,
    };

    assert_eq!(
        configured_low_latency_present_mode(&capabilities, Some("mailbox")),
        wgpu::PresentMode::Mailbox
    );
    assert_eq!(
        configured_low_latency_present_mode(&capabilities, Some("fifo-relaxed")),
        wgpu::PresentMode::Immediate,
        "unsupported overrides fall back to the normal low-latency policy"
    );
    assert_eq!(
        configured_low_latency_present_mode(&capabilities, Some("not-a-mode")),
        wgpu::PresentMode::Immediate
    );
    assert_eq!(
        configured_low_latency_present_mode(&capabilities, None),
        wgpu::PresentMode::Immediate
    );
    assert_eq!(
        configured_low_latency_present_mode(&capabilities, Some("immediate")),
        wgpu::PresentMode::Immediate,
        "supported overrides remain available for diagnostics"
    );
}


#[test]
fn interactive_surface_latency_uses_one_frame_for_non_vsync_present_modes() {
    assert_eq!(
        interactive_desired_maximum_frame_latency(wgpu::PresentMode::Mailbox),
        LOW_LATENCY_SURFACE_FRAME_LATENCY
    );
    assert_eq!(
        interactive_desired_maximum_frame_latency(wgpu::PresentMode::Fifo),
        PACED_SURFACE_FRAME_LATENCY
    );
    assert_eq!(
        interactive_desired_maximum_frame_latency(wgpu::PresentMode::Immediate),
        LOW_LATENCY_SURFACE_FRAME_LATENCY
    );
    assert_eq!(
        interactive_desired_maximum_frame_latency(wgpu::PresentMode::AutoNoVsync),
        LOW_LATENCY_SURFACE_FRAME_LATENCY
    );
}


#[test]
fn configured_surface_frame_latency_honors_bounded_override() {
    assert_eq!(
        configured_desired_maximum_frame_latency(wgpu::PresentMode::Mailbox, None),
        (LOW_LATENCY_SURFACE_FRAME_LATENCY, "present_mode_default")
    );
    assert_eq!(
        configured_desired_maximum_frame_latency(wgpu::PresentMode::Mailbox, Some("2")),
        (2, "env_override")
    );
    assert_eq!(
        configured_desired_maximum_frame_latency(wgpu::PresentMode::Mailbox, Some("0")),
        (1, "env_override")
    );
    assert_eq!(
        configured_desired_maximum_frame_latency(wgpu::PresentMode::Mailbox, Some("99")),
        (MAX_CONFIGURED_SURFACE_FRAME_LATENCY, "env_override")
    );
    assert_eq!(
        configured_desired_maximum_frame_latency(wgpu::PresentMode::Mailbox, Some("nope")),
        (LOW_LATENCY_SURFACE_FRAME_LATENCY, "invalid_env_default")
    );
}


#[test]
fn semantic_scene_lowers_to_accesskit_tree_update_with_stable_ids() {
    let root_id = boon_host::SemanticId("semantic:root".to_owned());
    let button_id = boon_host::SemanticId("semantic:save".to_owned());
    let checkbox_id = boon_host::SemanticId("semantic:done".to_owned());
    let input_id = boon_host::SemanticId("semantic:filter".to_owned());
    let mut scene = boon_host::SemanticScene {
        root: Some(root_id.clone()),
        focused: Some(input_id.clone()),
        ..boon_host::SemanticScene::default()
    };
    scene.nodes.insert(
        root_id.clone(),
        boon_host::SemanticNode {
            id: root_id.clone(),
            node: boon_host::DocumentNodeId("root".to_owned()),
            role: boon_host::SemanticRole::Application,
            name: Some("Boon app".to_owned()),
            description: None,
            value: None,
            state: boon_host::SemanticState::default(),
            actions: boon_host::SemanticActions::default(),
            relations: boon_host::SemanticRelations {
                children: vec![button_id.clone(), checkbox_id.clone(), input_id.clone()],
                ..boon_host::SemanticRelations::default()
            },
            bounds: Some(boon_host::Rect {
                x: 0.0,
                y: 0.0,
                width: 320.0,
                height: 180.0,
            }),
            language: Some("en".to_owned()),
            heading_level: None,
            href: None,
            source_binding_id: None,
            source_path: None,
            source_intent: None,
        },
    );
    scene.nodes.insert(
        button_id.clone(),
        boon_host::SemanticNode {
            id: button_id.clone(),
            node: boon_host::DocumentNodeId("save".to_owned()),
            role: boon_host::SemanticRole::Button,
            name: Some("Save".to_owned()),
            description: None,
            value: None,
            state: boon_host::SemanticState::default(),
            actions: boon_host::SemanticActions {
                focus: true,
                press: true,
                set_text: false,
                increment: false,
                decrement: false,
            },
            relations: boon_host::SemanticRelations {
                parent: Some(root_id.clone()),
                ..boon_host::SemanticRelations::default()
            },
            bounds: Some(boon_host::Rect {
                x: 8.0,
                y: 8.0,
                width: 80.0,
                height: 28.0,
            }),
            language: None,
            heading_level: None,
            href: None,
            source_binding_id: Some(boon_host::SourceBindingId("source:save".to_owned())),
            source_path: Some("toolbar.save".to_owned()),
            source_intent: Some("press".to_owned()),
        },
    );
    scene.nodes.insert(
        checkbox_id.clone(),
        boon_host::SemanticNode {
            id: checkbox_id.clone(),
            node: boon_host::DocumentNodeId("done".to_owned()),
            role: boon_host::SemanticRole::Checkbox,
            name: Some("Done".to_owned()),
            description: None,
            value: Some(boon_host::SemanticValue::Bool { value: true }),
            state: boon_host::SemanticState {
                checked: Some(true),
                ..boon_host::SemanticState::default()
            },
            actions: boon_host::SemanticActions {
                focus: true,
                press: true,
                set_text: false,
                increment: false,
                decrement: false,
            },
            relations: boon_host::SemanticRelations {
                parent: Some(root_id.clone()),
                ..boon_host::SemanticRelations::default()
            },
            bounds: None,
            language: None,
            heading_level: None,
            href: None,
            source_binding_id: None,
            source_path: None,
            source_intent: None,
        },
    );
    scene.nodes.insert(
        input_id.clone(),
        boon_host::SemanticNode {
            id: input_id.clone(),
            node: boon_host::DocumentNodeId("filter".to_owned()),
            role: boon_host::SemanticRole::TextInput,
            name: Some("Filter".to_owned()),
            description: None,
            value: Some(boon_host::SemanticValue::Text {
                text: "abc".to_owned(),
            }),
            state: boon_host::SemanticState {
                focused: true,
                ..boon_host::SemanticState::default()
            },
            actions: boon_host::SemanticActions {
                focus: true,
                press: false,
                set_text: true,
                increment: false,
                decrement: false,
            },
            relations: boon_host::SemanticRelations {
                parent: Some(root_id.clone()),
                ..boon_host::SemanticRelations::default()
            },
            bounds: None,
            language: None,
            heading_level: None,
            href: None,
            source_binding_id: None,
            source_path: None,
            source_intent: None,
        },
    );

    let snapshot = accesskit_tree_update_from_semantic_scene(&scene, "boon-native", "test-version");
    let repeat = accesskit_tree_update_from_semantic_scene(&scene, "boon-native", "test-version");

    assert_eq!(snapshot.metrics.semantic_node_count, 4);
    assert_eq!(snapshot.metrics.accesskit_node_count, 4);
    assert_eq!(snapshot.metrics.interactive_node_count, 3);
    assert_eq!(snapshot.metrics.focusable_node_count, 3);
    assert_eq!(snapshot.metrics.text_input_node_count, 1);
    assert_eq!(snapshot.metrics.checked_node_count, 1);
    assert_eq!(snapshot.metrics.node_id_collision_count, 0);
    assert!(snapshot.metrics.root_present);
    assert!(snapshot.metrics.focus_present);
    assert_eq!(snapshot.semantic_node_ids, repeat.semantic_node_ids);
    assert_eq!(
        snapshot.tree_update.tree.as_ref().unwrap().root,
        snapshot
            .semantic_node_ids
            .iter()
            .find(|mapping| mapping.semantic_id == "semantic:root")
            .map(|mapping| accesskit::NodeId(mapping.accesskit_node_id))
            .unwrap()
    );
    assert_eq!(
        snapshot.tree_update.focus,
        snapshot
            .semantic_node_ids
            .iter()
            .find(|mapping| mapping.semantic_id == "semantic:filter")
            .map(|mapping| accesskit::NodeId(mapping.accesskit_node_id))
            .unwrap()
    );
    let focus_update =
        accesskit_focus_update_from_semantic_node(&input_id, scene.nodes.get(&input_id));
    assert_eq!(focus_update.tree_update.focus, snapshot.tree_update.focus);
    assert!(
        focus_update.tree_update.tree.is_none(),
        "focus-only updates must not republish unchanged tree metadata"
    );
    assert_eq!(
        focus_update.tree_update.nodes.len(),
        1,
        "focused-node patch should upsert only the changed semantic node"
    );
    assert_eq!(
        focus_update.tree_update.nodes[0].0,
        snapshot.tree_update.focus
    );
    assert_eq!(focus_update.metrics.accesskit_node_count, 1);
    assert!(focus_update.metrics.focus_present);

    let root = snapshot
        .tree_update
        .nodes
        .iter()
        .find(|(_, node)| node.role() == accesskit::Role::Application)
        .expect("root application node should exist");
    assert_eq!(root.1.children().len(), 3);

    let button = snapshot
        .tree_update
        .nodes
        .iter()
        .find(|(_, node)| node.role() == accesskit::Role::Button)
        .expect("button node should exist");
    assert!(button.1.supports_action(accesskit::Action::Click));
    assert!(button.1.supports_action(accesskit::Action::Focus));

    let text_input = snapshot
        .tree_update
        .nodes
        .iter()
        .find(|(_, node)| node.role() == accesskit::Role::TextInput)
        .expect("text input node should exist");
    assert!(text_input.1.supports_action(accesskit::Action::SetValue));
    assert!(
        text_input
            .1
            .supports_action(accesskit::Action::ReplaceSelectedText)
    );

    let checkbox = snapshot
        .tree_update
        .nodes
        .iter()
        .find(|(_, node)| node.role() == accesskit::Role::CheckBox)
        .expect("checkbox node should exist");
    assert!(checkbox.1.supports_action(accesskit::Action::Click));
    assert_eq!(checkbox.1.toggled(), Some(accesskit::Toggled::True));
}
