use std::collections::BTreeMap;
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use boon_compiler::{
    CompilerSourceUnit, compile_runtime_source_units_to_machine_plan_with_persistence_catalog,
};
use boon_plan::{
    DEFAULT_PERSISTENCE_SCHEMA_VERSION, MachinePlan, MigrationPredecessorBinding, TargetProfile,
};
use boon_runtime::{
    ApplicationIdentity, ProgramArtifact, ProgramDiagnostic, ProgramHostRequest, ProgramRequestId,
    ProgramSessionId, compile_program_artifact,
};
use futures::channel::mpsc;

use crate::protocol::{MigrationBundle, PreviewIntent, SourceUnit, TestStep};

#[derive(Clone)]
pub struct CompileRequest {
    pub intent: PreviewIntent,
    pub request_id: Option<u64>,
    pub application: ApplicationIdentity,
    pub revision: u64,
    pub units: Vec<SourceUnit>,
    pub test_steps: Vec<TestStep>,
    pub migration: Option<MigrationBundle>,
    pub migration_stage: Option<String>,
}

pub struct CompiledPreview {
    pub intent: PreviewIntent,
    pub request_id: Option<u64>,
    pub revision: u64,
    pub elapsed: Duration,
    pub source_key: String,
    pub plan: Arc<MachinePlan>,
    pub test_steps: Vec<TestStep>,
}

pub struct CompileOutcome {
    pub revision: u64,
    pub result: Result<CompiledPreview, String>,
}

#[derive(Default)]
struct State {
    pending: Option<CompileRequest>,
    closing: bool,
    replaced: u64,
}

pub struct CompileWorker {
    state: Arc<(Mutex<State>, Condvar)>,
    thread: Option<JoinHandle<()>>,
}

impl CompileWorker {
    pub fn start() -> (Self, mpsc::UnboundedReceiver<CompileOutcome>) {
        let state = Arc::new((Mutex::new(State::default()), Condvar::new()));
        let worker_state = Arc::clone(&state);
        let (output, receiver) = mpsc::unbounded();
        let thread = thread::Builder::new()
            .name("boon-preview-compile".to_owned())
            .spawn(move || compile_loop(worker_state, output))
            .expect("spawn preview compile worker");
        (
            Self {
                state,
                thread: Some(thread),
            },
            receiver,
        )
    }

    pub fn replace(&self, request: CompileRequest) {
        let (lock, wake) = &*self.state;
        let mut state = lock.lock().expect("compile worker lock");
        if state.pending.replace(request).is_some() {
            state.replaced = state.replaced.saturating_add(1);
        }
        wake.notify_one();
    }

    pub fn replaced_count(&self) -> u64 {
        self.state.0.lock().expect("compile worker lock").replaced
    }
}

