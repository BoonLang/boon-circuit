use boon_document::{
    DocumentFrame, DocumentNodeId, DocumentNodeKind, DocumentState, LayoutDemand, StyleValue,
};
use boon_host::{HostEvent, PointerButton, PointerPhase};
use boon_runtime::{
    DocumentPatch, DocumentPatchStatus, LiveRuntime, RowId, RuntimePhaseTimings, RuntimeTurn,
    SourcePayload, Value,
};
use std::time::{Duration, Instant};

use crate::view::HitTarget;

type ViewResult<T> = Result<T, String>;

const DOUBLE_CLICK_INTERVAL: Duration = Duration::from_millis(500);

pub struct RuntimeView {
    runtime: LiveRuntime,
    hovered: Option<String>,
    pressed: Option<String>,
    focused: Option<String>,
    text_inputs: std::collections::BTreeMap<String, String>,
    scroll_offsets: std::collections::BTreeMap<String, boon_document_model::ScrollState>,
    pending_patches: Vec<DocumentPatch>,
    sequence: u64,
    last_dispatched_source: Option<String>,
    last_primary_click: Option<(String, Instant)>,
    last_runtime_phase: RuntimePhaseTimings,
}

impl RuntimeView {
    pub fn mount(runtime: LiveRuntime, turn: RuntimeTurn) -> ViewResult<Self> {
        if turn.document_patch_status != DocumentPatchStatus::Complete {
            return Err("MachinePlan did not produce complete typed document bindings".to_owned());
        }
        let mounted = state_from_mount(turn.document_patches)?;
        let frame = runtime
            .document_frame()
            .ok_or_else(|| "mounted runtime has no document frame".to_owned())?;
        debug_assert_eq!(mounted.frame(), frame);
        let text_inputs = frame
            .nodes
            .values()
            .filter(|node| node.kind == DocumentNodeKind::TextInput)
            .map(|node| {
                (
                    node.id.0.clone(),
                    node.text
                        .as_ref()
                        .map(|text| text.text.clone())
                        .unwrap_or_default(),
                )
            })
            .collect();
        Ok(Self {
            runtime,
            hovered: None,
            pressed: None,
            focused: None,
            text_inputs,
            scroll_offsets: std::collections::BTreeMap::new(),
            pending_patches: Vec::new(),
            sequence: 0,
            last_dispatched_source: None,
            last_primary_click: None,
            last_runtime_phase: RuntimePhaseTimings::default(),
        })
    }

    pub fn frame(&self) -> DocumentFrame {
        let mut frame = self
            .runtime
            .document_frame()
            .expect("mounted runtime keeps a document frame")
            .clone();
        for (id, scroll) in &self.scroll_offsets {
            if let Some(node) = frame.nodes.get_mut(&DocumentNodeId(id.clone())) {
                node.scroll = Some(*scroll);
            }
        }
        frame
    }

    pub fn hovered(&self) -> Option<&str> {
        self.hovered.as_deref()
    }

    pub fn focused(&self) -> Option<&str> {
        self.focused.as_deref()
    }

    pub fn event_sequence(&self) -> u64 {
        self.sequence
    }

    pub fn last_dispatched_source(&self) -> Option<&str> {
        self.last_dispatched_source.as_deref()
    }

    pub fn last_runtime_phase(&self) -> RuntimePhaseTimings {
        self.last_runtime_phase
    }

    pub fn scenario_target_row(
        &self,
        source_path: &str,
        target_text: Option<&str>,
        address: Option<&str>,
        occurrence: Option<u64>,
    ) -> ViewResult<Option<(u64, u64)>> {
        let Some(target_text) = target_text.or(address) else {
            return Ok(None);
        };
        let occurrence = usize::try_from(occurrence.unwrap_or(0))
            .map_err(|_| "scenario target occurrence exceeds usize".to_owned())?;
        Ok(self
            .runtime
            .row_target_for_source_text(source_path, target_text, occurrence)
            .map_err(|error| error.to_string())?
            .map(|row| (row.key, row.generation)))
    }

