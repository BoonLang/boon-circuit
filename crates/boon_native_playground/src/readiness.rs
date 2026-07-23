use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use boon_plan::MachinePlan;
use boon_runtime::{PersistentPlanBuildRequest, PreparedPersistentPlanActivation};
use futures::channel::mpsc;

use crate::compile::{CompiledExecutable, CompiledPreview};
use crate::protocol::{AssetBlob, PreviewIntent, TestStep};
use crate::runtime_view::{RuntimeOwnerStamp, RuntimeView};

const MAX_AUTHORITY_RECAPTURE_ATTEMPTS: u8 = 2;

#[derive(Clone)]
pub struct RuntimeReadinessRetry {
    job_id: u64,
    intent: PreviewIntent,
    request_id: Option<u64>,
    revision: u64,
    compile_elapsed: Duration,
    source_key: String,
    plan: Arc<MachinePlan>,
    test_steps: Vec<TestStep>,
    attempt: u8,
}

impl RuntimeReadinessRetry {
    pub fn recapture(self, runtime: &RuntimeView) -> Result<RuntimeReadinessRequest, String> {
        if self.attempt >= MAX_AUTHORITY_RECAPTURE_ATTEMPTS {
            return Err(format!(
                "prepared replacement remained stale after {} authority recaptures",
                self.attempt
            ));
        }
        if runtime.application_identity() != &self.plan.application.identity {
            return Err("replacement retry belongs to a different runtime owner".to_owned());
        }
        if !runtime.plan_schema_matches(&self.plan) {
            return Err("replacement retry schema no longer matches the active runtime".to_owned());
        }
        let build = runtime.prepare_machine_plan_build(Arc::clone(&self.plan))?;
        let expected_runtime_owner = Some(runtime.owner_stamp());
        let next_retry = Self {
            attempt: self.attempt.saturating_add(1),
            ..self
        };
        Ok(RuntimeReadinessRequest {
            job_id: next_retry.job_id,
            intent: next_retry.intent,
            request_id: next_retry.request_id,
            revision: next_retry.revision,
            compile_elapsed: next_retry.compile_elapsed,
            source_key: next_retry.source_key.clone(),
            expected_runtime_owner,
            build: RuntimeBuild::Replacement(build),
            test_steps: next_retry.test_steps.clone(),
            retry: Some(next_retry),
        })
    }

    pub const fn attempt(&self) -> u8 {
        self.attempt
    }
}

pub enum PreparedRuntimeActivation {
    Opened(Box<RuntimeView>),
    Replacement(PreparedPersistentPlanActivation),
}

pub struct ReadyPreview {
    pub intent: PreviewIntent,
    pub request_id: Option<u64>,
    pub revision: u64,
    pub compile_elapsed: Duration,
    pub readiness_elapsed: Duration,
    pub source_key: String,
    pub expected_runtime_owner: Option<RuntimeOwnerStamp>,
    pub activation: Result<PreparedRuntimeActivation, String>,
    pub test_steps: Vec<TestStep>,
    pub retry: Option<RuntimeReadinessRetry>,
}

pub struct RuntimeReadinessOutcome {
    pub job_id: u64,
    pub revision: u64,
    pub result: Result<ReadyPreview, String>,
}

enum RuntimeBuild {
    Open {
        executable: CompiledExecutable,
        deterministic_scenario: bool,
        isolated_scenario: bool,
        assets: Arc<Vec<AssetBlob>>,
    },
    Replacement(PersistentPlanBuildRequest),
    ActivationFailed(String),
    CompileFailed(String),
}

pub struct RuntimeReadinessRequest {
    job_id: u64,
    intent: PreviewIntent,
    request_id: Option<u64>,
    revision: u64,
    compile_elapsed: Duration,
    source_key: String,
    expected_runtime_owner: Option<RuntimeOwnerStamp>,
    build: RuntimeBuild,
    test_steps: Vec<TestStep>,
    retry: Option<RuntimeReadinessRetry>,
}

impl RuntimeReadinessRequest {
    pub fn compile_failed(job_id: u64, revision: u64, error: String) -> Self {
        Self {
            job_id,
            intent: PreviewIntent::Replace,
            request_id: None,
            revision,
            compile_elapsed: Duration::ZERO,
            source_key: String::new(),
            expected_runtime_owner: None,
            build: RuntimeBuild::CompileFailed(error),
            test_steps: Vec::new(),
            retry: None,
        }
    }

