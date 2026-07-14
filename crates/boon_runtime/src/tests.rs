use super::*;

fn runtime_for_path(path: &str) -> LiveRuntime {
    let units = source_units_for_path(Path::new(path)).unwrap();
    LiveRuntime::from_project(path, &units).unwrap()
}

fn apply_mount(patches: &[DocumentPatch]) -> boon_document::DocumentState {
    let root = patches
        .iter()
        .find_map(|patch| match patch {
            DocumentPatch::UpsertNode(node)
                if node.kind == boon_document_model::DocumentNodeKind::Root =>
            {
                Some(node.id.0.clone())
            }
            _ => None,
        })
        .expect("mount contains a root node");
    let mut state = boon_document::DocumentState::new(root);
    for patch in patches {
        state.apply_patch(patch.clone()).unwrap();
    }
    state
}

#[test]
fn scenario_parser_exposes_typed_source_events() {
    let scenario = parse_scenario(Path::new("../../examples/counter.scn")).unwrap();
    let event = scenario
        .steps
        .iter()
        .find_map(|step| step.source_event.as_ref())
        .unwrap();

    assert_eq!(event.source, "store.sources.increment_button.press");
    assert_eq!(event.payload, SourcePayload::default());
}

#[test]
fn unscoped_scenario_source_ignores_visual_target_text() {
    let mut runtime = runtime_for_path("../../examples/counter.bn");
    let scenario = parse_scenario(Path::new("../../examples/counter.scn")).unwrap();

    let turns = runtime.run_scenario(&scenario).unwrap();

    assert_eq!(turns.len(), 6);
    assert_eq!(
        runtime.root_value_current("store.count").unwrap(),
        Value::Number(0)
    );
}

#[test]
fn novywave_panel_arrangement_toggles_once_per_source_event() {
    let mut runtime = runtime_for_path("../../examples/novywave/RUN.bn");
    assert_eq!(
        runtime
            .root_value_current("store.panel_arrangement")
            .unwrap(),
        Value::Text("Stacked".to_owned())
    );

    let event = runtime
        .source_event(
            1,
            "store.elements.panels_toggle_arrangement",
            None,
            SourcePayload::default(),
        )
        .unwrap();
    runtime.dispatch(event).unwrap();

    assert_eq!(
        runtime
            .root_value_current("store.panel_arrangement")
            .unwrap(),
        Value::Text("Docked".to_owned())
    );
}

#[test]
fn novywave_signal_formats_remain_row_local() {
    let mut runtime = runtime_for_path("../../examples/novywave/RUN.bn");
    let formatters = runtime
        .inspect_value_current("selected_signal.formatter", 32)
        .unwrap();
    let Value::List(formatters) = formatters else {
        panic!("selected_signal.formatter inspection did not return a list");
    };
    assert_eq!(formatters.len(), 14);
    let variable_formatters = formatters
        .iter()
        .filter_map(|row| match row {
            Value::Record(fields) => fields.get("value"),
            _ => None,
        })
        .filter(|value| **value != Value::Null)
        .collect::<Vec<_>>();
    assert_eq!(variable_formatters.len(), 13);
    assert!(
        variable_formatters
            .iter()
            .all(|value| **value == Value::Text("Hexadecimal".to_owned()))
    );

    let scenario = parse_scenario(Path::new("../../examples/novywave.scn")).unwrap();
    let mut sequence = 1;
    for step in scenario
        .steps
        .iter()
        .take_while(|step| step.id != "select-temperature-analog")
    {
        if let Some(source) = &step.source_event {
            let target = runtime.scenario_target(source).unwrap();
            let event = runtime
                .source_event(sequence, &source.source, target, source.payload.clone())
                .unwrap();
            runtime.dispatch(event).unwrap();
            sequence += 1;
        }
        let formatters = runtime
            .inspect_value_current("selected_signal.formatter", 32)
            .unwrap();
        let Value::List(formatters) = formatters else {
            panic!("selected_signal.formatter inspection did not return a list");
        };
        let Value::Record(temperature) = &formatters[8] else {
            panic!("temperature formatter inspection did not return a record");
        };
        assert_eq!(
            temperature.get("value"),
            Some(&Value::Text("Hexadecimal".to_owned())),
            "temperature formatter changed after scenario step {}",
            step.id
        );
        assert_eq!(
            runtime.root_value_current("store.value_format").unwrap(),
            Value::Text("Hexadecimal".to_owned()),
            "global value format changed after scenario step {}",
            step.id
        );
    }

    let target = runtime
        .row_target_for_source_text("signal.signal_elements.select_signal", "temperature", 0)
        .unwrap()
        .expect("temperature signal row exists");
    let event = runtime
        .source_event(
            sequence,
            "signal.signal_elements.select_signal",
            Some(target),
            SourcePayload::default(),
        )
        .unwrap();
    runtime.dispatch(event).unwrap();

    assert_eq!(
        runtime.root_value_current("store.active_signal").unwrap(),
        Value::Text("temperature".to_owned())
    );
    assert_eq!(
        runtime.root_value_current("store.value_format").unwrap(),
        Value::Text("Hexadecimal".to_owned())
    );
    let formatters = runtime
        .inspect_value_current("selected_signal.formatter", 32)
        .unwrap();
    let Value::List(formatters) = formatters else {
        panic!("selected_signal.formatter inspection did not return a list");
    };
    let Value::Record(temperature) = &formatters[8] else {
        panic!("temperature formatter inspection did not return a record");
    };
    assert_eq!(
        temperature.get("value"),
        Some(&Value::Text("Hexadecimal".to_owned()))
    );
    assert_eq!(
        runtime
            .root_value_current("store.active_signal_format")
            .unwrap(),
        Value::Text("Hexadecimal".to_owned())
    );
    assert_eq!(
        runtime
            .root_value_current("store.temperature_rendered_value")
            .unwrap(),
        Value::Text("42.8 C".to_owned())
    );
}

