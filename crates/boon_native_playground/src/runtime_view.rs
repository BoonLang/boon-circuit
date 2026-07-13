use boon_document::{
    DocumentFrame, DocumentNodeId, DocumentNodeKind, DocumentState, LayoutDemand, StylePatch,
    StyleValue, TextValue,
};
use boon_editor::{Buffer, Command, Position};
use boon_host::{HostEvent, PointerButton, PointerPhase};
use boon_runtime::{
    DocumentPatch, DocumentPatchStatus, LiveRuntime, RowId, RuntimePhaseTimings, RuntimeTurn,
    SourcePayload, Value,
};
use std::time::{Duration, Instant};

use crate::view::HitTarget;

type ViewResult<T> = Result<T, String>;

const DOUBLE_CLICK_INTERVAL: Duration = Duration::from_millis(500);
const CARET_BLINK_INTERVAL: Duration = Duration::from_millis(500);

struct ScheduledSource {
    path: String,
    interval: Duration,
    next: Instant,
}

struct TextInputState {
    buffer: Buffer,
    caret_visible: bool,
    next_blink_at: Option<Instant>,
}

impl TextInputState {
    fn new(text: &str) -> Self {
        Self {
            buffer: Buffer::new(text),
            caret_visible: true,
            next_blink_at: None,
        }
    }

    fn reset(&mut self, text: &str, column: usize) {
        self.buffer = Buffer::new(text);
        self.buffer.set_caret(Position { line: 0, column }, false);
        self.reset_blink();
    }

    fn reset_blink(&mut self) {
        self.caret_visible = true;
        self.next_blink_at = Some(Instant::now() + CARET_BLINK_INTERVAL);
    }
}

#[derive(Default)]
struct InputModifiers {
    shift: bool,
    control: bool,
    alt: bool,
    meta: bool,
}

