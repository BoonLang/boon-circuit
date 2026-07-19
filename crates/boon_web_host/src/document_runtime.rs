use crate::{
    BrowserFrameCompletion, BrowserFrameScheduler, BrowserFrameSchedulerConfig,
    BrowserFrameWakeReason, SemanticProjectionState, SemanticProjectionUpdate, WebHostError,
};
use boon_document::render_scene::RenderTextColumnMeasurer;
use boon_document::{
    DocumentFrame, DocumentNodeId, DocumentState, HitSideTable, LayoutDemand, LayoutFrame,
    PatchApplyError, RenderScene, RetainedDocument, RetainedDocumentStats, RetainedDocumentUpdate,
    SemanticInputEvent, SemanticScene, SemanticWebBridgeSnapshot, SemanticWebInputEvent,
    semantic_scene_from_document_layout,
};
use boon_host::{
    HostEvent, ImeInputKind, LogicalKey, PointerButton, PointerPhase, SemanticId,
    SensitiveInputHandle, Viewport,
};
use boon_runtime::{DocumentPatchStatus, RuntimeTurn, SourcePayload, Value};
use std::error::Error;
use std::fmt::{self, Display, Formatter};

pub const DEFAULT_BROWSER_DOCUMENT_TURN_LIMIT: usize = 256;
pub const DEFAULT_BROWSER_DOCUMENT_PATCH_LIMIT: usize = 16_384;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BrowserDocumentRuntimeConfig {
    pub through_runtime_sequence: u64,
    pub max_turns_per_update: usize,
    pub max_document_patches_per_update: usize,
    pub frame_scheduler: BrowserFrameSchedulerConfig,
}

impl Default for BrowserDocumentRuntimeConfig {
    fn default() -> Self {
        Self {
            through_runtime_sequence: 0,
            max_turns_per_update: DEFAULT_BROWSER_DOCUMENT_TURN_LIMIT,
            max_document_patches_per_update: DEFAULT_BROWSER_DOCUMENT_PATCH_LIMIT,
            frame_scheduler: BrowserFrameSchedulerConfig::default(),
        }
    }
}

#[derive(Debug)]
pub enum BrowserDocumentRuntimeError {
    Host(WebHostError),
    Document(PatchApplyError),
    InvalidViewport,
    TurnLimitExceeded {
        limit: usize,
        actual: usize,
    },
    PatchLimitExceeded {
        limit: usize,
        actual: usize,
    },
    TurnSequence {
        expected: u64,
        actual: u64,
    },
    RuntimeSequenceExhausted,
    IncompleteDocumentNotification {
        sequence: u64,
    },
    AuthoritativeFrameMismatch {
        through_sequence: u64,
        retained_node_count: usize,
        authoritative_node_count: usize,
    },
}

impl Display for BrowserDocumentRuntimeError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Host(error) => Display::fmt(error, formatter),
            Self::Document(error) => Display::fmt(error, formatter),
            Self::InvalidViewport => formatter.write_str(
                "browser document viewport dimensions and scale must be finite and positive",
            ),
            Self::TurnLimitExceeded { limit, actual } => write!(
                formatter,
                "browser document update contains {actual} turns; limit is {limit}"
            ),
            Self::PatchLimitExceeded { limit, actual } => write!(
                formatter,
                "browser document update contains {actual} patches; limit is {limit}"
            ),
            Self::TurnSequence { expected, actual } => write!(
                formatter,
                "browser document RuntimeTurn sequence mismatch: expected {expected}, got {actual}"
            ),
            Self::RuntimeSequenceExhausted => {
                formatter.write_str("browser document RuntimeTurn sequence is exhausted")
            }
            Self::IncompleteDocumentNotification { sequence } => write!(
                formatter,
                "RuntimeTurn {sequence} did not contain a complete document notification"
            ),
            Self::AuthoritativeFrameMismatch {
                through_sequence,
                retained_node_count,
                authoritative_node_count,
            } => write!(
                formatter,
                "retained document diverged from the authoritative runtime frame through turn {through_sequence} (retained nodes {retained_node_count}, authoritative nodes {authoritative_node_count})"
            ),
        }
    }
}

