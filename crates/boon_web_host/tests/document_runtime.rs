use boon_document::render_scene::ApproximateTextColumnMeasurer;
use boon_document::{
    DocumentFrame, DocumentNode, DocumentNodeId, DocumentNodeKind, DocumentPatch, DocumentState,
    SemanticWebInputEvent, StyleValue, TextValue,
};
use boon_host::{
    HostEvent, KeyEvent, LogicalKey, LogicalSize, PhysicalSize, PointerButton, PointerEvent,
    PointerPhase, SemanticId, SensitiveInputEvent, SensitiveInputHandle, SurfaceId,
    SurfaceResizeEvent, TextInputEvent, Viewport,
};
use boon_runtime::{DocumentPatchStatus, RuntimeTurn};
use boon_web_host::{
    BrowserDocumentRuntime, BrowserDocumentRuntimeConfig, BrowserDocumentRuntimeError,
    BrowserFramePacing,
};
use serde::de::{
    Deserializer, IntoDeserializer, Visitor,
    value::{Error as ValueError, MapDeserializer, SeqDeserializer},
};

fn viewport() -> Viewport {
    Viewport {
        surface: 7,
        width: 320.0,
        height: 180.0,
        scale: 1.0,
    }
}

fn fixture_frame() -> DocumentFrame {
    let mut frame = DocumentFrame::empty("root");
    let root_id = frame.root.clone();
    let root = frame.nodes.get_mut(&root_id).unwrap();
    root.style
        .insert("width".to_owned(), StyleValue::Number(320.0));
    root.style
        .insert("height".to_owned(), StyleValue::Number(180.0));
    root.style.insert("gap".to_owned(), StyleValue::Number(8.0));

    let mut button = DocumentNode::new("action", DocumentNodeKind::Button);
    button.parent = Some(root_id.clone());
    button.text = Some(TextValue {
        text: "Apply".to_owned(),
    });
    button
        .style
        .insert("width".to_owned(), StyleValue::Number(120.0));
    button
        .style
        .insert("height".to_owned(), StyleValue::Number(36.0));
    push_source_binding(&mut button, "binding:action:press", "ui.action", "press");

    let mut input = DocumentNode::new("query", DocumentNodeKind::TextInput);
    input.parent = Some(root_id.clone());
    input.text = Some(TextValue {
        text: "initial".to_owned(),
    });
    input
        .style
        .insert("width".to_owned(), StyleValue::Number(180.0));
    input
        .style
        .insert("height".to_owned(), StyleValue::Number(36.0));
    for (suffix, path, intent) in [
        ("change", "ui.query.change", "change"),
        ("key", "ui.query.key", "key_down"),
        ("blur", "ui.query.blur", "blur"),
    ] {
        push_source_binding(&mut input, &format!("binding:query:{suffix}"), path, intent);
    }

    frame.nodes.get_mut(&root_id).unwrap().children = vec![button.id.clone(), input.id.clone()];
    frame.nodes.insert(button.id.clone(), button);
    frame.nodes.insert(input.id.clone(), input);
    frame
}

fn sensitive_fixture_frame() -> DocumentFrame {
    let mut frame = fixture_frame();
    let input = frame
        .nodes
        .get_mut(&DocumentNodeId("query".to_owned()))
        .unwrap();
    input.text = Some(TextValue {
        text: String::new(),
    });
    input
        .style
        .insert("sensitive".to_owned(), StyleValue::Bool(true));
    frame
}

fn push_source_binding(node: &mut DocumentNode, id: &str, source_path: &str, intent: &str) {
    let fields = [
        ("id", FixtureBindingValue::Tuple(id)),
        ("source_path", FixtureBindingValue::Text(source_path)),
        ("intent", FixtureBindingValue::Text(intent)),
    ]
    .into_iter()
    .map(|(name, value)| (name.into_deserializer(), value));
    let binding =
        serde::Deserialize::deserialize(MapDeserializer::<_, serde::de::value::Error>::new(fields))
            .unwrap();
    node.source_bindings.push(binding);
}

enum FixtureBindingValue<'a> {
    Text(&'a str),
    Tuple(&'a str),
}

impl<'de> IntoDeserializer<'de, ValueError> for FixtureBindingValue<'de> {
    type Deserializer = Self;

    fn into_deserializer(self) -> Self::Deserializer {
        self
    }
}

