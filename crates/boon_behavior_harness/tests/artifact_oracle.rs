#![cfg(feature = "test-flat-oracle")]

use boon_behavior_harness::{BehaviorAsset, BehaviorEffectTranscript, BehaviorHarness};
use boon_compiler::{CompileRequest, CompilerSourceUnit, compile_artifact_oracle_pair};
use boon_document::{
    DocumentFrame, DocumentNodeId, DocumentNodeKind, EmbeddedProgramDescriptor,
    MapViewportDescriptor, MaterializedRange, ScrollState, StyleMap, TextValue,
};
use boon_plan::{
    ApplicationIdentity, ProgramRole, TargetProfile, machine_plan_stable_contract_sha256,
    plan_sha256,
};
use boon_runtime::{ExampleManifestEntry, Scenario, ScenarioStep};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq)]
struct BehaviorFrame {
    root: BehaviorNode,
    focus: Option<Vec<usize>>,
    scroll_roots: Vec<(Vec<usize>, ScrollState)>,
}

#[derive(Clone, Debug, PartialEq)]
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

struct OracleRuntime {
    harness: BehaviorHarness,
    plan: Arc<boon_plan::MachinePlan>,
}

#[derive(Clone, Debug)]
struct OracleObservation {
    behavior: BehaviorFrame,
    semantic: boon_persistence::RestoreImage,
    store_epoch: u64,
    turn_trace: Vec<boon_behavior_harness::BehaviorTurnTrace>,
}

impl OracleRuntime {
    fn new_recording(
        plan: boon_plan::MachinePlan,
        assets: &[BehaviorAsset],
    ) -> Result<Self, String> {
        let plan = Arc::new(plan);
        let harness = BehaviorHarness::new(
            Arc::clone(&plan),
            assets,
            workspace_root().join("target/boon-behavior-harness"),
        )?;
        Ok(Self { harness, plan })
    }

    fn new_replaying(
        plan: boon_plan::MachinePlan,
        transcript: BehaviorEffectTranscript,
    ) -> Result<Self, String> {
        let plan = Arc::new(plan);
        let harness = BehaviorHarness::replay(Arc::clone(&plan), transcript)?;
        Ok(Self { harness, plan })
    }

    fn dispatch_step(&mut self, step: &ScenarioStep) -> Result<(), String> {
        self.harness.dispatch_scenario_step(step)
    }

    fn assert_step(&mut self, step: &ScenarioStep) -> Result<(), String> {
        self.harness.assert_scenario_step(step)
    }

    fn behavior(&self) -> BehaviorFrame {
        behavior_frame(self.harness.frame())
    }

    fn observation(&self) -> Result<OracleObservation, String> {
        let mut semantic = self.harness.semantic_value_image()?;
        let store_epoch = semantic.epoch;
        semantic.epoch = 0;
        Ok(OracleObservation {
            behavior: self.behavior(),
            semantic,
            store_epoch,
            turn_trace: self.harness.turn_trace().to_vec(),
        })
    }

    fn diagnostic_roots(&mut self) -> Vec<(String, String)> {
        [
            "active_signal",
            "real_first_signal_id",
            "row_selected_signal_key",
            "startup_primary_signal",
        ]
        .into_iter()
        .map(|path| {
            (
                path.to_owned(),
                self.harness
                    .inspect_root_current(path)
                    .unwrap_or_else(|error| format!("<error: {error}>")),
            )
        })
        .collect()
    }
}

