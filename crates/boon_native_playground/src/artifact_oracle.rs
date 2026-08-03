use crate::catalog::Catalog;
use crate::preview::test_step_pointer_position;
use crate::protocol::{AssetBlob, TestStep};
use crate::runtime_view::RuntimeView;
use crate::view::RetainedView;
use boon_compiler::{CompileRequest, CompilerSourceUnit, compile_artifact_oracle_pair};
use boon_document::render_scene::ApproximateTextColumnMeasurer;
use boon_document::{
    DocumentFrame, DocumentNodeId, DocumentNodeKind, EmbeddedProgramDescriptor,
    MapViewportDescriptor, MaterializedRange, ScrollState, StyleMap, TextValue,
};
use boon_host::{HostEvent, PointerButton, PointerEvent, PointerPhase, SurfaceId, Viewport};
use boon_plan::{ProgramRole, TargetProfile, machine_plan_stable_contract_sha256, plan_sha256};
use boon_runtime::ScenarioStep;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

const ORACLE_VIEWPORT: Viewport = Viewport {
    surface: 1,
    width: 980.0,
    height: 760.0,
    scale: 1.0,
};
const SETTLE_TIMEOUT: Duration = Duration::from_secs(2);

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

struct OracleRuntime {
    runtime: RuntimeView,
    view: RetainedView,
    columns: ApproximateTextColumnMeasurer,
    cursor: (f32, f32),
    surface: SurfaceId,
}

impl OracleRuntime {
    fn new(plan: boon_plan::MachinePlan, assets: &[AssetBlob]) -> Result<Self, String> {
        let runtime = RuntimeView::open_for_scenario_with_assets(Arc::new(plan), assets)?;
        let mut columns = ApproximateTextColumnMeasurer;
        let view = RetainedView::new(runtime.frame(), ORACLE_VIEWPORT, &mut columns)
            .map_err(|error| error.to_string())?;
        let mut oracle = Self {
            runtime,
            view,
            columns,
            cursor: (0.0, 0.0),
            surface: SurfaceId("artifact-oracle".to_owned()),
        };
        oracle.sync_document()?;
        oracle.settle()?;
        Ok(oracle)
    }

    fn dispatch_step(&mut self, step: &TestStep) -> Result<(), String> {
        if step.source_path.is_empty() {
            self.settle()?;
            return Ok(());
        }
        if step.action_kind.as_deref() != Some("click") {
            return Err(format!(
                "artifact oracle supports authored click steps, got {:?} for `{}`",
                step.action_kind, step.id
            ));
        }

        self.runtime.begin_scenario_step(&step.source_path);
        let target = self
            .view
            .target_for_scenario(
                &step.source_path,
                step.action_kind.as_deref(),
                step.target_text.as_deref(),
                step.address.as_deref(),
                step.target_key.zip(step.target_generation),
            )
            .ok_or_else(|| {
                let visible_routes = self
                    .view
                    .visible_source_action_bounds()
                    .into_iter()
                    .filter(|(path, _, _)| path == &step.source_path)
                    .collect::<Vec<_>>();
                let route_diagnostics =
                    self.view.source_action_diagnostics(&step.source_path);
                let frame = self.runtime.frame();
                let matching_text = step.target_text.as_deref().map_or_else(Vec::new, |expected| {
                    frame
                        .nodes
                        .values()
                        .filter_map(|node| {
                            node.text.as_ref().and_then(|text| {
                                (text.text == expected).then(|| {
                                    (node.id.0.clone(), self.view.node_bounds(&node.id.0))
                                })
                            })
                        })
                        .collect::<Vec<_>>()
                });
                format!(
                    "artifact oracle could not resolve visible source `{}` for step `{}`; visible matching routes={visible_routes:?}; route diagnostics={route_diagnostics:?}; matching text nodes={matching_text:?}",
                    step.source_path, step.id,
                )
            })?;
        let point = test_step_pointer_position(&self.view, &target, step);
        self.cursor = point;
        for (phase, button) in [
            (PointerPhase::Move, None),
            (PointerPhase::Down, Some(PointerButton::Primary)),
            (PointerPhase::Up, Some(PointerButton::Primary)),
        ] {
            self.runtime.handle_event(
                &HostEvent::Pointer(PointerEvent {
                    surface: self.surface.clone(),
                    x: point.0,
                    y: point.1,
                    phase,
                    button,
                }),
                Some(target.clone()),
            )?;
            self.sync_document()?;
        }
        self.settle()?;
        Ok(())
    }

