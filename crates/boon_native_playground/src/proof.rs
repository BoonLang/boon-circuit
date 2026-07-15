use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use boon_document::RenderScene;
use boon_host::SurfaceId;
use boon_native_gpu::{
    AppOwnedProofRenderer, AppOwnedRenderSceneRequest, RenderAssetSource, RenderProof,
    RenderProofArtifact,
};
use futures::StreamExt;
use futures::channel::mpsc;

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

#[derive(Clone)]
pub struct ProofRequest {
    pub key: FrameEvidenceKey,
    pub scene: RenderScene,
    pub width: u32,
    pub height: u32,
    pub surface_id: SurfaceId,
    pub artifact_label: String,
}

pub struct ProofResult {
    pub key: FrameEvidenceKey,
    pub elapsed: Duration,
    pub proof: Result<RenderProof, String>,
}

impl ProofResult {
    pub fn observer_event(
        self,
        completed_after_frame_id: u64,
        replaced_count: u64,
        result_drop_count: u64,
    ) -> ObserverEvent {
        let elapsed_us = self.elapsed.as_micros().try_into().unwrap_or(u64::MAX);
        match self.proof {
            Ok(proof) => ObserverEvent::ProofCompleted {
                key: self.key,
                completed_after_frame_id,
                elapsed_us,
                replaced_count,
                result_drop_count,
                artifact: proof_artifact(proof),
                error: None,
            },
            Err(error) => ObserverEvent::ProofCompleted {
                key: self.key,
                completed_after_frame_id,
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
    pending_asset_sources: Option<Vec<RenderAssetSource>>,
    closing: bool,
    replaced: u64,
}

impl QueueState {
    fn replace_pending(&mut self, request: ProofRequest) {
        if self.pending.replace(request).is_some() {
            self.replaced = self.replaced.saturating_add(1);
        }
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
        if !request
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
        state.replace_pending(request);
        wake.notify_one();
        Ok(())
    }

    pub fn replace_asset_sources(&self, sources: Vec<RenderAssetSource>) -> Result<(), String> {
        let (lock, wake) = &*self.queue;
        let mut state = lock.lock().expect("proof queue lock");
        if state.closing {
            return Err("proof worker is closing".to_owned());
        }
        state.pending_asset_sources = Some(sources);
        wake.notify_one();
        Ok(())
    }

    pub async fn next_result(&mut self) -> Option<ProofResult> {
        self.results.next().await
    }

    pub fn replaced_count(&self) -> u64 {
        self.queue.0.lock().expect("proof queue lock").replaced
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
    let mut gpu = futures::executor::block_on(create_gpu());
    let mut asset_error = None;
    loop {
        let (asset_sources, request) = {
            let (lock, wake) = &*queue;
            let mut state = lock.lock().expect("proof queue lock");
            while state.pending.is_none() && state.pending_asset_sources.is_none() && !state.closing
            {
                state = wake.wait(state).expect("proof queue wait");
            }
            if state.closing {
                return;
            }
            (state.pending_asset_sources.take(), state.pending.take())
        };

        if let Some(sources) = asset_sources {
            asset_error = match &mut gpu {
                Ok((_, _, renderer)) => renderer
                    .replace_asset_sources(sources)
                    .err()
                    .map(|error| error.to_string()),
                Err(error) => Some(error.clone()),
            };
        }

        let Some(request) = request else {
            continue;
        };
        let started = Instant::now();
        let proof = match asset_error.as_ref() {
            Some(error) => Err(error.clone()),
            None => match &mut gpu {
                Ok((device, queue, renderer)) => renderer
                    .render_scene_pixels(AppOwnedRenderSceneRequest {
                        device,
                        queue,
                        scene: &request.scene,
                        render_identity_hash: &render_identity(&request.key),
                        surface_id: request.surface_id,
                        surface_epoch: request.key.surface_epoch,
                        width: request.width,
                        height: request.height,
                        artifact_dir: &artifact_dir,
                        artifact_label: &request.artifact_label,
                    })
                    .map_err(|error| error.to_string()),
                Err(error) => Err(error.clone()),
            },
        };
        if results
            .try_send(ProofResult {
                key: request.key,
                elapsed: started.elapsed(),
                proof,
            })
            .is_err()
        {
            result_drops.fetch_add(1, Ordering::Relaxed);
        }
    }
}

async fn create_gpu() -> Result<(wgpu::Device, wgpu::Queue, AppOwnedProofRenderer), String> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            force_fallback_adapter: false,
            compatible_surface: None,
        })
        .await
        .map_err(|error| format!("request proof adapter: {error}"))?;
    let (device, queue) = adapter
        .request_device(&wgpu::DeviceDescriptor {
            label: Some("boon-proof-device"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits()),
            memory_hints: wgpu::MemoryHints::MemoryUsage,
            trace: wgpu::Trace::default(),
            experimental_features: wgpu::ExperimentalFeatures::default(),
        })
        .await
        .map_err(|error| format!("request proof device: {error}"))?;
    let renderer = AppOwnedProofRenderer::new(&device, &queue);
    Ok((device, queue, renderer))
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
                nonblank_samples: nonblank_samples.try_into().unwrap_or(u64::MAX),
                unique_rgba_values: unique_rgba_values.try_into().unwrap_or(u64::MAX),
            })
        }
        RenderProofArtifact::CopyToPresent { .. } => None,
    }
}