fn assert_matches_observation(
    expected: &OracleObservation,
    runtime: &OracleRuntime,
) -> Result<(), String> {
    let actual = runtime.observation()?;
    if expected.behavior != actual.behavior {
        return Err(format!(
            "document frame differs: {}",
            first_behavior_difference(&expected.behavior, &actual.behavior)
        ));
    }
    if expected.semantic != actual.semantic {
        return Err(format!(
            "semantic image differs after normalizing only store-local epoch {}/{}: {}",
            expected.store_epoch,
            actual.store_epoch,
            first_restore_difference(&runtime.plan, &expected.semantic, &actual.semantic),
        ));
    }
    if expected.turn_trace != actual.turn_trace {
        let index = expected
            .turn_trace
            .iter()
            .zip(&actual.turn_trace)
            .position(|(left, right)| left != right)
            .unwrap_or_else(|| expected.turn_trace.len().min(actual.turn_trace.len()));
        return Err(format!(
            "runtime turn trace differs at index {index}: expected={:?} actual={:?}",
            expected.turn_trace.get(index),
            actual.turn_trace.get(index),
        ));
    }
    Ok(())
}

fn assert_same_behavior(left: &OracleRuntime, right: &OracleRuntime) -> Result<(), String> {
    let left_behavior = left.behavior();
    let right_behavior = right.behavior();
    if left_behavior != right_behavior {
        return Err(format!(
            "retained and flat NovyWave document frames differ: {}",
            first_behavior_difference(&left_behavior, &right_behavior)
        ));
    }
    let mut left_state = left.harness.semantic_value_image()?;
    let mut right_state = right.harness.semantic_value_image()?;
    let left_epoch = left_state.epoch;
    let right_epoch = right_state.epoch;
    left_state.epoch = 0;
    right_state.epoch = 0;
    if left_state != right_state {
        let first_difference = first_restore_difference(&left.plan, &left_state, &right_state);
        let first_turn_difference = left
            .harness
            .turn_trace()
            .iter()
            .zip(right.harness.turn_trace())
            .position(|(left, right)| left != right)
            .map(|index| {
                let start = index.saturating_sub(2);
                let retained_end = (index + 10).min(left.harness.turn_trace().len());
                let flat_end = (index + 10).min(right.harness.turn_trace().len());
                format!(
                    "index={index} retained={:?} flat={:?} retained_tail={:?} flat_tail={:?}",
                    left.harness.turn_trace()[index],
                    right.harness.turn_trace()[index],
                    &left.harness.turn_trace()[start..retained_end],
                    &right.harness.turn_trace()[start..flat_end],
                )
            })
            .unwrap_or_else(|| {
                format!(
                    "common_prefix={} retained_len={} flat_len={}",
                    left.harness
                        .turn_trace()
                        .len()
                        .min(right.harness.turn_trace().len()),
                    left.harness.turn_trace().len(),
                    right.harness.turn_trace().len(),
                )
            });
        return Err(format!(
            "retained and flat NovyWave semantic state images differ after normalizing only the store-local epoch; epochs={left_epoch}/{right_epoch} turns={}/{} scalars={}/{} lists={}/{} maps={}/{} sets={}/{} outbox={}/{}; first={first_difference}; turn_trace={first_turn_difference}",
            left_state.through_turn_sequence,
            right_state.through_turn_sequence,
            left_state.scalars.len(),
            right_state.scalars.len(),
            left_state.lists.len(),
            right_state.lists.len(),
            left_state.maps.len(),
            right_state.maps.len(),
            left_state.sets.len(),
            right_state.sets.len(),
            left_state.outbox.len(),
            right_state.outbox.len(),
        ));
    }
    Ok(())
}

fn first_behavior_difference(left: &BehaviorFrame, right: &BehaviorFrame) -> String {
    if left.focus != right.focus {
        return format!("focus retained={:?} flat={:?}", left.focus, right.focus);
    }
    if left.scroll_roots != right.scroll_roots {
        return format!(
            "scroll roots retained={:?} flat={:?}",
            left.scroll_roots, right.scroll_roots
        );
    }
    fn visit(left: &BehaviorNode, right: &BehaviorNode, path: &mut Vec<usize>) -> Option<String> {
        macro_rules! compare {
            ($field:ident) => {
                if left.$field != right.$field {
                    return Some(format!(
                        "node {path:?} {} retained={:?} flat={:?}",
                        stringify!($field),
                        left.$field,
                        right.$field
                    ));
                }
            };
        }
        compare!(kind);
        compare!(text);
        compare!(style);
        compare!(map_viewport);
        compare!(embedded_program);
        compare!(source_bindings);
        compare!(has_text_input);
        compare!(activation_focus);
        compare!(scroll);
        compare!(materialized);
        if left.children.len() != right.children.len() {
            return Some(format!(
                "node {path:?} child count retained={} flat={}",
                left.children.len(),
                right.children.len()
            ));
        }
        for (index, (left, right)) in left.children.iter().zip(&right.children).enumerate() {
            path.push(index);
            if let Some(difference) = visit(left, right, path) {
                return Some(difference);
            }
            path.pop();
        }
        None
    }
    visit(&left.root, &right.root, &mut Vec::new())
        .unwrap_or_else(|| "unclassified frame difference".to_owned())
}

