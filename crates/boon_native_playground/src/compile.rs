use std::collections::BTreeMap;
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use boon_compiler::{
    CompileRequest as MachinePlanCompileRequest, CompilerSourceUnit, compile_machine_plan,
};
use boon_contract::{CanonicalSourceBundleV1, SourceBundleDigestV1, SourceBundleUnit};
use boon_plan::{
    DEFAULT_PERSISTENCE_SCHEMA_VERSION, MachinePlan, MigrationPredecessorBinding, ProgramRole,
    TargetProfile,
};
use boon_program_runtime::{
    DistributedProgramBundle, ProgramArtifact, ProgramDiagnostic, ProgramHostRequest,
    ProgramRequestId, ProgramSessionId, compile_program_artifact,
};
use boon_runtime::ApplicationIdentity;
use futures::channel::mpsc;
use sha2::{Digest, Sha256};

use crate::distributed_program::compile_distributed_program;
#[cfg(test)]
use crate::protocol::ProgramSource;
use crate::protocol::{MigrationBundle, PreviewIntent, PreviewSource, SourceUnit, TestStep};

#[derive(Clone)]
pub struct CompileRequest {
    pub job_id: u64,
    pub intent: PreviewIntent,
    pub request_id: Option<u64>,
    pub revision: u64,
    pub source: PreviewSource,
    pub test_steps: Vec<TestStep>,
    pub migration: Option<MigrationBundle>,
    pub migration_stage: Option<String>,
}

pub enum CompiledExecutable {
    BuiltInSingleRole(Arc<MachinePlan>),
    DistributedPackage(DistributedProgramBundle),
}

pub struct CompiledPreview {
    pub job_id: u64,
    pub intent: PreviewIntent,
    pub request_id: Option<u64>,
    pub revision: u64,
    pub elapsed: Duration,
    pub source_key: String,
    pub executable: CompiledExecutable,
    pub test_steps: Vec<TestStep>,
}

pub struct CompileOutcome {
    pub job_id: u64,
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
    pub revision: u64,
    pub elapsed: Duration,
    pub queue_wait: Duration,
    pub queued_at: Instant,
    pub completed_at: Instant,
    pub pending_depth: u32,
    pub result: Result<ProgramArtifact, ProgramDiagnostic>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProgramCompileReceipt {
    pub accepted: bool,
    pub pending_depth: u32,
}

struct PendingProgramCompile {
    request: ProgramHostRequest,
    pending_depth: u32,
    queued_at: Instant,
}

#[derive(Default)]
struct ProgramCompileState {
    pending: BTreeMap<ProgramSessionId, PendingProgramCompile>,
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

