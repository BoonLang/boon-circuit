use std::collections::BTreeMap;
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use boon_compiler::{
    CancellationToken, CheckedSourceSyntax, CompileIntent, CompilerProject, CompilerSession,
    CompilerSourceUnit, ProjectId, Revision as CompilerRevision, UnitUpdate,
};
use boon_contract::{CanonicalSourceBundleV1, SourceBundleDigestV1, SourceBundleUnit};
use boon_editor::language::{
    LanguageProjectSnapshot, project_checked_language, project_checked_unit_native_language,
    project_parse_error_language,
};
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
    /// Editor data projected from the exact checked artifact used by this
    /// compile attempt. This remains publishable when verification rejects a
    /// type-invalid revision, but never for a superseded worker generation.
    pub language: Option<LanguageProjectSnapshot>,
    pub result: Result<CompiledPreview, String>,
}

struct CompileAttempt {
    language: Option<LanguageProjectSnapshot>,
    result: Result<CompiledPreview, String>,
}

#[derive(Default)]
struct State {
    generation: u64,
    pending: Option<PendingCompile>,
    active: bool,
    active_cancellation: Option<CancellationToken>,
    closing: bool,
    replaced: u64,
}

struct PendingCompile {
    generation: u64,
    request: CompileRequest,
    cancellation: CancellationToken,
}

pub struct CompileWorker {
    state: Arc<(Mutex<State>, Condvar)>,
    thread: Option<JoinHandle<()>>,
}

#[derive(Default)]
struct PreviewCompiler {
    session: CompilerSession,
    built_in: Option<BuiltInProjectSlot>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct BuiltInProjectConfig {
    entrypoint: String,
    unit_paths: Vec<String>,
    target_profile: TargetProfile,
    program_role: ProgramRole,
    application_identity: ApplicationIdentity,
    schema_version: u64,
    migration_predecessors: Vec<MigrationPredecessorBinding>,
}

struct BuiltInProjectSlot {
    config: BuiltInProjectConfig,
    project: ProjectId,
    revision: CompilerRevision,
    unit_sources: BTreeMap<String, String>,
    language: Option<(CompilerRevision, LanguageProjectSnapshot)>,
    verified_plan: Option<(CompilerRevision, Arc<MachinePlan>)>,
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
        state.generation = state.generation.saturating_add(1);
        if state.pending.is_some() || state.active {
            state.replaced = state.replaced.saturating_add(1);
        }
        if let Some(pending) = state.pending.take() {
            pending.cancellation.cancel();
        }
        if let Some(active) = state.active_cancellation.as_ref() {
            active.cancel();
        }
        let generation = state.generation;
        state.pending = Some(PendingCompile {
            generation,
            request,
            cancellation: CancellationToken::new(),
        });
        wake.notify_one();
    }

    pub fn replaced_count(&self) -> u64 {
        self.state.0.lock().expect("compile worker lock").replaced
    }
}

impl Drop for CompileWorker {
    fn drop(&mut self) {
        let (lock, wake) = &*self.state;
        let mut state = lock.lock().expect("compile worker lock");
        state.closing = true;
        if let Some(pending) = state.pending.as_ref() {
            pending.cancellation.cancel();
        }
        if let Some(active) = state.active_cancellation.as_ref() {
            active.cancel();
        }
        drop(state);
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
    let mut compiler = PreviewCompiler::default();
    loop {
        let pending = {
            let (lock, wake) = &*state;
            let mut state = lock.lock().expect("compile worker lock");
            while state.pending.is_none() && !state.closing {
                state = wake.wait(state).expect("compile worker wait");
            }
            if state.closing {
                return;
            }
            state.active = true;
            let pending = state.pending.take().expect("pending compile request");
            state.active_cancellation = Some(pending.cancellation.clone());
            pending
        };
        let PendingCompile {
            generation,
            request,
            cancellation,
        } = pending;
        let job_id = request.job_id;
        let revision = request.revision;
        let CompileAttempt { language, result } = compiler.compile(request, &cancellation);
        let mut state = state.0.lock().expect("compile worker lock");
        state.active = false;
        state.active_cancellation = None;
        if state.closing {
            return;
        }
        if state.generation != generation {
            continue;
        }
        if output
            .unbounded_send(CompileOutcome {
                job_id,
                revision,
                language,
                result,
            })
            .is_err()
        {
            return;
        }
    }
}

impl PreviewCompiler {
    fn compile(
        &mut self,
        request: CompileRequest,
        cancellation: &CancellationToken,
    ) -> CompileAttempt {
        let mut language = None;
        let result = self.compile_result(request, cancellation, &mut language);
        CompileAttempt { language, result }
    }