fn first_restore_difference(
    plan: &boon_plan::MachinePlan,
    left: &boon_persistence::RestoreImage,
    right: &boon_persistence::RestoreImage,
) -> String {
    for memory_id in left
        .scalars
        .keys()
        .chain(right.scalars.keys())
        .collect::<std::collections::BTreeSet<_>>()
    {
        let left_value = left.scalars.get(memory_id);
        let right_value = right.scalars.get(memory_id);
        if left_value != right_value {
            let path = plan
                .persistence
                .memory
                .iter()
                .find(|memory| memory.memory_id == *memory_id)
                .map(|memory| memory.semantic_path.as_str())
                .unwrap_or("<unknown-scalar>");
            return format!(
                "scalar {path} ({memory_id:?}) left={left_value:?} right={right_value:?}"
            );
        }
    }
    for memory_id in left
        .lists
        .keys()
        .chain(right.lists.keys())
        .collect::<std::collections::BTreeSet<_>>()
    {
        let left_value = left.lists.get(memory_id);
        let right_value = right.lists.get(memory_id);
        if left_value != right_value {
            let path = plan
                .persistence
                .lists
                .iter()
                .find(|memory| memory.memory_id == *memory_id)
                .map(|memory| memory.semantic_path.as_str())
                .unwrap_or("<unknown-list>");
            if let (Some(left), Some(right)) = (left_value, right_value) {
                let first_row_difference = left
                    .rows
                    .iter()
                    .zip(&right.rows)
                    .position(|(left, right)| left != right)
                    .map(|index| {
                        format!(
                            "index={index} retained={}:{} flat={}:{}",
                            left.rows[index].key,
                            left.rows[index].generation,
                            right.rows[index].key,
                            right.rows[index].generation,
                        )
                    })
                    .unwrap_or_else(|| "none-in-common-prefix".to_owned());
                return format!(
                    "list {path} retained=(touched={} revision={} rows={} next_key={} next_order={}) flat=(touched={} revision={} rows={} next_key={} next_order={}) first_row={first_row_difference}",
                    left.touched,
                    left.revision,
                    left.rows.len(),
                    left.next_key,
                    left.next_order_token,
                    right.touched,
                    right.revision,
                    right.rows.len(),
                    right.next_key,
                    right.next_order_token,
                );
            }
            return format!(
                "list {path} presence retained={} flat={}",
                left_value.is_some(),
                right_value.is_some()
            );
        }
    }
    for (label, equal) in [
        ("application", left.application == right.application),
        (
            "schema_version",
            left.schema_version == right.schema_version,
        ),
        ("schema_hash", left.schema_hash == right.schema_hash),
        (
            "through_turn_sequence",
            left.through_turn_sequence == right.through_turn_sequence,
        ),
        ("maps", left.maps == right.maps),
        ("sets", left.sets == right.sets),
        (
            "completed_migration_edges",
            left.completed_migration_edges == right.completed_migration_edges,
        ),
        ("outbox", left.outbox == right.outbox),
        (
            "content_artifact_manifest",
            left.content_artifact_manifest == right.content_artifact_manifest,
        ),
    ] {
        if !equal {
            return label.to_owned();
        }
    }
    "unclassified restore-image difference".to_owned()
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

#[test]
fn retained_and_flat_share_one_real_host_effect_transcript() -> Result<(), String> {
    let source = r#"
store: [
    fire: SOURCE
    random:
        NotRequested |> HOLD random {
            fire |> THEN { Random/bytes(byte_count: 8) }
        }
]

document: Document/new(
    root: Element/label(
        element: [events: [press: store.fire]]
        label: TEXT { Generate }
    )
)
"#;
    let pair = compile_artifact_oracle_pair(CompileRequest::source_text(
        "effect-transcript-oracle.bn",
        source,
        TargetProfile::SoftwareBounded,
        ProgramRole::Client,
        ApplicationIdentity::new("dev.boon.effect-transcript", "test", "local"),
    ))
    .map_err(|error| error.to_string())?;
    let step = ScenarioStep {
        id: "generate".to_owned(),
        user_action_kind: Some("click".to_owned()),
        user_action_text: Some("Generate".to_owned()),
        user_action_key: None,
        source_event: Some(boon_runtime::ScenarioSourceEvent {
            source: "store.fire".to_owned(),
            target_list: None,
            target_key: None,
            target_generation: None,
            target_text: Some("Generate".to_owned()),
            target_occurrence: None,
            payload: boon_runtime::SourcePayload::default(),
        }),
        expectations: Vec::new(),
    };

    let mut retained = OracleRuntime::new_recording(pair.retained, &[])?;
    retained.dispatch_step(&step)?;
    let expected = retained.observation()?;
    let transcript = retained.harness.recorded_effect_transcript()?;
    if transcript.is_empty() {
        return Err("real-host effect transcript unexpectedly contains no events".to_owned());
    }

    let mut flat = OracleRuntime::new_replaying(pair.flat_specialized, transcript)?;
    flat.dispatch_step(&step)?;
    flat.harness.assert_effect_replay_consumed()?;
    assert_matches_observation(&expected, &flat)
}

#[test]
#[ignore = "full real-host NovyWave artifact oracle; run explicitly with the optimized test profile"]
fn retained_novywave_matches_flat_real_host_scenario_and_restart() -> Result<(), String> {
    let example = load_novywave()?;
    let pair = compile_artifact_oracle_pair(CompileRequest::source_units(
        &example.entry_path,
        &example.units,
        TargetProfile::SoftwareBounded,
        ProgramRole::Client,
        example.application.clone(),
    ))
    .map_err(|error| error.to_string())?;

    let retained_contract =
        machine_plan_stable_contract_sha256(&pair.retained).map_err(|error| error.to_string())?;
    let flat_contract = machine_plan_stable_contract_sha256(&pair.flat_specialized)
        .map_err(|error| error.to_string())?;
    if retained_contract != flat_contract {
        return Err("NovyWave stable contracts differ before host execution".to_owned());
    }
    let retained_hash = plan_sha256(&pair.retained).map_err(|error| error.to_string())?;
    let flat_hash = plan_sha256(&pair.flat_specialized).map_err(|error| error.to_string())?;
    if retained_hash == flat_hash {
        return Err("NovyWave oracle representations unexpectedly have one full hash".to_owned());
    }

    let retained_plan = pair.retained.clone();
    let flat_plan = pair.flat_specialized.clone();
    let mut retained = OracleRuntime::new_recording(pair.retained, &example.assets)?;
    let mut observations = vec![retained.observation()?];
    for step in &example.scenario.steps {
        retained.dispatch_step(step)?;
        let retained_assertion = retained.assert_step(step);
        if let Err(error) = retained_assertion {
            let retained_roots = retained.diagnostic_roots();
            return Err(format!(
                "step `{}` retained authored scenario mismatch: {error}; roots={retained_roots:?}",
                step.id,
            ));
        }
        observations.push(retained.observation()?);
    }
    retained.harness.settle()?;
    let scenario_transcript = retained.harness.recorded_effect_transcript()?;

    let mut flat =
        OracleRuntime::new_replaying(pair.flat_specialized, scenario_transcript.clone())?;
    assert_matches_observation(&observations[0], &flat)?;
    for (step, expected) in example.scenario.steps.iter().zip(&observations[1..]) {
        flat.dispatch_step(step)?;
        assert_matches_observation(expected, &flat)
            .map_err(|error| format!("step `{}`: {error}", step.id))?;
        if let Err(error) = flat.assert_step(step) {
            let flat_roots = flat.diagnostic_roots();
            return Err(format!(
                "step `{}` flat replay authored scenario mismatch: {error}; roots={flat_roots:?}",
                step.id,
            ));
        }
    }
    flat.harness.settle()?;
    flat.harness.assert_effect_replay_consumed()?;
    assert_same_behavior(&retained, &flat)?;
    let retained_artifact = retained.harness.export_state_artifact()?;
    let flat_artifact = flat.harness.export_state_artifact()?;
    let retained_transfer = boon_persistence::decode_application_transfer(
        &retained_artifact,
        boon_persistence::DecodeLimits::default(),
    )
    .map_err(|error| error.to_string())?;
    let flat_transfer = boon_persistence::decode_application_transfer(
        &flat_artifact,
        boon_persistence::DecodeLimits::default(),
    )
    .map_err(|error| error.to_string())?;
    let mut retained_semantic_transfer = retained_transfer.clone();
    let mut flat_semantic_transfer = flat_transfer.clone();
    retained_semantic_transfer.restore_image.epoch = 0;
    flat_semantic_transfer.restore_image.epoch = 0;
    if retained_semantic_transfer != flat_semantic_transfer {
        return Err(format!(
            "retained and flat NovyWave state artifacts differ after normalizing only the store-local epoch; retained_epoch={} flat_epoch={} retained_turn={} flat_turn={} retained_lists={:?} flat_lists={:?}",
            retained_transfer.restore_image.epoch,
            flat_transfer.restore_image.epoch,
            retained_transfer.restore_image.through_turn_sequence,
            flat_transfer.restore_image.through_turn_sequence,
            artifact_list_summary(&retained_plan, &retained_transfer),
            artifact_list_summary(&flat_plan, &flat_transfer),
        ));
    }
    eprintln!(
        "artifact lists: {:?}",
        artifact_list_summary(&retained_plan, &retained_transfer)
    );
    let retained_semantic = retained.harness.semantic_value_image()?;
    eprintln!(
        "retained live semantic lists: {:?}",
        artifact_list_summary_from_image(&retained_plan, &retained_semantic)
    );
    eprintln!(
        "retained persistent dataflow: {:?}",
        persistent_dataflow_summary(&retained_plan)
    );
    eprintln!(
        "flat persistent dataflow: {:?}",
        persistent_dataflow_summary(&flat_plan)
    );

    let baseline = flat.harness.semantic_value_image()?;
    let mut corrupt = retained_artifact.clone();
    let last = corrupt
        .last_mut()
        .ok_or_else(|| "NovyWave state artifact is empty".to_owned())?;
    *last ^= 0x5a;
    if flat.harness.preview_state_artifact(&corrupt).is_ok() {
        return Err("corrupt NovyWave state artifact was accepted".to_owned());
    }
    if flat.harness.semantic_value_image()? != baseline {
        return Err("corrupt NovyWave import changed live semantic state".to_owned());
    }

    let mut activated_retained =
        OracleRuntime::new_recording(retained_plan.clone(), &example.assets)?;
    activated_retained
        .harness
        .activate_state_artifact(&retained_artifact)
        .map_err(|error| format!("activate retained artifact in retained runtime: {error}"))?;
    let activation_transcript = activated_retained.harness.recorded_effect_transcript()?;
    let mut activated_flat =
        OracleRuntime::new_replaying(flat_plan.clone(), activation_transcript.clone())?;
    activated_flat
        .harness
        .activate_state_artifact(&retained_artifact)
        .map_err(|error| format!("activate retained artifact after flat restart: {error}"))?;
    activated_flat.harness.assert_effect_replay_consumed()?;
    assert_same_behavior(&retained, &activated_retained)
        .map_err(|error| format!("retained artifact restart changed behavior: {error}"))?;
    assert_same_behavior(&activated_retained, &activated_flat)
        .map_err(|error| format!("flat restart from retained artifact: {error}"))?;

    let migrated = activated_flat.harness.export_state_artifact()?;
    let mut restarted_retained = OracleRuntime::new_recording(retained_plan, &example.assets)?;
    restarted_retained
        .harness
        .activate_state_artifact(&migrated)
        .map_err(|error| format!("activate migrated artifact after retained restart: {error}"))?;
    let restart_transcript = restarted_retained.harness.recorded_effect_transcript()?;
    let mut restarted_flat = OracleRuntime::new_replaying(flat_plan, restart_transcript.clone())?;
    restarted_flat
        .harness
        .activate_state_artifact(&migrated)
        .map_err(|error| format!("activate migrated artifact after flat restart: {error}"))?;
    restarted_flat.harness.assert_effect_replay_consumed()?;
    assert_same_behavior(&restarted_retained, &restarted_flat)
        .map_err(|error| format!("retained/flat restart from migrated artifact: {error}"))?;

    eprintln!(
        "novywave behavior artifact oracle: steps={} stable_contract={} retained_plan={} flat_plan={} state_artifact_bytes={} scenario_effect_events={} activation_effect_events={} restart_effect_events={}",
        example.scenario.steps.len(),
        retained_contract,
        retained_hash,
        flat_hash,
        retained_artifact.len(),
        scenario_transcript.event_count(),
        activation_transcript.event_count(),
        restart_transcript.event_count(),
    );
    Ok(())
}

struct LoadedNovyWave {
    application: ApplicationIdentity,
    entry_path: String,
    units: Vec<CompilerSourceUnit>,
    scenario: Scenario,
    assets: Vec<BehaviorAsset>,
}

fn load_novywave() -> Result<LoadedNovyWave, String> {
    let entry =
        boon_runtime::example_manifest_entry("novywave").map_err(|error| error.to_string())?;
    let units = boon_runtime::source_units_for_entry(&entry)
        .map_err(|error| error.to_string())?
        .into_iter()
        .map(|unit| CompilerSourceUnit {
            path: unit.path,
            source: unit.source,
        })
        .collect();
    let scenario = boon_runtime::parse_scenario(Path::new(&entry.scenario))
        .map_err(|error| error.to_string())?;
    let mut assets = entry
        .asset_files
        .iter()
        .map(|path| load_asset(&entry.id, path))
        .collect::<Result<Vec<_>, _>>()?;
    for directory in &entry.asset_directories {
        let mut paths = Vec::new();
        collect_asset_paths(&resolve_repo_path(directory), &mut paths)?;
        paths.sort();
        for path in paths {
            assets.push(load_asset(&entry.id, &path.to_string_lossy())?);
        }
    }
    assets.sort_by(|left, right| left.url.cmp(&right.url));
    Ok(LoadedNovyWave {
        application: built_in_application_identity(&entry),
        entry_path: entry.source,
        units,
        scenario,
        assets,
    })
}

fn built_in_application_identity(entry: &ExampleManifestEntry) -> ApplicationIdentity {
    let explicit = entry.application_identity();
    ApplicationIdentity::new(
        explicit.as_ref().map_or_else(
            || format!("dev.boon.example.{}", entry.id),
            |value| value.package_id.clone(),
        ),
        explicit
            .as_ref()
            .and_then(|value| value.state_namespace.clone())
            .unwrap_or_else(|| format!("builtin:example:{}", entry.id)),
        explicit.map_or_else(|| "builtin".to_owned(), |value| value.deployment_domain),
    )
}

fn load_asset(example_id: &str, path: &str) -> Result<BehaviorAsset, String> {
    let filesystem_path = resolve_repo_path(path);
    let bytes = fs::read(&filesystem_path).map_err(|error| {
        format!(
            "read behavior asset `{}`: {error}",
            filesystem_path.display()
        )
    })?;
    let relative = path
        .split_once("/assets/")
        .map(|(_, relative)| relative)
        .or_else(|| Path::new(path).file_name().and_then(|name| name.to_str()))
        .ok_or_else(|| format!("asset path `{path}` has no file name"))?;
    let extension = Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let media_type = match extension.as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "vcd" => "application/vnd.boon.waveform.vcd",
        "fst" => "application/vnd.boon.waveform.fst",
        "ghw" => "application/vnd.boon.waveform.ghw",
        extension => return Err(format!("unsupported asset extension `{extension}`: {path}")),
    };
    Ok(BehaviorAsset::new(
        format!("asset://{example_id}/{relative}"),
        media_type,
        bytes,
    ))
}