#[test]
fn counter_mount_and_update_are_complete_typed_document_patches() {
    let mut runtime = LiveRuntime::from_source(
        "examples/counter.bn",
        include_str!("../../../examples/counter.bn"),
    )
    .unwrap();
    let mount = runtime.mount();

    assert_eq!(mount.document_patch_status, DocumentPatchStatus::Complete);
    assert!(!mount.document_patches.is_empty());
    assert_eq!(mount.materialization.full_evaluation_count, 1);
    assert_eq!(mount.materialization.retained_scalar_evaluation_count, 0);
    let applied = apply_mount(&mount.document_patches);
    let mounted = runtime.document_frame().unwrap();
    assert_eq!(applied.frame(), mounted);
    assert!(mounted.nodes.len() > 5);
    assert!(
        mounted
            .nodes
            .values()
            .any(|node| { node.text.as_ref().is_some_and(|text| text.text == "0") })
    );
    assert_eq!(
        mounted
            .nodes
            .values()
            .flat_map(|node| node.source_bindings())
            .count(),
        3
    );

    let event = runtime
        .source_event(
            1,
            "store.sources.increment_button.press",
            None,
            SourcePayload::default(),
        )
        .unwrap();
    let turn = runtime.dispatch(event).unwrap();

    assert_eq!(turn.sequence, 1);
    assert!(!turn.deltas.is_empty());
    assert!(!turn.document_patches.is_empty());
    assert_eq!(turn.document_patch_status, DocumentPatchStatus::Complete);
    assert_eq!(turn.materialization.full_evaluation_count, 1);
    assert!(turn.materialization.retained_scalar_evaluation_count > 0);
    assert!(
        turn.document_patches.iter().any(|patch| {
            matches!(patch, DocumentPatch::SetText { text, .. } if text.text == "1")
        })
    );
    assert!(
        !turn
            .document_patches
            .iter()
            .any(|patch| matches!(patch, DocumentPatch::UpsertNode(_)))
    );
    assert!(
        runtime
            .document_frame()
            .unwrap()
            .nodes
            .values()
            .any(|node| { node.text.as_ref().is_some_and(|text| text.text == "1") })
    );
}

#[test]
fn unsettled_runtime_turn_rolls_back_authority_document_and_sequence() {
    let mut runtime = LiveRuntime::from_source(
        "examples/counter.bn",
        include_str!("../../../examples/counter.bn"),
    )
    .unwrap();
    let original_frame = runtime.document_frame().cloned().unwrap();
    let event = runtime
        .source_event(
            1,
            "store.sources.increment_button.press",
            None,
            SourcePayload::default(),
        )
        .unwrap();

    let prepared = runtime.dispatch_unsettled(event.clone()).unwrap();
    assert!(!prepared.durable_changes.is_empty());
    assert_eq!(
        runtime.root_value_current("store.count").unwrap(),
        Value::Number(1)
    );

    runtime.rollback_unsettled_turn().unwrap();
    assert_eq!(
        runtime.root_value_current("store.count").unwrap(),
        Value::Number(0)
    );
    assert_eq!(runtime.document_frame(), Some(&original_frame));

    runtime.dispatch(event).unwrap();
    assert_eq!(
        runtime.root_value_current("store.count").unwrap(),
        Value::Number(1)
    );
}