impl Drop for CompileWorker {
    fn drop(&mut self) {
        let (lock, wake) = &*self.state;
        lock.lock().expect("compile worker lock").closing = true;
        wake.notify_one();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

pub struct ProgramCompileOutcome {
    pub request_id: ProgramRequestId,
    pub session: ProgramSessionId,
    pub result: Result<ProgramArtifact, ProgramDiagnostic>,
}

#[derive(Default)]
struct ProgramCompileState {
    pending: BTreeMap<ProgramSessionId, ProgramHostRequest>,
    closing: bool,
    replaced: u64,
}

pub struct ProgramCompileWorker {
    state: Arc<(Mutex<ProgramCompileState>, Condvar)>,
    thread: Option<JoinHandle<()>>,
}

impl ProgramCompileWorker {
    pub fn start() -> (Self, mpsc::UnboundedReceiver<ProgramCompileOutcome>) {
        let state = Arc::new((Mutex::new(ProgramCompileState::default()), Condvar::new()));
        let worker_state = Arc::clone(&state);
        let (output, receiver) = mpsc::unbounded();
        let thread = thread::Builder::new()
            .name("boon-program-compile".to_owned())
            .spawn(move || program_compile_loop(worker_state, output))
            .expect("spawn child program compile worker");
        (
            Self {
                state,
                thread: Some(thread),
            },
            receiver,
        )
    }

    pub fn replace(&self, request: ProgramHostRequest) {
        let (lock, wake) = &*self.state;
        let mut state = lock.lock().expect("program compile worker lock");
        if state
            .pending
            .insert(request.session.clone(), request)
            .is_some()
        {
            state.replaced = state.replaced.saturating_add(1);
        }
        wake.notify_one();
    }

    #[cfg(test)]
    pub fn replaced_count(&self) -> u64 {
        self.state
            .0
            .lock()
            .expect("program compile worker lock")
            .replaced
    }
}

impl Drop for ProgramCompileWorker {
    fn drop(&mut self) {
        let (lock, wake) = &*self.state;
        lock.lock().expect("program compile worker lock").closing = true;
        wake.notify_one();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn program_compile_loop(
    state: Arc<(Mutex<ProgramCompileState>, Condvar)>,
    output: mpsc::UnboundedSender<ProgramCompileOutcome>,
) {
    loop {
        let request = {
            let (lock, wake) = &*state;
            let mut state = lock.lock().expect("program compile worker lock");
            while state.pending.is_empty() && !state.closing {
                state = wake.wait(state).expect("program compile worker wait");
            }
            if state.closing {
                return;
            }
            let session = state
                .pending
                .keys()
                .next()
                .cloned()
                .expect("nonempty program compile queue");
            state
                .pending
                .remove(&session)
                .expect("program compile request")
        };
        let request_id = request.request_id;
        let session = request.session;
        let result = compile_program_artifact(&request.compile);
        if output
            .unbounded_send(ProgramCompileOutcome {
                request_id,
                session,
                result,
            })
            .is_err()
        {
            return;
        }
    }
}

fn compile_loop(
    state: Arc<(Mutex<State>, Condvar)>,
    output: mpsc::UnboundedSender<CompileOutcome>,
) {
    loop {
        let request = {
            let (lock, wake) = &*state;
            let mut state = lock.lock().expect("compile worker lock");
            while state.pending.is_none() && !state.closing {
                state = wake.wait(state).expect("compile worker wait");
            }
            if state.closing {
                return;
            }
            state.pending.take().expect("pending compile request")
        };
        let revision = request.revision;
        let result = compile(request);
        if output
            .unbounded_send(CompileOutcome { revision, result })
            .is_err()
        {
            return;
        }
    }
}

fn compile(request: CompileRequest) -> Result<CompiledPreview, String> {
    if request.units.is_empty() {
        return Err("preview source bundle is empty".to_owned());
    }
    let started = Instant::now();
    let source_key = project_key_for_stage(
        &request.application,
        &request.units,
        request.migration_stage.as_deref(),
    );
    let label = request
        .units
        .last()
        .map(|unit| unit.path.clone())
        .unwrap_or_else(|| "preview.bn".to_owned());
    let plan = match (&request.migration, request.migration_stage.as_deref()) {
        (Some(migration), Some(stage_id)) => compile_migration_stage_with_units(
            &request.application,
            migration,
            stage_id,
            Some(&request.units),
        )?,
        (None, None) => {
            let units = compiler_units(&request.units);
            Arc::new(
                compile_runtime_source_units_to_machine_plan_with_persistence_catalog(
                    &label,
                    &units,
                    TargetProfile::SoftwareDefault,
                    request.application,
                    DEFAULT_PERSISTENCE_SCHEMA_VERSION,
                    &[] as &[MigrationPredecessorBinding],
                )
                .map_err(|error| error.to_string())?
                .plan,
            )
        }
        _ => {
            return Err(
                "migration source compile requires both a bundle and an active stage".to_owned(),
            );
        }
    };
    Ok(CompiledPreview {
        intent: request.intent,
        request_id: request.request_id,
        revision: request.revision,
        elapsed: started.elapsed(),
        source_key,
        plan,
        test_steps: request.test_steps,
    })
}

pub fn compile_migration_stage(
    application: &ApplicationIdentity,
    migration: &MigrationBundle,
    target_stage: &str,
) -> Result<Arc<MachinePlan>, String> {
    compile_migration_stage_with_units(application, migration, target_stage, None)
}

fn compile_migration_stage_with_units(
    application: &ApplicationIdentity,
    migration: &MigrationBundle,
    target_stage: &str,
    target_units: Option<&[SourceUnit]>,
) -> Result<Arc<MachinePlan>, String> {
    if migration.stage(target_stage).is_none() {
        return Err(format!("migration stage `{target_stage}` is absent"));
    }
    let mut predecessor = None::<MigrationPredecessorBinding>;
    for stage in &migration.stages {
        let units = compiler_units(if stage.id == target_stage {
            target_units.unwrap_or(&stage.units)
        } else {
            &stage.units
        });
        let predecessors = predecessor.as_slice();
        let plan = Arc::new(
            compile_runtime_source_units_to_machine_plan_with_persistence_catalog(
                &stage.source,
                &units,
                TargetProfile::SoftwareDefault,
                application.clone(),
                stage.schema_version,
                predecessors,
            )
            .map_err(|error| format!("migration stage `{}` failed to compile: {error}", stage.id))?
            .plan,
        );
        if stage.id == target_stage {
            return Ok(plan);
        }
        predecessor = Some(MigrationPredecessorBinding::from_machine_plan(&plan));
    }
    Err(format!("migration stage `{target_stage}` is absent"))
}

fn compiler_units(units: &[SourceUnit]) -> Vec<CompilerSourceUnit> {
    units
        .iter()
        .map(|unit| CompilerSourceUnit {
            path: unit.path.clone(),
            source: unit.source.clone(),
        })
        .collect()
}

pub fn source_key(units: &[SourceUnit]) -> String {
    let parts = units
        .iter()
        .map(|unit| (unit.path.as_str(), unit.source.as_str()))
        .collect::<Vec<_>>();
    boon_runtime::source_unit_parts_hash(&parts)
}

#[cfg(test)]
pub fn project_key(application: &ApplicationIdentity, units: &[SourceUnit]) -> String {
    project_key_for_stage(application, units, None)
}

pub fn project_key_for_stage(
    application: &ApplicationIdentity,
    units: &[SourceUnit],
    migration_stage: Option<&str>,
) -> String {
    let source = source_key(units);
    boon_runtime::source_unit_parts_hash(&[
        ("application.package_id", application.package_id.as_str()),
        (
            "application.state_namespace",
            application.state_namespace.as_str(),
        ),
        (
            "application.deployment_domain",
            application.deployment_domain.as_str(),
        ),
        ("migration.stage", migration_stage.unwrap_or_default()),
        ("source", source.as_str()),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn application(namespace: &str) -> ApplicationIdentity {
        ApplicationIdentity::new("dev.boon.test", namespace, "test")
    }

    #[test]
    fn mailbox_is_depth_one_and_latest_wins() {
        let (worker, _results) = CompileWorker::start();
        for revision in 1..=4 {
            worker.replace(CompileRequest {
                intent: PreviewIntent::Replace,
                request_id: None,
                application: application(&format!("mailbox-{revision}")),
                revision,
                units: Vec::new(),
                test_steps: Vec::new(),
                migration: None,
                migration_stage: None,
            });
        }
        assert!(worker.replaced_count() >= 1);
    }

    #[test]
    fn child_program_mailbox_is_depth_one_per_session_and_latest_wins() {
        let worker = ProgramCompileWorker {
            state: Arc::new((Mutex::new(ProgramCompileState::default()), Condvar::new())),
            thread: None,
        };
        let host = boon_document_model::DocumentNodeId("program-host".to_owned());
        let session = ProgramSessionId("public-page".to_owned());
        for revision in 1..=4 {
            worker.replace(ProgramHostRequest {
                request_id: ProgramRequestId(format!("request-{revision}")),
                session: session.clone(),
                host: host.clone(),
                compile: boon_runtime::ProgramCompileRequest {
                    revision,
                    source_label: "Child.bn".to_owned(),
                    units: vec![boon_runtime::RuntimeSourceUnit {
                        path: "RUN.bn".to_owned(),
                        source: format!("value: {revision}\n"),
                    }],
                    application: application("child-mailbox"),
                    capability_profile:
                        boon_document_model::ProgramCapabilityProfile::PublicDocument,
                },
            });
        }

        assert_eq!(worker.replaced_count(), 3);
        let state = worker.state.0.lock().unwrap();
        assert_eq!(state.pending.len(), 1);
        assert_eq!(state.pending[&session].compile.revision, 4);
    }

    #[test]
    fn project_key_partitions_identical_source_by_application_identity() {
        let units = vec![SourceUnit {
            path: "RUN.bn".to_owned(),
            source: "value: 1\n".to_owned(),
        }];
        assert_ne!(
            project_key(&application("first"), &units),
            project_key(&application("second"), &units)
        );
    }

    #[test]
    fn compile_installs_the_host_application_identity_in_the_machine_plan() {
        let application = application("compile-propagation");
        let compiled = compile(CompileRequest {
            intent: PreviewIntent::Replace,
            request_id: None,
            application: application.clone(),
            revision: 1,
            units: vec![SourceUnit {
                path: "examples/minimal.bn".to_owned(),
                source: include_str!("../../../examples/minimal.bn").to_owned(),
            }],
            test_steps: Vec::new(),
            migration: None,
            migration_stage: None,
        })
        .expect("compile preview with host identity");
        assert_eq!(compiled.plan.application.identity, application);
        assert_eq!(
            compiled.plan.persistence.schema_version,
            DEFAULT_PERSISTENCE_SCHEMA_VERSION
        );
    }
}
