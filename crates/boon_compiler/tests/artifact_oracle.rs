use boon_compiler::{ArtifactOraclePlanPair, CompileRequest, compile_artifact_oracle_pair};
use boon_document::runtime::DocumentRuntime;
use boon_document::{
    DocumentFrame, DocumentNodeId, DocumentNodeKind, EmbeddedProgramDescriptor,
    MapViewportDescriptor, MaterializedRange, ScrollState, StyleMap, TextValue,
};
use boon_plan::{
    ApplicationIdentity, DataTypePlan, ListActivationMode, MachinePlan, ProgramRole, TargetProfile,
    machine_plan_stable_contract_sha256, plan_sha256,
};
use boon_plan_executor::{MachineInstance, SessionOptions, Snapshot, SourceEvent, SourcePayload};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

#[derive(Debug, PartialEq)]
struct BehaviorFrame {
    root: BehaviorNode,
    focus: Option<Vec<usize>>,
    scroll_roots: Vec<(Vec<usize>, ScrollState)>,
}

#[derive(Debug, PartialEq)]
struct BehaviorNode {
    kind: DocumentNodeKind,
    text: Option<TextValue>,
    style: StyleMap,
    map_viewport: Option<MapViewportDescriptor>,
    embedded_program: Option<EmbeddedProgramDescriptor>,
    source_bindings: Vec<(String, String)>,
    has_text_input: bool,
    activation_focus: Option<(Vec<usize>, u64, u64)>,
    scroll: Option<ScrollState>,
    materialized: Vec<MaterializedRange>,
    children: Vec<BehaviorNode>,
}

fn counter_source() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/counter.bn")
        .canonicalize()
        .expect("Counter source path")
}

fn novywave_source() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/novywave/RUN.bn")
        .canonicalize()
        .expect("NovyWave source path")
}

fn start(
    plan: MachinePlan,
) -> Result<(MachineInstance, DocumentRuntime), Box<dyn std::error::Error>> {
    let mut session = MachineInstance::new(plan, SessionOptions::default())?;
    let document = DocumentRuntime::new(&mut session)?.expect("document runtime");
    Ok((session, document))
}

fn apply_source(
    session: &mut MachineInstance,
    document: &mut DocumentRuntime,
    sequence: u64,
    path: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let source = session
        .plan()
        .source_routes
        .iter()
        .find(|route| route.path == path)
        .unwrap_or_else(|| panic!("missing source route `{path}`"))
        .source_id;
    let event = SourceEvent {
        sequence,
        route: session.source_route_token(source, &[])?,
        source,
        target: None,
        payload: SourcePayload::default(),
    };
    let turn = session.apply(event)?;
    document.apply_turn(session, &turn.deltas)?;
    session.settle_turn();
    Ok(())
}

fn frame_paths(frame: &DocumentFrame) -> BTreeMap<DocumentNodeId, Vec<usize>> {
    fn visit(
        frame: &DocumentFrame,
        id: &DocumentNodeId,
        path: Vec<usize>,
        paths: &mut BTreeMap<DocumentNodeId, Vec<usize>>,
    ) {
        assert!(
            paths.insert(id.clone(), path.clone()).is_none(),
            "document frame repeats node {}",
            id.0
        );
        let node = frame
            .nodes
            .get(id)
            .unwrap_or_else(|| panic!("document frame omits node {}", id.0));
        for (index, child) in node.children.iter().enumerate() {
            let mut child_path = path.clone();
            child_path.push(index);
            visit(frame, child, child_path, paths);
        }
    }

    let mut paths = BTreeMap::new();
    visit(frame, &frame.root, Vec::new(), &mut paths);
    assert_eq!(paths.len(), frame.nodes.len(), "document frame has orphans");
    paths
}