impl<'de> Deserializer<'de> for FixtureBindingValue<'de> {
    type Error = ValueError;

    fn deserialize_any<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self {
            Self::Text(value) | Self::Tuple(value) => visitor.visit_borrowed_str(value),
        }
    }

    fn deserialize_tuple_struct<V>(
        self,
        _name: &'static str,
        _len: usize,
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        let value = match self {
            Self::Text(value) | Self::Tuple(value) => value,
        };
        visitor.visit_seq(SeqDeserializer::new(std::iter::once(
            value.into_deserializer(),
        )))
    }

    fn deserialize_newtype_struct<V>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        let value = match self {
            Self::Text(value) | Self::Tuple(value) => value,
        };
        visitor.visit_newtype_struct(value.into_deserializer())
    }

    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 u8 u16 u32 u64 f32 f64 char str string bytes
        byte_buf option unit unit_struct seq tuple map struct enum
        identifier ignored_any
    }
}

fn runtime_turn(sequence: u64, document_patches: Vec<DocumentPatch>) -> RuntimeTurn {
    RuntimeTurn {
        sequence,
        source_sequence: None,
        deltas: Vec::new(),
        authority_deltas: Vec::new(),
        durable_changes: Vec::new(),
        outbox_changes: Vec::new(),
        transient_effects: Vec::new(),
        cancelled_transient_effects: Vec::new(),
        transient_effect_credit_grants: Vec::new(),
        document_patches,
        document_patch_status: DocumentPatchStatus::Complete,
        metrics: Default::default(),
        materialization: Default::default(),
        phase_timings: Default::default(),
    }
}

fn frame_after(frame: &DocumentFrame, patches: Vec<DocumentPatch>) -> DocumentFrame {
    let mut state = DocumentState::from_frame(frame.clone()).unwrap();
    state
        .apply_batch(boon_document::DocumentChangeBatch { patches })
        .unwrap();
    state.into_frame()
}

fn drain_initial_frame(runtime: &mut BrowserDocumentRuntime) {
    let start = runtime.begin_animation_frame();
    assert!(start.render);
    assert!(start.dirty.content);
    assert!(start.dirty.layout);
    assert!(start.dirty.render);
    assert!(start.dirty.semantics);
    let completion = runtime.complete_animation_frame(0, false, false);
    assert!(!completion.schedule_next_animation_frame);
    assert_eq!(completion.pacing, BrowserFramePacing::Idle);
}

#[test]
fn authoritative_mount_builds_retained_and_semantic_state() {
    let frame = fixture_frame();
    let mut columns = ApproximateTextColumnMeasurer;
    let mut runtime = BrowserDocumentRuntime::new(frame.clone(), viewport(), &mut columns).unwrap();

    assert_eq!(runtime.frame(), &frame);
    assert_eq!(runtime.stats().full_lower_count, 1);
    assert!(!runtime.layout().display_list.is_empty());
    assert!(runtime.hits().entry_for_source_path("ui.action").is_some());
    assert!(
        runtime
            .semantic_scene()
            .nodes
            .contains_key(&SemanticId::from_document_node_id(&DocumentNodeId(
                "action".to_owned()
            )))
    );
    assert!(runtime.animation_frame_pending());
    drain_initial_frame(&mut runtime);
}

#[test]
fn ordered_turn_batch_updates_retained_scene_and_coalesces_one_render_request() {
    let initial = fixture_frame();
    let button_patch = DocumentPatch::SetText {
        id: DocumentNodeId("action".to_owned()),
        text: TextValue {
            text: "Applied".to_owned(),
        },
    };
    let input_patch = DocumentPatch::SetText {
        id: DocumentNodeId("query".to_owned()),
        text: TextValue {
            text: "updated".to_owned(),
        },
    };
    let authoritative = frame_after(&initial, vec![button_patch.clone(), input_patch.clone()]);
    let turns = vec![
        runtime_turn(1, vec![button_patch]),
        runtime_turn(2, vec![input_patch]),
    ];
    let mut columns = ApproximateTextColumnMeasurer;
    let mut runtime = BrowserDocumentRuntime::new(initial, viewport(), &mut columns).unwrap();
    drain_initial_frame(&mut runtime);

    let update = runtime
        .consume_turns_and_verify(&turns, &authoritative, 20, &mut columns)
        .unwrap();
    assert_eq!(update.turn_count, 2);
    assert_eq!(update.patch_count, 2);
    assert_eq!(update.through_runtime_sequence, 2);
    assert!(update.retained.content_changed);
    assert!(update.retained.render_changed);
    assert!(update.semantic.is_some());
    assert!(update.scheduling.request_animation_frame);
    assert_eq!(runtime.frame(), &authoritative);
    assert_eq!(
        runtime
            .frame()
            .nodes
            .get(&DocumentNodeId("query".to_owned()))
            .and_then(|node| node.text.as_ref())
            .map(|text| text.text.as_str()),
        Some("updated")
    );

    let start = runtime.begin_animation_frame();
    assert!(start.render);
    assert!(start.dirty.content);
    assert!(start.dirty.render);
    assert!(!runtime.begin_animation_frame().render);
}