    fn compile_result(
        &mut self,
        request: CompileRequest,
        cancellation: &CancellationToken,
        language: &mut Option<LanguageProjectSnapshot>,
    ) -> Result<CompiledPreview, String> {
        let started = Instant::now();
        cancellation_checkpoint(cancellation)?;
        let source_key = preview_project_key(
            &request.source,
            request.migration.as_ref(),
            request.migration_stage.as_deref(),
        )?;
        cancellation_checkpoint(cancellation)?;
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
                        Some(request.revision),
                        language,
                        cancellation,
                    )?,
                    (None, None) => self.compile_built_in(
                        entry_path,
                        units,
                        application,
                        request.revision,
                        language,
                        cancellation,
                    )?,
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
                cancellation_checkpoint(cancellation)?;
                let compiled = compile_distributed_program(programs.clone(), request.revision)
                    .map_err(|error| error.to_string())?;
                cancellation_checkpoint(cancellation)?;
                *language = Some(compiled.client_projection);
                CompiledExecutable::DistributedPackage(compiled.bundle)
            }
        };
        cancellation_checkpoint(cancellation)?;
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

    fn compile_built_in(
        &mut self,
        entry_path: &str,
        units: &[SourceUnit],
        application: &ApplicationIdentity,
        language_revision: u64,
        language: &mut Option<LanguageProjectSnapshot>,
        cancellation: &CancellationToken,
    ) -> Result<Arc<MachinePlan>, String> {
        cancellation_checkpoint(cancellation)?;
        let bundle = canonical_preview_source_bundle(entry_path, units)?;
        let project = CompilerProject::new(
            bundle.entrypoint().to_owned(),
            bundle
                .units()
                .iter()
                .map(|unit| CompilerSourceUnit {
                    path: unit.path().to_owned(),
                    source: unit.source().to_owned(),
                })
                .collect(),
            TargetProfile::SoftwareDefault,
            ProgramRole::Client,
            application.clone(),
        )
        .with_persistence_catalog(DEFAULT_PERSISTENCE_SCHEMA_VERSION, Vec::new());
        let (project, revision) = self.sync_built_in_project(project)?;
        cancellation_checkpoint(cancellation)?;
        let cached_plan = self.built_in.as_ref().and_then(|slot| {
            slot.verified_plan
                .as_ref()
                .filter(|(cached_revision, _)| *cached_revision == revision)
                .map(|(_, plan)| Arc::clone(plan))
        });
        if let Some(mut snapshot) = self.built_in.as_ref().and_then(|slot| {
            slot.language
                .as_ref()
                .filter(|(cached_revision, _)| *cached_revision == revision)
                .map(|(_, snapshot)| snapshot.clone())
        }) {
            snapshot.revision = language_revision;
            *language = Some(snapshot);
        }
        if let (Some(plan), Some(_)) = (cached_plan.as_ref(), language.as_ref()) {
            return Ok(Arc::clone(plan));
        }

        if language.is_none() {
            let projected = {
                let result = match self.session.request(
                    project,
                    revision,
                    CompileIntent::Diagnostics,
                    cancellation,
                ) {
                    Ok(result) => result,
                    Err(error) => {
                        let error_text = error.to_string();
                        if let Some(parse_error) = error.downcast_ref::<boon_parser::ParseError>() {
                            let snapshot = project_parse_error_language(
                                language_revision,
                                entry_path,
                                units,
                                parse_error,
                            )
                            .map_err(|projection| {
                                format!(
                                    "{error_text}; parser diagnostic projection failed: {projection}"
                                )
                            })?;
                            self.cache_built_in_language(project, revision, snapshot.clone())?;
                            *language = Some(snapshot);
                        }
                        return Err(error_text);
                    }
                };
                let checked = result.diagnostics().ok_or_else(|| {
                    "diagnostics compiler request produced no checked source".to_owned()
                })?;
                match &checked.syntax {
                    CheckedSourceSyntax::Assembled(program) => project_checked_language(
                        language_revision,
                        units,
                        program,
                        &checked.output,
                    )?,
                    CheckedSourceSyntax::UnitNative(program) => {
                        project_checked_unit_native_language(
                            language_revision,
                            units,
                            program,
                            &checked.output,
                        )?
                    }
                }
            };
            cancellation_checkpoint(cancellation)?;
            self.cache_built_in_language(project, revision, projected.clone())?;
            *language = Some(projected);
        }

        if let Some(plan) = cached_plan {
            return Ok(plan);
        }
        let plan = {
            let result = self
                .session
                .request(
                    project,
                    revision,
                    CompileIntent::VerifiedPreview,
                    cancellation,
                )
                .map_err(|error| error.to_string())?;
            let plan = result
                .compiled()
                .ok_or_else(|| "verified compiler request produced no MachinePlan".to_owned())?
                .plan
                .shared_plan();
            plan
        };
        cancellation_checkpoint(cancellation)?;
        let slot = self
            .built_in
            .as_mut()
            .ok_or_else(|| "built-in compiler project slot disappeared".to_owned())?;
        if slot.project != project || slot.revision != revision {
            return Err("built-in compiler project changed before publication".to_owned());
        }
        slot.verified_plan = Some((revision, Arc::clone(&plan)));
        Ok(plan)
    }

    fn cache_built_in_language(
        &mut self,
        project: ProjectId,
        revision: CompilerRevision,
        snapshot: LanguageProjectSnapshot,
    ) -> Result<(), String> {
        let slot = self
            .built_in
            .as_mut()
            .ok_or_else(|| "built-in compiler project slot disappeared".to_owned())?;
        if slot.project != project || slot.revision != revision {
            return Err("built-in compiler project changed before language publication".to_owned());
        }
        slot.language = Some((revision, snapshot));
        Ok(())
    }

    fn sync_built_in_project(
        &mut self,
        project: CompilerProject,
    ) -> Result<(ProjectId, CompilerRevision), String> {
        let config = BuiltInProjectConfig {
            entrypoint: project.entrypoint.clone(),
            unit_paths: project.units.iter().map(|unit| unit.path.clone()).collect(),
            target_profile: project.target_profile,
            program_role: project.program_role,
            application_identity: project.application_identity.clone(),
            schema_version: project.schema_version,
            migration_predecessors: project.migration_predecessors.clone(),
        };
        let unit_sources = project
            .units
            .iter()
            .map(|unit| (unit.path.clone(), unit.source.clone()))
            .collect::<BTreeMap<_, _>>();
        let reuse = self
            .built_in
            .as_ref()
            .is_some_and(|slot| slot.config == config);
        if !reuse {
            if let Some(replaced) = self.built_in.take() {
                self.session
                    .close_project(replaced.project)
                    .map_err(|error| error.to_string())?;
            }
            let project_id = self
                .session
                .open_project(project)
                .map_err(|error| error.to_string())?;
            let revision = self
                .session
                .revision(project_id)
                .map_err(|error| error.to_string())?;
            self.built_in = Some(BuiltInProjectSlot {
                config,
                project: project_id,
                revision,
                unit_sources,
                language: None,
                verified_plan: None,
            });
            return Ok((project_id, revision));
        }

        let slot = self
            .built_in
            .as_ref()
            .ok_or_else(|| "reusable built-in compiler project is absent".to_owned())?;
        let project_id = slot.project;
        let updates = unit_sources
            .iter()
            .filter(|(path, source)| {
                slot.unit_sources
                    .get(path.as_str())
                    .is_none_or(|current| current != *source)
            })
            .map(|(path, source)| UnitUpdate::new(path.clone(), source.clone()))
            .collect::<Vec<_>>();
        let revision = self
            .session
            .apply_updates(project_id, updates)
            .map_err(|error| error.to_string())?;
        let slot = self
            .built_in
            .as_mut()
            .ok_or_else(|| "reusable built-in compiler project is absent".to_owned())?;
        if revision != slot.revision {
            slot.language = None;
            slot.verified_plan = None;
        }
        slot.revision = revision;
        slot.unit_sources = unit_sources;
        Ok((project_id, revision))
    }
}

