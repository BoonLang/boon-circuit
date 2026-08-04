use crate::{
    CheckedCompileRequest, CheckedSourceFromSource, CompiledSealedMachinePlanFromSource,
    CompilerResult, CompilerSourceUnit, check_diagnostics_project_syntax_source,
    check_runtime_project_syntax_source, finish_checked_machine_plan_with_cancellation,
};
use boon_compilation_db::{
    RequestEvaluationStats, RequestFingerprint, Revision as EvaluationRevision,
    TypedRequestEvaluator,
};
use boon_parser::{
    ParseWorkCounters, ParsedSourceUnit, ProjectSyntaxSnapshot, ProjectUnitLinkKey,
    UnitSyntaxSnapshot, link_project_source_unit_profiled, parse_project_source_unit_profiled,
    project_unit_link_keys,
};
use boon_plan::{
    ApplicationIdentity, MigrationPredecessorBinding, PlanError, ProgramRole, TargetProfile,
};
use boon_syntax::SourceUnitId;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
#[cfg(not(target_arch = "wasm32"))]
use std::time::Instant;
#[cfg(target_arch = "wasm32")]
use web_time::Instant;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct ProjectId(pub u64);

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Revision(pub u64);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompileIntent {
    Diagnostics,
    VerifiedCheck,
    VerifiedPreview,
    Handoff,
}

#[derive(Clone, Debug)]
pub struct CompilerProject {
    pub entrypoint: String,
    pub units: Vec<CompilerSourceUnit>,
    pub target_profile: TargetProfile,
    pub program_role: ProgramRole,
    pub application_identity: ApplicationIdentity,
    pub schema_version: u64,
    pub migration_predecessors: Vec<MigrationPredecessorBinding>,
}

impl CompilerProject {
    pub fn new(
        entrypoint: impl Into<String>,
        units: Vec<CompilerSourceUnit>,
        target_profile: TargetProfile,
        program_role: ProgramRole,
        application_identity: ApplicationIdentity,
    ) -> Self {
        Self {
            entrypoint: entrypoint.into(),
            units,
            target_profile,
            program_role,
            application_identity,
            schema_version: boon_plan::DEFAULT_PERSISTENCE_SCHEMA_VERSION,
            migration_predecessors: Vec::new(),
        }
    }

