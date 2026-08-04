use crate::{
    CheckedCompileRequest, CheckedSourceFromSource, CompiledSealedMachinePlanFromSource,
    CompilerResult, CompilerSourceUnit, check_diagnostics_project_syntax_source,
    check_runtime_project_syntax_source, finish_checked_machine_plan_with_cancellation,
};
use boon_compilation_db::{
    RequestAbortReason, RequestEvaluationStats, RequestEvaluatorGraph, RequestFamily,
    RequestFingerprint, RequestInputFingerprint, RequestOutputFingerprint, RequestStart,
    Revision as EvaluationRevision, TypedRequestTable,
};
use boon_parser::{
    ParseWorkCounters, ParsedSourceUnit, ProjectSyntaxSnapshot, ProjectUnitLinkKey,
    UnitSyntaxSnapshot, link_project_source_unit_profiled, parse_project_source_unit_profiled,
    project_module_name_for_source_unit, project_syntax_namespaces,
};
use boon_plan::{
    ApplicationIdentity, MigrationPredecessorBinding, PlanError, ProgramRole, TargetProfile,
};
use boon_syntax::StableCheckOwnerKey;
use boon_syntax::{SourceUnitId, SyntaxUnitNamespace, UnitItemKind};
use boon_typecheck::{
    OwnerConstraintDependencyKind, OwnerConstraintSeed, OwnerConstraintSummary,
    OwnerDeclarationKind, OwnerInterfaceScc, OwnerInterfaceSccKey, OwnerInterfaceSccResult,
    OwnerInterfaceTopology, OwnerReferenceKind, OwnerSourceMap, OwnerSymbolReference,
    OwnerSyntaxInput, ResolvedOwnerSymbolReference, build_owner_interface_topology,
    project_owner_constraint_seed, project_owner_source_map, project_owner_syntax_input,
    resolve_owner_constraint_seed, solve_owner_interface_scc,
    stable_check_owner_key_fingerprint_v1,
};
use serde::Serialize;
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
    syntax_evaluator: RequestEvaluatorGraph,
    parse_requests: TypedRequestTable<ParseUnitRequest>,
    unit_link_summary_requests: TypedRequestTable<UnitLinkSummaryRequest>,
    project_namespace_requests: TypedRequestTable<ProjectNamespaceRequest>,
    project_module_requests: TypedRequestTable<ProjectModuleRequest>,
    unit_link_overlay_requests: TypedRequestTable<UnitLinkOverlayRequest>,
    link_requests: TypedRequestTable<LinkUnitRequest>,
    owner_input_requests: TypedRequestTable<OwnerInputRequest>,
    owner_source_map_requests: TypedRequestTable<OwnerSourceMapRequest>,
    owner_constraint_seed_requests: TypedRequestTable<OwnerConstraintSeedRequest>,
    project_owner_symbol_requests: TypedRequestTable<ProjectOwnerSymbolRequest>,
    owner_constraint_requests: TypedRequestTable<OwnerConstraintRequest>,
    project_owner_interface_topology_requests:
        TypedRequestTable<ProjectOwnerInterfaceTopologyRequest>,
    owner_interface_scc_plan_requests: TypedRequestTable<OwnerInterfaceSccPlanRequest>,
    owner_interface_scc_requests: TypedRequestTable<OwnerInterfaceSccRequest>,
    checked: Option<CheckedSourceFromSource>,
    compiled: Option<(Revision, CompiledSealedMachinePlanFromSource)>,
    request_graph: Option<(
        Revision,
        Arc<boon_compilation_db::SealedRequestGraphSnapshot>,
    )>,
}

struct ParseUnitRequest;

impl RequestFamily for ParseUnitRequest {
    type Key = SourceUnitId;
    type Value = Arc<ParsedSourceUnit>;

    const NAME: &'static str = "boon.compiler.parse-unit.v1";

    fn key_fingerprint(key: &Self::Key) -> RequestFingerprint {
        source_unit_key_fingerprint(b"boon.compiler.parse-unit-key.v1\0", key)
    }