#[cfg(test)]
fn compile(
    request: CompileRequest,
    cancellation: &CancellationToken,
) -> Result<CompiledPreview, String> {
    PreviewCompiler::default()
        .compile(request, cancellation)
        .result
}

pub fn compile_migration_stage(
    application: &ApplicationIdentity,
    migration: &MigrationBundle,
    target_stage: &str,
) -> Result<Arc<MachinePlan>, String> {
    let mut language = None;
    compile_migration_stage_with_units(
        application,
        migration,
        target_stage,
        None,
        None,
        &mut language,
        &CancellationToken::new(),
    )
}

fn compile_migration_stage_with_units(
    application: &ApplicationIdentity,
    migration: &MigrationBundle,
    target_stage: &str,
    target_source: Option<(&str, &[SourceUnit])>,
    language_revision: Option<u64>,
    language: &mut Option<LanguageProjectSnapshot>,
    cancellation: &CancellationToken,
) -> Result<Arc<MachinePlan>, String> {
    if migration.stage(target_stage).is_none() {
        return Err(format!("migration stage `{target_stage}` is absent"));
    }
    let mut predecessor = None::<MigrationPredecessorBinding>;
    for stage in &migration.stages {
        cancellation_checkpoint(cancellation)?;
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
        let plan = compile_source_bundle(
            entry_path,
            units,
            TargetProfile::SoftwareDefault,
            ProgramRole::Client,
            application.clone(),
            stage.schema_version,
            predecessor.iter().cloned().collect(),
            CompileIntent::VerifiedPreview,
            (stage.id == target_stage)
                .then_some(language_revision)
                .flatten(),
            language,
            cancellation,
        )
        .map_err(|error| format!("migration stage `{}` failed to compile: {error}", stage.id))?;
        if stage.id == target_stage {
            return Ok(plan);
        }
        predecessor = Some(MigrationPredecessorBinding::from_machine_plan(&plan));
    }
    Err(format!("migration stage `{target_stage}` is absent"))
}