    pub fn take_patches(&mut self) -> Vec<DocumentPatch> {
        std::mem::take(&mut self.pending_patches)
    }

    pub fn apply_layout_demands(&mut self, demands: &[LayoutDemand]) -> ViewResult<bool> {
        let mut windows =
            std::collections::BTreeMap::<u64, (std::ops::Range<u64>, std::ops::Range<u64>)>::new();
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
        let mut changed = false;
        for (materialization, (visible, overscan)) in windows {
            for patch in self
                .runtime
                .demand_document_window_by_id(materialization, visible, overscan)
                .map_err(|error| error.to_string())?
            {
                let patch = self.with_view_state(patch);
                self.sync_text_input_patch(&patch);
                self.pending_patches.push(patch);
                changed = true;
            }
        }
        Ok(changed)
    }

    pub fn handle_event(
        &mut self,
        event: &HostEvent,
        target: Option<HitTarget>,
    ) -> ViewResult<bool> {
        self.last_runtime_phase = RuntimePhaseTimings::default();
        match event {
            HostEvent::Pointer(pointer) => match pointer.phase {
                PointerPhase::Move => {
                    let next = target.map(|target| target.node);
                    let changed = next != self.hovered;
                    self.hovered = next;
                    Ok(changed)
                }
                PointerPhase::Leave => Ok(self.hovered.take().is_some()),
                PointerPhase::Down if pointer.button == Some(PointerButton::Primary) => {
                    let focus_requires_immediate_present = target
                        .as_ref()
                        .is_some_and(|target| self.target_is_text_input(target));
                    self.pressed = target.as_ref().map(|target| target.node.clone());
                    let next_focus = target.as_ref().map(|target| target.node.clone());
                    let changed = next_focus != self.focused;
                    let mut dirty = false;
                    if changed && let Some(previous) = self.focused.clone() {
                        dirty |= self.dispatch_node_intent(
                            &previous,
                            &["blur", "source"],
                            SourcePayload::default(),
                        )?;
                        self.text_inputs.remove(&previous);
                    }
                    self.focused = next_focus;
                    self.sync_focused_text_from_document();
                    Ok(dirty || (changed && focus_requires_immediate_present))
                }
                PointerPhase::Up if pointer.button == Some(PointerButton::Primary) => {
                    let matches = self.pressed.take().as_deref()
                        == target.as_ref().map(|target| target.node.as_str());
                    if matches {
                        if let Some(target) = target {
                            if target.source_intent.as_deref() == Some("double_click") {
                                let now = Instant::now();
                                let is_double_click =
                                    self.last_primary_click.take().is_some_and(|(node, at)| {
                                        node == target.node
                                            && now.saturating_duration_since(at)
                                                <= DOUBLE_CLICK_INTERVAL
                                    });
                                if is_double_click {
                                    return self.dispatch_target(&target, SourcePayload::default());
                                }
                                self.last_primary_click = Some((target.node, now));
                                return Ok(false);
                            }
                            if pointer_activation_intent(target.source_intent.as_deref())
                                && !self.bare_source_is_text_input(&target)
                            {
                                return self.dispatch_target(&target, SourcePayload::default());
                            }
                        }
                    }
                    Ok(false)
                }
                _ => Ok(false),
            },
            HostEvent::Wheel(wheel) => {
                let Some(root) = target.and_then(|target| target.scroll_root) else {
                    return Ok(false);
                };
                let root = DocumentNodeId(root);
                let mut scroll = self
                    .scroll_offsets
                    .get(&root.0)
                    .copied()
                    .or_else(|| {
                        self.runtime
                            .document_frame()
                            .and_then(|frame| frame.nodes.get(&root))
                            .and_then(|node| node.scroll)
                    })
                    .unwrap_or(boon_document_model::ScrollState { x: 0.0, y: 0.0 });
                scroll.x = (scroll.x + wheel.delta_x).max(0.0);
                scroll.y = (scroll.y + wheel.delta_y).max(0.0);
                let root_id = root.0.clone();
                let patch = DocumentPatch::SetScroll { id: root, scroll };
                self.scroll_offsets.insert(root_id, scroll);
                self.pending_patches.push(patch);
                Ok(true)
            }
            HostEvent::TextInput(text) => {
                self.set_focused_text(text.text.clone());
                self.dispatch_focused(
                    &["change", "text", "input", "source"],
                    SourcePayload {
                        text: Some(text.text.clone()),
                        ..SourcePayload::default()
                    },
                )
            }
            HostEvent::Ime(ime) => {
                if let boon_host::ImeInputKind::Commit { text } = &ime.kind {
                    self.set_focused_text(text.clone());
                    self.dispatch_focused(
                        &["change", "text", "input", "source"],
                        SourcePayload {
                            text: Some(text.clone()),
                            ..SourcePayload::default()
                        },
                    )
                } else {
                    Ok(false)
                }
            }
            HostEvent::Keyboard(key) if key.pressed => {
                let value = match &key.logical_key {
                    boon_host::LogicalKey::Character(value)
                    | boon_host::LogicalKey::Named(value) => value.clone(),
                    boon_host::LogicalKey::Dead(Some(value)) => value.to_string(),
                    boon_host::LogicalKey::Dead(None) | boon_host::LogicalKey::Unidentified => {
                        return Ok(false);
                    }
                };
                let intents: &[&str] = if value == "Enter" {
                    &["commit", "submit", "key_down", "source"]
                } else if value == "Escape" {
                    &["cancel", "escape", "key_down", "source"]
                } else {
                    &["key_down", "source"]
                };
                let clear_text = matches!(value.as_str(), "Enter" | "Escape");
                let changed = self.dispatch_focused(
                    intents,
                    SourcePayload {
                        key: Some(value),
                        ..SourcePayload::default()
                    },
                )?;
                if clear_text && let Some(focused) = self.focused.as_ref() {
                    self.text_inputs.remove(focused);
                }
                Ok(changed)
            }
            HostEvent::Focus { focused: false, .. } => {
                let previous = self.focused.take();
                let Some(previous) = previous else {
                    return Ok(false);
                };
                self.dispatch_node_intent(
                    &previous,
                    &["blur", "source"],
                    SourcePayload::default(),
                )?;
                self.text_inputs.remove(&previous);
                Ok(true)
            }
            _ => Ok(false),
        }
    }