#[test]
fn sequence_gap_and_authoritative_divergence_fail_before_retained_mutation() {
    let initial = fixture_frame();
    let patch = DocumentPatch::SetText {
        id: DocumentNodeId("action".to_owned()),
        text: TextValue {
            text: "changed".to_owned(),
        },
    };
    let mut columns = ApproximateTextColumnMeasurer;
    let mut runtime =
        BrowserDocumentRuntime::new(initial.clone(), viewport(), &mut columns).unwrap();
    drain_initial_frame(&mut runtime);

    let gap = runtime_turn(2, vec![patch.clone()]);
    assert!(matches!(
        runtime.consume_turn(&gap, 1, &mut columns),
        Err(BrowserDocumentRuntimeError::TurnSequence {
            expected: 1,
            actual: 2
        })
    ));
    assert_eq!(runtime.frame(), &initial);
    assert_eq!(runtime.through_runtime_sequence(), 0);

    let next = runtime_turn(1, vec![patch]);
    assert!(matches!(
        runtime.consume_turn_and_verify(&next, &initial, 2, &mut columns),
        Err(BrowserDocumentRuntimeError::AuthoritativeFrameMismatch {
            through_sequence: 1,
            ..
        })
    ));
    assert_eq!(runtime.frame(), &initial);
    assert_eq!(runtime.through_runtime_sequence(), 0);
}

#[test]
fn configured_turn_and_patch_limits_reject_oversized_notifications() {
    let mut columns = ApproximateTextColumnMeasurer;
    let mut runtime = BrowserDocumentRuntime::with_config(
        fixture_frame(),
        viewport(),
        &mut columns,
        BrowserDocumentRuntimeConfig {
            max_turns_per_update: 1,
            max_document_patches_per_update: 1,
            ..BrowserDocumentRuntimeConfig::default()
        },
    )
    .unwrap();
    let turns = [runtime_turn(1, Vec::new()), runtime_turn(2, Vec::new())];
    let authoritative = runtime.frame().clone();
    assert!(matches!(
        runtime.consume_turns_and_verify(&turns, &authoritative, 0, &mut columns),
        Err(BrowserDocumentRuntimeError::TurnLimitExceeded {
            limit: 1,
            actual: 2
        })
    ));
}

#[test]
fn generic_host_and_semantic_events_resolve_only_declared_source_routes() {
    let mut columns = ApproximateTextColumnMeasurer;
    let mut runtime =
        BrowserDocumentRuntime::new(fixture_frame(), viewport(), &mut columns).unwrap();
    drain_initial_frame(&mut runtime);
    let surface = SurfaceId("browser".to_owned());

    let action = runtime.hits().entry_for_source_path("ui.action").unwrap();
    let action_x = action.bounds.x + action.bounds.width / 2.0;
    let action_y = action.bounds.y + action.bounds.height / 2.0;
    let down = HostEvent::Pointer(PointerEvent {
        surface: surface.clone(),
        x: action_x,
        y: action_y,
        phase: PointerPhase::Down,
        button: Some(PointerButton::Primary),
    });
    let up = HostEvent::Pointer(PointerEvent {
        surface: surface.clone(),
        x: action_x,
        y: action_y,
        phase: PointerPhase::Up,
        button: Some(PointerButton::Primary),
    });
    assert!(
        runtime
            .handle_host_event(&down, 10, &mut columns)
            .unwrap()
            .dispatch
            .is_none()
    );
    let press = runtime
        .handle_host_event(&up, 11, &mut columns)
        .unwrap()
        .dispatch
        .unwrap();
    assert_eq!(press.source_path, "ui.action");
    assert_eq!(press.source_intent.as_deref(), Some("press"));
    assert!(press.payload.fields.contains_key("pointer_x"));
    assert!(press.payload.fields.contains_key("pointer_width"));

    let query = runtime
        .hits()
        .entry_for_source_path("ui.query.change")
        .unwrap();
    let query_down = HostEvent::Pointer(PointerEvent {
        surface: surface.clone(),
        x: query.bounds.x + query.bounds.width / 2.0,
        y: query.bounds.y + query.bounds.height / 2.0,
        phase: PointerPhase::Down,
        button: Some(PointerButton::Primary),
    });
    runtime
        .handle_host_event(&query_down, 12, &mut columns)
        .unwrap();
    let text = runtime
        .source_dispatch_for_host_event(&HostEvent::TextInput(TextInputEvent {
            surface: surface.clone(),
            text: "fjord".to_owned(),
        }))
        .unwrap();
    assert_eq!(text.source_path, "ui.query.change");
    assert_eq!(text.payload.text.as_deref(), Some("fjord"));

    let key = runtime
        .source_dispatch_for_host_event(&HostEvent::Keyboard(KeyEvent {
            surface,
            physical_key: Some("Enter".to_owned()),
            logical_key: LogicalKey::Named("Enter".to_owned()),
            pressed: true,
        }))
        .unwrap();
    assert_eq!(key.source_path, "ui.query.key");
    assert_eq!(key.payload.key.as_deref(), Some("Enter"));

    let semantic = runtime
        .source_dispatch_for_semantic_web_event(SemanticWebInputEvent::Press {
            semantic_id: SemanticId::from_document_node_id(&DocumentNodeId("action".to_owned())),
        })
        .unwrap();
    assert_eq!(semantic.source_path, "ui.action");
    assert_eq!(semantic.payload, Default::default());
}