impl Error for BrowserDocumentRuntimeError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Host(error) => Some(error),
            Self::Document(error) => Some(error),
            _ => None,
        }
    }
}

impl From<WebHostError> for BrowserDocumentRuntimeError {
    fn from(error: WebHostError) -> Self {
        Self::Host(error)
    }
}

impl From<PatchApplyError> for BrowserDocumentRuntimeError {
    fn from(error: PatchApplyError) -> Self {
        Self::Document(error)
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct BrowserDocumentDirtyState {
    pub content: bool,
    pub layout: bool,
    pub render: bool,
    pub semantics: bool,
}

impl BrowserDocumentDirtyState {
    pub fn is_empty(self) -> bool {
        !self.content && !self.layout && !self.render && !self.semantics
    }

    fn merge(&mut self, other: Self) {
        self.content |= other.content;
        self.layout |= other.layout;
        self.render |= other.render;
        self.semantics |= other.semantics;
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct BrowserDocumentSchedulingOutput {
    pub dirty: BrowserDocumentDirtyState,
    pub request_animation_frame: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct BrowserDocumentRenderStart {
    pub render: bool,
    pub proof_sample: bool,
    pub dirty: BrowserDocumentDirtyState,
    pub render_revision: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BrowserDocumentSourceDispatch {
    pub node: DocumentNodeId,
    pub source_path: String,
    pub source_intent: Option<String>,
    pub payload: SourcePayload,
    pub sensitive_input: Option<SensitiveInputHandle>,
}

#[derive(Debug, Default)]
pub struct BrowserDocumentRuntimeUpdate {
    pub through_runtime_sequence: u64,
    pub turn_count: usize,
    pub patch_count: usize,
    pub retained: RetainedDocumentUpdate,
    pub semantic: Option<SemanticProjectionUpdate>,
    pub scheduling: BrowserDocumentSchedulingOutput,
}

#[derive(Debug, Default)]
pub struct BrowserDocumentHostEventOutput {
    pub dispatch: Option<BrowserDocumentSourceDispatch>,
    pub retained: RetainedDocumentUpdate,
    pub semantic: Option<SemanticProjectionUpdate>,
    pub scheduling: BrowserDocumentSchedulingOutput,
}

/// Platform-neutral retained presentation state for the browser Client runtime.
///
/// `RetainedDocument` is the sole owner of the host copy of `DocumentState`, its
/// derived indexes, layout, hit table, and render scene. Runtime turns are only
/// notifications: this type never owns or invokes a local runtime Session.
pub struct BrowserDocumentRuntime {
    retained: RetainedDocument,
    semantics: SemanticProjectionState,
    frames: BrowserFrameScheduler,
    pending_dirty: BrowserDocumentDirtyState,
    viewport: Viewport,
    through_runtime_sequence: u64,
    max_turns_per_update: usize,
    max_document_patches_per_update: usize,
    hovered: Option<DocumentNodeId>,
    pressed: Option<DocumentNodeId>,
    focused: Option<DocumentNodeId>,
}

impl BrowserDocumentRuntime {
    pub fn new(
        authoritative_frame: DocumentFrame,
        viewport: Viewport,
        columns: &mut impl RenderTextColumnMeasurer,
    ) -> Result<Self, BrowserDocumentRuntimeError> {
        Self::with_config(
            authoritative_frame,
            viewport,
            columns,
            BrowserDocumentRuntimeConfig::default(),
        )
    }

    pub fn with_config(
        authoritative_frame: DocumentFrame,
        viewport: Viewport,
        columns: &mut impl RenderTextColumnMeasurer,
        config: BrowserDocumentRuntimeConfig,
    ) -> Result<Self, BrowserDocumentRuntimeError> {
        validate_viewport(viewport)?;
        if config.max_turns_per_update == 0 || config.max_document_patches_per_update == 0 {
            return Err(WebHostError::InvalidInput {
                field: "browser document runtime limits".to_owned(),
                reason: "turn and document patch limits must be non-zero".to_owned(),
            }
            .into());
        }

        let focused = authoritative_frame.focus.clone();
        let mut retained = RetainedDocument::new(authoritative_frame, viewport, columns)?;
        if focused.is_some() {
            retained.set_interaction_state(None, focused.clone(), columns)?;
        }
        let semantic_scene =
            semantic_scene_from_document_layout(retained.frame(), retained.layout());
        let semantics = SemanticProjectionState::new(semantic_scene);
        let mut frames = BrowserFrameScheduler::new(config.frame_scheduler)?;
        frames.wake(BrowserFrameWakeReason::RuntimePatch, 0);
        Ok(Self {
            retained,
            semantics,
            frames,
            pending_dirty: BrowserDocumentDirtyState {
                content: true,
                layout: true,
                render: true,
                semantics: true,
            },
            viewport,
            through_runtime_sequence: config.through_runtime_sequence,
            max_turns_per_update: config.max_turns_per_update,
            max_document_patches_per_update: config.max_document_patches_per_update,
            hovered: None,
            pressed: None,
            focused,
        })
    }

    pub fn frame(&self) -> &DocumentFrame {
        self.retained.frame()
    }

    pub fn layout(&self) -> &LayoutFrame {
        self.retained.layout()
    }

    pub fn hits(&self) -> &HitSideTable {
        self.retained.hits()
    }

    pub fn render_scene(&self) -> &RenderScene {
        self.retained.scene()
    }

    pub fn semantic_scene(&self) -> &SemanticScene {
        self.semantics.scene()
    }

    pub fn semantic_bridge(&self) -> &SemanticWebBridgeSnapshot {
        self.semantics.bridge()
    }

    pub fn demands(&self) -> &[LayoutDemand] {
        self.retained.demands()
    }

    pub fn stats(&self) -> RetainedDocumentStats {
        self.retained.stats()
    }

    pub fn viewport(&self) -> Viewport {
        self.viewport
    }

    pub fn through_runtime_sequence(&self) -> u64 {
        self.through_runtime_sequence
    }

    pub fn hovered(&self) -> Option<&DocumentNodeId> {
        self.hovered.as_ref()
    }

    pub fn focused(&self) -> Option<&DocumentNodeId> {
        self.focused.as_ref()
    }

    pub fn animation_frame_pending(&self) -> bool {
        self.frames.animation_frame_pending()
    }

    pub fn consume_turn(
        &mut self,
        turn: &RuntimeTurn,
        now_ms: u64,
        columns: &mut impl RenderTextColumnMeasurer,
    ) -> Result<BrowserDocumentRuntimeUpdate, BrowserDocumentRuntimeError> {
        self.consume_turns_inner(std::slice::from_ref(turn), None, now_ms, columns)
    }

    pub fn consume_turn_and_verify(
        &mut self,
        turn: &RuntimeTurn,
        authoritative_frame: &DocumentFrame,
        now_ms: u64,
        columns: &mut impl RenderTextColumnMeasurer,
    ) -> Result<BrowserDocumentRuntimeUpdate, BrowserDocumentRuntimeError> {
        self.consume_turns_inner(
            std::slice::from_ref(turn),
            Some(authoritative_frame),
            now_ms,
            columns,
        )
    }

    pub fn consume_turns_and_verify(
        &mut self,
        turns: &[RuntimeTurn],
        authoritative_frame: &DocumentFrame,
        now_ms: u64,
        columns: &mut impl RenderTextColumnMeasurer,
    ) -> Result<BrowserDocumentRuntimeUpdate, BrowserDocumentRuntimeError> {
        self.consume_turns_inner(turns, Some(authoritative_frame), now_ms, columns)
    }

    fn consume_turns_inner(
        &mut self,
        turns: &[RuntimeTurn],
        authoritative_frame: Option<&DocumentFrame>,
        now_ms: u64,
        columns: &mut impl RenderTextColumnMeasurer,
    ) -> Result<BrowserDocumentRuntimeUpdate, BrowserDocumentRuntimeError> {
        if turns.len() > self.max_turns_per_update {
            return Err(BrowserDocumentRuntimeError::TurnLimitExceeded {
                limit: self.max_turns_per_update,
                actual: turns.len(),
            });
        }

        let mut previous_sequence = self.through_runtime_sequence;
        let mut patch_count = 0usize;
        for turn in turns {
            let expected = previous_sequence
                .checked_add(1)
                .ok_or(BrowserDocumentRuntimeError::RuntimeSequenceExhausted)?;
            if turn.sequence != expected {
                return Err(BrowserDocumentRuntimeError::TurnSequence {
                    expected,
                    actual: turn.sequence,
                });
            }
            if !matches!(turn.document_patch_status, DocumentPatchStatus::Complete) {
                return Err(
                    BrowserDocumentRuntimeError::IncompleteDocumentNotification {
                        sequence: turn.sequence,
                    },
                );
            }
            patch_count = patch_count.checked_add(turn.document_patches.len()).ok_or(
                BrowserDocumentRuntimeError::PatchLimitExceeded {
                    limit: self.max_document_patches_per_update,
                    actual: usize::MAX,
                },
            )?;
            if patch_count > self.max_document_patches_per_update {
                return Err(BrowserDocumentRuntimeError::PatchLimitExceeded {
                    limit: self.max_document_patches_per_update,
                    actual: patch_count,
                });
            }
            previous_sequence = turn.sequence;
        }

        if let Some(authoritative_frame) = authoritative_frame {
            let mut candidate = DocumentState::from_frame(self.retained.frame().clone())?;
            for turn in turns {
                candidate.apply_batch(boon_document::DocumentChangeBatch {
                    patches: turn.document_patches.clone(),
                })?;
            }
            if candidate.frame() != authoritative_frame {
                return Err(BrowserDocumentRuntimeError::AuthoritativeFrameMismatch {
                    through_sequence: turns
                        .last()
                        .map_or(self.through_runtime_sequence, |turn| turn.sequence),
                    retained_node_count: candidate.frame().nodes.len(),
                    authoritative_node_count: authoritative_frame.nodes.len(),
                });
            }
        }

        let patches = turns
            .iter()
            .flat_map(|turn| turn.document_patches.iter().cloned())
            .collect();
        let mut retained = self.retained.apply_patches(patches, columns)?;
        if let Some(last) = turns.last() {
            self.through_runtime_sequence = last.sequence;
        }

        let interaction_removed = self.clear_stale_interaction_nodes();
        if interaction_removed {
            merge_retained_update(
                &mut retained,
                self.retained.set_interaction_state(
                    self.hovered.clone(),
                    self.focused.clone(),
                    columns,
                )?,
            );
        }
        let (semantic, scheduling) =
            self.finish_retained_update(retained, BrowserFrameWakeReason::RuntimePatch, now_ms);
        Ok(BrowserDocumentRuntimeUpdate {
            through_runtime_sequence: self.through_runtime_sequence,
            turn_count: turns.len(),
            patch_count,
            retained,
            semantic,
            scheduling,
        })
    }

    pub fn resize(
        &mut self,
        viewport: Viewport,
        now_ms: u64,
        columns: &mut impl RenderTextColumnMeasurer,
    ) -> Result<BrowserDocumentRuntimeUpdate, BrowserDocumentRuntimeError> {
        validate_viewport(viewport)?;
        self.viewport = viewport;
        let retained = self.retained.resize(viewport, columns)?;
        let (semantic, scheduling) =
            self.finish_retained_update(retained, BrowserFrameWakeReason::SurfaceChanged, now_ms);
        Ok(BrowserDocumentRuntimeUpdate {
            through_runtime_sequence: self.through_runtime_sequence,
            retained,
            semantic,
            scheduling,
            ..BrowserDocumentRuntimeUpdate::default()
        })
    }

    pub fn set_interaction_state(
        &mut self,
        hovered: Option<DocumentNodeId>,
        focused: Option<DocumentNodeId>,
        now_ms: u64,
        columns: &mut impl RenderTextColumnMeasurer,
    ) -> Result<BrowserDocumentRuntimeUpdate, BrowserDocumentRuntimeError> {
        self.hovered = hovered.filter(|id| self.retained.frame().nodes.contains_key(id));
        self.focused = focused.filter(|id| self.retained.frame().nodes.contains_key(id));
        let retained = self.retained.set_interaction_state(
            self.hovered.clone(),
            self.focused.clone(),
            columns,
        )?;
        let (semantic, scheduling) =
            self.finish_retained_update(retained, BrowserFrameWakeReason::VisibleInput, now_ms);
        Ok(BrowserDocumentRuntimeUpdate {
            through_runtime_sequence: self.through_runtime_sequence,
            retained,
            semantic,
            scheduling,
            ..BrowserDocumentRuntimeUpdate::default()
        })
    }

    pub fn source_dispatch_for_host_event(
        &self,
        event: &HostEvent,
    ) -> Option<BrowserDocumentSourceDispatch> {
        match event {
            HostEvent::Pointer(pointer) => {
                let entry = self.retained.hits().hit_test(pointer.x, pointer.y)?;
                let intents: &[&str] = match (pointer.phase, pointer.button) {
                    (PointerPhase::Move, _) => &["pointer_move", "move"],
                    (PointerPhase::Down, _) => &["pointer_down", "down", "focus"],
                    (PointerPhase::Up, Some(PointerButton::Primary)) => &[
                        "press", "click", "source", "activate", "toggle", "submit", "open",
                        "select",
                    ],
                    (PointerPhase::Up, _) => &["pointer_up", "up"],
                    (PointerPhase::Leave, _) => return None,
                };
                let route = intents.iter().find_map(|intent| {
                    entry
                        .source_routes
                        .iter()
                        .find(|route| route.intent == *intent)
                })?;
                Some(BrowserDocumentSourceDispatch {
                    node: entry.node.clone(),
                    source_path: route.source_path.clone(),
                    source_intent: Some(route.intent.clone()),
                    payload: pointer_payload(pointer.x, pointer.y, entry.bounds),
                    sensitive_input: None,
                })
            }
            HostEvent::Wheel(wheel) => {
                let entry = self.retained.hits().hit_test(wheel.x, wheel.y)?;
                let route = ["wheel", "scroll"].iter().find_map(|intent| {
                    entry
                        .source_routes
                        .iter()
                        .find(|route| route.intent == *intent)
                })?;
                let mut payload = SourcePayload::default();
                insert_rounded_number(&mut payload, "delta_x", wheel.delta_x);
                insert_rounded_number(&mut payload, "delta_y", wheel.delta_y);
                Some(BrowserDocumentSourceDispatch {
                    node: entry.node.clone(),
                    source_path: route.source_path.clone(),
                    source_intent: Some(route.intent.clone()),
                    payload,
                    sensitive_input: None,
                })
            }
            HostEvent::Keyboard(key) if key.pressed => {
                let logical = logical_key_text(&key.logical_key)?;
                let intents: &[&str] = match logical.as_str() {
                    "Enter" => &["commit", "submit", "key_down", "source"],
                    "Escape" => &["cancel", "escape", "key_down", "source"],
                    _ => &["key_down", "source"],
                };
                self.dispatch_for_focused_node(
                    intents,
                    SourcePayload {
                        key: Some(logical),
                        ..SourcePayload::default()
                    },
                )
            }
            HostEvent::TextInput(text) => self.dispatch_for_focused_node(
                &["change", "text", "input", "source"],
                SourcePayload {
                    text: Some(text.text.clone()),
                    ..SourcePayload::default()
                },
            ),
            HostEvent::Ime(ime) => match &ime.kind {
                ImeInputKind::Commit { text } => self.dispatch_for_focused_node(
                    &["change", "text", "input", "source"],
                    SourcePayload {
                        text: Some(text.clone()),
                        ..SourcePayload::default()
                    },
                ),
                _ => None,
            },
            HostEvent::Focus { focused: false, .. } => {
                self.dispatch_for_focused_node(&["blur", "source"], SourcePayload::default())
            }
            HostEvent::Focus { focused: true, .. } => {
                self.dispatch_for_focused_node(&["focus"], SourcePayload::default())
            }
            HostEvent::SensitiveInput(input) => {
                self.dispatch_for_focused_sensitive_node(input.handle)
            }
            HostEvent::Accessibility(_)
            | HostEvent::CloseRequested { .. }
            | HostEvent::Resize(_)
            | HostEvent::Keyboard(_) => None,
        }
    }

    pub fn source_dispatch_for_semantic_web_event(
        &self,
        event: SemanticWebInputEvent,
    ) -> Option<BrowserDocumentSourceDispatch> {
        let sensitive_target = match &event {
            SemanticWebInputEvent::SetText { semantic_id, .. }
            | SemanticWebInputEvent::ReplaceSelectedText { semantic_id, .. } => Some(semantic_id),
            _ => None,
        };
        if sensitive_target.is_some_and(|semantic_id| {
            self.semantics
                .scene()
                .nodes
                .get(semantic_id)
                .is_some_and(|node| node.state.sensitive)
        }) {
            return None;
        }
        semantic_dispatch(self.semantics.source_dispatch_for_web_event(event))
    }

    pub fn source_dispatch_for_semantic_event(
        &self,
        event: SemanticInputEvent,
    ) -> Option<BrowserDocumentSourceDispatch> {
        let sensitive_target = match &event {
            SemanticInputEvent::SetText { semantic_id, .. }
            | SemanticInputEvent::ReplaceSelectedText { semantic_id, .. } => Some(semantic_id),
            _ => None,
        };
        if sensitive_target.is_some_and(|semantic_id| {
            self.semantics
                .scene()
                .nodes
                .get(semantic_id)
                .is_some_and(|node| node.state.sensitive)
        }) {
            return None;
        }
        semantic_dispatch(self.semantics.source_dispatch_for_semantic_event(event))
    }

    pub fn source_dispatch_for_sensitive_semantic_input(
        &self,
        semantic_id: &SemanticId,
        handle: SensitiveInputHandle,
    ) -> Option<BrowserDocumentSourceDispatch> {
        let node = self.semantics.scene().nodes.get(semantic_id)?;
        if !node.state.sensitive || !node.actions.sensitive_input {
            return None;
        }
        Some(BrowserDocumentSourceDispatch {
            node: node.node.clone(),
            source_path: node.source_path.clone()?,
            source_intent: node.source_intent.clone(),
            payload: SourcePayload::default(),
            sensitive_input: Some(handle),
        })
    }

    pub fn handle_host_event(
        &mut self,
        event: &HostEvent,
        now_ms: u64,
        columns: &mut impl RenderTextColumnMeasurer,
    ) -> Result<BrowserDocumentHostEventOutput, BrowserDocumentRuntimeError> {
        if let HostEvent::Resize(resize) = event {
            let update = self.resize(
                Viewport {
                    surface: self.viewport.surface,
                    width: resize.logical_size.width,
                    height: resize.logical_size.height,
                    scale: resize.scale,
                },
                now_ms,
                columns,
            )?;
            return Ok(BrowserDocumentHostEventOutput {
                retained: update.retained,
                semantic: update.semantic,
                scheduling: update.scheduling,
                ..BrowserDocumentHostEventOutput::default()
            });
        }

        let mut dispatch = self.source_dispatch_for_host_event(event);
        let previous_hovered = self.hovered.clone();
        let previous_focused = self.focused.clone();
        match event {
            HostEvent::Pointer(pointer) => {
                let target = self
                    .retained
                    .hits()
                    .hit_test(pointer.x, pointer.y)
                    .map(|entry| entry.node.clone());
                match pointer.phase {
                    PointerPhase::Move => self.hovered = target,
                    PointerPhase::Leave => self.hovered = None,
                    PointerPhase::Down if pointer.button == Some(PointerButton::Primary) => {
                        self.pressed = target.clone();
                        self.focused = target;
                    }
                    PointerPhase::Up if pointer.button == Some(PointerButton::Primary) => {
                        if self.pressed.take() != target {
                            dispatch = None;
                        }
                    }
                    _ => {}
                }
            }
            HostEvent::Focus { focused: false, .. } => {
                self.pressed = None;
                self.focused = None;
            }
            _ => {}
        }

        let interaction_changed =
            previous_hovered != self.hovered || previous_focused != self.focused;
        let (retained, semantic, mut scheduling) = if interaction_changed {
            let retained = self.retained.set_interaction_state(
                self.hovered.clone(),
                self.focused.clone(),
                columns,
            )?;
            let (semantic, scheduling) =
                self.finish_retained_update(retained, event_wake_reason(event), now_ms);
            (retained, semantic, scheduling)
        } else {
            (
                RetainedDocumentUpdate::default(),
                None,
                BrowserDocumentSchedulingOutput::default(),
            )
        };
        if scheduling.dirty.is_empty() && dispatch.is_some() {
            scheduling.request_animation_frame = self.frames.wake(event_wake_reason(event), now_ms);
        }
        Ok(BrowserDocumentHostEventOutput {
            dispatch,
            retained,
            semantic,
            scheduling,
        })
    }

    pub fn begin_animation_frame(&mut self) -> BrowserDocumentRenderStart {
        let start = self.frames.begin_animation_frame();
        let dirty = if start.render {
            std::mem::take(&mut self.pending_dirty)
        } else {
            BrowserDocumentDirtyState::default()
        };
        BrowserDocumentRenderStart {
            render: start.render,
            proof_sample: start.proof_sample,
            dirty,
            render_revision: self.retained.stats().render_revision,
        }
    }

    pub fn complete_animation_frame(
        &mut self,
        now_ms: u64,
        visible_changed: bool,
        wants_animation: bool,
    ) -> BrowserFrameCompletion {
        self.frames
            .complete_animation_frame(now_ms, visible_changed, wants_animation)
    }

    pub fn set_visible(&mut self, visible: bool) -> bool {
        self.frames.set_visible(visible)
    }

    fn dispatch_for_focused_node(
        &self,
        intents: &[&str],
        payload: SourcePayload,
    ) -> Option<BrowserDocumentSourceDispatch> {
        let node_id = self.focused.as_ref()?;
        let node = self.retained.frame().nodes.get(node_id)?;
        if node.is_sensitive_text_input() && payload.text.is_some() {
            return None;
        }
        let binding = intents.iter().find_map(|intent| {
            node.source_bindings
                .iter()
                .find(|binding| binding.intent == *intent)
        })?;
        Some(BrowserDocumentSourceDispatch {
            node: node_id.clone(),
            source_path: binding.source_path.clone(),
            source_intent: Some(binding.intent.clone()),
            payload,
            sensitive_input: None,
        })
    }

    fn dispatch_for_focused_sensitive_node(
        &self,
        handle: SensitiveInputHandle,
    ) -> Option<BrowserDocumentSourceDispatch> {
        let node_id = self.focused.as_ref()?;
        let node = self.retained.frame().nodes.get(node_id)?;
        if !node.is_sensitive_text_input() {
            return None;
        }
        let binding = ["change", "text", "input", "source"]
            .iter()
            .find_map(|intent| {
                node.source_bindings
                    .iter()
                    .find(|binding| binding.intent == *intent)
            })?;
        Some(BrowserDocumentSourceDispatch {
            node: node_id.clone(),
            source_path: binding.source_path.clone(),
            source_intent: Some(binding.intent.clone()),
            payload: SourcePayload::default(),
            sensitive_input: Some(handle),
        })
    }

    fn clear_stale_interaction_nodes(&mut self) -> bool {
        let frame = self.retained.frame();
        let hovered_removed = self
            .hovered
            .as_ref()
            .is_some_and(|id| !frame.nodes.contains_key(id));
        let pressed_removed = self
            .pressed
            .as_ref()
            .is_some_and(|id| !frame.nodes.contains_key(id));
        let focused_removed = self
            .focused
            .as_ref()
            .is_some_and(|id| !frame.nodes.contains_key(id));
        if hovered_removed {
            self.hovered = None;
        }
        if pressed_removed {
            self.pressed = None;
        }
        if focused_removed {
            self.focused = None;
        }
        hovered_removed || focused_removed
    }

    fn finish_retained_update(
        &mut self,
        retained: RetainedDocumentUpdate,
        reason: BrowserFrameWakeReason,
        now_ms: u64,
    ) -> (
        Option<SemanticProjectionUpdate>,
        BrowserDocumentSchedulingOutput,
    ) {
        let semantic = if retained.content_changed || retained.layout_changed {
            let next =
                semantic_scene_from_document_layout(self.retained.frame(), self.retained.layout());
            let update = self.semantics.update(next);
            (!update.patch.operations.is_empty()).then_some(update)
        } else {
            None
        };
        let dirty = BrowserDocumentDirtyState {
            content: retained.content_changed,
            layout: retained.layout_changed,
            render: retained.render_changed,
            semantics: semantic.is_some(),
        };
        let request_animation_frame = if dirty.is_empty() {
            false
        } else {
            self.pending_dirty.merge(dirty);
            self.frames.wake(reason, now_ms)
        };
        (
            semantic,
            BrowserDocumentSchedulingOutput {
                dirty,
                request_animation_frame,
            },
        )
    }
}

fn validate_viewport(viewport: Viewport) -> Result<(), BrowserDocumentRuntimeError> {
    if viewport.width.is_finite()
        && viewport.width > 0.0
        && viewport.height.is_finite()
        && viewport.height > 0.0
        && viewport.scale.is_finite()
        && viewport.scale > 0.0
    {
        Ok(())
    } else {
        Err(BrowserDocumentRuntimeError::InvalidViewport)
    }
}

fn merge_retained_update(target: &mut RetainedDocumentUpdate, update: RetainedDocumentUpdate) {
    target.content_changed |= update.content_changed;
    target.layout_changed |= update.layout_changed;
    target.render_changed |= update.render_changed;
    target.full_lowered |= update.full_lowered;
    target.patched_node_count = target
        .patched_node_count
        .saturating_add(update.patched_node_count);
}

fn pointer_payload(x: f32, y: f32, bounds: boon_document::Rect) -> SourcePayload {
    let mut payload = SourcePayload::default();
    if bounds.width.is_finite()
        && bounds.height.is_finite()
        && bounds.width > 0.0
        && bounds.height > 0.0
    {
        insert_rounded_number(
            &mut payload,
            "pointer_x",
            (x - bounds.x).clamp(0.0, bounds.width),
        );
        insert_rounded_number(
            &mut payload,
            "pointer_y",
            (y - bounds.y).clamp(0.0, bounds.height),
        );
        insert_rounded_number(&mut payload, "pointer_width", bounds.width);
        insert_rounded_number(&mut payload, "pointer_height", bounds.height);
    }
    payload
}

fn insert_rounded_number(payload: &mut SourcePayload, name: &str, value: f32) {
    if value.is_finite()
        && let Ok(value) = Value::integer(value.round() as i64)
    {
        payload.fields.insert(name.to_owned(), value);
    }
}

fn logical_key_text(key: &LogicalKey) -> Option<String> {
    match key {
        LogicalKey::Character(value) | LogicalKey::Named(value) => Some(value.clone()),
        LogicalKey::Dead(Some(value)) => Some(value.to_string()),
        LogicalKey::Dead(None) | LogicalKey::Unidentified => None,
    }
}

fn semantic_dispatch(
    dispatch: Option<boon_host::SemanticSourceDispatch>,
) -> Option<BrowserDocumentSourceDispatch> {
    let dispatch = dispatch?;
    Some(BrowserDocumentSourceDispatch {
        node: dispatch.node,
        source_path: dispatch.source_path,
        source_intent: dispatch.source_intent,
        payload: SourcePayload {
            text: dispatch.text,
            ..SourcePayload::default()
        },
        sensitive_input: None,
    })
}

fn event_wake_reason(event: &HostEvent) -> BrowserFrameWakeReason {
    match event {
        HostEvent::Pointer(_) | HostEvent::Wheel(_) => BrowserFrameWakeReason::ScrollOrGesture,
        HostEvent::Keyboard(_) | HostEvent::TextInput(_) | HostEvent::Ime(_) => {
            BrowserFrameWakeReason::TextCaret
        }
        HostEvent::Resize(_) => BrowserFrameWakeReason::SurfaceChanged,
        HostEvent::SensitiveInput(_)
        | HostEvent::Accessibility(_)
        | HostEvent::Focus { .. }
        | HostEvent::CloseRequested { .. } => BrowserFrameWakeReason::VisibleInput,
    }
}