    pub fn replace(&self, request: ProgramHostRequest) -> ProgramCompileReceipt {
        let (lock, wake) = &*self.state;
        let mut state = lock.lock().expect("program compile worker lock");
        if state.pending.contains_key(&request.session) {
            state.replaced = state.replaced.saturating_add(1);
        }
        let session = request.session.clone();
        state.pending.insert(
            session.clone(),
            PendingProgramCompile {
                request,
                pending_depth: 0,
                queued_at: Instant::now(),
            },
        );
        let pending_depth = state.pending.len().try_into().unwrap_or(u32::MAX);
        state
            .pending
            .get_mut(&session)
            .expect("inserted child program compile")
            .pending_depth = pending_depth;
        wake.notify_one();
        ProgramCompileReceipt {
            accepted: true,
            pending_depth,
        }
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
        let PendingProgramCompile {
            request,
            pending_depth,
            queued_at,
        } = request;
        let request_id = request.request_id;
        let session = request.session;
        let revision = request.compile.revision;
        let started = Instant::now();
        let queue_wait = started.saturating_duration_since(queued_at);
        let result = compile_program_artifact(&request.compile);
        let elapsed = started.elapsed();
        let completed_at = Instant::now();
        if output
            .unbounded_send(ProgramCompileOutcome {
                request_id,
                session,
                revision,
                elapsed,
                queue_wait,
                queued_at,
                completed_at,
                pending_depth,
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
        let job_id = request.job_id;
        let revision = request.revision;
        let result = compile(request);
        if output
            .unbounded_send(CompileOutcome {
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

fn compile(request: CompileRequest) -> Result<CompiledPreview, String> {
    let started = Instant::now();
    let source_key = preview_project_key(
        &request.source,
        request.migration.as_ref(),
        request.migration_stage.as_deref(),
    )?;
    let executable = match &request.source {
        PreviewSource::BuiltInSingleRole {
            application,
            entry_path,
            units,
        } => {
            if units.is_empty() {
                return Err("preview source bundle is empty".to_owned());
            }
            let plan = match (&request.migration, request.migration_stage.as_deref()) {
                (Some(migration), Some(stage_id)) => compile_migration_stage_with_units(
                    application,
                    migration,
                    stage_id,
                    Some((entry_path, units)),
                )?,
                (None, None) => {
                    let bundle = canonical_preview_source_bundle(entry_path, units)?;
                    let entry_path = bundle.entrypoint().to_owned();
                    let units = bundle
                        .units()
                        .iter()
                        .map(|unit| CompilerSourceUnit {
                            path: unit.path().to_owned(),
                            source: unit.source().to_owned(),
                        })
                        .collect::<Vec<_>>();
                    Arc::new(
                        compile_machine_plan(
                            MachinePlanCompileRequest::source_units(
                                &entry_path,
                                &units,
                                TargetProfile::SoftwareDefault,
                                ProgramRole::Client,
                                application.clone(),
                            )
                            .with_persistence_catalog(
                                DEFAULT_PERSISTENCE_SCHEMA_VERSION,
                                &[] as &[MigrationPredecessorBinding],
                            ),
                        )
                        .map_err(|error| error.to_string())?
                        .plan,
                    )
                }
                _ => {
                    return Err(
                        "migration source compile requires both a bundle and an active stage"
                            .to_owned(),
                    );
                }
            };
            CompiledExecutable::BuiltInSingleRole(plan)
        }
        PreviewSource::DistributedPackage { programs } => {
            if programs.is_empty() {
                return Err("distributed package source is empty".to_owned());
            }
            if request.migration.is_some() || request.migration_stage.is_some() {
                return Err(
                    "distributed packages cannot use single-role migration bundles".to_owned(),
                );
            }
            CompiledExecutable::DistributedPackage(
                compile_distributed_program(programs.clone()).map_err(|error| error.to_string())?,
            )
        }
    };
    Ok(CompiledPreview {
        job_id: request.job_id,
        intent: request.intent,
        request_id: request.request_id,
        revision: request.revision,
        elapsed: started.elapsed(),
        source_key,
        executable,
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
    target_source: Option<(&str, &[SourceUnit])>,
) -> Result<Arc<MachinePlan>, String> {
    if migration.stage(target_stage).is_none() {
        return Err(format!("migration stage `{target_stage}` is absent"));
    }
    let mut predecessor = None::<MigrationPredecessorBinding>;
    for stage in &migration.stages {
        let source_units = if stage.id == target_stage {
            if let Some((entry_path, units)) = target_source {
                let working_entrypoint = canonical_preview_source_bundle(entry_path, units)?
                    .entrypoint()
                    .to_owned();
                let declared_entrypoint = canonical_preview_source_bundle(&stage.source, units)?
                    .entrypoint()
                    .to_owned();
                if working_entrypoint != declared_entrypoint {
                    return Err(format!(
                        "migration stage `{target_stage}` working entrypoint `{working_entrypoint}` differs from declared entrypoint `{declared_entrypoint}`"
                    ));
                }
                units
            } else {
                &stage.units
            }
        } else {
            &stage.units
        };
        let bundle = canonical_preview_source_bundle(&stage.source, source_units)?;
        let entry_path = bundle.entrypoint().to_owned();
        let units = bundle
            .units()
            .iter()
            .map(|unit| CompilerSourceUnit {
                path: unit.path().to_owned(),
                source: unit.source().to_owned(),
            })
            .collect::<Vec<_>>();
        let predecessors = predecessor.as_slice();
        let plan = Arc::new(
            compile_machine_plan(
                MachinePlanCompileRequest::source_units(
                    &entry_path,
                    &units,
                    TargetProfile::SoftwareDefault,
                    ProgramRole::Client,
                    application.clone(),
                )
                .with_persistence_catalog(stage.schema_version, predecessors),
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

fn canonical_preview_source_bundle<'a>(
    entry_path: &str,
    units: &'a [SourceUnit],
) -> Result<CanonicalSourceBundleV1<'a>, String> {
    CanonicalSourceBundleV1::new(
        entry_path,
        units
            .iter()
            .map(|unit| SourceBundleUnit::new(&unit.path, &unit.source)),
    )
    .map_err(|error| format!("invalid preview source bundle identity: {error}"))
}

#[cfg(test)]
pub fn source_key(entry_path: &str, units: &[SourceUnit]) -> Result<String, String> {
    canonical_preview_source_bundle(entry_path, units).map(|bundle| bundle.digest().to_string())
}

#[cfg(test)]
pub fn project_key(
    application: &ApplicationIdentity,
    entry_path: &str,
    units: &[SourceUnit],
) -> Result<String, String> {
    project_key_for_stage(application, entry_path, units, None)
}

pub fn project_key_for_stage(
    application: &ApplicationIdentity,
    entry_path: &str,
    units: &[SourceUnit],
    migration_stage: Option<&str>,
) -> Result<String, String> {
    let digest = canonical_preview_source_bundle(entry_path, units)?.digest();
    Ok(preview_identity_key(
        migration_stage,
        [(boon_plan::ProgramRole::Client, application, digest)],
        [],
    ))
}

pub fn preview_project_key(
    source: &PreviewSource,
    migration: Option<&MigrationBundle>,
    migration_stage: Option<&str>,
) -> Result<String, String> {
    let programs = match source {
        PreviewSource::BuiltInSingleRole {
            application,
            entry_path,
            units,
        } => {
            return match (migration, migration_stage) {
                (None, None) => project_key_for_stage(application, entry_path, units, None),
                (Some(migration), Some(stage_id)) => migration_project_key(
                    application,
                    migration,
                    stage_id,
                    Some((entry_path, units)),
                ),
                _ => Err(
                    "migration source identity requires both a bundle and an active stage"
                        .to_owned(),
                ),
            };
        }
        PreviewSource::DistributedPackage { programs } => programs,
    };
    if migration.is_some() || migration_stage.is_some() {
        return Err("distributed packages cannot use single-role migration identity".to_owned());
    }
    let mut programs = programs.iter().collect::<Vec<_>>();
    programs.sort_by_key(|program| match program.role {
        boon_plan::ProgramRole::Client => 0,
        boon_plan::ProgramRole::Session => 1,
        boon_plan::ProgramRole::Server => 2,
    });
    let mut identities = Vec::with_capacity(programs.len());
    for program in programs {
        let digest = canonical_preview_source_bundle(&program.entry_path, &program.units)?.digest();
        identities.push((program.role, &program.application, digest));
    }
    Ok(preview_identity_key(None, identities, []))
}

pub(crate) fn migration_project_key(
    application: &ApplicationIdentity,
    migration: &MigrationBundle,
    target_stage: &str,
    target_source: Option<(&str, &[SourceUnit])>,
) -> Result<String, String> {
    let mut stages = Vec::new();
    for stage in &migration.stages {
        let units = if stage.id == target_stage {
            if let Some((entry_path, units)) = target_source {
                let working_entrypoint = canonical_preview_source_bundle(entry_path, units)?
                    .entrypoint()
                    .to_owned();
                let declared_entrypoint = canonical_preview_source_bundle(&stage.source, units)?
                    .entrypoint()
                    .to_owned();
                if working_entrypoint != declared_entrypoint {
                    return Err(format!(
                        "migration stage `{target_stage}` working entrypoint `{working_entrypoint}` differs from declared entrypoint `{declared_entrypoint}`"
                    ));
                }
                units
            } else {
                &stage.units
            }
        } else {
            &stage.units
        };
        let digest = canonical_preview_source_bundle(&stage.source, units)?.digest();
        stages.push((stage.id.as_str(), stage.schema_version, digest));
        if stage.id == target_stage {
            return Ok(preview_identity_key(
                Some(target_stage),
                [(boon_plan::ProgramRole::Client, application, digest)],
                stages,
            ));
        }
    }
    Err(format!("migration stage `{target_stage}` is absent"))
}

fn preview_identity_key<'a, 'b>(
    migration_stage: Option<&str>,
    programs: impl IntoIterator<
        Item = (
            boon_plan::ProgramRole,
            &'a ApplicationIdentity,
            SourceBundleDigestV1,
        ),
    >,
    migration_stages: impl IntoIterator<Item = (&'b str, u64, SourceBundleDigestV1)>,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"boon.preview-project.v3\0");
    hash_preview_identity_part(&mut hasher, migration_stage.unwrap_or_default().as_bytes());
    let programs = programs.into_iter().collect::<Vec<_>>();
    hash_preview_identity_part(&mut hasher, &(programs.len() as u64).to_be_bytes());
    for (role, application, digest) in programs {
        hash_preview_identity_part(&mut hasher, role.as_str().as_bytes());
        hash_preview_identity_part(&mut hasher, application.package_id.as_bytes());
        hash_preview_identity_part(&mut hasher, application.state_namespace.as_bytes());
        hash_preview_identity_part(&mut hasher, application.deployment_domain.as_bytes());
        hash_preview_identity_part(&mut hasher, digest.as_bytes());
    }
    let migration_stages = migration_stages.into_iter().collect::<Vec<_>>();
    hash_preview_identity_part(&mut hasher, &(migration_stages.len() as u64).to_be_bytes());
    for (stage_id, schema_version, digest) in migration_stages {
        hash_preview_identity_part(&mut hasher, stage_id.as_bytes());
        hash_preview_identity_part(&mut hasher, &schema_version.to_be_bytes());
        hash_preview_identity_part(&mut hasher, digest.as_bytes());
    }
    format!("{:x}", hasher.finalize())
}

fn hash_preview_identity_part(hasher: &mut Sha256, bytes: &[u8]) {
    hasher.update((bytes.len() as u64).to_be_bytes());
    hasher.update(bytes);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn application(namespace: &str) -> ApplicationIdentity {
        ApplicationIdentity::new("dev.boon.test", namespace, "test")
    }

    #[test]
    fn mailbox_is_depth_one_and_latest_wins() {
        let worker = CompileWorker {
            state: Arc::new((Mutex::new(State::default()), Condvar::new())),
            thread: None,
        };
        for job_id in 1..=4 {
            worker.replace(CompileRequest {
                job_id,
                intent: PreviewIntent::Replace,
                request_id: None,
                revision: 7,
                source: PreviewSource::BuiltInSingleRole {
                    application: application(&format!("mailbox-{job_id}")),
                    entry_path: "RUN.bn".to_owned(),
                    units: Vec::new(),
                },
                test_steps: Vec::new(),
                migration: None,
                migration_stage: None,
            });
        }
        assert_eq!(worker.replaced_count(), 3);
        {
            let state = worker.state.0.lock().unwrap();
            assert_eq!(state.pending.as_ref().unwrap().job_id, 4);
            assert_eq!(state.pending.as_ref().unwrap().revision, 7);
        }
    }

    #[test]
    fn child_program_mailbox_is_depth_one_per_session_and_last_arrival_wins() {
        let worker = ProgramCompileWorker {
            state: Arc::new((Mutex::new(ProgramCompileState::default()), Condvar::new())),
            thread: None,
        };
        let host = boon_document_model::DocumentNodeId("program-host".to_owned());
        let session = ProgramSessionId("public-page".to_owned());
        for revision in [4, 1, 3, 2] {
            let receipt = worker.replace(ProgramHostRequest {
                request_id: ProgramRequestId(format!("request-{revision}")),
                session: session.clone(),
                host: host.clone(),
                compile: boon_program_runtime::ProgramCompileRequest {
                    revision,
                    entry_path: "RUN.bn".to_owned(),
                    units: vec![boon_runtime::RuntimeSourceUnit {
                        path: "RUN.bn".to_owned(),
                        source: format!("value: {revision}\n"),
                    }],
                    application: application("child-mailbox"),
                    role: boon_plan::ProgramRole::Client,
                    capability_profile: boon_document_model::ProgramCapabilityProfile::PublicClient,
                },
                artifact_id: None,
                artifact_ownership: None,
            });
            assert!(receipt.accepted);
        }

        {
            let state = worker.state.0.lock().unwrap();
            assert_eq!(state.pending[&session].request.compile.revision, 2);
        }
        for request_id in ["ff", "00"] {
            let receipt = worker.replace(ProgramHostRequest {
                request_id: ProgramRequestId(request_id.to_owned()),
                session: session.clone(),
                host: host.clone(),
                compile: boon_program_runtime::ProgramCompileRequest {
                    revision: 2,
                    entry_path: "RUN.bn".to_owned(),
                    units: vec![boon_runtime::RuntimeSourceUnit {
                        path: "RUN.bn".to_owned(),
                        source: format!("value: {request_id}\n"),
                    }],
                    application: application("child-mailbox"),
                    role: boon_plan::ProgramRole::Client,
                    capability_profile: boon_document_model::ProgramCapabilityProfile::PublicClient,
                },
                artifact_id: None,
                artifact_ownership: None,
            });
            assert!(receipt.accepted);
        }

        assert_eq!(worker.replaced_count(), 5);
        let state = worker.state.0.lock().unwrap();
        assert_eq!(state.pending.len(), 1);
        assert_eq!(state.pending[&session].request.compile.revision, 2);
        assert_eq!(state.pending[&session].request.request_id.0, "00");
        assert_eq!(state.pending[&session].pending_depth, 1);
    }

    #[test]
    fn project_key_partitions_identical_source_by_application_identity() {
        let units = vec![SourceUnit {
            path: "RUN.bn".to_owned(),
            source: "value: 1\n".to_owned(),
        }];
        assert_ne!(
            project_key(&application("first"), "RUN.bn", &units).unwrap(),
            project_key(&application("second"), "RUN.bn", &units).unwrap()
        );
    }

    #[test]
    fn source_key_includes_entry_path_and_normalizes_unit_order_and_separators() {
        let source = "value: 1\n";
        assert_ne!(
            source_key(
                "A.bn",
                &[SourceUnit {
                    path: "A.bn".to_owned(),
                    source: source.to_owned(),
                }],
            )
            .unwrap(),
            source_key(
                "B.bn",
                &[SourceUnit {
                    path: "B.bn".to_owned(),
                    source: source.to_owned(),
                }],
            )
            .unwrap()
        );

        let slash = vec![
            SourceUnit {
                path: "nested/Entry.bn".to_owned(),
                source: source.to_owned(),
            },
            SourceUnit {
                path: "nested/Support.bn".to_owned(),
                source: "FUNCTION helper() {\n    []\n}\n".to_owned(),
            },
        ];
        let mut backslash = slash.clone();
        backslash.reverse();
        for unit in &mut backslash {
            unit.path = unit.path.replace('/', "\\");
        }
        assert_eq!(
            source_key("nested/Entry.bn", &slash).unwrap(),
            source_key("nested\\Entry.bn", &backslash).unwrap()
        );
    }

    #[test]
    fn migration_project_key_covers_ordered_predecessor_source_and_schema() {
        let example = crate::catalog::Catalog::load()
            .unwrap()
            .open("counter_migration")
            .unwrap();
        let migration = example.migration.expect("counter migration bundle");
        let target_index = migration.stages.len() - 1;
        let target = &migration.stages[target_index];
        let target_stage = target.id.clone();
        assert!(target_index > 0, "fixture must have a predecessor stage");
        let source = PreviewSource::BuiltInSingleRole {
            application: example.application,
            entry_path: target.source.clone(),
            units: target.units.clone(),
        };
        let before = preview_project_key(&source, Some(&migration), Some(&target_stage)).unwrap();

        let mut changed_source = migration.clone();
        changed_source.stages[target_index - 1].units[0]
            .source
            .push('\n');
        assert_ne!(
            before,
            preview_project_key(&source, Some(&changed_source), Some(&target_stage)).unwrap()
        );

        let mut changed_schema = migration;
        changed_schema.stages[target_index - 1].schema_version += 1;
        assert_ne!(
            before,
            preview_project_key(&source, Some(&changed_schema), Some(&target_stage)).unwrap()
        );
    }

    #[test]
    fn migration_project_key_rejects_a_working_entrypoint_mismatch() {
        let example = crate::catalog::Catalog::load()
            .unwrap()
            .open("counter_migration")
            .unwrap();
        let migration = example.migration.expect("counter migration bundle");
        let target = migration.stages.last().expect("migration target stage");
        let target_stage = target.id.clone();
        let mut units = target.units.clone();
        units.push(SourceUnit {
            path: "DifferentEntry.bn".to_owned(),
            source: "scene: []\n".to_owned(),
        });
        let source = PreviewSource::BuiltInSingleRole {
            application: example.application,
            entry_path: "DifferentEntry.bn".to_owned(),
            units,
        };
        let error =
            preview_project_key(&source, Some(&migration), Some(&target_stage)).unwrap_err();
        assert!(error.contains("working entrypoint"), "{error}");
        assert!(
            error.contains("differs from declared entrypoint"),
            "{error}"
        );
    }

    #[test]
    fn distributed_project_key_covers_non_client_role_source() {
        let client_units = vec![SourceUnit {
            path: "Client/RUN.bn".to_owned(),
            source: "value: 1\n".to_owned(),
        }];
        let mut programs = vec![
            ProgramSource {
                role: boon_plan::ProgramRole::Client,
                entry_path: "Client/RUN.bn".to_owned(),
                units: client_units.clone(),
                application: application("client"),
            },
            ProgramSource {
                role: boon_plan::ProgramRole::Session,
                entry_path: "Session/RUN.bn".to_owned(),
                units: vec![SourceUnit {
                    path: "Session/RUN.bn".to_owned(),
                    source: "value: 2\n".to_owned(),
                }],
                application: application("session"),
            },
            ProgramSource {
                role: boon_plan::ProgramRole::Server,
                entry_path: "Server/RUN.bn".to_owned(),
                units: vec![SourceUnit {
                    path: "Server/RUN.bn".to_owned(),
                    source: "value: 3\n".to_owned(),
                }],
                application: application("server"),
            },
        ];
        let before = preview_project_key(
            &PreviewSource::DistributedPackage {
                programs: programs.clone(),
            },
            None,
            None,
        )
        .unwrap();
        let mut equivalent = programs.clone();
        equivalent.reverse();
        for program in &mut equivalent {
            program.entry_path = program.entry_path.replace('/', "\\");
            program.units.reverse();
            for unit in &mut program.units {
                unit.path = unit.path.replace('/', "\\");
            }
        }
        assert_eq!(
            before,
            preview_project_key(
                &PreviewSource::DistributedPackage {
                    programs: equivalent,
                },
                None,
                None,
            )
            .unwrap()
        );
        programs[2].units[0].source = "value: 4\n".to_owned();
        let after =
            preview_project_key(&PreviewSource::DistributedPackage { programs }, None, None)
                .unwrap();
        assert_ne!(before, after);
    }

    #[test]
    fn distributed_preview_compiles_one_three_role_executable() {
        const SHARED_PATH: &str = "distributed_fixture/Shared/DistributedContract.bn";
        const CLIENT_PATH: &str = "distributed_fixture/Client/RUN.bn";
        const SESSION_PATH: &str = "distributed_fixture/Session/RUN.bn";
        const SERVER_PATH: &str = "distributed_fixture/Server/RUN.bn";
        let shared = SourceUnit {
            path: SHARED_PATH.to_owned(),
            source: include_str!("../testdata/distributed_fixture/Shared/DistributedContract.bn")
                .to_owned(),
        };
        let client_units = vec![
            shared.clone(),
            SourceUnit {
                path: CLIENT_PATH.to_owned(),
                source: include_str!("../testdata/distributed_fixture/Client/RUN.bn").to_owned(),
            },
        ];
        let client_application = application("distributed-client");
        let programs = vec![
            ProgramSource {
                role: boon_plan::ProgramRole::Client,
                entry_path: CLIENT_PATH.to_owned(),
                units: client_units.clone(),
                application: client_application.clone(),
            },
            ProgramSource {
                role: boon_plan::ProgramRole::Session,
                entry_path: SESSION_PATH.to_owned(),
                units: vec![
                    shared.clone(),
                    SourceUnit {
                        path: SESSION_PATH.to_owned(),
                        source: include_str!("../testdata/distributed_fixture/Session/RUN.bn")
                            .to_owned(),
                    },
                ],
                application: application("distributed-session"),
            },
            ProgramSource {
                role: boon_plan::ProgramRole::Server,
                entry_path: SERVER_PATH.to_owned(),
                units: vec![
                    shared,
                    SourceUnit {
                        path: SERVER_PATH.to_owned(),
                        source: include_str!("../testdata/distributed_fixture/Server/RUN.bn")
                            .to_owned(),
                    },
                ],
                application: application("distributed-server"),
            },
        ];
        let compiled = compile(CompileRequest {
            job_id: 7,
            intent: PreviewIntent::Replace,
            request_id: None,
            revision: 7,
            source: PreviewSource::DistributedPackage { programs },
            test_steps: Vec::new(),
            migration: None,
            migration_stage: None,
        })
        .expect("compile distributed preview");
        let CompiledExecutable::DistributedPackage(bundle) = compiled.executable else {
            panic!("distributed source collapsed to a single-role executable");
        };
        assert_eq!(bundle.artifacts().len(), 3);
        for role in [
            boon_plan::ProgramRole::Client,
            boon_plan::ProgramRole::Session,
            boon_plan::ProgramRole::Server,
        ] {
            assert!(bundle.artifact(role).is_some(), "missing {role:?} artifact");
        }
    }

    #[test]
    fn compile_installs_the_host_application_identity_in_the_machine_plan() {
        let application = application("compile-propagation");
        let compiled = compile(CompileRequest {
            job_id: 1,
            intent: PreviewIntent::Replace,
            request_id: None,
            revision: 1,
            source: PreviewSource::BuiltInSingleRole {
                application: application.clone(),
                entry_path: "examples/minimal.bn".to_owned(),
                units: vec![SourceUnit {
                    path: "examples/minimal.bn".to_owned(),
                    source: include_str!("../../../examples/minimal.bn").to_owned(),
                }],
            },
            test_steps: Vec::new(),
            migration: None,
            migration_stage: None,
        })
        .expect("compile preview with host identity");
        let CompiledExecutable::BuiltInSingleRole(plan) = compiled.executable else {
            panic!("single-role source compiled as a distributed package");
        };
        assert_eq!(plan.application.identity, application);
        assert_eq!(
            plan.persistence.schema_version,
            DEFAULT_PERSISTENCE_SCHEMA_VERSION
        );
    }
}
