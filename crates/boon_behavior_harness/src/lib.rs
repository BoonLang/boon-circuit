//! Product-faithful retained behavior and persistence harness without a native
//! window, GPU renderer, editor, compositor, or browser shell.

#![forbid(unsafe_code)]

use boon_document::render_scene::ApproximateTextColumnMeasurer;
use boon_document::retained_view::{HitTarget, RetainedView};
use boon_document::{DocumentFrame, DocumentNodeId, LayoutDemand};
use boon_host::Viewport;
use boon_host_runtime::{
    HostEffectRouter, HostEffectWorker, PersistentRuntime, PersistentStateArtifactPreview,
};
use boon_local_host::{LocalTransientCompletion, LocalTransientHost, PackageAsset};
use boon_persistence::{InMemoryDriver, PersistenceWorkerConfig, RestoreImage};
use boon_plan::{EffectReplay, ExactNumber, MachinePlan, SourceRouteToken};
use boon_runtime::{
    RuntimeTurn, ScenarioSourceEvent, ScenarioStep, SessionOptions, SourcePayload, Value,
};
use std::collections::BTreeMap;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

const DEFAULT_SETTLE_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_TRANSIENT_COMPLETIONS_PER_POLL: usize = 8;
const MAX_LAYOUT_DEMAND_PASSES: usize = 4;
const HOST_LIFECYCLE_STARTED_SOURCE: &str = "host.lifecycle.started";
static NEXT_CONTENT_ROOT: AtomicU64 = AtomicU64::new(1);

