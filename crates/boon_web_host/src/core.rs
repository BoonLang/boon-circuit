use crate::{
    BrowserEventQueue, BrowserFrameScheduler, BrowserFrameSchedulerConfig, BrowserFrameWakeReason,
    BrowserHostEvent, BrowserHostSupport, SemanticProjectionState, SemanticProjectionUpdate,
    WebHostResult,
};
use boon_host::SemanticScene;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BrowserDocumentHostConfig {
    pub event_queue_capacity: usize,
    pub frame_scheduler: BrowserFrameSchedulerConfig,
}

impl Default for BrowserDocumentHostConfig {
    fn default() -> Self {
        Self {
            event_queue_capacity: 1_024,
            frame_scheduler: BrowserFrameSchedulerConfig::default(),
        }
    }
}

/// Deterministic platform-neutral core around the browser adapters. Runtime
/// integration drains these public host events through the same host-event
/// route used by native windows.
pub struct BrowserDocumentHostCore {
    support: BrowserHostSupport,
    events: BrowserEventQueue,
    frames: BrowserFrameScheduler,
    semantics: SemanticProjectionState,
}

impl BrowserDocumentHostCore {
    pub fn new(config: BrowserDocumentHostConfig) -> WebHostResult<Self> {
        Ok(Self {
            support: BrowserHostSupport::foundation(),
            events: BrowserEventQueue::new(config.event_queue_capacity)?,
            frames: BrowserFrameScheduler::new(config.frame_scheduler)?,
            semantics: SemanticProjectionState::default(),
        })
    }

    pub fn support(&self) -> &BrowserHostSupport {
        &self.support
    }

    pub fn frames(&self) -> &BrowserFrameScheduler {
        &self.frames
    }

    pub fn frames_mut(&mut self) -> &mut BrowserFrameScheduler {
        &mut self.frames
    }

    pub fn semantics(&self) -> &SemanticProjectionState {
        &self.semantics
    }

    pub fn accept_event(&mut self, event: BrowserHostEvent, now_ms: u64) -> WebHostResult<bool> {
        let wake_reason = match &event {
            BrowserHostEvent::Input { envelope } => match &envelope.event {
                boon_host::HostEvent::Wheel(_) | boon_host::HostEvent::Pointer(_) => {
                    BrowserFrameWakeReason::ScrollOrGesture
                }
                boon_host::HostEvent::Resize(_) => BrowserFrameWakeReason::SurfaceChanged,
                _ => BrowserFrameWakeReason::VisibleInput,
            },
            BrowserHostEvent::Gesture { .. } => BrowserFrameWakeReason::ScrollOrGesture,
            BrowserHostEvent::Clipboard { .. } | BrowserHostEvent::UrlChanged { .. } => {
                BrowserFrameWakeReason::VisibleInput
            }
            BrowserHostEvent::Lifecycle { .. } => BrowserFrameWakeReason::RuntimePatch,
            BrowserHostEvent::Rejected { .. } => BrowserFrameWakeReason::RuntimePatch,
        };
        self.events.push(event)?;
        Ok(self.frames.wake(wake_reason, now_ms))
    }

    pub fn update_semantics(
        &mut self,
        next: SemanticScene,
        now_ms: u64,
    ) -> (SemanticProjectionUpdate, bool) {
        let update = self.semantics.update(next);
        let schedule = if update.patch.operations.is_empty() {
            false
        } else {
            self.frames
                .wake(BrowserFrameWakeReason::RuntimePatch, now_ms)
        };
        (update, schedule)
    }

    pub fn drain_events(&mut self) -> impl Iterator<Item = BrowserHostEvent> + '_ {
        self.events.drain()
    }

    pub fn queued_event_count(&self) -> usize {
        self.events.len()
    }
}