#[allow(clippy::too_many_arguments)]
fn compile_source_bundle(
    entry_path: String,
    units: Vec<CompilerSourceUnit>,
    target_profile: TargetProfile,
    program_role: ProgramRole,
    application_identity: ApplicationIdentity,
    schema_version: u64,
    migration_predecessors: Vec<MigrationPredecessorBinding>,
    intent: CompileIntent,
    language_revision: Option<u64>,
    language: &mut Option<LanguageProjectSnapshot>,
    cancellation: &CancellationToken,
) -> Result<Arc<MachinePlan>, String> {
    cancellation_checkpoint(cancellation)?;
    let editor_projection_source = language_revision.map(|_| {
        (
            entry_path.clone(),
            units
                .iter()
                .map(|unit| SourceUnit {
                    path: unit.path.clone(),
                    source: unit.source.clone(),
                })
                .collect::<Vec<_>>(),
        )
    });
    let project = CompilerProject::new(
        entry_path,
        units,
        target_profile,
        program_role,
        application_identity,
    )
    .with_persistence_catalog(schema_version, migration_predecessors);
    let mut session = CompilerSession::new();
    let project = session
        .open_project(project)
        .map_err(|error| error.to_string())?;
    let revision = session
        .revision(project)
        .map_err(|error| error.to_string())?;
    if let Some(language_revision) = language_revision {
        let (editor_entrypoint, editor_units) = editor_projection_source
            .as_ref()
            .ok_or_else(|| "migration editor projection source disappeared".to_owned())?;
        let diagnostics = match session.request(
            project,
            revision,
            CompileIntent::Diagnostics,
            cancellation,
        ) {
            Ok(result) => result,
            Err(error) => {
                let error_text = error.to_string();
                if let Some(parse_error) = error.downcast_ref::<boon_parser::ParseError>() {
                    *language = Some(
                        project_parse_error_language(
                            language_revision,
                            editor_entrypoint,
                            editor_units,
                            parse_error,
                        )
                        .map_err(|projection| {
                            format!(
                                "{error_text}; migration parser diagnostic projection failed: {projection}"
                            )
                        })?,
                    );
                }
                return Err(error_text);
            }
        };
        let checked = diagnostics
            .diagnostics()
            .ok_or_else(|| "migration diagnostics request produced no checked source".to_owned())?;
        *language = Some(match &checked.syntax {
            CheckedSourceSyntax::Assembled(program) => {
                project_checked_language(language_revision, editor_units, program, &checked.output)?
            }
            CheckedSourceSyntax::UnitNative(program) => project_checked_unit_native_language(
                language_revision,
                editor_units,
                program,
                &checked.output,
            )?,
        });
        cancellation_checkpoint(cancellation)?;
    }
    let result = session
        .request(project, revision, intent, cancellation)
        .map_err(|error| error.to_string())?;
    let plan = result
        .compiled()
        .ok_or_else(|| "verified compiler request produced no MachinePlan".to_owned())?
        .plan
        .shared_plan();
    cancellation_checkpoint(cancellation)?;
    Ok(plan)
}