    fn output_fingerprint(
        value: &Self::Value,
    ) -> Result<RequestOutputFingerprint, boon_compilation_db::CompilationDbError> {
        Ok(RequestOutputFingerprint(source_unit_request_fingerprint(
            &value.path,
            &value.source,
        )))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct UnitLinkSummary {
    source_unit_id: SourceUnitId,
    declared_functions: Vec<String>,
}

struct UnitLinkSummaryRequest;

impl RequestFamily for UnitLinkSummaryRequest {
    type Key = SourceUnitId;
    type Value = UnitLinkSummary;

    const NAME: &'static str = "boon.compiler.unit-link-summary.v1";

    fn key_fingerprint(key: &Self::Key) -> RequestFingerprint {
        source_unit_key_fingerprint(b"boon.compiler.unit-link-summary-key.v1\0", key)
    }

    fn output_fingerprint(
        value: &Self::Value,
    ) -> Result<RequestOutputFingerprint, boon_compilation_db::CompilationDbError> {
        let mut hasher = Sha256::new();
        hasher.update(b"boon.compiler.unit-link-summary-result.v1\0");
        update_request_fingerprint_part(&mut hasher, value.source_unit_id.as_str().as_bytes());
        hasher.update((value.declared_functions.len() as u64).to_le_bytes());
        for function in &value.declared_functions {
            update_request_fingerprint_part(&mut hasher, function.as_bytes());
        }
        Ok(RequestOutputFingerprint(hasher.finalize().into()))
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct ProjectNamespaceKey;

struct ProjectNamespaceRequest;

impl RequestFamily for ProjectNamespaceRequest {
    type Key = ProjectNamespaceKey;
    type Value = Arc<BTreeMap<SourceUnitId, SyntaxUnitNamespace>>;

    const NAME: &'static str = "boon.compiler.project-namespace-plan.v1";

    fn key_fingerprint(_key: &Self::Key) -> RequestFingerprint {
        request_fingerprint(
            b"boon.compiler.project-namespace-plan-key.v1\0",
            std::iter::empty(),
        )
    }

    fn output_fingerprint(
        value: &Self::Value,
    ) -> Result<RequestOutputFingerprint, boon_compilation_db::CompilationDbError> {
        let mut hasher = Sha256::new();
        hasher.update(b"boon.compiler.project-namespace-plan-result.v1\0");
        hasher.update((value.len() as u64).to_le_bytes());
        for (source_unit_id, namespace) in value.iter() {
            update_request_fingerprint_part(&mut hasher, source_unit_id.as_str().as_bytes());
            hasher.update(namespace.get().to_le_bytes());
        }
        Ok(RequestOutputFingerprint(hasher.finalize().into()))
    }
}

struct ProjectModuleRequest;

impl RequestFamily for ProjectModuleRequest {
    type Key = String;
    type Value = Arc<Vec<String>>;

    const NAME: &'static str = "boon.compiler.project-module-index.v1";

    fn key_fingerprint(key: &Self::Key) -> RequestFingerprint {
        request_fingerprint(
            b"boon.compiler.project-module-index-key.v1\0",
            [key.as_bytes()],
        )
    }

    fn output_fingerprint(
        value: &Self::Value,
    ) -> Result<RequestOutputFingerprint, boon_compilation_db::CompilationDbError> {
        let mut hasher = Sha256::new();
        hasher.update(b"boon.compiler.project-module-index-result.v1\0");
        hasher.update((value.len() as u64).to_le_bytes());
        for function in value.iter() {
            update_request_fingerprint_part(&mut hasher, function.as_bytes());
        }
        Ok(RequestOutputFingerprint(hasher.finalize().into()))
    }
}

struct UnitLinkOverlayRequest;

impl RequestFamily for UnitLinkOverlayRequest {
    type Key = SourceUnitId;
    type Value = ProjectUnitLinkKey;

    const NAME: &'static str = "boon.compiler.unit-link-overlay.v1";

    fn key_fingerprint(key: &Self::Key) -> RequestFingerprint {
        source_unit_key_fingerprint(b"boon.compiler.unit-link-overlay-key.v1\0", key)
    }

    fn output_fingerprint(
        value: &Self::Value,
    ) -> Result<RequestOutputFingerprint, boon_compilation_db::CompilationDbError> {
        Ok(RequestOutputFingerprint(project_link_request_fingerprint(
            value,
        )))
    }
}

struct LinkUnitRequest;

impl RequestFamily for LinkUnitRequest {
    type Key = SourceUnitId;
    type Value = Arc<UnitSyntaxSnapshot>;

    const NAME: &'static str = "boon.compiler.link-unit.v1";

    fn key_fingerprint(key: &Self::Key) -> RequestFingerprint {
        source_unit_key_fingerprint(b"boon.compiler.link-unit-key.v1\0", key)
    }

    fn output_fingerprint(
        value: &Self::Value,
    ) -> Result<RequestOutputFingerprint, boon_compilation_db::CompilationDbError> {
        Ok(RequestOutputFingerprint(request_fingerprint(
            b"boon.compiler.link-unit-result.v1\0",
            [
                value.path.as_bytes(),
                value.source.as_bytes(),
                project_link_request_fingerprint(value.link_key()).as_slice(),
            ],
        )))
    }
}

struct OwnerInputRequest;

impl RequestFamily for OwnerInputRequest {
    type Key = StableCheckOwnerKey;
    type Value = Arc<OwnerSyntaxInput>;

    const NAME: &'static str = "boon.compiler.owner-input.v1";

    fn key_fingerprint(key: &Self::Key) -> RequestFingerprint {
        stable_check_owner_key_fingerprint_v1(key)
    }

    fn output_fingerprint(
        value: &Self::Value,
    ) -> Result<RequestOutputFingerprint, boon_compilation_db::CompilationDbError> {
        Ok(RequestOutputFingerprint(value.fingerprint_v1()))
    }
}

struct OwnerSourceMapRequest;

impl RequestFamily for OwnerSourceMapRequest {
    type Key = StableCheckOwnerKey;
    type Value = Arc<OwnerSourceMap>;

    const NAME: &'static str = "boon.compiler.owner-source-map.v2";

    fn key_fingerprint(key: &Self::Key) -> RequestFingerprint {
        stable_check_owner_key_fingerprint_v1(key)
    }

    fn output_fingerprint(
        value: &Self::Value,
    ) -> Result<RequestOutputFingerprint, boon_compilation_db::CompilationDbError> {
        Ok(RequestOutputFingerprint(value.fingerprint_v2()))
    }
}

struct OwnerConstraintSeedRequest;

impl RequestFamily for OwnerConstraintSeedRequest {
    type Key = StableCheckOwnerKey;
    type Value = Arc<OwnerConstraintSeed>;

    const NAME: &'static str = "boon.compiler.owner-constraint-seed.v1";

    fn key_fingerprint(key: &Self::Key) -> RequestFingerprint {
        stable_check_owner_key_fingerprint_v1(key)
    }

    fn output_fingerprint(
        value: &Self::Value,
    ) -> Result<RequestOutputFingerprint, boon_compilation_db::CompilationDbError> {
        Ok(RequestOutputFingerprint(value.fingerprint_v1()))
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct ProjectOwnerSymbolKey;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
enum OwnerSymbolNamespace {
    Value,
    Callable,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
struct OwnerSymbolKey {
    namespace: OwnerSymbolNamespace,
    parts: Vec<String>,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
struct OwnerSymbolCandidate {
    priority: u8,
    owner: StableCheckOwnerKey,
    parameters: Box<[boon_typecheck::OwnerParameterConstraint]>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
struct ProjectOwnerSymbolIndex {
    symbols: BTreeMap<OwnerSymbolKey, Box<[OwnerSymbolCandidate]>>,
}

impl ProjectOwnerSymbolIndex {
    fn resolve(
        &self,
        owner: &StableCheckOwnerKey,
        reference: &OwnerSymbolReference,
    ) -> Option<OwnerSymbolCandidate> {
        let namespace = match reference.kind {
            OwnerReferenceKind::Value => OwnerSymbolNamespace::Value,
            OwnerReferenceKind::Callable => OwnerSymbolNamespace::Callable,
        };
        let mut paths = Vec::new();
        if namespace == OwnerSymbolNamespace::Value {
            let mut parent = owner_parent_value_path(owner);
            loop {
                let mut candidate = parent.clone();
                candidate.extend(reference.parts.iter().cloned());
                paths.push(candidate);
                if parent.pop().is_none() {
                    break;
                }
            }
        }
        paths.push(reference.parts.to_vec());
        paths.into_iter().find_map(|parts| {
            let candidates = self.symbols.get(&OwnerSymbolKey { namespace, parts })?;
            let best = candidates.first()?.priority;
            let mut best_candidates = candidates
                .iter()
                .take_while(|candidate| candidate.priority == best);
            let candidate = best_candidates.next()?;
            best_candidates.next().is_none().then(|| candidate.clone())
        })
    }
}

struct ProjectOwnerSymbolRequest;

impl RequestFamily for ProjectOwnerSymbolRequest {
    type Key = ProjectOwnerSymbolKey;
    type Value = Arc<ProjectOwnerSymbolIndex>;

    const NAME: &'static str = "boon.compiler.project-owner-symbol-index.v1";

    fn key_fingerprint(_key: &Self::Key) -> RequestFingerprint {
        request_fingerprint(
            b"boon.compiler.project-owner-symbol-index-key.v1\0",
            std::iter::empty(),
        )
    }

    fn output_fingerprint(
        value: &Self::Value,
    ) -> Result<RequestOutputFingerprint, boon_compilation_db::CompilationDbError> {
        let mut hasher = Sha256::new();
        hasher.update(b"boon.compiler.project-owner-symbol-index-result.v1\0");
        hasher.update((value.symbols.len() as u64).to_le_bytes());
        for (key, candidates) in &value.symbols {
            hasher.update([match key.namespace {
                OwnerSymbolNamespace::Value => 0,
                OwnerSymbolNamespace::Callable => 1,
            }]);
            hasher.update((key.parts.len() as u64).to_le_bytes());
            for part in &key.parts {
                update_request_fingerprint_part(&mut hasher, part.as_bytes());
            }
            hasher.update((candidates.len() as u64).to_le_bytes());
            for candidate in candidates {
                hasher.update([candidate.priority]);
                hasher.update(stable_check_owner_key_fingerprint_v1(&candidate.owner));
                hasher.update((candidate.parameters.len() as u64).to_le_bytes());
                for parameter in &candidate.parameters {
                    update_request_fingerprint_part(&mut hasher, parameter.name.as_bytes());
                    hasher.update([match parameter.kind {
                        boon_typecheck::OwnerParameterKind::Value => 0,
                        boon_typecheck::OwnerParameterKind::Out => 1,
                    }]);
                    hasher.update(parameter.ordinal.to_le_bytes());
                }
            }
        }
        Ok(RequestOutputFingerprint(hasher.finalize().into()))
    }
}

struct OwnerConstraintRequest;

impl RequestFamily for OwnerConstraintRequest {
    type Key = StableCheckOwnerKey;
    type Value = Arc<OwnerConstraintSummary>;

    const NAME: &'static str = "boon.compiler.owner-constraint-summary.v1";

    fn key_fingerprint(key: &Self::Key) -> RequestFingerprint {
        stable_check_owner_key_fingerprint_v1(key)
    }

    fn output_fingerprint(
        value: &Self::Value,
    ) -> Result<RequestOutputFingerprint, boon_compilation_db::CompilationDbError> {
        Ok(RequestOutputFingerprint(value.fingerprint_v1()))
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct ProjectOwnerInterfaceTopologyKey;

struct ProjectOwnerInterfaceTopologyRequest;

impl RequestFamily for ProjectOwnerInterfaceTopologyRequest {
    type Key = ProjectOwnerInterfaceTopologyKey;
    type Value = Arc<OwnerInterfaceTopology>;

    const NAME: &'static str = "boon.compiler.project-owner-interface-topology.v1";

    fn key_fingerprint(_key: &Self::Key) -> RequestFingerprint {
        request_fingerprint(
            b"boon.compiler.project-owner-interface-topology-key.v1\0",
            std::iter::empty(),
        )
    }

    fn output_fingerprint(
        value: &Self::Value,
    ) -> Result<RequestOutputFingerprint, boon_compilation_db::CompilationDbError> {
        Ok(RequestOutputFingerprint(value.fingerprint_v1()))
    }
}

struct OwnerInterfaceSccPlanRequest;

impl RequestFamily for OwnerInterfaceSccPlanRequest {
    type Key = OwnerInterfaceSccKey;
    type Value = Arc<OwnerInterfaceScc>;

    const NAME: &'static str = "boon.compiler.owner-interface-scc-plan.v1";

    fn key_fingerprint(key: &Self::Key) -> RequestFingerprint {
        let fingerprints = key
            .members
            .iter()
            .map(stable_check_owner_key_fingerprint_v1)
            .collect::<Vec<_>>();
        request_fingerprint(
            b"boon.compiler.owner-interface-scc-plan-key.v1\0",
            fingerprints.iter().map(<[u8; 32]>::as_slice),
        )
    }

    fn output_fingerprint(
        value: &Self::Value,
    ) -> Result<RequestOutputFingerprint, boon_compilation_db::CompilationDbError> {
        Ok(RequestOutputFingerprint(value.fingerprint_v1()))
    }
}

struct OwnerInterfaceSccRequest;

impl RequestFamily for OwnerInterfaceSccRequest {
    type Key = OwnerInterfaceSccKey;
    type Value = Arc<OwnerInterfaceSccResult>;

    const NAME: &'static str = "boon.compiler.owner-interface-scc-result.v1";

    fn key_fingerprint(key: &Self::Key) -> RequestFingerprint {
        OwnerInterfaceSccPlanRequest::key_fingerprint(key)
    }

    fn output_fingerprint(
        value: &Self::Value,
    ) -> Result<RequestOutputFingerprint, boon_compilation_db::CompilationDbError> {
        Ok(RequestOutputFingerprint(value.fingerprint_v1()))
    }
}

impl CompilerSession {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn open_project(&mut self, project: CompilerProject) -> CompilerResult<ProjectId> {
        validate_project(&project)?;
        self.next_project = self
            .next_project
            .checked_add(1)
            .ok_or_else(|| session_error("compiler session project id overflow"))?;
        let id = ProjectId(self.next_project);
        self.projects.insert(
            id,
            ProjectState {
                source: project,
                revision: Revision(0),
                syntax_evaluator: RequestEvaluatorGraph::new(EvaluationRevision(0)),
                parse_requests: TypedRequestTable::new(),
                unit_link_summary_requests: TypedRequestTable::new(),
                project_namespace_requests: TypedRequestTable::new(),
                project_module_requests: TypedRequestTable::new(),
                unit_link_overlay_requests: TypedRequestTable::new(),
                link_requests: TypedRequestTable::new(),
                owner_input_requests: TypedRequestTable::new(),
                owner_source_map_requests: TypedRequestTable::new(),
                owner_constraint_seed_requests: TypedRequestTable::new(),
                project_owner_symbol_requests: TypedRequestTable::new(),
                owner_constraint_requests: TypedRequestTable::new(),
                project_owner_interface_topology_requests: TypedRequestTable::new(),
                owner_interface_scc_plan_requests: TypedRequestTable::new(),
                owner_interface_scc_requests: TypedRequestTable::new(),
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
            .syntax_evaluator
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
        let mut surviving_modules = BTreeSet::new();
        for unit in &candidate.units {
            let source_unit_id = SourceUnitId::from_path(&unit.path).map_err(|error| {
                session_error(format!(
                    "compiler project {} has invalid source unit `{}`: {error}",
                    project.0, unit.path
                ))
            })?;
            if let Some(module) =
                project_module_name_for_source_unit(&candidate.entrypoint, source_unit_id.as_str())
            {
                surviving_modules.insert(module);
            }
            surviving_sources.insert(source_unit_id);
        }
        let next_revision = state.revision.0.checked_add(1).ok_or_else(|| {
            session_error(format!("compiler project {} revision overflow", project.0))
        })?;
        state
            .syntax_evaluator
            .advance_to(EvaluationRevision(next_revision))?;
        state
            .parse_requests
            .retain(&mut state.syntax_evaluator, |key| {
                surviving_sources.contains(key)
            })?;
        state
            .unit_link_summary_requests
            .retain(&mut state.syntax_evaluator, |key| {
                surviving_sources.contains(key)
            })?;
        state
            .project_module_requests
            .retain(&mut state.syntax_evaluator, |key| {
                surviving_modules.contains(key)
            })?;
        state
            .unit_link_overlay_requests
            .retain(&mut state.syntax_evaluator, |key| {
                surviving_sources.contains(key)
            })?;
        state
            .link_requests
            .retain(&mut state.syntax_evaluator, |key| {
                surviving_sources.contains(key)
            })?;
        state
            .owner_input_requests
            .retain(&mut state.syntax_evaluator, |key| {
                surviving_sources.contains(key.source_unit_id())
            })?;
        state
            .owner_source_map_requests
            .retain(&mut state.syntax_evaluator, |key| {
                surviving_sources.contains(key.source_unit_id())
            })?;
        state
            .owner_constraint_seed_requests
            .retain(&mut state.syntax_evaluator, |key| {
                surviving_sources.contains(key.source_unit_id())
            })?;
        state
            .owner_constraint_requests
            .retain(&mut state.syntax_evaluator, |key| {
                surviving_sources.contains(key.source_unit_id())
            })?;
        state
            .owner_interface_scc_plan_requests
            .retain(&mut state.syntax_evaluator, |key| {
                key.members
                    .iter()
                    .all(|owner| surviving_sources.contains(owner.source_unit_id()))
            })?;
        state
            .owner_interface_scc_requests
            .retain(&mut state.syntax_evaluator, |key| {
                key.members
                    .iter()
                    .all(|owner| surviving_sources.contains(owner.source_unit_id()))
            })?;
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

    /// Cumulative typed frontend request work for this session project.
    pub fn frontend_request_stats(
        &self,
        project: ProjectId,
    ) -> CompilerResult<RequestEvaluationStats> {
        let state = self
            .projects
            .get(&project)
            .ok_or_else(|| session_error(format!("unknown compiler project {}", project.0)))?;
        Ok(state.syntax_evaluator.stats())
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
            .link_requests
            .current_value(&state.syntax_evaluator, &source_unit_id)?
            .map(Arc::clone))
    }

    /// Returns the current span-free syntax shard for one stable check owner.
    pub fn owner_syntax_input(
        &self,
        project: ProjectId,
        owner: &StableCheckOwnerKey,
    ) -> CompilerResult<Option<Arc<OwnerSyntaxInput>>> {
        let state = self
            .projects
            .get(&project)
            .ok_or_else(|| session_error(format!("unknown compiler project {}", project.0)))?;
        Ok(state
            .owner_input_requests
            .current_value(&state.syntax_evaluator, owner)?
            .map(Arc::clone))
    }

    /// Returns current source positions for one stable check owner.
    pub fn owner_source_map(
        &self,
        project: ProjectId,
        owner: &StableCheckOwnerKey,
    ) -> CompilerResult<Option<Arc<OwnerSourceMap>>> {
        let state = self
            .projects
            .get(&project)
            .ok_or_else(|| session_error(format!("unknown compiler project {}", project.0)))?;
        Ok(state
            .owner_source_map_requests
            .current_value(&state.syntax_evaluator, owner)?
            .map(Arc::clone))
    }

    /// Returns the current span/literal-payload-free constraint seed.
    pub fn owner_constraint_seed(
        &self,
        project: ProjectId,
        owner: &StableCheckOwnerKey,
    ) -> CompilerResult<Option<Arc<OwnerConstraintSeed>>> {
        let state = self
            .projects
            .get(&project)
            .ok_or_else(|| session_error(format!("unknown compiler project {}", project.0)))?;
        Ok(state
            .owner_constraint_seed_requests
            .current_value(&state.syntax_evaluator, owner)?
            .map(Arc::clone))
    }

    /// Returns stable symbol-resolved dependencies for one owner.
    pub fn owner_constraint_summary(
        &self,
        project: ProjectId,
        owner: &StableCheckOwnerKey,
    ) -> CompilerResult<Option<Arc<OwnerConstraintSummary>>> {
        let state = self
            .projects
            .get(&project)
            .ok_or_else(|| session_error(format!("unknown compiler project {}", project.0)))?;
        Ok(state
            .owner_constraint_requests
            .current_value(&state.syntax_evaluator, owner)?
            .map(Arc::clone))
    }

    /// Returns the current tagged, dependency-first interface SCC topology.
    pub fn owner_interface_topology(
        &self,
        project: ProjectId,
    ) -> CompilerResult<Option<Arc<OwnerInterfaceTopology>>> {
        let state = self
            .projects
            .get(&project)
            .ok_or_else(|| session_error(format!("unknown compiler project {}", project.0)))?;
        Ok(state
            .project_owner_interface_topology_requests
            .current_value(&state.syntax_evaluator, &ProjectOwnerInterfaceTopologyKey)?
            .map(Arc::clone))
    }

    /// Returns the alpha-normalized current interface result for the SCC that
    /// owns `owner`.
    pub fn owner_interface_result(
        &self,
        project: ProjectId,
        owner: &StableCheckOwnerKey,
    ) -> CompilerResult<Option<Arc<OwnerInterfaceSccResult>>> {
        let state = self
            .projects
            .get(&project)
            .ok_or_else(|| session_error(format!("unknown compiler project {}", project.0)))?;
        let Some(topology) = state
            .project_owner_interface_topology_requests
            .current_value(&state.syntax_evaluator, &ProjectOwnerInterfaceTopologyKey)?
        else {
            return Ok(None);
        };
        let Some(scc) = topology.scc_for_owner(owner) else {
            return Ok(None);
        };
        Ok(state
            .owner_interface_scc_requests
            .current_value(&state.syntax_evaluator, &scc.key)?
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
        let input_fingerprint =
            RequestInputFingerprint(source_unit_request_fingerprint(&unit.path, &unit.source));
        match state.parse_requests.begin(
            &mut state.syntax_evaluator,
            source_unit_id.clone(),
            input_fingerprint,
        )? {
            RequestStart::Reused => work.record_reused_source_units(1),
            RequestStart::Execute(ticket) => {
                let (parsed, profile) =
                    parse_project_source_unit_profiled(unit.path.clone(), unit.source.clone());
                let parsed = match parsed {
                    Ok(parsed) => Arc::new(parsed),
                    Err(error) => {
                        state.parse_requests.abort(
                            &mut state.syntax_evaluator,
                            ticket,
                            RequestAbortReason::Failed,
                        )?;
                        return Err(error.into());
                    }
                };
                work.accumulate(profile.work_counters);
                state
                    .parse_requests
                    .publish(&mut state.syntax_evaluator, ticket, parsed)?;
            }
        }
        let parsed = Arc::clone(
            state
                .parse_requests
                .current_value(&state.syntax_evaluator, &source_unit_id)
                .expect("request value read occurs outside evaluation")
                .expect("parse request publishes current parsed syntax"),
        );
        parsed_units.push((source_unit_id, parsed));
    }

    let summary_input = RequestInputFingerprint(request_fingerprint(
        b"boon.compiler.unit-link-summary-input.v1\0",
        std::iter::empty(),
    ));
    for (source_unit_id, _) in &parsed_units {
        match state.unit_link_summary_requests.begin(
            &mut state.syntax_evaluator,
            source_unit_id.clone(),
            summary_input,
        )? {
            RequestStart::Reused => {}
            RequestStart::Execute(mut ticket) => {
                let summary = match state.parse_requests.require(
                    &state.syntax_evaluator,
                    &mut ticket,
                    source_unit_id,
                ) {
                    Ok(parsed) => UnitLinkSummary {
                        source_unit_id: parsed.source_unit_id.clone(),
                        declared_functions: parsed.declared_functions.clone(),
                    },
                    Err(error) => {
                        state.unit_link_summary_requests.abort(
                            &mut state.syntax_evaluator,
                            ticket,
                            RequestAbortReason::Failed,
                        )?;
                        return Err(error.into());
                    }
                };
                state.unit_link_summary_requests.publish(
                    &mut state.syntax_evaluator,
                    ticket,
                    summary,
                )?;
            }
        }
    }

    let project_namespace_key = ProjectNamespaceKey;
    let project_namespace_input = RequestInputFingerprint(project_namespace_input_fingerprint(
        parsed_units
            .iter()
            .map(|(source_unit_id, _)| source_unit_id),
    ));
    match state.project_namespace_requests.begin(
        &mut state.syntax_evaluator,
        project_namespace_key.clone(),
        project_namespace_input,
    )? {
        RequestStart::Reused => {}
        RequestStart::Execute(ticket) => {
            let namespaces = project_syntax_namespaces(
                parsed_units
                    .iter()
                    .map(|(source_unit_id, _)| source_unit_id.clone()),
            );
            let namespaces = match namespaces {
                Ok(namespaces) => Arc::new(namespaces),
                Err(error) => {
                    state.project_namespace_requests.abort(
                        &mut state.syntax_evaluator,
                        ticket,
                        RequestAbortReason::Failed,
                    )?;
                    return Err(error.into());
                }
            };
            state.project_namespace_requests.publish(
                &mut state.syntax_evaluator,
                ticket,
                namespaces,
            )?;
        }
    }

    let mut unit_modules = BTreeMap::new();
    let mut module_members = BTreeMap::<String, Vec<SourceUnitId>>::new();
    for (source_unit_id, _) in &parsed_units {
        let module =
            project_module_name_for_source_unit(&state.source.entrypoint, source_unit_id.as_str());
        if let Some(module) = &module {
            module_members
                .entry(module.clone())
                .or_default()
                .push(source_unit_id.clone());
        }
        unit_modules.insert(source_unit_id.clone(), module);
    }

    for (module, members) in &module_members {
        let module_input = RequestInputFingerprint(project_module_input_fingerprint(members));
        match state.project_module_requests.begin(
            &mut state.syntax_evaluator,
            module.clone(),
            module_input,
        )? {
            RequestStart::Reused => {}
            RequestStart::Execute(mut ticket) => {
                let module_functions = (|| -> CompilerResult<_> {
                    let mut functions = BTreeSet::new();
                    for source_unit_id in members {
                        let summary = state.unit_link_summary_requests.require(
                            &state.syntax_evaluator,
                            &mut ticket,
                            source_unit_id,
                        )?;
                        functions.extend(summary.declared_functions.iter().cloned());
                    }
                    Ok(Arc::new(functions.into_iter().collect::<Vec<_>>()))
                })();
                let module_functions = match module_functions {
                    Ok(module_functions) => module_functions,
                    Err(error) => {
                        state.project_module_requests.abort(
                            &mut state.syntax_evaluator,
                            ticket,
                            RequestAbortReason::Failed,
                        )?;
                        return Err(error);
                    }
                };
                state.project_module_requests.publish(
                    &mut state.syntax_evaluator,
                    ticket,
                    module_functions,
                )?;
            }
        }
    }

    let overlay_input_for = |module: Option<&str>| {
        RequestInputFingerprint(request_fingerprint(
            b"boon.compiler.unit-link-overlay-input.v1\0",
            module.into_iter().map(str::as_bytes),
        ))
    };
    for (source_unit_id, _) in &parsed_units {
        let module = unit_modules
            .get(source_unit_id)
            .expect("every parsed source has a module classification");
        match state.unit_link_overlay_requests.begin(
            &mut state.syntax_evaluator,
            source_unit_id.clone(),
            overlay_input_for(module.as_deref()),
        )? {
            RequestStart::Reused => {}
            RequestStart::Execute(mut ticket) => {
                let link_key = (|| -> CompilerResult<_> {
                    let namespaces = state.project_namespace_requests.require(
                        &state.syntax_evaluator,
                        &mut ticket,
                        &project_namespace_key,
                    )?;
                    let namespace = *namespaces.get(source_unit_id).ok_or_else(|| {
                        session_error(format!(
                            "compiler project source unit `{source_unit_id}` has no syntax namespace"
                        ))
                    })?;
                    let module_functions = match module {
                        Some(module) => state
                            .project_module_requests
                            .require(&state.syntax_evaluator, &mut ticket, module)?
                            .as_ref()
                            .clone(),
                        None => Vec::new(),
                    };
                    Ok(ProjectUnitLinkKey {
                        namespace,
                        module: module.clone(),
                        module_functions,
                    })
                })();
                let link_key = match link_key {
                    Ok(link_key) => link_key,
                    Err(error) => {
                        state.unit_link_overlay_requests.abort(
                            &mut state.syntax_evaluator,
                            ticket,
                            RequestAbortReason::Failed,
                        )?;
                        return Err(error);
                    }
                };
                state.unit_link_overlay_requests.publish(
                    &mut state.syntax_evaluator,
                    ticket,
                    link_key,
                )?;
            }
        }
    }

    let mut linked_units = Vec::with_capacity(parsed_units.len());
    let link_input = RequestInputFingerprint(request_fingerprint(
        b"boon.compiler.link-unit-input.v1\0",
        std::iter::empty(),
    ));
    for (source_unit_id, _) in &parsed_units {
        match state.link_requests.begin(
            &mut state.syntax_evaluator,
            source_unit_id.clone(),
            link_input,
        )? {
            RequestStart::Reused => {}
            RequestStart::Execute(mut ticket) => {
                let linked = (|| -> CompilerResult<_> {
                    let parsed = Arc::clone(state.parse_requests.require(
                        &state.syntax_evaluator,
                        &mut ticket,
                        source_unit_id,
                    )?);
                    let link_key = state.unit_link_overlay_requests.require(
                        &state.syntax_evaluator,
                        &mut ticket,
                        source_unit_id,
                    )?;
                    let (linked, profile) = link_project_source_unit_profiled(
                        parsed.as_ref().clone(),
                        link_key.clone(),
                    );
                    let linked = Arc::new(linked?);
                    Ok((linked, profile.work_counters))
                })();
                let (linked, link_work) = match linked {
                    Ok(linked) => linked,
                    Err(error) => {
                        state.link_requests.abort(
                            &mut state.syntax_evaluator,
                            ticket,
                            RequestAbortReason::Failed,
                        )?;
                        return Err(error);
                    }
                };
                work.accumulate(link_work);
                state
                    .link_requests
                    .publish(&mut state.syntax_evaluator, ticket, linked)?;
            }
        }
        let linked = Arc::clone(
            state
                .link_requests
                .current_value(&state.syntax_evaluator, source_unit_id)
                .expect("request value read occurs outside evaluation")
                .expect("link request publishes current linked unit syntax"),
        );
        linked_units.push(linked);
    }

    evaluate_owner_requests(state, &linked_units)?;

    let project =
        ProjectSyntaxSnapshot::from_unit_snapshots(&state.source.entrypoint, linked_units)?;
    Ok((project, work, started.elapsed().as_secs_f64() * 1_000.0))
}

fn evaluate_owner_requests(
    state: &mut ProjectState,
    linked_units: &[Arc<UnitSyntaxSnapshot>],
) -> CompilerResult<()> {
    let mut owners = Vec::new();
    let mut live_owners = BTreeSet::new();
    for unit in linked_units {
        for owner in unit.stable_check_owner_keys() {
            if !live_owners.insert(owner.clone()) {
                return Err(session_error(format!(
                    "compiler project has duplicate stable check owner {owner:?}"
                )));
            }
            owners.push(owner);
        }
    }
    state
        .owner_input_requests
        .retain(&mut state.syntax_evaluator, |owner| {
            live_owners.contains(owner)
        })?;
    state
        .owner_source_map_requests
        .retain(&mut state.syntax_evaluator, |owner| {
            live_owners.contains(owner)
        })?;
    state
        .owner_constraint_seed_requests
        .retain(&mut state.syntax_evaluator, |owner| {
            live_owners.contains(owner)
        })?;
    state
        .owner_constraint_requests
        .retain(&mut state.syntax_evaluator, |owner| {
            live_owners.contains(owner)
        })?;

    let owner_input = RequestInputFingerprint(request_fingerprint(
        b"boon.compiler.owner-input-dependencies.v1\0",
        std::iter::empty(),
    ));
    let source_map_input = RequestInputFingerprint(request_fingerprint(
        b"boon.compiler.owner-source-map-dependencies.v1\0",
        std::iter::empty(),
    ));
    for owner in &owners {
        match state.owner_source_map_requests.begin(
            &mut state.syntax_evaluator,
            owner.clone(),
            source_map_input,
        )? {
            RequestStart::Reused => {}
            RequestStart::Execute(mut ticket) => {
                let source_map = (|| -> CompilerResult<_> {
                    let parsed = state.parse_requests.require(
                        &state.syntax_evaluator,
                        &mut ticket,
                        owner.source_unit_id(),
                    )?;
                    let view = parsed.owner_view_for_key(owner).ok_or_else(|| {
                        session_error(format!(
                            "parsed source has no current owner view for {owner:?}"
                        ))
                    })?;
                    Ok(Arc::new(project_owner_source_map(view)?))
                })();
                let source_map = match source_map {
                    Ok(source_map) => source_map,
                    Err(error) => {
                        state.owner_source_map_requests.abort(
                            &mut state.syntax_evaluator,
                            ticket,
                            RequestAbortReason::Failed,
                        )?;
                        return Err(error);
                    }
                };
                state.owner_source_map_requests.publish(
                    &mut state.syntax_evaluator,
                    ticket,
                    source_map,
                )?;
            }
        }

        match state.owner_input_requests.begin(
            &mut state.syntax_evaluator,
            owner.clone(),
            owner_input,
        )? {
            RequestStart::Reused => {}
            RequestStart::Execute(mut ticket) => {
                let input = (|| -> CompilerResult<_> {
                    let linked = state.link_requests.require(
                        &state.syntax_evaluator,
                        &mut ticket,
                        owner.source_unit_id(),
                    )?;
                    let view = linked.owner_view_for_key(owner).ok_or_else(|| {
                        session_error(format!(
                            "linked source has no current owner view for {owner:?}"
                        ))
                    })?;
                    Ok(Arc::new(project_owner_syntax_input(view)?))
                })();
                let input = match input {
                    Ok(input) => input,
                    Err(error) => {
                        state.owner_input_requests.abort(
                            &mut state.syntax_evaluator,
                            ticket,
                            RequestAbortReason::Failed,
                        )?;
                        return Err(error);
                    }
                };
                state
                    .owner_input_requests
                    .publish(&mut state.syntax_evaluator, ticket, input)?;
            }
        }
    }
    evaluate_owner_constraint_requests(state, linked_units, &owners)?;
    Ok(())
}

fn owner_route_value_path(owner: &StableCheckOwnerKey, include_last: bool) -> Vec<String> {
    let StableCheckOwnerKey::Item(owner) = owner else {
        return Vec::new();
    };
    let segments = owner.item_route.segments();
    let retained = if include_last {
        segments
    } else {
        &segments[..segments.len().saturating_sub(1)]
    };
    let mut path = Vec::new();
    for segment in retained {
        if segment.kind == UnitItemKind::Function {
            continue;
        }
        let Some(name) = segment.names.first() else {
            continue;
        };
        if path.last() != Some(name) {
            path.push(name.clone());
        }
    }
    path
}

fn owner_parent_value_path(owner: &StableCheckOwnerKey) -> Vec<String> {
    owner_route_value_path(owner, false)
}

fn owner_symbol_priority(kind: OwnerDeclarationKind) -> Option<u8> {
    match kind {
        OwnerDeclarationKind::Field | OwnerDeclarationKind::Source | OwnerDeclarationKind::List => {
            Some(0)
        }
        OwnerDeclarationKind::Hold => Some(1),
        OwnerDeclarationKind::Function => None,
    }
}

fn add_owner_symbol(
    symbols: &mut BTreeMap<OwnerSymbolKey, Vec<OwnerSymbolCandidate>>,
    key: OwnerSymbolKey,
    candidate: OwnerSymbolCandidate,
) {
    symbols.entry(key).or_default().push(candidate);
}

fn build_project_owner_symbol_index(
    seeds: &[Arc<OwnerConstraintSeed>],
    modules: &BTreeMap<SourceUnitId, Option<String>>,
) -> ProjectOwnerSymbolIndex {
    let mut symbols = BTreeMap::<OwnerSymbolKey, Vec<OwnerSymbolCandidate>>::new();
    for seed in seeds {
        let Some(declaration) = seed
            .declarations
            .iter()
            .find(|declaration| declaration.public)
        else {
            continue;
        };
        if declaration.kind == OwnerDeclarationKind::Function {
            let Some(name) = declaration.names.first() else {
                continue;
            };
            let candidate = OwnerSymbolCandidate {
                priority: 0,
                owner: seed.owner.clone(),
                parameters: declaration.parameters.clone(),
            };
            add_owner_symbol(
                &mut symbols,
                OwnerSymbolKey {
                    namespace: OwnerSymbolNamespace::Callable,
                    parts: vec![name.clone()],
                },
                candidate.clone(),
            );
            if let Some(module) = modules
                .get(seed.owner.source_unit_id())
                .and_then(|module| module.as_ref())
            {
                add_owner_symbol(
                    &mut symbols,
                    OwnerSymbolKey {
                        namespace: OwnerSymbolNamespace::Callable,
                        parts: vec![module.clone(), name.clone()],
                    },
                    candidate,
                );
            }
            continue;
        }
        let Some(priority) = owner_symbol_priority(declaration.kind) else {
            continue;
        };
        let path = owner_route_value_path(&seed.owner, true);
        if path.is_empty() {
            continue;
        }
        add_owner_symbol(
            &mut symbols,
            OwnerSymbolKey {
                namespace: OwnerSymbolNamespace::Value,
                parts: path,
            },
            OwnerSymbolCandidate {
                priority,
                owner: seed.owner.clone(),
                parameters: Box::new([]),
            },
        );
    }
    ProjectOwnerSymbolIndex {
        symbols: symbols
            .into_iter()
            .map(|(key, mut candidates)| {
                candidates.sort();
                candidates.dedup();
                (key, candidates.into_boxed_slice())
            })
            .collect(),
    }
}

fn evaluate_owner_constraint_requests(
    state: &mut ProjectState,
    linked_units: &[Arc<UnitSyntaxSnapshot>],
    owners: &[StableCheckOwnerKey],
) -> CompilerResult<()> {
    let seed_input = RequestInputFingerprint(request_fingerprint(
        b"boon.compiler.owner-constraint-seed-dependencies.v1\0",
        std::iter::empty(),
    ));
    for owner in owners {
        match state.owner_constraint_seed_requests.begin(
            &mut state.syntax_evaluator,
            owner.clone(),
            seed_input,
        )? {
            RequestStart::Reused => {}
            RequestStart::Execute(mut ticket) => {
                let seed = (|| -> CompilerResult<_> {
                    let input = state.owner_input_requests.require(
                        &state.syntax_evaluator,
                        &mut ticket,
                        owner,
                    )?;
                    Ok(Arc::new(project_owner_constraint_seed(input)?))
                })();
                let seed = match seed {
                    Ok(seed) => seed,
                    Err(error) => {
                        state.owner_constraint_seed_requests.abort(
                            &mut state.syntax_evaluator,
                            ticket,
                            RequestAbortReason::Failed,
                        )?;
                        return Err(error);
                    }
                };
                state.owner_constraint_seed_requests.publish(
                    &mut state.syntax_evaluator,
                    ticket,
                    seed,
                )?;
            }
        }
    }

    let symbol_key = ProjectOwnerSymbolKey;
    let owner_key_fingerprints = owners
        .iter()
        .map(stable_check_owner_key_fingerprint_v1)
        .collect::<Vec<_>>();
    let symbol_input = RequestInputFingerprint(request_fingerprint(
        b"boon.compiler.project-owner-symbol-index-dependencies.v1\0",
        owner_key_fingerprints.iter().map(<[u8; 32]>::as_slice),
    ));
    match state.project_owner_symbol_requests.begin(
        &mut state.syntax_evaluator,
        symbol_key.clone(),
        symbol_input,
    )? {
        RequestStart::Reused => {}
        RequestStart::Execute(mut ticket) => {
            let symbols = (|| -> CompilerResult<_> {
                let seeds = owners
                    .iter()
                    .map(|owner| {
                        state
                            .owner_constraint_seed_requests
                            .require(&state.syntax_evaluator, &mut ticket, owner)
                            .map(Arc::clone)
                            .map_err(Into::into)
                    })
                    .collect::<CompilerResult<Vec<_>>>()?;
                let mut modules = BTreeMap::new();
                for unit in linked_units {
                    let linked = state.link_requests.require(
                        &state.syntax_evaluator,
                        &mut ticket,
                        &unit.source_unit_id,
                    )?;
                    modules.insert(
                        linked.source_unit_id.clone(),
                        linked.module().map(str::to_owned),
                    );
                }
                Ok(Arc::new(build_project_owner_symbol_index(&seeds, &modules)))
            })();
            let symbols = match symbols {
                Ok(symbols) => symbols,
                Err(error) => {
                    state.project_owner_symbol_requests.abort(
                        &mut state.syntax_evaluator,
                        ticket,
                        RequestAbortReason::Failed,
                    )?;
                    return Err(error);
                }
            };
            state.project_owner_symbol_requests.publish(
                &mut state.syntax_evaluator,
                ticket,
                symbols,
            )?;
        }
    }

    let constraint_input = RequestInputFingerprint(request_fingerprint(
        b"boon.compiler.owner-constraint-summary-dependencies.v1\0",
        std::iter::empty(),
    ));
    for owner in owners {
        match state.owner_constraint_requests.begin(
            &mut state.syntax_evaluator,
            owner.clone(),
            constraint_input,
        )? {
            RequestStart::Reused => {}
            RequestStart::Execute(mut ticket) => {
                let summary = (|| -> CompilerResult<_> {
                    let seed = Arc::clone(state.owner_constraint_seed_requests.require(
                        &state.syntax_evaluator,
                        &mut ticket,
                        owner,
                    )?);
                    let symbols = Arc::clone(state.project_owner_symbol_requests.require(
                        &state.syntax_evaluator,
                        &mut ticket,
                        &symbol_key,
                    )?);
                    let dependencies = seed.references.iter().filter_map(|reference| {
                        let candidate = symbols.resolve(owner, reference)?;
                        Some(ResolvedOwnerSymbolReference {
                            reference: reference.clone(),
                            owner: candidate.owner,
                            parameters: candidate.parameters,
                        })
                    });
                    Ok(Arc::new(resolve_owner_constraint_seed(
                        &seed,
                        dependencies,
                    )?))
                })();
                let summary = match summary {
                    Ok(summary) => summary,
                    Err(error) => {
                        state.owner_constraint_requests.abort(
                            &mut state.syntax_evaluator,
                            ticket,
                            RequestAbortReason::Failed,
                        )?;
                        return Err(error);
                    }
                };
                state.owner_constraint_requests.publish(
                    &mut state.syntax_evaluator,
                    ticket,
                    summary,
                )?;
            }
        }
    }

    let topology_key = ProjectOwnerInterfaceTopologyKey;
    let topology_input = RequestInputFingerprint(request_fingerprint(
        b"boon.compiler.project-owner-interface-topology-dependencies.v1\0",
        owner_key_fingerprints.iter().map(<[u8; 32]>::as_slice),
    ));
    match state.project_owner_interface_topology_requests.begin(
        &mut state.syntax_evaluator,
        topology_key,
        topology_input,
    )? {
        RequestStart::Reused => {}
        RequestStart::Execute(mut ticket) => {
            let topology = (|| -> CompilerResult<_> {
                let summaries = owners
                    .iter()
                    .map(|owner| {
                        state
                            .owner_constraint_requests
                            .require(&state.syntax_evaluator, &mut ticket, owner)
                            .map(Arc::clone)
                            .map_err(Into::into)
                    })
                    .collect::<CompilerResult<Vec<_>>>()?;
                Ok(Arc::new(build_owner_interface_topology(
                    summaries.iter().map(Arc::as_ref),
                )?))
            })();
            let topology = match topology {
                Ok(topology) => topology,
                Err(error) => {
                    state.project_owner_interface_topology_requests.abort(
                        &mut state.syntax_evaluator,
                        ticket,
                        RequestAbortReason::Failed,
                    )?;
                    return Err(error);
                }
            };
            state.project_owner_interface_topology_requests.publish(
                &mut state.syntax_evaluator,
                ticket,
                topology,
            )?;
        }
    }
    evaluate_owner_interface_scc_requests(state)?;
    Ok(())
}

fn evaluate_owner_interface_scc_requests(state: &mut ProjectState) -> CompilerResult<()> {
    let topology = Arc::clone(
        state
            .project_owner_interface_topology_requests
            .current_value(&state.syntax_evaluator, &ProjectOwnerInterfaceTopologyKey)?
            .ok_or_else(|| session_error("owner interface topology was not published"))?,
    );
    let live_keys = topology
        .sccs
        .iter()
        .map(|scc| scc.key.clone())
        .collect::<BTreeSet<_>>();
    state
        .owner_interface_scc_plan_requests
        .retain(&mut state.syntax_evaluator, |key| live_keys.contains(key))?;
    state
        .owner_interface_scc_requests
        .retain(&mut state.syntax_evaluator, |key| live_keys.contains(key))?;

    let plan_input = RequestInputFingerprint(request_fingerprint(
        b"boon.compiler.owner-interface-scc-plan-dependencies.v1\0",
        std::iter::empty(),
    ));
    for expected in &topology.sccs {
        match state.owner_interface_scc_plan_requests.begin(
            &mut state.syntax_evaluator,
            expected.key.clone(),
            plan_input,
        )? {
            RequestStart::Reused => {}
            RequestStart::Execute(mut ticket) => {
                let plan = (|| -> CompilerResult<_> {
                    let topology = state.project_owner_interface_topology_requests.require(
                        &state.syntax_evaluator,
                        &mut ticket,
                        &ProjectOwnerInterfaceTopologyKey,
                    )?;
                    topology
                        .sccs
                        .iter()
                        .find(|scc| scc.key == expected.key)
                        .cloned()
                        .map(Arc::new)
                        .ok_or_else(|| {
                            session_error(format!(
                                "owner interface topology has no SCC plan for {:?}",
                                expected.key
                            ))
                        })
                })();
                let plan = match plan {
                    Ok(plan) => plan,
                    Err(error) => {
                        state.owner_interface_scc_plan_requests.abort(
                            &mut state.syntax_evaluator,
                            ticket,
                            RequestAbortReason::Failed,
                        )?;
                        return Err(error);
                    }
                };
                state.owner_interface_scc_plan_requests.publish(
                    &mut state.syntax_evaluator,
                    ticket,
                    plan,
                )?;
            }
        }
    }

    let result_input = RequestInputFingerprint(request_fingerprint(
        b"boon.compiler.owner-interface-scc-result-dependencies.v1\0",
        std::iter::empty(),
    ));
    for expected in &topology.sccs {
        match state.owner_interface_scc_requests.begin(
            &mut state.syntax_evaluator,
            expected.key.clone(),
            result_input,
        )? {
            RequestStart::Reused => {}
            RequestStart::Execute(mut ticket) => {
                let result = (|| -> CompilerResult<_> {
                    let plan = Arc::clone(state.owner_interface_scc_plan_requests.require(
                        &state.syntax_evaluator,
                        &mut ticket,
                        &expected.key,
                    )?);
                    let seeds = plan
                        .key
                        .members
                        .iter()
                        .map(|owner| {
                            state
                                .owner_constraint_seed_requests
                                .require(&state.syntax_evaluator, &mut ticket, owner)
                                .map(Arc::clone)
                                .map_err(Into::into)
                        })
                        .collect::<CompilerResult<Vec<_>>>()?;
                    let summaries = plan
                        .key
                        .members
                        .iter()
                        .map(|owner| {
                            state
                                .owner_constraint_requests
                                .require(&state.syntax_evaluator, &mut ticket, owner)
                                .map(Arc::clone)
                                .map_err(Into::into)
                        })
                        .collect::<CompilerResult<Vec<_>>>()?;
                    let dependencies = plan
                        .dependencies
                        .iter()
                        .map(|dependency| {
                            state
                                .owner_interface_scc_requests
                                .require(&state.syntax_evaluator, &mut ticket, dependency)
                                .map(Arc::clone)
                                .map_err(Into::into)
                        })
                        .collect::<CompilerResult<Vec<_>>>()?;
                    Ok(Arc::new(solve_owner_interface_scc(
                        &plan,
                        seeds.iter().map(Arc::as_ref),
                        summaries.iter().map(Arc::as_ref),
                        dependencies.iter().map(Arc::as_ref),
                    )?))
                })();
                let result = match result {
                    Ok(result) => result,
                    Err(error) => {
                        state.owner_interface_scc_requests.abort(
                            &mut state.syntax_evaluator,
                            ticket,
                            RequestAbortReason::Failed,
                        )?;
                        return Err(error);
                    }
                };
                state.owner_interface_scc_requests.publish(
                    &mut state.syntax_evaluator,
                    ticket,
                    result,
                )?;
            }
        }
    }
    Ok(())
}

fn source_unit_request_fingerprint(path: &str, source: &str) -> RequestFingerprint {
    request_fingerprint(
        b"boon.compiler-session.source-unit-parse.v1\0",
        [path.as_bytes(), source.as_bytes()],
    )
}

fn source_unit_key_fingerprint(domain: &[u8], source_unit_id: &SourceUnitId) -> RequestFingerprint {
    request_fingerprint(domain, [source_unit_id.as_str().as_bytes()])
}

fn project_namespace_input_fingerprint<'a>(
    source_unit_ids: impl IntoIterator<Item = &'a SourceUnitId>,
) -> RequestFingerprint {
    let mut source_unit_ids = source_unit_ids.into_iter().collect::<Vec<_>>();
    source_unit_ids.sort();
    let mut hasher = Sha256::new();
    hasher.update(b"boon.compiler.project-namespace-plan-input.v1\0");
    hasher.update((source_unit_ids.len() as u64).to_le_bytes());
    for source_unit_id in source_unit_ids {
        update_request_fingerprint_part(&mut hasher, source_unit_id.as_str().as_bytes());
    }
    hasher.finalize().into()
}

fn project_module_input_fingerprint(source_unit_ids: &[SourceUnitId]) -> RequestFingerprint {
    let mut source_unit_ids = source_unit_ids.iter().collect::<Vec<_>>();
    source_unit_ids.sort();
    let mut hasher = Sha256::new();
    hasher.update(b"boon.compiler.project-module-index-input.v1\0");
    hasher.update((source_unit_ids.len() as u64).to_le_bytes());
    for source_unit_id in source_unit_ids {
        update_request_fingerprint_part(&mut hasher, source_unit_id.as_str().as_bytes());
    }
    hasher.finalize().into()
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
        let first_stats = session.frontend_request_stats(project).unwrap();
        assert_eq!(first_stats.demanded, 36);
        assert_eq!(first_stats.executed, 36);
        assert_eq!(first_stats.reused, 0);
        assert_eq!(first_stats.changed, 36);
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
        let second_stats = session.frontend_request_stats(project).unwrap();
        assert_eq!(second_stats.demanded, 72);
        assert_eq!(second_stats.executed, 45);
        assert_eq!(second_stats.reused, 27);
        assert_eq!(second_stats.backdated, 6);
        assert_eq!(second_stats.changed, 39);

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
    fn owner_input_backdates_semantics_independently_from_current_source_maps() {
        let mut session = CompilerSession::new();
        let project = session
            .open_project(project("left: 1\nright: left\n"))
            .unwrap();
        parse_project_snapshot(session.projects.get_mut(&project).unwrap()).unwrap();
        let unit = session
            .unit_syntax_snapshot(project, "RUN.bn")
            .unwrap()
            .unwrap();
        let owner_named = |name: &str| {
            unit.stable_check_owner_keys()
                .find(|owner| {
                    matches!(
                        owner,
                        StableCheckOwnerKey::Item(owner)
                            if owner.item_route.segments().last().is_some_and(|segment| segment.names == [name])
                    )
                })
                .unwrap()
        };
        let root = StableCheckOwnerKey::UnitRoot(SourceUnitId::from_path("RUN.bn").unwrap());
        let left = owner_named("left");
        let right = owner_named("right");
        let first_root_input = session.owner_syntax_input(project, &root).unwrap().unwrap();
        let first_left_input = session.owner_syntax_input(project, &left).unwrap().unwrap();
        let first_right_input = session
            .owner_syntax_input(project, &right)
            .unwrap()
            .unwrap();
        let first_root_map = session.owner_source_map(project, &root).unwrap().unwrap();
        let first_left_map = session.owner_source_map(project, &left).unwrap().unwrap();
        let first_right_map = session.owner_source_map(project, &right).unwrap().unwrap();
        let first_left_seed = session
            .owner_constraint_seed(project, &left)
            .unwrap()
            .unwrap();
        let first_right_seed = session
            .owner_constraint_seed(project, &right)
            .unwrap()
            .unwrap();
        let first_left_summary = session
            .owner_constraint_summary(project, &left)
            .unwrap()
            .unwrap();
        let first_right_summary = session
            .owner_constraint_summary(project, &right)
            .unwrap()
            .unwrap();
        let first_topology = session.owner_interface_topology(project).unwrap().unwrap();
        let first_left_interface = session
            .owner_interface_result(project, &left)
            .unwrap()
            .unwrap();
        let first_right_interface = session
            .owner_interface_result(project, &right)
            .unwrap()
            .unwrap();
        assert_eq!(first_right_summary.dependencies.len(), 1);
        assert_eq!(first_right_summary.dependencies[0].request, right);
        assert_eq!(first_right_summary.dependencies[0].dependency, left);
        assert_eq!(
            first_right_summary.dependencies[0].kind,
            OwnerConstraintDependencyKind::ValueRead
        );

        session
            .apply_update(
                project,
                UnitUpdate::new("RUN.bn", "left: 100\nright: left\n"),
            )
            .unwrap();
        parse_project_snapshot(session.projects.get_mut(&project).unwrap()).unwrap();

        let second_root_input = session.owner_syntax_input(project, &root).unwrap().unwrap();
        let second_left_input = session.owner_syntax_input(project, &left).unwrap().unwrap();
        let second_right_input = session
            .owner_syntax_input(project, &right)
            .unwrap()
            .unwrap();
        let second_root_map = session.owner_source_map(project, &root).unwrap().unwrap();
        let second_left_map = session.owner_source_map(project, &left).unwrap().unwrap();
        let second_right_map = session.owner_source_map(project, &right).unwrap().unwrap();
        let second_left_seed = session
            .owner_constraint_seed(project, &left)
            .unwrap()
            .unwrap();
        let second_right_seed = session
            .owner_constraint_seed(project, &right)
            .unwrap()
            .unwrap();
        let second_left_summary = session
            .owner_constraint_summary(project, &left)
            .unwrap()
            .unwrap();
        let second_right_summary = session
            .owner_constraint_summary(project, &right)
            .unwrap()
            .unwrap();
        let second_topology = session.owner_interface_topology(project).unwrap().unwrap();
        let second_left_interface = session
            .owner_interface_result(project, &left)
            .unwrap()
            .unwrap();
        let second_right_interface = session
            .owner_interface_result(project, &right)
            .unwrap()
            .unwrap();

        assert!(Arc::ptr_eq(&first_root_input, &second_root_input));
        assert!(!Arc::ptr_eq(&first_left_input, &second_left_input));
        assert!(Arc::ptr_eq(&first_right_input, &second_right_input));
        assert!(Arc::ptr_eq(&first_root_map, &second_root_map));
        assert!(!Arc::ptr_eq(&first_left_map, &second_left_map));
        assert!(!Arc::ptr_eq(&first_right_map, &second_right_map));
        assert!(Arc::ptr_eq(&first_left_seed, &second_left_seed));
        assert!(Arc::ptr_eq(&first_right_seed, &second_right_seed));
        assert!(Arc::ptr_eq(&first_left_summary, &second_left_summary));
        assert!(Arc::ptr_eq(&first_right_summary, &second_right_summary));
        assert!(Arc::ptr_eq(&first_topology, &second_topology));
        assert!(Arc::ptr_eq(&first_left_interface, &second_left_interface));
        assert!(Arc::ptr_eq(&first_right_interface, &second_right_interface));

        let state = session.projects.get(&project).unwrap();
        assert_eq!(
            state
                .owner_input_requests
                .memo(&state.syntax_evaluator, &left)
                .unwrap()
                .changed_at,
            EvaluationRevision(1)
        );
        assert_eq!(
            state
                .owner_input_requests
                .memo(&state.syntax_evaluator, &right)
                .unwrap()
                .changed_at,
            EvaluationRevision(0)
        );
        assert_eq!(
            state
                .owner_source_map_requests
                .memo(&state.syntax_evaluator, &right)
                .unwrap()
                .changed_at,
            EvaluationRevision(1)
        );
        assert_eq!(
            state
                .owner_constraint_seed_requests
                .memo(&state.syntax_evaluator, &left)
                .unwrap()
                .changed_at,
            EvaluationRevision(0)
        );
    }

    #[test]
    fn body_edit_backdates_an_unchanged_alpha_normalized_interface() {
        let before = "FUNCTION identity(input) {\n    input\n}\n";
        let after = concat!(
            "FUNCTION identity(input) {\n",
            "    BLOCK {\n",
            "        result: input\n",
            "        result\n",
            "    }\n",
            "}\n",
        );
        let mut session = CompilerSession::new();
        let project = session.open_project(project(before)).unwrap();
        parse_project_snapshot(session.projects.get_mut(&project).unwrap()).unwrap();
        let unit = session
            .unit_syntax_snapshot(project, "RUN.bn")
            .unwrap()
            .unwrap();
        let owner = unit
            .stable_check_owner_keys()
            .find(|owner| {
                matches!(
                    owner,
                    StableCheckOwnerKey::Item(owner)
                        if owner.item_route.segments().last().is_some_and(|segment| segment.names == ["identity"])
                )
            })
            .unwrap();
        let first_seed = session
            .owner_constraint_seed(project, &owner)
            .unwrap()
            .unwrap();
        let first_interface = session
            .owner_interface_result(project, &owner)
            .unwrap()
            .unwrap();
        let first_owner = first_interface.owner(&owner).unwrap();
        assert_eq!(
            first_owner.parameters[0].flow_type.ty,
            boon_checked::Type::Var(boon_checked::TypeVar(0))
        );
        assert_eq!(
            first_owner.result.ty,
            boon_checked::Type::Var(boon_checked::TypeVar(0))
        );

        session
            .apply_update(project, UnitUpdate::new("RUN.bn", after))
            .unwrap();
        parse_project_snapshot(session.projects.get_mut(&project).unwrap()).unwrap();
        let second_seed = session
            .owner_constraint_seed(project, &owner)
            .unwrap()
            .unwrap();
        let second_interface = session
            .owner_interface_result(project, &owner)
            .unwrap()
            .unwrap();
        assert!(!Arc::ptr_eq(&first_seed, &second_seed));
        assert!(Arc::ptr_eq(&first_interface, &second_interface));
        let state = session.projects.get(&project).unwrap();
        let key = second_interface.key.clone();
        let memo = state
            .owner_interface_scc_requests
            .memo(&state.syntax_evaluator, &key)
            .unwrap();
        assert_eq!(memo.changed_at, EvaluationRevision(0));
        assert_eq!(memo.verified_at, EvaluationRevision(1));
    }

    #[test]
    fn module_interface_changes_relink_only_that_module_while_body_edits_stay_local() {
        let first_source = "FUNCTION first(input) {\n    input\n}\n";
        let first_with_export = concat!(
            "FUNCTION first(input) {\n    input\n}\n",
            "FUNCTION third(input) {\n    input\n}\n",
        );
        let first_body_edit = concat!(
            "FUNCTION first(input) {\n    input + 0\n}\n",
            "FUNCTION third(input) {\n    input\n}\n",
        );
        let mut session = CompilerSession::new();
        let project = session
            .open_project(CompilerProject::new(
                "RUN.bn",
                vec![
                    CompilerSourceUnit {
                        path: "left/Math.bn".to_owned(),
                        source: first_source.to_owned(),
                    },
                    CompilerSourceUnit {
                        path: "right/Math.bn".to_owned(),
                        source: "FUNCTION second(input) {\n    input\n}\n".to_owned(),
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
        parse_project_snapshot(session.projects.get_mut(&project).unwrap()).unwrap();
        let original_math = session
            .unit_syntax_snapshot(project, "right/Math.bn")
            .unwrap()
            .unwrap();
        let original_run = session
            .unit_syntax_snapshot(project, "RUN.bn")
            .unwrap()
            .unwrap();
        assert_eq!(
            original_math.link_key().module_functions,
            ["first", "second"]
        );
        let cold_topology = session.owner_interface_topology(project).unwrap().unwrap();
        let cold_stats = session.frontend_request_stats(project).unwrap();

        session
            .apply_update(project, UnitUpdate::new("left/Math.bn", first_with_export))
            .unwrap();
        parse_project_snapshot(session.projects.get_mut(&project).unwrap()).unwrap();
        let exported_math = session
            .unit_syntax_snapshot(project, "right/Math.bn")
            .unwrap()
            .unwrap();
        let retained_run = session
            .unit_syntax_snapshot(project, "RUN.bn")
            .unwrap()
            .unwrap();
        assert_eq!(
            exported_math.link_key().module_functions,
            ["first", "second", "third"]
        );
        assert!(!Arc::ptr_eq(&original_math, &exported_math));
        assert!(Arc::ptr_eq(&original_run, &retained_run));
        let interface_topology = session.owner_interface_topology(project).unwrap().unwrap();
        assert_eq!(
            interface_topology.stats.nodes,
            cold_topology.stats.nodes + 1
        );
        let interface_stats = session.frontend_request_stats(project).unwrap();
        assert_eq!(interface_stats.demanded - cold_stats.demanded, 58);
        assert_eq!(interface_stats.executed - cold_stats.executed, 34);
        assert_eq!(interface_stats.reused - cold_stats.reused, 24);
        assert_eq!(interface_stats.backdated - cold_stats.backdated, 18);
        assert_eq!(interface_stats.changed - cold_stats.changed, 16);

        session
            .apply_update(project, UnitUpdate::new("left/Math.bn", first_body_edit))
            .unwrap();
        parse_project_snapshot(session.projects.get_mut(&project).unwrap()).unwrap();
        let body_edit_math = session
            .unit_syntax_snapshot(project, "right/Math.bn")
            .unwrap()
            .unwrap();
        let body_edit_run = session
            .unit_syntax_snapshot(project, "RUN.bn")
            .unwrap()
            .unwrap();
        assert!(Arc::ptr_eq(&exported_math, &body_edit_math));
        assert!(Arc::ptr_eq(&retained_run, &body_edit_run));
        let body_topology = session.owner_interface_topology(project).unwrap().unwrap();
        assert!(Arc::ptr_eq(&interface_topology, &body_topology));
        let body_stats = session.frontend_request_stats(project).unwrap();
        assert_eq!(body_stats.demanded - interface_stats.demanded, 58);
        assert_eq!(body_stats.executed - interface_stats.executed, 14);
        assert_eq!(body_stats.reused - interface_stats.reused, 44);
        assert_eq!(body_stats.backdated - interface_stats.backdated, 6);
        assert_eq!(body_stats.changed - interface_stats.changed, 8);
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