    fn assert_step(&mut self, step: &TestStep) -> Result<(), String> {
        self.runtime.assert_scenario_step(&ScenarioStep {
            id: step.id.clone(),
            user_action_kind: step.action_kind.clone(),
            user_action_text: step.text.clone(),
            user_action_key: step.key.clone(),
            source_event: None,
            expectations: step.expectations.clone(),
        })
    }

    fn settle(&mut self) -> Result<(), String> {
        let started = Instant::now();
        loop {
            if started.elapsed() > SETTLE_TIMEOUT {
                return Err(format!(
                    "artifact oracle host did not settle within {}ms; effect_pending={}, persistence_pending={}",
                    SETTLE_TIMEOUT.as_millis(),
                    self.runtime.effect_poll_deadline().is_some(),
                    self.runtime.persistence_poll_deadline().is_some(),
                ));
            }

            self.sync_document()?;
            if let Some(deadline) = self.runtime.effect_poll_deadline() {
                sleep_until(deadline);
                self.runtime.poll_host_effects(Instant::now())?;
            }
            if let Some(deadline) = self.runtime.persistence_poll_deadline() {
                sleep_until(deadline);
                self.runtime
                    .poll_persistence_acknowledgement(Instant::now());
            }
            self.sync_document()?;

            if self.runtime.effect_poll_deadline().is_none()
                && self.runtime.persistence_poll_deadline().is_none()
            {
                return Ok(());
            }
        }
    }

    fn sync_document(&mut self) -> Result<(), String> {
        self.view
            .apply_patches(self.runtime.take_patches(), &mut self.columns)
            .map_err(|error| error.to_string())?;
        self.view
            .set_interaction_state(
                self.runtime.hovered(),
                self.runtime.focused(),
                &mut self.columns,
            )
            .map_err(|error| error.to_string())?;
        for _ in 0..4 {
            let demands = self.view.demands().to_vec();
            if !self.runtime.apply_layout_demands(&demands)? {
                return Ok(());
            }
            self.view
                .apply_patches(self.runtime.take_patches(), &mut self.columns)
                .map_err(|error| error.to_string())?;
        }
        Err("artifact oracle document demands did not converge in four passes".to_owned())
    }

    fn replace_view_from_runtime(&mut self) -> Result<(), String> {
        self.view
            .replace(self.runtime.frame(), ORACLE_VIEWPORT, &mut self.columns)
            .map_err(|error| error.to_string())?;
        self.sync_document()
    }

    fn behavior(&self) -> BehaviorFrame {
        behavior_frame(&self.runtime.frame())
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
                self.runtime
                    .inspect_root_current(path)
                    .unwrap_or_else(|error| format!("<error: {error}>")),
            )
        })
        .collect()
    }
}

fn sleep_until(deadline: Instant) {
    let remaining = deadline.saturating_duration_since(Instant::now());
    if !remaining.is_zero() {
        thread::sleep(remaining.min(Duration::from_millis(2)));
    }
}