#[test]
fn unsettled_structural_turn_swaps_back_list_and_document_without_cloning_runtime() {
    let mut runtime = runtime_for_path("../../examples/todomvc.bn");
    let change = runtime
        .source_event(
            1,
            "store.sources.new_todo_input.change",
            None,
            SourcePayload {
                text: Some("Transactional todo".into()),
                ..SourcePayload::default()
            },
        )
        .unwrap();
    runtime.dispatch(change).unwrap();
    let original_frame = runtime.document_frame().cloned().unwrap();
    let original_snapshot = runtime.snapshot().unwrap();
    let submit = runtime
        .source_event(
            2,
            "store.sources.new_todo_input.key_down",
            None,
            SourcePayload {
                key: Some("Enter".into()),
                ..SourcePayload::default()
            },
        )
        .unwrap();

    let prepared = runtime.dispatch_unsettled(submit.clone()).unwrap();
    assert!(!prepared.durable_changes.is_empty());
    assert_ne!(runtime.document_frame(), Some(&original_frame));

    runtime.rollback_unsettled_turn().unwrap();
    assert_eq!(runtime.snapshot().unwrap(), original_snapshot);
    assert_eq!(runtime.document_frame(), Some(&original_frame));

    runtime.dispatch(submit).unwrap();
    assert_ne!(runtime.document_frame(), Some(&original_frame));
}

#[test]
fn source_pipeline_binds_the_declared_widget_event_slot() {
    let runtime = LiveRuntime::from_source(
        "source-pipeline.bn",
        r#"
store: [
    button_source: SOURCE
    checkbox_source: SOURCE
    value: 0 |> HOLD value {
        LATEST {
            button_source |> THEN { value + 1 }
            checkbox_source |> THEN { value + 1 }
        }
    }
]
document: Document/new(root: app())

FUNCTION app() {
    Element/container(
        element: []
        style: [width: Fill, height: Fill]
        child: Element/stripe(
            element: []
            direction: Row
            style: [width: Fill, height: 40]
            items: LIST {
                bound_button() |> SOURCE { store.button_source }
                bound_checkbox() |> SOURCE { store.checkbox_source }
            }
        )
    )
}

FUNCTION bound_button() {
    Element/button(
        element: [event: [click: SOURCE], hovered: SOURCE]
        style: [width: 100, height: 40]
        label: bound_label()
    )
}

FUNCTION bound_label() {
    Element/text(
            element: []
            style: [width: 100, height: 40]
            text: TEXT { Bound }
    )
}

FUNCTION bound_checkbox() {
    BLOCK {
        Element/checkbox(
            element: [event: [click: SOURCE], hovered: SOURCE]
            style: [width: 40, height: 40]
            checked: False
        )
    }
}
"#,
    )
    .unwrap();
    let frame = runtime.document_frame().unwrap();
    assert!(
        frame
            .nodes
            .values()
            .any(|node| node.kind == boon_document_model::DocumentNodeKind::Checkbox),
        "node kinds: {:?}",
        frame
            .nodes
            .values()
            .map(|node| &node.kind)
            .collect::<Vec<_>>()
    );
    assert!(
        frame
            .nodes
            .values()
            .any(|node| node.text.as_ref().is_some_and(|text| text.text == "Bound"))
    );
    let bindings = frame
        .nodes
        .values()
        .flat_map(|node| node.source_bindings())
        .map(|binding| (binding.source_path.as_str(), binding.intent.as_str()))
        .collect::<Vec<_>>();

    assert_eq!(
        bindings,
        [
            ("store.button_source", "click"),
            ("store.checkbox_source", "click")
        ]
    );
}

#[test]
fn document_match_arm_reads_tagged_pattern_fields_without_type_hint_fallbacks() {
    let runtime = LiveRuntime::from_source(
        "tagged-pattern-document.bn",
        r#"
events: SOURCE
value: 0 |> HOLD value { LATEST { events |> THEN { value } } }
document: Document/new(root: app())

FUNCTION app() {
    BLOCK {
        choice: Choice[value: TEXT { selected }]

        Element/text(
            element: []
            style: [width: 100, height: 20]
            text: choice |> WHEN {
                Choice[value] => Status[active: True] |> WHEN {
                    Status[active] => value
                }
            }
        )
    }
}
"#,
    )
    .unwrap();

    let texts = runtime
        .document_frame()
        .unwrap()
        .nodes
        .values()
        .filter_map(|node| node.text.as_ref().map(|text| text.text.clone()))
        .collect::<Vec<_>>();
    assert!(texts.iter().any(|text| text == "selected"), "{texts:?}");
}