fn collect_asset_paths(directory: &Path, paths: &mut Vec<PathBuf>) -> Result<(), String> {
    for entry in fs::read_dir(directory)
        .map_err(|error| format!("read asset directory `{}`: {error}", directory.display()))?
    {
        let path = entry.map_err(|error| error.to_string())?.path();
        if path.is_dir() {
            collect_asset_paths(&path, paths)?;
        } else {
            paths.push(path);
        }
    }
    Ok(())
}

fn resolve_repo_path(path: &str) -> PathBuf {
    let candidate = PathBuf::from(path);
    if candidate.is_absolute() {
        candidate
    } else {
        workspace_root().join(candidate)
    }
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("behavior harness lives under the workspace crates directory")
        .to_path_buf()
}

fn artifact_list_summary(
    plan: &boon_plan::MachinePlan,
    transfer: &boon_persistence::ApplicationTransfer,
) -> Vec<(
    String,
    bool,
    Vec<(u64, u64, usize, Option<(String, u64, u64)>)>,
)> {
    artifact_list_summary_from_image(plan, &transfer.restore_image)
}

fn artifact_list_summary_from_image(
    plan: &boon_plan::MachinePlan,
    image: &boon_persistence::RestoreImage,
) -> Vec<(
    String,
    bool,
    Vec<(u64, u64, usize, Option<(String, u64, u64)>)>,
)> {
    image
        .lists
        .iter()
        .filter(|(_, list)| !list.rows.is_empty())
        .map(|(memory_id, list)| {
            let path = plan
                .persistence
                .lists
                .iter()
                .find(|memory| memory.memory_id == *memory_id)
                .map(|memory| memory.semantic_path.clone())
                .unwrap_or_else(|| "<unknown>".to_owned());
            (
                path,
                list.touched,
                list.rows
                    .iter()
                    .map(|row| {
                        let origin = row.materialization_origin.as_ref().and_then(|owner| {
                            owner.ancestors.last().map(|origin| {
                                let path = plan
                                    .persistence
                                    .lists
                                    .iter()
                                    .find(|memory| memory.memory_id == origin.list_memory_id)
                                    .map(|memory| memory.semantic_path.clone())
                                    .unwrap_or_else(|| "<unknown-origin>".to_owned());
                                (path, origin.row_key, origin.row_generation)
                            })
                        });
                        (row.key, row.generation, row.touched_fields.len(), origin)
                    })
                    .collect(),
            )
        })
        .collect()
}

fn persistent_dataflow_summary(
    plan: &boon_plan::MachinePlan,
) -> Vec<(
    String,
    usize,
    boon_plan::ListActivationMode,
    Option<usize>,
    Vec<usize>,
)> {
    plan.persistence
        .lists
        .iter()
        .filter_map(|memory| {
            let list_id = plan
                .storage_layout
                .list_slots
                .iter()
                .find(|slot| slot.id == memory.runtime_slot)?
                .list_id;
            let dataflow = plan
                .list_dataflow
                .iter()
                .find(|entry| entry.list_id == list_id)?;
            Some((
                memory.semantic_path.clone(),
                list_id.0,
                dataflow.activation_mode,
                dataflow.reconstruction_output.map(|list| list.0),
                dataflow.dependencies.iter().map(|list| list.0).collect(),
            ))
        })
        .collect()
}