fn behavior_frame(frame: &DocumentFrame) -> BehaviorFrame {
    fn normalize_node(
        frame: &DocumentFrame,
        id: &DocumentNodeId,
        input_paths: &BTreeMap<String, Vec<usize>>,
    ) -> BehaviorNode {
        let node = &frame.nodes[id];
        BehaviorNode {
            kind: node.kind.clone(),
            text: node.text.clone(),
            style: node.style.clone(),
            map_viewport: node.map_viewport.as_deref().cloned(),
            embedded_program: node.embedded_program.clone(),
            source_bindings: node
                .source_bindings
                .iter()
                .map(|binding| (binding.source_path.clone(), binding.intent.clone()))
                .collect(),
            has_text_input: node.text_input_id.is_some(),
            activation_focus: node.activation_focus.as_ref().map(|focus| {
                (
                    input_paths
                        .get(&focus.input_id.0)
                        .unwrap_or_else(|| {
                            panic!("focus references missing input {}", focus.input_id.0)
                        })
                        .clone(),
                    focus.line,
                    focus.column,
                )
            }),
            scroll: node.scroll,
            materialized: node.materialized.clone(),
            children: node
                .children
                .iter()
                .map(|child| normalize_node(frame, child, input_paths))
                .collect(),
        }
    }

    let paths = frame_paths(frame);
    let input_paths = frame
        .nodes
        .values()
        .filter_map(|node| {
            node.text_input_id
                .as_ref()
                .map(|input| (input.0.clone(), paths[&node.id].clone()))
        })
        .collect::<BTreeMap<_, _>>();
    let focus = frame.focus.as_ref().map(|focus| paths[focus].clone());
    let mut scroll_roots = frame
        .scroll_roots
        .iter()
        .map(|(root, scroll)| {
            let node = DocumentNodeId(root.0.clone());
            (paths[&node].clone(), *scroll)
        })
        .collect::<Vec<_>>();
    scroll_roots.sort_by(|left, right| left.0.cmp(&right.0));
    BehaviorFrame {
        root: normalize_node(frame, &frame.root, &input_paths),
        focus,
        scroll_roots,
    }
}

fn assert_same_observation(
    retained_session: &MachineInstance,
    retained_document: &DocumentRuntime,
    flat_session: &MachineInstance,
    flat_document: &DocumentRuntime,
) -> Result<(), Box<dyn std::error::Error>> {
    assert_eq!(
        behavior_frame(retained_document.frame()),
        behavior_frame(flat_document.frame())
    );
    let retained_snapshot: Snapshot = retained_session.snapshot()?;
    let flat_snapshot: Snapshot = flat_session.snapshot()?;
    assert_eq!(retained_snapshot, flat_snapshot);
    Ok(())
}

fn assert_same_contract(pair: &ArtifactOraclePlanPair) -> Result<(), Box<dyn std::error::Error>> {
    assert_eq!(pair.retained.persistence, pair.flat_specialized.persistence);
    assert_eq!(pair.retained.effects, pair.flat_specialized.effects);
    assert_eq!(
        pair.retained.source_routes,
        pair.flat_specialized.source_routes
    );
    assert_eq!(pair.retained.demand, pair.flat_specialized.demand);
    assert_eq!(pair.retained.delta_plan, pair.flat_specialized.delta_plan);
    assert_eq!(
        machine_plan_stable_contract_sha256(&pair.retained)?,
        machine_plan_stable_contract_sha256(&pair.flat_specialized)?,
    );
    let retained_hash = plan_sha256(&pair.retained)?;
    let flat_hash = plan_sha256(&pair.flat_specialized)?;
    assert_ne!(
        retained_hash, flat_hash,
        "oracle must compare independently lowered representations"
    );
    Ok(())
}

#[test]
fn retained_counter_matches_flat_specialization_contract_and_behavior()
-> Result<(), Box<dyn std::error::Error>> {
    let source = counter_source();
    let pair = compile_artifact_oracle_pair(CompileRequest::source_path(
        &source,
        TargetProfile::SoftwareBounded,
        ProgramRole::Client,
        ApplicationIdentity::compiler_default(),
    ))?;

    assert_same_contract(&pair)?;

    let (mut retained_session, mut retained_document) = start(pair.retained)?;
    let (mut flat_session, mut flat_document) = start(pair.flat_specialized)?;
    assert_same_observation(
        &retained_session,
        &retained_document,
        &flat_session,
        &flat_document,
    )?;

    for (sequence, path) in [
        (1, "store.sources.increment_button.events.press"),
        (2, "store.sources.increment_button.events.press"),
        (3, "store.sources.decrement_button.events.press"),
        (4, "store.sources.reset_button.events.press"),
    ] {
        apply_source(
            &mut retained_session,
            &mut retained_document,
            sequence,
            path,
        )?;
        apply_source(&mut flat_session, &mut flat_document, sequence, path)?;
        assert_same_observation(
            &retained_session,
            &retained_document,
            &flat_session,
            &flat_document,
        )?;
    }
    Ok(())
}