#[test]
fn source_turn_rebuilds_a_function_returned_conditional_subtree() {
    let mut runtime = LiveRuntime::from_source(
        "conditional-subtree.bn",
        r#"
store: [
    open_dialog: SOURCE
    dialog: Closed |> HOLD dialog {
        LATEST {
            open_dialog |> THEN { Open }
        }
    }
]
document: Document/new(root: app())

FUNCTION app() {
    Element/stripe(
        element: []
        direction: Column
        style: [width: Fill, height: Fill]
        items: LIST {
            Element/button(
                element: [event: [press: store.open_dialog]]
                style: [width: 100, height: 40]
                label: TEXT { Open }
            )
            dialog()
        }
    )
}

FUNCTION dialog() {
    store.dialog == Open |> WHEN {
        True => Element/button(
            element: []
            style: [width: 100, height: 40]
            label: TEXT { Dialog action }
        )
        __ => NoElement
    }
}
"#,
    )
    .unwrap();
    let mut applied = apply_mount(&runtime.mount().document_patches);
    assert!(
        !runtime
            .document_frame()
            .unwrap()
            .nodes
            .values()
            .any(|node| {
                node.text
                    .as_ref()
                    .is_some_and(|text| text.text == "Dialog action")
            })
    );

    let event = runtime
        .source_event(1, "store.open_dialog", None, SourcePayload::default())
        .unwrap();
    let turn = runtime.dispatch(event).unwrap();
    let dialog = runtime.root_value_current("store.dialog").unwrap();
    assert_eq!(dialog, Value::Text("Open".to_owned()));
    for patch in &turn.document_patches {
        applied.apply_patch(patch.clone()).unwrap();
    }

    assert!(
        runtime
            .document_frame()
            .unwrap()
            .nodes
            .values()
            .any(|node| {
                node.text
                    .as_ref()
                    .is_some_and(|text| text.text == "Dialog action")
            }),
        "patches: {:#?}; deltas: {:#?}; stats: {:#?}",
        turn.document_patches,
        turn.deltas,
        turn.materialization
    );
    assert!(turn.materialization.full_evaluation_count > 1);
    assert_eq!(applied.frame(), runtime.document_frame().unwrap());
}

#[test]
fn row_sources_survive_helper_parameter_renaming() {
    let mut runtime = LiveRuntime::from_source(
        "renamed-row-source.bn",
        r#"
store: [
    rows:
        LIST {
            [label: TEXT { First }]
        }
        |> List/map(item, new: new_row(item: item))
    row_selected:
        rows
        |> List/map(item, new: LATEST {
            item.controls.select.event.press |> THEN { item.label }
        })
        |> List/latest()
    selected:
        TEXT { none } |> HOLD selected {
            LATEST { row_selected }
        }
]
document: Document/new(root: app())

FUNCTION new_row(item) {
    [
        controls: [select: SOURCE]
        label: item.label
    ]
}

FUNCTION app() {
    Element/stripe(
        element: []
        direction: Column
        style: [width: Fill, height: Fill]
        items: store.rows |> List/map(item, new: row_button(row: item))
    )
}

FUNCTION row_button(row) {
    Element/button(
        element: [event: [press: row.controls.select]]
        style: [width: 100, height: 40]
        label: row.label
    )
}
"#,
    )
    .unwrap();
    let source = runtime
        .source_inventory()
        .sources
        .iter()
        .find(|source| source.path.ends_with(".controls.select"))
        .expect("row source route");
    assert!(
        runtime
            .document_frame()
            .unwrap()
            .nodes
            .values()
            .flat_map(|node| node.source_bindings())
            .any(|binding| binding.source_path == source.path),
        "row source {} was not attached through the renamed helper parameter",
        source.path
    );
    let source_path = source.path.clone();
    let row = runtime
        .row_target_for_source_text(&source_path, "First", 0)
        .unwrap()
        .expect("rendered row remains addressable by model text");
    let event = runtime
        .source_event(1, &source_path, Some(row), SourcePayload::default())
        .unwrap();
    runtime.dispatch(event).unwrap();
    assert_eq!(
        runtime.root_value_current("store.row_selected").unwrap(),
        Value::Text("First".to_owned())
    );
}

