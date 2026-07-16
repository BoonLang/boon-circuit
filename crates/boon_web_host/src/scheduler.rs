use crate::{WebHostError, WebHostResult};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct BrowserFrameSchedulerConfig {
    pub burst_min_frames: u32,
    pub burst_quiet_ms: u64,
    pub burst_hard_cap_ms: u64,
}

impl Default for BrowserFrameSchedulerConfig {
    fn default() -> Self {
        Self {
            burst_min_frames: 2,
            burst_quiet_ms: 100,
            burst_hard_cap_ms: 1_000,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrowserFrameWakeReason {
    VisibleInput,
    ScrollOrGesture,
    TextCaret,
    RuntimePatch,
    LayoutPatch,
    AssetReady,
    SurfaceChanged,
    AnimationRequested,
    ProofSample,
}

impl BrowserFrameWakeReason {
    fn starts_burst(self) -> bool {
        matches!(
            self,
            Self::VisibleInput | Self::ScrollOrGesture | Self::TextCaret | Self::AnimationRequested
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BrowserFramePacing {
    Idle,
    RequestedAnimationBurst {
        started_at_ms: u64,
        quiet_after_ms: u64,
        hard_stop_after_ms: u64,
        rendered_frames: u32,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BrowserFrameStart {
    pub render: bool,
    pub proof_sample: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BrowserFrameCompletion {
    pub schedule_next_animation_frame: bool,
    pub pacing: BrowserFramePacing,
}

#[derive(Clone, Debug)]
pub struct BrowserFrameScheduler {
    config: BrowserFrameSchedulerConfig,
    pacing: BrowserFramePacing,
    visible: bool,
    dirty: bool,
    proof_sample: bool,
    animation_frame_pending: bool,
}

impl BrowserFrameScheduler {
    pub fn new(config: BrowserFrameSchedulerConfig) -> WebHostResult<Self> {
        if config.burst_min_frames == 0
            || config.burst_quiet_ms == 0
            || config.burst_hard_cap_ms < config.burst_quiet_ms
        {
            return Err(WebHostError::InvalidInput {
                field: "browser frame scheduler configuration".to_owned(),
                reason: "requires non-zero minimum/quiet values and hard cap >= quiet interval"
                    .to_owned(),
            });
        }
        Ok(Self {
            config,
            pacing: BrowserFramePacing::Idle,
            visible: true,
            dirty: false,
            proof_sample: false,
            animation_frame_pending: false,
        })
    }

    pub fn pacing(&self) -> BrowserFramePacing {
        self.pacing
    }

    pub fn animation_frame_pending(&self) -> bool {
        self.animation_frame_pending
    }

    /// Marks retained state dirty. The return value tells the browser adapter
    /// whether it must issue a new `requestAnimationFrame` call.
    pub fn wake(&mut self, reason: BrowserFrameWakeReason, now_ms: u64) -> bool {
        self.dirty = true;
        self.proof_sample |= reason == BrowserFrameWakeReason::ProofSample;
        if reason.starts_burst() {
            self.extend_or_start_burst(now_ms);
        }
        self.schedule_if_needed()
    }

    pub fn set_visible(&mut self, visible: bool) -> bool {
        self.visible = visible;
        if !visible {
            self.animation_frame_pending = false;
            return false;
        }
        self.schedule_if_needed()
    }

    pub fn begin_animation_frame(&mut self) -> BrowserFrameStart {
        self.animation_frame_pending = false;
        if !self.visible {
            return BrowserFrameStart {
                render: false,
                proof_sample: false,
            };
        }
        let render = self.dirty
            || matches!(
                self.pacing,
                BrowserFramePacing::RequestedAnimationBurst { .. }
            );
        let proof_sample = render && self.proof_sample;
        self.dirty = false;
        self.proof_sample = false;
        BrowserFrameStart {
            render,
            proof_sample,
        }
    }

    pub fn complete_animation_frame(
        &mut self,
        now_ms: u64,
        visible_changed: bool,
        wants_animation: bool,
    ) -> BrowserFrameCompletion {
        let hard_cap_reached = matches!(
            self.pacing,
            BrowserFramePacing::RequestedAnimationBurst {
                hard_stop_after_ms,
                ..
            } if now_ms >= hard_stop_after_ms
        );
        if (visible_changed || wants_animation) && !hard_cap_reached {
            self.dirty |= wants_animation;
            self.extend_or_start_burst(now_ms);
        } else if hard_cap_reached {
            self.dirty = false;
        }

        if let BrowserFramePacing::RequestedAnimationBurst {
            started_at_ms,
            quiet_after_ms,
            hard_stop_after_ms,
            rendered_frames,
        } = self.pacing
        {
            let rendered_frames = rendered_frames.saturating_add(1);
            let minimum_met = rendered_frames >= self.config.burst_min_frames;
            let quiet = now_ms >= quiet_after_ms && !self.dirty && !wants_animation;
            let hard_stopped = now_ms >= hard_stop_after_ms;
            self.pacing = if hard_stopped || (minimum_met && quiet) {
                BrowserFramePacing::Idle
            } else {
                BrowserFramePacing::RequestedAnimationBurst {
                    started_at_ms,
                    quiet_after_ms,
                    hard_stop_after_ms,
                    rendered_frames,
                }
            };
        }

        let schedule_next_animation_frame = self.schedule_if_needed();
        BrowserFrameCompletion {
            schedule_next_animation_frame,
            pacing: self.pacing,
        }
    }

    fn extend_or_start_burst(&mut self, now_ms: u64) {
        self.pacing = match self.pacing {
            BrowserFramePacing::Idle => BrowserFramePacing::RequestedAnimationBurst {
                started_at_ms: now_ms,
                quiet_after_ms: now_ms.saturating_add(self.config.burst_quiet_ms),
                hard_stop_after_ms: now_ms.saturating_add(self.config.burst_hard_cap_ms),
                rendered_frames: 0,
            },
            BrowserFramePacing::RequestedAnimationBurst {
                started_at_ms,
                hard_stop_after_ms,
                rendered_frames,
                ..
            } => BrowserFramePacing::RequestedAnimationBurst {
                started_at_ms,
                quiet_after_ms: now_ms
                    .saturating_add(self.config.burst_quiet_ms)
                    .min(hard_stop_after_ms),
                hard_stop_after_ms,
                rendered_frames,
            },
        };
    }

    fn schedule_if_needed(&mut self) -> bool {
        let wants_frame = self.visible
            && (self.dirty
                || matches!(
                    self.pacing,
                    BrowserFramePacing::RequestedAnimationBurst { .. }
                ));
        if wants_frame && !self.animation_frame_pending {
            self.animation_frame_pending = true;
            true
        } else {
            false
        }
    }
}