fn assert_same_behavior(left: &OracleRuntime, right: &OracleRuntime) -> Result<(), String> {
    if left.behavior() != right.behavior() {
        return Err("retained and flat NovyWave document frames differ".to_owned());
    }
    let left_state = left.runtime.semantic_value_image()?;
    let right_state = right.runtime.semantic_value_image()?;
    if left_state != right_state {
        return Err("retained and flat NovyWave semantic state images differ".to_owned());
    }
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

#[test]
#[ignore = "full real-host NovyWave artifact oracle; run explicitly with the optimized test profile"]
fn retained_novywave_matches_flat_real_host_scenario_and_restart() -> Result<(), String> {
    let example = Catalog::load()
        .and_then(|catalog| catalog.open("novywave"))
        .map_err(|error| error.to_string())?;
    let units = example
        .units
        .iter()
        .map(|unit| CompilerSourceUnit {
            path: unit.path.clone(),
            source: unit.source.clone(),
        })
        .collect::<Vec<_>>();
    let pair = compile_artifact_oracle_pair(CompileRequest::source_units(
        &example.entry_path,
        &units,
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
    let mut retained = OracleRuntime::new(pair.retained, &example.assets)?;
    let mut flat = OracleRuntime::new(pair.flat_specialized, &example.assets)?;
    assert_same_behavior(&retained, &flat)?;
    for step in &example.test_steps {
        retained.dispatch_step(step)?;
        flat.dispatch_step(step)?;
        assert_same_behavior(&retained, &flat)
            .map_err(|error| format!("step `{}`: {error}", step.id))?;
        let retained_assertion = retained.assert_step(step);
        let flat_assertion = flat.assert_step(step);
        if retained_assertion.is_err() || flat_assertion.is_err() {
            let retained_roots = retained.diagnostic_roots();
            let flat_roots = flat.diagnostic_roots();
            return Err(format!(
                "step `{}` authored scenario mismatch: retained={:?} roots={retained_roots:?}; flat={:?} roots={flat_roots:?}",
                step.id,
                retained_assertion.err(),
                flat_assertion.err(),
            ));
        }
    }

    retained.settle()?;
    flat.settle()?;
    let retained_artifact = retained.runtime.export_state_artifact()?;
    let flat_artifact = flat.runtime.export_state_artifact()?;
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

    let baseline = flat.runtime.semantic_value_image()?;
    let mut corrupt = retained_artifact.clone();
    let last = corrupt
        .last_mut()
        .ok_or_else(|| "NovyWave state artifact is empty".to_owned())?;
    *last ^= 0x5a;
    if flat.runtime.preview_state_artifact(&corrupt).is_ok() {
        return Err("corrupt NovyWave state artifact was accepted".to_owned());
    }
    if flat.runtime.semantic_value_image()? != baseline {
        return Err("corrupt NovyWave import changed live semantic state".to_owned());
    }

    flat.runtime.activate_state_artifact(&retained_artifact)?;
    flat.replace_view_from_runtime()?;
    flat.settle()?;
    assert_same_behavior(&retained, &flat)?;

    let migrated = flat.runtime.export_state_artifact()?;
    let mut restarted_retained = OracleRuntime::new(retained_plan, &example.assets)?;
    restarted_retained
        .runtime
        .activate_state_artifact(&migrated)?;
    restarted_retained.replace_view_from_runtime()?;
    restarted_retained.settle()?;
    assert_same_behavior(&flat, &restarted_retained)?;

    let mut restarted_flat = OracleRuntime::new(flat_plan, &example.assets)?;
    restarted_flat
        .runtime
        .activate_state_artifact(&retained_artifact)?;
    restarted_flat.replace_view_from_runtime()?;
    restarted_flat.settle()?;
    assert_same_behavior(&retained, &restarted_flat)?;

    eprintln!(
        "novywave artifact oracle: steps={} stable_contract={} retained_plan={} flat_plan={} state_artifact_bytes={}",
        example.test_steps.len(),
        retained_contract,
        retained_hash,
        flat_hash,
        retained_artifact.len(),
    );
    Ok(())
}

fn artifact_list_summary(
    plan: &boon_plan::MachinePlan,
    transfer: &boon_persistence::ApplicationTransfer,
) -> Vec<(String, bool, Vec<(u64, u64, usize)>)> {
    transfer
        .restore_image
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
                    .map(|row| (row.key, row.generation, row.touched_fields.len()))
                    .collect(),
            )
        })
        .collect()
}