    fn dispatch_focused(&mut self, intents: &[&str], payload: SourcePayload) -> ViewResult<bool> {
        let Some(focused) = self.focused.clone() else {
            return Ok(false);
        };
        self.dispatch_node_intent(&focused, intents, payload)
    }

    fn dispatch_node_intent(
        &mut self,
        node_id: &str,
        intents: &[&str],
        mut payload: SourcePayload,
    ) -> ViewResult<bool> {
        let frame = self
            .runtime
            .document_frame()
            .ok_or_else(|| "mounted runtime has no document frame".to_owned())?;
        let Some(node) = frame.nodes.get(&DocumentNodeId(node_id.to_owned())) else {
            return Ok(false);
        };
        let Some(binding) = intents.iter().find_map(|intent| {
            node.source_bindings
                .iter()
                .find(|binding| binding.intent == *intent)
        }) else {
            return Ok(false);
        };
        let target = HitTarget {
            node: node_id.to_owned(),
            source_path: Some(binding.source_path.clone()),
            source_intent: Some(binding.intent.clone()),
            row_key: style_u64(node, &["row_key", "target_key", "__row_key"]),
            row_generation: style_u64(
                node,
                &["row_generation", "target_generation", "__row_generation"],
            ),
            scroll_root: None,
            center_x: 0.0,
            center_y: 0.0,
        };
        if payload.text.is_none()
            && (matches!(binding.intent.as_str(), "commit" | "submit" | "blur")
                || (node.kind == DocumentNodeKind::TextInput
                    && (binding.intent == "source" || payload.key.is_some())))
        {
            payload.text = self
                .text_inputs
                .get(node_id)
                .cloned()
                .or_else(|| node.text.as_ref().map(|text| text.text.clone()));
        }
        self.dispatch_target(&target, payload)
    }