#[test]
fn sensitive_text_uses_only_a_host_owned_sidecar() {
    let mut columns = ApproximateTextColumnMeasurer;
    let mut runtime =
        BrowserDocumentRuntime::new(sensitive_fixture_frame(), viewport(), &mut columns).unwrap();
    drain_initial_frame(&mut runtime);
    let semantic_id = SemanticId::from_document_node_id(&DocumentNodeId("query".to_owned()));
    let handle = SensitiveInputHandle::from_host_sequence(9).unwrap();

    assert!(
        runtime
            .source_dispatch_for_semantic_web_event(SemanticWebInputEvent::SetText {
                semantic_id: semantic_id.clone(),
                text: "must-not-enter-runtime".to_owned(),
            })
            .is_none()
    );
    let dispatch = runtime
        .source_dispatch_for_sensitive_semantic_input(&semantic_id, handle)
        .unwrap();
    assert_eq!(dispatch.source_path, "ui.query.change");
    assert_eq!(dispatch.payload, Default::default());
    assert_eq!(dispatch.sensitive_input, Some(handle));

    let query = runtime
        .hits()
        .entry_for_source_path("ui.query.change")
        .unwrap();
    runtime
        .handle_host_event(
            &HostEvent::Pointer(PointerEvent {
                surface: SurfaceId("browser".to_owned()),
                x: query.bounds.x + query.bounds.width / 2.0,
                y: query.bounds.y + query.bounds.height / 2.0,
                phase: PointerPhase::Down,
                button: Some(PointerButton::Primary),
            }),
            10,
            &mut columns,
        )
        .unwrap();
    assert!(
        runtime
            .source_dispatch_for_host_event(&HostEvent::TextInput(TextInputEvent {
                surface: SurfaceId("browser".to_owned()),
                text: "must-not-enter-runtime".to_owned(),
            }))
            .is_none()
    );
    let host_dispatch = runtime
        .source_dispatch_for_host_event(&HostEvent::SensitiveInput(SensitiveInputEvent {
            surface: SurfaceId("browser".to_owned()),
            handle,
        }))
        .unwrap();
    assert_eq!(host_dispatch.payload, Default::default());
    assert_eq!(host_dispatch.sensitive_input, Some(handle));
}

#[test]
fn resize_rebuilds_layout_and_exposes_one_coalesced_surface_render() {
    let mut columns = ApproximateTextColumnMeasurer;
    let mut runtime =
        BrowserDocumentRuntime::new(fixture_frame(), viewport(), &mut columns).unwrap();
    drain_initial_frame(&mut runtime);
    let output = runtime
        .handle_host_event(
            &HostEvent::Resize(SurfaceResizeEvent {
                surface: SurfaceId("browser".to_owned()),
                logical_size: LogicalSize {
                    width: 640.0,
                    height: 360.0,
                },
                scale: 2.0,
                physical_size: PhysicalSize {
                    width: 1280,
                    height: 720,
                },
                epoch: 2,
            }),
            50,
            &mut columns,
        )
        .unwrap();
    assert!(output.retained.layout_changed);
    assert!(output.retained.render_changed);
    assert!(output.scheduling.request_animation_frame);
    assert_eq!(runtime.viewport().width, 640.0);
    assert_eq!(runtime.viewport().height, 360.0);
    assert_eq!(runtime.render_scene().viewport.width, 640.0);
    assert_eq!(runtime.render_scene().viewport.height, 360.0);
    let render = runtime.begin_animation_frame();
    assert!(render.render);
    assert!(render.dirty.layout);
    assert!(render.dirty.render);
}