fn cancellation_checkpoint(cancellation: &CancellationToken) -> Result<(), String> {
    if cancellation.is_canceled() {
        Err("compiler request canceled".to_owned())
    } else {
        Ok(())
    }
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

    fn mailbox_request(job_id: u64) -> CompileRequest {
        CompileRequest {
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
        }
    }

    fn built_in_project(
        application: ApplicationIdentity,
        units: Vec<CompilerSourceUnit>,
    ) -> CompilerProject {
        CompilerProject::new(
            "RUN.bn",
            units,
            TargetProfile::SoftwareDefault,
            ProgramRole::Client,
            application,
        )
        .with_persistence_catalog(DEFAULT_PERSISTENCE_SCHEMA_VERSION, Vec::new())
    }

    #[test]
    fn built_in_project_slot_reuses_updates_and_closes_replacements() {
        let mut compiler = PreviewCompiler::default();
        let first_application = application("persistent-slot");
        let (first_project, first_revision) = compiler
            .sync_built_in_project(built_in_project(
                first_application.clone(),
                vec![CompilerSourceUnit {
                    path: "RUN.bn".to_owned(),
                    source: "value: 1".to_owned(),
                }],
            ))
            .unwrap();
        assert_eq!(first_revision, CompilerRevision(0));

        let (same_project, unchanged_revision) = compiler
            .sync_built_in_project(built_in_project(
                first_application.clone(),
                vec![CompilerSourceUnit {
                    path: "RUN.bn".to_owned(),
                    source: "value: 1".to_owned(),
                }],
            ))
            .unwrap();
        assert_eq!(same_project, first_project);
        assert_eq!(unchanged_revision, first_revision);

        let (updated_project, updated_revision) = compiler
            .sync_built_in_project(built_in_project(
                first_application.clone(),
                vec![CompilerSourceUnit {
                    path: "RUN.bn".to_owned(),
                    source: "value: 2".to_owned(),
                }],
            ))
            .unwrap();
        assert_eq!(updated_project, first_project);
        assert_eq!(updated_revision, CompilerRevision(1));

        let (topology_replacement, topology_revision) = compiler
            .sync_built_in_project(built_in_project(
                first_application,
                vec![
                    CompilerSourceUnit {
                        path: "RUN.bn".to_owned(),
                        source: "value: 2".to_owned(),
                    },
                    CompilerSourceUnit {
                        path: "Shared.bn".to_owned(),
                        source: "shared: 1".to_owned(),
                    },
                ],
            ))
            .unwrap();
        assert_ne!(topology_replacement, first_project);
        assert_eq!(topology_revision, CompilerRevision(0));
        assert!(compiler.session.revision(first_project).is_err());

        let (config_replacement, replacement_revision) = compiler
            .sync_built_in_project(built_in_project(
                application("replacement-slot"),
                vec![
                    CompilerSourceUnit {
                        path: "RUN.bn".to_owned(),
                        source: "value: 2".to_owned(),
                    },
                    CompilerSourceUnit {
                        path: "Shared.bn".to_owned(),
                        source: "shared: 1".to_owned(),
                    },
                ],
            ))
            .unwrap();
        assert_ne!(config_replacement, topology_replacement);
        assert_eq!(replacement_revision, CompilerRevision(0));
        assert!(compiler.session.revision(topology_replacement).is_err());
    }

    #[test]
    fn unchanged_built_in_bytes_reuse_the_verified_plan_arc() {
        let mut compiler = PreviewCompiler::default();
        let application = application("verified-plan-reuse");
        let units = vec![SourceUnit {
            path: "examples/minimal.bn".to_owned(),
            source: include_str!("../../../examples/minimal.bn").to_owned(),
        }];
        let cancellation = CancellationToken::new();
        let mut first_language = None;
        let first = compiler
            .compile_built_in(
                "examples/minimal.bn",
                &units,
                &application,
                7,
                &mut first_language,
                &cancellation,
            )
            .unwrap();
        let mut second_language = None;
        let second = compiler
            .compile_built_in(
                "examples/minimal.bn",
                &units,
                &application,
                8,
                &mut second_language,
                &cancellation,
            )
            .unwrap();
        assert!(Arc::ptr_eq(&first, &second));
        assert_eq!(
            first_language.as_ref().map(|snapshot| snapshot.revision),
            Some(7)
        );
        assert_eq!(
            second_language.as_ref().map(|snapshot| snapshot.revision),
            Some(8)
        );
    }

    #[test]
    fn parser_failure_still_publishes_exact_revision_language_diagnostics() {
        let request = CompileRequest {
            job_id: 11,
            intent: PreviewIntent::Replace,
            request_id: None,
            revision: 19,
            source: PreviewSource::BuiltInSingleRole {
                application: application("parse-error-language"),
                entry_path: "RUN.bn".to_owned(),
                units: vec![SourceUnit {
                    path: "RUN.bn".to_owned(),
                    source: "value: [".to_owned(),
                }],
            },
            test_steps: Vec::new(),
            migration: None,
            migration_stage: None,
        };
        let attempt = PreviewCompiler::default().compile(request, &CancellationToken::new());
        assert!(attempt.result.is_err());
        let language = attempt.language.expect("parse diagnostic projection");
        assert_eq!(language.revision, 19);
        assert_eq!(language.source_bundle_digest_v1.to_string().len(), 64);
        assert_eq!(language.diagnostics.len(), 1);
        assert_eq!(language.diagnostics[0].location.path, "RUN.bn");
    }

    #[test]
    fn mailbox_is_depth_one_and_latest_wins() {
        let worker = CompileWorker {
            state: Arc::new((Mutex::new(State::default()), Condvar::new())),
            thread: None,
        };
        for job_id in 1..=4 {
            let superseded = worker
                .state
                .0
                .lock()
                .unwrap()
                .pending
                .as_ref()
                .map(|pending| pending.cancellation.clone());
            worker.replace(mailbox_request(job_id));
            if let Some(superseded) = superseded {
                assert!(superseded.is_canceled());
            }
        }
        assert_eq!(worker.replaced_count(), 3);
        {
            let state = worker.state.0.lock().unwrap();
            assert_eq!(state.pending.as_ref().unwrap().request.job_id, 4);
            assert_eq!(state.pending.as_ref().unwrap().request.revision, 7);
        }
    }

    #[test]
    fn replacing_an_active_compile_cancels_its_generation() {
        let active = CancellationToken::new();
        let worker = CompileWorker {
            state: Arc::new((
                Mutex::new(State {
                    active: true,
                    active_cancellation: Some(active.clone()),
                    ..State::default()
                }),
                Condvar::new(),
            )),
            thread: None,
        };
        worker.replace(mailbox_request(2));
        assert!(active.is_canceled());
        assert_eq!(worker.replaced_count(), 1);
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
        let compiled = compile(
            CompileRequest {
                job_id: 7,
                intent: PreviewIntent::Replace,
                request_id: None,
                revision: 7,
                source: PreviewSource::DistributedPackage { programs },
                test_steps: Vec::new(),
                migration: None,
                migration_stage: None,
            },
            &CancellationToken::new(),
        )
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
        let compiled = compile(
            CompileRequest {
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
            },
            &CancellationToken::new(),
        )
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
