use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use boon_host::SurfaceId;
use boon_native_gpu::{
    PresentedTextureReadbackRequest, RenderProof, RenderProofArtifact,
    complete_presented_texture_readback,
};
use futures::StreamExt;
use futures::channel::mpsc;
use sha2::{Digest, Sha256};

use crate::frame::PresentedReadbackTicket;
use crate::observer::{
    FrameEvidenceKey, ObserverEvent, PROOF_ARTIFACT_DIR_ENV, PROOF_MODE_ENV,
    PROOF_SAMPLE_ORDINAL_ENV, ProofArtifact,
};

const RESULT_QUEUE_DEPTH: usize = 2;
const DEFAULT_PROOF_SAMPLE_ORDINAL: u64 = 64;

#[derive(Clone, Debug)]
pub struct ProofConfig {
    pub artifact_dir: PathBuf,
    pub sample_ordinal: u64,
}

impl ProofConfig {
    pub fn from_env() -> Result<Option<Self>, String> {
        match std::env::var(PROOF_MODE_ENV).ok().as_deref() {
            None | Some("") | Some("off") => return Ok(None),
            Some("readback") => {}
            Some(value) => return Err(format!("unsupported verifier proof mode `{value}`")),
        }
        let artifact_dir = std::env::var_os(PROOF_ARTIFACT_DIR_ENV)
            .map(PathBuf::from)
            .ok_or_else(|| format!("{PROOF_ARTIFACT_DIR_ENV} is required in readback mode"))?;
        let sample_ordinal = std::env::var(PROOF_SAMPLE_ORDINAL_ENV)
            .ok()
            .map(|value| {
                value.parse::<u64>().map_err(|error| {
                    format!("invalid {PROOF_SAMPLE_ORDINAL_ENV} value `{value}`: {error}")
                })
            })
            .transpose()?
            .unwrap_or(DEFAULT_PROOF_SAMPLE_ORDINAL)
            .max(1);
        Ok(Some(Self {
            artifact_dir,
            sample_ordinal,
        }))
    }
}

pub struct ProofRequest {
    pub key: FrameEvidenceKey,
    pub readback: PresentedReadbackTicket,
    pub queued_at: Instant,
    pub queue_depth: u32,
}

pub struct ProofResult {
    pub key: FrameEvidenceKey,
    pub elapsed: Duration,
    pub queue_wait: Duration,
    pub end_to_end: Duration,
    pub queue_depth: u32,
    pub proof: Result<RenderProof, String>,
}

impl ProofResult {
    pub fn observer_event(
        self,
        completed_after_key: FrameEvidenceKey,
        replaced_count: u64,
        result_drop_count: u64,
    ) -> ObserverEvent {
        let elapsed_us = self.elapsed.as_micros().try_into().unwrap_or(u64::MAX);
        match self.proof {
            Ok(proof) => ObserverEvent::ProofCompleted {
                key: self.key,
                completed_after_key,
                elapsed_us,
                replaced_count,
                result_drop_count,
                artifact: proof_artifact(proof),
                error: None,
            },
            Err(error) => ObserverEvent::ProofCompleted {
                key: self.key,
                completed_after_key,
                elapsed_us,
                replaced_count,
                result_drop_count,
                artifact: None,
                error: Some(error),
            },
        }
    }
}

#[derive(Default)]
struct QueueState {
    pending: Option<ProofRequest>,
    closing: bool,
}

impl QueueState {
    fn enqueue(&mut self, request: ProofRequest) -> Result<(), String> {
        if self.pending.is_some() {
            return Err("proof queue already contains a pending production readback".to_owned());
        }
        self.pending = Some(request);
        Ok(())
    }
}

pub struct ProofWorker {
    queue: Arc<(Mutex<QueueState>, Condvar)>,
    results: mpsc::Receiver<ProofResult>,
    result_drops: Arc<AtomicU64>,
    thread: Option<JoinHandle<()>>,
}

impl ProofWorker {
    pub fn start(artifact_dir: PathBuf) -> Result<Self, String> {
        std::fs::create_dir_all(&artifact_dir).map_err(|error| {
            format!(
                "create verifier proof directory {}: {error}",
                artifact_dir.display()
            )
        })?;
        let queue = Arc::new((Mutex::new(QueueState::default()), Condvar::new()));
        let worker_queue = Arc::clone(&queue);
        let (results_tx, results) = mpsc::channel(RESULT_QUEUE_DEPTH);
        let result_drops = Arc::new(AtomicU64::new(0));
        let worker_drops = Arc::clone(&result_drops);
        let worker_artifact_dir = artifact_dir.clone();
        let thread = thread::Builder::new()
            .name("boon-proof-worker".to_owned())
            .spawn(move || proof_loop(worker_queue, results_tx, worker_drops, worker_artifact_dir))
            .map_err(|error| format!("spawn proof worker: {error}"))?;
        Ok(Self {
            queue,
            results,
            result_drops,
            thread: Some(thread),
        })
    }

    pub fn request_latest(&self, request: ProofRequest) -> Result<(), String> {
        if !request.key.is_complete() {
            return Err("proof request has an incomplete frame evidence key".to_owned());
        }
        if request.key != request.readback.key {
            return Err("proof request key differs from its production readback".to_owned());
        }
        if request.queue_depth == 0 {
            return Err("proof request has a zero queue depth".to_owned());
        }
        if !request
            .readback
            .artifact_label
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        {
            return Err("proof artifact label contains an unsafe character".to_owned());
        }
        let (lock, wake) = &*self.queue;
        let mut state = lock.lock().expect("proof queue lock");
        if state.closing {
            return Err("proof worker is closing".to_owned());
        }
        state.enqueue(request)?;
        wake.notify_one();
        Ok(())
    }