#[test]
fn retained_variant_persistence_matches_flat_specialization_and_keeps_widest()
-> Result<(), Box<dyn std::error::Error>> {
    let source = r#"
store: [
    sources: [
        grow: SOURCE
        shrink: SOURCE
    ]

    selected_value_column_width_key:
        Normal |> HOLD selected_value_column_width_key {
            LATEST {
                sources.grow |> THEN {
                    grow_width(width: selected_value_column_width_key)
                }
                sources.shrink |> THEN {
                    shrink_width(width: selected_value_column_width_key)
                }
            }
        }
]

FUNCTION grow_width(width) {
    width |> WHEN {
        Compact => Normal
        Normal => Wide
        Wide => Widest
        __ => Widest
    }
}

FUNCTION shrink_width(width) {
    width |> WHEN {
        Widest => Wide
        Wide => Normal
        Normal => Compact
        __ => Compact
    }
}

FUNCTION labeled(value) {
    Element/label(element: [], style: [], label: value)
}

FUNCTION width_label(width) {
    width |> WHEN {
        Compact => TEXT { Compact }
        Normal => TEXT { Normal }
        Wide => TEXT { Wide }
        Widest => TEXT { Widest }
    }
}

document: Document/new(
    root: labeled(value: width_label(width: store.selected_value_column_width_key))
)
"#;
    let pair = compile_artifact_oracle_pair(CompileRequest::source_text(
        "variant-persistence-oracle.bn",
        source,
        TargetProfile::SoftwareBounded,
        ProgramRole::Client,
        ApplicationIdentity::compiler_default(),
    ))?;
    assert_same_contract(&pair)?;

    let width = pair
        .retained
        .persistence
        .memory
        .iter()
        .find(|memory| memory.semantic_path == "store.selected_value_column_width_key")
        .expect("width persistence memory");
    let DataTypePlan::Variant { variants } = &width.data_type else {
        panic!(
            "width persistence type is not a variant: {:?}",
            width.data_type
        );
    };
    assert_eq!(
        variants
            .iter()
            .map(|variant| variant.tag.as_str())
            .collect::<Vec<_>>(),
        ["Compact", "Normal", "Wide", "Widest"]
    );

    let (mut retained_session, mut retained_document) = start(pair.retained)?;
    let (mut flat_session, mut flat_document) = start(pair.flat_specialized)?;
    assert_same_observation(
        &retained_session,
        &retained_document,
        &flat_session,
        &flat_document,
    )?;
    for (sequence, path) in [
        (1, "store.sources.grow"),
        (2, "store.sources.grow"),
        (3, "store.sources.grow"),
        (4, "store.sources.shrink"),
    ] {
        apply_source(
            &mut retained_session,
            &mut retained_document,
            sequence,
            path,
        )?;
        apply_source(&mut flat_session, &mut flat_document, sequence, path)?;
        assert_same_observation(
            &retained_session,
            &retained_document,
            &flat_session,
            &flat_document,
        )?;
    }
    Ok(())
}

#[test]
fn contextual_row_sources_do_not_publish_without_their_own_event()
-> Result<(), Box<dyn std::error::Error>> {
    let source = r#"
store: [
    sources: [
        load: SOURCE
    ]
    version:
        Before |> HOLD version {
            sources.load |> THEN { After }
        }
    base_rows: LIST {
        [
            key: version |> WHEN {
                Before => TEXT { A }
                After => TEXT { A2 }
            }
        ]
        [
            key: version |> WHEN {
                Before => TEXT { B }
                After => TEXT { B2 }
            }
        ]
        [
            key: version |> WHEN {
                Before => TEXT { C }
                After => TEXT { C2 }
            }
        ]
    }
    rows:
        base_rows
        |> List/map(item, new: selectable(provided_key: item.key))
    selected:
        rows
        |> List/map(item, new:
            item.elements.select |> THEN { item.key }
        )
        |> List/latest()
    selected_key:
        TEXT { none } |> HOLD selected_key {
            selected
        }
]

FUNCTION selectable(provided_key) {
    [
        elements: [select: SOURCE]
        key: provided_key
    ]
}

document: Document/new(
    root: Element/label(element: [], style: [], label: TEXT { oracle })
)
"#;
    let pair = compile_artifact_oracle_pair(CompileRequest::source_text(
        "contextual-row-source-oracle.bn",
        source,
        TargetProfile::SoftwareBounded,
        ProgramRole::Client,
        ApplicationIdentity::compiler_default(),
    ))?;
    assert_eq!(
        machine_plan_stable_contract_sha256(&pair.retained)?,
        machine_plan_stable_contract_sha256(&pair.flat_specialized)?,
    );

    for plan in [pair.retained, pair.flat_specialized] {
        if std::env::var_os("BOON_ORACLE_DUMP").is_some() {
            eprintln!("source routes: {:#?}", plan.source_routes);
            eprintln!("debug map: {:#?}", plan.debug_map);
            eprintln!("regions: {:#?}", plan.regions);
            eprintln!(
                "row expressions: {:#?}",
                plan.row_expressions.iter().collect::<Vec<_>>()
            );
        }
        let (mut session, _) = start(plan)?;
        assert_eq!(
            session.root_value_current("selected_key")?,
            boon_plan_executor::Value::Text("none".to_owned())
        );
        let source = session
            .plan()
            .source_routes
            .iter()
            .find(|route| route.path == "store.sources.load")
            .expect("load source route")
            .source_id;
        let event = SourceEvent {
            sequence: 1,
            route: session.source_route_token(source, &[])?,
            source,
            target: None,
            payload: SourcePayload::default(),
        };
        session.apply(event)?;
        session.settle_turn();
        assert_eq!(
            session.root_value_current("selected_key")?,
            boon_plan_executor::Value::Text("none".to_owned()),
            "load event falsely published a contextual row source"
        );
    }
    Ok(())
}