    fn bare_source_is_text_input(&self, target: &HitTarget) -> bool {
        target.source_intent.as_deref() == Some("source") && self.target_is_text_input(target)
    }

    fn target_is_text_input(&self, target: &HitTarget) -> bool {
        self.runtime
            .document_frame()
            .and_then(|frame| frame.nodes.get(&DocumentNodeId(target.node.clone())))
            .is_some_and(|node| node.kind == DocumentNodeKind::TextInput)
    }

    fn dispatch_target(
        &mut self,
        target: &HitTarget,
        mut payload: SourcePayload,
    ) -> ViewResult<bool> {
        let Some(path) = target.source_path.as_deref() else {
            return Ok(false);
        };
        if target.row_key.is_none()
            && let Some(field) = self
                .runtime
                .source_row_lookup_field(path)
                .map(str::to_owned)
            && let Some(value) = self
                .runtime
                .document_frame()
                .and_then(|frame| frame.nodes.get(&DocumentNodeId(target.node.clone())))
                .and_then(|node| node.style.get(&field))
                .and_then(style_payload_value)
        {
            match (field.as_str(), value) {
                ("address", Value::Text(value)) => payload.address = Some(value),
                ("key", Value::Text(value)) => payload.key = Some(value),
                ("text", Value::Text(value)) => payload.text = Some(value),
                (field, value) => {
                    payload.fields.insert(field.to_owned(), value);
                }
            }
        }
        self.sequence = self.sequence.saturating_add(1);
        let row = self.row_target(path, target.row_key, target.row_generation)?;
        let event = self
            .runtime
            .source_event(self.sequence, path, row, payload)
            .map_err(|error| error.to_string())?;
        let turn = self
            .runtime
            .dispatch(event)
            .map_err(|error| error.to_string())?;
        self.last_runtime_phase = turn.phase_timings;
        self.last_dispatched_source = Some(path.to_owned());
        let changed = !turn.document_patches.is_empty();
        for patch in turn.document_patches {
            let patch = self.with_view_state(patch);
            self.sync_text_input_patch(&patch);
            self.pending_patches.push(patch);
        }
        Ok(changed)
    }

    fn set_focused_text(&mut self, text: String) {
        let Some(focused) = self.focused.as_ref() else {
            return;
        };
        if self
            .runtime
            .document_frame()
            .and_then(|frame| frame.nodes.get(&DocumentNodeId(focused.clone())))
            .is_some_and(|node| node.kind == DocumentNodeKind::TextInput)
        {
            self.text_inputs.insert(focused.clone(), text);
        }
    }

    fn sync_focused_text_from_document(&mut self) {
        let Some(focused) = self.focused.as_ref() else {
            return;
        };
        let Some(node) = self
            .runtime
            .document_frame()
            .and_then(|frame| frame.nodes.get(&DocumentNodeId(focused.clone())))
        else {
            return;
        };
        if node.kind == DocumentNodeKind::TextInput {
            self.text_inputs.insert(
                focused.clone(),
                node.text
                    .as_ref()
                    .map(|text| text.text.clone())
                    .unwrap_or_default(),
            );
        }
    }

    fn sync_text_input_patch(&mut self, patch: &DocumentPatch) {
        match patch {
            DocumentPatch::UpsertNode(node) if node.kind == DocumentNodeKind::TextInput => {
                if self.focused.as_deref() != Some(node.id.0.as_str()) {
                    self.text_inputs.insert(
                        node.id.0.clone(),
                        node.text
                            .as_ref()
                            .map(|text| text.text.clone())
                            .unwrap_or_default(),
                    );
                }
            }
            DocumentPatch::SetText { id, text }
                if self.focused.as_deref() != Some(id.0.as_str()) =>
            {
                self.text_inputs.insert(id.0.clone(), text.text.clone());
            }
            DocumentPatch::RemoveNode { id } => {
                self.text_inputs.remove(&id.0);
            }
            _ => {}
        }
    }