#[test]
fn empty_materialization_window_restores_rows_when_the_source_refills() {
    let mut runtime = runtime_for_path("../../examples/todomvc.bn");
    for (sequence, path) in [
        (1, "store.sources.toggle_all_checkbox.click"),
        (2, "store.sources.toggle_all_checkbox.click"),
        (3, "store.sources.filter_completed.press"),
    ] {
        let event = runtime
            .source_event(sequence, path, None, SourcePayload::default())
            .unwrap();
        runtime.dispatch(event).unwrap();
    }
    assert!(
        !runtime
            .document_frame()
            .unwrap()
            .nodes
            .values()
            .any(|node| node
                .text
                .as_ref()
                .is_some_and(|text| text.text == "Read documentation"))
    );

    let materialization = runtime.document_materialization_ids()[0];
    runtime
        .demand_document_window(DocumentWindowDemand {
            materialization,
            visible: 0..0,
            overscan: 0..0,
        })
        .unwrap();
    let all = runtime
        .source_event(
            4,
            "store.sources.filter_all.press",
            None,
            SourcePayload::default(),
        )
        .unwrap();
    runtime.dispatch(all).unwrap();

    assert!(
        runtime
            .document_frame()
            .unwrap()
            .nodes
            .values()
            .any(|node| node
                .text
                .as_ref()
                .is_some_and(|text| text.text == "Read documentation"))
    );
}

#[test]
fn cells_mount_is_sparse_and_selected_formula_is_current() {
    let mut runtime = runtime_for_path("../../examples/cells.bn");
    let mount = runtime.mount();
    let stats = mount.materialization;

    assert_eq!(mount.document_patch_status, DocumentPatchStatus::Complete);
    assert!(stats.logical_rows >= 2_600, "{stats:?}");
    assert!(stats.materialized_rows < stats.logical_rows, "{stats:?}");
    assert!(stats.materialized_nodes < 1_000, "{stats:?}");
    assert_eq!(stats.full_evaluation_count, 1, "{stats:?}");
    let frame = runtime.document_frame().unwrap();
    assert!(frame.nodes.values().any(|node| {
        node.kind == boon_document_model::DocumentNodeKind::TextInput
            && node.text.as_ref().is_some_and(|text| text.text == "5")
    }));

    let (path, key, generation) = frame
        .nodes
        .values()
        .find_map(|node| {
            (node.style.get("address")
                == Some(&boon_document_model::StyleValue::Text("A2".to_owned())))
            .then(|| {
                let binding = node.primary_source_binding()?;
                let number = |name| match node.style.get(name) {
                    Some(boon_document_model::StyleValue::Number(value)) => Some(*value as u64),
                    _ => None,
                };
                Some((
                    binding.source_path.clone(),
                    number("row_key")?,
                    number("row_generation")?,
                ))
            })
            .flatten()
        })
        .expect("A2 has a typed source binding and row identity");
    let row = runtime
        .row_target_for_source_path(&path, key, generation)
        .unwrap();
    let event = runtime
        .source_event(1, &path, Some(row), SourcePayload::default())
        .unwrap();
    let turn = runtime.dispatch(event).unwrap();
    assert!(!turn.document_patches.is_empty());
    assert_eq!(turn.materialization.full_evaluation_count, 1);
    assert!(turn.materialization.retained_scalar_evaluation_count > 0);
    assert!(
        turn.materialization.retained_scalar_evaluation_count <= 16,
        "selection reevaluated an unbounded visible binding fanout: {:?}",
        turn.materialization
    );
    assert!(
        runtime
            .document_frame()
            .unwrap()
            .nodes
            .values()
            .any(|node| {
                node.kind == boon_document_model::DocumentNodeKind::TextInput
                    && node.text.as_ref().is_some_and(|text| text.text == "15")
            })
    );

    let materialization = runtime.document_materialization_ids()[0];
    let patches = runtime
        .demand_document_window(DocumentWindowDemand {
            materialization,
            visible: 2..4,
            overscan: 1..5,
        })
        .unwrap();
    assert!(!patches.is_empty());
    let demand_stats = runtime.document_materialization_stats();
    assert!(demand_stats.materialized_rows < stats.logical_rows);
    assert_eq!(demand_stats.full_evaluation_count, 2);
}