    pub async fn next_result(&mut self) -> Option<ProofResult> {
        self.results.next().await
    }

    pub fn replaced_count(&self) -> u64 {
        0
    }

    pub fn result_drop_count(&self) -> u64 {
        self.result_drops.load(Ordering::Relaxed)
    }
}

impl Drop for ProofWorker {
    fn drop(&mut self) {
        let (lock, wake) = &*self.queue;
        lock.lock().expect("proof queue lock").closing = true;
        wake.notify_one();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn proof_loop(
    queue: Arc<(Mutex<QueueState>, Condvar)>,
    mut results: mpsc::Sender<ProofResult>,
    result_drops: Arc<AtomicU64>,
    artifact_dir: PathBuf,
) {
    demote_proof_worker();
    loop {
        let request = {
            let (lock, wake) = &*queue;
            let mut state = lock.lock().expect("proof queue lock");
            while state.pending.is_none() && !state.closing {
                state = wake.wait(state).expect("proof queue wait");
            }
            if state.closing {
                return;
            }
            state.pending.take()
        };
        let Some(request) = request else {
            continue;
        };
        let started = Instant::now();
        let queue_wait = started.saturating_duration_since(request.queued_at);
        let capture_token_digest = frame_capture_token_digest(&request.key);
        let readback = request.readback;
        let proof = complete_presented_texture_readback(PresentedTextureReadbackRequest {
            device: &readback.device,
            submission_index: readback.submission_index,
            buffer: &readback.buffer,
            width: readback.width,
            height: readback.height,
            unpadded_bytes_per_row: readback.unpadded_bytes_per_row,
            padded_bytes_per_row: readback.padded_bytes_per_row,
            format: readback.format,
            surface_id: SurfaceId(request.key.surface_id.clone()),
            surface_epoch: request.key.surface_epoch,
            frame_seq: request.key.frame_id,
            capture_token_digest: &capture_token_digest,
            artifact_dir: &artifact_dir,
            artifact_label: &readback.artifact_label,
        })
        .map_err(|error| error.to_string());
        if results
            .try_send(ProofResult {
                key: request.key,
                elapsed: started.elapsed(),
                queue_wait,
                end_to_end: request.queued_at.elapsed(),
                queue_depth: request.queue_depth,
                proof,
            })
            .is_err()
        {
            result_drops.fetch_add(1, Ordering::Relaxed);
        }
    }
}

#[cfg(target_os = "linux")]
fn demote_proof_worker() {
    let scheduler = libc::sched_param { sched_priority: 0 };
    unsafe {
        libc::sched_setscheduler(0, libc::SCHED_BATCH, &scheduler);
        libc::setpriority(libc::PRIO_PROCESS, 0, 10);
    }
}

#[cfg(not(target_os = "linux"))]
fn demote_proof_worker() {}

fn proof_artifact(proof: RenderProof) -> Option<ProofArtifact> {
    match proof.artifact {
        RenderProofArtifact::AppOwnedPixels {
            artifact_path,
            artifact_sha256,
            capture_method,
            render_scene_identity_hash,
            nonblank_samples,
            unique_rgba_values,
            ..
        } => {
            let byte_len = std::fs::metadata(&artifact_path).ok()?.len();
            Some(ProofArtifact {
                path: artifact_path,
                sha256: artifact_sha256,
                byte_len,
                capture_method,
                capture_token_digest: render_scene_identity_hash,
                nonblank_samples: nonblank_samples.try_into().unwrap_or(u64::MAX),
                unique_rgba_values: unique_rgba_values.try_into().unwrap_or(u64::MAX),
            })
        }
        RenderProofArtifact::CopyToPresent { .. } => None,
    }
}

pub(crate) fn frame_capture_token_digest(key: &FrameEvidenceKey) -> String {
    let mut digest = Sha256::new();
    digest.update((key.surface_id.len() as u64).to_le_bytes());
    digest.update(key.surface_id.as_bytes());
    digest.update(key.process_id.to_le_bytes());
    digest.update((key.session_id.len() as u64).to_le_bytes());
    digest.update(key.session_id.as_bytes());
    for revision in [
        key.frame_id,
        key.input_id,
        key.content_id,
        key.layout_id,
        key.render_id,
        key.surface_epoch,
        key.present_id,
        key.proof_id,
    ] {
        digest.update(revision.to_le_bytes());
    }
    format!("{:x}", digest.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(frame_id: u64) -> FrameEvidenceKey {
        FrameEvidenceKey {
            surface_id: "preview-surface".to_owned(),
            process_id: 42,
            session_id: "launch-primary".to_owned(),
            frame_id,
            input_id: 2,
            content_id: 3,
            layout_id: 4,
            render_id: 5,
            surface_epoch: 6,
            present_id: 7,
            proof_id: 8,
        }
    }

    #[test]
    fn evidence_identity_includes_every_revision() {
        let baseline = frame_capture_token_digest(&key(1));
        assert_eq!(baseline.len(), 64);
        let mut changed = key(1);
        changed.present_id += 1;
        assert_ne!(baseline, frame_capture_token_digest(&changed));
    }

    #[test]
    fn proof_mode_is_off_without_an_explicit_environment_request() {
        if std::env::var_os(PROOF_MODE_ENV).is_none() {
            assert!(ProofConfig::from_env().unwrap().is_none());
        }
    }
}