    fn with_view_state(&self, patch: DocumentPatch) -> DocumentPatch {
        match patch {
            DocumentPatch::UpsertNode(mut node) => {
                if let Some(scroll) = self.scroll_offsets.get(&node.id.0).copied() {
                    node.scroll = Some(scroll);
                }
                DocumentPatch::UpsertNode(node)
            }
            patch => patch,
        }
    }

    fn row_target(
        &self,
        source_path: &str,
        key: Option<u64>,
        generation: Option<u64>,
    ) -> ViewResult<Option<RowId>> {
        let Some(key) = key else {
            return Ok(None);
        };
        self.runtime
            .row_target_for_source_path(source_path, key, generation.unwrap_or(1))
            .map(Some)
            .map_err(|error| error.to_string())
    }
}

fn pointer_activation_intent(intent: Option<&str>) -> bool {
    intent.is_some_and(|intent| {
        matches!(
            intent,
            "press" | "click" | "source" | "activate" | "toggle" | "submit" | "open" | "select"
        )
    })
}

fn style_payload_value(value: &StyleValue) -> Option<Value> {
    match value {
        StyleValue::Text(value) => Some(Value::Text(value.clone())),
        StyleValue::Number(value) if value.is_finite() => Some(Value::Number(*value as i64)),
        StyleValue::Number(_) => None,
        StyleValue::Bool(value) => Some(Value::Bool(*value)),
        StyleValue::RichTextSpans(_) | StyleValue::EditorTypeHints(_) => None,
    }
}

fn state_from_mount(patches: Vec<DocumentPatch>) -> ViewResult<DocumentState> {
    let root = patches.iter().find_map(|patch| match patch {
        DocumentPatch::UpsertNode(node)
            if node.parent.is_none() && node.kind == DocumentNodeKind::Root =>
        {
            Some(node.id.0.clone())
        }
        _ => None,
    });
    let root = root.ok_or_else(|| "typed mount patches contain no document root".to_owned())?;
    let mut state = DocumentState::new(root);
    for patch in patches {
        state
            .apply_patch(patch)
            .map_err(|error| error.to_string())?;
    }
    Ok(state)
}