#[test]
fn inspector_reads_bounded_current_list_fields_without_materializing_the_grid() {
    let mut runtime = runtime_for_path("../../examples/cells.bn");
    runtime.mount();

    let Value::List(values) = runtime.inspect_value_current("cell.address", 3).unwrap() else {
        panic!("cell.address inspection should return bounded row samples");
    };
    assert_eq!(values.len(), 3);
    assert!(values.iter().all(|value| matches!(value, Value::Record(_))));

    let Value::List(values) = runtime.inspect_value_current("cell.value", 1).unwrap() else {
        panic!("cell.value inspection should cross the row currentness barrier");
    };
    assert_eq!(values.len(), 1);
}

#[test]
fn cells_commit_with_unchanged_display_emits_no_redundant_patch() {
    let mut runtime = runtime_for_path("../../examples/cells.bn");
    let event = |runtime: &LiveRuntime, sequence, path: &str, address: &str, text: Option<&str>| {
        runtime
            .source_event(
                sequence,
                path,
                None,
                SourcePayload {
                    address: Some(address.to_owned()),
                    text: text.map(str::to_owned),
                    ..SourcePayload::default()
                },
            )
            .unwrap()
    };
    runtime
        .dispatch(event(&runtime, 1, "cell.sources.editor.select", "A3", None))
        .unwrap();
    runtime
        .dispatch(event(
            &runtime,
            2,
            "cell.sources.editor.change",
            "A3",
            Some("20"),
        ))
        .unwrap();
    let turn = runtime
        .dispatch(event(
            &runtime,
            3,
            "cell.sources.editor.commit",
            "A3",
            Some("20"),
        ))
        .unwrap();

    assert!(
        turn.deltas.iter().any(|delta| matches!(
            delta,
            Delta::SetValue {
                target: ValueTarget::RowField {
                    field: boon_plan::FieldId(6),
                    ..
                },
                value: Value::Text(value),
            } if value == "20"
        )),
        "commit deltas: {:#?}",
        turn.deltas
    );
    assert!(
        turn.document_patches.is_empty(),
        "{:#?}",
        turn.document_patches
    );
    assert!(
        turn.durable_changes.iter().all(|change| matches!(
            change,
            DurableChange::SetRowField { .. } | DurableChange::SetScalar { .. }
        )),
        "Cells edit emitted structural persistence: {:#?}",
        turn.durable_changes
    );
    let durable = runtime.durable_restore_image(1, BTreeSet::new()).unwrap();
    let persisted_rows = durable
        .lists
        .values()
        .map(|list| {
            assert!(!list.touched, "Cells edit persisted full list structure");
            list.rows.len()
        })
        .sum::<usize>();
    assert_eq!(persisted_rows, 1, "{durable:#?}");
}

#[test]
fn cells_formula_dependency_recomputes_visible_fanout() {
    let mut runtime = runtime_for_path("../../examples/cells.bn");
    let b0_value = ValueTarget::RowField {
        row: RowId {
            list: boon_plan::ListId(1),
            key: 2,
            generation: 1,
        },
        field: boon_plan::FieldId(6),
    };
    let event = |runtime: &LiveRuntime, sequence, path: &str, text: Option<&str>| {
        runtime
            .source_event(
                sequence,
                path,
                None,
                SourcePayload {
                    address: Some("A0".to_owned()),
                    text: text.map(str::to_owned),
                    ..SourcePayload::default()
                },
            )
            .unwrap()
    };
    runtime
        .dispatch(event(&runtime, 1, "cell.sources.editor.select", None))
        .unwrap();
    runtime
        .dispatch(event(&runtime, 2, "cell.sources.editor.change", Some("41")))
        .unwrap();
    let turn = runtime
        .dispatch(event(&runtime, 3, "cell.sources.editor.commit", Some("41")))
        .unwrap();
    let value = runtime.session.project_current(&[b0_value]).unwrap()[&b0_value].clone();

    assert!(turn.metrics.recomputed_targets.contains(&b0_value));
    assert_eq!(
        value,
        Value::Number(51),
        "metrics: {:#?}; deltas: {:#?}",
        turn.metrics,
        turn.deltas
    );
}