pub const DEFAULT_BEHAVIOR_VIEWPORT: Viewport = Viewport {
    surface: 1,
    width: 980.0,
    height: 760.0,
    scale: 1.0,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BehaviorAsset {
    pub url: String,
    pub media_type: String,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BehaviorTurnTrace {
    pub sequence: u64,
    pub source_sequence: Option<u64>,
    pub durable_change_count: usize,
    pub transient_effect_count: usize,
    pub cancelled_transient_effect_count: usize,
    pub document_patch_count: usize,
    pub durable_changes: Vec<String>,
    pub transient_effects: Vec<String>,
}

impl BehaviorAsset {
    pub fn new(url: impl Into<String>, media_type: impl Into<String>, bytes: Vec<u8>) -> Self {
        Self {
            url: url.into(),
            media_type: media_type.into(),
            bytes,
        }
    }
}

/// One single-program product runtime driven through retained document hits.
///
/// Embedded `Program` sessions fail closed at construction. The harness never
/// silently bypasses them or dispatches an expected source path directly; an
/// authored action must resolve to a visible typed route in the retained frame.
pub struct BehaviorHarness {
    runtime: PersistentRuntime,
    transient_host: LocalTransientHost,
    effect_worker: HostEffectWorker,
    view: RetainedView,
    columns: ApproximateTextColumnMeasurer,
    viewport: Viewport,
    materialization_overscan: BTreeMap<u64, Range<u64>>,
    sequence: u64,
    hovered: Option<String>,
    pressed: Option<String>,
    focused: Option<String>,
    scenario_trigger_source: Option<String>,
    scenario_trigger_turn: Option<RuntimeTurn>,
    settle_timeout: Duration,
    host_identity_generation: u64,
    turn_trace: Vec<BehaviorTurnTrace>,
}

impl BehaviorHarness {
    pub fn new(
        plan: Arc<MachinePlan>,
        assets: &[BehaviorAsset],
        content_root: impl AsRef<Path>,
    ) -> Result<Self, String> {
        Self::new_with_viewport(plan, assets, content_root, DEFAULT_BEHAVIOR_VIEWPORT)
    }

    pub fn new_with_viewport(
        plan: Arc<MachinePlan>,
        assets: &[BehaviorAsset],
        content_root: impl AsRef<Path>,
        viewport: Viewport,
    ) -> Result<Self, String> {
        validate_plan(&plan)?;
        let (runtime, _) = PersistentRuntime::from_shared_machine_plan(
            Arc::clone(&plan),
            SessionOptions::default(),
            InMemoryDriver::default(),
            PersistenceWorkerConfig::default(),
        )
        .map_err(|error| error.to_string())?;
        let mount = runtime.runtime().mount();
        let sequence = mount.source_sequence.unwrap_or(0);
        let content_root = isolated_content_root(content_root.as_ref());
        let mut transient_host = LocalTransientHost::new(
            content_root,
            assets.iter().map(|asset| PackageAsset {
                url: &asset.url,
                media: &asset.media_type,
                bytes: &asset.bytes,
            }),
            transient_effect_ids(&plan),
        )?;
        transient_host.route_turn(&mount)?;
        let frame = runtime
            .runtime()
            .primary_retained_output_frame()
            .map_err(|error| error.to_string())?
            .clone();
        reject_embedded_programs(&frame)?;
        let mut columns = ApproximateTextColumnMeasurer;
        let view =
            RetainedView::new(frame, viewport, &mut columns).map_err(|error| error.to_string())?;
        let effect_worker =
            HostEffectWorker::start(HostEffectRouter::new()).map_err(|error| error.to_string())?;
        let mut harness = Self {
            runtime,
            transient_host,
            effect_worker,
            view,
            columns,
            viewport,
            materialization_overscan: BTreeMap::new(),
            sequence,
            hovered: None,
            pressed: None,
            focused: None,
            scenario_trigger_source: None,
            scenario_trigger_turn: None,
            settle_timeout: DEFAULT_SETTLE_TIMEOUT,
            host_identity_generation: 1,
            turn_trace: Vec::new(),
        };
        harness.sync_layout_demands()?;
        harness.dispatch_host_lifecycle_started()?;
        harness.settle()?;
        Ok(harness)
    }

    pub fn frame(&self) -> &DocumentFrame {
        self.runtime
            .runtime()
            .primary_retained_output_frame()
            .expect("behavior harness validated its retained output")
    }

    pub fn retained_view(&self) -> &RetainedView {
        &self.view
    }

    pub fn dispatch_scenario_step(&mut self, step: &ScenarioStep) -> Result<(), String> {
        let Some(source_event) = step.source_event.as_ref() else {
            if step.user_action_kind.is_some() {
                return Err(format!(
                    "scenario step `{}` has an authored action without a source contract",
                    step.id
                ));
            }
            self.scenario_trigger_source = None;
            self.scenario_trigger_turn = None;
            return self.settle();
        };
        if step.user_action_kind.as_deref() != Some("click") {
            return Err(format!(
                "behavior harness currently accepts authored click actions, got {:?} for `{}`",
                step.user_action_kind, step.id
            ));
        }

        self.scenario_trigger_source = Some(source_event.source.clone());
        self.scenario_trigger_turn = None;
        let target = self.resolve_scenario_target(step, source_event)?;
        let point = scenario_pointer_position(&self.view, &target, source_event);
        self.pointer_move(&target, point)?;
        self.pointer_down(&target)?;
        self.pointer_up(target, point)?;
        self.settle()
    }

    pub fn assert_scenario_step(&mut self, step: &ScenarioStep) -> Result<(), String> {
        self.scenario_trigger_source = None;
        let turn = self.scenario_trigger_turn.take();
        self.runtime
            .assert_scenario_step(step, turn.as_ref())
            .map_err(|error| error.to_string())
    }

    pub fn settle(&mut self) -> Result<(), String> {
        let started = Instant::now();
        loop {
            if started.elapsed() > self.settle_timeout {
                let status = self.runtime.status();
                return Err(format!(
                    "behavior host did not settle within {}ms; runtime_effect_work={} local_effect_work={} persistence_queue={} persistence_reserved={}",
                    self.settle_timeout.as_millis(),
                    self.runtime.has_effect_work(),
                    self.transient_host.has_work(),
                    status.queue_depth,
                    status.reserved_slots,
                ));
            }

            let mut progressed = false;
            if let Some(turn) = self
                .runtime
                .poll_effect_worker(&mut self.effect_worker)
                .map_err(|error| error.to_string())?
            {
                self.finish_turn(turn)?;
                progressed = true;
            }
            for _ in 0..MAX_TRANSIENT_COMPLETIONS_PER_POLL {
                let Some(completion) = self.transient_host.try_completion()? else {
                    break;
                };
                let turn = match completion {
                    LocalTransientCompletion::Single { call_id, outcome } => self
                        .runtime
                        .complete_transient_effect(call_id, outcome)
                        .map_err(|error| error.to_string())?,
                    LocalTransientCompletion::File(event) if event.is_stream() => self
                        .runtime
                        .deliver_transient_effect_result(
                            event.call_id,
                            event.result_sequence,
                            event.outcome,
                        )
                        .map_err(|error| error.to_string())?,
                    LocalTransientCompletion::File(event) => self
                        .runtime
                        .complete_transient_effect(event.call_id, event.outcome)
                        .map_err(|error| error.to_string())?,
                };
                self.finish_turn(turn)?;
                progressed = true;
            }
            self.sync_layout_demands()?;

            let status = self.runtime.status();
            let persistence_idle = status.pending.is_none()
                && status.queue_depth == 0
                && status.reserved_slots == 0
                && status.pending_content_artifact_stores == 0
                && status.pending_content_artifact_loads == 0;
            if persistence_idle
                && !self.runtime.has_effect_work()
                && !self.effect_worker.is_busy()
                && !self.transient_host.has_work()
            {
                return Ok(());
            }
            if !progressed {
                thread::sleep(Duration::from_millis(1));
            }
        }
    }

    pub fn semantic_value_image(&self) -> Result<RestoreImage, String> {
        self.runtime.semantic_value_image()
    }

    pub fn turn_trace(&self) -> &[BehaviorTurnTrace] {
        &self.turn_trace
    }

    pub fn export_state_artifact(&self) -> Result<Vec<u8>, String> {
        self.runtime
            .export_state_artifact()
            .map_err(|error| error.to_string())
    }

    pub fn preview_state_artifact(
        &self,
        artifact: &[u8],
    ) -> Result<PersistentStateArtifactPreview, String> {
        self.runtime
            .preview_state_artifact(artifact, SessionOptions::default())
            .map_err(|error| error.to_string())
    }

    pub fn activate_state_artifact(
        &mut self,
        artifact: &[u8],
    ) -> Result<PersistentStateArtifactPreview, String> {
        let activation = self
            .runtime
            .activate_state_artifact(artifact, SessionOptions::default())
            .map_err(|error| error.to_string())?;
        let preview = activation.preview;
        self.transient_host.route_turn(&activation.mount)?;
        self.sequence = activation.mount.source_sequence.unwrap_or(self.sequence);
        self.materialization_overscan.clear();
        self.hovered = None;
        self.pressed = None;
        self.focused = None;
        self.scenario_trigger_source = None;
        self.scenario_trigger_turn = None;
        let frame = self
            .runtime
            .runtime()
            .primary_retained_output_frame()
            .map_err(|error| error.to_string())?
            .clone();
        reject_embedded_programs(&frame)?;
        self.view
            .replace(frame, self.viewport, &mut self.columns)
            .map_err(|error| error.to_string())?;
        self.sync_layout_demands()?;
        self.dispatch_host_lifecycle_started()?;
        self.settle()?;
        Ok(preview)
    }

    pub fn inspect_root_current(&mut self, path: &str) -> Result<String, String> {
        self.runtime
            .runtime_mut()
            .inspect_value_current(path, 8)
            .map(|value| format!("{value:?}"))
            .map_err(|error| error.to_string())
    }

    fn resolve_scenario_target(
        &self,
        step: &ScenarioStep,
        event: &ScenarioSourceEvent,
    ) -> Result<HitTarget, String> {
        self.view
            .target_for_scenario(
                &event.source,
                step.user_action_kind.as_deref(),
                event.target_text.as_deref(),
                event.payload.address.as_deref(),
                event.target_key.zip(event.target_generation),
            )
            .ok_or_else(|| {
                let visible_routes = self
                    .view
                    .visible_source_action_bounds()
                    .into_iter()
                    .filter(|(path, _, _)| path == &event.source)
                    .collect::<Vec<_>>();
                let route_diagnostics = self.view.source_action_diagnostics(&event.source);
                format!(
                    "behavior harness could not resolve visible source `{}` for step `{}`; visible matching routes={visible_routes:?}; route diagnostics={route_diagnostics:?}",
                    event.source, step.id,
                )
            })
    }

    fn pointer_move(&mut self, target: &HitTarget, point: (f32, f32)) -> Result<(), String> {
        self.hovered = Some(target.node.clone());
        self.refresh_interaction_state()?;
        let binding = self
            .view
            .frame()
            .nodes
            .get(&DocumentNodeId(target.node.clone()))
            .and_then(|node| {
                ["pointer_move", "move"].into_iter().find_map(|intent| {
                    node.source_bindings
                        .iter()
                        .find(|binding| binding.intent == intent)
                })
            })
            .cloned();
        if let Some(binding) = binding {
            let route = binding.route.ok_or_else(|| {
                format!(
                    "retained pointer-move binding `{}` has no typed source route",
                    binding.source_path
                )
            })?;
            self.dispatch_source(
                &binding.source_path,
                route,
                pointer_source_payload(point, target),
            )?;
        }
        Ok(())
    }

    fn pointer_down(&mut self, target: &HitTarget) -> Result<(), String> {
        self.pressed = Some(target.node.clone());
        let next_focus = Some(target.node.clone());
        if next_focus != self.focused {
            if let Some(previous) = self.focused.clone() {
                self.dispatch_node_intent(&previous, &["blur", "source"])?;
            }
            self.focused = next_focus;
        }
        self.refresh_interaction_state()
    }

    fn pointer_up(&mut self, target: HitTarget, point: (f32, f32)) -> Result<(), String> {
        let matches = self.pressed.take().as_deref() == Some(target.node.as_str());
        self.refresh_interaction_state()?;
        if matches && pointer_activation_intent(target.source_intent.as_deref()) {
            let path = target.source_path.as_deref().ok_or_else(|| {
                format!("retained hit target `{}` has no source path", target.node)
            })?;
            let route = target.source_route.clone().ok_or_else(|| {
                format!(
                    "retained hit target `{}` has no typed source route",
                    target.node
                )
            })?;
            self.dispatch_source(path, route, pointer_source_payload(point, &target))?;
        }
        Ok(())
    }

    fn dispatch_node_intent(&mut self, node_id: &str, intents: &[&str]) -> Result<(), String> {
        let binding = self
            .view
            .frame()
            .nodes
            .get(&DocumentNodeId(node_id.to_owned()))
            .and_then(|node| {
                intents.iter().find_map(|intent| {
                    node.source_bindings
                        .iter()
                        .find(|binding| binding.intent == *intent)
                })
            })
            .cloned();
        let Some(binding) = binding else {
            return Ok(());
        };
        let route = binding.route.ok_or_else(|| {
            format!(
                "retained `{}` binding `{}` has no typed source route",
                binding.intent, binding.source_path
            )
        })?;
        self.dispatch_source(&binding.source_path, route, SourcePayload::default())
    }

    fn dispatch_source(
        &mut self,
        path: &str,
        route: SourceRouteToken,
        payload: SourcePayload,
    ) -> Result<(), String> {
        let next_sequence = self.sequence.saturating_add(1);
        let event = self
            .runtime
            .runtime()
            .source_event(next_sequence, route, payload)
            .map_err(|error| error.to_string())?;
        let turn = self
            .runtime
            .dispatch(event)
            .map_err(|error| error.to_string())?;
        self.capture_scenario_turn(path, &turn);
        self.finish_turn(turn)
    }

    fn finish_turn(&mut self, turn: RuntimeTurn) -> Result<(), String> {
        self.turn_trace.push(BehaviorTurnTrace {
            sequence: turn.sequence,
            source_sequence: turn.source_sequence,
            durable_change_count: turn.durable_changes.len(),
            transient_effect_count: turn.transient_effects.len(),
            cancelled_transient_effect_count: turn.cancelled_transient_effects.len(),
            document_patch_count: turn.document_patches.len(),
            durable_changes: turn
                .durable_changes
                .iter()
                .map(durable_change_trace)
                .collect(),
            transient_effects: turn
                .transient_effects
                .iter()
                .map(|effect| {
                    format!(
                        "effect={:?} invocation={:?} target={:?} trigger={} authority_turn={}",
                        effect.effect_id,
                        effect.invocation_id,
                        effect.target,
                        effect.trigger_sequence,
                        effect.authority_turn_sequence,
                    )
                })
                .collect(),
        });
        self.transient_host.route_turn(&turn)?;
        self.sequence = turn.source_sequence.unwrap_or(self.sequence);
        self.view
            .apply_patches(turn.document_patches, &mut self.columns)
            .map_err(|error| error.to_string())?;
        self.retain_interaction_state();
        self.refresh_interaction_state()?;
        self.sync_layout_demands()
    }

    fn retain_interaction_state(&mut self) {
        let contains = |id: &str| {
            self.view
                .frame()
                .nodes
                .contains_key(&DocumentNodeId(id.to_owned()))
        };
        self.hovered = self.hovered.take().filter(|id| contains(id));
        self.pressed = self.pressed.take().filter(|id| contains(id));
        self.focused = self.focused.take().filter(|id| contains(id));
    }

    fn refresh_interaction_state(&mut self) -> Result<(), String> {
        self.view
            .set_interaction_state(
                self.hovered.as_deref(),
                self.focused.as_deref(),
                &mut self.columns,
            )
            .map(|_| ())
            .map_err(|error| error.to_string())
    }

    fn sync_layout_demands(&mut self) -> Result<(), String> {
        for _ in 0..MAX_LAYOUT_DEMAND_PASSES {
            let windows = coalesced_layout_demands(self.view.demands());
            let mut changed = false;
            for (materialization, (visible, overscan)) in windows {
                if self
                    .materialization_overscan
                    .get(&materialization)
                    .is_some_and(|current| {
                        current.start <= visible.start && current.end >= visible.end
                    })
                {
                    continue;
                }
                let patches = self
                    .runtime
                    .demand_document_window_by_id(materialization, visible, overscan.clone())
                    .map_err(|error| error.to_string())?;
                self.materialization_overscan
                    .insert(materialization, overscan);
                if patches.is_empty() {
                    continue;
                }
                self.view
                    .apply_patches(patches, &mut self.columns)
                    .map_err(|error| error.to_string())?;
                changed = true;
            }
            if !changed {
                return Ok(());
            }
        }
        Err(format!(
            "behavior document demands did not converge in {MAX_LAYOUT_DEMAND_PASSES} passes"
        ))
    }

    fn capture_scenario_turn(&mut self, source_path: &str, turn: &RuntimeTurn) {
        if self.scenario_trigger_source.as_deref() != Some(source_path) {
            if self.scenario_trigger_source.is_some() && !turn.document_patches.is_empty() {
                self.scenario_trigger_turn = Some(turn.clone());
            }
            return;
        }
        let mut declared = turn.clone();
        if let Some(earlier) = self.scenario_trigger_turn.take() {
            declared.document_patches.extend(earlier.document_patches);
        }
        self.scenario_trigger_turn = Some(declared);
        self.scenario_trigger_source = None;
    }

    fn dispatch_host_lifecycle_started(&mut self) -> Result<(), String> {
        if !self
            .runtime
            .runtime()
            .machine_plan()
            .source_routes
            .iter()
            .any(|source| source.path == HOST_LIFECYCLE_STARTED_SOURCE)
        {
            return Ok(());
        }
        let generation = self.host_identity_generation.min(0xffff_ffff_ffff);
        let payload = SourcePayload {
            fields: [
                (
                    "instance_id".to_owned(),
                    Value::Text(format!("00000000-0000-4000-8000-{generation:012x}")),
                ),
                (
                    "grant_id".to_owned(),
                    Value::Text(format!("10000000-0000-4000-8000-{generation:012x}")),
                ),
            ]
            .into_iter()
            .collect(),
            ..SourcePayload::default()
        };
        let route = self
            .runtime
            .runtime()
            .source_route_token_for_path(HOST_LIFECYCLE_STARTED_SOURCE, &[])
            .map_err(|error| error.to_string())?;
        self.dispatch_source(HOST_LIFECYCLE_STARTED_SOURCE, route, payload)
    }
}

fn durable_change_trace(change: &boon_persistence::DurableChange) -> String {
    match change {
        boon_persistence::DurableChange::SetScalar { memory_id, .. } => {
            format!("set-scalar:{memory_id}")
        }
        boon_persistence::DurableChange::DeleteScalar { memory_id } => {
            format!("delete-scalar:{memory_id}")
        }
        boon_persistence::DurableChange::SetList { memory_id, .. } => {
            format!("set-list:{memory_id}")
        }
        boon_persistence::DurableChange::SetRowField {
            memory_id,
            row_key,
            row_generation,
            ..
        } => format!("set-row-field:{memory_id}:{row_key}:{row_generation}"),
        boon_persistence::DurableChange::InsertRow { memory_id, row, .. } => {
            format!("insert-row:{memory_id}:{}:{}", row.key, row.generation)
        }
        boon_persistence::DurableChange::RemoveRow {
            memory_id,
            row_key,
            row_generation,
            next_key,
            ..
        } => format!("remove-row:{memory_id}:{row_key}:{row_generation}:next={next_key}"),
        boon_persistence::DurableChange::DeleteList { memory_id } => {
            format!("delete-list:{memory_id}")
        }
        boon_persistence::DurableChange::SetMap { collection_id, .. } => {
            format!("set-map:{}", collection_id.memory_id)
        }
        boon_persistence::DurableChange::MapUpsert { collection_id, .. } => {
            format!("map-upsert:{}", collection_id.memory_id)
        }
        boon_persistence::DurableChange::MapRemove { collection_id, .. } => {
            format!("map-remove:{}", collection_id.memory_id)
        }
        boon_persistence::DurableChange::DeleteMap { collection_id } => {
            format!("delete-map:{}", collection_id.memory_id)
        }
        boon_persistence::DurableChange::SetSet { collection_id, .. } => {
            format!("set-set:{}", collection_id.memory_id)
        }
        boon_persistence::DurableChange::SetAdd { collection_id, .. } => {
            format!("set-add:{}", collection_id.memory_id)
        }
        boon_persistence::DurableChange::SetRemove { collection_id, .. } => {
            format!("set-remove:{}", collection_id.memory_id)
        }
        boon_persistence::DurableChange::DeleteSet { collection_id } => {
            format!("delete-set:{}", collection_id.memory_id)
        }
    }
}

fn validate_plan(plan: &MachinePlan) -> Result<(), String> {
    if !plan.application.identity.is_valid() {
        return Err("behavior MachinePlan has an invalid application identity".to_owned());
    }
    if plan.document_plan().is_none() {
        return Err("behavior MachinePlan has no typed document plan".to_owned());
    }
    Ok(())
}

fn reject_embedded_programs(frame: &DocumentFrame) -> Result<(), String> {
    if let Some(node) = frame
        .nodes
        .values()
        .find(|node| node.embedded_program.is_some())
    {
        return Err(format!(
            "behavior harness does not silently bypass embedded Program node `{}`",
            node.id.0
        ));
    }
    Ok(())
}

fn transient_effect_ids(plan: &MachinePlan) -> impl Iterator<Item = boon_plan::EffectId> + '_ {
    plan.effects.iter().filter_map(|contract| {
        matches!(
            contract.replay,
            EffectReplay::ReadOnly | EffectReplay::ProcessScoped
        )
        .then_some(contract.effect_id)
    })
}

fn isolated_content_root(parent: &Path) -> PathBuf {
    let ordinal = NEXT_CONTENT_ROOT.fetch_add(1, Ordering::Relaxed);
    parent.join(format!("{}-{ordinal}", std::process::id()))
}

fn coalesced_layout_demands(demands: &[LayoutDemand]) -> BTreeMap<u64, (Range<u64>, Range<u64>)> {
    let mut windows = BTreeMap::<u64, (Range<u64>, Range<u64>)>::new();
    for demand in demands {
        let Some(materialization) = demand.materialization else {
            continue;
        };
        windows
            .entry(materialization)
            .and_modify(|(visible, overscan)| {
                visible.start = visible.start.min(demand.visible.start);
                visible.end = visible.end.max(demand.visible.end);
                overscan.start = overscan.start.min(demand.overscan.start);
                overscan.end = overscan.end.max(demand.overscan.end);
            })
            .or_insert_with(|| (demand.visible.clone(), demand.overscan.clone()));
    }
    windows
}

fn pointer_activation_intent(intent: Option<&str>) -> bool {
    intent.is_some_and(|intent| {
        matches!(
            intent,
            "press" | "click" | "source" | "activate" | "toggle" | "submit" | "open" | "select"
        )
    })
}

fn scenario_pointer_position(
    view: &RetainedView,
    target: &HitTarget,
    event: &ScenarioSourceEvent,
) -> (f32, f32) {
    let Some(bounds) = view.node_bounds(&target.node) else {
        return (target.center_x, target.center_y);
    };
    (
        projected_scenario_coordinate(
            event.payload.fields.get("pointer_x"),
            event.payload.fields.get("pointer_width"),
            bounds.x,
            bounds.width,
            target.center_x,
        ),
        projected_scenario_coordinate(
            event.payload.fields.get("pointer_y"),
            event.payload.fields.get("pointer_height"),
            bounds.y,
            bounds.height,
            target.center_y,
        ),
    )
}

fn projected_scenario_coordinate(
    value: Option<&Value>,
    source_span: Option<&Value>,
    target_start: f32,
    target_span: f32,
    fallback: f32,
) -> f32 {
    let Some(value) = value.and_then(value_as_f32) else {
        return fallback;
    };
    let Some(source_span) = source_span
        .and_then(value_as_f32)
        .filter(|span| span.is_finite() && *span > 0.0)
    else {
        return fallback;
    };
    if !value.is_finite() || !target_span.is_finite() || target_span <= 0.0 {
        return fallback;
    }
    let inset = (target_span * 0.5).min(0.5);
    let usable = (target_span - inset * 2.0).max(0.0);
    target_start + inset + usable * (value / source_span).clamp(0.0, 1.0)
}

fn value_as_f32(value: &Value) -> Option<f32> {
    match value {
        Value::Text(value) => value.parse().ok(),
        Value::Number(value) => value.to_f64_host_rounded().ok().map(|value| value as f32),
        _ => None,
    }
}

fn pointer_source_payload(point: (f32, f32), target: &HitTarget) -> SourcePayload {
    let mut payload = SourcePayload::default();
    if target.bounds_width.is_finite()
        && target.bounds_height.is_finite()
        && target.bounds_width > 0.0
        && target.bounds_height > 0.0
    {
        let local_x = (point.0 - target.bounds_x).clamp(0.0, target.bounds_width);
        let local_y = (point.1 - target.bounds_y).clamp(0.0, target.bounds_height);
        for (name, value) in [
            ("pointer_x", local_x),
            ("pointer_y", local_y),
            ("pointer_width", target.bounds_width),
            ("pointer_height", target.bounds_height),
        ] {
            payload.fields.insert(
                name.to_owned(),
                Value::Number(
                    ExactNumber::from_f64_boundary_exact(f64::from(value.round()))
                        .expect("finite retained pointer geometry"),
                ),
            );
        }
    }
    payload
}