    pub fn with_persistence_catalog(
        mut self,
        schema_version: u64,
        migration_predecessors: Vec<MigrationPredecessorBinding>,
    ) -> Self {
        self.schema_version = schema_version;
        self.migration_predecessors = migration_predecessors;
        self
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnitUpdate {
    pub path: String,
    pub source: String,
}

impl UnitUpdate {
    pub fn new(path: impl Into<String>, source: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            source: source.into(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum UnitChange {
    Upsert(UnitUpdate),
    Remove { path: String },
    Rename { from: String, to: String },
}

impl UnitChange {
    pub fn upsert(path: impl Into<String>, source: impl Into<String>) -> Self {
        Self::Upsert(UnitUpdate::new(path, source))
    }

    pub fn remove(path: impl Into<String>) -> Self {
        Self::Remove { path: path.into() }
    }

    pub fn rename(from: impl Into<String>, to: impl Into<String>) -> Self {
        Self::Rename {
            from: from.into(),
            to: to.into(),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct CancellationToken {
    canceled: Arc<AtomicBool>,
}

impl CancellationToken {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.canceled.store(true, Ordering::Release);
    }

    pub fn is_canceled(&self) -> bool {
        self.canceled.load(Ordering::Acquire)
    }
}

pub enum CompilerSessionResult<'a> {
    Diagnostics(&'a CheckedSourceFromSource),
    Verified {
        intent: CompileIntent,
        compiled: &'a CompiledSealedMachinePlanFromSource,
    },
}

impl CompilerSessionResult<'_> {
    pub fn diagnostics(&self) -> Option<&CheckedSourceFromSource> {
        match self {
            Self::Diagnostics(checked) => Some(checked),
            Self::Verified { .. } => None,
        }
    }

    pub fn compiled(&self) -> Option<&CompiledSealedMachinePlanFromSource> {
        match self {
            Self::Diagnostics(_) => None,
            Self::Verified { compiled, .. } => Some(compiled),
        }
    }
}

#[derive(Default)]
pub struct CompilerSession {
    next_project: u64,
    projects: BTreeMap<ProjectId, ProjectState>,
}

struct ProjectState {
    source: CompilerProject,
    revision: Revision,
    syntax_requests: TypedRequestEvaluator<SessionSyntaxRequestKey, SessionSyntaxRequestValue>,
    checked: Option<CheckedSourceFromSource>,
    compiled: Option<(Revision, CompiledSealedMachinePlanFromSource)>,
    request_graph: Option<(
        Revision,
        Arc<boon_compilation_db::SealedRequestGraphSnapshot>,
    )>,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum SessionSyntaxRequestKey {
    Parse(SourceUnitId),
    Link(SourceUnitId),
}

impl SessionSyntaxRequestKey {
    fn source_unit_id(&self) -> &SourceUnitId {
        match self {
            Self::Parse(source_unit_id) | Self::Link(source_unit_id) => source_unit_id,
        }
    }
}

enum SessionSyntaxRequestValue {
    Parsed(Arc<ParsedSourceUnit>),
    Linked(Arc<UnitSyntaxSnapshot>),
}

impl SessionSyntaxRequestValue {
    fn parsed(&self) -> Option<&Arc<ParsedSourceUnit>> {
        match self {
            Self::Parsed(parsed) => Some(parsed),
            Self::Linked(_) => None,
        }
    }

    fn linked(&self) -> Option<&Arc<UnitSyntaxSnapshot>> {
        match self {
            Self::Parsed(_) => None,
            Self::Linked(linked) => Some(linked),
        }
    }
}

impl CompilerSession {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn open_project(&mut self, project: CompilerProject) -> CompilerResult<ProjectId> {
        validate_project(&project)?;
        self.next_project = self.next_project.saturating_add(1);
        let id = ProjectId(self.next_project);
        self.projects.insert(
            id,
            ProjectState {
                source: project,
                revision: Revision(0),
                syntax_requests: TypedRequestEvaluator::new(EvaluationRevision(0)),
                checked: None,
                compiled: None,
                request_graph: None,
            },
        );
        Ok(id)
    }

    pub fn revision(&self, project: ProjectId) -> CompilerResult<Revision> {
        self.projects
            .get(&project)
            .map(|state| state.revision)
            .ok_or_else(|| session_error(format!("unknown compiler project {}", project.0)))
    }

    pub fn apply_update(
        &mut self,
        project: ProjectId,
        update: UnitUpdate,
    ) -> CompilerResult<Revision> {
        self.apply_updates(project, [update])
    }

    /// Atomically installs one source snapshot delta and advances the project
    /// revision at most once. Validation completes before any unit changes, so
    /// a malformed batch cannot leave the compiler database at a partial
    /// revision.
    pub fn apply_updates(
        &mut self,
        project: ProjectId,
        updates: impl IntoIterator<Item = UnitUpdate>,
    ) -> CompilerResult<Revision> {
        let state = self
            .projects
            .get_mut(&project)
            .ok_or_else(|| session_error(format!("unknown compiler project {}", project.0)))?;
        let updates = updates.into_iter().collect::<Vec<_>>();
        let mut update_paths = BTreeSet::new();
        for update in &updates {
            if update.path.is_empty() || !update_paths.insert(update.path.as_str()) {
                return Err(session_error(format!(
                    "compiler project {} update has an empty or duplicate source path `{}`",
                    project.0, update.path
                )));
            }
            if !state
                .source
                .units
                .iter()
                .any(|unit| unit.path == update.path)
            {
                return Err(session_error(format!(
                    "compiler project {} has no source unit `{}`",
                    project.0, update.path
                )));
            }
        }
        let changed = updates.iter().any(|update| {
            state
                .source
                .units
                .iter()
                .find(|unit| unit.path == update.path)
                .is_some_and(|unit| unit.source != update.source)
        });
        if !changed {
            return Ok(state.revision);
        }
        let next_revision = state.revision.0.checked_add(1).ok_or_else(|| {
            session_error(format!("compiler project {} revision overflow", project.0))
        })?;
        state
            .syntax_requests
            .advance_to(EvaluationRevision(next_revision))?;
        for update in updates {
            let unit = state
                .source
                .units
                .iter_mut()
                .find(|unit| unit.path == update.path)
                .ok_or_else(|| {
                    session_error(format!(
                        "compiler project {} has no source unit `{}`",
                        project.0, update.path
                    ))
                })?;
            if unit.source != update.source {
                unit.source = update.source;
            }
        }
        state.revision = Revision(next_revision);
        state.checked = None;
        // Keep the last verified artifact alive while the replacement revision
        // checks and verifies. Invalid or canceled source must not blank a
        // running preview; only a successful current-revision request replaces
        // this slot.
        Ok(state.revision)
    }

    /// Atomically applies source-unit topology and content changes.
    ///
    /// Upsert may add or replace a unit. Rename deliberately creates a new
    /// [`SourceUnitId`], and renaming the entrypoint moves the entrypoint with
    /// it. The complete candidate project validates before the live revision or
    /// retained syntax snapshot changes.
    pub fn apply_unit_changes(
        &mut self,
        project: ProjectId,
        changes: impl IntoIterator<Item = UnitChange>,
    ) -> CompilerResult<Revision> {
        let state = self
            .projects
            .get_mut(&project)
            .ok_or_else(|| session_error(format!("unknown compiler project {}", project.0)))?;
        let changes = changes.into_iter().collect::<Vec<_>>();
        if changes.is_empty() {
            return Ok(state.revision);
        }

        let mut candidate = state.source.clone();
        for change in changes {
            match change {
                UnitChange::Upsert(update) => {
                    if update.path.is_empty() {
                        return Err(session_error(format!(
                            "compiler project {} upsert has an empty source path",
                            project.0
                        )));
                    }
                    if let Some(unit) = candidate
                        .units
                        .iter_mut()
                        .find(|unit| unit.path == update.path)
                    {
                        unit.source = update.source;
                    } else {
                        candidate.units.push(CompilerSourceUnit {
                            path: update.path,
                            source: update.source,
                        });
                    }
                }
                UnitChange::Remove { path } => {
                    let index = candidate
                        .units
                        .iter()
                        .position(|unit| unit.path == path)
                        .ok_or_else(|| {
                            session_error(format!(
                                "compiler project {} has no source unit `{path}`",
                                project.0
                            ))
                        })?;
                    candidate.units.remove(index);
                }
                UnitChange::Rename { from, to } => {
                    if from.is_empty() || to.is_empty() {
                        return Err(session_error(format!(
                            "compiler project {} rename has an empty source path",
                            project.0
                        )));
                    }
                    if from != to && candidate.units.iter().any(|unit| unit.path == to) {
                        return Err(session_error(format!(
                            "compiler project {} already has source unit `{to}`",
                            project.0
                        )));
                    }
                    let unit = candidate
                        .units
                        .iter_mut()
                        .find(|unit| unit.path == from)
                        .ok_or_else(|| {
                            session_error(format!(
                                "compiler project {} has no source unit `{from}`",
                                project.0
                            ))
                        })?;
                    unit.path = to.clone();
                    if candidate.entrypoint == from {
                        candidate.entrypoint = to;
                    }
                }
            }
        }
        validate_project(&candidate)?;
        if candidate.entrypoint == state.source.entrypoint && candidate.units == state.source.units
        {
            return Ok(state.revision);
        }

        let mut surviving_sources = BTreeSet::new();
        for unit in &candidate.units {
            let source_unit_id = SourceUnitId::from_path(&unit.path).map_err(|error| {
                session_error(format!(
                    "compiler project {} has invalid source unit `{}`: {error}",
                    project.0, unit.path
                ))
            })?;
            surviving_sources.insert(source_unit_id);
        }
        let next_revision = state.revision.0.checked_add(1).ok_or_else(|| {
            session_error(format!("compiler project {} revision overflow", project.0))
        })?;
        state
            .syntax_requests
            .advance_to(EvaluationRevision(next_revision))?;
        state
            .syntax_requests
            .retain_requests(|key| surviving_sources.contains(key.source_unit_id()));
        state.source = candidate;
        state.revision = Revision(next_revision);
        state.checked = None;
        Ok(state.revision)
    }

    pub fn close_project(&mut self, project: ProjectId) -> CompilerResult<()> {
        self.projects
            .remove(&project)
            .map(|_| ())
            .ok_or_else(|| session_error(format!("unknown compiler project {}", project.0)))
    }

    pub fn last_verified(
        &self,
        project: ProjectId,
    ) -> CompilerResult<Option<(Revision, &CompiledSealedMachinePlanFromSource)>> {
        let state = self
            .projects
            .get(&project)
            .ok_or_else(|| session_error(format!("unknown compiler project {}", project.0)))?;
        Ok(state
            .compiled
            .as_ref()
            .map(|(revision, compiled)| (*revision, compiled)))
    }

    pub fn request_graph_snapshot(
        &self,
        project: ProjectId,
    ) -> CompilerResult<
        Option<(
            Revision,
            Arc<boon_compilation_db::SealedRequestGraphSnapshot>,
        )>,
    > {
        let state = self
            .projects
            .get(&project)
            .ok_or_else(|| session_error(format!("unknown compiler project {}", project.0)))?;
        Ok(state
            .request_graph
            .as_ref()
            .map(|(revision, graph)| (*revision, Arc::clone(graph))))
    }

    /// Cumulative typed parser/link request work for this session project.
    pub fn syntax_request_stats(
        &self,
        project: ProjectId,
    ) -> CompilerResult<RequestEvaluationStats> {
        let state = self
            .projects
            .get(&project)
            .ok_or_else(|| session_error(format!("unknown compiler project {}", project.0)))?;
        Ok(state.syntax_requests.stats())
    }

    /// Returns the retained context-independent syntax artifact for one unit,
    /// if that unit has been parsed by a request in this session.
    pub fn unit_syntax_snapshot(
        &self,
        project: ProjectId,
        path: &str,
    ) -> CompilerResult<Option<Arc<UnitSyntaxSnapshot>>> {
        let state = self
            .projects
            .get(&project)
            .ok_or_else(|| session_error(format!("unknown compiler project {}", project.0)))?;
        let source_unit_id = SourceUnitId::from_path(path)
            .map_err(|error| session_error(format!("invalid source unit `{path}`: {error}")))?;
        Ok(state
            .syntax_requests
            .current_value(&SessionSyntaxRequestKey::Link(source_unit_id))
            .and_then(SessionSyntaxRequestValue::linked)
            .map(Arc::clone))
    }

    pub fn request<'a>(
        &'a mut self,
        project: ProjectId,
        revision: Revision,
        intent: CompileIntent,
        cancellation: &CancellationToken,
    ) -> CompilerResult<CompilerSessionResult<'a>> {
        if cancellation.is_canceled() {
            return Err(canceled_error());
        }
        let state = self
            .projects
            .get_mut(&project)
            .ok_or_else(|| session_error(format!("unknown compiler project {}", project.0)))?;
        if state.revision != revision {
            return Err(session_error(format!(
                "compiler project {} is at revision {}, not requested revision {}",
                project.0, state.revision.0, revision.0
            )));
        }
        if intent == CompileIntent::Diagnostics {
            if state.checked.is_none() {
                let (parsed, parse_work, parse_ms) = parse_project_snapshot(state)?;
                state.checked = Some(check_diagnostics_project_syntax_source(
                    parsed,
                    parse_work,
                    parse_ms,
                    state.source.program_role,
                )?);
            }
            if cancellation.is_canceled() {
                state.checked = None;
                return Err(canceled_error());
            }
            return Ok(CompilerSessionResult::Diagnostics(
                state.checked.as_ref().expect("checked project"),
            ));
        }

        let current_artifact_available = state
            .compiled
            .as_ref()
            .is_some_and(|(compiled_revision, _)| *compiled_revision == revision);
        if !current_artifact_available {
            if state.checked.is_none() {
                let (parsed, parse_work, parse_ms) = parse_project_snapshot(state)?;
                state.checked = Some(check_runtime_project_syntax_source(
                    parsed,
                    parse_work,
                    parse_ms,
                    state.source.program_role,
                )?);
            }
            if cancellation.is_canceled() {
                state.checked = None;
                return Err(canceled_error());
            }
            if state
                .checked
                .as_ref()
                .is_some_and(|checked| checked.output.report.has_errors())
            {
                return Err(session_error(
                    "verified compiler request cannot emit an artifact from checked errors",
                ));
            }
            let checked = state.checked.take().expect("checked project");
            let compiled = finish_checked_machine_plan_with_cancellation(
                checked,
                CheckedCompileRequest::new(
                    state.source.target_profile,
                    state.source.program_role,
                    state.source.application_identity.clone(),
                )
                .with_persistence_catalog(
                    state.source.schema_version,
                    &state.source.migration_predecessors,
                ),
                Some(cancellation),
            )?;
            if cancellation.is_canceled() {
                return Err(canceled_error());
            }
            let request_graph = compiled.request_graph_snapshot();
            let compiled = compiled.seal()?;
            if cancellation.is_canceled() {
                return Err(canceled_error());
            }
            state.request_graph = Some((revision, request_graph));
            state.compiled = Some((revision, compiled));
        }
        Ok(CompilerSessionResult::Verified {
            intent,
            compiled: &state.compiled.as_ref().expect("compiled project").1,
        })
    }
}

fn parse_project_snapshot(
    state: &mut ProjectState,
) -> CompilerResult<(ProjectSyntaxSnapshot, ParseWorkCounters, f64)> {
    let started = Instant::now();
    let mut work = ParseWorkCounters::default();
    let mut parsed_units = Vec::with_capacity(state.source.units.len());

    for unit in &state.source.units {
        let source_unit_id = SourceUnitId::from_path(&unit.path).map_err(|error| {
            session_error(format!(
                "compiler project has invalid source unit `{}`: {error}",
                unit.path
            ))
        })?;
        let request_key = SessionSyntaxRequestKey::Parse(source_unit_id);
        let input_fingerprint = source_unit_request_fingerprint(&unit.path, &unit.source);
        let mut executed_work = None;
        let evaluation = state
            .syntax_requests
            .evaluate::<Box<dyn std::error::Error>>(
                request_key.clone(),
                input_fingerprint,
                std::iter::empty(),
                || {
                    let (parsed, profile) =
                        parse_project_source_unit_profiled(unit.path.clone(), unit.source.clone());
                    executed_work = Some(profile.work_counters);
                    Ok((
                        SessionSyntaxRequestValue::Parsed(Arc::new(parsed?)),
                        input_fingerprint,
                    ))
                },
            )?;
        if evaluation.was_reused() {
            work.record_reused_source_units(1);
        } else {
            work.accumulate(
                executed_work.expect("executed parser request publishes exact work counters"),
            );
        }
        let parsed = Arc::clone(
            evaluation
                .value()
                .parsed()
                .expect("parse request publishes parsed syntax"),
        );
        let result_fingerprint = state
            .syntax_requests
            .memo(&request_key)
            .expect("parse request publishes memo currentness")
            .result_fingerprint;
        parsed_units.push((parsed, request_key, result_fingerprint));
    }

    let link_keys = project_unit_link_keys(
        &state.source.entrypoint,
        parsed_units
            .iter()
            .map(|(unit, _, _)| (unit.source_unit_id.clone(), unit.declared_functions.clone())),
    )?;
    let mut linked_units = Vec::with_capacity(parsed_units.len());
    for (parsed, parse_request, parse_result_fingerprint) in parsed_units {
        let source_unit_id = parsed.source_unit_id.clone();
        let link_key = link_keys.get(&source_unit_id).cloned().ok_or_else(|| {
            session_error(format!(
                "compiler project source unit `{}` has no project link context",
                parsed.path
            ))
        })?;
        let request_key = SessionSyntaxRequestKey::Link(source_unit_id);
        let input_fingerprint = project_link_request_fingerprint(&link_key);
        let result_fingerprint = dependent_request_fingerprint(
            b"boon.compiler-session.unit-link-result.v1\0",
            parse_result_fingerprint,
            input_fingerprint,
        );
        let mut executed_work = None;
        let evaluation = state
            .syntax_requests
            .evaluate::<Box<dyn std::error::Error>>(
                request_key,
                input_fingerprint,
                [parse_request],
                || {
                    let (linked, profile) = link_project_source_unit_profiled(
                        parsed.as_ref().clone(),
                        link_key.clone(),
                    );
                    executed_work = Some(profile.work_counters);
                    Ok((
                        SessionSyntaxRequestValue::Linked(Arc::new(linked?)),
                        result_fingerprint,
                    ))
                },
            )?;
        if !evaluation.was_reused() {
            work.accumulate(
                executed_work.expect("executed link request publishes exact work counters"),
            );
        }
        let linked = Arc::clone(
            evaluation
                .value()
                .linked()
                .expect("link request publishes linked unit syntax"),
        );
        linked_units.push(linked);
    }

    let project =
        ProjectSyntaxSnapshot::from_unit_snapshots(&state.source.entrypoint, linked_units)?;
    Ok((project, work, started.elapsed().as_secs_f64() * 1_000.0))
}

fn source_unit_request_fingerprint(path: &str, source: &str) -> RequestFingerprint {
    request_fingerprint(
        b"boon.compiler-session.source-unit-parse.v1\0",
        [path.as_bytes(), source.as_bytes()],
    )
}

fn project_link_request_fingerprint(link_key: &ProjectUnitLinkKey) -> RequestFingerprint {
    let mut hasher = Sha256::new();
    hasher.update(b"boon.compiler-session.source-unit-link.v1\0");
    hasher.update(link_key.namespace.get().to_le_bytes());
    match link_key.module.as_deref() {
        Some(module) => {
            hasher.update([1]);
            update_request_fingerprint_part(&mut hasher, module.as_bytes());
        }
        None => hasher.update([0]),
    }
    hasher.update((link_key.module_functions.len() as u64).to_le_bytes());
    for function in &link_key.module_functions {
        update_request_fingerprint_part(&mut hasher, function.as_bytes());
    }
    hasher.finalize().into()
}

fn dependent_request_fingerprint(
    domain: &[u8],
    dependency: RequestFingerprint,
    local: RequestFingerprint,
) -> RequestFingerprint {
    request_fingerprint(domain, [dependency.as_slice(), local.as_slice()])
}

fn request_fingerprint<'a>(
    domain: &[u8],
    parts: impl IntoIterator<Item = &'a [u8]>,
) -> RequestFingerprint {
    let mut hasher = Sha256::new();
    hasher.update(domain);
    for part in parts {
        update_request_fingerprint_part(&mut hasher, part);
    }
    hasher.finalize().into()
}

fn update_request_fingerprint_part(hasher: &mut Sha256, part: &[u8]) {
    hasher.update((part.len() as u64).to_le_bytes());
    hasher.update(part);
}

fn validate_project(project: &CompilerProject) -> CompilerResult<()> {
    if project.units.is_empty() {
        return Err(session_error("compiler project source bundle is empty"));
    }
    let entrypoint = SourceUnitId::from_path(&project.entrypoint).map_err(|error| {
        session_error(format!(
            "compiler project entrypoint `{}` is invalid: {error}",
            project.entrypoint
        ))
    })?;
    let mut paths = BTreeSet::new();
    for unit in &project.units {
        let source_unit_id = SourceUnitId::from_path(&unit.path).map_err(|error| {
            session_error(format!(
                "compiler project source path `{}` is invalid: {error}",
                unit.path
            ))
        })?;
        if !paths.insert(source_unit_id) {
            return Err(session_error(format!(
                "compiler project has a duplicate canonical source path `{}`",
                unit.path
            )));
        }
    }
    if !paths.contains(&entrypoint) {
        return Err(session_error(format!(
            "compiler project entrypoint `{}` is absent from its source bundle",
            project.entrypoint
        )));
    }
    Ok(())
}

fn canceled_error() -> Box<dyn std::error::Error> {
    session_error("compiler request canceled")
}

fn session_error(message: impl Into<String>) -> Box<dyn std::error::Error> {
    PlanError::new(message).into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn project(source: &str) -> CompilerProject {
        CompilerProject::new(
            "RUN.bn",
            vec![CompilerSourceUnit {
                path: "RUN.bn".to_owned(),
                source: source.to_owned(),
            }],
            TargetProfile::SoftwareDefault,
            ProgramRole::Server,
            ApplicationIdentity::compiler_default(),
        )
    }

    #[test]
    fn checked_stage_is_consumed_once_by_verified_stage_and_then_reused() {
        let mut session = CompilerSession::new();
        let project = session.open_project(project("value: 1")).unwrap();
        let revision = session.revision(project).unwrap();
        let token = CancellationToken::new();
        let diagnostics = session
            .request(project, revision, CompileIntent::Diagnostics, &token)
            .unwrap();
        assert!(
            !diagnostics
                .diagnostics()
                .unwrap()
                .output
                .report
                .has_errors()
        );
        let diagnostics_output = &diagnostics.diagnostics().unwrap().output;
        assert!(diagnostics_output.program.is_none());
        assert!(diagnostics_output.construction.is_some());
        let first_plan = session
            .request(project, revision, CompileIntent::VerifiedPreview, &token)
            .unwrap()
            .compiled()
            .unwrap()
            .plan
            .plan()
            .clone();
        let second_plan = session
            .request(project, revision, CompileIntent::VerifiedPreview, &token)
            .unwrap()
            .compiled()
            .unwrap()
            .plan
            .plan()
            .clone();
        assert_eq!(first_plan, second_plan);
    }

    #[test]
    fn unit_native_session_verified_artifact_matches_assembled_oracle() {
        let units = vec![
            CompilerSourceUnit {
                path: "RUN.bn".to_owned(),
                source: "value: Math/double(input: 2)\n".to_owned(),
            },
            CompilerSourceUnit {
                path: "Math.bn".to_owned(),
                source: "FUNCTION double(input) {\n    input + input\n}\n".to_owned(),
            },
        ];
        let mut session = CompilerSession::new();
        let project = session
            .open_project(CompilerProject::new(
                "RUN.bn",
                units.clone(),
                TargetProfile::SoftwareDefault,
                ProgramRole::Server,
                ApplicationIdentity::compiler_default(),
            ))
            .unwrap();
        let revision = session.revision(project).unwrap();
        let unit_native = session
            .request(
                project,
                revision,
                CompileIntent::VerifiedPreview,
                &CancellationToken::new(),
            )
            .unwrap();
        let unit_native = unit_native.compiled().unwrap();
        let unit_native_artifact = (
            unit_native.source_bundle_digest_v1,
            unit_native.semantic_program_digest,
            unit_native.verification_manifest_digest,
            unit_native.plan.plan().clone(),
            unit_native.plan.plan_hash().to_owned(),
        );

        let assembled = crate::compile_sealed_machine_plan(crate::CompileRequest::source_units(
            "RUN.bn",
            &units,
            TargetProfile::SoftwareDefault,
            ProgramRole::Server,
            ApplicationIdentity::compiler_default(),
        ))
        .unwrap();
        let assembled_artifact = (
            assembled.source_bundle_digest_v1,
            assembled.semantic_program_digest,
            assembled.verification_manifest_digest,
            assembled.plan.plan().clone(),
            assembled.plan.plan_hash().to_owned(),
        );
        assert_eq!(unit_native_artifact, assembled_artifact);
    }

    #[test]
    fn verified_request_installs_and_retains_the_cold_request_graph_snapshot() {
        let mut session = CompilerSession::new();
        let project = session.open_project(project("value: 1")).unwrap();
        let revision = session.revision(project).unwrap();
        session
            .request(
                project,
                revision,
                CompileIntent::VerifiedPreview,
                &CancellationToken::new(),
            )
            .unwrap();

        let (graph_revision, graph) = session
            .request_graph_snapshot(project)
            .unwrap()
            .expect("verified request installs its request graph");
        assert_eq!(graph_revision, revision);
        assert_eq!(graph.revision(), boon_compilation_db::Revision(0));
        assert!(graph.request_count() > 0);

        let next_revision = session
            .apply_update(project, UnitUpdate::new("RUN.bn", "value: 2"))
            .unwrap();
        let (retained_revision, retained) = session
            .request_graph_snapshot(project)
            .unwrap()
            .expect("source update keeps the last verified graph alive");
        assert_eq!(retained_revision, revision);
        assert!(Arc::ptr_eq(&graph, &retained));
        assert_ne!(next_revision, retained_revision);
    }

    #[test]
    fn update_invalidates_checked_and_verified_results() {
        let mut session = CompilerSession::new();
        let project = session.open_project(project("value: 1")).unwrap();
        let revision = session.revision(project).unwrap();
        session
            .request(
                project,
                revision,
                CompileIntent::VerifiedPreview,
                &CancellationToken::new(),
            )
            .unwrap();
        let revision = session
            .apply_update(project, UnitUpdate::new("RUN.bn", "value: 2"))
            .unwrap();
        let result = session
            .request(
                project,
                revision,
                CompileIntent::VerifiedPreview,
                &CancellationToken::new(),
            )
            .unwrap();
        assert!(result.compiled().is_some());
    }

    #[test]
    fn source_updates_are_atomic_and_advance_one_revision() {
        let mut session = CompilerSession::new();
        let project = session
            .open_project(CompilerProject::new(
                "RUN.bn",
                vec![
                    CompilerSourceUnit {
                        path: "A.bn".to_owned(),
                        source: "a: 1".to_owned(),
                    },
                    CompilerSourceUnit {
                        path: "RUN.bn".to_owned(),
                        source: "value: 1".to_owned(),
                    },
                ],
                TargetProfile::SoftwareDefault,
                ProgramRole::Server,
                ApplicationIdentity::compiler_default(),
            ))
            .unwrap();
        let revision = session
            .apply_updates(
                project,
                [
                    UnitUpdate::new("A.bn", "a: 2"),
                    UnitUpdate::new("RUN.bn", "value: 2"),
                ],
            )
            .unwrap();
        assert_eq!(revision, Revision(1));

        let error = session
            .apply_updates(
                project,
                [
                    UnitUpdate::new("A.bn", "a: 3"),
                    UnitUpdate::new("missing.bn", "missing: 1"),
                ],
            )
            .unwrap_err();
        assert!(error.to_string().contains("missing.bn"));
        assert_eq!(session.revision(project).unwrap(), Revision(1));
    }

    #[test]
    fn session_reuses_unchanged_unit_syntax_and_reparses_only_the_changed_unit() {
        let mut session = CompilerSession::new();
        let source_project = CompilerProject::new(
            "RUN.bn",
            vec![
                CompilerSourceUnit {
                    path: "A.bn".to_owned(),
                    source: "a: 1\n".to_owned(),
                },
                CompilerSourceUnit {
                    path: "RUN.bn".to_owned(),
                    source: "value: 1\n".to_owned(),
                },
            ],
            TargetProfile::SoftwareDefault,
            ProgramRole::Server,
            ApplicationIdentity::compiler_default(),
        );
        let project = session.open_project(source_project).unwrap();
        let first_revision = session.revision(project).unwrap();
        {
            let result = session
                .request(
                    project,
                    first_revision,
                    CompileIntent::Diagnostics,
                    &CancellationToken::new(),
                )
                .unwrap();
            let first = result.diagnostics().unwrap();
            assert_eq!(first.profile.parse_work.source_units_attempted, 2);
            assert_eq!(first.profile.parse_work.source_units_parsed, 2);
            assert_eq!(first.profile.parse_work.source_units_reused, 0);
            assert_eq!(first.profile.parse_work.nodes_rebased, 0);
        }
        let first_stats = session.syntax_request_stats(project).unwrap();
        assert_eq!(first_stats.executed, 4);
        assert_eq!(first_stats.reused, 0);
        assert_eq!(first_stats.changed, 4);
        let retained_a = session
            .unit_syntax_snapshot(project, "A.bn")
            .unwrap()
            .unwrap();
        let replaced_run = session
            .unit_syntax_snapshot(project, "RUN.bn")
            .unwrap()
            .unwrap();

        let second_revision = session
            .apply_update(project, UnitUpdate::new("RUN.bn", "value: 2\n"))
            .unwrap();
        assert!(
            session
                .unit_syntax_snapshot(project, "RUN.bn")
                .unwrap()
                .is_none()
        );
        let second_work = {
            let result = session
                .request(
                    project,
                    second_revision,
                    CompileIntent::Diagnostics,
                    &CancellationToken::new(),
                )
                .unwrap();
            let second = result.diagnostics().unwrap();
            assert_eq!(second.profile.parse_work.source_units_attempted, 1);
            assert_eq!(second.profile.parse_work.source_units_parsed, 1);
            assert_eq!(second.profile.parse_work.source_units_reused, 1);
            assert_eq!(second.profile.parse_work.nodes_rebased, 0);
            second.profile.parse_work
        };
        let second_stats = session.syntax_request_stats(project).unwrap();
        assert_eq!(second_stats.executed, 6);
        assert_eq!(second_stats.reused, 2);
        assert_eq!(second_stats.changed, 6);

        let mut isolated = CompilerSession::new();
        let isolated_project = isolated
            .open_project(CompilerProject::new(
                "RUN.bn",
                vec![CompilerSourceUnit {
                    path: "RUN.bn".to_owned(),
                    source: "value: 2\n".to_owned(),
                }],
                TargetProfile::SoftwareDefault,
                ProgramRole::Server,
                ApplicationIdentity::compiler_default(),
            ))
            .unwrap();
        let isolated_revision = isolated.revision(isolated_project).unwrap();
        let isolated_result = isolated
            .request(
                isolated_project,
                isolated_revision,
                CompileIntent::Diagnostics,
                &CancellationToken::new(),
            )
            .unwrap();
        let mut isolated_work = isolated_result.diagnostics().unwrap().profile.parse_work;
        isolated_work.record_reused_source_units(1);
        assert_eq!(
            second_work, isolated_work,
            "warm parser and validation work must equal the changed unit alone",
        );

        let current_a = session
            .unit_syntax_snapshot(project, "A.bn")
            .unwrap()
            .unwrap();
        let current_run = session
            .unit_syntax_snapshot(project, "RUN.bn")
            .unwrap()
            .unwrap();
        assert!(Arc::ptr_eq(&retained_a, &current_a));
        assert!(!Arc::ptr_eq(&replaced_run, &current_run));
    }

    #[test]
    fn todo_text_edit_warm_diagnostics_match_a_clean_session() {
        const SOURCE: &str = "TEXT { Walk the dog }";
        const EDITED: &str = "TEXT { Walk the dogs }";
        let (entrypoint, units) =
            crate::compiler_source_project_for_path(Path::new("examples/todo_mvc_physical/RUN.bn"))
                .unwrap();
        let edit_path = "examples/todo_mvc_physical/RUN.bn";
        let edited_source = units
            .iter()
            .find(|unit| unit.path == edit_path)
            .unwrap()
            .source
            .replacen(SOURCE, EDITED, 1);

        let project_for = |units| {
            CompilerProject::new(
                entrypoint.clone(),
                units,
                TargetProfile::SoftwareDefault,
                ProgramRole::Client,
                ApplicationIdentity::compiler_default(),
            )
        };
        let mut warm = CompilerSession::new();
        let warm_project = warm.open_project(project_for(units.clone())).unwrap();
        let base_revision = warm.revision(warm_project).unwrap();
        warm.request(
            warm_project,
            base_revision,
            CompileIntent::Diagnostics,
            &CancellationToken::new(),
        )
        .unwrap();
        let edited_revision = warm
            .apply_update(
                warm_project,
                UnitUpdate::new(edit_path, edited_source.clone()),
            )
            .unwrap();
        let warm_diagnostics = warm
            .request(
                warm_project,
                edited_revision,
                CompileIntent::Diagnostics,
                &CancellationToken::new(),
            )
            .unwrap()
            .diagnostics()
            .unwrap()
            .output
            .report
            .clone();

        let mut edited_units = units;
        edited_units
            .iter_mut()
            .find(|unit| unit.path == edit_path)
            .unwrap()
            .source = edited_source;
        let mut clean = CompilerSession::new();
        let clean_project = clean.open_project(project_for(edited_units)).unwrap();
        let clean_revision = clean.revision(clean_project).unwrap();
        let clean_diagnostics = clean
            .request(
                clean_project,
                clean_revision,
                CompileIntent::Diagnostics,
                &CancellationToken::new(),
            )
            .unwrap()
            .diagnostics()
            .unwrap()
            .output
            .report
            .clone();

        assert_eq!(warm_diagnostics, clean_diagnostics);
        assert!(
            !warm_diagnostics.has_errors(),
            "text-only TodoMVC edit produced diagnostics: {:#?}",
            warm_diagnostics.diagnostics,
        );
    }

    #[test]
    fn unit_topology_changes_are_atomic_and_preserve_unaffected_syntax() {
        let mut session = CompilerSession::new();
        let project = session
            .open_project(CompilerProject::new(
                "RUN.bn",
                vec![
                    CompilerSourceUnit {
                        path: "A.bn".to_owned(),
                        source: "a: 1\n".to_owned(),
                    },
                    CompilerSourceUnit {
                        path: "RUN.bn".to_owned(),
                        source: "value: 1\n".to_owned(),
                    },
                ],
                TargetProfile::SoftwareDefault,
                ProgramRole::Server,
                ApplicationIdentity::compiler_default(),
            ))
            .unwrap();
        let revision = session.revision(project).unwrap();
        session
            .request(
                project,
                revision,
                CompileIntent::Diagnostics,
                &CancellationToken::new(),
            )
            .unwrap();
        let retained_run = session
            .unit_syntax_snapshot(project, "RUN.bn")
            .unwrap()
            .unwrap();

        let revision = session
            .apply_unit_changes(
                project,
                [
                    UnitChange::rename("A.bn", "C.bn"),
                    UnitChange::upsert("B.bn", "b: 2\n"),
                ],
            )
            .unwrap();
        let result = session
            .request(
                project,
                revision,
                CompileIntent::Diagnostics,
                &CancellationToken::new(),
            )
            .unwrap();
        let diagnostics = result.diagnostics().unwrap();
        assert_eq!(diagnostics.profile.parse_work.source_units_attempted, 2);
        assert_eq!(diagnostics.profile.parse_work.source_units_parsed, 2);
        assert_eq!(diagnostics.profile.parse_work.source_units_reused, 1);
        drop(result);
        assert!(Arc::ptr_eq(
            &retained_run,
            &session
                .unit_syntax_snapshot(project, "RUN.bn")
                .unwrap()
                .unwrap()
        ));
        assert!(
            session
                .unit_syntax_snapshot(project, "A.bn")
                .unwrap()
                .is_none()
        );

        let error = session
            .apply_unit_changes(project, [UnitChange::remove("RUN.bn")])
            .unwrap_err();
        assert!(error.to_string().contains("entrypoint"));
        assert_eq!(session.revision(project).unwrap(), revision);
        assert!(
            session
                .unit_syntax_snapshot(project, "RUN.bn")
                .unwrap()
                .is_some()
        );
    }

    #[test]
    fn closed_projects_fail_closed() {
        let mut session = CompilerSession::new();
        let project = session.open_project(project("value: 1")).unwrap();
        session.close_project(project).unwrap();
        assert!(session.revision(project).is_err());
    }

    #[test]
    fn invalid_update_keeps_the_last_verified_artifact() {
        let mut session = CompilerSession::new();
        let project = session.open_project(project("value: 1")).unwrap();
        let first_revision = session.revision(project).unwrap();
        let first_plan = session
            .request(
                project,
                first_revision,
                CompileIntent::VerifiedPreview,
                &CancellationToken::new(),
            )
            .unwrap()
            .compiled()
            .unwrap()
            .plan
            .plan()
            .clone();

        let invalid_revision = session
            .apply_update(project, UnitUpdate::new("RUN.bn", "value: ["))
            .unwrap();
        assert!(
            session
                .request(
                    project,
                    invalid_revision,
                    CompileIntent::VerifiedPreview,
                    &CancellationToken::new(),
                )
                .is_err()
        );

        let (retained_revision, retained) = session
            .last_verified(project)
            .unwrap()
            .expect("last verified artifact remains available");
        assert_eq!(retained_revision, first_revision);
        assert_eq!(retained.plan.plan(), &first_plan);
    }

    #[test]
    fn canceled_request_publishes_nothing() {
        let mut session = CompilerSession::new();
        let project = session.open_project(project("value: 1")).unwrap();
        let revision = session.revision(project).unwrap();
        let token = CancellationToken::new();
        token.cancel();
        let error = match session.request(project, revision, CompileIntent::Diagnostics, &token) {
            Ok(_) => panic!("canceled request unexpectedly published a result"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("canceled"));
    }
}