fn render_identity(key: &FrameEvidenceKey) -> String {
    format!(
        "surface-id:{}:process:{}:session:{}:frame:{}:input:{}:content:{}:layout:{}:render:{}:surface-epoch:{}:present:{}:proof:{}",
        key.surface_id,
        key.process_id,
        key.session_id,
        key.frame_id,
        key.input_id,
        key.content_id,
        key.layout_id,
        key.render_id,
        key.surface_epoch,
        key.present_id,
        key.proof_id
    )
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

    fn request(frame_id: u64) -> ProofRequest {
        ProofRequest {
            key: key(frame_id),
            scene: RenderScene {
                viewport: boon_document::Rect {
                    x: 0.0,
                    y: 0.0,
                    width: 1.0,
                    height: 1.0,
                },
                items: Vec::new(),
                visual_primitives: Vec::new(),
                quad_batches: Vec::new(),
                text_runs: Vec::new(),
                metrics: boon_document::render_scene::RenderSceneMetrics::default(),
            },
            width: 1,
            height: 1,
            surface_id: SurfaceId("preview".to_owned()),
            artifact_label: format!("frame-{frame_id}"),
        }
    }

    #[test]
    fn evidence_identity_includes_every_revision() {
        let identity = render_identity(&key(1));
        for value in 1..=8 {
            assert!(identity.contains(&value.to_string()));
        }
    }

    #[test]
    fn pending_queue_is_latest_wins_at_depth_one() {
        let mut queue = QueueState::default();
        queue.replace_pending(request(1));
        queue.replace_pending(request(2));
        queue.replace_pending(request(3));
        assert_eq!(queue.replaced, 2);
        assert_eq!(queue.pending.unwrap().key.frame_id, 3);
    }

    #[test]
    fn pending_asset_sources_are_latest_wins_without_consuming_proof_depth() {
        let mut queue = QueueState::default();
        queue.replace_pending(request(1));
        queue.pending_asset_sources = Some(vec![asset("asset://first")]);
        queue.pending_asset_sources = Some(vec![asset("asset://latest")]);

        assert_eq!(queue.replaced, 0);
        assert_eq!(queue.pending.unwrap().key.frame_id, 1);
        assert_eq!(
            queue.pending_asset_sources.unwrap()[0].url,
            "asset://latest"
        );
    }

    fn asset(url: &str) -> RenderAssetSource {
        RenderAssetSource {
            url: url.to_owned(),
            media_type: "image/png".to_owned(),
            sha256: "00".repeat(32),
            bytes: vec![0].into(),
        }
    }

    #[test]
    fn proof_mode_is_off_without_an_explicit_environment_request() {
        if std::env::var_os(PROOF_MODE_ENV).is_none() {
            assert!(ProofConfig::from_env().unwrap().is_none());
        }
    }
}