#[test]
fn todomvc_compiles_and_mounts_a_bounded_typed_document() {
    let runtime = runtime_for_path("../../examples/todomvc.bn");
    let mount = runtime.mount();

    assert_eq!(mount.document_patch_status, DocumentPatchStatus::Complete);
    assert!(!mount.document_patches.is_empty());
    assert!(runtime.document_frame().unwrap().nodes.len() > 10);
    assert!(mount.materialization.materialized_rows <= mount.materialization.logical_rows);
    let bindings = runtime
        .document_frame()
        .unwrap()
        .nodes
        .values()
        .flat_map(|node| node.source_bindings())
        .map(|binding| binding.source_path.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    for expected in [
        "store.sources.new_todo_input.change",
        "store.sources.new_todo_input.key_down",
        "todo.sources.todo_title_element.double_click",
        "todo.sources.editing_todo_title_element.blur",
    ] {
        assert!(
            bindings.contains(expected),
            "missing {expected}: {bindings:?}"
        );
    }
}

#[test]
fn todomvc_append_updates_executor_owned_rows_and_document() {
    let mut runtime = runtime_for_path("../../examples/todomvc.bn");
    let change = runtime
        .source_event(
            1,
            "store.sources.new_todo_input.change",
            None,
            SourcePayload {
                text: Some("Test todo".into()),
                ..SourcePayload::default()
            },
        )
        .unwrap();
    runtime.dispatch(change).unwrap();
    let submit = runtime
        .source_event(
            2,
            "store.sources.new_todo_input.key_down",
            None,
            SourcePayload {
                key: Some("Enter".into()),
                ..SourcePayload::default()
            },
        )
        .unwrap();
    runtime.dispatch(submit).unwrap();

    let scope = runtime
        .session
        .plan()
        .source_routes
        .iter()
        .find(|route| route.path == "todo.sources.todo_checkbox.click")
        .and_then(|route| route.scope_id)
        .expect("todo checkbox row scope");
    let list = runtime
        .session
        .plan()
        .storage_layout
        .list_slots
        .iter()
        .find(|slot| slot.scope_id == Some(scope))
        .map(|slot| slot.list_id)
        .expect("todo list");
    let snapshot = runtime.session.snapshot().unwrap();
    let rows = &snapshot.lists[&list];
    assert!(
        rows.iter().any(|row| row
            .fields
            .values()
            .any(|value| value == &Value::Text("Test todo".into()))),
        "{rows:#?}"
    );
    assert!(
        runtime
            .session
            .find_row_by_text(list, "Test todo", 0)
            .is_some()
    );
}

#[test]
fn todomvc_edit_draft_keeps_the_committed_title_addressable() {
    fn dispatch(
        runtime: &mut LiveRuntime,
        sequence: u64,
        path: &str,
        row: RowId,
        payload: SourcePayload,
    ) -> RuntimeTurn {
        let event = runtime
            .source_event(sequence, path, Some(row), payload)
            .unwrap();
        runtime.dispatch(event).unwrap()
    }

    let mut runtime = runtime_for_path("../../examples/todomvc.bn");
    for (sequence, path, payload) in [
        (
            1,
            "store.sources.new_todo_input.change",
            SourcePayload {
                text: Some("Test todo".into()),
                ..SourcePayload::default()
            },
        ),
        (
            2,
            "store.sources.new_todo_input.key_down",
            SourcePayload {
                key: Some("Enter".into()),
                ..SourcePayload::default()
            },
        ),
    ] {
        let event = runtime.source_event(sequence, path, None, payload).unwrap();
        runtime.dispatch(event).unwrap();
    }
    let scope = runtime
        .session
        .plan()
        .source_routes
        .iter()
        .find(|route| route.path == "todo.sources.todo_checkbox.click")
        .and_then(|route| route.scope_id)
        .unwrap();
    let list = runtime
        .session
        .plan()
        .storage_layout
        .list_slots
        .iter()
        .find(|slot| slot.scope_id == Some(scope))
        .map(|slot| slot.list_id)
        .unwrap();
    let row = runtime
        .session
        .find_row_by_text(list, "Test todo", 0)
        .unwrap();
    dispatch(
        &mut runtime,
        3,
        "todo.sources.todo_title_element.double_click",
        row,
        SourcePayload::default(),
    );
    dispatch(
        &mut runtime,
        4,
        "todo.sources.editing_todo_title_element.change",
        row,
        SourcePayload {
            text: Some("Test todo edited".into()),
            ..SourcePayload::default()
        },
    );
    dispatch(
        &mut runtime,
        5,
        "todo.sources.editing_todo_title_element.key_down",
        row,
        SourcePayload {
            key: Some("Enter".into()),
            ..SourcePayload::default()
        },
    );
    assert!(
        runtime
            .session
            .find_row_by_text(list, "Test todo edited", 0)
            .is_some()
    );

    dispatch(
        &mut runtime,
        6,
        "todo.sources.todo_title_element.double_click",
        row,
        SourcePayload::default(),
    );
    let turn = dispatch(
        &mut runtime,
        7,
        "todo.sources.editing_todo_title_element.change",
        row,
        SourcePayload {
            text: Some("Cancelled title".into()),
            ..SourcePayload::default()
        },
    );

    assert!(
        runtime
            .session
            .find_row_by_text(list, "Test todo edited", 0)
            .is_some(),
        "draft change deltas: {:#?}",
        turn.deltas
    );
}

#[test]
fn runtime_plan_cache_is_keyed_by_source_content() {
    let source = include_str!("../../../examples/counter.bn");
    let (_, first) = LiveRuntime::from_source_profiled("first.bn", source).unwrap();
    let (_, second) = LiveRuntime::from_source_profiled("second.bn", source).unwrap();

    assert_eq!(
        first.compile.expression_count,
        second.compile.expression_count
    );
    assert!(second.cache_hit);
}

#[test]
fn runtime_source_cache_is_partitioned_by_application_identity() {
    let source = include_str!("../../../examples/counter.bn");
    let first_identity =
        ApplicationIdentity::new("dev.boon.runtime-cache", "first", "runtime-test");
    let second_identity =
        ApplicationIdentity::new("dev.boon.runtime-cache", "second", "runtime-test");

    let (first, _) = LiveRuntime::from_source_profiled_with_identity(
        "identity-first.bn",
        source,
        first_identity.clone(),
    )
    .unwrap();
    let (second, second_profile) = LiveRuntime::from_source_profiled_with_identity(
        "identity-second.bn",
        source,
        second_identity.clone(),
    )
    .unwrap();
    let (_, first_again_profile) = LiveRuntime::from_source_profiled_with_identity(
        "identity-first-again.bn",
        source,
        first_identity.clone(),
    )
    .unwrap();

    assert_eq!(first.machine_plan().application.identity, first_identity);
    assert_eq!(second.machine_plan().application.identity, second_identity);
    assert!(!second_profile.cache_hit);
    assert!(first_again_profile.cache_hit);
}

#[test]
fn runtime_source_units_preserve_identity_and_partition_the_cache() {
    let source = include_str!("../../../examples/counter.bn");
    let units = [RuntimeSourceUnit {
        path: "counter-unit.bn".to_owned(),
        source: source.to_owned(),
    }];
    let first_identity =
        ApplicationIdentity::new("dev.boon.runtime-units", "first", "runtime-test");
    let second_identity =
        ApplicationIdentity::new("dev.boon.runtime-units", "second", "runtime-test");

    let first =
        LiveRuntime::from_project_with_identity("counter-project", &units, first_identity.clone())
            .unwrap();
    let (second, second_profile) = LiveRuntime::from_project_profiled_with_identity(
        "counter-project",
        &units,
        second_identity.clone(),
    )
    .unwrap();

    assert_eq!(first.machine_plan().application.identity, first_identity);
    assert_eq!(second.machine_plan().application.identity, second_identity);
    assert!(!second_profile.cache_hit);
}

#[test]
fn durable_restore_settles_before_initial_document_materialization() {
    let identity = ApplicationIdentity::new("dev.boon.runtime-restore", "counter", "runtime-test");
    let mut runtime = LiveRuntime::from_source_with_identity(
        "counter-restore.bn",
        include_str!("../../../examples/counter.bn"),
        identity.clone(),
    )
    .unwrap();
    let plan = runtime.shared_machine_plan();
    let increment = runtime
        .source_event(
            1,
            "store.sources.increment_button.press",
            None,
            SourcePayload::default(),
        )
        .unwrap();
    let turn = runtime.dispatch(increment).unwrap();
    assert!(!turn.authority_deltas.is_empty());

    let image = runtime.durable_restore_image(7, BTreeSet::new()).unwrap();
    assert_eq!(image.application, identity);
    assert_eq!(image.epoch, 7);

    let mut restored = LiveRuntime::from_shared_machine_plan_with_restore(
        plan,
        SessionOptions::default(),
        Some(image),
    )
    .unwrap();

    assert_eq!(
        restored.root_value_current("store.count").unwrap(),
        Value::Number(1)
    );
    assert!(
        restored
            .document_frame()
            .unwrap()
            .nodes
            .values()
            .any(|node| node.text.as_ref().is_some_and(|text| text.text == "1"))
    );
}