#[test]
#[ignore = "full NovyWave oracle preflight; run explicitly with the optimized test profile"]
fn retained_novywave_matches_flat_specialization_contract_and_persistence_type()
-> Result<(), Box<dyn std::error::Error>> {
    let source = novywave_source();
    let pair = compile_artifact_oracle_pair(CompileRequest::source_path(
        &source,
        TargetProfile::SoftwareBounded,
        ProgramRole::Client,
        ApplicationIdentity::compiler_default(),
    ))?;
    assert_same_contract(&pair)?;

    for (path, expected) in [
        (
            "store.real_hierarchy_signal_rows",
            ListActivationMode::MaterializedAuthority,
        ),
        ("store.signal_catalog", ListActivationMode::DerivedDefault),
        (
            "store.selected_signal_defaults",
            ListActivationMode::DerivedDefault,
        ),
    ] {
        let memory = pair
            .retained
            .persistence
            .lists
            .iter()
            .find(|memory| memory.semantic_path == path)
            .unwrap_or_else(|| panic!("missing NovyWave activation memory `{path}`"));
        let list_id = pair
            .retained
            .storage_layout
            .list_slots
            .iter()
            .find(|slot| slot.id == memory.runtime_slot)
            .unwrap_or_else(|| panic!("missing NovyWave runtime list for `{path}`"))
            .list_id;
        assert_eq!(
            pair.retained
                .list_dataflow
                .iter()
                .find(|entry| entry.list_id == list_id)
                .unwrap_or_else(|| panic!("missing NovyWave list dataflow for `{path}`"))
                .activation_mode,
            expected,
            "NovyWave activation ownership changed for `{path}`"
        );
    }
    assert!(
        pair.retained
            .persistence
            .lists
            .iter()
            .all(|memory| memory.semantic_path != "store.variable_rows"),
        "pure List/chunk view entered NovyWave persistence"
    );
    for (path, expected_fields) in [
        (
            "store.signal_catalog",
            BTreeSet::from([
                "store.signal_catalog.alias",
                "store.signal_catalog.selected_once",
            ]),
        ),
        (
            "store.selected_signal_defaults",
            BTreeSet::from([
                "store.selected_signal_defaults.format_dropdown_state",
                "store.selected_signal_defaults.formatter",
            ]),
        ),
    ] {
        let memory = pair
            .retained
            .persistence
            .lists
            .iter()
            .find(|memory| memory.semantic_path == path)
            .unwrap_or_else(|| panic!("missing NovyWave overlay memory `{path}`"));
        assert_eq!(
            memory
                .row_fields
                .iter()
                .map(|field| field.semantic_path.as_str())
                .collect::<BTreeSet<_>>(),
            expected_fields,
            "NovyWave derived view `{path}` persisted computed row values"
        );
    }

    let width = pair
        .retained
        .persistence
        .memory
        .iter()
        .find(|memory| memory.semantic_path == "store.selected_value_column_width_key")
        .expect("NovyWave selected-value width memory");
    let DataTypePlan::Variant { variants } = &width.data_type else {
        panic!(
            "NovyWave width type is not a variant: {:?}",
            width.data_type
        );
    };
    assert_eq!(
        variants
            .iter()
            .map(|variant| variant.tag.as_str())
            .collect::<Vec<_>>(),
        ["Compact", "Normal", "Wide", "Widest"]
    );

    // NovyWave's standalone Client plan deliberately starts with host-owned
    // values privately absent, so a product behavior trace must install the
    // real host fixture first. Counter and the focused width fixture above own
    // bounded runtime differential coverage; the later oracle report owns the
    // complete NovyWave host scenario rather than manufacturing defaults here.
    Ok(())
}