pub struct RuntimeView {
    runtime: LiveRuntime,
    hovered: Option<String>,
    pressed: Option<String>,
    focused: Option<String>,
    text_inputs: std::collections::BTreeMap<String, TextInputState>,
    text_drag: Option<String>,
    modifiers: InputModifiers,
    scroll_offsets: std::collections::BTreeMap<String, boon_document_model::ScrollState>,
    materialization_overscan: std::collections::BTreeMap<u64, std::ops::Range<u64>>,
    pending_patches: Vec<DocumentPatch>,
    sequence: u64,
    last_dispatched_source: Option<String>,
    pending_external_url: Option<String>,
    last_primary_click: Option<(String, Instant)>,
    last_runtime_phase: RuntimePhaseTimings,
    scheduled_sources: Vec<ScheduledSource>,
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
        if let Some(source) = runtime
            .source_inventory()
            .sources
            .iter()
            .find(|source| source.interval_ms == Some(0))
        {
            return Err(format!(
                "scheduled source `{}` has a zero interval",
                source.path
            ));
        }
        debug_assert_eq!(mounted.frame(), frame);
        let text_inputs = frame
            .nodes
            .values()
            .filter(|node| node.kind == DocumentNodeKind::TextInput)
            .map(|node| {
                (
                    node.id.0.clone(),
                    TextInputState::new(
                        node.text
                            .as_ref()
                            .map(|text| text.text.as_str())
                            .unwrap_or_default(),
                    ),
                )
            })
            .collect();
        let now = Instant::now();
        let scheduled_sources = runtime
            .source_inventory()
            .sources
            .iter()
            .filter_map(|source| {
                let interval = Duration::from_millis(source.interval_ms?);
                Some(ScheduledSource {
                    path: source.path.clone(),
                    interval,
                    next: now + interval,
                })
            })
            .collect();
        Ok(Self {
            runtime,
            hovered: None,
            pressed: None,
            focused: None,
            text_inputs,
            text_drag: None,
            modifiers: InputModifiers::default(),
            scroll_offsets: std::collections::BTreeMap::new(),
            materialization_overscan: std::collections::BTreeMap::new(),
            pending_patches: Vec::new(),
            sequence: 0,
            last_dispatched_source: None,
            pending_external_url: None,
            last_primary_click: None,
            last_runtime_phase: RuntimePhaseTimings::default(),
            scheduled_sources,
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

    pub fn take_external_url(&mut self) -> Option<String> {
        self.pending_external_url.take()
    }

    pub fn last_runtime_phase(&self) -> RuntimePhaseTimings {
        self.last_runtime_phase
    }

    pub fn scheduled_source_deadline(&self) -> Option<Instant> {
        self.scheduled_sources
            .iter()
            .map(|source| source.next)
            .min()
    }

    pub fn advance_scheduled_sources(&mut self, now: Instant) -> ViewResult<bool> {
        let mut due = Vec::new();
        for source in &mut self.scheduled_sources {
            if source.next > now {
                continue;
            }
            due.push(source.path.clone());
            source.next += source.interval;
            if source.next <= now {
                source.next = now + source.interval;
            }
        }
        let mut changed = false;
        for path in due {
            changed |= self.dispatch_source(&path, None, SourcePayload::default())?;
        }
        Ok(changed)
    }

    pub fn inspect_root_current(&mut self, path: &str) -> ViewResult<String> {
        let value = self
            .runtime
            .inspect_value_current(path, 8)
            .map_err(|error| error.to_string())?;
        Ok(format_inspection_value(&value, 0))
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
            if self
                .materialization_overscan
                .get(&materialization)
                .is_some_and(|current| current.start <= visible.start && current.end >= visible.end)
            {
                continue;
            }
            let patches = self
                .runtime
                .demand_document_window_by_id(materialization, visible, overscan.clone())
                .map_err(|error| error.to_string())?;
            self.materialization_overscan
                .insert(materialization, overscan);
            for patch in patches {
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
                    let next = target.as_ref().map(|target| target.node.clone());
                    let hover_changed = next != self.hovered;
                    self.hovered = next;
                    let selection_changed = if let (Some(drag), Some(target)) =
                        (self.text_drag.clone(), target.as_ref())
                        && target.node == drag
                        && let Some(column) = target.text_column
                    {
                        self.set_text_input_caret(&drag, column, true)
                    } else {
                        false
                    };
                    let source_changed = if let Some(target) = target.as_ref() {
                        self.dispatch_pointer_intent(
                            target,
                            &["pointer_move", "move"],
                            pointer_source_payload(pointer, target),
                        )?
                    } else {
                        false
                    };
                    Ok(hover_changed || selection_changed || source_changed)
                }
                PointerPhase::Leave => {
                    self.text_drag = None;
                    Ok(self.hovered.take().is_some())
                }
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
                        self.sync_text_input_from_document(&previous, None);
                        self.queue_text_input_overlay(&previous);
                    }
                    self.focused = next_focus;
                    if let Some(target) = target.as_ref().filter(|target| {
                        self.focused.as_deref() == Some(target.node.as_str())
                            && self.target_is_text_input(target)
                    }) {
                        self.sync_text_input_from_document(&target.node, target.text_column);
                        self.text_drag = Some(target.node.clone());
                        self.queue_text_input_overlay(&target.node);
                    } else {
                        self.text_drag = None;
                    }
                    Ok(dirty || focus_requires_immediate_present || changed)
                }
                PointerPhase::Up if pointer.button == Some(PointerButton::Primary) => {
                    self.text_drag = None;
                    let matches = self.pressed.take().as_deref()
                        == target.as_ref().map(|target| target.node.as_str());
                    if matches {
                        if let Some(target) = target {
                            if let Some(url) = self.external_url_for_node(&target.node) {
                                self.pending_external_url = Some(url);
                                return Ok(true);
                            }
                            if target.source_intent.as_deref() == Some("double_click") {
                                let now = Instant::now();
                                let is_double_click =
                                    self.last_primary_click.take().is_some_and(|(node, at)| {
                                        node == target.node
                                            && now.saturating_duration_since(at)
                                                <= DOUBLE_CLICK_INTERVAL
                                    });
                                if is_double_click {
                                    return self.dispatch_target(
                                        &target,
                                        pointer_source_payload(pointer, &target),
                                    );
                                }
                                self.last_primary_click = Some((target.node, now));
                                return Ok(false);
                            }
                            if pointer_activation_intent(target.source_intent.as_deref())
                                && !self.bare_source_is_text_input(&target)
                            {
                                return self.dispatch_target(
                                    &target,
                                    pointer_source_payload(pointer, &target),
                                );
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
                self.edit_focused_text(Command::InsertPlain(single_line_text(&text.text)))
            }
            HostEvent::Ime(ime) => match &ime.kind {
                boon_host::ImeInputKind::Commit { text } => {
                    self.edit_focused_text(Command::InsertPlain(single_line_text(text)))
                }
                boon_host::ImeInputKind::DeleteSurrounding {
                    before_bytes,
                    after_bytes,
                } => self.delete_surrounding(*before_bytes, *after_bytes),
                _ => Ok(false),
            },
            HostEvent::Keyboard(key) => self.handle_keyboard(key),
            HostEvent::Focus { focused: false, .. } => {
                let previous = self.focused.take();
                self.text_drag = None;
                self.modifiers = InputModifiers::default();
                let Some(previous) = previous else {
                    return Ok(false);
                };
                self.dispatch_node_intent(
                    &previous,
                    &["blur", "source"],
                    SourcePayload::default(),
                )?;
                self.sync_text_input_from_document(&previous, None);
                self.queue_text_input_overlay(&previous);
                Ok(true)
            }
            _ => Ok(false),
        }
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
            bounds_x: 0.0,
            bounds_y: 0.0,
            bounds_width: 0.0,
            bounds_height: 0.0,
            text_column: None,
        };
        if payload.text.is_none()
            && (matches!(binding.intent.as_str(), "commit" | "submit" | "blur")
                || (node.kind == DocumentNodeKind::TextInput
                    && (binding.intent == "source" || payload.key.is_some())))
        {
            payload.text = self
                .text_inputs
                .get(node_id)
                .map(|state| state.buffer.text())
                .or_else(|| node.text.as_ref().map(|text| text.text.clone()));
        }
        self.dispatch_target(&target, payload)
    }

    fn dispatch_pointer_intent(
        &mut self,
        target: &HitTarget,
        intents: &[&str],
        payload: SourcePayload,
    ) -> ViewResult<bool> {
        let binding = self
            .runtime
            .document_frame()
            .and_then(|frame| frame.nodes.get(&DocumentNodeId(target.node.clone())))
            .and_then(|node| {
                intents.iter().find_map(|intent| {
                    node.source_bindings
                        .iter()
                        .find(|binding| binding.intent == *intent)
                })
            })
            .cloned();
        let Some(binding) = binding else {
            return Ok(false);
        };
        let mut routed = target.clone();
        routed.source_path = Some(binding.source_path);
        routed.source_intent = Some(binding.intent);
        self.dispatch_target(&routed, payload)
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

    fn external_url_for_node(&self, node_id: &str) -> Option<String> {
        let value = self
            .runtime
            .document_frame()?
            .nodes
            .get(&DocumentNodeId(node_id.to_owned()))?
            .style
            .get("to")?;
        let StyleValue::Text(url) = value else {
            return None;
        };
        (url.starts_with("https://") || url.starts_with("http://")).then(|| url.clone())
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
        let row = if self.runtime.source_is_row_scoped(path) == Some(true) {
            self.row_target(path, target.row_key, target.row_generation)?
        } else {
            None
        };
        self.dispatch_source(path, row, payload)
    }

    fn dispatch_source(
        &mut self,
        path: &str,
        row: Option<RowId>,
        payload: SourcePayload,
    ) -> ViewResult<bool> {
        self.sequence = self.sequence.saturating_add(1);
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

    pub fn caret_blink_deadline(&self) -> Option<Instant> {
        self.focused
            .as_ref()
            .and_then(|focused| self.text_inputs.get(focused))
            .and_then(|state| state.next_blink_at)
    }

    pub fn advance_caret_blink(&mut self, now: Instant) -> bool {
        let Some(focused) = self.focused.clone() else {
            return false;
        };
        let Some(state) = self.text_inputs.get_mut(&focused) else {
            return false;
        };
        if state.next_blink_at.is_none_or(|deadline| deadline > now) {
            return false;
        }
        state.caret_visible = !state.caret_visible;
        state.next_blink_at = Some(now + CARET_BLINK_INTERVAL);
        self.queue_text_input_style(&focused);
        true
    }

    fn set_text_input_caret(&mut self, id: &str, column: usize, extend: bool) -> bool {
        let Some(state) = self.text_inputs.get_mut(id) else {
            return false;
        };
        let changed = state.buffer.set_caret(Position { line: 0, column }, extend);
        state.reset_blink();
        self.queue_text_input_style(id);
        let _ = changed;
        true
    }

    fn edit_focused_text(&mut self, command: Command) -> ViewResult<bool> {
        let Some(focused) = self.focused.clone() else {
            return Ok(false);
        };
        let Some(state) = self.text_inputs.get_mut(&focused) else {
            return Ok(false);
        };
        if !state.buffer.apply(command) {
            return Ok(false);
        }
        state.reset_blink();
        self.finish_focused_edit(&focused)
    }

    fn finish_focused_edit(&mut self, focused: &str) -> ViewResult<bool> {
        let text = self
            .text_inputs
            .get(focused)
            .map(|state| state.buffer.text())
            .unwrap_or_default();
        let runtime_changed = self.dispatch_node_intent(
            focused,
            &["change", "text", "input", "source"],
            SourcePayload {
                text: Some(text),
                ..SourcePayload::default()
            },
        )?;
        self.queue_text_input_overlay(focused);
        let _ = runtime_changed;
        Ok(true)
    }

    fn delete_surrounding(&mut self, before_bytes: u32, after_bytes: u32) -> ViewResult<bool> {
        let Some(focused) = self.focused.clone() else {
            return Ok(false);
        };
        let Some(state) = self.text_inputs.get_mut(&focused) else {
            return Ok(false);
        };
        let mut changed = false;
        for _ in 0..before_bytes {
            changed |= state.buffer.apply(Command::DeleteBackward);
        }
        for _ in 0..after_bytes {
            changed |= state.buffer.apply(Command::DeleteForward);
        }
        if !changed {
            return Ok(false);
        }
        state.reset_blink();
        self.finish_focused_edit(&focused)
    }

    fn handle_keyboard(&mut self, key: &boon_host::KeyEvent) -> ViewResult<bool> {
        let value = logical_key_text(&key.logical_key);
        if update_modifier(&mut self.modifiers, &value, key.pressed) {
            return Ok(false);
        }
        if !key.pressed {
            return Ok(false);
        }
        let Some(focused) = self.focused.clone() else {
            return Ok(false);
        };
        if !self.text_inputs.contains_key(&focused) {
            return self.dispatch_node_intent(
                &focused,
                &["key_down", "source"],
                SourcePayload {
                    key: Some(value),
                    ..SourcePayload::default()
                },
            );
        }

        let normalized = normalize_key(&value);
        if self.modifiers.control || self.modifiers.meta {
            match normalized.as_str() {
                "a" => {
                    let state = self.text_inputs.get_mut(&focused).expect("focused input");
                    let changed = state.buffer.apply(Command::SelectAll);
                    state.reset_blink();
                    self.queue_text_input_style(&focused);
                    let _ = changed;
                    return Ok(true);
                }
                "c" => {
                    self.copy_selection_to_clipboard(&focused, false)?;
                    return Ok(false);
                }
                "x" => return self.copy_selection_to_clipboard(&focused, true),
                "v" => {
                    if let Ok(mut clipboard) = arboard::Clipboard::new()
                        && let Ok(text) = clipboard.get_text()
                    {
                        return self
                            .edit_focused_text(Command::InsertPlain(single_line_text(&text)));
                    }
                    return Ok(false);
                }
                "z" if self.modifiers.shift => {
                    return self.edit_focused_text(Command::Redo);
                }
                "z" => return self.edit_focused_text(Command::Undo),
                "y" => return self.edit_focused_text(Command::Redo),
                _ => return Ok(false),
            }
        }

        let extend = self.modifiers.shift;
        let command = match normalized.as_str() {
            "left" => Some(Command::MoveLeft { extend }),
            "right" => Some(Command::MoveRight { extend }),
            "home" => Some(Command::MoveHome { extend }),
            "end" => Some(Command::MoveEnd { extend }),
            "backspace" => Some(Command::DeleteBackward),
            "delete" => Some(Command::DeleteForward),
            _ => None,
        };
        if let Some(command) = command {
            if matches!(&command, Command::DeleteBackward | Command::DeleteForward) {
                return self.edit_focused_text(command);
            }
            let state = self.text_inputs.get_mut(&focused).expect("focused input");
            let changed = state.buffer.apply(command);
            state.reset_blink();
            self.queue_text_input_style(&focused);
            let _ = changed;
            return Ok(true);
        }

        if normalized == "enter" {
            let changed = self.dispatch_node_intent(
                &focused,
                &["commit", "submit", "key_down", "source"],
                SourcePayload {
                    key: Some("Enter".to_owned()),
                    ..SourcePayload::default()
                },
            )?;
            self.sync_text_input_from_document(&focused, None);
            self.queue_text_input_overlay(&focused);
            let _ = changed;
            return Ok(true);
        }
        if normalized == "escape" {
            let changed = self.dispatch_node_intent(
                &focused,
                &["cancel", "escape", "key_down", "source"],
                SourcePayload {
                    key: Some("Escape".to_owned()),
                    ..SourcePayload::default()
                },
            )?;
            self.sync_text_input_from_document(&focused, None);
            self.queue_text_input_overlay(&focused);
            let _ = changed;
            return Ok(true);
        }
        Ok(false)
    }

    fn copy_selection_to_clipboard(&mut self, focused: &str, cut: bool) -> ViewResult<bool> {
        let Some(state) = self.text_inputs.get_mut(focused) else {
            return Ok(false);
        };
        let selected = state.buffer.selected_text();
        if selected.is_empty() {
            return Ok(false);
        }
        if let Ok(mut clipboard) = arboard::Clipboard::new() {
            let _ = clipboard.set_text(selected);
        }
        if !cut || !state.buffer.apply(Command::InsertPlain(String::new())) {
            return Ok(false);
        }
        state.reset_blink();
        self.finish_focused_edit(focused)
    }

    fn sync_text_input_from_document(&mut self, id: &str, column: Option<usize>) {
        let Some(text) = self
            .runtime
            .document_frame()
            .and_then(|frame| frame.nodes.get(&DocumentNodeId(id.to_owned())))
            .filter(|node| node.kind == DocumentNodeKind::TextInput)
            .map(|node| {
                node.text
                    .as_ref()
                    .map(|text| text.text.clone())
                    .unwrap_or_default()
            })
        else {
            return;
        };
        let state = self
            .text_inputs
            .entry(id.to_owned())
            .or_insert_with(|| TextInputState::new(&text));
        state.reset(&text, column.unwrap_or(usize::MAX));
    }

    fn queue_text_input_overlay(&mut self, id: &str) {
        let Some(state) = self.text_inputs.get(id) else {
            return;
        };
        self.pending_patches.push(DocumentPatch::SetText {
            id: DocumentNodeId(id.to_owned()),
            text: TextValue {
                text: state.buffer.text(),
            },
        });
        self.queue_text_input_style(id);
    }

    fn queue_text_input_style(&mut self, id: &str) {
        let Some(state) = self.text_inputs.get(id) else {
            return;
        };
        let focused = self.focused.as_deref() == Some(id);
        let selection = state.buffer.selection();
        let (start, end) = if selection.anchor <= selection.head {
            (selection.anchor.column, selection.head.column)
        } else {
            (selection.head.column, selection.anchor.column)
        };
        let mut patch = StylePatch::new();
        patch.insert(
            "caret_visible".to_owned(),
            Some(StyleValue::Bool(focused && state.caret_visible)),
        );
        patch.insert(
            "caret_column".to_owned(),
            Some(StyleValue::Number(state.buffer.caret().column as f64)),
        );
        patch.insert(
            "selection_start".to_owned(),
            (focused && start != end).then_some(StyleValue::Number(start as f64)),
        );
        patch.insert(
            "selection_end".to_owned(),
            (focused && start != end).then_some(StyleValue::Number(end as f64)),
        );
        self.pending_patches.push(DocumentPatch::SetStyle {
            id: DocumentNodeId(id.to_owned()),
            patch,
        });
    }

    fn sync_text_input_patch(&mut self, patch: &DocumentPatch) {
        match patch {
            DocumentPatch::UpsertNode(node) if node.kind == DocumentNodeKind::TextInput => {
                if self.focused.as_deref() != Some(node.id.0.as_str()) {
                    self.text_inputs.insert(
                        node.id.0.clone(),
                        TextInputState::new(
                            node.text
                                .as_ref()
                                .map(|text| text.text.as_str())
                                .unwrap_or_default(),
                        ),
                    );
                }
            }
            DocumentPatch::SetText { id, text }
                if self.focused.as_deref() != Some(id.0.as_str()) =>
            {
                self.text_inputs
                    .insert(id.0.clone(), TextInputState::new(&text.text));
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

fn single_line_text(text: &str) -> String {
    text.replace("\r\n", " ").replace(['\r', '\n'], " ")
}

fn logical_key_text(key: &boon_host::LogicalKey) -> String {
    match key {
        boon_host::LogicalKey::Character(value) | boon_host::LogicalKey::Named(value) => {
            value.clone()
        }
        boon_host::LogicalKey::Dead(Some(value)) => value.to_string(),
        boon_host::LogicalKey::Dead(None) | boon_host::LogicalKey::Unidentified => String::new(),
    }
}

fn normalize_key(value: &str) -> String {
    match value.to_ascii_lowercase().as_str() {
        "arrowleft" | "leftarrow" => "left".to_owned(),
        "arrowright" | "rightarrow" => "right".to_owned(),
        "back_space" => "backspace".to_owned(),
        "return" | "kp_enter" => "enter".to_owned(),
        value => value.to_owned(),
    }
}

fn update_modifier(modifiers: &mut InputModifiers, value: &str, pressed: bool) -> bool {
    let normalized = value.to_ascii_lowercase();
    let target = if normalized == "shift" || normalized.starts_with("shift_") {
        Some(&mut modifiers.shift)
    } else if matches!(normalized.as_str(), "control" | "ctrl")
        || normalized.starts_with("control_")
        || normalized.starts_with("ctrl_")
    {
        Some(&mut modifiers.control)
    } else if normalized == "alt" || normalized.starts_with("alt_") {
        Some(&mut modifiers.alt)
    } else if matches!(normalized.as_str(), "meta" | "super")
        || normalized.starts_with("meta_")
        || normalized.starts_with("super_")
    {
        Some(&mut modifiers.meta)
    } else {
        None
    };
    if let Some(target) = target {
        *target = pressed;
        true
    } else {
        false
    }
}

fn format_inspection_value(value: &Value, depth: usize) -> String {
    const MAX_DEPTH: usize = 4;
    const MAX_ITEMS: usize = 24;
    const MAX_TEXT: usize = 256;
    if depth >= MAX_DEPTH {
        return "...".to_owned();
    }
    match value {
        Value::Null => "Null".to_owned(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        Value::Text(value) => {
            let mut bounded = value.chars().take(MAX_TEXT).collect::<String>();
            if value.chars().count() > MAX_TEXT {
                bounded.push_str("...");
            }
            format!("\"{bounded}\"")
        }
        Value::Bytes(value) => format!("Bytes[{}]", value.len()),
        Value::List(values) => {
            let mut parts = values
                .iter()
                .take(MAX_ITEMS)
                .map(|value| format_inspection_value(value, depth + 1))
                .collect::<Vec<_>>();
            if values.len() > MAX_ITEMS {
                parts.push(format!("... {} more", values.len() - MAX_ITEMS));
            }
            format!("[{}]", parts.join(", "))
        }
        Value::Record(fields) => {
            let mut parts = fields
                .iter()
                .take(MAX_ITEMS)
                .map(|(name, value)| {
                    format!("{name}: {}", format_inspection_value(value, depth + 1))
                })
                .collect::<Vec<_>>();
            if fields.len() > MAX_ITEMS {
                parts.push(format!("... {} more", fields.len() - MAX_ITEMS));
            }
            format!("[{}]", parts.join(", "))
        }
        Value::MappedRow { id, fields } => {
            let mut parts = fields
                .iter()
                .take(MAX_ITEMS)
                .map(|(name, value)| {
                    format!("{name}: {}", format_inspection_value(value, depth + 1))
                })
                .collect::<Vec<_>>();
            if fields.len() > MAX_ITEMS {
                parts.push(format!("... {} more", fields.len() - MAX_ITEMS));
            }
            format!(
                "MappedRow(list={}, key={}, generation={}, [{}])",
                id.list.0,
                id.key,
                id.generation,
                parts.join(", ")
            )
        }
        Value::Row { id, fields } => format!(
            "Row(list={}, key={}, generation={}, fields={})",
            id.list.0,
            id.key,
            id.generation,
            fields.len()
        ),
        Value::Error { code } => format!("Error[{code}]"),
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

fn pointer_source_payload(pointer: &boon_host::PointerEvent, target: &HitTarget) -> SourcePayload {
    let mut payload = SourcePayload::default();
    if target.bounds_width.is_finite()
        && target.bounds_height.is_finite()
        && target.bounds_width > 0.0
        && target.bounds_height > 0.0
    {
        let local_x = (pointer.x - target.bounds_x).clamp(0.0, target.bounds_width);
        let local_y = (pointer.y - target.bounds_y).clamp(0.0, target.bounds_height);
        payload.fields.insert(
            "pointer_x".to_owned(),
            Value::Number(local_x.round() as i64),
        );
        payload.fields.insert(
            "pointer_y".to_owned(),
            Value::Number(local_y.round() as i64),
        );
        payload.fields.insert(
            "pointer_width".to_owned(),
            Value::Number(target.bounds_width.round() as i64),
        );
        payload.fields.insert(
            "pointer_height".to_owned(),
            Value::Number(target.bounds_height.round() as i64),
        );
    }
    payload
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
                width: 440.0,
                height: 680.0,
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
        let scroll_target = target.clone();
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
        assert!(
            !model.apply_layout_demands(view.demands()).unwrap(),
            "scrolling inside retained overscan must not rematerialize rows"
        );
        assert!(
            view.frame()
                .nodes
                .values()
                .any(|node| node.scroll.is_some_and(|scroll| scroll.y == 52.0))
        );
        assert!(
            model
                .handle_event(
                    &HostEvent::Wheel(WheelEvent {
                        surface: SurfaceId("preview".to_owned()),
                        x: scroll_target.center_x,
                        y: scroll_target.center_y,
                        delta_x: 0.0,
                        delta_y: 520.0,
                    }),
                    Some(scroll_target),
                )
                .unwrap()
        );
        view.apply_patches(model.take_patches(), &mut columns)
            .unwrap();
        assert!(
            model.apply_layout_demands(view.demands()).unwrap(),
            "leaving retained overscan must request a new materialization window"
        );
        view.apply_patches(model.take_patches(), &mut columns)
            .unwrap();
        assert!(!model.apply_layout_demands(view.demands()).unwrap());
    }

    #[test]
    fn text_input_supports_caret_editing_selection_cancel_and_blink() {
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
                width: 510.0,
                height: 540.0,
                scale: 1.0,
            },
            &mut columns,
        )
        .unwrap();
        converge_test_demands(&mut model, &mut view, &mut columns);

        let surface = SurfaceId("preview".to_owned());
        let mut target = view
            .target_for_source("cell.sources.editor.change", None)
            .expect("formula text input");
        target.text_column = Some(0);
        let mut focused_dirty = false;
        for phase in [PointerPhase::Down, PointerPhase::Up] {
            focused_dirty |= model
                .handle_event(
                    &HostEvent::Pointer(PointerEvent {
                        surface: surface.clone(),
                        x: target.center_x,
                        y: target.center_y,
                        phase,
                        button: Some(PointerButton::Primary),
                    }),
                    Some(target.clone()),
                )
                .unwrap();
        }
        assert!(focused_dirty);
        view.apply_patches(model.take_patches(), &mut columns)
            .unwrap();
        view.set_interaction_state(model.hovered(), model.focused(), &mut columns)
            .unwrap();
        let focused = model.focused().unwrap().to_owned();
        assert_eq!(model.text_inputs[&focused].buffer.caret().column, 0);
        assert!(
            view.frame().nodes[&DocumentNodeId(focused.clone())]
                .style
                .get("caret_visible")
                .is_some_and(|value| value == &StyleValue::Bool(true))
        );

        assert!(
            model
                .handle_event(
                    &HostEvent::TextInput(TextInputEvent {
                        surface: surface.clone(),
                        text: "=".to_owned(),
                    }),
                    None,
                )
                .unwrap()
        );
        assert_eq!(model.text_inputs[&focused].buffer.text(), "=5");
        assert!(
            model
                .handle_event(
                    &HostEvent::Keyboard(KeyEvent {
                        surface: surface.clone(),
                        physical_key: None,
                        logical_key: LogicalKey::Named("BackSpace".to_owned()),
                        pressed: true,
                    }),
                    None,
                )
                .unwrap()
        );
        assert_eq!(model.text_inputs[&focused].buffer.text(), "5");

        for (logical_key, pressed) in [
            (LogicalKey::Named("Control_L".to_owned()), true),
            (LogicalKey::Character("a".to_owned()), true),
            (LogicalKey::Character("a".to_owned()), false),
            (LogicalKey::Named("Control_L".to_owned()), false),
        ] {
            model
                .handle_event(
                    &HostEvent::Keyboard(KeyEvent {
                        surface: surface.clone(),
                        physical_key: None,
                        logical_key,
                        pressed,
                    }),
                    None,
                )
                .unwrap();
        }
        assert!(
            !model.text_inputs[&focused]
                .buffer
                .selection()
                .is_collapsed()
        );
        assert!(
            model
                .handle_event(
                    &HostEvent::TextInput(TextInputEvent {
                        surface: surface.clone(),
                        text: "=A1+1".to_owned(),
                    }),
                    None,
                )
                .unwrap()
        );
        assert_eq!(model.text_inputs[&focused].buffer.text(), "=A1+1");

        let blink_at = model.caret_blink_deadline().unwrap();
        assert!(model.advance_caret_blink(blink_at + Duration::from_millis(1)));
        view.apply_patches(model.take_patches(), &mut columns)
            .unwrap();
        assert!(
            view.frame().nodes[&DocumentNodeId(focused.clone())]
                .style
                .get("caret_visible")
                .is_some_and(|value| value == &StyleValue::Bool(false))
        );

        assert!(
            model
                .handle_event(
                    &HostEvent::Keyboard(KeyEvent {
                        surface: surface.clone(),
                        physical_key: None,
                        logical_key: LogicalKey::Named("Escape".to_owned()),
                        pressed: true,
                    }),
                    None,
                )
                .unwrap()
        );
        assert_eq!(model.text_inputs[&focused].buffer.text(), "5");

        assert!(
            model
                .handle_event(
                    &HostEvent::TextInput(TextInputEvent {
                        surface: surface.clone(),
                        text: "9".to_owned(),
                    }),
                    None,
                )
                .unwrap()
        );
        assert_eq!(model.text_inputs[&focused].buffer.text(), "59");
        assert!(
            model
                .handle_event(
                    &HostEvent::Keyboard(KeyEvent {
                        surface,
                        physical_key: None,
                        logical_key: LogicalKey::Named("Return".to_owned()),
                        pressed: true,
                    }),
                    None,
                )
                .unwrap()
        );
        assert_eq!(model.text_inputs[&focused].buffer.text(), "59");
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
        let target_point = crate::preview::test_step_pointer_position(view, &target, step);
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
                            x: target_point.0,
                            y: target_point.1,
                            phase,
                            button: (phase != PointerPhase::Move).then_some(PointerButton::Primary),
                        }),
                        Some(target.clone()),
                    )
                    .unwrap_or_else(|error| {
                        panic!(
                            "scenario action failed to dispatch {} ({phase:?}) to row {target_row:?}: {error}",
                            step.source_path
                        )
                    });
            }
        }
        if let Some(text) = &step.text {
            for (logical_key, pressed) in [
                (LogicalKey::Named("Control_L".to_owned()), true),
                (LogicalKey::Character("a".to_owned()), true),
                (LogicalKey::Character("a".to_owned()), false),
                (LogicalKey::Named("Control_L".to_owned()), false),
            ] {
                dirty |= model
                    .handle_event(
                        &HostEvent::Keyboard(KeyEvent {
                            surface: surface.clone(),
                            physical_key: None,
                            logical_key,
                            pressed,
                        }),
                        None,
                    )
                    .unwrap();
            }
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
        for example_id in ["counter", "todomvc", "cells", "novywave"] {
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
                    width: if example_id == "cells" {
                        510.0
                    } else {
                        1_100.0
                    },
                    height: if example_id == "cells" { 540.0 } else { 760.0 },
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
            if example_id == "cells" {
                for ordinal in 0..24 {
                    drive_scenario_step(
                        &mut model,
                        &mut view,
                        &mut columns,
                        &example.test_steps[ordinal % 2],
                    );
                }
                let step = example.test_steps.first().expect("Cells test target");
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
                    .expect("visible Cells target after TEST");
                model
                    .handle_event(
                        &HostEvent::Pointer(PointerEvent {
                            surface: SurfaceId("preview".to_owned()),
                            x: target.center_x,
                            y: target.center_y,
                            phase: PointerPhase::Move,
                            button: None,
                        }),
                        Some(target.clone()),
                    )
                    .unwrap();
                view.set_interaction_state(model.hovered(), model.focused(), &mut columns)
                    .unwrap();
                let wheel_target = view
                    .wheel_target(target.center_x, target.center_y, 0.0, 4.0)
                    .filter(|target| target.scroll_root.is_some())
                    .expect("post-TEST hovered Cells target must retain a vertical scroll owner");
                assert!(
                    model
                        .handle_event(
                            &HostEvent::Wheel(WheelEvent {
                                surface: SurfaceId("preview".to_owned()),
                                x: target.center_x,
                                y: target.center_y,
                                delta_x: 0.0,
                                delta_y: 4.0,
                            }),
                            Some(wheel_target),
                        )
                        .unwrap(),
                    "wheel event must enqueue a retained scroll patch"
                );
                let scroll_update = view
                    .apply_patches(model.take_patches(), &mut columns)
                    .unwrap();
                assert!(
                    scroll_update.layout_changed || scroll_update.render_changed,
                    "wheel patch must visibly update retained layout or rendering"
                );
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
    fn novywave_test_slice_builds_complete_loaded_render_scene() {
        let example = crate::catalog::Catalog::load()
            .unwrap()
            .open("novywave")
            .unwrap();
        let units = example
            .units
            .iter()
            .map(|unit| RuntimeSourceUnit {
                path: unit.path.clone(),
                source: unit.source.clone(),
            })
            .collect::<Vec<_>>();
        let runtime = LiveRuntime::from_project("examples/novywave/RUN.bn", &units).unwrap();
        let mount = runtime.mount();
        let mut model = RuntimeView::mount(runtime, mount).unwrap();
        let mut columns = boon_document::render_scene::ApproximateTextColumnMeasurer;
        let mut view = crate::view::RetainedView::new(
            model.frame(),
            boon_host::Viewport {
                surface: 1,
                width: 508.0,
                height: 540.0,
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

        let selected_lane_count = model
            .runtime
            .inspect_value_current("selected_lane_materialized_row_count", 1)
            .unwrap();
        assert_eq!(
            selected_lane_count,
            boon_runtime::Value::Number(3),
            "NovyWave selected lane model is not current"
        );

        for expected in [
            "Variables",
            "Selected Variables",
            "Value",
            "ghw.counter[3:0]",
            "ghw.enable",
            "ghw.state",
            "3",
            "1",
            "Count",
        ] {
            let node = view
                .frame()
                .nodes
                .values()
                .find(|node| node.text.as_ref().is_some_and(|text| text.text == expected))
                .unwrap_or_else(|| {
                    panic!(
                        "NovyWave loaded frame is missing `{expected}`; selected lane count={selected_lane_count:?}"
                    )
                });
            let bounds = view
                .node_bounds(&node.id.0)
                .unwrap_or_else(|| panic!("NovyWave `{expected}` has no retained layout bounds"));
            let vertical_limit = 540.0;
            assert!(
                bounds.width > 1.0
                    && bounds.height > 1.0
                    && bounds.x < 508.0
                    && bounds.y < vertical_limit
                    && bounds.x + bounds.width > 0.0
                    && bounds.y + bounds.height > 0.0,
                "NovyWave `{expected}` is outside the retained viewport: {bounds:?}"
            );
        }

        let fills = view
            .scene()
            .visual_primitives
            .iter()
            .filter(|primitive| {
                matches!(
                    primitive.primitive,
                    boon_document::RenderVisualPrimitiveKind::Fill
                )
            })
            .collect::<Vec<_>>();
        assert!(
            fills.iter().any(|primitive| {
                primitive.bounds.x >= 220.0
                    && primitive.bounds.y <= 60.0
                    && primitive.bounds.width >= 220.0
                    && primitive.bounds.height >= 300.0
                    && primitive.color[0] < 80
                    && primitive.color[1] < 80
                    && primitive.color[2] < 100
            }),
            "NovyWave Variables panel has no retained dark surface; large fills={:?}",
            fills
                .iter()
                .filter(|primitive| {
                    primitive.bounds.width >= 300.0 && primitive.bounds.height >= 100.0
                })
                .map(|primitive| (primitive.node.0.as_str(), primitive.bounds, primitive.color))
                .collect::<Vec<_>>()
        );
        assert!(
            fills
                .iter()
                .filter(|primitive| primitive.color[2] > 150)
                .count()
                >= 4,
            "NovyWave loaded waveform has no visible trace segments"
        );
    }

    #[test]
    fn novywave_all_scenario_steps_reach_retained_host_targets() {
        let example = crate::catalog::Catalog::load()
            .unwrap()
            .open("novywave")
            .unwrap();
        let units = example
            .units
            .iter()
            .map(|unit| RuntimeSourceUnit {
                path: unit.path.clone(),
                source: unit.source.clone(),
            })
            .collect::<Vec<_>>();
        let runtime = LiveRuntime::from_project("examples/novywave/RUN.bn", &units).unwrap();
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

        for step in &example.test_steps {
            drive_scenario_step(&mut model, &mut view, &mut columns, step);
        }

        assert_eq!(model.event_sequence(), example.test_steps.len() as u64);
        assert_eq!(
            model
                .runtime
                .inspect_value_current("cursor_position", 1)
                .unwrap(),
            Value::Text("Cursor48".to_owned())
        );
        assert_eq!(
            model
                .runtime
                .inspect_value_current("keyboard_cursor_label", 1)
                .unwrap(),
            Value::Text("150 s".to_owned())
        );
    }

    #[test]
    fn basic_examples_mount_render_and_schedule_real_intervals() {
        let catalog = crate::catalog::Catalog::load().unwrap();
        for (example_id, expected_text) in [
            ("minimal", "Minimal"),
            ("hello_world", "Hello, world!"),
            ("counter_latest", "Counter without HOLD"),
            ("fibonacci", "Position 10 is 55"),
            ("interval_latest", "Interval without HOLD"),
            ("interval_hold", "Interval with HOLD"),
            ("flow_operators", "LATEST, THEN, WHEN, WHILE"),
            ("layers", "Front layer"),
            ("pages", "Pages"),
        ] {
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
                    width: 980.0,
                    height: 760.0,
                    scale: 1.0,
                },
                &mut columns,
            )
            .unwrap();
            converge_test_demands(&mut model, &mut view, &mut columns);
            assert!(
                view.scene()
                    .text_runs
                    .iter()
                    .any(|run| run.text == expected_text),
                "{example_id} did not render {expected_text:?}; runs={:?}",
                view.scene()
                    .text_runs
                    .iter()
                    .map(|run| run.text.as_str())
                    .collect::<Vec<_>>()
            );

            if example_id.starts_with("interval_") {
                let deadline = model
                    .scheduled_source_deadline()
                    .expect("interval example must expose a scheduled source");
                assert!(model.advance_scheduled_sources(deadline).unwrap());
                assert_eq!(model.inspect_root_current("store.count").unwrap(), "1");
                view.apply_patches(model.take_patches(), &mut columns)
                    .unwrap();
            } else {
                assert!(model.scheduled_source_deadline().is_none());
            }
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

        assert_eq!(example.test_steps.len(), 6);
        for (step, expected_count) in example
            .test_steps
            .iter()
            .zip(["1", "2", "1", "0", "-1", "0"])
        {
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
            assert_eq!(
                model.inspect_root_current("store.count").unwrap(),
                expected_count
            );
            assert_eq!(
                model.inspect_root_current("count").unwrap(),
                expected_count,
                "the HOLD state name and qualified field must expose the same current value"
            );
        }

        assert_eq!(view.retained_stats().full_lower_count, initial_full_lowers);
    }

    #[test]
    fn todomvc_physical_mounts_complete_visual_structure_and_one_inline_editor() {
        let mount_started = Instant::now();
        let example = crate::catalog::Catalog::load()
            .unwrap()
            .open("todo_mvc_physical")
            .unwrap();
        let units = example
            .units
            .into_iter()
            .map(|unit| RuntimeSourceUnit {
                path: unit.path,
                source: unit.source,
            })
            .collect::<Vec<_>>();
        let runtime =
            LiveRuntime::from_project("examples/todo_mvc_physical/RUN.bn", &units).unwrap();
        let mount = runtime.mount();
        let mut model = RuntimeView::mount(runtime, mount).unwrap();
        let mut columns = boon_document::render_scene::ApproximateTextColumnMeasurer;
        let mut view = crate::view::RetainedView::new(
            model.frame(),
            boon_host::Viewport {
                surface: 1,
                width: 510.0,
                height: 540.0,
                scale: 1.0,
            },
            &mut columns,
        )
        .unwrap();
        assert!(
            mount_started.elapsed() < Duration::from_secs(10),
            "physical TodoMVC compile, mount, and retained layout exceeded the switch regression ceiling"
        );
        let text_values = || {
            view.frame()
                .nodes
                .values()
                .filter_map(|node| node.text.as_ref().map(|text| text.text.as_str()))
                .collect::<Vec<_>>()
        };
        let texts = text_values();
        assert!(
            view.scene()
                .text_runs
                .iter()
                .all(|run| run.text.parse::<f64>().is_err()),
            "layout and material scalars must not become visual child text"
        );
        for expected in [
            "todos",
            "Read documentation",
            "Finish TodoMVC renderer",
            "Walk the dog",
            "Buy groceries",
            "3 items left",
            "All",
            "Active",
            "Completed",
            "Double-click to edit a todo",
            "Created by",
            "Martin Kavík",
            "Part of",
            "TodoMVC",
        ] {
            assert!(
                texts.contains(&expected),
                "missing mounted text `{expected}`"
            );
        }
        {
            let uniquely_visible = [
                "todos",
                "Read documentation",
                "Finish TodoMVC renderer",
                "Walk the dog",
                "Buy groceries",
                "3 items left",
                "All",
                "Active",
                "Completed",
                "Double-click to edit a todo",
                "Created by",
                "Martin Kavík",
                "Part of",
                "TodoMVC",
                "Classic",
                "Professional",
                "Glass",
                "Brutalist",
                "Neumorphic",
                "Dark mode",
            ];
            for expected in uniquely_visible {
                let runs = view
                    .scene()
                    .text_runs
                    .iter()
                    .filter(|run| run.text == expected)
                    .collect::<Vec<_>>();
                assert_eq!(
                    runs.len(),
                    1,
                    "`{expected}` must produce exactly one visible text run, got {runs:?}"
                );
                let bounds = runs[0].bounds;
                assert!(
                    bounds.x >= -0.5
                        && bounds.y >= -0.5
                        && bounds.x + bounds.width <= 510.5
                        && bounds.y + bounds.height <= 540.5,
                    "`{expected}` is clipped outside the 510x540 preview: {bounds:?}"
                );
            }

            let run = |text: &str| {
                view.scene()
                    .text_runs
                    .iter()
                    .find(|run| run.text == text)
                    .expect("unique visible text run")
            };
            let todo_titles = [
                run("Read documentation"),
                run("Finish TodoMVC renderer"),
                run("Walk the dog"),
                run("Buy groceries"),
            ];
            for pair in todo_titles.windows(2) {
                assert!(
                    pair[0].bounds.y + pair[0].bounds.height <= pair[1].bounds.y + 1.0,
                    "todo labels overlap: {:?} and {:?}",
                    pair[0],
                    pair[1]
                );
            }
            for pair in [
                [run("3 items left"), run("All")],
                [run("All"), run("Active")],
                [run("Active"), run("Completed")],
            ] {
                assert!(
                    pair[0].bounds.x + pair[0].bounds.width <= pair[1].bounds.x + 1.0,
                    "panel footer labels overlap: {:?} and {:?}",
                    pair[0],
                    pair[1]
                );
            }
            assert!(
                run("Double-click to edit a todo").bounds.y
                    > run("3 items left").bounds.y + run("3 items left").bounds.height,
                "instructions must be below the panel footer"
            );
            assert!(
                run("Classic").bounds.y > run("TodoMVC").bounds.y + run("TodoMVC").bounds.height,
                "theme controls must be below the reference footer"
            );
        }
        assert_eq!(
            view.frame()
                .nodes
                .values()
                .filter(|node| node.kind == DocumentNodeKind::TextInput)
                .count(),
            1,
            "only the new-todo input is visible before editing"
        );
        assert!(
            view.scene()
                .text_runs
                .iter()
                .all(|run| !run.text.contains("Reference[")),
            "checkbox accessibility labels must not render as visual text"
        );
        assert!(
            view.scene().text_runs.iter().all(|run| {
                view.frame()
                    .nodes
                    .get(&run.owner_node)
                    .is_none_or(|node| node.kind != DocumentNodeKind::Checkbox)
            }),
            "checkbox semantics must never become painted label text"
        );
        assert_eq!(
            view.scene()
                .visual_primitives
                .iter()
                .filter(|primitive| primitive.primitive
                    == boon_document::RenderVisualPrimitiveKind::Checkbox)
                .count(),
            4,
            "each todo must produce one checkbox primitive"
        );
        assert_eq!(
            view.scene()
                .visual_primitives
                .iter()
                .filter(|primitive| primitive.primitive
                    == boon_document::RenderVisualPrimitiveKind::CheckboxCheckmark)
                .count(),
            1,
            "the initially completed todo must produce one checkmark"
        );
        let bounded_content = view
            .frame()
            .nodes
            .values()
            .find(|node| {
                node.style.get("width") == Some(&StyleValue::Text("Fill".to_owned()))
                    && node.style.get("min_width") == Some(&StyleValue::Number(230.0))
                    && node.style.get("max_width") == Some(&StyleValue::Number(552.0))
            })
            .expect("bounded TodoMVC content column");
        let bounded_content_rect = view.node_bounds(&bounded_content.id.0).unwrap();
        assert_eq!(bounded_content_rect.x, 16.0);
        assert_eq!(bounded_content_rect.width, 478.0);

        let node_with_text = |text: &str| {
            view.frame()
                .nodes
                .values()
                .find(|node| node.text.as_ref().is_some_and(|value| value.text == text))
                .expect("mounted text node")
        };
        let title = node_with_text("Read documentation");
        let title_label = view
            .frame()
            .nodes
            .get(title.parent.as_ref().unwrap())
            .unwrap();
        let todo_row = view
            .frame()
            .nodes
            .get(title_label.parent.as_ref().unwrap())
            .unwrap();
        assert_eq!(
            todo_row.style.get("height"),
            Some(&StyleValue::Number(50.0))
        );
        assert_eq!(view.node_bounds(&todo_row.id.0).unwrap().height, 50.0);

        let new_input = view
            .frame()
            .nodes
            .values()
            .find(|node| {
                node.kind == DocumentNodeKind::TextInput
                    && node.style.get("placeholder")
                        == Some(&StyleValue::Text("What needs to be done?".to_owned()))
            })
            .expect("new todo input");
        let new_todo_row = view
            .frame()
            .nodes
            .get(new_input.parent.as_ref().unwrap())
            .unwrap();
        let new_todo_row_id = new_todo_row.id.clone();
        assert_eq!(
            new_todo_row.style.get("height"),
            Some(&StyleValue::Number(56.0))
        );
        assert_eq!(view.node_bounds(&new_todo_row.id.0).unwrap().height, 56.0);
        let all_label = node_with_text("All");
        let all_button = view
            .frame()
            .nodes
            .get(all_label.parent.as_ref().unwrap())
            .unwrap();
        assert_eq!(
            all_button.style.get("border_width"),
            Some(&StyleValue::Number(1.0))
        );
        assert!(view.scene().visual_primitives.iter().any(|primitive| {
            primitive.node == all_button.id
                && primitive.primitive == boon_document::RenderVisualPrimitiveKind::Border
        }));

        let author = node_with_text("Martin Kavík");
        let author_line = view
            .frame()
            .nodes
            .get(author.parent.as_ref().unwrap())
            .unwrap();
        let author_parts = author_line
            .children
            .iter()
            .filter_map(|child| view.frame().nodes.get(child)?.text.as_ref())
            .map(|text| text.text.as_str())
            .collect::<Vec<_>>();
        assert_eq!(author_parts, ["Created by", " ", "Martin Kavík"]);

        let links = view
            .frame()
            .nodes
            .values()
            .filter(|node| node.style.get("link") == Some(&StyleValue::Bool(true)))
            .collect::<Vec<_>>();
        assert_eq!(links.len(), 2);
        assert!(links.iter().all(|link| {
            matches!(link.style.get("to"), Some(StyleValue::Text(url)) if url.starts_with("http"))
                && link.style.get("cursor") == Some(&StyleValue::Text("pointer".to_owned()))
        }));
        let link = links[0];
        let link_bounds = view.node_bounds(&link.id.0).unwrap();
        let link_target = HitTarget {
            node: link.id.0.clone(),
            source_path: None,
            source_intent: None,
            row_key: None,
            row_generation: None,
            scroll_root: None,
            center_x: link_bounds.x + link_bounds.width / 2.0,
            center_y: link_bounds.y + link_bounds.height / 2.0,
            bounds_x: link_bounds.x,
            bounds_y: link_bounds.y,
            bounds_width: link_bounds.width,
            bounds_height: link_bounds.height,
            text_column: None,
        };
        for phase in [PointerPhase::Down, PointerPhase::Up] {
            model
                .handle_event(
                    &HostEvent::Pointer(PointerEvent {
                        surface: SurfaceId("preview".to_owned()),
                        x: link_target.center_x,
                        y: link_target.center_y,
                        phase,
                        button: Some(PointerButton::Primary),
                    }),
                    Some(link_target.clone()),
                )
                .unwrap();
        }
        assert!(model.take_external_url().is_some());

        let title_source = view
            .frame()
            .nodes
            .values()
            .flat_map(|node| &node.source_bindings)
            .find(|binding| binding.intent == "double_click")
            .map(|binding| binding.source_path.clone())
            .expect("todo title double-click source");
        let target = view
            .target_for_source(&title_source, Some("Read documentation"))
            .expect("first todo title target");
        for _ in 0..2 {
            for phase in [PointerPhase::Down, PointerPhase::Up] {
                let changed = model
                    .handle_event(
                        &HostEvent::Pointer(PointerEvent {
                            surface: SurfaceId("preview".to_owned()),
                            x: target.center_x,
                            y: target.center_y,
                            phase,
                            button: Some(PointerButton::Primary),
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

        let editing_inputs = view
            .frame()
            .nodes
            .values()
            .filter(|node| node.kind == DocumentNodeKind::TextInput)
            .collect::<Vec<_>>();
        assert_eq!(editing_inputs.len(), 2, "one row editor plus the new input");
        assert_eq!(
            editing_inputs
                .iter()
                .filter(|node| node
                    .text
                    .as_ref()
                    .is_some_and(|text| text.text == "Read documentation"))
                .count(),
            1,
            "the double-clicked title is the only row editor"
        );
        assert_eq!(
            view.frame()
                .nodes
                .values()
                .filter(|node| {
                    node.kind == DocumentNodeKind::Text
                        && node
                            .text
                            .as_ref()
                            .is_some_and(|text| text.text == "Read documentation")
                })
                .count(),
            0,
            "the editor replaces the title instead of rendering beside it"
        );

        let theme_source = view
            .frame()
            .nodes
            .values()
            .flat_map(|node| &node.source_bindings)
            .find(|binding| binding.source_path.ends_with("theme_switcher.neumorphism"))
            .map(|binding| binding.source_path.clone())
            .expect("neumorphism theme source");
        let theme_target = view
            .target_for_source(&theme_source, None)
            .expect("neumorphism theme target");
        for phase in [PointerPhase::Down, PointerPhase::Up] {
            let changed = model
                .handle_event(
                    &HostEvent::Pointer(PointerEvent {
                        surface: SurfaceId("preview".to_owned()),
                        x: theme_target.center_x,
                        y: theme_target.center_y,
                        phase,
                        button: Some(PointerButton::Primary),
                    }),
                    Some(theme_target.clone()),
                )
                .unwrap();
            if changed {
                view.apply_patches(model.take_patches(), &mut columns)
                    .unwrap();
                view.set_interaction_state(model.hovered(), model.focused(), &mut columns)
                    .unwrap();
            }
        }
        let new_todo_row = view.frame().nodes.get(&new_todo_row_id).unwrap();
        assert_eq!(
            new_todo_row.style.get("height"),
            Some(&StyleValue::Number(56.0))
        );
        assert_eq!(view.node_bounds(&new_todo_row.id.0).unwrap().height, 56.0);
        for expected in [
            "Read documentation",
            "Finish TodoMVC renderer",
            "Walk the dog",
            "Buy groceries",
            "All",
            "Active",
            "Completed",
        ] {
            assert_eq!(
                view.scene()
                    .text_runs
                    .iter()
                    .filter(|run| run.text == expected)
                    .count(),
                1,
                "theme updates must not duplicate `{expected}`"
            );
        }
        let author = view
            .frame()
            .nodes
            .values()
            .find(|node| {
                node.text
                    .as_ref()
                    .is_some_and(|text| text.text == "Martin Kavík")
            })
            .unwrap();
        assert_eq!(author.style.get("size"), Some(&StyleValue::Number(11.0)));
        assert!(author.style.contains_key("color"));
        let title_run = view
            .scene()
            .text_runs
            .iter()
            .find(|run| run.text == "todos")
            .expect("theme switch must retain the visible title text run");
        assert!(
            title_run.color[3] > 0,
            "title text must not become transparent"
        );
        assert!(
            title_run.bounds.y < 540.0,
            "title text must remain in the viewport"
        );

        let created = view
            .frame()
            .nodes
            .values()
            .find(|node| {
                node.text
                    .as_ref()
                    .is_some_and(|text| text.text == "Created by")
            })
            .unwrap();
        let created_bounds = view.node_bounds(&created.id.0).unwrap();
        let author_bounds = view.node_bounds(&author.id.0).unwrap();
        let inline_gap = author_bounds.x - (created_bounds.x + created_bounds.width);
        assert!(
            inline_gap <= 12.0,
            "inline paragraph gap is {inline_gap}, created={created_bounds:?}, author={author_bounds:?}"
        );

        let mode_source = view
            .frame()
            .nodes
            .values()
            .flat_map(|node| &node.source_bindings)
            .find(|binding| binding.source_path.ends_with("theme_switcher.mode_toggle"))
            .map(|binding| binding.source_path.clone())
            .expect("theme mode source");
        let mode_target = view
            .target_for_source(&mode_source, None)
            .expect("theme mode target");
        for phase in [PointerPhase::Down, PointerPhase::Up] {
            let changed = model
                .handle_event(
                    &HostEvent::Pointer(PointerEvent {
                        surface: SurfaceId("preview".to_owned()),
                        x: mode_target.center_x,
                        y: mode_target.center_y,
                        phase,
                        button: Some(PointerButton::Primary),
                    }),
                    Some(mode_target.clone()),
                )
                .unwrap();
            if changed {
                view.apply_patches(model.take_patches(), &mut columns)
                    .unwrap();
                view.set_interaction_state(model.hovered(), model.focused(), &mut columns)
                    .unwrap();
            }
        }
        let dark_title_run = view
            .scene()
            .text_runs
            .iter()
            .find(|run| run.text == "todos")
            .expect("dark mode must retain the visible title text run");
        assert!(
            dark_title_run.color[..3]
                .iter()
                .map(|channel| u16::from(*channel))
                .sum::<u16>()
                > 300,
            "dark-mode title color is too dark: {:?}",
            dark_title_run.color
        );
        assert_eq!(
            view.frame()
                .nodes
                .get(&new_todo_row_id)
                .and_then(|node| node.style.get("height")),
            Some(&StyleValue::Number(56.0))
        );
    }
}