    pub fn prepare(
        compiled: CompiledPreview,
        runtime: Option<&RuntimeView>,
        deterministic_scenario: bool,
        isolated_scenario: bool,
        assets: Arc<Vec<AssetBlob>>,
    ) -> Self {
        let CompiledPreview {
            job_id,
            intent,
            request_id,
            revision,
            elapsed,
            source_key,
            executable,
            test_steps,
        } = compiled;
        let (build, expected_runtime_owner, replacement_plan) = match executable {
            CompiledExecutable::BuiltInSingleRole(plan)
                if !isolated_scenario
                    && runtime.is_some_and(|runtime| {
                        runtime.application_identity() == &plan.application.identity
                    }) =>
            {
                let runtime = runtime.expect("compatible runtime checked above");
                if !runtime.plan_schema_matches(&plan) {
                    (
                        RuntimeBuild::ActivationFailed(
                            "same-identity schema change requires Migration Preview and Activate"
                                .to_owned(),
                        ),
                        None,
                        None,
                    )
                } else {
                    let owner = runtime.owner_stamp();
                    let replacement_plan = Arc::clone(&plan);
                    (
                        runtime
                            .prepare_machine_plan_build(plan)
                            .map(RuntimeBuild::Replacement)
                            .unwrap_or_else(RuntimeBuild::ActivationFailed),
                        Some(owner),
                        Some(replacement_plan),
                    )
                }
            }
            executable => (
                RuntimeBuild::Open {
                    executable,
                    deterministic_scenario,
                    isolated_scenario,
                    assets,
                },
                None,
                None,
            ),
        };
        let retry = replacement_plan.map(|plan| RuntimeReadinessRetry {
            job_id,
            intent,
            request_id,
            revision,
            compile_elapsed: elapsed,
            source_key: source_key.clone(),
            plan,
            test_steps: test_steps.clone(),
            attempt: 0,
        });
        Self {
            job_id,
            intent,
            request_id,
            revision,
            compile_elapsed: elapsed,
            source_key,
            expected_runtime_owner,
            build,
            test_steps,
            retry,
        }
    }
}

#[derive(Default)]
struct State {
    pending: Option<RuntimeReadinessRequest>,
    closing: bool,
    replaced: u64,
}

pub struct RuntimeReadinessWorker {
    state: Arc<(Mutex<State>, Condvar)>,
    thread: Option<JoinHandle<()>>,
}

impl RuntimeReadinessWorker {
    pub fn start() -> (Self, mpsc::UnboundedReceiver<RuntimeReadinessOutcome>) {
        let state = Arc::new((Mutex::new(State::default()), Condvar::new()));
        let worker_state = Arc::clone(&state);
        let (output, receiver) = mpsc::unbounded();
        let thread = thread::Builder::new()
            .name("boon-preview-readiness".to_owned())
            .spawn(move || readiness_loop(worker_state, output))
            .expect("spawn preview readiness worker");
        (
            Self {
                state,
                thread: Some(thread),
            },
            receiver,
        )
    }

    pub fn replace(&self, request: RuntimeReadinessRequest) {
        let (lock, wake) = &*self.state;
        let mut state = lock.lock().expect("readiness worker lock");
        if state.pending.replace(request).is_some() {
            state.replaced = state.replaced.saturating_add(1);
        }
        wake.notify_one();
    }

    pub fn replaced_count(&self) -> u64 {
        self.state.0.lock().expect("readiness worker lock").replaced
    }
}

impl Drop for RuntimeReadinessWorker {
    fn drop(&mut self) {
        let (lock, wake) = &*self.state;
        lock.lock().expect("readiness worker lock").closing = true;
        wake.notify_one();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn readiness_loop(
    state: Arc<(Mutex<State>, Condvar)>,
    output: mpsc::UnboundedSender<RuntimeReadinessOutcome>,
) {
    loop {
        let request = {
            let (lock, wake) = &*state;
            let mut state = lock.lock().expect("readiness worker lock");
            while state.pending.is_none() && !state.closing {
                state = wake.wait(state).expect("readiness worker wait");
            }
            if state.closing {
                return;
            }
            state.pending.take().expect("pending readiness request")
        };
        let job_id = request.job_id;
        let revision = request.revision;
        let result = prepare_runtime(request);
        if output
            .unbounded_send(RuntimeReadinessOutcome {
                job_id,
                revision,
                result,
            })
            .is_err()
        {
            return;
        }
    }
}

fn prepare_runtime(request: RuntimeReadinessRequest) -> Result<ReadyPreview, String> {
    let RuntimeReadinessRequest {
        job_id: _,
        intent,
        request_id,
        revision,
        compile_elapsed,
        source_key,
        expected_runtime_owner,
        build,
        test_steps,
        retry,
    } = request;
    let started = Instant::now();
    let activation = match build {
        RuntimeBuild::Open {
            executable,
            deterministic_scenario,
            isolated_scenario,
            assets,
        } => open_runtime(
            executable,
            deterministic_scenario,
            isolated_scenario,
            &assets,
        ),
        RuntimeBuild::Replacement(request) => request
            .build()
            .map(PreparedRuntimeActivation::Replacement)
            .map_err(|error| error.to_string()),
        RuntimeBuild::ActivationFailed(error) => Err(error),
        RuntimeBuild::CompileFailed(error) => return Err(error),
    };
    Ok(ReadyPreview {
        intent,
        request_id,
        revision,
        compile_elapsed,
        readiness_elapsed: started.elapsed(),
        source_key,
        expected_runtime_owner,
        activation,
        test_steps,
        retry,
    })
}

fn open_runtime(
    executable: CompiledExecutable,
    deterministic_scenario: bool,
    isolated_scenario: bool,
    assets: &[AssetBlob],
) -> Result<PreparedRuntimeActivation, String> {
    match executable {
        CompiledExecutable::BuiltInSingleRole(plan) if isolated_scenario => {
            RuntimeView::open_for_scenario_with_assets(plan, assets)
        }
        CompiledExecutable::BuiltInSingleRole(plan) => {
            RuntimeView::open_with_assets(plan, deterministic_scenario, assets)
        }
        CompiledExecutable::DistributedPackage(bundle) => {
            RuntimeView::open_distributed_with_assets(
                bundle,
                deterministic_scenario || isolated_scenario,
                assets,
            )
        }
    }
    .map(|runtime| PreparedRuntimeActivation::Opened(Box::new(runtime)))
}