fn style_u64(node: &boon_document::DocumentNode, keys: &[&str]) -> Option<u64> {
    keys.iter().find_map(|key| match node.style.get(*key) {
        Some(StyleValue::Number(value)) if value.is_finite() && *value >= 0.0 => {
            Some(*value as u64)
        }
        Some(StyleValue::Text(value)) => value.parse().ok(),
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use boon_host::{KeyEvent, LogicalKey, PointerEvent, SurfaceId, TextInputEvent, WheelEvent};
    use boon_runtime::RuntimeSourceUnit;

    #[test]
    fn cells_scroll_patches_retained_view_and_requests_typed_window() {
        let example = crate::catalog::Catalog::load()
            .unwrap()
            .open("cells")
            .unwrap();
        let units = example
            .units
            .into_iter()
            .map(|unit| RuntimeSourceUnit {
                path: unit.path,
                source: unit.source,
            })
            .collect::<Vec<_>>();
        let runtime = LiveRuntime::from_project("examples/cells.bn", &units).unwrap();
        let mount = runtime.mount();
        let mut model = RuntimeView::mount(runtime, mount).unwrap();
        let mut columns = boon_document::render_scene::ApproximateTextColumnMeasurer;
        let mut view = crate::view::RetainedView::new(
            model.frame(),
            boon_host::Viewport {
                surface: 1,
                width: 1_100.0,
                height: 720.0,
                scale: 1.0,
            },
            &mut columns,
        )
        .unwrap();

        let mut converged = false;
        for _ in 0..4 {
            let demands = view.demands().to_vec();
            if !model.apply_layout_demands(&demands).unwrap() {
                converged = true;
                break;
            }
            view.apply_patches(model.take_patches(), &mut columns)
                .unwrap();
        }
        assert!(converged, "initial Cells window demands must converge");
        assert!(view.demands().iter().any(|demand| {
            demand.materialization.is_some()
                && demand.logical_item_count >= 100
                && demand.item_extent_milli.is_some()
        }));

        for step in example.test_steps.iter().take(4) {
            drive_scenario_step(&mut model, &mut view, &mut columns, step);
        }
        let frame = view.frame();
        let mut source_counts = std::collections::BTreeMap::new();
        for binding in frame
            .nodes
            .values()
            .flat_map(|node| node.source_bindings.iter())
        {
            *source_counts
                .entry((binding.source_path.clone(), binding.intent.clone()))
                .or_insert(0_usize) += 1;
        }
        let formula_inputs = frame
            .nodes
            .values()
            .filter(|node| node.kind == DocumentNodeKind::TextInput)
            .map(|node| {
                let bindings = node
                    .source_bindings
                    .iter()
                    .map(|binding| (binding.source_path.clone(), binding.intent.clone()))
                    .collect::<Vec<_>>();
                let identity = ["address", "key", "target"]
                    .into_iter()
                    .filter_map(|key| {
                        node.style
                            .get(key)
                            .map(|value| (key.to_owned(), format!("{value:?}")))
                    })
                    .collect::<Vec<_>>();
                (node.id.0.clone(), bindings, identity, node.text.clone())
            })
            .collect::<Vec<_>>();
        assert!(
            view.target_for_scenario(
                "cell.sources.editor.select",
                Some("click"),
                Some("20"),
                Some("A3"),
                None,
            )
            .is_some(),
            "formula input must commit through the selected row route; source_counts={source_counts:?}; text_inputs={formula_inputs:?}"
        );

        let target = view
            .target_for_source("cell.sources.editor.select", Some("15"))
            .expect("visible A2 target");
        assert!(target.scroll_root.is_some());
        let full_lowers = view.retained_stats().full_lower_count;
        let changed = model
            .handle_event(
                &HostEvent::Wheel(WheelEvent {
                    surface: SurfaceId("preview".to_owned()),
                    x: target.center_x,
                    y: target.center_y,
                    delta_x: 0.0,
                    delta_y: 52.0,
                }),
                Some(target),
            )
            .unwrap();
        assert!(changed);
        let update = view
            .apply_patches(model.take_patches(), &mut columns)
            .unwrap();
        assert!(!update.full_lowered);
        assert_eq!(view.retained_stats().full_lower_count, full_lowers);
        assert!(model.apply_layout_demands(view.demands()).unwrap());
        view.apply_patches(model.take_patches(), &mut columns)
            .unwrap();
        assert!(
            view.frame()
                .nodes
                .values()
                .any(|node| node.scroll.is_some_and(|scroll| scroll.y == 52.0))
        );
        assert!(!model.apply_layout_demands(view.demands()).unwrap());
    }

    fn drive_scenario_step(
        model: &mut RuntimeView,
        view: &mut crate::view::RetainedView,
        columns: &mut boon_document::render_scene::ApproximateTextColumnMeasurer,
        step: &crate::protocol::TestStep,
    ) {
        let surface = SurfaceId("preview".to_owned());
        let sequence_before = model.event_sequence();
        let target_row = model
            .scenario_target_row(
                &step.source_path,
                step.target_text.as_deref(),
                step.address.as_deref(),
                step.target_occurrence,
            )
            .unwrap();
        let target = view
            .target_for_scenario(
                &step.source_path,
                step.action_kind.as_deref(),
                step.target_text.as_deref(),
                step.address.as_deref(),
                target_row,
            )
            .unwrap_or_else(|| {
                let available = view
                    .frame()
                    .nodes
                    .values()
                    .flat_map(|node| node.source_bindings.iter())
                    .map(|binding| (binding.source_path.clone(), binding.intent.clone()))
                    .take(32)
                    .collect::<Vec<_>>();
                let candidates = view
                    .frame()
                    .nodes
                    .values()
                    .filter(|node| {
                        node.source_bindings
                            .iter()
                            .any(|binding| binding.source_path == step.source_path)
                    })
                    .map(|node| {
                        (
                            node.id.0.clone(),
                            node.kind.clone(),
                            node.text.as_ref().map(|text| text.text.clone()),
                            node.style.get("target").cloned(),
                            node.style.get("label").cloned(),
                            node.style.get("row_list").cloned(),
                            node.style.get("row_key").cloned(),
                        )
                    })
                    .take(16)
                    .collect::<Vec<_>>();
                let runtime_bindings = model
                    .runtime
                    .document_frame()
                    .expect("mounted runtime document")
                    .nodes
                    .values()
                    .flat_map(|node| node.source_bindings.iter())
                    .filter(|binding| binding.source_path == step.source_path)
                    .map(|binding| (binding.source_path.clone(), binding.intent.clone()))
                    .take(16)
                    .collect::<Vec<_>>();
                panic!(
                    "missing addressed target for {}; target_text={:?}; address={:?}; document bindings={available:?}; runtime bindings={runtime_bindings:?}; candidates={candidates:?}",
                    step.source_path,
                    step.target_text,
                    step.address
                )
            });
        let mut dirty = false;
        let pointer_cycles = usize::from(
            step.action_kind.as_deref() == Some("double_click")
                || target.source_intent.as_deref() == Some("double_click"),
        ) + 1;
        for _ in 0..pointer_cycles {
            for phase in [PointerPhase::Move, PointerPhase::Down, PointerPhase::Up] {
                dirty |= model
                    .handle_event(
                        &HostEvent::Pointer(PointerEvent {
                            surface: surface.clone(),
                            x: target.center_x,
                            y: target.center_y,
                            phase,
                            button: (phase != PointerPhase::Move).then_some(PointerButton::Primary),
                        }),
                        Some(target.clone()),
                    )
                    .unwrap();
            }
        }
        if let Some(text) = &step.text {
            dirty |= model
                .handle_event(
                    &HostEvent::TextInput(TextInputEvent {
                        surface: surface.clone(),
                        text: text.clone(),
                    }),
                    None,
                )
                .unwrap();
        }
        if let Some(key) = &step.key {
            dirty |= model
                .handle_event(
                    &HostEvent::Keyboard(KeyEvent {
                        surface: surface.clone(),
                        physical_key: None,
                        logical_key: LogicalKey::Named(key.clone()),
                        pressed: true,
                    }),
                    None,
                )
                .unwrap();
        }
        if step.action_kind.as_deref() == Some("blur")
            || target.source_intent.as_deref() == Some("blur")
        {
            dirty |= model
                .handle_event(
                    &HostEvent::Focus {
                        surface,
                        focused: false,
                    },
                    None,
                )
                .unwrap();
        }
        assert!(
            model.event_sequence() > sequence_before
                && model.last_dispatched_source() == Some(step.source_path.as_str()),
            "public host events did not dispatch {} ({:?}); target={}; focused={:?}; key={:?}; text={:?}; focused_bindings={:?}",
            step.source_path,
            target.source_intent,
            target.node,
            model.focused(),
            step.key,
            step.text,
            model
                .focused()
                .and_then(|focused| model
                    .runtime
                    .document_frame()
                    .expect("mounted runtime document")
                    .nodes
                    .get(&DocumentNodeId(focused.to_owned())))
                .map(|node| node
                    .source_bindings
                    .iter()
                    .map(|binding| (binding.source_path.clone(), binding.intent.clone()))
                    .collect::<Vec<_>>())
        );
        if dirty {
            view.apply_patches(model.take_patches(), columns).unwrap();
            view.set_interaction_state(model.hovered(), model.focused(), columns)
                .unwrap();
        }
        converge_test_demands(model, view, columns);
    }

    fn converge_test_demands(
        model: &mut RuntimeView,
        view: &mut crate::view::RetainedView,
        columns: &mut boon_document::render_scene::ApproximateTextColumnMeasurer,
    ) {
        for _ in 0..8 {
            let demands = view.demands().to_vec();
            if !model.apply_layout_demands(&demands).unwrap() {
                return;
            }
            view.apply_patches(model.take_patches(), columns).unwrap();
        }
        panic!("typed document demands did not converge");
    }

    #[test]
    fn core_examples_test_slice_dispatches_declared_public_host_events() {
        let catalog = crate::catalog::Catalog::load().unwrap();
        for example_id in ["counter", "todomvc", "cells"] {
            let example = catalog.open(example_id).unwrap();
            let units = example
                .units
                .iter()
                .map(|unit| RuntimeSourceUnit {
                    path: unit.path.clone(),
                    source: unit.source.clone(),
                })
                .collect::<Vec<_>>();
            let runtime =
                LiveRuntime::from_project(&format!("examples/{example_id}.bn"), &units).unwrap();
            let mount = runtime.mount();
            let mut model = RuntimeView::mount(runtime, mount).unwrap();
            let mut columns = boon_document::render_scene::ApproximateTextColumnMeasurer;
            let mut view = crate::view::RetainedView::new(
                model.frame(),
                boon_host::Viewport {
                    surface: 1,
                    width: 1_100.0,
                    height: 760.0,
                    scale: 1.0,
                },
                &mut columns,
            )
            .unwrap();
            converge_test_demands(&mut model, &mut view, &mut columns);

            for step in example
                .test_steps
                .iter()
                .take(crate::preview::TEST_STEP_LIMIT)
            {
                drive_scenario_step(&mut model, &mut view, &mut columns, step);
            }
            assert!(
                model.event_sequence()
                    >= example
                        .test_steps
                        .len()
                        .min(crate::preview::TEST_STEP_LIMIT) as u64,
                "{example_id} missed source events"
            );
        }
    }

    #[test]
    fn counter_public_pointer_sequence_crosses_zero_without_rebuilding() {
        let example = crate::catalog::Catalog::load()
            .unwrap()
            .open("counter")
            .unwrap();
        let units = example
            .units
            .into_iter()
            .map(|unit| RuntimeSourceUnit {
                path: unit.path,
                source: unit.source,
            })
            .collect::<Vec<_>>();
        let runtime = LiveRuntime::from_project("examples/counter.bn", &units).unwrap();
        let mount = runtime.mount();
        let mut model = RuntimeView::mount(runtime, mount).unwrap();
        let mut columns = boon_document::render_scene::ApproximateTextColumnMeasurer;
        let mut view = crate::view::RetainedView::new(
            model.frame(),
            boon_host::Viewport {
                surface: 1,
                width: 980.0,
                height: 760.0,
                scale: 1.0,
            },
            &mut columns,
        )
        .unwrap();
        let initial_full_lowers = view.retained_stats().full_lower_count;

        for step in &example.test_steps {
            let target = view
                .target_for_source(&step.source_path, step.target_text.as_deref())
                .unwrap_or_else(|| panic!("missing target {}", step.source_path));
            for phase in [PointerPhase::Move, PointerPhase::Down, PointerPhase::Up] {
                let changed = model
                    .handle_event(
                        &HostEvent::Pointer(PointerEvent {
                            surface: SurfaceId("preview".to_owned()),
                            x: target.center_x,
                            y: target.center_y,
                            phase,
                            button: (phase != PointerPhase::Move).then_some(PointerButton::Primary),
                        }),
                        Some(target.clone()),
                    )
                    .unwrap();
                if changed {
                    view.apply_patches(model.take_patches(), &mut columns)
                        .unwrap();
                    view.set_interaction_state(model.hovered(), model.focused(), &mut columns)
                        .unwrap();
                }
            }
        }

        assert_eq!(view.retained_stats().full_lower_count, initial_full_lowers);
    }
}
