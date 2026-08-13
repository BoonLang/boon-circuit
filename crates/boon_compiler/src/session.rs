use crate::{
    CheckedCompileRequest, CheckedSourceFromSource, CompiledSealedMachinePlanFromSource,
    CompilerDiagnostics, CompilerResult, CompilerSourceUnit, checked_source_from_owner_assembly,
    compiler_diagnostics_from_owner_aggregate, finish_checked_machine_plan_with_cancellation,
};
use boon_compilation_db::{
    RequestAbortReason, RequestEvaluationStats, RequestEvaluatorGraph, RequestFamily,
    RequestFingerprint, RequestInputFingerprint, RequestOutputFingerprint, RequestStart,
    Revision as EvaluationRevision, TypedRequestTable,
};
use boon_parser::{
    ParseWorkCounters, ParsedSourceUnit, ProjectSourceUnitLayout, ProjectSyntaxSnapshot,
    ProjectUnitLinkKey, UnitSyntaxSnapshot, link_project_source_unit_profiled,
    parse_project_source_unit_profiled, project_module_name_for_source_unit,
    project_syntax_namespaces,
};
use boon_plan::{
    ApplicationIdentity, MigrationPredecessorBinding, PlanError, ProgramRole, TargetProfile,
};
use boon_syntax::StableCheckOwnerKey;
use boon_syntax::{SourceUnitId, SyntaxUnitNamespace, UnitItemKind};
#[cfg(test)]
use boon_typecheck::OwnerConstraintDependencyKind;
use boon_typecheck::{
    AmbiguousOwnerSymbolCandidate, CheckedOwnerProjectAssembly, CheckedOwnerShard,
    OwnerAbiEnvironment, OwnerBodyInferenceEvaluation, OwnerBodyInferenceShard,
    OwnerBodyInterfacePlanner, OwnerCallableAbiEnvironment, OwnerCallableAbiLookup,
    OwnerCallableAbiLookupOutcome, OwnerCallableResolutionPlan, OwnerCallableScopeScc,
    OwnerCallableScopeSccEvaluation, OwnerCallableScopeSccKey, OwnerCallableScopeSccResult,
    OwnerCallableScopeTopology, OwnerConstraintSeed, OwnerConstraintSummary,
    OwnerConstructionAbiEnvironment, OwnerConstructionCallableAbiLookup,
    OwnerConstructionValueAbiLookup, OwnerDeclarationKind, OwnerDeclarationSurface,
    OwnerDiagnosticsAggregate, OwnerInferenceAbiEnvironment, OwnerInterfaceScc,
    OwnerInterfaceSccCurrentnessReceipt, OwnerInterfaceSccKey, OwnerInterfaceSccResult,
    OwnerInterfaceTopology, OwnerInterfaceTransferModule, OwnerLexicalPlan,
    OwnerParameterRequirementKey, OwnerParameterRequirementLookup, OwnerReferenceKind,
    OwnerSourceMap, OwnerSourcePayloadAbiLookup, OwnerSymbolReference, OwnerSymbolResolution,
    OwnerSyntaxInput, OwnerValueAbiLookup, ProjectDiagnosticFacts, ProjectOutputFlowFacts,
    SourceUnitOwnerDiagnostics, SourceUnitProjectDiagnostics,
    SourceUnitProjectDiagnosticsEvaluation, aggregate_source_unit_diagnostics,
    assemble_checked_owner_project, build_checked_owner_shard, build_owner_callable_scope_topology,
    build_owner_interface_topology, evaluate_owner_body_with_signature_plan,
    evaluate_owner_callable_scope_scc, evaluate_owner_interface_scc_component,
    evaluate_source_unit_project_diagnostics, project_diagnostic_facts, project_output_flow_facts,
    project_owner_abi_environment, project_owner_callable_resolution_plan,
    project_owner_constraint_seed_with_lexical_plan, project_owner_declaration_surface,
    project_owner_lexical_plan, project_owner_source_map, project_owner_syntax_input,
    project_source_unit_owner_diagnostics, resolve_owner_constraint_seed_with_signature_plan,
    stable_check_owner_key_fingerprint_v2,
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
    /// Complete checked rows and editor-facing type/presentation tables without
    /// sealing an executable artifact. Callers must opt into this larger root.
    EditorDiagnostics,
    VerifiedCheck,
    VerifiedPreview,
    Handoff,
}

struct OwnerRequestTrace {
    enabled: bool,
    started: Instant,
    phase_started: Instant,
}

impl OwnerRequestTrace {
    fn new() -> Self {
        let now = Instant::now();
        Self {
            enabled: std::env::var_os("BOON_OWNER_REQUEST_TRACE").is_some(),
            started: now,
            phase_started: now,
        }
    }

    fn checkpoint(&mut self, phase: &str, items: usize) {
        let now = Instant::now();
        if self.enabled {
            eprintln!(
                "boon owner requests phase={phase} items={items} phase_ms={:.3} total_ms={:.3}",
                now.duration_since(self.phase_started).as_secs_f64() * 1_000.0,
                now.duration_since(self.started).as_secs_f64() * 1_000.0,
            );
        }
        self.phase_started = now;
    }
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
    Diagnostics(&'a CompilerDiagnostics),
    EditorDiagnostics(&'a CheckedSourceFromSource),
    Verified {
        intent: CompileIntent,
        compiled: &'a CompiledSealedMachinePlanFromSource,
    },
}

impl CompilerSessionResult<'_> {
    pub fn diagnostics(&self) -> Option<&CompilerDiagnostics> {
        match self {
            Self::Diagnostics(diagnostics) => Some(diagnostics),
            Self::EditorDiagnostics(_) | Self::Verified { .. } => None,
        }
    }

    pub fn editor_diagnostics(&self) -> Option<&CheckedSourceFromSource> {
        match self {
            Self::EditorDiagnostics(checked) => Some(checked),
            Self::Diagnostics(_) | Self::Verified { .. } => None,
        }
    }

    pub fn compiled(&self) -> Option<&CompiledSealedMachinePlanFromSource> {
        match self {
            Self::Diagnostics(_) | Self::EditorDiagnostics(_) => None,
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
    owner_declaration_surface_requests: TypedRequestTable<OwnerDeclarationSurfaceRequest>,
    owner_lexical_plan_requests: TypedRequestTable<OwnerLexicalPlanRequest>,
    owner_constraint_seed_requests: TypedRequestTable<OwnerConstraintSeedRequest>,
    project_owner_abi_requests: TypedRequestTable<ProjectOwnerAbiRequest>,
    project_owner_callable_abi_requests: TypedRequestTable<ProjectOwnerCallableAbiRequest>,
    owner_callable_abi_lookup_requests: TypedRequestTable<OwnerCallableAbiLookupRequest>,
    owner_value_abi_lookup_requests: TypedRequestTable<OwnerValueAbiLookupRequest>,
    owner_source_payload_abi_lookup_requests: TypedRequestTable<OwnerSourcePayloadAbiLookupRequest>,
    owner_parameter_requirement_lookup_requests:
        TypedRequestTable<OwnerParameterRequirementLookupRequest>,
    owner_callable_inference_abi_requests: TypedRequestTable<OwnerCallableInferenceAbiRequest>,
    owner_inference_abi_requests: TypedRequestTable<OwnerInferenceAbiRequest>,
    owner_construction_callable_abi_lookup_requests:
        TypedRequestTable<OwnerConstructionCallableAbiLookupRequest>,
    owner_construction_value_abi_lookup_requests:
        TypedRequestTable<OwnerConstructionValueAbiLookupRequest>,
    owner_construction_abi_requests: TypedRequestTable<OwnerConstructionAbiRequest>,
    project_owner_symbol_requests: TypedRequestTable<ProjectOwnerSymbolRequest>,
    owner_callable_resolution_requests: TypedRequestTable<OwnerCallableResolutionRequest>,
    project_owner_callable_scope_topology_requests:
        TypedRequestTable<ProjectOwnerCallableScopeTopologyRequest>,
    owner_callable_scope_scc_plan_requests: TypedRequestTable<OwnerCallableScopeSccPlanRequest>,
    owner_callable_scope_scc_evaluation_requests:
        TypedRequestTable<OwnerCallableScopeSccEvaluationRequest>,
    owner_callable_scope_scc_requests: TypedRequestTable<OwnerCallableScopeSccRequest>,
    owner_callable_scope_provider_requests: TypedRequestTable<OwnerCallableScopeProviderRequest>,
    owner_constraint_requests: TypedRequestTable<OwnerConstraintRequest>,
    project_owner_interface_topology_requests:
        TypedRequestTable<ProjectOwnerInterfaceTopologyRequest>,
    owner_interface_scc_plan_requests: TypedRequestTable<OwnerInterfaceSccPlanRequest>,
    owner_interface_scc_evaluation_requests: TypedRequestTable<OwnerInterfaceSccEvaluationRequest>,
    owner_interface_scc_requests: TypedRequestTable<OwnerInterfaceSccRequest>,
    owner_interface_transfer_module_requests:
        TypedRequestTable<OwnerInterfaceTransferModuleRequest>,
    owner_interface_provider_requests: TypedRequestTable<OwnerInterfaceProviderRequest>,
    owner_body_inference_evaluation_requests:
        TypedRequestTable<OwnerBodyInferenceEvaluationRequest>,
    owner_body_inference_requests: TypedRequestTable<OwnerBodyInferenceRequest>,
    project_output_flow_facts_requests: TypedRequestTable<ProjectOutputFlowFactsRequest>,
    project_diagnostic_facts_requests: TypedRequestTable<ProjectDiagnosticFactsRequest>,
    source_unit_owner_diagnostics_requests: TypedRequestTable<SourceUnitOwnerDiagnosticsRequest>,
    source_unit_project_diagnostics_evaluation_requests:
        TypedRequestTable<SourceUnitProjectDiagnosticsEvaluationRequest>,
    source_unit_project_diagnostics_requests:
        TypedRequestTable<SourceUnitProjectDiagnosticsRequest>,
    owner_diagnostics_aggregate_requests: TypedRequestTable<OwnerDiagnosticsAggregateRequest>,
    checked_owner_shard_requests: TypedRequestTable<CheckedOwnerShardRequest>,
    checked_owner_project_assembly_requests: TypedRequestTable<CheckedOwnerProjectAssemblyRequest>,
    diagnostics: Option<CompilerDiagnostics>,
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

    const NAME: &'static str = "boon.compiler.owner-input.v4";

    fn key_fingerprint(key: &Self::Key) -> RequestFingerprint {
        stable_check_owner_key_fingerprint_v2(key)
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

    const NAME: &'static str = "boon.compiler.owner-source-map.v3";

    fn key_fingerprint(key: &Self::Key) -> RequestFingerprint {
        stable_check_owner_key_fingerprint_v2(key)
    }

    fn output_fingerprint(
        value: &Self::Value,
    ) -> Result<RequestOutputFingerprint, boon_compilation_db::CompilationDbError> {
        Ok(RequestOutputFingerprint(value.fingerprint_v2()))
    }
}

struct OwnerConstraintSeedRequest;

struct OwnerDeclarationSurfaceRequest;

impl RequestFamily for OwnerDeclarationSurfaceRequest {
    type Key = StableCheckOwnerKey;
    type Value = Arc<OwnerDeclarationSurface>;

    const NAME: &'static str = "boon.compiler.owner-declaration-surface.v1";

    fn key_fingerprint(key: &Self::Key) -> RequestFingerprint {
        stable_check_owner_key_fingerprint_v2(key)
    }

    fn output_fingerprint(
        value: &Self::Value,
    ) -> Result<RequestOutputFingerprint, boon_compilation_db::CompilationDbError> {
        Ok(RequestOutputFingerprint(value.fingerprint_v1()))
    }
}

struct OwnerLexicalPlanRequest;

impl RequestFamily for OwnerLexicalPlanRequest {
    type Key = StableCheckOwnerKey;
    type Value = Arc<OwnerLexicalPlan>;

    const NAME: &'static str = "boon.compiler.owner-lexical-plan.v4";

    fn key_fingerprint(key: &Self::Key) -> RequestFingerprint {
        stable_check_owner_key_fingerprint_v2(key)
    }

    fn output_fingerprint(
        value: &Self::Value,
    ) -> Result<RequestOutputFingerprint, boon_compilation_db::CompilationDbError> {
        Ok(RequestOutputFingerprint(value.fingerprint_v1()))
    }
}

impl RequestFamily for OwnerConstraintSeedRequest {
    type Key = StableCheckOwnerKey;
    type Value = Arc<OwnerConstraintSeed>;

    const NAME: &'static str = "boon.compiler.owner-constraint-seed.v6";

    fn key_fingerprint(key: &Self::Key) -> RequestFingerprint {
        stable_check_owner_key_fingerprint_v2(key)
    }

    fn output_fingerprint(
        value: &Self::Value,
    ) -> Result<RequestOutputFingerprint, boon_compilation_db::CompilationDbError> {
        Ok(RequestOutputFingerprint(value.fingerprint_v1()))
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct ProjectOwnerAbiKey;

struct ProjectOwnerAbiRequest;

impl RequestFamily for ProjectOwnerAbiRequest {
    type Key = ProjectOwnerAbiKey;
    type Value = Arc<OwnerAbiEnvironment>;

    const NAME: &'static str = "boon.compiler.project-owner-abi.v2";

    fn key_fingerprint(_key: &Self::Key) -> RequestFingerprint {
        request_fingerprint(
            b"boon.compiler.project-owner-abi-key.v1\0",
            std::iter::empty(),
        )
    }

    fn output_fingerprint(
        value: &Self::Value,
    ) -> Result<RequestOutputFingerprint, boon_compilation_db::CompilationDbError> {
        Ok(RequestOutputFingerprint(value.fingerprint_v1()))
    }
}

struct ProjectOwnerCallableAbiRequest;

impl RequestFamily for ProjectOwnerCallableAbiRequest {
    type Key = ProjectOwnerAbiKey;
    type Value = Arc<OwnerCallableAbiEnvironment>;

    const NAME: &'static str = "boon.compiler.project-owner-callable-abi.v2";

    fn key_fingerprint(_key: &Self::Key) -> RequestFingerprint {
        request_fingerprint(
            b"boon.compiler.project-owner-callable-abi-key.v1\0",
            std::iter::empty(),
        )
    }

    fn output_fingerprint(
        value: &Self::Value,
    ) -> Result<RequestOutputFingerprint, boon_compilation_db::CompilationDbError> {
        Ok(RequestOutputFingerprint(value.fingerprint_v1()))
    }
}

struct OwnerCallableAbiLookupRequest;

impl RequestFamily for OwnerCallableAbiLookupRequest {
    type Key = String;
    type Value = Arc<OwnerCallableAbiLookup>;

    const NAME: &'static str = "boon.compiler.owner-callable-abi-lookup.v3";

    fn key_fingerprint(key: &Self::Key) -> RequestFingerprint {
        request_fingerprint(
            b"boon.compiler.owner-callable-abi-lookup-key.v1\0",
            [key.as_bytes()],
        )
    }

    fn output_fingerprint(
        value: &Self::Value,
    ) -> Result<RequestOutputFingerprint, boon_compilation_db::CompilationDbError> {
        Ok(RequestOutputFingerprint(value.fingerprint_v1()))
    }
}

struct OwnerValueAbiLookupRequest;

impl RequestFamily for OwnerValueAbiLookupRequest {
    type Key = String;
    type Value = Arc<OwnerValueAbiLookup>;

    const NAME: &'static str = "boon.compiler.owner-value-abi-lookup.v1";

    fn key_fingerprint(key: &Self::Key) -> RequestFingerprint {
        request_fingerprint(
            b"boon.compiler.owner-value-abi-lookup-key.v1\0",
            [key.as_bytes()],
        )
    }

    fn output_fingerprint(
        value: &Self::Value,
    ) -> Result<RequestOutputFingerprint, boon_compilation_db::CompilationDbError> {
        Ok(RequestOutputFingerprint(value.fingerprint_v1()))
    }
}

struct OwnerSourcePayloadAbiLookupRequest;

impl RequestFamily for OwnerSourcePayloadAbiLookupRequest {
    type Key = String;
    type Value = Arc<OwnerSourcePayloadAbiLookup>;

    const NAME: &'static str = "boon.compiler.owner-source-payload-abi-lookup.v1";

    fn key_fingerprint(key: &Self::Key) -> RequestFingerprint {
        request_fingerprint(
            b"boon.compiler.owner-source-payload-abi-lookup-key.v1\0",
            [key.as_bytes()],
        )
    }

    fn output_fingerprint(
        value: &Self::Value,
    ) -> Result<RequestOutputFingerprint, boon_compilation_db::CompilationDbError> {
        Ok(RequestOutputFingerprint(value.fingerprint_v1()))
    }
}

struct OwnerParameterRequirementLookupRequest;

impl RequestFamily for OwnerParameterRequirementLookupRequest {
    type Key = OwnerParameterRequirementKey;
    type Value = Arc<OwnerParameterRequirementLookup>;

    const NAME: &'static str = "boon.compiler.owner-parameter-requirement-lookup.v1";

    fn key_fingerprint(key: &Self::Key) -> RequestFingerprint {
        request_fingerprint(
            b"boon.compiler.owner-parameter-requirement-lookup-key.v1\0",
            [
                stable_check_owner_key_fingerprint_v2(key.owner()).as_slice(),
                key.parameter_ordinal().to_le_bytes().as_slice(),
            ],
        )
    }

    fn output_fingerprint(
        value: &Self::Value,
    ) -> Result<RequestOutputFingerprint, boon_compilation_db::CompilationDbError> {
        Ok(RequestOutputFingerprint(value.fingerprint_v1()))
    }
}

struct OwnerCallableInferenceAbiRequest;

impl RequestFamily for OwnerCallableInferenceAbiRequest {
    type Key = StableCheckOwnerKey;
    type Value = Arc<OwnerInferenceAbiEnvironment>;

    const NAME: &'static str = "boon.compiler.owner-callable-inference-abi.v2";

    fn key_fingerprint(key: &Self::Key) -> RequestFingerprint {
        stable_check_owner_key_fingerprint_v2(key)
    }

    fn output_fingerprint(
        value: &Self::Value,
    ) -> Result<RequestOutputFingerprint, boon_compilation_db::CompilationDbError> {
        Ok(RequestOutputFingerprint(value.fingerprint_v1()))
    }
}

struct OwnerInferenceAbiRequest;

impl RequestFamily for OwnerInferenceAbiRequest {
    type Key = StableCheckOwnerKey;
    type Value = Arc<OwnerInferenceAbiEnvironment>;

    const NAME: &'static str = "boon.compiler.owner-inference-abi.v7";

    fn key_fingerprint(key: &Self::Key) -> RequestFingerprint {
        stable_check_owner_key_fingerprint_v2(key)
    }

    fn output_fingerprint(
        value: &Self::Value,
    ) -> Result<RequestOutputFingerprint, boon_compilation_db::CompilationDbError> {
        Ok(RequestOutputFingerprint(value.fingerprint_v1()))
    }
}

struct OwnerCallableResolutionRequest;

impl RequestFamily for OwnerCallableResolutionRequest {
    type Key = StableCheckOwnerKey;
    type Value = Arc<OwnerCallableResolutionPlan>;

    const NAME: &'static str = "boon.compiler.owner-callable-resolution.v2";

    fn key_fingerprint(key: &Self::Key) -> RequestFingerprint {
        stable_check_owner_key_fingerprint_v2(key)
    }

    fn output_fingerprint(
        value: &Self::Value,
    ) -> Result<RequestOutputFingerprint, boon_compilation_db::CompilationDbError> {
        Ok(RequestOutputFingerprint(value.fingerprint_v1()))
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct ProjectOwnerCallableScopeTopologyKey;

struct ProjectOwnerCallableScopeTopologyRequest;

impl RequestFamily for ProjectOwnerCallableScopeTopologyRequest {
    type Key = ProjectOwnerCallableScopeTopologyKey;
    type Value = Arc<OwnerCallableScopeTopology>;

    const NAME: &'static str = "boon.compiler.project-owner-callable-scope-topology.v2";

    fn key_fingerprint(_key: &Self::Key) -> RequestFingerprint {
        request_fingerprint(
            b"boon.compiler.project-owner-callable-scope-topology-key.v1\0",
            std::iter::empty(),
        )
    }

    fn output_fingerprint(
        value: &Self::Value,
    ) -> Result<RequestOutputFingerprint, boon_compilation_db::CompilationDbError> {
        Ok(RequestOutputFingerprint(value.fingerprint_v1()))
    }
}

struct OwnerCallableScopeSccPlanRequest;

impl RequestFamily for OwnerCallableScopeSccPlanRequest {
    type Key = OwnerCallableScopeSccKey;
    type Value = Arc<OwnerCallableScopeScc>;

    const NAME: &'static str = "boon.compiler.owner-callable-scope-scc-plan.v2";

    fn key_fingerprint(key: &Self::Key) -> RequestFingerprint {
        let fingerprints = key
            .members
            .iter()
            .map(stable_check_owner_key_fingerprint_v2)
            .collect::<Vec<_>>();
        request_fingerprint(
            b"boon.compiler.owner-callable-scope-scc-key.v1\0",
            fingerprints.iter().map(<[u8; 32]>::as_slice),
        )
    }

    fn output_fingerprint(
        value: &Self::Value,
    ) -> Result<RequestOutputFingerprint, boon_compilation_db::CompilationDbError> {
        Ok(RequestOutputFingerprint(value.fingerprint_v1()))
    }
}

struct OwnerCallableScopeSccEvaluationRequest;

impl RequestFamily for OwnerCallableScopeSccEvaluationRequest {
    type Key = OwnerCallableScopeSccKey;
    type Value = Arc<OwnerCallableScopeSccEvaluation>;

    const NAME: &'static str = "boon.compiler.owner-callable-scope-scc-evaluation.v3";

    fn key_fingerprint(key: &Self::Key) -> RequestFingerprint {
        OwnerCallableScopeSccPlanRequest::key_fingerprint(key)
    }

    fn output_fingerprint(
        value: &Self::Value,
    ) -> Result<RequestOutputFingerprint, boon_compilation_db::CompilationDbError> {
        Ok(RequestOutputFingerprint(value.currentness.fingerprint_v1()))
    }
}

struct OwnerCallableScopeSccRequest;

impl RequestFamily for OwnerCallableScopeSccRequest {
    type Key = OwnerCallableScopeSccKey;
    type Value = Arc<OwnerCallableScopeSccResult>;

    const NAME: &'static str = "boon.compiler.owner-callable-scope-scc-result.v3";

    fn key_fingerprint(key: &Self::Key) -> RequestFingerprint {
        OwnerCallableScopeSccPlanRequest::key_fingerprint(key)
    }

    fn output_fingerprint(
        value: &Self::Value,
    ) -> Result<RequestOutputFingerprint, boon_compilation_db::CompilationDbError> {
        Ok(RequestOutputFingerprint(value.fingerprint_v1()))
    }
}

struct OwnerCallableScopeProviderRequest;

#[derive(Clone, Debug, Eq, PartialEq)]
struct OwnerCallableScopeProvider {
    key: Arc<OwnerCallableScopeSccKey>,
    fingerprint: RequestFingerprint,
}

impl OwnerCallableScopeProvider {
    fn key(&self) -> &OwnerCallableScopeSccKey {
        &self.key
    }
}

impl RequestFamily for OwnerCallableScopeProviderRequest {
    type Key = StableCheckOwnerKey;
    type Value = Arc<OwnerCallableScopeProvider>;

    const NAME: &'static str = "boon.compiler.owner-callable-scope-provider.v3";

    fn key_fingerprint(key: &Self::Key) -> RequestFingerprint {
        stable_check_owner_key_fingerprint_v2(key)
    }

    fn output_fingerprint(
        value: &Self::Value,
    ) -> Result<RequestOutputFingerprint, boon_compilation_db::CompilationDbError> {
        Ok(RequestOutputFingerprint(value.fingerprint))
    }
}

struct OwnerConstructionCallableAbiLookupRequest;

impl RequestFamily for OwnerConstructionCallableAbiLookupRequest {
    type Key = String;
    type Value = Arc<OwnerConstructionCallableAbiLookup>;

    const NAME: &'static str = "boon.compiler.owner-construction-callable-abi-lookup.v2";

    fn key_fingerprint(key: &Self::Key) -> RequestFingerprint {
        request_fingerprint(
            b"boon.compiler.owner-construction-callable-abi-lookup-key.v1\0",
            [key.as_bytes()],
        )
    }

    fn output_fingerprint(
        value: &Self::Value,
    ) -> Result<RequestOutputFingerprint, boon_compilation_db::CompilationDbError> {
        Ok(RequestOutputFingerprint(value.fingerprint_v1()))
    }
}

struct OwnerConstructionValueAbiLookupRequest;

impl RequestFamily for OwnerConstructionValueAbiLookupRequest {
    type Key = String;
    type Value = Arc<OwnerConstructionValueAbiLookup>;

    const NAME: &'static str = "boon.compiler.owner-construction-value-abi-lookup.v1";

    fn key_fingerprint(key: &Self::Key) -> RequestFingerprint {
        request_fingerprint(
            b"boon.compiler.owner-construction-value-abi-lookup-key.v1\0",
            [key.as_bytes()],
        )
    }

    fn output_fingerprint(
        value: &Self::Value,
    ) -> Result<RequestOutputFingerprint, boon_compilation_db::CompilationDbError> {
        Ok(RequestOutputFingerprint(value.fingerprint_v1()))
    }
}

struct OwnerConstructionAbiRequest;

impl RequestFamily for OwnerConstructionAbiRequest {
    type Key = StableCheckOwnerKey;
    type Value = Arc<OwnerConstructionAbiEnvironment>;

    const NAME: &'static str = "boon.compiler.owner-construction-abi.v3";

    fn key_fingerprint(key: &Self::Key) -> RequestFingerprint {
        stable_check_owner_key_fingerprint_v2(key)
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
    value_suffixes: BTreeMap<OwnerSymbolKey, Box<[OwnerSymbolCandidate]>>,
}

enum ProjectOwnerSymbolLookup {
    Resolved {
        candidate: OwnerSymbolCandidate,
        projection: Box<[String]>,
    },
    Ambiguous {
        candidates: Box<[OwnerSymbolCandidate]>,
    },
    Unresolved,
}

impl ProjectOwnerSymbolIndex {
    fn resolve(
        &self,
        owner: &StableCheckOwnerKey,
        reference: &OwnerSymbolReference,
    ) -> ProjectOwnerSymbolLookup {
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
                // A scoped lookup must consume at least the first component
                // of the authored reference. Accepting the parent declaration
                // alone would turn a sibling read such as `store.elements`
                // from inside `theme_options` into the unrelated projection
                // `theme_options.store.elements` and prevent the root-scope
                // lookup from ever running.
                paths.push((candidate, parent.len().saturating_add(1)));
                if parent.pop().is_none() {
                    break;
                }
            }
        } else {
            paths.push((reference.parts.to_vec(), reference.parts.len()));
        }
        for (parts, minimum_prefix) in paths {
            let minimum_prefix = if namespace == OwnerSymbolNamespace::Value {
                minimum_prefix
            } else {
                parts.len()
            };
            for prefix_len in (minimum_prefix..=parts.len()).rev() {
                let key = OwnerSymbolKey {
                    namespace,
                    parts: parts[..prefix_len].to_vec(),
                };
                // Exact paths always win. Only when no exact declaration has
                // that path may an unqualified, uniquely ranked nested value
                // (for example `visible_todos` for `store.visible_todos`) be
                // selected by suffix, matching the whole-program checker.
                let Some(candidates) = self.symbols.get(&key).or_else(|| {
                    (namespace == OwnerSymbolNamespace::Value)
                        .then(|| self.value_suffixes.get(&key))
                        .flatten()
                }) else {
                    continue;
                };
                let Some(best) = candidates.first().map(|candidate| candidate.priority) else {
                    continue;
                };
                let best_candidates = candidates
                    .iter()
                    .take_while(|candidate| candidate.priority == best)
                    .cloned()
                    .collect::<Vec<_>>();
                let projection = parts[prefix_len..].to_vec().into_boxed_slice();
                return if let [candidate] = best_candidates.as_slice() {
                    ProjectOwnerSymbolLookup::Resolved {
                        candidate: candidate.clone(),
                        projection,
                    }
                } else {
                    ProjectOwnerSymbolLookup::Ambiguous {
                        candidates: best_candidates.into_boxed_slice(),
                    }
                };
            }
        }
        ProjectOwnerSymbolLookup::Unresolved
    }

    fn resolves_callable_spelling(
        &self,
        owner: &StableCheckOwnerKey,
        reference: &OwnerSymbolReference,
    ) -> bool {
        let mut callable = reference.clone();
        callable.kind = OwnerReferenceKind::Callable;
        !matches!(
            self.resolve(owner, &callable),
            ProjectOwnerSymbolLookup::Unresolved
        )
    }
}

struct ProjectOwnerSymbolRequest;

impl RequestFamily for ProjectOwnerSymbolRequest {
    type Key = ProjectOwnerSymbolKey;
    type Value = Arc<ProjectOwnerSymbolIndex>;

    const NAME: &'static str = "boon.compiler.project-owner-symbol-index.v3";

    fn key_fingerprint(_key: &Self::Key) -> RequestFingerprint {
        request_fingerprint(
            b"boon.compiler.project-owner-symbol-index-key.v3\0",
            std::iter::empty(),
        )
    }

    fn output_fingerprint(
        value: &Self::Value,
    ) -> Result<RequestOutputFingerprint, boon_compilation_db::CompilationDbError> {
        let mut hasher = Sha256::new();
        hasher.update(b"boon.compiler.project-owner-symbol-index-result.v3\0");
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
                hasher.update(stable_check_owner_key_fingerprint_v2(&candidate.owner));
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

    const NAME: &'static str = "boon.compiler.owner-constraint-summary.v3";

    fn key_fingerprint(key: &Self::Key) -> RequestFingerprint {
        stable_check_owner_key_fingerprint_v2(key)
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

    const NAME: &'static str = "boon.compiler.project-owner-interface-topology.v2";

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

    const NAME: &'static str = "boon.compiler.owner-interface-scc-plan.v2";

    fn key_fingerprint(key: &Self::Key) -> RequestFingerprint {
        let fingerprints = key
            .members
            .iter()
            .map(stable_check_owner_key_fingerprint_v2)
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

struct OwnerInterfaceSccEvaluationRequest;

/// Exact component transaction result. The semantic public-interface and
/// compiled-residual projections retain independent fingerprints, while this
/// root seals that they were produced from one current input transaction.
#[derive(Clone, Debug)]
struct OwnerInterfaceComponentEvaluation {
    #[cfg_attr(not(test), allow(dead_code))]
    currentness: OwnerInterfaceSccCurrentnessReceipt,
    result: Arc<OwnerInterfaceSccResult>,
    module: Arc<OwnerInterfaceTransferModule>,
    fingerprint_v1: [u8; 32],
}

impl RequestFamily for OwnerInterfaceSccEvaluationRequest {
    type Key = OwnerInterfaceSccKey;
    type Value = Arc<OwnerInterfaceComponentEvaluation>;

    const NAME: &'static str = "boon.compiler.owner-interface-scc-evaluation.v10";

    fn key_fingerprint(key: &Self::Key) -> RequestFingerprint {
        OwnerInterfaceSccPlanRequest::key_fingerprint(key)
    }

    fn output_fingerprint(
        value: &Self::Value,
    ) -> Result<RequestOutputFingerprint, boon_compilation_db::CompilationDbError> {
        Ok(RequestOutputFingerprint(value.fingerprint_v1))
    }
}

struct OwnerInterfaceSccRequest;

impl RequestFamily for OwnerInterfaceSccRequest {
    type Key = OwnerInterfaceSccKey;
    type Value = Arc<OwnerInterfaceSccResult>;

    const NAME: &'static str = "boon.compiler.owner-interface-scc-result.v8";

    fn key_fingerprint(key: &Self::Key) -> RequestFingerprint {
        OwnerInterfaceSccPlanRequest::key_fingerprint(key)
    }

    fn output_fingerprint(
        value: &Self::Value,
    ) -> Result<RequestOutputFingerprint, boon_compilation_db::CompilationDbError> {
        Ok(RequestOutputFingerprint(value.fingerprint_v1()))
    }
}

struct OwnerInterfaceTransferModuleRequest;

impl RequestFamily for OwnerInterfaceTransferModuleRequest {
    type Key = OwnerInterfaceSccKey;
    type Value = Arc<OwnerInterfaceTransferModule>;

    const NAME: &'static str = "boon.compiler.owner-interface-transfer-module.v6";

    fn key_fingerprint(key: &Self::Key) -> RequestFingerprint {
        OwnerInterfaceSccPlanRequest::key_fingerprint(key)
    }

    fn output_fingerprint(
        value: &Self::Value,
    ) -> Result<RequestOutputFingerprint, boon_compilation_db::CompilationDbError> {
        Ok(RequestOutputFingerprint(value.fingerprint_v1()))
    }
}

/// Per-owner projection of the current interface topology.
///
/// Its input is the exact SCC key, so an unrelated topology change can
/// backdate this owner-to-provider mapping instead of invalidating every body
/// import plan through the project-wide topology result.
struct OwnerInterfaceProviderRequest;

#[derive(Clone, Debug, Eq, PartialEq)]
struct OwnerInterfaceProvider {
    key: Arc<OwnerInterfaceSccKey>,
    fingerprint: RequestFingerprint,
}

impl OwnerInterfaceProvider {
    fn key(&self) -> &OwnerInterfaceSccKey {
        &self.key
    }
}

impl RequestFamily for OwnerInterfaceProviderRequest {
    type Key = StableCheckOwnerKey;
    type Value = Arc<OwnerInterfaceProvider>;

    const NAME: &'static str = "boon.compiler.owner-interface-provider.v3";

    fn key_fingerprint(key: &Self::Key) -> RequestFingerprint {
        stable_check_owner_key_fingerprint_v2(key)
    }

    fn output_fingerprint(
        value: &Self::Value,
    ) -> Result<RequestOutputFingerprint, boon_compilation_db::CompilationDbError> {
        Ok(RequestOutputFingerprint(value.fingerprint))
    }
}

struct OwnerBodyInferenceEvaluationRequest;

impl RequestFamily for OwnerBodyInferenceEvaluationRequest {
    type Key = StableCheckOwnerKey;
    type Value = Arc<OwnerBodyInferenceEvaluation>;

    const NAME: &'static str = "boon.compiler.owner-body-inference-evaluation.v13";

    fn key_fingerprint(key: &Self::Key) -> RequestFingerprint {
        stable_check_owner_key_fingerprint_v2(key)
    }

    fn output_fingerprint(
        value: &Self::Value,
    ) -> Result<RequestOutputFingerprint, boon_compilation_db::CompilationDbError> {
        Ok(RequestOutputFingerprint(value.currentness.fingerprint_v1()))
    }
}

struct OwnerBodyInferenceRequest;

impl RequestFamily for OwnerBodyInferenceRequest {
    type Key = StableCheckOwnerKey;
    type Value = Arc<OwnerBodyInferenceShard>;

    const NAME: &'static str = "boon.compiler.owner-body-inference.v10";

    fn key_fingerprint(key: &Self::Key) -> RequestFingerprint {
        stable_check_owner_key_fingerprint_v2(key)
    }

    fn output_fingerprint(
        value: &Self::Value,
    ) -> Result<RequestOutputFingerprint, boon_compilation_db::CompilationDbError> {
        Ok(RequestOutputFingerprint(value.fingerprint_v1()))
    }
}

struct CheckedOwnerShardRequest;

impl RequestFamily for CheckedOwnerShardRequest {
    type Key = StableCheckOwnerKey;
    type Value = Arc<CheckedOwnerShard>;

    const NAME: &'static str = "boon.compiler.checked-owner-shard.v9";

    fn key_fingerprint(key: &Self::Key) -> RequestFingerprint {
        stable_check_owner_key_fingerprint_v2(key)
    }

    fn output_fingerprint(
        value: &Self::Value,
    ) -> Result<RequestOutputFingerprint, boon_compilation_db::CompilationDbError> {
        Ok(RequestOutputFingerprint(value.fingerprint_v1()))
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct OwnerDiagnosticsAggregateKey;

struct SourceUnitOwnerDiagnosticsRequest;

impl RequestFamily for SourceUnitOwnerDiagnosticsRequest {
    type Key = SourceUnitId;
    type Value = Arc<SourceUnitOwnerDiagnostics>;

    const NAME: &'static str = "boon.compiler.source-unit-owner-diagnostics.v1";

    fn key_fingerprint(key: &Self::Key) -> RequestFingerprint {
        source_unit_key_fingerprint(b"boon.compiler.source-unit-owner-diagnostics-key.v1\0", key)
    }

    fn output_fingerprint(
        value: &Self::Value,
    ) -> Result<RequestOutputFingerprint, boon_compilation_db::CompilationDbError> {
        Ok(RequestOutputFingerprint(value.fingerprint_v1()))
    }
}

struct SourceUnitProjectDiagnosticsEvaluationRequest;

impl RequestFamily for SourceUnitProjectDiagnosticsEvaluationRequest {
    type Key = SourceUnitId;
    type Value = Arc<SourceUnitProjectDiagnosticsEvaluation>;

    const NAME: &'static str = "boon.compiler.source-unit-project-diagnostics-evaluation.v1";

    fn key_fingerprint(key: &Self::Key) -> RequestFingerprint {
        source_unit_key_fingerprint(
            b"boon.compiler.source-unit-project-diagnostics-evaluation-key.v1\0",
            key,
        )
    }

    fn output_fingerprint(
        value: &Self::Value,
    ) -> Result<RequestOutputFingerprint, boon_compilation_db::CompilationDbError> {
        Ok(RequestOutputFingerprint(value.currentness.fingerprint_v1()))
    }
}

struct SourceUnitProjectDiagnosticsRequest;

impl RequestFamily for SourceUnitProjectDiagnosticsRequest {
    type Key = SourceUnitId;
    type Value = Arc<SourceUnitProjectDiagnostics>;

    const NAME: &'static str = "boon.compiler.source-unit-project-diagnostics.v1";

    fn key_fingerprint(key: &Self::Key) -> RequestFingerprint {
        source_unit_key_fingerprint(
            b"boon.compiler.source-unit-project-diagnostics-key.v1\0",
            key,
        )
    }

    fn output_fingerprint(
        value: &Self::Value,
    ) -> Result<RequestOutputFingerprint, boon_compilation_db::CompilationDbError> {
        Ok(RequestOutputFingerprint(value.fingerprint_v1()))
    }
}

struct OwnerDiagnosticsAggregateRequest;

impl RequestFamily for OwnerDiagnosticsAggregateRequest {
    type Key = OwnerDiagnosticsAggregateKey;
    type Value = Arc<OwnerDiagnosticsAggregate>;

    const NAME: &'static str = "boon.compiler.owner-diagnostics-aggregate.v10";

    fn key_fingerprint(_key: &Self::Key) -> RequestFingerprint {
        request_fingerprint(
            b"boon.compiler.owner-diagnostics-aggregate-key.v1\0",
            std::iter::empty(),
        )
    }

    fn output_fingerprint(
        value: &Self::Value,
    ) -> Result<RequestOutputFingerprint, boon_compilation_db::CompilationDbError> {
        Ok(RequestOutputFingerprint(value.fingerprint_v1()))
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct ProjectOutputFlowFactsKey;

struct ProjectOutputFlowFactsRequest;

impl RequestFamily for ProjectOutputFlowFactsRequest {
    type Key = ProjectOutputFlowFactsKey;
    type Value = Arc<ProjectOutputFlowFacts>;

    const NAME: &'static str = "boon.compiler.project-output-flow-facts.v3";

    fn key_fingerprint(_key: &Self::Key) -> RequestFingerprint {
        request_fingerprint(
            b"boon.compiler.project-output-flow-facts-key.v1\0",
            std::iter::empty(),
        )
    }

    fn output_fingerprint(
        value: &Self::Value,
    ) -> Result<RequestOutputFingerprint, boon_compilation_db::CompilationDbError> {
        Ok(RequestOutputFingerprint(value.fingerprint_v1()))
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct ProjectDiagnosticFactsKey;

struct ProjectDiagnosticFactsRequest;

impl RequestFamily for ProjectDiagnosticFactsRequest {
    type Key = ProjectDiagnosticFactsKey;
    type Value = Arc<ProjectDiagnosticFacts>;

    const NAME: &'static str = "boon.compiler.project-diagnostic-facts.v15";

    fn key_fingerprint(_key: &Self::Key) -> RequestFingerprint {
        request_fingerprint(
            b"boon.compiler.project-diagnostic-facts-key.v1\0",
            std::iter::empty(),
        )
    }

    fn output_fingerprint(
        value: &Self::Value,
    ) -> Result<RequestOutputFingerprint, boon_compilation_db::CompilationDbError> {
        Ok(RequestOutputFingerprint(value.fingerprint_v1()))
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct CheckedOwnerProjectAssemblyKey;

struct CheckedOwnerProjectAssemblyRequest;

impl RequestFamily for CheckedOwnerProjectAssemblyRequest {
    type Key = CheckedOwnerProjectAssemblyKey;
    type Value = Arc<CheckedOwnerProjectAssembly>;

    const NAME: &'static str = "boon.compiler.checked-owner-project-assembly.v7";

    fn key_fingerprint(_key: &Self::Key) -> RequestFingerprint {
        request_fingerprint(
            b"boon.compiler.checked-owner-project-assembly-key.v1\0",
            std::iter::empty(),
        )
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
                owner_declaration_surface_requests: TypedRequestTable::new(),
                owner_lexical_plan_requests: TypedRequestTable::new(),
                owner_constraint_seed_requests: TypedRequestTable::new(),
                project_owner_abi_requests: TypedRequestTable::new(),
                project_owner_callable_abi_requests: TypedRequestTable::new(),
                owner_callable_abi_lookup_requests: TypedRequestTable::new(),
                owner_value_abi_lookup_requests: TypedRequestTable::new(),
                owner_source_payload_abi_lookup_requests: TypedRequestTable::new(),
                owner_parameter_requirement_lookup_requests: TypedRequestTable::new(),
                owner_callable_inference_abi_requests: TypedRequestTable::new(),
                owner_inference_abi_requests: TypedRequestTable::new(),
                owner_construction_callable_abi_lookup_requests: TypedRequestTable::new(),
                owner_construction_value_abi_lookup_requests: TypedRequestTable::new(),
                owner_construction_abi_requests: TypedRequestTable::new(),
                project_owner_symbol_requests: TypedRequestTable::new(),
                owner_callable_resolution_requests: TypedRequestTable::new(),
                project_owner_callable_scope_topology_requests: TypedRequestTable::new(),
                owner_callable_scope_scc_plan_requests: TypedRequestTable::new(),
                owner_callable_scope_scc_evaluation_requests: TypedRequestTable::new(),
                owner_callable_scope_scc_requests: TypedRequestTable::new(),
                owner_callable_scope_provider_requests: TypedRequestTable::new(),
                owner_constraint_requests: TypedRequestTable::new(),
                project_owner_interface_topology_requests: TypedRequestTable::new(),
                owner_interface_scc_plan_requests: TypedRequestTable::new(),
                owner_interface_scc_evaluation_requests: TypedRequestTable::new(),
                owner_interface_scc_requests: TypedRequestTable::new(),
                owner_interface_transfer_module_requests: TypedRequestTable::new(),
                owner_interface_provider_requests: TypedRequestTable::new(),
                owner_body_inference_evaluation_requests: TypedRequestTable::new(),
                owner_body_inference_requests: TypedRequestTable::new(),
                project_output_flow_facts_requests: TypedRequestTable::new(),
                project_diagnostic_facts_requests: TypedRequestTable::new(),
                source_unit_owner_diagnostics_requests: TypedRequestTable::new(),
                source_unit_project_diagnostics_evaluation_requests: TypedRequestTable::new(),
                source_unit_project_diagnostics_requests: TypedRequestTable::new(),
                owner_diagnostics_aggregate_requests: TypedRequestTable::new(),
                checked_owner_shard_requests: TypedRequestTable::new(),
                checked_owner_project_assembly_requests: TypedRequestTable::new(),
                diagnostics: None,
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
        state.diagnostics = None;
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
            .owner_declaration_surface_requests
            .retain(&mut state.syntax_evaluator, |key| {
                surviving_sources.contains(key.source_unit_id())
            })?;
        state
            .owner_lexical_plan_requests
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
            .owner_callable_resolution_requests
            .retain(&mut state.syntax_evaluator, |key| {
                surviving_sources.contains(key.source_unit_id())
            })?;
        state
            .owner_callable_inference_abi_requests
            .retain(&mut state.syntax_evaluator, |key| {
                surviving_sources.contains(key.source_unit_id())
            })?;
        state
            .owner_inference_abi_requests
            .retain(&mut state.syntax_evaluator, |key| {
                surviving_sources.contains(key.source_unit_id())
            })?;
        state
            .owner_construction_abi_requests
            .retain(&mut state.syntax_evaluator, |key| {
                surviving_sources.contains(key.source_unit_id())
            })?;
        state.owner_callable_scope_scc_plan_requests.retain(
            &mut state.syntax_evaluator,
            |key| {
                key.members
                    .iter()
                    .all(|owner| surviving_sources.contains(owner.source_unit_id()))
            },
        )?;
        state.owner_callable_scope_scc_evaluation_requests.retain(
            &mut state.syntax_evaluator,
            |key| {
                key.members
                    .iter()
                    .all(|owner| surviving_sources.contains(owner.source_unit_id()))
            },
        )?;
        state
            .owner_callable_scope_scc_requests
            .retain(&mut state.syntax_evaluator, |key| {
                key.members
                    .iter()
                    .all(|owner| surviving_sources.contains(owner.source_unit_id()))
            })?;
        state
            .owner_callable_scope_provider_requests
            .retain(&mut state.syntax_evaluator, |owner| {
                surviving_sources.contains(owner.source_unit_id())
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
        state
            .owner_interface_provider_requests
            .retain(&mut state.syntax_evaluator, |owner| {
                surviving_sources.contains(owner.source_unit_id())
            })?;
        state
            .owner_body_inference_evaluation_requests
            .retain(&mut state.syntax_evaluator, |owner| {
                surviving_sources.contains(owner.source_unit_id())
            })?;
        state
            .owner_body_inference_requests
            .retain(&mut state.syntax_evaluator, |owner| {
                surviving_sources.contains(owner.source_unit_id())
            })?;
        state
            .source_unit_owner_diagnostics_requests
            .retain(&mut state.syntax_evaluator, |source_unit_id| {
                surviving_sources.contains(source_unit_id)
            })?;
        state
            .source_unit_project_diagnostics_evaluation_requests
            .retain(&mut state.syntax_evaluator, |source_unit_id| {
                surviving_sources.contains(source_unit_id)
            })?;
        state
            .source_unit_project_diagnostics_requests
            .retain(&mut state.syntax_evaluator, |source_unit_id| {
                surviving_sources.contains(source_unit_id)
            })?;
        state
            .checked_owner_shard_requests
            .retain(&mut state.syntax_evaluator, |owner| {
                surviving_sources.contains(owner.source_unit_id())
            })?;
        state.source = candidate;
        state.revision = Revision(next_revision);
        state.diagnostics = None;
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

    /// Returns the body-independent public declaration projection.
    pub fn owner_declaration_surface(
        &self,
        project: ProjectId,
        owner: &StableCheckOwnerKey,
    ) -> CompilerResult<Option<Arc<OwnerDeclarationSurface>>> {
        let state = self
            .projects
            .get(&project)
            .ok_or_else(|| session_error(format!("unknown compiler project {}", project.0)))?;
        Ok(state
            .owner_declaration_surface_requests
            .current_value(&state.syntax_evaluator, owner)?
            .map(Arc::clone))
    }

    /// Returns the syntax-owned base lexical plan for one owner.
    pub fn owner_lexical_plan(
        &self,
        project: ProjectId,
        owner: &StableCheckOwnerKey,
    ) -> CompilerResult<Option<Arc<OwnerLexicalPlan>>> {
        let state = self
            .projects
            .get(&project)
            .ok_or_else(|| session_error(format!("unknown compiler project {}", project.0)))?;
        Ok(state
            .owner_lexical_plan_requests
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

    /// Returns the current immutable, span-free inference result for one
    /// stable owner. This is preparatory input to checked-row construction;
    /// it is not a checked-program shard.
    pub fn owner_body_inference(
        &self,
        project: ProjectId,
        owner: &StableCheckOwnerKey,
    ) -> CompilerResult<Option<Arc<OwnerBodyInferenceShard>>> {
        let state = self
            .projects
            .get(&project)
            .ok_or_else(|| session_error(format!("unknown compiler project {}", project.0)))?;
        Ok(state
            .owner_body_inference_requests
            .current_value(&state.syntax_evaluator, owner)?
            .map(Arc::clone))
    }

    /// Returns the current complete span-free checked product for one stable
    /// authored owner. Dense compatibility IDs and source positions are
    /// deliberately assigned by a separate non-checking assembly request.
    pub fn checked_owner_shard(
        &self,
        project: ProjectId,
        owner: &StableCheckOwnerKey,
    ) -> CompilerResult<Option<Arc<CheckedOwnerShard>>> {
        let state = self
            .projects
            .get(&project)
            .ok_or_else(|| session_error(format!("unknown compiler project {}", project.0)))?;
        Ok(state
            .checked_owner_shard_requests
            .current_value(&state.syntax_evaluator, owner)?
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
            if state.diagnostics.is_none() {
                let (parsed, aggregate, parse_work, parse_ms, typecheck_ms) =
                    parse_project_diagnostics_snapshot(state)?;
                state.diagnostics = Some(compiler_diagnostics_from_owner_aggregate(
                    parsed,
                    &aggregate,
                    parse_work,
                    parse_ms,
                    typecheck_ms,
                )?);
            }
            if cancellation.is_canceled() {
                state.diagnostics = None;
                return Err(canceled_error());
            }
            return Ok(CompilerSessionResult::Diagnostics(
                state.diagnostics.as_ref().expect("diagnostics project"),
            ));
        }

        if intent == CompileIntent::EditorDiagnostics {
            if state.checked.is_none() {
                let (parsed, assembly, parse_work, parse_ms, typecheck_ms) =
                    parse_project_snapshot(state)?;
                state.checked = Some(checked_source_from_owner_assembly(
                    parsed,
                    &assembly,
                    parse_work,
                    parse_ms,
                    boon_typecheck::TypeCheckWorkCounters::default(),
                    typecheck_ms,
                ));
            }
            if cancellation.is_canceled() {
                state.checked = None;
                return Err(canceled_error());
            }
            return Ok(CompilerSessionResult::EditorDiagnostics(
                state.checked.as_ref().expect("checked project"),
            ));
        }

        let current_artifact_available = state
            .compiled
            .as_ref()
            .is_some_and(|(compiled_revision, _)| *compiled_revision == revision);
        if !current_artifact_available {
            if state.checked.is_none() {
                let (parsed, assembly, parse_work, parse_ms, typecheck_ms) =
                    parse_project_snapshot(state)?;
                state.checked = Some(checked_source_from_owner_assembly(
                    parsed,
                    &assembly,
                    parse_work,
                    parse_ms,
                    boon_typecheck::TypeCheckWorkCounters::default(),
                    typecheck_ms,
                ));
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

fn parse_project_syntax_snapshot(
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

    let project =
        ProjectSyntaxSnapshot::from_unit_snapshots(&state.source.entrypoint, linked_units)?;
    let parse_ms = started.elapsed().as_secs_f64() * 1_000.0;
    Ok((project, work, parse_ms))
}

fn parse_project_snapshot(
    state: &mut ProjectState,
) -> CompilerResult<(
    ProjectSyntaxSnapshot,
    Arc<CheckedOwnerProjectAssembly>,
    ParseWorkCounters,
    f64,
    f64,
)> {
    let (project, work, parse_ms) = parse_project_syntax_snapshot(state)?;
    let typecheck_started = Instant::now();
    evaluate_owner_requests(state, project.units())?;
    let owner_requests_ms = typecheck_started.elapsed().as_secs_f64() * 1_000.0;
    let assembly_started = Instant::now();
    evaluate_checked_owner_project_assembly_request(state, &project)?;
    let assembly = Arc::clone(
        state
            .checked_owner_project_assembly_requests
            .current_value(&state.syntax_evaluator, &CheckedOwnerProjectAssemblyKey)?
            .ok_or_else(|| session_error("checked owner project assembly was not published"))?,
    );
    let typecheck_ms = typecheck_started.elapsed().as_secs_f64() * 1_000.0;
    if std::env::var_os("BOON_OWNER_REQUEST_TRACE").is_some() {
        eprintln!(
            "boon owner requests phase=project-assembly items={} phase_ms={:.3} total_ms={typecheck_ms:.3}",
            project.stable_check_owner_keys().count(),
            assembly_started.elapsed().as_secs_f64() * 1_000.0,
        );
        eprintln!(
            "boon owner requests phase=owner-request-total items={} phase_ms={owner_requests_ms:.3} total_ms={typecheck_ms:.3}",
            project.stable_check_owner_keys().count(),
        );
    }
    Ok((project, assembly, work, parse_ms, typecheck_ms))
}

fn parse_project_diagnostics_snapshot(
    state: &mut ProjectState,
) -> CompilerResult<(
    ProjectSyntaxSnapshot,
    Arc<OwnerDiagnosticsAggregate>,
    ParseWorkCounters,
    f64,
    f64,
)> {
    let (project, work, parse_ms) = parse_project_syntax_snapshot(state)?;
    let typecheck_started = Instant::now();
    evaluate_owner_body_requests(state, project.units())?;
    evaluate_owner_diagnostics_aggregate_request(state, &project)?;
    let aggregate = Arc::clone(
        state
            .owner_diagnostics_aggregate_requests
            .current_value(&state.syntax_evaluator, &OwnerDiagnosticsAggregateKey)?
            .ok_or_else(|| session_error("owner diagnostics aggregate was not published"))?,
    );
    let typecheck_ms = typecheck_started.elapsed().as_secs_f64() * 1_000.0;
    if std::env::var_os("BOON_OWNER_REQUEST_TRACE").is_some() {
        eprintln!(
            "boon owner requests phase=diagnostics-aggregate items={} diagnostics={} total_ms={typecheck_ms:.3}",
            aggregate.owner_count(),
            aggregate.diagnostics().len(),
        );
    }
    Ok((project, aggregate, work, parse_ms, typecheck_ms))
}

fn evaluate_owner_body_requests(
    state: &mut ProjectState,
    linked_units: &[Arc<UnitSyntaxSnapshot>],
) -> CompilerResult<()> {
    let mut trace = OwnerRequestTrace::new();
    let mut owners = Vec::new();
    let mut live_owners = BTreeSet::new();
    let live_source_units = linked_units
        .iter()
        .map(|unit| unit.source_unit_id.clone())
        .collect::<BTreeSet<_>>();
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
    trace.checkpoint("collect-owners", owners.len());
    evaluate_project_owner_abi_request(state, linked_units)?;
    trace.checkpoint("project-abi", linked_units.len());
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
        .owner_declaration_surface_requests
        .retain(&mut state.syntax_evaluator, |owner| {
            live_owners.contains(owner)
        })?;
    state
        .owner_lexical_plan_requests
        .retain(&mut state.syntax_evaluator, |owner| {
            live_owners.contains(owner)
        })?;
    state
        .owner_constraint_seed_requests
        .retain(&mut state.syntax_evaluator, |owner| {
            live_owners.contains(owner)
        })?;
    state
        .owner_callable_resolution_requests
        .retain(&mut state.syntax_evaluator, |owner| {
            live_owners.contains(owner)
        })?;
    state
        .owner_callable_inference_abi_requests
        .retain(&mut state.syntax_evaluator, |owner| {
            live_owners.contains(owner)
        })?;
    state
        .owner_inference_abi_requests
        .retain(&mut state.syntax_evaluator, |owner| {
            live_owners.contains(owner)
        })?;
    state
        .owner_callable_scope_provider_requests
        .retain(&mut state.syntax_evaluator, |owner| {
            live_owners.contains(owner)
        })?;
    state
        .owner_constraint_requests
        .retain(&mut state.syntax_evaluator, |owner| {
            live_owners.contains(owner)
        })?;
    state
        .owner_interface_provider_requests
        .retain(&mut state.syntax_evaluator, |owner| {
            live_owners.contains(owner)
        })?;
    state
        .owner_body_inference_evaluation_requests
        .retain(&mut state.syntax_evaluator, |owner| {
            live_owners.contains(owner)
        })?;
    state
        .owner_body_inference_requests
        .retain(&mut state.syntax_evaluator, |owner| {
            live_owners.contains(owner)
        })?;
    state
        .source_unit_owner_diagnostics_requests
        .retain(&mut state.syntax_evaluator, |source_unit_id| {
            live_source_units.contains(source_unit_id)
        })?;
    state
        .source_unit_project_diagnostics_evaluation_requests
        .retain(&mut state.syntax_evaluator, |source_unit_id| {
            live_source_units.contains(source_unit_id)
        })?;
    state
        .source_unit_project_diagnostics_requests
        .retain(&mut state.syntax_evaluator, |source_unit_id| {
            live_source_units.contains(source_unit_id)
        })?;

    let owner_input = RequestInputFingerprint(request_fingerprint(
        b"boon.compiler.owner-input-dependencies.v3\0",
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
    trace.checkpoint("owner-input-and-source-map", owners.len());
    evaluate_owner_constraint_requests(state, linked_units, &owners)?;
    trace.checkpoint("constraints-through-owner-bodies", owners.len());
    Ok(())
}

fn evaluate_owner_requests(
    state: &mut ProjectState,
    linked_units: &[Arc<UnitSyntaxSnapshot>],
) -> CompilerResult<()> {
    evaluate_owner_body_requests(state, linked_units)?;
    let topology = Arc::clone(
        state
            .project_owner_interface_topology_requests
            .current_value(&state.syntax_evaluator, &ProjectOwnerInterfaceTopologyKey)?
            .ok_or_else(|| session_error("owner interface topology was not published"))?,
    );
    evaluate_checked_owner_shard_requests(state, &topology)
}

fn project_owner_abi_input_fingerprint(
    state: &ProjectState,
    linked_units: &[Arc<UnitSyntaxSnapshot>],
) -> RequestInputFingerprint {
    let mut hasher = Sha256::new();
    hasher.update(b"boon.compiler.project-owner-abi-dependencies.v2\0");
    update_request_fingerprint_part(&mut hasher, state.source.entrypoint.as_bytes());
    update_request_fingerprint_part(&mut hasher, state.source.program_role.as_str().as_bytes());
    hasher.update((linked_units.len() as u64).to_le_bytes());
    for unit in linked_units {
        update_request_fingerprint_part(&mut hasher, unit.source_unit_id.as_str().as_bytes());
    }
    RequestInputFingerprint(hasher.finalize().into())
}

fn evaluate_project_owner_abi_request(
    state: &mut ProjectState,
    linked_units: &[Arc<UnitSyntaxSnapshot>],
) -> CompilerResult<()> {
    let key = ProjectOwnerAbiKey;
    let input_fingerprint = project_owner_abi_input_fingerprint(state, linked_units);
    match state.project_owner_abi_requests.begin(
        &mut state.syntax_evaluator,
        key,
        input_fingerprint,
    )? {
        RequestStart::Reused => {}
        RequestStart::Execute(mut ticket) => {
            let abi = (|| -> CompilerResult<_> {
                let mut required_units = Vec::with_capacity(linked_units.len());
                for unit in linked_units {
                    required_units.push(Arc::clone(state.link_requests.require(
                        &state.syntax_evaluator,
                        &mut ticket,
                        &unit.source_unit_id,
                    )?));
                }
                let project = ProjectSyntaxSnapshot::from_unit_snapshots(
                    &state.source.entrypoint,
                    required_units,
                )?;
                let external_types =
                    boon_checked::ExternalTypeEnvironment::empty(state.source.program_role);
                Ok(Arc::new(project_owner_abi_environment(
                    &project,
                    &external_types,
                )?))
            })();
            let abi = match abi {
                Ok(abi) => abi,
                Err(error) => {
                    state.project_owner_abi_requests.abort(
                        &mut state.syntax_evaluator,
                        ticket,
                        RequestAbortReason::Failed,
                    )?;
                    return Err(error);
                }
            };
            state
                .project_owner_abi_requests
                .publish(&mut state.syntax_evaluator, ticket, abi)?;
        }
    }
    evaluate_project_owner_callable_abi_request(state)
}

fn evaluate_project_owner_callable_abi_request(state: &mut ProjectState) -> CompilerResult<()> {
    let key = ProjectOwnerAbiKey;
    let input = RequestInputFingerprint(request_fingerprint(
        b"boon.compiler.project-owner-callable-abi-dependencies.v2\0",
        std::iter::empty(),
    ));
    match state.project_owner_callable_abi_requests.begin(
        &mut state.syntax_evaluator,
        key,
        input,
    )? {
        RequestStart::Reused => Ok(()),
        RequestStart::Execute(mut ticket) => {
            let callable_abi = (|| -> CompilerResult<_> {
                let abi = state.project_owner_abi_requests.require(
                    &state.syntax_evaluator,
                    &mut ticket,
                    &ProjectOwnerAbiKey,
                )?;
                Ok(Arc::new(abi.callable_environment()?))
            })();
            let callable_abi = match callable_abi {
                Ok(callable_abi) => callable_abi,
                Err(error) => {
                    state.project_owner_callable_abi_requests.abort(
                        &mut state.syntax_evaluator,
                        ticket,
                        RequestAbortReason::Failed,
                    )?;
                    return Err(error);
                }
            };
            state.project_owner_callable_abi_requests.publish(
                &mut state.syntax_evaluator,
                ticket,
                callable_abi,
            )?;
            Ok(())
        }
    }
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
    surfaces: &[Arc<OwnerDeclarationSurface>],
    modules: &BTreeMap<SourceUnitId, Option<String>>,
) -> ProjectOwnerSymbolIndex {
    let mut symbols = BTreeMap::<OwnerSymbolKey, Vec<OwnerSymbolCandidate>>::new();
    let mut value_suffixes = BTreeMap::<OwnerSymbolKey, Vec<OwnerSymbolCandidate>>::new();
    for surface in surfaces {
        let Some(declaration) = surface.public() else {
            continue;
        };
        if declaration.kind == OwnerDeclarationKind::Function {
            let Some(name) = declaration.names.first() else {
                continue;
            };
            let canonical_parts = name.split('/').map(str::to_owned).collect::<Vec<_>>();
            let candidate = OwnerSymbolCandidate {
                priority: 0,
                owner: surface.owner().clone(),
                parameters: declaration.parameters.clone(),
            };
            add_owner_symbol(
                &mut symbols,
                OwnerSymbolKey {
                    namespace: OwnerSymbolNamespace::Callable,
                    parts: canonical_parts.clone(),
                },
                candidate.clone(),
            );
            if canonical_parts.len() == 1
                && let Some(module) = modules
                    .get(surface.owner().source_unit_id())
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
        let path = owner_route_value_path(surface.owner(), true);
        // A nested implementation owner can repeat its declaring field name,
        // so the normalized public path gains no segment. It is not a second
        // project value: the nearest ancestor with that path owns the public
        // symbol, while lexical child/sibling references use stable imports.
        if path.is_empty() || path == owner_parent_value_path(surface.owner()) {
            continue;
        }
        let candidate = OwnerSymbolCandidate {
            priority,
            owner: surface.owner().clone(),
            parameters: Box::new([]),
        };
        add_owner_symbol(
            &mut symbols,
            OwnerSymbolKey {
                namespace: OwnerSymbolNamespace::Value,
                parts: path.clone(),
            },
            candidate.clone(),
        );
        for start in 1..path.len() {
            add_owner_symbol(
                &mut value_suffixes,
                OwnerSymbolKey {
                    namespace: OwnerSymbolNamespace::Value,
                    parts: path[start..].to_vec(),
                },
                candidate.clone(),
            );
        }
    }
    let freeze = |symbols: BTreeMap<OwnerSymbolKey, Vec<OwnerSymbolCandidate>>| {
        symbols
            .into_iter()
            .map(|(key, mut candidates)| {
                candidates.sort();
                candidates.dedup();
                (key, candidates.into_boxed_slice())
            })
            .collect()
    };
    ProjectOwnerSymbolIndex {
        symbols: freeze(symbols),
        value_suffixes: freeze(value_suffixes),
    }
}

fn owner_authoritative_callable_names(
    owner: &StableCheckOwnerKey,
    seed: &OwnerConstraintSeed,
    symbols: &ProjectOwnerSymbolIndex,
) -> Box<[String]> {
    seed.references
        .iter()
        .filter(|reference| {
            reference.kind == OwnerReferenceKind::Callable
                && matches!(
                    symbols.resolve(owner, reference),
                    ProjectOwnerSymbolLookup::Unresolved
                )
        })
        .map(|reference| reference.parts.join("/"))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>()
        .into_boxed_slice()
}

fn owner_authoritative_callable_value_names(
    owner: &StableCheckOwnerKey,
    references: &[OwnerSymbolReference],
    symbols: &ProjectOwnerSymbolIndex,
) -> Box<[String]> {
    references
        .iter()
        .filter(|reference| {
            reference.kind == OwnerReferenceKind::Value
                && !reference
                    .parts
                    .first()
                    .is_some_and(|part| boon_syntax::is_program_role_root(part))
                && matches!(
                    symbols.resolve(owner, reference),
                    ProjectOwnerSymbolLookup::Unresolved
                )
                && !symbols.resolves_callable_spelling(owner, reference)
        })
        .map(|reference| reference.parts.join("/"))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>()
        .into_boxed_slice()
}

fn owner_authoritative_value_paths(
    owner: &StableCheckOwnerKey,
    references: &[OwnerSymbolReference],
    symbols: &ProjectOwnerSymbolIndex,
) -> Box<[String]> {
    references
        .iter()
        .filter(|reference| {
            reference.kind == OwnerReferenceKind::Value
                && reference
                    .parts
                    .first()
                    .is_some_and(|part| boon_syntax::is_program_role_root(part))
                && matches!(
                    symbols.resolve(owner, reference),
                    ProjectOwnerSymbolLookup::Unresolved
                )
        })
        .map(|reference| boon_syntax::canonical_value_path(&reference.parts))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>()
        .into_boxed_slice()
}

fn evaluate_owner_callable_inference_abi_requests(
    state: &mut ProjectState,
    owners: &[StableCheckOwnerKey],
) -> CompilerResult<()> {
    let symbols = state
        .project_owner_symbol_requests
        .current_value(&state.syntax_evaluator, &ProjectOwnerSymbolKey)?
        .ok_or_else(|| session_error("project owner symbol index was not published"))?;
    let callable_names = owners
        .iter()
        .map(|owner| {
            state
                .owner_constraint_seed_requests
                .current_value(&state.syntax_evaluator, owner)?
                .map(|seed| owner_authoritative_callable_names(owner, seed, symbols).into_vec())
                .ok_or_else(|| {
                    session_error(format!(
                        "owner callable ABI {owner:?} has no current constraint seed"
                    ))
                })
        })
        .collect::<CompilerResult<Vec<_>>>()?
        .into_iter()
        .flatten()
        .collect::<BTreeSet<_>>();
    let lookup_input = RequestInputFingerprint(request_fingerprint(
        b"boon.compiler.owner-callable-abi-lookup-dependencies.v3\0",
        std::iter::empty(),
    ));
    for name in &callable_names {
        match state.owner_callable_abi_lookup_requests.begin(
            &mut state.syntax_evaluator,
            name.clone(),
            lookup_input,
        )? {
            RequestStart::Reused => {}
            RequestStart::Execute(mut ticket) => {
                let lookup = (|| -> CompilerResult<_> {
                    let abi = state.project_owner_callable_abi_requests.require(
                        &state.syntax_evaluator,
                        &mut ticket,
                        &ProjectOwnerAbiKey,
                    )?;
                    Ok(Arc::new(abi.lookup(name)?))
                })();
                let lookup = match lookup {
                    Ok(lookup) => lookup,
                    Err(error) => {
                        state.owner_callable_abi_lookup_requests.abort(
                            &mut state.syntax_evaluator,
                            ticket,
                            RequestAbortReason::Failed,
                        )?;
                        return Err(error);
                    }
                };
                state.owner_callable_abi_lookup_requests.publish(
                    &mut state.syntax_evaluator,
                    ticket,
                    lookup,
                )?;
            }
        }
    }

    let owner_input = RequestInputFingerprint(request_fingerprint(
        b"boon.compiler.owner-callable-inference-abi-dependencies.v2\0",
        std::iter::empty(),
    ));
    for owner in owners {
        match state.owner_callable_inference_abi_requests.begin(
            &mut state.syntax_evaluator,
            owner.clone(),
            owner_input,
        )? {
            RequestStart::Reused => {}
            RequestStart::Execute(mut ticket) => {
                let environment = (|| -> CompilerResult<_> {
                    let seed = state.owner_constraint_seed_requests.require(
                        &state.syntax_evaluator,
                        &mut ticket,
                        owner,
                    )?;
                    let symbols = state.project_owner_symbol_requests.require(
                        &state.syntax_evaluator,
                        &mut ticket,
                        &ProjectOwnerSymbolKey,
                    )?;
                    let lookups = owner_authoritative_callable_names(owner, seed, symbols)
                        .iter()
                        .map(|name| {
                            state
                                .owner_callable_abi_lookup_requests
                                .require(&state.syntax_evaluator, &mut ticket, name)
                                .map(|lookup| lookup.as_ref().clone())
                                .map_err(Into::into)
                        })
                        .collect::<CompilerResult<Vec<_>>>()?;
                    Ok(Arc::new(OwnerInferenceAbiEnvironment::from_lookups(
                        [owner.clone()],
                        lookups,
                    )?))
                })();
                let environment = match environment {
                    Ok(environment) => environment,
                    Err(error) => {
                        state.owner_callable_inference_abi_requests.abort(
                            &mut state.syntax_evaluator,
                            ticket,
                            RequestAbortReason::Failed,
                        )?;
                        return Err(error);
                    }
                };
                state.owner_callable_inference_abi_requests.publish(
                    &mut state.syntax_evaluator,
                    ticket,
                    environment,
                )?;
            }
        }
    }
    Ok(())
}

fn evaluate_owner_inference_abi_requests(
    state: &mut ProjectState,
    owners: &[StableCheckOwnerKey],
) -> CompilerResult<()> {
    let symbols = state
        .project_owner_symbol_requests
        .current_value(&state.syntax_evaluator, &ProjectOwnerSymbolKey)?
        .ok_or_else(|| session_error("project owner symbol index was not published"))?;
    let mut callable_names = owners
        .iter()
        .map(|owner| {
            state
                .owner_constraint_seed_requests
                .current_value(&state.syntax_evaluator, owner)?
                .map(|seed| owner_authoritative_callable_names(owner, seed, symbols).into_vec())
                .ok_or_else(|| {
                    session_error(format!(
                        "owner inference ABI {owner:?} has no current constraint seed"
                    ))
                })
        })
        .collect::<CompilerResult<Vec<_>>>()?
        .into_iter()
        .flatten()
        .collect::<BTreeSet<_>>();
    callable_names.extend(
        owners
            .iter()
            .map(|owner| {
                let provider = state
                    .owner_callable_scope_provider_requests
                    .current_value(&state.syntax_evaluator, owner)?
                    .ok_or_else(|| {
                        session_error(format!(
                            "owner inference ABI {owner:?} has no callable-scope provider"
                        ))
                    })?;
                let result = state
                    .owner_callable_scope_scc_requests
                    .current_value(&state.syntax_evaluator, provider.key())?
                    .ok_or_else(|| {
                        session_error(format!(
                            "owner inference ABI {owner:?} has no callable-scope result"
                        ))
                    })?;
                let owner_result = result.owner(owner).ok_or_else(|| {
                    session_error(format!(
                        "owner callable-scope result {:?} omits {owner:?}",
                        result.key
                    ))
                })?;
                Ok(owner_authoritative_callable_value_names(
                    owner,
                    owner_result.lexical_plan().external_candidates(),
                    symbols,
                )
                .into_vec())
            })
            .collect::<CompilerResult<Vec<_>>>()?
            .into_iter()
            .flatten(),
    );
    let value_paths = owners
        .iter()
        .map(|owner| {
            let provider = state
                .owner_callable_scope_provider_requests
                .current_value(&state.syntax_evaluator, owner)?
                .ok_or_else(|| {
                    session_error(format!(
                        "owner inference ABI {owner:?} has no callable-scope provider"
                    ))
                })?;
            let result = state
                .owner_callable_scope_scc_requests
                .current_value(&state.syntax_evaluator, provider.key())?
                .ok_or_else(|| {
                    session_error(format!(
                        "owner inference ABI {owner:?} has no callable-scope result"
                    ))
                })?;
            let owner_result = result.owner(owner).ok_or_else(|| {
                session_error(format!(
                    "owner callable-scope result {:?} omits {owner:?}",
                    result.key
                ))
            })?;
            Ok(owner_authoritative_value_paths(
                owner,
                owner_result.lexical_plan().external_candidates(),
                symbols,
            )
            .into_vec())
        })
        .collect::<CompilerResult<Vec<_>>>()?
        .into_iter()
        .flatten()
        .collect::<BTreeSet<_>>();
    let source_payload_paths = owners
        .iter()
        .map(|owner| {
            state
                .owner_constraint_seed_requests
                .current_value(&state.syntax_evaluator, owner)?
                .map(|seed| seed.source_payload_abi_paths().into_vec())
                .ok_or_else(|| {
                    session_error(format!(
                        "owner inference ABI {owner:?} has no current constraint seed"
                    ))
                })
        })
        .collect::<CompilerResult<Vec<_>>>()?
        .into_iter()
        .flatten()
        .collect::<BTreeSet<_>>();
    let parameter_requirement_keys = owners
        .iter()
        .map(|owner| {
            state
                .owner_constraint_seed_requests
                .current_value(&state.syntax_evaluator, owner)?
                .map(|seed| seed.parameter_requirement_keys().into_vec())
                .ok_or_else(|| {
                    session_error(format!(
                        "owner inference ABI {owner:?} has no current constraint seed"
                    ))
                })
        })
        .collect::<CompilerResult<Vec<_>>>()?
        .into_iter()
        .flatten()
        .collect::<BTreeSet<_>>();
    state
        .owner_callable_abi_lookup_requests
        .retain(&mut state.syntax_evaluator, |name| {
            callable_names.contains(name)
        })?;
    state
        .owner_value_abi_lookup_requests
        .retain(&mut state.syntax_evaluator, |path| {
            value_paths.contains(path)
        })?;
    state
        .owner_source_payload_abi_lookup_requests
        .retain(&mut state.syntax_evaluator, |path| {
            source_payload_paths.contains(path)
        })?;
    state
        .owner_parameter_requirement_lookup_requests
        .retain(&mut state.syntax_evaluator, |key| {
            parameter_requirement_keys.contains(key)
        })?;

    let lookup_input = RequestInputFingerprint(request_fingerprint(
        b"boon.compiler.owner-callable-abi-lookup-dependencies.v3\0",
        std::iter::empty(),
    ));
    for name in &callable_names {
        match state.owner_callable_abi_lookup_requests.begin(
            &mut state.syntax_evaluator,
            name.clone(),
            lookup_input,
        )? {
            RequestStart::Reused => {}
            RequestStart::Execute(mut ticket) => {
                let lookup = (|| -> CompilerResult<_> {
                    let abi = state.project_owner_callable_abi_requests.require(
                        &state.syntax_evaluator,
                        &mut ticket,
                        &ProjectOwnerAbiKey,
                    )?;
                    Ok(Arc::new(abi.lookup(name)?))
                })();
                let lookup = match lookup {
                    Ok(lookup) => lookup,
                    Err(error) => {
                        state.owner_callable_abi_lookup_requests.abort(
                            &mut state.syntax_evaluator,
                            ticket,
                            RequestAbortReason::Failed,
                        )?;
                        return Err(error);
                    }
                };
                state.owner_callable_abi_lookup_requests.publish(
                    &mut state.syntax_evaluator,
                    ticket,
                    lookup,
                )?;
            }
        }
    }

    let value_lookup_input = RequestInputFingerprint(request_fingerprint(
        b"boon.compiler.owner-value-abi-lookup-dependencies.v1\0",
        std::iter::empty(),
    ));
    for path in &value_paths {
        match state.owner_value_abi_lookup_requests.begin(
            &mut state.syntax_evaluator,
            path.clone(),
            value_lookup_input,
        )? {
            RequestStart::Reused => {}
            RequestStart::Execute(mut ticket) => {
                let lookup = (|| -> CompilerResult<_> {
                    let abi = state.project_owner_abi_requests.require(
                        &state.syntax_evaluator,
                        &mut ticket,
                        &ProjectOwnerAbiKey,
                    )?;
                    Ok(Arc::new(abi.value_lookup(path)?))
                })();
                let lookup = match lookup {
                    Ok(lookup) => lookup,
                    Err(error) => {
                        state.owner_value_abi_lookup_requests.abort(
                            &mut state.syntax_evaluator,
                            ticket,
                            RequestAbortReason::Failed,
                        )?;
                        return Err(error);
                    }
                };
                state.owner_value_abi_lookup_requests.publish(
                    &mut state.syntax_evaluator,
                    ticket,
                    lookup,
                )?;
            }
        }
    }

    let source_payload_lookup_input = RequestInputFingerprint(request_fingerprint(
        b"boon.compiler.owner-source-payload-abi-lookup-dependencies.v1\0",
        std::iter::empty(),
    ));
    for path in &source_payload_paths {
        match state.owner_source_payload_abi_lookup_requests.begin(
            &mut state.syntax_evaluator,
            path.clone(),
            source_payload_lookup_input,
        )? {
            RequestStart::Reused => {}
            RequestStart::Execute(mut ticket) => {
                let lookup = (|| -> CompilerResult<_> {
                    let abi = state.project_owner_abi_requests.require(
                        &state.syntax_evaluator,
                        &mut ticket,
                        &ProjectOwnerAbiKey,
                    )?;
                    Ok(Arc::new(abi.source_payload_lookup(path)?))
                })();
                let lookup = match lookup {
                    Ok(lookup) => lookup,
                    Err(error) => {
                        state.owner_source_payload_abi_lookup_requests.abort(
                            &mut state.syntax_evaluator,
                            ticket,
                            RequestAbortReason::Failed,
                        )?;
                        return Err(error);
                    }
                };
                state.owner_source_payload_abi_lookup_requests.publish(
                    &mut state.syntax_evaluator,
                    ticket,
                    lookup,
                )?;
            }
        }
    }

    let parameter_requirement_input = RequestInputFingerprint(request_fingerprint(
        b"boon.compiler.owner-parameter-requirement-lookup-dependencies.v1\0",
        std::iter::empty(),
    ));
    for key in &parameter_requirement_keys {
        match state.owner_parameter_requirement_lookup_requests.begin(
            &mut state.syntax_evaluator,
            key.clone(),
            parameter_requirement_input,
        )? {
            RequestStart::Reused => {}
            RequestStart::Execute(mut ticket) => {
                let lookup = (|| -> CompilerResult<_> {
                    let seed = state.owner_constraint_seed_requests.require(
                        &state.syntax_evaluator,
                        &mut ticket,
                        key.owner(),
                    )?;
                    let (function, parameter) = seed
                        .parameter_requirement_names(key.parameter_ordinal())
                        .ok_or_else(|| {
                            session_error(format!(
                                "parameter requirement key {key:?} has no matching function parameter"
                            ))
                        })?;
                    let abi = state.project_owner_abi_requests.require(
                        &state.syntax_evaluator,
                        &mut ticket,
                        &ProjectOwnerAbiKey,
                    )?;
                    Ok(Arc::new(abi.parameter_requirement_lookup(
                        key.clone(),
                        function,
                        parameter,
                    )?))
                })();
                let lookup = match lookup {
                    Ok(lookup) => lookup,
                    Err(error) => {
                        state.owner_parameter_requirement_lookup_requests.abort(
                            &mut state.syntax_evaluator,
                            ticket,
                            RequestAbortReason::Failed,
                        )?;
                        return Err(error);
                    }
                };
                state.owner_parameter_requirement_lookup_requests.publish(
                    &mut state.syntax_evaluator,
                    ticket,
                    lookup,
                )?;
            }
        }
    }

    let owner_input = RequestInputFingerprint(request_fingerprint(
        b"boon.compiler.owner-inference-abi-dependencies.v7\0",
        std::iter::empty(),
    ));
    for owner in owners {
        match state.owner_inference_abi_requests.begin(
            &mut state.syntax_evaluator,
            owner.clone(),
            owner_input,
        )? {
            RequestStart::Reused => {}
            RequestStart::Execute(mut ticket) => {
                let environment = (|| -> CompilerResult<_> {
                    let seed = state.owner_constraint_seed_requests.require(
                        &state.syntax_evaluator,
                        &mut ticket,
                        owner,
                    )?;
                    let callable_abi = state.owner_callable_inference_abi_requests.require(
                        &state.syntax_evaluator,
                        &mut ticket,
                        owner,
                    )?;
                    let symbols = state.project_owner_symbol_requests.require(
                        &state.syntax_evaluator,
                        &mut ticket,
                        &ProjectOwnerSymbolKey,
                    )?;
                    let provider =
                        Arc::clone(state.owner_callable_scope_provider_requests.require(
                            &state.syntax_evaluator,
                            &mut ticket,
                            owner,
                        )?);
                    let scope_result = state.owner_callable_scope_scc_requests.require(
                        &state.syntax_evaluator,
                        &mut ticket,
                        provider.key(),
                    )?;
                    let signature_plan = &scope_result
                        .owner(owner)
                        .ok_or_else(|| {
                            session_error(format!(
                                "owner callable-scope result {:?} omits {owner:?}",
                                scope_result.key
                            ))
                        })?
                        .lexical_plan();
                    let lookups = callable_abi.lookups().to_vec();
                    let value_lookups = owner_authoritative_value_paths(
                        owner,
                        signature_plan.external_candidates(),
                        symbols,
                    )
                    .iter()
                    .map(|path| {
                        state
                            .owner_value_abi_lookup_requests
                            .require(&state.syntax_evaluator, &mut ticket, path)
                            .map(|lookup| lookup.as_ref().clone())
                            .map_err(Into::into)
                    })
                    .collect::<CompilerResult<Vec<_>>>()?;
                    let source_payload_lookups = seed
                        .source_payload_abi_paths()
                        .iter()
                        .map(|path| {
                            state
                                .owner_source_payload_abi_lookup_requests
                                .require(&state.syntax_evaluator, &mut ticket, path)
                                .map(|lookup| lookup.as_ref().clone())
                                .map_err(Into::into)
                        })
                        .collect::<CompilerResult<Vec<_>>>()?;
                    let parameter_requirement_lookups = seed
                        .parameter_requirement_keys()
                        .iter()
                        .map(|key| {
                            state
                                .owner_parameter_requirement_lookup_requests
                                .require(&state.syntax_evaluator, &mut ticket, key)
                                .map(|lookup| lookup.as_ref().clone())
                                .map_err(Into::into)
                        })
                        .collect::<CompilerResult<Vec<_>>>()?;
                    Ok(Arc::new(
                        OwnerInferenceAbiEnvironment::from_complete_lookup_sets(
                            [owner.clone()],
                            lookups,
                            value_lookups,
                            source_payload_lookups,
                            parameter_requirement_lookups,
                        )?,
                    ))
                })();
                let environment = match environment {
                    Ok(environment) => environment,
                    Err(error) => {
                        state.owner_inference_abi_requests.abort(
                            &mut state.syntax_evaluator,
                            ticket,
                            RequestAbortReason::Failed,
                        )?;
                        return Err(error);
                    }
                };
                state.owner_inference_abi_requests.publish(
                    &mut state.syntax_evaluator,
                    ticket,
                    environment,
                )?;
            }
        }
    }
    Ok(())
}

fn evaluate_owner_callable_scope_requests(
    state: &mut ProjectState,
    owners: &[StableCheckOwnerKey],
) -> CompilerResult<()> {
    let mut trace = OwnerRequestTrace::new();
    let resolution_input = RequestInputFingerprint(request_fingerprint(
        b"boon.compiler.owner-callable-resolution-dependencies.v2\0",
        std::iter::empty(),
    ));
    for owner in owners {
        match state.owner_callable_resolution_requests.begin(
            &mut state.syntax_evaluator,
            owner.clone(),
            resolution_input,
        )? {
            RequestStart::Reused => {}
            RequestStart::Execute(mut ticket) => {
                let plan = (|| -> CompilerResult<_> {
                    let seed = state.owner_constraint_seed_requests.require(
                        &state.syntax_evaluator,
                        &mut ticket,
                        owner,
                    )?;
                    let symbols = state.project_owner_symbol_requests.require(
                        &state.syntax_evaluator,
                        &mut ticket,
                        &ProjectOwnerSymbolKey,
                    )?;
                    let abi = state.owner_callable_inference_abi_requests.require(
                        &state.syntax_evaluator,
                        &mut ticket,
                        owner,
                    )?;
                    let resolutions = seed
                        .references
                        .iter()
                        .filter(|reference| reference.kind == OwnerReferenceKind::Callable)
                        .map(|reference| -> CompilerResult<_> {
                            Ok(match symbols.resolve(owner, reference) {
                                ProjectOwnerSymbolLookup::Resolved {
                                    candidate,
                                    projection,
                                } => OwnerSymbolResolution::Resolved {
                                    reference: reference.clone(),
                                    owner: candidate.owner,
                                    projection,
                                    parameters: candidate.parameters,
                                },
                                ProjectOwnerSymbolLookup::Ambiguous { candidates } => {
                                    OwnerSymbolResolution::Ambiguous {
                                        reference: reference.clone(),
                                        candidates: candidates
                                            .into_vec()
                                            .into_iter()
                                            .map(|candidate| AmbiguousOwnerSymbolCandidate {
                                                owner: candidate.owner,
                                                parameters: candidate.parameters,
                                            })
                                            .collect::<Vec<_>>()
                                            .into_boxed_slice(),
                                    }
                                }
                                ProjectOwnerSymbolLookup::Unresolved => {
                                    let canonical_name = reference.parts.join("/");
                                    let lookup = abi.lookup(&canonical_name).ok_or_else(|| {
                                        session_error(format!(
                                            "owner callable resolution {owner:?} has no exact ABI lookup for `{canonical_name}`"
                                        ))
                                    })?;
                                    match lookup.outcome() {
                                        OwnerCallableAbiLookupOutcome::Found { .. } => {
                                            OwnerSymbolResolution::Authoritative {
                                                reference: reference.clone(),
                                            }
                                        }
                                        OwnerCallableAbiLookupOutcome::Missing => {
                                            OwnerSymbolResolution::Unresolved {
                                                reference: reference.clone(),
                                            }
                                        }
                                        OwnerCallableAbiLookupOutcome::Conflict { .. } => {
                                            return Err(session_error(format!(
                                                "owner callable resolution {owner:?} has conflicting authoritative ABI contracts for `{canonical_name}`"
                                            )));
                                        }
                                    }
                                }
                            })
                        })
                        .collect::<CompilerResult<Vec<_>>>()?;
                    Ok(Arc::new(project_owner_callable_resolution_plan(
                        seed,
                        resolutions,
                    )?))
                })();
                let plan = match plan {
                    Ok(plan) => plan,
                    Err(error) => {
                        state.owner_callable_resolution_requests.abort(
                            &mut state.syntax_evaluator,
                            ticket,
                            RequestAbortReason::Failed,
                        )?;
                        return Err(error);
                    }
                };
                state.owner_callable_resolution_requests.publish(
                    &mut state.syntax_evaluator,
                    ticket,
                    plan,
                )?;
            }
        }
    }
    trace.checkpoint("callable-resolution", owners.len());

    let topology_key = ProjectOwnerCallableScopeTopologyKey;
    let topology_input = RequestInputFingerprint(request_fingerprint(
        b"boon.compiler.project-owner-callable-scope-topology-dependencies.v2\0",
        owners
            .iter()
            .map(stable_check_owner_key_fingerprint_v2)
            .collect::<Vec<_>>()
            .iter()
            .map(<[u8; 32]>::as_slice),
    ));
    match state.project_owner_callable_scope_topology_requests.begin(
        &mut state.syntax_evaluator,
        topology_key.clone(),
        topology_input,
    )? {
        RequestStart::Reused => {}
        RequestStart::Execute(mut ticket) => {
            let topology = (|| -> CompilerResult<_> {
                let plans = owners
                    .iter()
                    .map(|owner| {
                        state
                            .owner_callable_resolution_requests
                            .require(&state.syntax_evaluator, &mut ticket, owner)
                            .map(Arc::clone)
                            .map_err(Into::into)
                    })
                    .collect::<CompilerResult<Vec<_>>>()?;
                Ok(Arc::new(build_owner_callable_scope_topology(
                    plans.iter().map(Arc::as_ref),
                )?))
            })();
            let topology = match topology {
                Ok(topology) => topology,
                Err(error) => {
                    state.project_owner_callable_scope_topology_requests.abort(
                        &mut state.syntax_evaluator,
                        ticket,
                        RequestAbortReason::Failed,
                    )?;
                    return Err(error);
                }
            };
            state
                .project_owner_callable_scope_topology_requests
                .publish(&mut state.syntax_evaluator, ticket, topology)?;
        }
    }
    let topology = Arc::clone(
        state
            .project_owner_callable_scope_topology_requests
            .current_value(&state.syntax_evaluator, &topology_key)?
            .ok_or_else(|| session_error("owner callable scope topology was not published"))?,
    );
    trace.checkpoint("callable-scope-topology", topology.sccs.len());
    let live_keys = topology
        .sccs
        .iter()
        .map(|scc| scc.key.clone())
        .collect::<BTreeSet<_>>();
    let live_owners = owners.iter().cloned().collect::<BTreeSet<_>>();
    state
        .owner_callable_scope_scc_plan_requests
        .retain(&mut state.syntax_evaluator, |key| live_keys.contains(key))?;
    state
        .owner_callable_scope_scc_evaluation_requests
        .retain(&mut state.syntax_evaluator, |key| live_keys.contains(key))?;
    state
        .owner_callable_scope_scc_requests
        .retain(&mut state.syntax_evaluator, |key| live_keys.contains(key))?;
    state
        .owner_callable_scope_provider_requests
        .retain(&mut state.syntax_evaluator, |owner| {
            live_owners.contains(owner)
        })?;
    for expected in &topology.sccs {
        let fingerprint = OwnerCallableScopeSccPlanRequest::key_fingerprint(&expected.key);
        let provider_input = RequestInputFingerprint(fingerprint);
        let provider = Arc::new(OwnerCallableScopeProvider {
            key: Arc::new(expected.key.clone()),
            fingerprint,
        });
        for owner in &expected.key.members {
            match state.owner_callable_scope_provider_requests.begin(
                &mut state.syntax_evaluator,
                owner.clone(),
                provider_input,
            )? {
                RequestStart::Reused => {}
                RequestStart::Execute(ticket) => {
                    state.owner_callable_scope_provider_requests.publish(
                        &mut state.syntax_evaluator,
                        ticket,
                        Arc::clone(&provider),
                    )?;
                }
            }
        }
    }
    trace.checkpoint("callable-scope-providers", owners.len());

    let plan_input = RequestInputFingerprint(request_fingerprint(
        b"boon.compiler.owner-callable-scope-scc-plan-dependencies.v2\0",
        std::iter::empty(),
    ));
    for (index, expected) in topology.sccs.iter().enumerate() {
        match state.owner_callable_scope_scc_plan_requests.begin(
            &mut state.syntax_evaluator,
            expected.key.clone(),
            plan_input,
        )? {
            RequestStart::Reused => {}
            RequestStart::Execute(mut ticket) => {
                let plan = (|| -> CompilerResult<_> {
                    let topology = state
                        .project_owner_callable_scope_topology_requests
                        .require(&state.syntax_evaluator, &mut ticket, &topology_key)?;
                    topology
                        .sccs
                        .get(index)
                        .filter(|scc| scc.key == expected.key)
                        .cloned()
                        .map(Arc::new)
                        .ok_or_else(|| {
                            session_error(format!(
                                "owner callable scope topology has no SCC plan for {:?}",
                                expected.key
                            ))
                        })
                })();
                let plan = match plan {
                    Ok(plan) => plan,
                    Err(error) => {
                        state.owner_callable_scope_scc_plan_requests.abort(
                            &mut state.syntax_evaluator,
                            ticket,
                            RequestAbortReason::Failed,
                        )?;
                        return Err(error);
                    }
                };
                state.owner_callable_scope_scc_plan_requests.publish(
                    &mut state.syntax_evaluator,
                    ticket,
                    plan,
                )?;
            }
        }
    }
    trace.checkpoint("callable-scope-plans", topology.sccs.len());

    let evaluation_input = RequestInputFingerprint(request_fingerprint(
        b"boon.compiler.owner-callable-scope-scc-evaluation-dependencies.v3\0",
        std::iter::empty(),
    ));
    let result_input = RequestInputFingerprint(request_fingerprint(
        b"boon.compiler.owner-callable-scope-scc-result-projection-dependencies.v3\0",
        std::iter::empty(),
    ));
    for expected in &topology.sccs {
        match state.owner_callable_scope_scc_evaluation_requests.begin(
            &mut state.syntax_evaluator,
            expected.key.clone(),
            evaluation_input,
        )? {
            RequestStart::Reused => {}
            RequestStart::Execute(mut ticket) => {
                let evaluation = (|| -> CompilerResult<_> {
                    let plan = Arc::clone(state.owner_callable_scope_scc_plan_requests.require(
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
                    let resolutions = plan
                        .key
                        .members
                        .iter()
                        .map(|owner| {
                            state
                                .owner_callable_resolution_requests
                                .require(&state.syntax_evaluator, &mut ticket, owner)
                                .map(Arc::clone)
                                .map_err(Into::into)
                        })
                        .collect::<CompilerResult<Vec<_>>>()?;
                    let abi_slices = plan
                        .key
                        .members
                        .iter()
                        .map(|owner| {
                            state
                                .owner_callable_inference_abi_requests
                                .require(&state.syntax_evaluator, &mut ticket, owner)
                                .map(Arc::clone)
                                .map_err(Into::into)
                        })
                        .collect::<CompilerResult<Vec<_>>>()?;
                    let abi =
                        OwnerInferenceAbiEnvironment::merge(abi_slices.iter().map(Arc::as_ref))?;
                    let dependencies = plan
                        .dependencies
                        .iter()
                        .map(|dependency| {
                            state
                                .owner_callable_scope_scc_requests
                                .require(&state.syntax_evaluator, &mut ticket, dependency)
                                .map(Arc::clone)
                                .map_err(Into::into)
                        })
                        .collect::<CompilerResult<Vec<_>>>()?;
                    let solve_started = Instant::now();
                    let evaluation = evaluate_owner_callable_scope_scc(
                        &plan,
                        seeds.iter().map(Arc::as_ref),
                        resolutions.iter().map(Arc::as_ref),
                        &abi,
                        dependencies.iter().map(Arc::as_ref),
                    )?;
                    let solve_ms = solve_started.elapsed().as_secs_f64() * 1_000.0;
                    if std::env::var_os("BOON_OWNER_REQUEST_TRACE").is_some() && solve_ms >= 10.0 {
                        eprintln!(
                            "boon owner callable scope scc members={} clusters={} dependencies={} call_cyclic={} sample_members={:?} solve_ms={solve_ms:.3}",
                            plan.key.members.len(),
                            plan.containment_clusters.len(),
                            plan.dependencies.len(),
                            plan.call_cyclic,
                            plan.key.members.iter().take(8).collect::<Vec<_>>(),
                        );
                    }
                    Ok(Arc::new(evaluation))
                })();
                let evaluation = match evaluation {
                    Ok(evaluation) => evaluation,
                    Err(error) => {
                        state.owner_callable_scope_scc_evaluation_requests.abort(
                            &mut state.syntax_evaluator,
                            ticket,
                            RequestAbortReason::Failed,
                        )?;
                        return Err(error);
                    }
                };
                state.owner_callable_scope_scc_evaluation_requests.publish(
                    &mut state.syntax_evaluator,
                    ticket,
                    evaluation,
                )?;
            }
        }
        match state.owner_callable_scope_scc_requests.begin(
            &mut state.syntax_evaluator,
            expected.key.clone(),
            result_input,
        )? {
            RequestStart::Reused => {}
            RequestStart::Execute(mut ticket) => {
                let result =
                    (|| -> CompilerResult<_> {
                        let evaluation = state
                            .owner_callable_scope_scc_evaluation_requests
                            .require(&state.syntax_evaluator, &mut ticket, &expected.key)?;
                        Ok(Arc::clone(&evaluation.result))
                    })();
                let result = match result {
                    Ok(result) => result,
                    Err(error) => {
                        state.owner_callable_scope_scc_requests.abort(
                            &mut state.syntax_evaluator,
                            ticket,
                            RequestAbortReason::Failed,
                        )?;
                        return Err(error);
                    }
                };
                state.owner_callable_scope_scc_requests.publish(
                    &mut state.syntax_evaluator,
                    ticket,
                    result,
                )?;
            }
        }
    }
    trace.checkpoint("callable-scope-results", topology.sccs.len());
    Ok(())
}

fn evaluate_owner_constraint_requests(
    state: &mut ProjectState,
    linked_units: &[Arc<UnitSyntaxSnapshot>],
    owners: &[StableCheckOwnerKey],
) -> CompilerResult<()> {
    let mut trace = OwnerRequestTrace::new();
    let projection_input = RequestInputFingerprint(request_fingerprint(
        b"boon.compiler.owner-declaration-and-lexical-dependencies.v3\0",
        std::iter::empty(),
    ));
    let seed_input = RequestInputFingerprint(request_fingerprint(
        b"boon.compiler.owner-constraint-seed-dependencies.v6\0",
        std::iter::empty(),
    ));
    for owner in owners {
        match state.owner_declaration_surface_requests.begin(
            &mut state.syntax_evaluator,
            owner.clone(),
            projection_input,
        )? {
            RequestStart::Reused => {}
            RequestStart::Execute(mut ticket) => {
                let surface = (|| -> CompilerResult<_> {
                    let input = state.owner_input_requests.require(
                        &state.syntax_evaluator,
                        &mut ticket,
                        owner,
                    )?;
                    Ok(Arc::new(project_owner_declaration_surface(input)?))
                })();
                let surface = match surface {
                    Ok(surface) => surface,
                    Err(error) => {
                        state.owner_declaration_surface_requests.abort(
                            &mut state.syntax_evaluator,
                            ticket,
                            RequestAbortReason::Failed,
                        )?;
                        return Err(error);
                    }
                };
                state.owner_declaration_surface_requests.publish(
                    &mut state.syntax_evaluator,
                    ticket,
                    surface,
                )?;
            }
        }
        match state.owner_lexical_plan_requests.begin(
            &mut state.syntax_evaluator,
            owner.clone(),
            projection_input,
        )? {
            RequestStart::Reused => {}
            RequestStart::Execute(mut ticket) => {
                let plan = (|| -> CompilerResult<_> {
                    let input = state.owner_input_requests.require(
                        &state.syntax_evaluator,
                        &mut ticket,
                        owner,
                    )?;
                    Ok(Arc::new(project_owner_lexical_plan(input)?))
                })();
                let plan = match plan {
                    Ok(plan) => plan,
                    Err(error) => {
                        state.owner_lexical_plan_requests.abort(
                            &mut state.syntax_evaluator,
                            ticket,
                            RequestAbortReason::Failed,
                        )?;
                        return Err(error);
                    }
                };
                state.owner_lexical_plan_requests.publish(
                    &mut state.syntax_evaluator,
                    ticket,
                    plan,
                )?;
            }
        }
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
                    let lexical_plan = state.owner_lexical_plan_requests.require(
                        &state.syntax_evaluator,
                        &mut ticket,
                        owner,
                    )?;
                    Ok(Arc::new(project_owner_constraint_seed_with_lexical_plan(
                        input,
                        lexical_plan,
                    )?))
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
    trace.checkpoint("declaration-lexical-and-constraint-seeds", owners.len());
    let symbol_key = ProjectOwnerSymbolKey;
    let owner_key_fingerprints = owners
        .iter()
        .map(stable_check_owner_key_fingerprint_v2)
        .collect::<Vec<_>>();
    let symbol_input = RequestInputFingerprint(request_fingerprint(
        b"boon.compiler.project-owner-symbol-index-dependencies.v3\0",
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
                let surfaces = owners
                    .iter()
                    .map(|owner| {
                        state
                            .owner_declaration_surface_requests
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
                Ok(Arc::new(build_project_owner_symbol_index(
                    &surfaces, &modules,
                )))
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
    trace.checkpoint("project-symbol-index", owners.len());
    evaluate_owner_callable_inference_abi_requests(state, owners)?;
    trace.checkpoint("callable-inference-abi", owners.len());
    evaluate_owner_callable_scope_requests(state, owners)?;
    trace.checkpoint("callable-scopes", owners.len());
    evaluate_owner_inference_abi_requests(state, owners)?;
    trace.checkpoint("inference-abi", owners.len());

    let constraint_input = RequestInputFingerprint(request_fingerprint(
        b"boon.compiler.owner-constraint-summary-dependencies.v3\0",
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
                    let abi = Arc::clone(state.owner_inference_abi_requests.require(
                        &state.syntax_evaluator,
                        &mut ticket,
                        owner,
                    )?);
                    let callable_resolutions = state.owner_callable_resolution_requests.require(
                        &state.syntax_evaluator,
                        &mut ticket,
                        owner,
                    )?;
                    let provider =
                        Arc::clone(state.owner_callable_scope_provider_requests.require(
                            &state.syntax_evaluator,
                            &mut ticket,
                            owner,
                        )?);
                    let scope_result = state.owner_callable_scope_scc_requests.require(
                        &state.syntax_evaluator,
                        &mut ticket,
                        provider.key(),
                    )?;
                    let signature_plan = &scope_result
                        .owner(owner)
                        .ok_or_else(|| {
                            session_error(format!(
                                "owner callable-scope result {:?} omits {owner:?}",
                                scope_result.key
                            ))
                        })?
                        .lexical_plan();
                    let authoritative_callable_value_lookups =
                        owner_authoritative_callable_value_names(
                            owner,
                            signature_plan.external_candidates(),
                            &symbols,
                        )
                        .iter()
                        .map(|name| {
                            state
                                .owner_callable_abi_lookup_requests
                                .require(&state.syntax_evaluator, &mut ticket, name)
                                .map(|lookup| (name.clone(), Arc::clone(lookup)))
                                .map_err(Into::into)
                        })
                        .collect::<CompilerResult<BTreeMap<_, _>>>()?;
                    let callable_by_reference = callable_resolutions
                        .resolutions()
                        .iter()
                        .map(|resolution| (resolution.reference(), resolution))
                        .collect::<BTreeMap<_, _>>();
                    let resolutions = signature_plan
                        .external_candidates()
                        .iter()
                        .map(|reference| -> CompilerResult<_> {
                            if reference.kind == OwnerReferenceKind::Callable {
                                return callable_by_reference
                                    .get(reference)
                                    .cloned()
                                    .cloned()
                                    .ok_or_else(|| {
                                        session_error(format!(
                                            "owner constraint {owner:?} has no callable resolution for {reference:?}"
                                        ))
                                    });
                            }
                            Ok(match symbols.resolve(owner, reference) {
                            ProjectOwnerSymbolLookup::Resolved {
                                candidate,
                                projection,
                            } => OwnerSymbolResolution::Resolved {
                                reference: reference.clone(),
                                owner: candidate.owner,
                                projection,
                                parameters: candidate.parameters,
                            },
                            ProjectOwnerSymbolLookup::Ambiguous { candidates } => {
                                OwnerSymbolResolution::Ambiguous {
                                    reference: reference.clone(),
                                    candidates: candidates
                                        .into_vec()
                                        .into_iter()
                                        .map(|candidate| AmbiguousOwnerSymbolCandidate {
                                            owner: candidate.owner,
                                            parameters: candidate.parameters,
                                        })
                                        .collect::<Vec<_>>()
                                        .into_boxed_slice(),
                                }
                            }
                            ProjectOwnerSymbolLookup::Unresolved
                                if reference.kind == OwnerReferenceKind::Value
                                    && reference
                                        .parts
                                        .first()
                                        .is_some_and(|part| {
                                            boon_syntax::is_program_role_root(part)
                                        }) =>
                            {
                                let canonical_path =
                                    boon_syntax::canonical_value_path(&reference.parts);
                                abi.value_lookup(&canonical_path).ok_or_else(|| {
                                    session_error(format!(
                                        "owner constraint {owner:?} has no exact external value ABI lookup for `{canonical_path}`"
                                    ))
                                })?;
                                OwnerSymbolResolution::Authoritative {
                                    reference: reference.clone(),
                                }
                            }
                            ProjectOwnerSymbolLookup::Unresolved
                                if reference.kind == OwnerReferenceKind::Value
                                    && (symbols.resolves_callable_spelling(owner, reference)
                                        || authoritative_callable_value_lookups
                                            .get(&reference.parts.join("/"))
                                            .is_some_and(|lookup| lookup.contract().is_some())) =>
                            {
                                OwnerSymbolResolution::CallableAsValue {
                                    reference: reference.clone(),
                                }
                            }
                            ProjectOwnerSymbolLookup::Unresolved => {
                                OwnerSymbolResolution::Unresolved {
                                    reference: reference.clone(),
                                }
                            }
                        })
                        })
                        .collect::<CompilerResult<Vec<_>>>()?;
                    Ok(Arc::new(resolve_owner_constraint_seed_with_signature_plan(
                        &seed,
                        signature_plan,
                        resolutions,
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
    trace.checkpoint("resolved-constraints", owners.len());

    let topology_key = ProjectOwnerInterfaceTopologyKey;
    let topology_input = RequestInputFingerprint(request_fingerprint(
        b"boon.compiler.project-owner-interface-topology-dependencies.v2\0",
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
    trace.checkpoint("interface-topology", owners.len());
    evaluate_owner_interface_scc_requests(state)?;
    trace.checkpoint("interfaces-bodies-and-shards", owners.len());
    Ok(())
}

fn evaluate_owner_interface_scc_requests(state: &mut ProjectState) -> CompilerResult<()> {
    let mut trace = OwnerRequestTrace::new();
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
        .owner_interface_scc_evaluation_requests
        .retain(&mut state.syntax_evaluator, |key| live_keys.contains(key))?;
    state
        .owner_interface_scc_requests
        .retain(&mut state.syntax_evaluator, |key| live_keys.contains(key))?;
    state
        .owner_interface_transfer_module_requests
        .retain(&mut state.syntax_evaluator, |key| live_keys.contains(key))?;

    let live_owners = topology
        .sccs
        .iter()
        .flat_map(|scc| scc.key.members.iter().cloned())
        .collect::<BTreeSet<_>>();
    state
        .owner_interface_provider_requests
        .retain(&mut state.syntax_evaluator, |owner| {
            live_owners.contains(owner)
        })?;
    for expected in &topology.sccs {
        let fingerprint = OwnerInterfaceSccPlanRequest::key_fingerprint(&expected.key);
        let provider_input = RequestInputFingerprint(fingerprint);
        let provider = Arc::new(OwnerInterfaceProvider {
            key: Arc::new(expected.key.clone()),
            fingerprint,
        });
        for owner in &expected.key.members {
            match state.owner_interface_provider_requests.begin(
                &mut state.syntax_evaluator,
                owner.clone(),
                provider_input,
            )? {
                RequestStart::Reused => {}
                RequestStart::Execute(ticket) => {
                    state.owner_interface_provider_requests.publish(
                        &mut state.syntax_evaluator,
                        ticket,
                        Arc::clone(&provider),
                    )?;
                }
            }
        }
    }
    trace.checkpoint("interface-providers", live_owners.len());

    let plan_input = RequestInputFingerprint(request_fingerprint(
        b"boon.compiler.owner-interface-scc-plan-dependencies.v2\0",
        std::iter::empty(),
    ));
    for (expected_index, expected) in topology.sccs.iter().enumerate() {
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
                        .get(expected_index)
                        .filter(|scc| scc.key == expected.key)
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
    trace.checkpoint("interface-scc-plans", topology.sccs.len());

    let evaluation_input = RequestInputFingerprint(request_fingerprint(
        b"boon.compiler.owner-interface-scc-evaluation-dependencies.v10\0",
        std::iter::empty(),
    ));
    let result_input = RequestInputFingerprint(request_fingerprint(
        b"boon.compiler.owner-interface-scc-result-projection-dependencies.v7\0",
        std::iter::empty(),
    ));
    let transfer_module_input = RequestInputFingerprint(request_fingerprint(
        b"boon.compiler.owner-interface-transfer-module-dependencies.v5\0",
        std::iter::empty(),
    ));
    for expected in &topology.sccs {
        match state.owner_interface_scc_evaluation_requests.begin(
            &mut state.syntax_evaluator,
            expected.key.clone(),
            evaluation_input,
        )? {
            RequestStart::Reused => {}
            RequestStart::Execute(mut ticket) => {
                let evaluation = (|| -> CompilerResult<_> {
                    let plan = Arc::clone(state.owner_interface_scc_plan_requests.require(
                        &state.syntax_evaluator,
                        &mut ticket,
                        &expected.key,
                    )?);
                    let abi_slices = plan
                        .key
                        .members
                        .iter()
                        .map(|owner| {
                            state
                                .owner_inference_abi_requests
                                .require(&state.syntax_evaluator, &mut ticket, owner)
                                .map(Arc::clone)
                                .map_err(Into::into)
                        })
                        .collect::<CompilerResult<Vec<_>>>()?;
                    let abi =
                        OwnerInferenceAbiEnvironment::merge(abi_slices.iter().map(Arc::as_ref))?;
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
                    let callable_scope_results = plan
                        .key
                        .members
                        .iter()
                        .map(|owner| -> CompilerResult<_> {
                            let provider =
                                Arc::clone(state.owner_callable_scope_provider_requests.require(
                                    &state.syntax_evaluator,
                                    &mut ticket,
                                    owner,
                                )?);
                            state
                                .owner_callable_scope_scc_requests
                                .require(&state.syntax_evaluator, &mut ticket, provider.key())
                                .map(Arc::clone)
                                .map_err(Into::into)
                        })
                        .collect::<CompilerResult<Vec<_>>>()?;
                    let signature_scopes = plan
                        .key
                        .members
                        .iter()
                        .zip(&callable_scope_results)
                        .map(|(owner, result)| {
                            result.owner(owner).ok_or_else(|| {
                                session_error(format!(
                                    "owner callable-scope result {:?} omits {owner:?}",
                                    result.key
                                ))
                            })
                        })
                        .collect::<CompilerResult<Vec<_>>>()?;
                    let solve_started = Instant::now();
                    let mut resolved_transfer_modules =
                        BTreeMap::<OwnerInterfaceSccKey, Arc<OwnerInterfaceTransferModule>>::new();
                    let component = evaluate_owner_interface_scc_component(
                        &plan,
                        &abi,
                        seeds.iter().map(Arc::as_ref),
                        summaries.iter().map(Arc::as_ref),
                        dependencies.iter().map(Arc::as_ref),
                        signature_scopes.iter().copied(),
                        |owners| {
                            let mut modules = BTreeMap::<
                                OwnerInterfaceSccKey,
                                Arc<OwnerInterfaceTransferModule>,
                            >::new();
                            for owner in owners {
                                let provider = Arc::clone(
                                    state
                                        .owner_interface_provider_requests
                                        .require(&state.syntax_evaluator, &mut ticket, owner)
                                        .map_err(|error| error.to_string())?,
                                );
                                if provider.key() == &expected.key {
                                    return Err(format!(
                                        "interface component {:?} classified own member {owner:?} as an external transfer dependency",
                                        expected.key
                                    ));
                                }
                                let module = Arc::clone(
                                    state
                                        .owner_interface_transfer_module_requests
                                        .require(
                                            &state.syntax_evaluator,
                                            &mut ticket,
                                            provider.key(),
                                        )
                                        .map_err(|error| error.to_string())?,
                                );
                                match modules.entry(provider.key().clone()) {
                                    std::collections::btree_map::Entry::Vacant(entry) => {
                                        entry.insert(Arc::clone(&module));
                                    }
                                    std::collections::btree_map::Entry::Occupied(entry)
                                        if entry.get().fingerprint_v1()
                                            == module.fingerprint_v1() => {}
                                    std::collections::btree_map::Entry::Occupied(_) => {
                                        return Err(format!(
                                            "interface component {:?} observed conflicting transfer dependency versions for {:?}",
                                            expected.key,
                                            provider.key()
                                        ));
                                    }
                                }
                                match resolved_transfer_modules.entry(provider.key().clone()) {
                                    std::collections::btree_map::Entry::Vacant(entry) => {
                                        entry.insert(module);
                                    }
                                    std::collections::btree_map::Entry::Occupied(entry)
                                        if entry.get().fingerprint_v1()
                                            == module.fingerprint_v1() => {}
                                    std::collections::btree_map::Entry::Occupied(_) => {
                                        return Err(format!(
                                            "interface component {:?} retained conflicting transfer dependency versions for {:?}",
                                            expected.key,
                                            provider.key()
                                        ));
                                    }
                                }
                            }
                            Ok(modules.into_values().collect())
                        },
                    )?;
                    let evaluation = component.evaluation;
                    let module = component.module;
                    let iterations = component.transfer_iterations;
                    let transfer_work = component.transfer_work;
                    let solve_ms = solve_started.elapsed().as_secs_f64() * 1_000.0;
                    if std::env::var_os("BOON_OWNER_REQUEST_TRACE").is_some() && solve_ms >= 10.0 {
                        let members = plan.key.members.iter().collect::<BTreeSet<_>>();
                        let mut internal_edge_kinds = BTreeMap::new();
                        for edge in plan
                            .edges
                            .iter()
                            .filter(|edge| members.contains(&edge.dependency))
                        {
                            *internal_edge_kinds.entry(edge.kind).or_insert(0usize) += 1;
                        }
                        eprintln!(
                            "boon owner interface scc members={} dependencies={} residual_iterations={} residual_occurrences={} residual_owner_dispatches={} residual_calls={} residual_op_visits={} residual_max_depth={} rounds={} expressions={} unifications={} internal_edges={internal_edge_kinds:?} sample_members={:?} solve_ms={solve_ms:.3}",
                            plan.key.members.len(),
                            plan.dependencies.len(),
                            iterations,
                            transfer_work.occurrences,
                            transfer_work.owner_dispatches,
                            transfer_work.compiled_call_dispatches,
                            transfer_work.op_visits,
                            transfer_work.maximum_owner_depth,
                            evaluation.result.work.solve_rounds,
                            evaluation.result.work.expressions,
                            evaluation.result.work.unification_steps,
                            plan.key.members.iter().take(8).collect::<Vec<_>>(),
                        );
                    }
                    let currentness_fingerprint = evaluation.currentness.fingerprint_v1();
                    let module_fingerprint = module.fingerprint_v1();
                    let mut component_inputs = vec![currentness_fingerprint, module_fingerprint];
                    component_inputs.extend(
                        resolved_transfer_modules
                            .values()
                            .map(|dependency| dependency.fingerprint_v1()),
                    );
                    let fingerprint_v1 = request_fingerprint(
                        b"boon.compiler.owner-interface-component-evaluation.v4\0",
                        component_inputs.iter().map(<[u8; 32]>::as_slice),
                    );
                    Ok(Arc::new(OwnerInterfaceComponentEvaluation {
                        currentness: evaluation.currentness,
                        result: evaluation.result,
                        module,
                        fingerprint_v1,
                    }))
                })();
                let evaluation = match evaluation {
                    Ok(evaluation) => evaluation,
                    Err(error) => {
                        state.owner_interface_scc_evaluation_requests.abort(
                            &mut state.syntax_evaluator,
                            ticket,
                            RequestAbortReason::Failed,
                        )?;
                        return Err(error);
                    }
                };
                state.owner_interface_scc_evaluation_requests.publish(
                    &mut state.syntax_evaluator,
                    ticket,
                    evaluation,
                )?;
            }
        }
        match state.owner_interface_scc_requests.begin(
            &mut state.syntax_evaluator,
            expected.key.clone(),
            result_input,
        )? {
            RequestStart::Reused => {}
            RequestStart::Execute(mut ticket) => {
                let result = (|| -> CompilerResult<_> {
                    let evaluation = state.owner_interface_scc_evaluation_requests.require(
                        &state.syntax_evaluator,
                        &mut ticket,
                        &expected.key,
                    )?;
                    Ok(Arc::clone(&evaluation.result))
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
        match state.owner_interface_transfer_module_requests.begin(
            &mut state.syntax_evaluator,
            expected.key.clone(),
            transfer_module_input,
        )? {
            RequestStart::Reused => {}
            RequestStart::Execute(mut ticket) => {
                let module = (|| -> CompilerResult<_> {
                    let evaluation = state.owner_interface_scc_evaluation_requests.require(
                        &state.syntax_evaluator,
                        &mut ticket,
                        &expected.key,
                    )?;
                    Ok(Arc::clone(&evaluation.module))
                })();
                let module = match module {
                    Ok(module) => module,
                    Err(error) => {
                        state.owner_interface_transfer_module_requests.abort(
                            &mut state.syntax_evaluator,
                            ticket,
                            RequestAbortReason::Failed,
                        )?;
                        return Err(error);
                    }
                };
                state.owner_interface_transfer_module_requests.publish(
                    &mut state.syntax_evaluator,
                    ticket,
                    module,
                )?;
            }
        }
    }
    trace.checkpoint(
        "interface-scc-results-and-transfer-modules",
        topology.sccs.len(),
    );
    evaluate_owner_body_inference_requests(state, &topology)?;
    Ok(())
}

fn evaluate_owner_body_inference_requests(
    state: &mut ProjectState,
    topology: &OwnerInterfaceTopology,
) -> CompilerResult<()> {
    let mut trace = OwnerRequestTrace::new();
    let owners = topology
        .sccs
        .iter()
        .flat_map(|scc| scc.key.members.iter().cloned())
        .collect::<Vec<_>>();
    let evaluation_input = RequestInputFingerprint(request_fingerprint(
        b"boon.compiler.owner-body-inference-evaluation-dependencies.v13\0",
        std::iter::empty(),
    ));
    let mut planning_ms = 0.0;
    let mut direct_owners = 0_u64;
    let mut required_owners = 0_u64;
    let mut provider_sccs = 0_u64;
    let mut result_transfers = 0_u64;
    let mut result_transfer_nodes = 0_u64;
    let mut result_transfer_edges = 0_u64;
    for owner in owners {
        let mut owner_planning_ms = 0.0;
        match state.owner_body_inference_evaluation_requests.begin(
            &mut state.syntax_evaluator,
            owner.clone(),
            evaluation_input,
        )? {
            RequestStart::Reused => {}
            RequestStart::Execute(mut ticket) => {
                let evaluation = (|| -> CompilerResult<_> {
                    let syntax = Arc::clone(state.owner_input_requests.require(
                        &state.syntax_evaluator,
                        &mut ticket,
                        &owner,
                    )?);
                    let lexical_plan = Arc::clone(state.owner_lexical_plan_requests.require(
                        &state.syntax_evaluator,
                        &mut ticket,
                        &owner,
                    )?);
                    let seed = Arc::clone(state.owner_constraint_seed_requests.require(
                        &state.syntax_evaluator,
                        &mut ticket,
                        &owner,
                    )?);
                    let summary = Arc::clone(state.owner_constraint_requests.require(
                        &state.syntax_evaluator,
                        &mut ticket,
                        &owner,
                    )?);
                    let abi = Arc::clone(state.owner_inference_abi_requests.require(
                        &state.syntax_evaluator,
                        &mut ticket,
                        &owner,
                    )?);
                    let planning_started = Instant::now();
                    let mut planner = OwnerBodyInterfacePlanner::new(&seed, &summary)?;
                    let mut interface_modules = BTreeMap::new();
                    while let Some(required_owner) = planner.next_required_owner().cloned() {
                        let provider =
                            Arc::clone(state.owner_interface_provider_requests.require(
                                &state.syntax_evaluator,
                                &mut ticket,
                                &required_owner,
                            )?);
                        let module = if let Some(module) = interface_modules.get(provider.key()) {
                            Arc::clone(module)
                        } else {
                            let module = Arc::clone(
                                state.owner_interface_transfer_module_requests.require(
                                    &state.syntax_evaluator,
                                    &mut ticket,
                                    provider.key(),
                                )?,
                            );
                            interface_modules.insert(provider.key().clone(), Arc::clone(&module));
                            module
                        };
                        planner.provide_interface_module(module)?;
                    }
                    let interface_plan = planner.finish()?;
                    owner_planning_ms = planning_started.elapsed().as_secs_f64() * 1_000.0;
                    let callable_scope_provider =
                        Arc::clone(state.owner_callable_scope_provider_requests.require(
                            &state.syntax_evaluator,
                            &mut ticket,
                            &owner,
                        )?);
                    let callable_scope_result = state.owner_callable_scope_scc_requests.require(
                        &state.syntax_evaluator,
                        &mut ticket,
                        callable_scope_provider.key(),
                    )?;
                    let signature_lexical_plan = &callable_scope_result
                        .owner(&owner)
                        .ok_or_else(|| {
                            session_error(format!(
                                "owner callable-scope result {:?} omits {owner:?}",
                                callable_scope_result.key
                            ))
                        })?
                        .lexical_plan();
                    let solve_started = Instant::now();
                    let evaluation = evaluate_owner_body_with_signature_plan(
                        &syntax,
                        &lexical_plan,
                        &seed,
                        &summary,
                        &abi,
                        &interface_plan,
                        signature_lexical_plan,
                    )?;
                    let solve_ms = solve_started.elapsed().as_secs_f64() * 1_000.0;
                    if std::env::var_os("BOON_OWNER_REQUEST_TRACE").is_some() && solve_ms >= 10.0 {
                        eprintln!(
                            "boon owner body owner={owner:?} import_sccs={} import_owners={} expressions={} calls={} unifications={} solve_ms={solve_ms:.3}",
                            interface_plan.imports().len(),
                            interface_plan.required_owner_count().saturating_sub(1),
                            evaluation.result.work.expressions,
                            evaluation.result.work.calls,
                            evaluation.result.work.unification_steps,
                        );
                    }
                    Ok(Arc::new(evaluation))
                })();
                let evaluation = match evaluation {
                    Ok(evaluation) => evaluation,
                    Err(error) => {
                        state.owner_body_inference_evaluation_requests.abort(
                            &mut state.syntax_evaluator,
                            ticket,
                            RequestAbortReason::Failed,
                        )?;
                        return Err(error);
                    }
                };
                state.owner_body_inference_evaluation_requests.publish(
                    &mut state.syntax_evaluator,
                    ticket,
                    evaluation,
                )?;
            }
        }
        planning_ms += owner_planning_ms;
        let body_evaluation = state
            .owner_body_inference_evaluation_requests
            .current_value(&state.syntax_evaluator, &owner)?
            .ok_or_else(|| {
                session_error(format!(
                    "owner body inference {owner:?} has no current evaluation"
                ))
            })?;
        let body_work = body_evaluation.result.work;
        direct_owners = direct_owners.saturating_add(body_work.interface_plan_direct_owners);
        required_owners = required_owners.saturating_add(body_work.interface_plan_required_owners);
        provider_sccs = provider_sccs.saturating_add(body_work.interface_plan_provider_sccs);
        result_transfers =
            result_transfers.saturating_add(body_work.interface_plan_result_transfers);
        result_transfer_nodes =
            result_transfer_nodes.saturating_add(body_work.interface_plan_transfer_nodes);
        result_transfer_edges =
            result_transfer_edges.saturating_add(body_work.interface_plan_transfer_edges);
        let result_input = RequestInputFingerprint(request_fingerprint(
            b"boon.compiler.owner-body-inference-result-projection-dependencies.v8\0",
            std::iter::empty(),
        ));
        match state.owner_body_inference_requests.begin(
            &mut state.syntax_evaluator,
            owner.clone(),
            result_input,
        )? {
            RequestStart::Reused => {}
            RequestStart::Execute(mut ticket) => {
                let body = (|| -> CompilerResult<_> {
                    let evaluation = state.owner_body_inference_evaluation_requests.require(
                        &state.syntax_evaluator,
                        &mut ticket,
                        &owner,
                    )?;
                    Ok(Arc::clone(&evaluation.result))
                })();
                let body = match body {
                    Ok(body) => body,
                    Err(error) => {
                        state.owner_body_inference_requests.abort(
                            &mut state.syntax_evaluator,
                            ticket,
                            RequestAbortReason::Failed,
                        )?;
                        return Err(error);
                    }
                };
                state.owner_body_inference_requests.publish(
                    &mut state.syntax_evaluator,
                    ticket,
                    body,
                )?;
            }
        }
    }
    if std::env::var_os("BOON_OWNER_REQUEST_TRACE").is_some() {
        eprintln!(
            "boon owner requests phase=owner-body-import-planning items={} direct_owners={} required_owners={} provider_sccs={} result_transfers={} transfer_nodes={} transfer_edges={} phase_ms={planning_ms:.3}",
            topology.stats.nodes,
            direct_owners,
            required_owners,
            provider_sccs,
            result_transfers,
            result_transfer_nodes,
            result_transfer_edges,
        );
    }
    trace.checkpoint("owner-body-results", topology.stats.nodes);
    Ok(())
}

fn evaluate_project_output_flow_facts_request(
    state: &mut ProjectState,
    project: &ProjectSyntaxSnapshot,
) -> CompilerResult<()> {
    let owners = project.stable_check_owner_keys().collect::<Vec<_>>();
    let owner_fingerprints = owners
        .iter()
        .map(stable_check_owner_key_fingerprint_v2)
        .collect::<Vec<_>>();
    let input = RequestInputFingerprint(request_fingerprint(
        b"boon.compiler.project-output-flow-facts-dependencies.v3\0",
        owner_fingerprints.iter().map(<[u8; 32]>::as_slice),
    ));
    let key = ProjectOutputFlowFactsKey;
    match state
        .project_output_flow_facts_requests
        .begin(&mut state.syntax_evaluator, key, input)?
    {
        RequestStart::Reused => Ok(()),
        RequestStart::Execute(mut ticket) => {
            let facts = (|| -> CompilerResult<_> {
                let abi = Arc::clone(state.project_owner_abi_requests.require(
                    &state.syntax_evaluator,
                    &mut ticket,
                    &ProjectOwnerAbiKey,
                )?);
                let mut bodies = Vec::with_capacity(owners.len());
                for owner in &owners {
                    bodies.push(Arc::clone(state.owner_body_inference_requests.require(
                        &state.syntax_evaluator,
                        &mut ticket,
                        owner,
                    )?));
                }
                Ok(Arc::new(project_output_flow_facts(
                    &abi,
                    owners.iter(),
                    bodies.iter().map(Arc::as_ref),
                )?))
            })();
            let facts = match facts {
                Ok(facts) => facts,
                Err(error) => {
                    state.project_output_flow_facts_requests.abort(
                        &mut state.syntax_evaluator,
                        ticket,
                        RequestAbortReason::Failed,
                    )?;
                    return Err(error);
                }
            };
            state.project_output_flow_facts_requests.publish(
                &mut state.syntax_evaluator,
                ticket,
                facts,
            )?;
            Ok(())
        }
    }
}

fn evaluate_project_diagnostic_facts_request(
    state: &mut ProjectState,
    project: &ProjectSyntaxSnapshot,
) -> CompilerResult<()> {
    evaluate_project_output_flow_facts_request(state, project)?;
    let owners = project.stable_check_owner_keys().collect::<Vec<_>>();
    let owner_fingerprints = owners
        .iter()
        .map(stable_check_owner_key_fingerprint_v2)
        .collect::<Vec<_>>();
    let source_digest = project.source_bundle_digest_v1().to_string();
    let input = RequestInputFingerprint(request_fingerprint(
        b"boon.compiler.project-diagnostic-facts-dependencies.v15\0",
        std::iter::once(source_digest.as_bytes())
            .chain(std::iter::once(program_role_request_tag(
                state.source.program_role,
            )))
            .chain(owner_fingerprints.iter().map(<[u8; 32]>::as_slice)),
    ));
    let key = ProjectDiagnosticFactsKey;
    match state
        .project_diagnostic_facts_requests
        .begin(&mut state.syntax_evaluator, key, input)?
    {
        RequestStart::Reused => Ok(()),
        RequestStart::Execute(mut ticket) => {
            let facts = (|| -> CompilerResult<_> {
                let abi = Arc::clone(state.project_owner_abi_requests.require(
                    &state.syntax_evaluator,
                    &mut ticket,
                    &ProjectOwnerAbiKey,
                )?);
                let output_flow = Arc::clone(state.project_output_flow_facts_requests.require(
                    &state.syntax_evaluator,
                    &mut ticket,
                    &ProjectOutputFlowFactsKey,
                )?);
                let mut syntax_inputs = Vec::with_capacity(owners.len());
                let mut lexical_plans = Vec::with_capacity(owners.len());
                let mut summaries = Vec::with_capacity(owners.len());
                let mut interfaces = Vec::with_capacity(owners.len());
                let mut bodies = Vec::with_capacity(owners.len());
                let mut inference_abis = Vec::with_capacity(owners.len());
                let mut source_maps = Vec::with_capacity(owners.len());
                let mut interface_results = BTreeMap::new();
                for owner in &owners {
                    syntax_inputs.push(Arc::clone(state.owner_input_requests.require(
                        &state.syntax_evaluator,
                        &mut ticket,
                        owner,
                    )?));
                    lexical_plans.push(Arc::clone(state.owner_lexical_plan_requests.require(
                        &state.syntax_evaluator,
                        &mut ticket,
                        owner,
                    )?));
                    summaries.push(Arc::clone(state.owner_constraint_requests.require(
                        &state.syntax_evaluator,
                        &mut ticket,
                        owner,
                    )?));
                    let provider = Arc::clone(state.owner_interface_provider_requests.require(
                        &state.syntax_evaluator,
                        &mut ticket,
                        owner,
                    )?);
                    let result = if let Some(result) = interface_results.get(provider.key()) {
                        Arc::clone(result)
                    } else {
                        let result = Arc::clone(state.owner_interface_scc_requests.require(
                            &state.syntax_evaluator,
                            &mut ticket,
                            provider.key(),
                        )?);
                        interface_results.insert(provider.key().clone(), Arc::clone(&result));
                        result
                    };
                    interfaces.push(result.owner(owner).cloned().ok_or_else(|| {
                        session_error(format!(
                            "project diagnostics interface SCC has no owner {owner:?}"
                        ))
                    })?);
                    bodies.push(Arc::clone(state.owner_body_inference_requests.require(
                        &state.syntax_evaluator,
                        &mut ticket,
                        owner,
                    )?));
                    inference_abis.push(Arc::clone(
                        state.owner_callable_inference_abi_requests.require(
                            &state.syntax_evaluator,
                            &mut ticket,
                            owner,
                        )?,
                    ));
                    source_maps.push(Arc::clone(state.owner_source_map_requests.require(
                        &state.syntax_evaluator,
                        &mut ticket,
                        owner,
                    )?));
                }
                Ok(Arc::new(project_diagnostic_facts(
                    project,
                    &abi,
                    &output_flow,
                    owners.iter(),
                    syntax_inputs.iter().map(Arc::as_ref),
                    lexical_plans.iter().map(Arc::as_ref),
                    summaries.iter().map(Arc::as_ref),
                    interfaces.iter(),
                    bodies.iter().map(Arc::as_ref),
                    owners.iter().zip(inference_abis.iter().map(Arc::as_ref)),
                    source_maps.iter().map(Arc::as_ref),
                )?))
            })();
            let facts = match facts {
                Ok(facts) => facts,
                Err(error) => {
                    state.project_diagnostic_facts_requests.abort(
                        &mut state.syntax_evaluator,
                        ticket,
                        RequestAbortReason::Failed,
                    )?;
                    return Err(error);
                }
            };
            state.project_diagnostic_facts_requests.publish(
                &mut state.syntax_evaluator,
                ticket,
                facts,
            )?;
            Ok(())
        }
    }
}

fn evaluate_source_unit_owner_diagnostics_requests(
    state: &mut ProjectState,
    project: &ProjectSyntaxSnapshot,
) -> CompilerResult<()> {
    let mut owners_by_unit = BTreeMap::<SourceUnitId, Vec<StableCheckOwnerKey>>::new();
    for owner in project.stable_check_owner_keys() {
        owners_by_unit
            .entry(owner.source_unit_id().clone())
            .or_default()
            .push(owner);
    }
    let expected_units = project
        .source_layouts()
        .iter()
        .map(|layout| layout.source_unit_id.clone())
        .collect::<BTreeSet<_>>();
    if owners_by_unit.keys().ne(expected_units.iter()) {
        return Err(session_error(
            "source-unit owner diagnostics coverage differs from the project source layout",
        ));
    }
    state
        .source_unit_owner_diagnostics_requests
        .retain(&mut state.syntax_evaluator, |source_unit_id| {
            expected_units.contains(source_unit_id)
        })?;

    for (source_unit_id, owners) in owners_by_unit {
        let owner_fingerprints = owners
            .iter()
            .map(stable_check_owner_key_fingerprint_v2)
            .collect::<Vec<_>>();
        let input = RequestInputFingerprint(request_fingerprint(
            b"boon.compiler.source-unit-owner-diagnostics-dependencies.v1\0",
            owner_fingerprints.iter().map(<[u8; 32]>::as_slice),
        ));
        match state.source_unit_owner_diagnostics_requests.begin(
            &mut state.syntax_evaluator,
            source_unit_id.clone(),
            input,
        )? {
            RequestStart::Reused => {}
            RequestStart::Execute(mut ticket) => {
                let projection = (|| -> CompilerResult<_> {
                    let mut body_evaluations = Vec::with_capacity(owners.len());
                    let mut source_maps = Vec::with_capacity(owners.len());
                    for owner in &owners {
                        body_evaluations.push(Arc::clone(
                            state.owner_body_inference_evaluation_requests.require(
                                &state.syntax_evaluator,
                                &mut ticket,
                                owner,
                            )?,
                        ));
                        source_maps.push(Arc::clone(state.owner_source_map_requests.require(
                            &state.syntax_evaluator,
                            &mut ticket,
                            owner,
                        )?));
                    }
                    Ok(Arc::new(project_source_unit_owner_diagnostics(
                        &source_unit_id,
                        owners.iter(),
                        body_evaluations
                            .iter()
                            .map(|evaluation| evaluation.result.as_ref()),
                        source_maps.iter().map(Arc::as_ref),
                    )?))
                })();
                let projection = match projection {
                    Ok(projection) => projection,
                    Err(error) => {
                        state.source_unit_owner_diagnostics_requests.abort(
                            &mut state.syntax_evaluator,
                            ticket,
                            RequestAbortReason::Failed,
                        )?;
                        return Err(error);
                    }
                };
                state.source_unit_owner_diagnostics_requests.publish(
                    &mut state.syntax_evaluator,
                    ticket,
                    projection,
                )?;
            }
        }
    }
    Ok(())
}

fn evaluate_source_unit_project_diagnostics_requests(
    state: &mut ProjectState,
    project: &ProjectSyntaxSnapshot,
) -> CompilerResult<()> {
    evaluate_project_diagnostic_facts_request(state, project)?;
    let expected_units = project
        .source_layouts()
        .iter()
        .map(|layout| layout.source_unit_id.clone())
        .collect::<BTreeSet<_>>();
    state
        .source_unit_project_diagnostics_evaluation_requests
        .retain(&mut state.syntax_evaluator, |source_unit_id| {
            expected_units.contains(source_unit_id)
        })?;
    state
        .source_unit_project_diagnostics_requests
        .retain(&mut state.syntax_evaluator, |source_unit_id| {
            expected_units.contains(source_unit_id)
        })?;

    for layout in project.source_layouts() {
        let source_unit_id = layout.source_unit_id.clone();
        let input = RequestInputFingerprint(source_unit_project_diagnostics_input_fingerprint(
            project, layout,
        ));
        match state
            .source_unit_project_diagnostics_evaluation_requests
            .begin(&mut state.syntax_evaluator, source_unit_id.clone(), input)?
        {
            RequestStart::Reused => {}
            RequestStart::Execute(mut ticket) => {
                let evaluation = (|| -> CompilerResult<_> {
                    let project_facts =
                        Arc::clone(state.project_diagnostic_facts_requests.require(
                            &state.syntax_evaluator,
                            &mut ticket,
                            &ProjectDiagnosticFactsKey,
                        )?);
                    Ok(Arc::new(evaluate_source_unit_project_diagnostics(
                        project,
                        &source_unit_id,
                        &project_facts,
                    )?))
                })();
                let evaluation = match evaluation {
                    Ok(evaluation) => evaluation,
                    Err(error) => {
                        state
                            .source_unit_project_diagnostics_evaluation_requests
                            .abort(
                                &mut state.syntax_evaluator,
                                ticket,
                                RequestAbortReason::Failed,
                            )?;
                        return Err(error);
                    }
                };
                state
                    .source_unit_project_diagnostics_evaluation_requests
                    .publish(&mut state.syntax_evaluator, ticket, evaluation)?;
            }
        }

        let input = RequestInputFingerprint(source_unit_key_fingerprint(
            b"boon.compiler.source-unit-project-diagnostics-dependencies.v1\0",
            &source_unit_id,
        ));
        match state.source_unit_project_diagnostics_requests.begin(
            &mut state.syntax_evaluator,
            source_unit_id.clone(),
            input,
        )? {
            RequestStart::Reused => {}
            RequestStart::Execute(mut ticket) => {
                let projection = (|| -> CompilerResult<_> {
                    let evaluation = state
                        .source_unit_project_diagnostics_evaluation_requests
                        .require(&state.syntax_evaluator, &mut ticket, &source_unit_id)?;
                    Ok(Arc::clone(&evaluation.result))
                })();
                let projection = match projection {
                    Ok(projection) => projection,
                    Err(error) => {
                        state.source_unit_project_diagnostics_requests.abort(
                            &mut state.syntax_evaluator,
                            ticket,
                            RequestAbortReason::Failed,
                        )?;
                        return Err(error);
                    }
                };
                state.source_unit_project_diagnostics_requests.publish(
                    &mut state.syntax_evaluator,
                    ticket,
                    projection,
                )?;
            }
        }
    }
    Ok(())
}

fn evaluate_owner_diagnostics_aggregate_request(
    state: &mut ProjectState,
    project: &ProjectSyntaxSnapshot,
) -> CompilerResult<()> {
    evaluate_project_diagnostic_facts_request(state, project)?;
    evaluate_source_unit_owner_diagnostics_requests(state, project)?;
    evaluate_source_unit_project_diagnostics_requests(state, project)?;
    let unit_fingerprints = project
        .source_layouts()
        .iter()
        .map(|layout| {
            source_unit_key_fingerprint(
                b"boon.compiler.owner-diagnostics-aggregate-unit.v1\0",
                &layout.source_unit_id,
            )
        })
        .collect::<Vec<_>>();
    let source_digest = project.source_bundle_digest_v1().to_string();
    let input = RequestInputFingerprint(request_fingerprint(
        b"boon.compiler.owner-diagnostics-aggregate-dependencies.v10\0",
        std::iter::once(source_digest.as_bytes())
            .chain(unit_fingerprints.iter().map(<[u8; 32]>::as_slice)),
    ));
    let key = OwnerDiagnosticsAggregateKey;
    match state.owner_diagnostics_aggregate_requests.begin(
        &mut state.syntax_evaluator,
        key,
        input,
    )? {
        RequestStart::Reused => Ok(()),
        RequestStart::Execute(mut ticket) => {
            let aggregate = (|| -> CompilerResult<_> {
                let project_facts = Arc::clone(state.project_diagnostic_facts_requests.require(
                    &state.syntax_evaluator,
                    &mut ticket,
                    &ProjectDiagnosticFactsKey,
                )?);
                let mut projections = Vec::with_capacity(project.source_layouts().len());
                let mut project_evaluations = Vec::with_capacity(project.source_layouts().len());
                let mut project_projections = Vec::with_capacity(project.source_layouts().len());
                for layout in project.source_layouts() {
                    projections.push(Arc::clone(
                        state.source_unit_owner_diagnostics_requests.require(
                            &state.syntax_evaluator,
                            &mut ticket,
                            &layout.source_unit_id,
                        )?,
                    ));
                    project_evaluations.push(Arc::clone(
                        state
                            .source_unit_project_diagnostics_evaluation_requests
                            .require(
                                &state.syntax_evaluator,
                                &mut ticket,
                                &layout.source_unit_id,
                            )?,
                    ));
                    project_projections.push(Arc::clone(
                        state.source_unit_project_diagnostics_requests.require(
                            &state.syntax_evaluator,
                            &mut ticket,
                            &layout.source_unit_id,
                        )?,
                    ));
                }
                Ok(Arc::new(aggregate_source_unit_diagnostics(
                    project,
                    &project_facts,
                    projections.iter().map(Arc::as_ref),
                    project_evaluations.iter().map(Arc::as_ref),
                    project_projections.iter().map(Arc::as_ref),
                )?))
            })();
            let aggregate = match aggregate {
                Ok(aggregate) => aggregate,
                Err(error) => {
                    state.owner_diagnostics_aggregate_requests.abort(
                        &mut state.syntax_evaluator,
                        ticket,
                        RequestAbortReason::Failed,
                    )?;
                    return Err(error);
                }
            };
            state.owner_diagnostics_aggregate_requests.publish(
                &mut state.syntax_evaluator,
                ticket,
                aggregate,
            )?;
            Ok(())
        }
    }
}

fn evaluate_owner_construction_abi_requests(
    state: &mut ProjectState,
    owners: &[StableCheckOwnerKey],
) -> CompilerResult<()> {
    let mut callable_names = BTreeSet::new();
    let mut value_paths = BTreeSet::new();
    for owner in owners {
        let summary = state
            .owner_constraint_requests
            .current_value(&state.syntax_evaluator, owner)?
            .ok_or_else(|| {
                session_error(format!(
                    "owner construction ABI {owner:?} has no current constraint summary"
                ))
            })?;
        callable_names.extend(summary.authoritative_abi_names().into_vec());
        value_paths.extend(summary.authoritative_value_abi_paths().into_vec());
    }
    state
        .owner_construction_callable_abi_lookup_requests
        .retain(&mut state.syntax_evaluator, |name| {
            callable_names.contains(name)
        })?;
    state
        .owner_construction_value_abi_lookup_requests
        .retain(&mut state.syntax_evaluator, |path| {
            value_paths.contains(path)
        })?;

    let lookup_input = RequestInputFingerprint(request_fingerprint(
        b"boon.compiler.owner-construction-abi-lookup-dependencies.v2\0",
        std::iter::empty(),
    ));
    for name in callable_names {
        match state
            .owner_construction_callable_abi_lookup_requests
            .begin(&mut state.syntax_evaluator, name.clone(), lookup_input)?
        {
            RequestStart::Reused => {}
            RequestStart::Execute(mut ticket) => {
                let lookup = (|| -> CompilerResult<_> {
                    let abi = state.project_owner_abi_requests.require(
                        &state.syntax_evaluator,
                        &mut ticket,
                        &ProjectOwnerAbiKey,
                    )?;
                    Ok(Arc::new(abi.construction_callable_lookup(&name)?))
                })();
                let lookup = match lookup {
                    Ok(lookup) => lookup,
                    Err(error) => {
                        state
                            .owner_construction_callable_abi_lookup_requests
                            .abort(
                                &mut state.syntax_evaluator,
                                ticket,
                                RequestAbortReason::Failed,
                            )?;
                        return Err(error);
                    }
                };
                state
                    .owner_construction_callable_abi_lookup_requests
                    .publish(&mut state.syntax_evaluator, ticket, lookup)?;
            }
        }
    }
    for path in value_paths {
        match state.owner_construction_value_abi_lookup_requests.begin(
            &mut state.syntax_evaluator,
            path.clone(),
            lookup_input,
        )? {
            RequestStart::Reused => {}
            RequestStart::Execute(mut ticket) => {
                let lookup = (|| -> CompilerResult<_> {
                    let abi = state.project_owner_abi_requests.require(
                        &state.syntax_evaluator,
                        &mut ticket,
                        &ProjectOwnerAbiKey,
                    )?;
                    Ok(Arc::new(abi.construction_value_lookup(&path)?))
                })();
                let lookup = match lookup {
                    Ok(lookup) => lookup,
                    Err(error) => {
                        state.owner_construction_value_abi_lookup_requests.abort(
                            &mut state.syntax_evaluator,
                            ticket,
                            RequestAbortReason::Failed,
                        )?;
                        return Err(error);
                    }
                };
                state.owner_construction_value_abi_lookup_requests.publish(
                    &mut state.syntax_evaluator,
                    ticket,
                    lookup,
                )?;
            }
        }
    }

    let construction_input = RequestInputFingerprint(request_fingerprint(
        b"boon.compiler.owner-construction-abi-dependencies.v3\0",
        [program_role_request_tag(state.source.program_role)],
    ));
    for owner in owners {
        match state.owner_construction_abi_requests.begin(
            &mut state.syntax_evaluator,
            owner.clone(),
            construction_input,
        )? {
            RequestStart::Reused => {}
            RequestStart::Execute(mut ticket) => {
                let environment = (|| -> CompilerResult<_> {
                    let summary = Arc::clone(state.owner_constraint_requests.require(
                        &state.syntax_evaluator,
                        &mut ticket,
                        owner,
                    )?);
                    let callables = summary
                        .authoritative_abi_names()
                        .into_vec()
                        .into_iter()
                        .map(|name| {
                            state
                                .owner_construction_callable_abi_lookup_requests
                                .require(&state.syntax_evaluator, &mut ticket, &name)
                                .map(|lookup| lookup.as_ref().clone())
                                .map_err(Into::into)
                        })
                        .collect::<CompilerResult<Vec<_>>>()?;
                    let values = summary
                        .authoritative_value_abi_paths()
                        .into_vec()
                        .into_iter()
                        .map(|path| {
                            state
                                .owner_construction_value_abi_lookup_requests
                                .require(&state.syntax_evaluator, &mut ticket, &path)
                                .map(|lookup| lookup.as_ref().clone())
                                .map_err(Into::into)
                        })
                        .collect::<CompilerResult<Vec<_>>>()?;
                    Ok(Arc::new(OwnerConstructionAbiEnvironment::new(
                        owner.clone(),
                        state.source.program_role,
                        callables,
                        values,
                    )?))
                })();
                let environment = match environment {
                    Ok(environment) => environment,
                    Err(error) => {
                        state.owner_construction_abi_requests.abort(
                            &mut state.syntax_evaluator,
                            ticket,
                            RequestAbortReason::Failed,
                        )?;
                        return Err(error);
                    }
                };
                state.owner_construction_abi_requests.publish(
                    &mut state.syntax_evaluator,
                    ticket,
                    environment,
                )?;
            }
        }
    }
    Ok(())
}

fn evaluate_checked_owner_shard_requests(
    state: &mut ProjectState,
    topology: &OwnerInterfaceTopology,
) -> CompilerResult<()> {
    let mut trace = OwnerRequestTrace::new();
    let owners = topology
        .sccs
        .iter()
        .flat_map(|scc| scc.key.members.iter().cloned())
        .collect::<Vec<_>>();
    let live = owners.iter().cloned().collect::<BTreeSet<_>>();
    state
        .owner_construction_abi_requests
        .retain(&mut state.syntax_evaluator, |owner| live.contains(owner))?;
    state
        .checked_owner_shard_requests
        .retain(&mut state.syntax_evaluator, |owner| live.contains(owner))?;
    evaluate_owner_construction_abi_requests(state, &owners)?;
    trace.checkpoint("construction-abi", owners.len());

    let shard_input = RequestInputFingerprint(request_fingerprint(
        b"boon.compiler.checked-owner-shard-dependencies.v9\0",
        std::iter::empty(),
    ));
    for owner in owners {
        match state.checked_owner_shard_requests.begin(
            &mut state.syntax_evaluator,
            owner.clone(),
            shard_input,
        )? {
            RequestStart::Reused => {}
            RequestStart::Execute(mut ticket) => {
                let shard = (|| -> CompilerResult<_> {
                    let syntax = Arc::clone(state.owner_input_requests.require(
                        &state.syntax_evaluator,
                        &mut ticket,
                        &owner,
                    )?);
                    let lexical_plan = Arc::clone(state.owner_lexical_plan_requests.require(
                        &state.syntax_evaluator,
                        &mut ticket,
                        &owner,
                    )?);
                    let seed = Arc::clone(state.owner_constraint_seed_requests.require(
                        &state.syntax_evaluator,
                        &mut ticket,
                        &owner,
                    )?);
                    let summary = Arc::clone(state.owner_constraint_requests.require(
                        &state.syntax_evaluator,
                        &mut ticket,
                        &owner,
                    )?);
                    let evaluation =
                        Arc::clone(state.owner_body_inference_evaluation_requests.require(
                            &state.syntax_evaluator,
                            &mut ticket,
                            &owner,
                        )?);
                    let body = Arc::clone(state.owner_body_inference_requests.require(
                        &state.syntax_evaluator,
                        &mut ticket,
                        &owner,
                    )?);
                    let inference_abi = Arc::clone(state.owner_inference_abi_requests.require(
                        &state.syntax_evaluator,
                        &mut ticket,
                        &owner,
                    )?);
                    let construction_abi =
                        Arc::clone(state.owner_construction_abi_requests.require(
                            &state.syntax_evaluator,
                            &mut ticket,
                            &owner,
                        )?);
                    let own_key = &evaluation.currentness.basis().own_scc.key;
                    let own_scc = Arc::clone(state.owner_interface_scc_requests.require(
                        &state.syntax_evaluator,
                        &mut ticket,
                        own_key,
                    )?);
                    let import_keys = evaluation
                        .currentness
                        .basis()
                        .imports
                        .iter()
                        .map(|frozen| frozen.key.clone())
                        .collect::<Vec<_>>();
                    let imports = import_keys
                        .iter()
                        .map(|key| {
                            state
                                .owner_interface_scc_requests
                                .require(&state.syntax_evaluator, &mut ticket, key)
                                .map(Arc::clone)
                                .map_err(Into::into)
                        })
                        .collect::<CompilerResult<Vec<_>>>()?;
                    let shard = build_checked_owner_shard(
                        &syntax,
                        &lexical_plan,
                        &seed,
                        &summary,
                        &body,
                        &evaluation.currentness,
                        &inference_abi,
                        &construction_abi,
                        &own_scc,
                        imports.iter().map(Arc::as_ref),
                    )
                    .map_err(|error| {
                        session_error(format!(
                            "checked owner shard construction failed for {owner:?}: {error}"
                        ))
                    })?;
                    Ok(Arc::new(shard))
                })();
                let shard = match shard {
                    Ok(shard) => shard,
                    Err(error) => {
                        state.checked_owner_shard_requests.abort(
                            &mut state.syntax_evaluator,
                            ticket,
                            RequestAbortReason::Failed,
                        )?;
                        return Err(error);
                    }
                };
                state.checked_owner_shard_requests.publish(
                    &mut state.syntax_evaluator,
                    ticket,
                    shard,
                )?;
            }
        }
    }
    trace.checkpoint("checked-shard-construction", live.len());
    Ok(())
}

fn evaluate_checked_owner_project_assembly_request(
    state: &mut ProjectState,
    project: &ProjectSyntaxSnapshot,
) -> CompilerResult<()> {
    evaluate_owner_diagnostics_aggregate_request(state, project)?;
    let key = CheckedOwnerProjectAssemblyKey;
    let input = RequestInputFingerprint(request_fingerprint(
        b"boon.compiler.checked-owner-project-assembly-dependencies.v7\0",
        [program_role_request_tag(state.source.program_role)],
    ));
    match state.checked_owner_project_assembly_requests.begin(
        &mut state.syntax_evaluator,
        key,
        input,
    )? {
        RequestStart::Reused => Ok(()),
        RequestStart::Execute(mut ticket) => {
            let assembly = (|| -> CompilerResult<_> {
                let mut units = Vec::with_capacity(project.units().len());
                for unit in project.units() {
                    units.push(Arc::clone(state.link_requests.require(
                        &state.syntax_evaluator,
                        &mut ticket,
                        &unit.source_unit_id,
                    )?));
                }
                let current_project =
                    ProjectSyntaxSnapshot::from_unit_snapshots(&state.source.entrypoint, units)?;
                let project_facts = Arc::clone(state.project_diagnostic_facts_requests.require(
                    &state.syntax_evaluator,
                    &mut ticket,
                    &ProjectDiagnosticFactsKey,
                )?);
                let diagnostics_aggregate =
                    Arc::clone(state.owner_diagnostics_aggregate_requests.require(
                        &state.syntax_evaluator,
                        &mut ticket,
                        &OwnerDiagnosticsAggregateKey,
                    )?);
                let owners = current_project
                    .stable_check_owner_keys()
                    .collect::<Vec<_>>();
                let mut shards = Vec::with_capacity(owners.len());
                let mut source_maps = Vec::with_capacity(owners.len());
                let mut construction_abis = Vec::with_capacity(owners.len());
                for owner in &owners {
                    shards.push(Arc::clone(state.checked_owner_shard_requests.require(
                        &state.syntax_evaluator,
                        &mut ticket,
                        owner,
                    )?));
                    source_maps.push(Arc::clone(state.owner_source_map_requests.require(
                        &state.syntax_evaluator,
                        &mut ticket,
                        owner,
                    )?));
                    construction_abis.push(Arc::clone(
                        state.owner_construction_abi_requests.require(
                            &state.syntax_evaluator,
                            &mut ticket,
                            owner,
                        )?,
                    ));
                }
                let external_types =
                    boon_checked::ExternalTypeEnvironment::empty(state.source.program_role);
                Ok(Arc::new(assemble_checked_owner_project(
                    &current_project,
                    state.source.program_role,
                    external_types,
                    &project_facts,
                    &diagnostics_aggregate,
                    shards.iter().map(Arc::as_ref),
                    source_maps.iter().map(Arc::as_ref),
                    construction_abis.iter().map(Arc::as_ref),
                )?))
            })();
            let assembly = match assembly {
                Ok(assembly) => assembly,
                Err(error) => {
                    state.checked_owner_project_assembly_requests.abort(
                        &mut state.syntax_evaluator,
                        ticket,
                        RequestAbortReason::Failed,
                    )?;
                    return Err(error);
                }
            };
            state.checked_owner_project_assembly_requests.publish(
                &mut state.syntax_evaluator,
                ticket,
                assembly,
            )?;
            Ok(())
        }
    }
}

const fn program_role_request_tag(role: ProgramRole) -> &'static [u8] {
    match role {
        ProgramRole::Client => b"client",
        ProgramRole::Session => b"session",
        ProgramRole::Server => b"server",
    }
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

fn source_unit_project_diagnostics_input_fingerprint(
    project: &ProjectSyntaxSnapshot,
    layout: &ProjectSourceUnitLayout,
) -> RequestFingerprint {
    let mut hasher = Sha256::new();
    hasher.update(b"boon.compiler.source-unit-project-diagnostics-evaluation-dependencies.v1\0");
    update_request_fingerprint_part(
        &mut hasher,
        project.source_bundle_digest_v1().to_string().as_bytes(),
    );
    update_request_fingerprint_part(&mut hasher, layout.source_unit_id.as_str().as_bytes());
    hasher.update(layout.start_line.to_le_bytes());
    hasher.update(layout.start_byte.to_le_bytes());
    hasher.update(layout.source_len.to_le_bytes());
    hasher.update(layout.line_count.to_le_bytes());
    hasher.finalize().into()
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
    fn owner_diagnostics_aggregate_globalizes_equal_local_diagnostics_before_dedup() {
        let source = "value: mystery(input: 1)\n";
        let mut session = CompilerSession::new();
        let project = session
            .open_project(CompilerProject::new(
                "RUN.bn",
                vec![
                    CompilerSourceUnit {
                        path: "RUN.bn".to_owned(),
                        source: source.to_owned(),
                    },
                    CompilerSourceUnit {
                        path: "Second.bn".to_owned(),
                        source: source.to_owned(),
                    },
                ],
                TargetProfile::SoftwareDefault,
                ProgramRole::Server,
                ApplicationIdentity::compiler_default(),
            ))
            .unwrap();
        let (syntax, aggregate, _, _, _) =
            parse_project_diagnostics_snapshot(session.projects.get_mut(&project).unwrap())
                .unwrap();
        let diagnostics = aggregate
            .diagnostics()
            .iter()
            .filter(|diagnostic| diagnostic.message == "unknown function `mystery`")
            .collect::<Vec<_>>();
        assert_eq!(
            diagnostics.len(),
            2,
            "aggregate diagnostics: {aggregate:#?}"
        );
        let actual = diagnostics
            .iter()
            .map(|diagnostic| (diagnostic.line, diagnostic.start, diagnostic.end))
            .collect::<BTreeSet<_>>();
        let expected = syntax
            .source_layouts()
            .iter()
            .map(|layout| {
                (
                    layout.start_line,
                    layout.start_byte + source.find("mystery").unwrap(),
                    layout.start_byte + source.trim_end().len(),
                )
            })
            .collect::<BTreeSet<_>>();
        assert_eq!(actual, expected);
        let paths = diagnostics
            .iter()
            .map(|diagnostic| {
                syntax
                    .source_layouts()
                    .iter()
                    .filter(|layout| layout.start_line <= diagnostic.line)
                    .max_by_key(|layout| layout.start_line)
                    .unwrap()
                    .path
                    .as_str()
            })
            .collect::<BTreeSet<_>>();
        assert_eq!(paths, BTreeSet::from(["RUN.bn", "Second.bn"]));
    }

    #[test]
    fn source_unit_owner_diagnostics_reuse_before_project_layout_relocation() {
        let mut session = CompilerSession::new();
        let project = session
            .open_project(CompilerProject::new(
                "RUN.bn",
                vec![
                    CompilerSourceUnit {
                        path: "RUN.bn".to_owned(),
                        source: "value: 1\n".to_owned(),
                    },
                    CompilerSourceUnit {
                        path: "Second.bn".to_owned(),
                        source: "value: mystery(input: 1)\n".to_owned(),
                    },
                ],
                TargetProfile::SoftwareDefault,
                ProgramRole::Server,
                ApplicationIdentity::compiler_default(),
            ))
            .unwrap();
        let second_id = SourceUnitId::from_path("Second.bn").unwrap();
        let first_revision = session.revision(project).unwrap();
        let first_result = session
            .request(
                project,
                first_revision,
                CompileIntent::Diagnostics,
                &CancellationToken::new(),
            )
            .unwrap();
        let first_diagnostic = first_result
            .diagnostics()
            .unwrap()
            .diagnostics()
            .iter()
            .find(|diagnostic| diagnostic.message == "unknown function `mystery`")
            .cloned()
            .unwrap();
        let first_projection = {
            let state = session.projects.get(&project).unwrap();
            Arc::clone(
                state
                    .source_unit_owner_diagnostics_requests
                    .current_value(&state.syntax_evaluator, &second_id)
                    .unwrap()
                    .unwrap(),
            )
        };
        assert_eq!(first_projection.diagnostics().len(), 1);

        let second_revision = session
            .apply_update(project, UnitUpdate::new("RUN.bn", "\nvalue: 1\n"))
            .unwrap();
        let second_result = session
            .request(
                project,
                second_revision,
                CompileIntent::Diagnostics,
                &CancellationToken::new(),
            )
            .unwrap();
        let second_diagnostic = second_result
            .diagnostics()
            .unwrap()
            .diagnostics()
            .iter()
            .find(|diagnostic| diagnostic.message == "unknown function `mystery`")
            .cloned()
            .unwrap();
        let second_projection = {
            let state = session.projects.get(&project).unwrap();
            Arc::clone(
                state
                    .source_unit_owner_diagnostics_requests
                    .current_value(&state.syntax_evaluator, &second_id)
                    .unwrap()
                    .unwrap(),
            )
        };

        assert!(Arc::ptr_eq(&first_projection, &second_projection));
        assert_eq!(
            first_projection.diagnostics(),
            second_projection.diagnostics()
        );
        assert_eq!(second_diagnostic.line, first_diagnostic.line + 1);
        assert_eq!(second_diagnostic.start, first_diagnostic.start + 1);
        assert_eq!(second_diagnostic.end, first_diagnostic.end + 1);
    }

    #[test]
    fn source_unit_project_diagnostics_backdate_before_layout_relocation() {
        let first_source = "value: 1\n";
        let second_source = "document: [\n    root: 1\n]\n";
        let project_source = |first_source: &str| {
            CompilerProject::new(
                "RUN.bn",
                vec![
                    CompilerSourceUnit {
                        path: "RUN.bn".to_owned(),
                        source: first_source.to_owned(),
                    },
                    CompilerSourceUnit {
                        path: "Second.bn".to_owned(),
                        source: second_source.to_owned(),
                    },
                ],
                TargetProfile::SoftwareDefault,
                ProgramRole::Client,
                ApplicationIdentity::compiler_default(),
            )
        };
        let mut session = CompilerSession::new();
        let project = session.open_project(project_source(first_source)).unwrap();
        let second_id = SourceUnitId::from_path("Second.bn").unwrap();
        let first_revision = session.revision(project).unwrap();
        let first_result = session
            .request(
                project,
                first_revision,
                CompileIntent::Diagnostics,
                &CancellationToken::new(),
            )
            .unwrap();
        let first_diagnostics = first_result.diagnostics().unwrap().diagnostics().to_vec();
        let (first_evaluation, first_projection) = {
            let state = session.projects.get(&project).unwrap();
            (
                Arc::clone(
                    state
                        .source_unit_project_diagnostics_evaluation_requests
                        .current_value(&state.syntax_evaluator, &second_id)
                        .unwrap()
                        .unwrap(),
                ),
                Arc::clone(
                    state
                        .source_unit_project_diagnostics_requests
                        .current_value(&state.syntax_evaluator, &second_id)
                        .unwrap()
                        .unwrap(),
                ),
            )
        };
        assert_eq!(first_projection.rows().len(), 1);
        assert_eq!(first_projection.rows()[0].diagnostic().line, 2);
        let message = first_projection.rows()[0].diagnostic().message.clone();
        let first_diagnostic = first_diagnostics
            .iter()
            .find(|diagnostic| diagnostic.message == message)
            .cloned()
            .unwrap();

        let updated_first_source = format!("\n{first_source}");
        let second_revision = session
            .apply_update(
                project,
                UnitUpdate::new("RUN.bn", updated_first_source.clone()),
            )
            .unwrap();
        let second_result = session
            .request(
                project,
                second_revision,
                CompileIntent::Diagnostics,
                &CancellationToken::new(),
            )
            .unwrap();
        let second_diagnostics = second_result.diagnostics().unwrap().diagnostics().to_vec();
        let second_diagnostic = second_diagnostics
            .iter()
            .find(|diagnostic| diagnostic.message == message)
            .cloned()
            .unwrap();
        let (second_evaluation, second_projection) = {
            let state = session.projects.get(&project).unwrap();
            (
                Arc::clone(
                    state
                        .source_unit_project_diagnostics_evaluation_requests
                        .current_value(&state.syntax_evaluator, &second_id)
                        .unwrap()
                        .unwrap(),
                ),
                Arc::clone(
                    state
                        .source_unit_project_diagnostics_requests
                        .current_value(&state.syntax_evaluator, &second_id)
                        .unwrap()
                        .unwrap(),
                ),
            )
        };

        assert!(!Arc::ptr_eq(&first_evaluation, &second_evaluation));
        assert!(Arc::ptr_eq(&first_projection, &second_projection));
        assert_eq!(second_diagnostic.line, first_diagnostic.line + 1);
        assert_eq!(second_diagnostic.start, first_diagnostic.start + 1);
        assert_eq!(second_diagnostic.end, first_diagnostic.end + 1);

        let mut clean = CompilerSession::new();
        let clean_project = clean
            .open_project(project_source(&updated_first_source))
            .unwrap();
        let clean_revision = clean.revision(clean_project).unwrap();
        let clean_result = clean
            .request(
                clean_project,
                clean_revision,
                CompileIntent::Diagnostics,
                &CancellationToken::new(),
            )
            .unwrap();
        assert_eq!(
            second_diagnostics,
            clean_result.diagnostics().unwrap().diagnostics()
        );
    }

    #[test]
    fn multiline_owner_diagnostic_uses_exact_second_unit_anchor_and_path() {
        let invalid = concat!(
            "value:\n",
            "    needs(\n",
            "        input: 1\n",
            "        extra: 2\n",
            "    )\n",
        );
        let mut session = CompilerSession::new();
        let project = session
            .open_project(CompilerProject::new(
                "RUN.bn",
                vec![
                    CompilerSourceUnit {
                        path: "RUN.bn".to_owned(),
                        source: "FUNCTION needs(input) {\n    input\n}\n".to_owned(),
                    },
                    CompilerSourceUnit {
                        path: "support/invalid.bn".to_owned(),
                        source: invalid.to_owned(),
                    },
                ],
                TargetProfile::SoftwareDefault,
                ProgramRole::Server,
                ApplicationIdentity::compiler_default(),
            ))
            .unwrap();
        let revision = session.revision(project).unwrap();
        let token = CancellationToken::new();
        let result = session
            .request(project, revision, CompileIntent::Diagnostics, &token)
            .unwrap();
        let diagnostics = result.diagnostics().unwrap();
        let layout = diagnostics
            .syntax
            .source_layouts()
            .iter()
            .find(|layout| layout.path == "support/invalid.bn")
            .unwrap();
        let diagnostic = diagnostics
            .diagnostics()
            .iter()
            .find(|diagnostic| {
                diagnostic.message == "`needs` has an unexpected extra call entry `extra`"
            })
            .unwrap_or_else(|| panic!("public diagnostics: {:#?}", diagnostics.diagnostics()));
        let local_start = invalid.find("extra: 2").unwrap();
        assert_eq!(diagnostic.line, layout.start_line + 3);
        assert_eq!(diagnostic.start, layout.start_byte + local_start);
        assert_eq!(diagnostic.end, diagnostic.start + "extra: 2".len());
        assert_eq!(
            &invalid[diagnostic.start - layout.start_byte..diagnostic.end - layout.start_byte],
            "extra: 2"
        );
        let diagnostic_path = diagnostics
            .syntax
            .source_layouts()
            .iter()
            .filter(|candidate| candidate.start_line <= diagnostic.line)
            .max_by_key(|candidate| candidate.start_line)
            .unwrap()
            .path
            .as_str();
        assert_eq!(diagnostic_path, "support/invalid.bn");
    }

    #[test]
    fn multi_unit_host_port_lines_are_global_in_diagnostics_and_metadata() {
        let ports = concat!(
            "host_ports: [\n",
            "    http: [\n",
            "        request: gateway.request\n",
            "        response: response\n",
            "    ]\n",
            "    websocket: [\n",
            "        open: gateway.open\n",
            "        message: gateway.message\n",
            "        close: gateway.close\n",
            "        error: gateway.error\n",
            "        actions: actions\n",
            "    ]\n",
            "]\n",
        );
        let mut session = CompilerSession::new();
        let project = session
            .open_project(CompilerProject::new(
                "RUN.bn",
                vec![
                    CompilerSourceUnit {
                        path: "RUN.bn".to_owned(),
                        source: concat!(
                            "gateway: [\n",
                            "    request: SOURCE\n",
                            "    open: SOURCE\n",
                            "    message: SOURCE\n",
                            "    close: SOURCE\n",
                            "    error: SOURCE\n",
                            "]\n",
                            "outputs: [\n",
                            "    response: 1\n",
                            "    actions: 2\n",
                            "]\n",
                        )
                        .to_owned(),
                    },
                    CompilerSourceUnit {
                        path: "support/ports.bn".to_owned(),
                        source: ports.to_owned(),
                    },
                ],
                TargetProfile::SoftwareDefault,
                ProgramRole::Server,
                ApplicationIdentity::compiler_default(),
            ))
            .unwrap();
        let revision = session.revision(project).unwrap();
        let token = CancellationToken::new();
        let result = session
            .request(project, revision, CompileIntent::Diagnostics, &token)
            .unwrap();
        let diagnostics = result.diagnostics().unwrap();
        let layout = diagnostics
            .syntax
            .source_layouts()
            .iter()
            .find(|layout| layout.path == "support/ports.bn")
            .unwrap();
        let local_line = |needle: &str| {
            1 + ports.as_bytes()[..ports.find(needle).unwrap()]
                .iter()
                .filter(|byte| **byte == b'\n')
                .count()
        };
        let http_line = layout.start_line + local_line("http: [") - 1;
        let websocket_line = layout.start_line + local_line("websocket: [") - 1;
        let http = diagnostics
            .diagnostics()
            .iter()
            .find(|diagnostic| {
                diagnostic
                    .message
                    .starts_with("host port `http.response` output `response` must be exactly")
            })
            .unwrap_or_else(|| panic!("public diagnostics: {:#?}", diagnostics.diagnostics()));
        let websocket = diagnostics
            .diagnostics()
            .iter()
            .find(|diagnostic| {
                diagnostic
                    .message
                    .starts_with("host port `websocket.actions` output `actions` must be a list")
            })
            .unwrap_or_else(|| panic!("public diagnostics: {:#?}", diagnostics.diagnostics()));
        assert_eq!(http.line, http_line);
        assert_eq!(websocket.line, websocket_line);
        for diagnostic in [http, websocket] {
            let path = diagnostics
                .syntax
                .source_layouts()
                .iter()
                .filter(|candidate| candidate.start_line <= diagnostic.line)
                .max_by_key(|candidate| candidate.start_line)
                .unwrap()
                .path
                .as_str();
            assert_eq!(path, "support/ports.bn");
        }

        let state = session.projects.get_mut(&project).unwrap();
        let (_, assembly, _, _, _) = parse_project_snapshot(state).unwrap();
        let table = &assembly.fields().lowering_metadata.host_port_table;
        assert_eq!(table.http.as_ref().unwrap().line, http_line);
        assert_eq!(table.websocket.as_ref().unwrap().line, websocket_line);
    }

    #[test]
    fn owner_diagnostics_aggregate_stops_before_checked_shards_and_verified_builds_them_once() {
        let mut session = CompilerSession::new();
        let project = session.open_project(project("value: 1")).unwrap();
        let revision = session.revision(project).unwrap();
        let token = CancellationToken::new();
        let (_, aggregate, _, _, _) = session
            .projects
            .get_mut(&project)
            .map(parse_project_diagnostics_snapshot)
            .expect("project")
            .unwrap();
        assert!(
            aggregate.diagnostics().is_empty(),
            "lean owner diagnostics: {:#?}",
            aggregate.diagnostics()
        );
        {
            let state = session.projects.get(&project).unwrap();
            assert_eq!(
                state.owner_diagnostics_aggregate_requests.request_count(),
                1
            );
            assert_eq!(state.owner_construction_abi_requests.request_count(), 0);
            assert_eq!(state.checked_owner_shard_requests.request_count(), 0);
            assert_eq!(
                state
                    .checked_owner_project_assembly_requests
                    .request_count(),
                0
            );
        }
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
    fn syntax_selected_interface_occurrence_reaches_stored_sibling_without_scheme_backflow() {
        use boon_checked::{Type, Variant};
        use boon_typecheck::OwnerConstraintEdgeRole;

        fn assert_rich_object(ty: &Type, variable_row: &Type, context: &str) {
            let Type::Object(shape) = ty else {
                panic!("{context} must be one rich object, not its principal union: {ty:#?}");
            };
            assert!(!shape.open, "{context} must be closed: {shape:#?}");
            assert_eq!(
                shape.fields.get("item_kind"),
                Some(variable_row),
                "{context} item_kind",
            );
            assert_eq!(shape.fields.get("id"), Some(&Type::Text), "{context} id");
            assert_eq!(
                shape.fields.get("rich_label"),
                Some(&Type::Text),
                "{context} rich_label",
            );
            assert_eq!(
                shape.fields.get("family"),
                Some(&Type::Text),
                "{context} family",
            );
        }

        fn assert_rich_list(ty: &Type, variable_row: &Type, context: &str) {
            let Type::List(item) = ty else {
                panic!("{context} must be a list: {ty:#?}");
            };
            assert_rich_object(item, variable_row, context);
        }

        let source = concat!(
            "store: [\n",
            "    source_rows:\n",
            "        LIST {\n",
            "            [\n",
            "                item_kind: VariableRow\n",
            "                id: TEXT { one }\n",
            "                segments: LIST {\n",
            "                    [\n",
            "                        file: TEXT { waveform.vcd }\n",
            "                        signal_id: TEXT { one }\n",
            "                        label: TEXT { high }\n",
            "                    ]\n",
            "                }\n",
            "            ]\n",
            "        }\n",
            "\n",
            "    metadata_rows:\n",
            "        LIST {\n",
            "            [\n",
            "                file: TEXT { waveform.vcd }\n",
            "                digest: TEXT { sha256:fixture }\n",
            "                canvas_width: 360\n",
            "                lane_count: lane_rows |> List/length()\n",
            "            ]\n",
            "        }\n",
            "\n",
            "    metadata_record:\n",
            "        metadata(file: TEXT { waveform.vcd })\n",
            "\n",
            "    metadata_digest:\n",
            "        metadata_record |> WHEN {\n",
            "            Found[value] => value.digest\n",
            "            NotFound => TEXT { sha256:none }\n",
            "        }\n",
            "\n",
            "    metadata_width:\n",
            "        metadata_record |> WHEN {\n",
            "            Found[value] => value.canvas_width\n",
            "            NotFound => 360\n",
            "        }\n",
            "\n",
            "    rich_rows:\n",
            "        source_rows\n",
            "        |> List/map(item, new:\n",
            "            dispatch(row: item)\n",
            "        )\n",
            "\n",
            "    rich_labels:\n",
            "        rich_rows\n",
            "        |> List/map(item, new: item.rich_label)\n",
            "\n",
            "    visible_rows:\n",
            "        rich_rows\n",
            "        |> List/filter(item, if: item.item_kind == VariableRow)\n",
            "        |> List/filter(item, if: item.id != TEXT { removed })\n",
            "        |> List/filter(item, if: item.rich_label == TEXT { rich })\n",
            "        |> List/filter(item, if: item.selected == True)\n",
            "        |> List/filter(item, if: item.family == TEXT { fixture })\n",
            "\n",
            "    lane_rows:\n",
            "        visible_rows\n",
            "        |> List/map(item, new:\n",
            "            lane(row: item)\n",
            "        )\n",
            "\n",
            "    feedback:\n",
            "        rich_rows\n",
            "        |> List/any(item, if: item.id == TEXT { one })\n",
            "]\n",
            "\n",
            "FUNCTION enrich(row) {\n",
            "    [\n",
            "        item_kind: row.item_kind\n",
            "        id: row.id\n",
            "        rich_label: TEXT { rich }\n",
            "        family: TEXT { fixture }\n",
            "        segments: row.segments\n",
            "        selected: store.feedback\n",
            "        visible_count: store.visible_rows |> List/length()\n",
            "    ]\n",
            "}\n",
            "\n",
            "FUNCTION dispatch(row) {\n",
            "    row.item_kind |> WHEN {\n",
            "        VariableRow => enrich(row: row)\n",
            "        __ => row\n",
            "    }\n",
            "}\n",
            "\n",
            "FUNCTION lane(row) {\n",
            "    row.item_kind |> WHEN {\n",
            "        VariableRow => lane_variable(row: row)\n",
            "        __ => lane_group(row: row)\n",
            "    }\n",
            "}\n",
            "\n",
            "FUNCTION lane_variable(row) {\n",
            "    [\n",
            "        item_kind: row.item_kind\n",
            "        id: row.id\n",
            "        label: row.rich_label\n",
            "        family: row.family\n",
            "        selected: row.selected\n",
            "        feedback: store.feedback\n",
            "        digest: store.metadata_digest\n",
            "        canvas_width: store.metadata_width\n",
            "        segments: segment_rows(row: row)\n",
            "    ]\n",
            "}\n",
            "\n",
            "FUNCTION lane_group(row) {\n",
            "    [\n",
            "        item_kind: row.item_kind\n",
            "        id: row.id\n",
            "        label: row.family\n",
            "        family: row.family\n",
            "        selected: row.selected\n",
            "        feedback: store.feedback\n",
            "        segments: segment_rows(row: row)\n",
            "    ]\n",
            "}\n",
            "\n",
            "FUNCTION metadata(file) {\n",
            "    store.metadata_rows\n",
            "    |> List/find(item, if: item.file == file)\n",
            "}\n",
            "\n",
            "FUNCTION segment_row(segment, row) {\n",
            "    [\n",
            "        file: segment.file\n",
            "        signal_id: segment.signal_id\n",
            "        lane_id: row.id\n",
            "        label: segment.label\n",
            "    ]\n",
            "}\n",
            "\n",
            "FUNCTION segment_rows(row) {\n",
            "    row.segments\n",
            "    |> List/retain(item, if: segment_is_visible(segment: item))\n",
            "    |> List/map(item, new:\n",
            "        segment_row(\n",
            "            segment: normalized_segment(segment: item)\n",
            "            row: row\n",
            "        )\n",
            "    )\n",
            "}\n",
            "\n",
            "FUNCTION segment_is_visible(segment) {\n",
            "    segment.label == TEXT { high }\n",
            "}\n",
            "\n",
            "FUNCTION normalized_segment(segment) {\n",
            "    [\n",
            "        file: segment.file\n",
            "        signal_id: segment.signal_id\n",
            "        label: segment.label\n",
            "    ]\n",
            "}\n",
        );
        let mut session = CompilerSession::new();
        let project = session.open_project(project(source)).unwrap();
        let (syntax, diagnostics, _, _, _) =
            parse_project_diagnostics_snapshot(session.projects.get_mut(&project).unwrap())
                .unwrap();
        assert!(
            diagnostics.diagnostics().is_empty(),
            "focused syntax-selection diagnostics: {:#?}",
            diagnostics.diagnostics(),
        );
        let owner_named = |name: &str| {
            syntax
                .stable_check_owner_keys()
                .find(|owner| {
                    matches!(
                        owner,
                        StableCheckOwnerKey::Item(item)
                            if item.item_route.segments().last().is_some_and(|segment| {
                                segment.names.as_ref() == [name]
                            })
                    )
                })
                .unwrap_or_else(|| panic!("missing focused owner {name}"))
        };
        let store = owner_named("store");
        let source_rows = owner_named("source_rows");
        let rich_rows = owner_named("rich_rows");
        let rich_labels = owner_named("rich_labels");
        let visible_rows = owner_named("visible_rows");
        let lane_rows = owner_named("lane_rows");
        let metadata_rows = owner_named("metadata_rows");
        let metadata_record = owner_named("metadata_record");
        let metadata_digest = owner_named("metadata_digest");
        let metadata_width = owner_named("metadata_width");
        let feedback = owner_named("feedback");
        let enrich = owner_named("enrich");
        let dispatch = owner_named("dispatch");
        let lane = owner_named("lane");
        let metadata = owner_named("metadata");
        let interface_for = |session: &CompilerSession, owner: &StableCheckOwnerKey| {
            let state = session.projects.get(&project).unwrap();
            let provider = state
                .owner_interface_provider_requests
                .current_value(&state.syntax_evaluator, owner)
                .unwrap()
                .unwrap();
            let result = state
                .owner_interface_scc_requests
                .current_value(&state.syntax_evaluator, provider.key())
                .unwrap()
                .unwrap();
            result
                .owner(owner)
                .unwrap_or_else(|| panic!("missing interface for {owner:?}"))
                .clone()
        };
        let variable_row = Type::VariantSet(vec![Variant::Tag("VariableRow".to_owned())].into());

        {
            let state = session.projects.get(&project).unwrap();
            let component = state
                .owner_interface_provider_requests
                .current_value(&state.syntax_evaluator, &rich_rows)
                .unwrap()
                .unwrap();
            for owner in [&visible_rows, &feedback, &enrich, &dispatch] {
                let provider = state
                    .owner_interface_provider_requests
                    .current_value(&state.syntax_evaluator, owner)
                    .unwrap()
                    .unwrap();
                assert_eq!(
                    provider.key(),
                    component.key(),
                    "focused recursive consumer must share the rich_rows interface component",
                );
            }
        }

        for (owner, name) in [
            (&metadata_rows, "metadata_rows"),
            (&metadata_record, "metadata_record"),
            (&metadata_digest, "metadata_digest"),
            (&metadata_width, "metadata_width"),
        ] {
            let interface = interface_for(&session, owner);
            assert!(
                boon_checked::type_is_recursively_closed(&interface.result.ty),
                "{name} must close the metadata find/projection chain: {interface:#?}",
            );
        }
        let metadata_interface = interface_for(&session, &metadata);
        assert!(
            boon_checked::type_is_recursively_closed(&metadata_interface.result.ty),
            "metadata callable must retain the closed List/find item: {metadata_interface:#?}",
        );

        let source_interface = interface_for(&session, &source_rows);
        let Type::List(source_item) = &source_interface.result.ty else {
            panic!(
                "source_rows must publish a list: {:#?}",
                source_interface.result
            );
        };
        let Type::Object(source_item) = source_item.as_ref() else {
            panic!(
                "source_rows must publish object items: {:#?}",
                source_interface.result
            );
        };
        assert!(!source_item.open);
        assert_eq!(source_item.fields.len(), 3);
        assert_eq!(source_item.fields.get("item_kind"), Some(&variable_row));
        assert_eq!(source_item.fields.get("id"), Some(&Type::Text));
        assert!(!source_item.fields.contains_key("rich_label"));

        let enrich_interface = interface_for(&session, &enrich);
        let Type::Object(enrich_parameter) = &enrich_interface.parameters[0].flow_type.ty else {
            panic!("enrich parameter must remain an open object scheme");
        };
        let Type::Object(enrich_result) = &enrich_interface.result.ty else {
            panic!("enrich result must remain a closed object scheme");
        };
        assert!(enrich_parameter.open);
        assert!(!enrich_result.open);
        let mut enrich_alphas = BTreeSet::new();
        for field in ["item_kind", "id"] {
            let Some(Type::Var(parameter)) = enrich_parameter.fields.get(field) else {
                panic!("enrich parameter must retain generic {field}");
            };
            let Some(Type::Var(result)) = enrich_result.fields.get(field) else {
                panic!("enrich result must reuse generic {field}");
            };
            assert_eq!(parameter, result, "enrich {field} alpha");
            enrich_alphas.insert(*parameter);
        }
        assert_eq!(enrich_alphas.len(), 2);
        assert_eq!(enrich_result.fields.get("rich_label"), Some(&Type::Text));

        let dispatch_interface = interface_for(&session, &dispatch);
        let Type::Object(dispatch_parameter) = &dispatch_interface.parameters[0].flow_type.ty
        else {
            panic!("dispatch parameter must remain an open object scheme");
        };
        assert!(dispatch_parameter.open);
        let Some(Type::Var(dispatch_kind_alpha)) = dispatch_parameter.fields.get("item_kind")
        else {
            panic!("dispatch parameter must retain generic item_kind");
        };
        let Some(Type::Var(dispatch_id_alpha)) = dispatch_parameter.fields.get("id") else {
            panic!("dispatch parameter must retain generic id");
        };
        assert_ne!(dispatch_kind_alpha, dispatch_id_alpha);
        assert!(
            !dispatch_parameter.fields.contains_key("rich_label"),
            "the rich branch must not flow its authored field back into the generic scheme",
        );
        let Type::Union(principal_members) = &dispatch_interface.result.ty else {
            panic!(
                "dispatch scheme must retain its broad rich-or-sparse principal: {:#?}",
                dispatch_interface.result,
            );
        };
        assert_eq!(principal_members.len(), 2, "dispatch principal members");
        let rich_principal = principal_members
            .iter()
            .find_map(|member| match member {
                Type::Object(shape) if shape.fields.get("rich_label") == Some(&Type::Text) => {
                    Some(shape)
                }
                _ => None,
            })
            .expect("dispatch principal rich member");
        let sparse_principal = principal_members
            .iter()
            .find_map(|member| match member {
                Type::Object(shape) if !shape.fields.contains_key("rich_label") => Some(shape),
                _ => None,
            })
            .expect("dispatch principal sparse member");
        assert!(!rich_principal.open);
        assert!(sparse_principal.open);
        for (field, alpha) in [
            ("item_kind", dispatch_kind_alpha),
            ("id", dispatch_id_alpha),
        ] {
            assert_eq!(
                rich_principal.fields.get(field),
                Some(&Type::Var(*alpha)),
                "rich principal {field} alpha",
            );
        }
        assert_eq!(
            sparse_principal.fields.get("item_kind"),
            Some(&Type::Var(*dispatch_kind_alpha)),
            "sparse principal selector alpha",
        );
        assert_eq!(rich_principal.fields.get("rich_label"), Some(&Type::Text));
        assert!(!sparse_principal.fields.contains_key("rich_label"));
        let rich_body = session
            .owner_body_inference(project, &rich_rows)
            .unwrap()
            .expect("rich_rows body inference");
        let dispatch_call = rich_body
            .calls
            .iter()
            .find(|call| call.function == "dispatch")
            .expect("rich_rows dispatch call");
        let dispatch_actual = dispatch_call
            .inputs
            .iter()
            .find(|input| {
                matches!(
                    &input.role,
                    OwnerConstraintEdgeRole::CallArgument { name, .. } if name == "row"
                )
            })
            .expect("dispatch row actual");
        assert_eq!(
            dispatch_actual.actual_type,
            Type::Object((*source_item).clone()),
            "dispatch must observe the closed source row before its generic formal",
        );
        assert!(
            dispatch_call.syntax_discriminated_result,
            "singleton VariableRow must select the rich WHEN arm: {dispatch_call:#?}",
        );
        assert_rich_object(
            &dispatch_call.result.ty,
            &variable_row,
            "dispatch occurrence result",
        );
        let producer_map = rich_body
            .calls
            .iter()
            .find(|call| call.function == "List/map")
            .expect("rich_rows List/map call");
        assert_rich_list(
            &producer_map.result.ty,
            &variable_row,
            "producer List/map result",
        );

        let rich_interface = interface_for(&session, &rich_rows);
        assert_rich_list(
            &rich_interface.result.ty,
            &variable_row,
            "rich_rows public interface",
        );
        let store_interface = interface_for(&session, &store);
        let Type::Object(store_result) = &store_interface.result.ty else {
            panic!("focused store must publish its record: {store_interface:#?}");
        };
        assert_eq!(
            store_result.fields.get("rich_rows"),
            Some(&rich_interface.result.ty),
            "the containing store interface must compose the final child endpoint",
        );
        let visible_interface = interface_for(&session, &visible_rows);
        assert_eq!(
            visible_interface.result.ty, rich_interface.result.ty,
            "five List/filter calls must retain their captured provider exactly",
        );
        assert_rich_list(
            &visible_interface.result.ty,
            &variable_row,
            "visible_rows public interface",
        );
        let visible_body = session
            .owner_body_inference(project, &visible_rows)
            .unwrap()
            .expect("visible_rows body inference");
        let visible_filters = visible_body
            .calls
            .iter()
            .filter(|call| call.function == "List/filter")
            .collect::<Vec<_>>();
        assert_eq!(visible_filters.len(), 5, "visible_rows filter count");
        for call in visible_filters {
            let pipe = call
                .inputs
                .iter()
                .find(|input| matches!(input.role, OwnerConstraintEdgeRole::PipeInput))
                .expect("visible_rows filter pipe input");
            assert_rich_list(
                &pipe.actual_type,
                &variable_row,
                "visible_rows filter pipe input",
            );
            assert_rich_list(&call.result.ty, &variable_row, "visible_rows filter result");
        }
        let labels_body = session
            .owner_body_inference(project, &rich_labels)
            .unwrap()
            .expect("rich_labels body inference");
        let labels_map = labels_body
            .calls
            .iter()
            .find(|call| call.function == "List/map")
            .expect("rich_labels List/map call");
        let labels_pipe = labels_map
            .inputs
            .iter()
            .find(|input| matches!(input.role, OwnerConstraintEdgeRole::PipeInput))
            .expect("rich_labels List/map pipe input");
        assert_eq!(
            labels_pipe.actual_type, rich_interface.result.ty,
            "sibling must consume the producer's exact public type",
        );
        assert_rich_list(
            &labels_pipe.actual_type,
            &variable_row,
            "rich_labels pipe input",
        );
        assert_eq!(
            labels_map.result.ty,
            Type::List(Type::shared(Type::Text)),
            "rich_labels List/map result",
        );
        let labels_interface = interface_for(&session, &rich_labels);
        assert_eq!(
            labels_interface.result.ty,
            Type::List(Type::shared(Type::Text)),
        );

        assert_eq!(source_interface, interface_for(&session, &source_rows));
        assert_eq!(enrich_interface, interface_for(&session, &enrich));
        assert_eq!(dispatch_interface, interface_for(&session, &dispatch));
        assert_eq!(rich_interface, interface_for(&session, &rich_rows));
        assert_eq!(visible_interface, interface_for(&session, &visible_rows));

        let lane_interface = interface_for(&session, &lane_rows);
        let Type::List(lane_item) = &lane_interface.result.ty else {
            panic!("lane_rows must publish a list: {lane_interface:#?}");
        };
        let Type::Object(lane_item) = lane_item.as_ref() else {
            panic!("lane_rows must publish record items: {lane_interface:#?}");
        };
        assert!(!lane_item.open);
        assert_eq!(lane_item.fields.get("item_kind"), Some(&variable_row));
        for field in ["id", "label", "family", "digest"] {
            assert_eq!(
                lane_item.fields.get(field),
                Some(&Type::Text),
                "lane {field}"
            );
        }
        assert_eq!(
            lane_item.fields.get("canvas_width"),
            Some(&Type::Number),
            "lane canvas_width",
        );
        let lane_scheme = interface_for(&session, &lane);
        let Type::Object(lane_parameter) = &lane_scheme.parameters[0].flow_type.ty else {
            panic!("lane parameter must retain its structural scheme: {lane_scheme:#?}");
        };
        assert!(lane_parameter.open);
        for field in ["item_kind", "id", "rich_label", "family", "selected"] {
            assert!(
                matches!(lane_parameter.fields.get(field), Some(Type::Var(_))),
                "lane parameter must retain generic {field}: {lane_scheme:#?}",
            );
        }
        assert!(
            matches!(lane_parameter.fields.get("segments"), Some(Type::List(_))),
            "lane parameter must retain its generic segment list: {lane_scheme:#?}",
        );
        let lane_body = session
            .owner_body_inference(project, &lane_rows)
            .unwrap()
            .expect("lane_rows body inference");
        let lane_call = lane_body
            .calls
            .iter()
            .find(|call| call.function == "lane")
            .expect("lane_rows dispatcher call");
        let lane_actual = lane_call
            .inputs
            .iter()
            .find(|input| {
                matches!(
                    &input.role,
                    OwnerConstraintEdgeRole::CallArgument { name, .. } if name == "row"
                )
            })
            .expect("lane row actual");
        assert_rich_object(&lane_actual.actual_type, &variable_row, "lane row actual");
        assert!(boon_checked::type_is_recursively_closed(
            &lane_call.result.ty
        ));

        let (_, assembly, _, _, _) =
            parse_project_snapshot(session.projects.get_mut(&project).unwrap()).unwrap();
        let fields = assembly.fields();
        let rich_declaration = fields
            .declarations
            .iter()
            .find(|declaration| declaration.name == "rich_rows")
            .expect("rich_rows checked declaration");
        assert_rich_list(
            &rich_declaration.flow_type.ty,
            &variable_row,
            "rich_rows checked declaration",
        );
        let visible_declaration = fields
            .declarations
            .iter()
            .find(|declaration| declaration.name == "visible_rows")
            .expect("visible_rows checked declaration");
        assert_eq!(
            visible_declaration.flow_type.ty, rich_declaration.flow_type.ty,
            "checked visible_rows must retain rich_rows exactly",
        );
        let labels_declaration = fields
            .declarations
            .iter()
            .find(|declaration| declaration.name == "rich_labels")
            .expect("rich_labels checked declaration");
        assert_eq!(
            labels_declaration.flow_type.ty,
            Type::List(Type::shared(Type::Text)),
        );
        let lane_callable = fields
            .callables
            .iter()
            .find(|callable| callable.name == "lane")
            .expect("lane checked callable");
        assert!(matches!(
            lane_callable.parameters[0].flow_type.ty,
            Type::Object(_)
        ));
        let lane_declaration = fields
            .declarations
            .iter()
            .find(|declaration| declaration.name == "lane_rows")
            .expect("lane_rows checked declaration");
        assert_eq!(lane_declaration.flow_type.ty, lane_interface.result.ty);

        let revision = session.revision(project).unwrap();
        session
            .request(
                project,
                revision,
                CompileIntent::VerifiedPreview,
                &CancellationToken::new(),
            )
            .unwrap()
            .compiled()
            .expect("syntax-selected fixture must publish a verified plan");
    }

    #[test]
    #[ignore = "large NovyWave owner assembly regression"]
    fn novywave_owner_assembly_preserves_tagged_arm_projection_types() {
        fn contains_unknown_type(ty: &boon_checked::Type) -> bool {
            match ty {
                boon_checked::Type::Unknown => true,
                boon_checked::Type::Object(shape) => {
                    shape.fields.values().any(contains_unknown_type)
                }
                boon_checked::Type::List(item) | boon_checked::Type::Set(item) => {
                    contains_unknown_type(item)
                }
                boon_checked::Type::Map { key, value } => {
                    contains_unknown_type(key) || contains_unknown_type(value)
                }
                boon_checked::Type::Function { args, result } => {
                    args.iter().any(contains_unknown_type) || contains_unknown_type(&result.ty)
                }
                boon_checked::Type::VariantSet(variants) => variants.iter().any(|variant| {
                    let boon_checked::Variant::Tagged { fields, .. } = variant else {
                        return false;
                    };
                    fields.fields.values().any(contains_unknown_type)
                }),
                boon_checked::Type::Union(members) => members.iter().any(contains_unknown_type),
                boon_checked::Type::Text
                | boon_checked::Type::Number
                | boon_checked::Type::Bytes(_)
                | boon_checked::Type::Bits { .. }
                | boon_checked::Type::Absent
                | boon_checked::Type::RenderContract
                | boon_checked::Type::UnresolvedShape { .. }
                | boon_checked::Type::Var(_) => false,
            }
        }

        fn type_at_object_path<'a>(
            mut ty: &'a boon_checked::Type,
            path: &[&str],
        ) -> Option<&'a boon_checked::Type> {
            for field in path {
                let boon_checked::Type::Object(shape) = ty else {
                    return None;
                };
                ty = shape.fields.get(*field)?;
            }
            Some(ty)
        }

        fn unresolved_type_paths(ty: &boon_checked::Type) -> Vec<String> {
            fn visit(ty: &boon_checked::Type, path: &str, unresolved: &mut Vec<String>) {
                match ty {
                    boon_checked::Type::Var(variable) => {
                        unresolved.push(format!("{path}: Var({})", variable.0));
                    }
                    boon_checked::Type::Unknown => {
                        unresolved.push(format!("{path}: Unknown"));
                    }
                    boon_checked::Type::UnresolvedShape { reason } => {
                        unresolved.push(format!("{path}: UnresolvedShape({reason})"));
                    }
                    boon_checked::Type::Object(shape) => {
                        if shape.open {
                            unresolved.push(format!("{path}: open Object"));
                        }
                        for (field, ty) in &shape.fields {
                            visit(ty, &format!("{path}.{field}"), unresolved);
                        }
                    }
                    boon_checked::Type::List(item) => {
                        visit(item, &format!("{path}[]"), unresolved);
                    }
                    boon_checked::Type::Set(item) => {
                        visit(item, &format!("{path}{{}}"), unresolved);
                    }
                    boon_checked::Type::Map { key, value } => {
                        visit(key, &format!("{path}.key"), unresolved);
                        visit(value, &format!("{path}.value"), unresolved);
                    }
                    boon_checked::Type::Function { args, result } => {
                        for (index, arg) in args.iter().enumerate() {
                            visit(arg, &format!("{path}.arg[{index}]"), unresolved);
                        }
                        visit(&result.ty, &format!("{path}.result"), unresolved);
                    }
                    boon_checked::Type::VariantSet(variants) => {
                        for variant in variants.iter() {
                            let boon_checked::Variant::Tagged { tag, fields } = variant else {
                                continue;
                            };
                            if fields.open {
                                unresolved.push(format!("{path}<{tag}>: open payload"));
                            }
                            for (field, ty) in &fields.fields {
                                visit(ty, &format!("{path}<{tag}>.{field}"), unresolved);
                            }
                        }
                    }
                    boon_checked::Type::Union(members) => {
                        for (index, member) in members.iter().enumerate() {
                            visit(member, &format!("{path}|{index}"), unresolved);
                        }
                    }
                    boon_checked::Type::Text
                    | boon_checked::Type::Number
                    | boon_checked::Type::Bytes(_)
                    | boon_checked::Type::Bits { .. }
                    | boon_checked::Type::Absent
                    | boon_checked::Type::RenderContract => {}
                }
            }

            let mut unresolved = Vec::new();
            visit(ty, "$", &mut unresolved);
            unresolved
        }

        let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/novywave/RUN.bn");
        let (entrypoint, units) = crate::compiler_source_project_for_path(&source).unwrap();
        let mut session = CompilerSession::new();
        let project = session
            .open_project(CompilerProject::new(
                entrypoint,
                units.clone(),
                TargetProfile::SoftwareDefault,
                ProgramRole::Client,
                ApplicationIdentity::compiler_default(),
            ))
            .unwrap();
        let (diagnostic_syntax, _, _, _, _) =
            parse_project_diagnostics_snapshot(session.projects.get_mut(&project).unwrap())
                .unwrap();
        let owner_named = |name: &str| {
            diagnostic_syntax
                .stable_check_owner_keys()
                .find(|owner| {
                    matches!(
                        owner,
                        StableCheckOwnerKey::Item(item)
                            if item.item_route.segments().last().is_some_and(|segment| {
                                segment.names.as_ref() == [name]
                            })
                    )
                })
                .unwrap_or_else(|| panic!("missing NovyWave owner {name}"))
        };
        let file_tree_rows = owner_named("file_tree_rows");
        let signal_catalog = owner_named("signal_catalog");
        let row_selected_signal_key = owner_named("row_selected_signal_key");
        let real_waveform_segment = owner_named("real_waveform_segment");
        let selected_signal_defaults = owner_named("selected_signal_defaults");
        let selected_visible_items = owner_named("selected_visible_items");
        let selected_signal_lane_rows = owner_named("selected_signal_lane_rows");
        let new_signal_lane_row = owner_named("new_signal_lane_row");
        let store = owner_named("store");
        let bridge_request_compact_label = owner_named("bridge_request_compact_label");
        let root = owner_named("root");
        let material = owner_named("material");
        {
            let state = session.projects.get(&project).unwrap();
            let current_interface = |owner: &StableCheckOwnerKey| {
                let provider = state
                    .owner_interface_provider_requests
                    .current_value(&state.syntax_evaluator, owner)
                    .unwrap()
                    .unwrap();
                let result = state
                    .owner_interface_scc_requests
                    .current_value(&state.syntax_evaluator, provider.key())
                    .unwrap()
                    .unwrap();
                result
                    .owner(owner)
                    .expect("NovyWave owner public interface")
                    .clone()
            };
            let provider = state
                .owner_interface_provider_requests
                .current_value(&state.syntax_evaluator, &file_tree_rows)
                .unwrap()
                .unwrap();
            let result = state
                .owner_interface_scc_requests
                .current_value(&state.syntax_evaluator, provider.key())
                .unwrap()
                .unwrap();
            let interface = result
                .owner(&file_tree_rows)
                .expect("file_tree_rows public interface");
            assert!(
                boon_checked::type_is_recursively_closed(&interface.result.ty),
                "file_tree_rows must publish a closed result before its sibling consumer: {:#?}",
                interface.result,
            );

            let provider = state
                .owner_interface_provider_requests
                .current_value(&state.syntax_evaluator, &signal_catalog)
                .unwrap()
                .unwrap();
            let result = state
                .owner_interface_scc_requests
                .current_value(&state.syntax_evaluator, provider.key())
                .unwrap()
                .unwrap();
            let interface = result
                .owner(&signal_catalog)
                .expect("signal_catalog public interface");
            assert!(
                boon_checked::type_is_recursively_closed(&interface.result.ty),
                "signal_catalog must publish a closed result before its sibling consumer: {:#?}",
                interface.result,
            );
            let boon_checked::Type::List(item) = &interface.result.ty else {
                panic!(
                    "signal_catalog must publish a list: {:#?}",
                    interface.result
                );
            };
            let boon_checked::Type::Object(item) = item.as_ref() else {
                panic!(
                    "signal_catalog must publish record items: {:#?}",
                    interface.result
                );
            };
            assert_eq!(item.fields.get("key"), Some(&boon_checked::Type::Text));

            for (owner, name) in [
                (&selected_signal_defaults, "selected_signal_defaults"),
                (&selected_visible_items, "selected_visible_items"),
            ] {
                let provider = state
                    .owner_interface_provider_requests
                    .current_value(&state.syntax_evaluator, owner)
                    .unwrap()
                    .unwrap();
                let result = state
                    .owner_interface_scc_requests
                    .current_value(&state.syntax_evaluator, provider.key())
                    .unwrap()
                    .unwrap();
                let interface = result.owner(owner).unwrap_or_else(|| {
                    panic!("{name} public interface is missing from its component")
                });
                assert!(
                    boon_checked::type_is_recursively_closed(&interface.result.ty),
                    "{name} must publish a recursively closed list: {interface:#?}",
                );
            }
            let bridge_interface = current_interface(&bridge_request_compact_label);
            assert_eq!(
                bridge_interface.result.ty,
                boon_checked::Type::Text,
                "bridge request compact label must publish Text",
            );
            let store_interface = current_interface(&store);
            let boon_checked::Type::Object(store_result) = &store_interface.result.ty else {
                panic!("NovyWave store must publish its record boundary: {store_interface:#?}");
            };
            assert_eq!(
                store_result.fields.get("bridge_request_compact_label"),
                Some(&boon_checked::Type::Text),
                "the parent store boundary must observe the child's closed public result",
            );
            assert_eq!(
                type_at_object_path(
                    &store_interface.result.ty,
                    &["bridge_request_descriptor", "identity", "time_unit"],
                ),
                Some(&boon_checked::Type::Text),
                "the store interface must carry the closed HOLD update type through bridge_request_descriptor.identity.time_unit",
            );
            let provider = state
                .owner_interface_provider_requests
                .current_value(&state.syntax_evaluator, &new_signal_lane_row)
                .unwrap()
                .unwrap();
            let result = state
                .owner_interface_scc_requests
                .current_value(&state.syntax_evaluator, provider.key())
                .unwrap()
                .unwrap();
            let interface = result
                .owner(&new_signal_lane_row)
                .expect("new_signal_lane_row public interface");
            assert!(
                matches!(
                    &interface.parameters[0].flow_type.ty,
                    boon_checked::Type::Object(_)
                ),
                "new_signal_lane_row must retain its structural parameter scheme: {interface:#?}",
            );
            assert!(
                !contains_unknown_type(&interface.parameters[0].flow_type.ty),
                "new_signal_lane_row parameter fields must remain generic variables, never ABI placeholders: {interface:#?}",
            );
        }
        let root_body = session
            .owner_body_inference(project, &root)
            .unwrap()
            .expect("NovyWave root body inference");
        let app_background_material = root_body
            .calls
            .iter()
            .find(|call| {
                call.function == "NovyTheme/material"
                    && matches!(
                        &call.target,
                        boon_typecheck::InferredOwnerCallableTarget::Owner { owner }
                            if owner == &material
                    )
                    && call.inputs.iter().any(|input| {
                        matches!(
                            &input.role,
                            boon_typecheck::OwnerConstraintEdgeRole::CallArgument {
                                name,
                                ..
                            } if name == "of"
                        ) && matches!(
                            &input.actual_type,
                            boon_checked::Type::VariantSet(variants)
                                if variants.len() == 1
                                    && matches!(
                                        variants.first(),
                                        Some(boon_checked::Variant::Tag(tag))
                                            if tag == "AppBackground"
                                    )
                        )
                    })
            })
            .expect("root NovyTheme/material(of: AppBackground) occurrence");
        assert!(
            app_background_material.valid,
            "root material occurrence must be valid: {app_background_material:#?}",
        );
        assert!(
            app_background_material.syntax_discriminated_result,
            "AppBackground must select its exact material arm: {app_background_material:#?}",
        );
        let boon_checked::Type::Object(material_result) = &app_background_material.result.ty else {
            panic!(
                "AppBackground material must be one closed object: {app_background_material:#?}"
            );
        };
        assert!(
            !material_result.open,
            "AppBackground material must be closed: {app_background_material:#?}",
        );
        assert_eq!(
            material_result.fields.len(),
            2,
            "AppBackground material fields: {app_background_material:#?}",
        );
        assert_eq!(
            material_result.fields.get("color"),
            Some(&boon_checked::Type::Text),
            "AppBackground material color",
        );
        assert_eq!(
            material_result.fields.get("gloss"),
            Some(&boon_checked::Type::Number),
            "AppBackground material gloss",
        );
        assert!(
            boon_checked::type_is_recursively_closed(&app_background_material.result.ty),
            "AppBackground material result must be recursively closed: {app_background_material:#?}",
        );
        let (_, assembly, _, _, _) =
            parse_project_snapshot(session.projects.get_mut(&project).unwrap()).unwrap();
        let fields = assembly.fields();
        if std::env::var_os("BOON_DEBUG_NOVY_OUT").is_some() {
            std::fs::write(
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../../target/novywave-owner-checked-current.toml"),
                toml::to_string(fields).unwrap(),
            )
            .unwrap();
        }
        let store_declaration = fields
            .declarations
            .iter()
            .find(|declaration| {
                declaration.name == "store"
                    && type_at_object_path(
                        &declaration.flow_type.ty,
                        &["bridge_request_descriptor", "identity", "time_unit"],
                    )
                    .is_some()
            })
            .expect("NovyWave assembled public store declaration");
        assert_eq!(
            type_at_object_path(
                &store_declaration.flow_type.ty,
                &["bridge_request_descriptor", "identity", "time_unit"],
            ),
            Some(&boon_checked::Type::Text),
            "assembled store must preserve the exact closed HOLD update type",
        );
        let default_file = fields
            .declarations
            .iter()
            .find(|declaration| declaration.name == "default_file_tree_selected_file")
            .expect("NovyWave default file-tree selection declaration");
        assert_eq!(
            default_file.flow_type.ty,
            boon_checked::Type::Text,
            "the public default file-tree selection must remain Text",
        );
        let default_file_value = default_file
            .value
            .and_then(|expression| fields.expressions.get(expression.0 as usize))
            .expect("default file-tree selection value expression");
        assert_eq!(
            default_file_value.flow_type.ty,
            boon_checked::Type::Text,
            "the owner-body List/latest occurrence must retain its closed Text FreshOut projection",
        );
        let declaration = fields
            .declarations
            .iter()
            .find(|declaration| declaration.name == "comparison_cursor_values_result")
            .expect("comparison cursor values declaration");
        let reads = fields
            .expressions
            .iter()
            .filter(|expression| {
                matches!(
                    &expression.kind,
                    boon_checked::CheckedExpressionKind::Read {
                        target,
                        projection,
                        ..
                    } if target == &declaration.id && projection.as_ref() == ["rows"]
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(reads.len(), 1, "tagged arm rows reads: {reads:#?}");
        assert!(
            reads.iter().all(|expression| {
                matches!(
                    &expression.flow_type.ty,
                    boon_checked::Type::List(item)
                        if matches!(
                            item.as_ref(),
                            boon_checked::Type::Object(shape)
                                if shape.fields.get("signal_id") == Some(&boon_checked::Type::Text)
                                    && matches!(
                                        shape.fields.get("value"),
                                        Some(boon_checked::Type::VariantSet(_))
                                    )
                        )
                ) && !format!("{:?}", expression.flow_type.ty).contains("Var(")
            }),
            "tagged arm rows reads: {reads:#?}",
        );
        let legacy = crate::check_diagnostics_source(crate::CompilerCheckRequest::source_units(
            "examples/novywave/RUN.bn",
            &units,
            ProgramRole::Client,
        ))
        .unwrap();
        assert!(
            legacy.output.report.diagnostics.is_empty(),
            "legacy NovyWave diagnostics: {:#?}",
            legacy.output.report.diagnostics,
        );

        let revision = session.revision(project).unwrap();
        let token = CancellationToken::new();
        let diagnostic_result = session
            .request(project, revision, CompileIntent::Diagnostics, &token)
            .unwrap();
        let diagnostics = diagnostic_result
            .diagnostics()
            .expect("NovyWave diagnostics projection");
        assert!(
            diagnostics.diagnostics().is_empty(),
            "owner NovyWave diagnostics: {:#?}",
            diagnostics.diagnostics(),
        );
        let signal_catalog_body = session
            .owner_body_inference(project, &signal_catalog)
            .unwrap()
            .expect("signal_catalog body inference");
        for function in ["real_signal_catalog_row", "new_signal", "List/map"] {
            let call = signal_catalog_body
                .calls
                .iter()
                .find(|call| call.function == function)
                .unwrap_or_else(|| panic!("missing signal_catalog call {function}"));
            assert!(
                call.valid && boon_checked::type_is_recursively_closed(&call.result.ty),
                "signal_catalog call {function} must close its occurrence result: {call:#?}",
            );
        }
        let waveform_body = session
            .owner_body_inference(project, &real_waveform_segment)
            .unwrap()
            .expect("real_waveform_segment body inference");
        for function in ["real_waveform_value_state", "real_waveform_value_text"] {
            let call = waveform_body
                .calls
                .iter()
                .find(|call| call.function == function)
                .unwrap_or_else(|| panic!("missing real_waveform_segment call {function}"));
            assert!(
                call.valid && boon_checked::type_is_recursively_closed(&call.result.ty),
                "real_waveform_segment call {function} must close every evaluated arm: {call:#?}",
            );
        }
        let lane_body = session
            .owner_body_inference(project, &selected_signal_lane_rows)
            .unwrap()
            .expect("selected_signal_lane_rows body inference");
        let lane_map = lane_body
            .calls
            .iter()
            .find(|call| call.function == "List/map")
            .expect("selected_signal_lane_rows List/map call");
        let lane_call = lane_body
            .calls
            .iter()
            .find(|call| call.function == "new_signal_lane_row")
            .expect("selected_signal_lane_rows dispatcher call");
        let lane_actual = lane_call
            .inputs
            .iter()
            .find(|input| {
                matches!(
                    &input.role,
                    boon_typecheck::OwnerConstraintEdgeRole::CallArgument { name, .. }
                        if name == "row"
                )
            })
            .expect("new_signal_lane_row row actual");
        assert!(
            boon_checked::type_is_recursively_closed(&lane_actual.actual_type),
            "new_signal_lane_row must receive the exact closed map item: {lane_call:#?}",
        );
        let lane_result_is_structural = match &lane_call.result.ty {
            boon_checked::Type::Object(_) => true,
            boon_checked::Type::Union(members) => members
                .iter()
                .all(|member| matches!(member, boon_checked::Type::Object(_))),
            _ => false,
        };
        assert!(
            lane_result_is_structural && !contains_unknown_type(&lane_call.result.ty),
            "new_signal_lane_row occurrence must retain its structural result before OUT elaboration: {lane_call:#?}",
        );
        assert!(
            boon_checked::type_is_recursively_closed(&lane_call.result.ty),
            "new_signal_lane_row occurrence must close before OUT elaboration; unresolved paths: {}",
            unresolved_type_paths(&lane_call.result.ty).join(", "),
        );
        let lane_input = lane_map
            .inputs
            .iter()
            .find(|input| {
                matches!(
                    input.role,
                    boon_typecheck::OwnerConstraintEdgeRole::PipeInput
                )
            })
            .expect("selected_signal_lane_rows List/map pipe input");
        assert!(
            boon_checked::type_is_recursively_closed(&lane_input.actual_type),
            "selected_signal_lane_rows must consume the closed visible list: {lane_map:#?}",
        );
        assert!(
            boon_checked::type_is_recursively_closed(&lane_map.result.ty),
            "selected_signal_lane_rows List/map must publish a closed result; unresolved paths: {}",
            unresolved_type_paths(&lane_map.result.ty).join(", "),
        );
        let body = session
            .owner_body_inference(project, &row_selected_signal_key)
            .unwrap()
            .expect("row_selected_signal_key body inference");
        let map = body
            .calls
            .iter()
            .find(|call| call.function == "List/map")
            .expect("row_selected_signal_key List/map call");
        let list = map
            .inputs
            .iter()
            .find(|input| {
                matches!(
                    input.role,
                    boon_typecheck::OwnerConstraintEdgeRole::PipeInput
                )
            })
            .expect("row_selected_signal_key List/map pipe input");
        assert!(
            matches!(&list.actual_type, boon_checked::Type::List(_))
                || matches!(
                    &list.actual_type,
                    boon_checked::Type::Union(members)
                        if members.iter().all(|member| matches!(member, boon_checked::Type::List(_)))
                ),
            "row_selected_signal_key List/map must retain its provider list before OUT specialization: {map:#?}",
        );
        session
            .request(project, revision, CompileIntent::VerifiedPreview, &token)
            .unwrap()
            .compiled()
            .expect("NovyWave must publish a verified compiled plan");
    }

    #[test]
    fn owner_diagnostics_aggregate_includes_signature_lexical_errors_without_checked_shards() {
        let mut session = CompilerSession::new();
        let project = session
            .open_project(project(concat!(
                "FUNCTION fill_both(first: OUT, second: OUT) {\n",
                "    first\n",
                "}\n",
                "FUNCTION caller(existing) {\n",
                "    values: LIST { existing }\n",
                "    fill_both(first, second: existing)\n",
                "}\n",
            )))
            .unwrap();
        let (_, aggregate, _, _, _) =
            parse_project_diagnostics_snapshot(session.projects.get_mut(&project).unwrap())
                .unwrap();
        assert!(
            aggregate.diagnostics().iter().any(|diagnostic| {
                diagnostic.message
                    == "no enclosing OUT named `existing` exists for output parameter `second`"
            }),
            "aggregate diagnostics: {aggregate:#?}"
        );

        let state = session.projects.get(&project).unwrap();
        assert_eq!(state.owner_construction_abi_requests.request_count(), 0);
        assert_eq!(state.checked_owner_shard_requests.request_count(), 0);
        assert_eq!(
            state
                .checked_owner_project_assembly_requests
                .request_count(),
            0
        );
    }

    #[test]
    fn project_diagnostic_facts_match_dense_project_diagnostics() {
        fn normalized_complete_diagnostics(
            checked: &CheckedSourceFromSource,
        ) -> Vec<boon_checked::TypeDiagnostic> {
            let mut diagnostics = checked.output.report.diagnostics.clone();
            diagnostics.extend(
                checked
                    .output
                    .report
                    .render_slot_table
                    .slots
                    .iter()
                    .flat_map(|slot| slot.diagnostics.iter().cloned()),
            );
            diagnostics.sort_by(|left, right| {
                let severity = |severity| match severity {
                    boon_checked::DiagnosticSeverity::Error => 0u8,
                    boon_checked::DiagnosticSeverity::Warning => 1u8,
                };
                (
                    left.line,
                    left.start,
                    left.end,
                    severity(left.severity),
                    &left.message,
                )
                    .cmp(&(
                        right.line,
                        right.start,
                        right.end,
                        severity(right.severity),
                        &right.message,
                    ))
            });
            diagnostics.dedup();
            diagnostics
        }

        fn normalized_invalid_oracle<'a>(
            diagnostics: impl IntoIterator<Item = &'a boon_checked::TypeDiagnostic>,
        ) -> BTreeSet<(u8, String)> {
            diagnostics
                .into_iter()
                .filter(|diagnostic| {
                    // The old dense checker emitted additional diagnostics
                    // containing revision-local declaration ids for the same
                    // duplicate declaration already represented by the stable
                    // record/output diagnostic. They are intentionally absent
                    // from the normalized source-level contract. The legacy
                    // checker also failed to bind the response output root in
                    // the host-port declaration and classified a read of a
                    // duplicated record-local field as an unknown project
                    // value. The exact owner lexical plan fixes both false
                    // positives while retaining the stable duplicate error.
                    !diagnostic.message.contains("conflicts with declaration ")
                        && !diagnostic
                            .message
                            .contains("canonical checked value contains an expansion cycle")
                        && diagnostic.message != "unknown identifier `response`"
                        && diagnostic.message != "unknown identifier `item`"
                        && diagnostic.message != "unknown path `PASSED.store`"
                })
                .map(|diagnostic| {
                    let severity = match diagnostic.severity {
                        boon_checked::DiagnosticSeverity::Error => 0,
                        boon_checked::DiagnosticSeverity::Warning => 1,
                    };
                    let quoted = diagnostic
                        .message
                        .split('`')
                        .skip(1)
                        .step_by(2)
                        .collect::<Vec<_>>();
                    let message = if (diagnostic.message.contains("unexpected extra call entry")
                        || diagnostic.message.contains("does not accept argument"))
                        && quoted.len() >= 2
                    {
                        format!("unexpected call entry `{}`.`{}`", quoted[0], quoted[1])
                    } else {
                        match diagnostic.message.as_str() {
                            "`PASSED` has no enclosing callable context"
                            | "`PASSED` is only available inside a user callable" => {
                                "unbound `PASSED` callable context".to_owned()
                            }
                            _ => diagnostic.message.clone(),
                        }
                    };
                    (severity, message)
                })
                .collect()
        }

        let fixtures = [
            (
                ProgramRole::Server,
                concat!(
                    "FUNCTION same() {\n",
                    "    1\n",
                    "}\n",
                    "FUNCTION same() {\n",
                    "    2\n",
                    "}\n",
                    "value: 1\n",
                ),
            ),
            (
                ProgramRole::Client,
                concat!(
                    "FUNCTION sized_box(size) {\n",
                    "    Element/container(\n",
                    "        element: []\n",
                    "        style: [width: size]\n",
                    "        child: Element/label(\n",
                    "            element: []\n",
                    "            style: []\n",
                    "            label: TEXT { child }\n",
                    "        )\n",
                    "    )\n",
                    "}\n",
                    "document: Document/new(root: sized_box(size: TEXT { invalid }))\n",
                ),
            ),
            (
                ProgramRole::Client,
                concat!(
                    "FUNCTION sized_box(size) {\n",
                    "    Element/container(\n",
                    "        element: []\n",
                    "        style: [width: size]\n",
                    "        child: Element/label(\n",
                    "            element: []\n",
                    "            style: []\n",
                    "            label: TEXT { child }\n",
                    "        )\n",
                    "    )\n",
                    "}\n",
                    "document: Document/new(root: sized_box(size: 24))\n",
                ),
            ),
            (
                ProgramRole::Client,
                concat!(
                    "FUNCTION render_indent(width_value) {\n",
                    "    Scene/Element/label(\n",
                    "        element: []\n",
                    "        style: [width: width_value, height: Fill]\n",
                    "        label: Scene/Element/text(\n",
                    "            element: []\n",
                    "            style: [width: Fill, height: Fill]\n",
                    "            text: TEXT { indent }\n",
                    "        )\n",
                    "    )\n",
                    "}\n",
                    "FUNCTION render_row(label_text, selected, row) {\n",
                    "    Scene/Element/stripe(\n",
                    "        element: []\n",
                    "        direction: Row\n",
                    "        items: LIST {\n",
                    "            render_indent(width_value: row.indent_width)\n",
                    "            Scene/Element/text(\n",
                    "                element: []\n",
                    "                style: [width: Fill, height: Fill]\n",
                    "                text: label_text\n",
                    "            )\n",
                    "        }\n",
                    "    )\n",
                    "}\n",
                    "scene: Scene/new(root: render_row(\n",
                    "    label_text: TEXT { label }\n",
                    "    selected: True\n",
                    "    row: [indent_width: 92]\n",
                    "))\n",
                ),
            ),
            (
                ProgramRole::Client,
                concat!(
                    "FUNCTION render_indent(width_value) {\n",
                    "    Scene/Element/label(\n",
                    "        element: []\n",
                    "        style: [width: width_value, height: Fill]\n",
                    "        label: Scene/Element/text(\n",
                    "            element: []\n",
                    "            style: [width: Fill, height: Fill]\n",
                    "            text: TEXT { indent }\n",
                    "        )\n",
                    "    )\n",
                    "}\n",
                    "FUNCTION render_row(label_text, selected, row) {\n",
                    "    Scene/Element/stripe(\n",
                    "        element: []\n",
                    "        direction: Row\n",
                    "        items: LIST {\n",
                    "            render_indent(width_value: row.indent_width)\n",
                    "            Scene/Element/text(\n",
                    "                element: []\n",
                    "                style: [width: Fill, height: Fill]\n",
                    "                text: label_text\n",
                    "            )\n",
                    "        }\n",
                    "    )\n",
                    "}\n",
                    "scene: Scene/new(root: render_row(\n",
                    "    label_text: TEXT { label }\n",
                    "    selected: True\n",
                    "    row: [indent_width: TEXT { invalid }]\n",
                    "))\n",
                ),
            ),
            (
                ProgramRole::Client,
                concat!(
                    "FUNCTION sized_box(size) {\n",
                    "    Element/container(\n",
                    "        element: []\n",
                    "        style: [width: size]\n",
                    "        child: Element/label(\n",
                    "            element: []\n",
                    "            style: []\n",
                    "            label: TEXT { child }\n",
                    "        )\n",
                    "    )\n",
                    "}\n",
                    "FUNCTION forwarded(value) {\n",
                    "    sized_box(size: value)\n",
                    "}\n",
                    "document: Document/new(root: forwarded(value: TEXT { invalid }))\n",
                ),
            ),
            (
                ProgramRole::Client,
                concat!(
                    "FUNCTION sized_box_child(size) {\n",
                    "    BLOCK {\n",
                    "        rendered: Element/container(\n",
                    "            element: []\n",
                    "            style: [width: size]\n",
                    "            child: Element/label(\n",
                    "                element: []\n",
                    "                style: []\n",
                    "                label: TEXT { child }\n",
                    "            )\n",
                    "        )\n",
                    "        rendered\n",
                    "    }\n",
                    "}\n",
                    "document: Document/new(root: sized_box_child(size: TEXT { invalid }))\n",
                ),
            ),
            (
                ProgramRole::Client,
                concat!(
                    "FUNCTION styled_alias(first, second) {\n",
                    "    BLOCK {\n",
                    "        alias: second\n",
                    "        rendered: Element/container(\n",
                    "            element: []\n",
                    "            style: [width: alias]\n",
                    "            child: Element/label(\n",
                    "                element: []\n",
                    "                style: []\n",
                    "                label: TEXT { child }\n",
                    "            )\n",
                    "        )\n",
                    "        rendered\n",
                    "    }\n",
                    "}\n",
                    "document: Document/new(root: styled_alias(first: 24, second: TEXT { invalid }))\n",
                ),
            ),
            (
                ProgramRole::Client,
                concat!(
                    "FUNCTION styled_alias(first, second) {\n",
                    "    BLOCK {\n",
                    "        alias: second\n",
                    "        rendered: Element/container(\n",
                    "            element: []\n",
                    "            style: [width: alias]\n",
                    "            child: Element/label(\n",
                    "                element: []\n",
                    "                style: []\n",
                    "                label: TEXT { child }\n",
                    "            )\n",
                    "        )\n",
                    "        rendered\n",
                    "    }\n",
                    "}\n",
                    "document: Document/new(root: styled_alias(first: TEXT { invalid }, second: 24))\n",
                ),
            ),
            (
                ProgramRole::Client,
                concat!(
                    "FUNCTION identity(value) {\n",
                    "    value\n",
                    "}\n",
                    "FUNCTION styled_call_alias(first, second) {\n",
                    "    BLOCK {\n",
                    "        alias: identity(value: second)\n",
                    "        rendered: Element/container(\n",
                    "            element: []\n",
                    "            style: [width: alias]\n",
                    "            child: Element/label(\n",
                    "                element: []\n",
                    "                style: []\n",
                    "                label: TEXT { child }\n",
                    "            )\n",
                    "        )\n",
                    "        rendered\n",
                    "    }\n",
                    "}\n",
                    "document: Document/new(root: styled_call_alias(first: 24, second: TEXT { invalid }))\n",
                ),
            ),
            (
                ProgramRole::Client,
                concat!(
                    "FUNCTION identity(value) {\n",
                    "    value\n",
                    "}\n",
                    "FUNCTION styled_call_alias(first, second) {\n",
                    "    BLOCK {\n",
                    "        alias: identity(value: second)\n",
                    "        rendered: Element/container(\n",
                    "            element: []\n",
                    "            style: [width: alias]\n",
                    "            child: Element/label(\n",
                    "                element: []\n",
                    "                style: []\n",
                    "                label: TEXT { child }\n",
                    "            )\n",
                    "        )\n",
                    "        rendered\n",
                    "    }\n",
                    "}\n",
                    "document: Document/new(root: styled_call_alias(first: TEXT { invalid }, second: 24))\n",
                ),
            ),
            (
                ProgramRole::Server,
                concat!(
                    "FUNCTION recurse_child() {\n",
                    "    BLOCK {\n",
                    "        next: recurse_child()\n",
                    "        next\n",
                    "    }\n",
                    "}\n",
                    "value: recurse_child()\n",
                ),
            ),
            (
                ProgramRole::Server,
                concat!(
                    "FUNCTION make_nested() {\n",
                    "    BLOCK {\n",
                    "        result: LIST { 1 }\n",
                    "        result\n",
                    "    }\n",
                    "}\n",
                    "collection_parent: LIST { make_nested() }\n",
                ),
            ),
            (
                ProgramRole::Server,
                concat!("outputs: [\n", "    value: 1\n", "    value: 2\n", "]\n",),
            ),
            (
                ProgramRole::Server,
                concat!("outputs: [\n", "    pending:\n", "]\n",),
            ),
            (
                ProgramRole::Server,
                concat!(
                    "store: [\n",
                    "    request: SOURCE\n",
                    "]\n",
                    "outputs: [\n",
                    "    response: 1\n",
                    "]\n",
                    "host_ports: [\n",
                    "    http: [\n",
                    "        request: store.request\n",
                    "        response: response\n",
                    "    ]\n",
                    "]\n",
                ),
            ),
            (ProgramRole::Client, "document: [\n    root: 1\n]\n"),
            (ProgramRole::Client, "document: [\n    root:\n]\n"),
            (
                ProgramRole::Server,
                concat!(
                    "gateway: [\n",
                    "    request: SOURCE\n",
                    "]\n",
                    "outputs: [\n",
                    "    other: 1\n",
                    "]\n",
                    "host_ports: [\n",
                    "    http: [\n",
                    "        request: gateway.request\n",
                    "        response: missing\n",
                    "    ]\n",
                    "]\n",
                ),
            ),
            (
                ProgramRole::Server,
                concat!(
                    "rows: LIST { [rank: 1] }\n",
                    "ordered: rows |> List/then_by(item, key: item.rank, direction: Ascending)\n",
                ),
            ),
            (
                ProgramRole::Server,
                concat!(
                    "rows: LIST { [rank: 1] }\n",
                    "ordered:\n",
                    "    rows\n",
                    "    |> List/sort_by(item, key: item.rank, direction: Ascending)\n",
                    "    |> List/append(item: [rank: 2])\n",
                    "    |> List/then_by(item, key: item.rank, direction: Ascending)\n",
                ),
            ),
            (
                ProgramRole::Server,
                concat!(
                    "FUNCTION sorted(list, entry: OUT, key) {\n",
                    "    list |> List/sort_by(item: entry, key: key, direction: Ascending)\n",
                    "}\n",
                    "rows: LIST { [name: TEXT { Alpha }] }\n",
                    "ordered: rows |> sorted(entry, key: [name: entry.name])\n",
                ),
            ),
            (
                ProgramRole::Server,
                concat!(
                    "rows: LIST { [name: TEXT { Alpha }] }\n",
                    "ordered:\n",
                    "    rows\n",
                    "    |> List/sort_by(\n",
                    "        item\n",
                    "        key: File/read_text(path: item.name)\n",
                    "        direction: Ascending\n",
                    "    )\n",
                ),
            ),
            (
                ProgramRole::Server,
                concat!(
                    "rows: LIST { [rank: TEXT { 1 }] }\n",
                    "ordered: rows |> List/sort_by(item, key: item.rank |> Text/to_number())\n",
                ),
            ),
            (
                ProgramRole::Server,
                concat!(
                    "rows: LIST { [rank: 1, name: TEXT { A }] }\n",
                    "ordered:\n",
                    "    rows\n",
                    "    |> List/sort_by(item, key: item.rank, direction: Ascending)\n",
                    "    |> List/then_by(item, key: item.name, direction: Descending)\n",
                ),
            ),
            (
                ProgramRole::Server,
                concat!(
                    "rows: LIST { [rank: 1] }\n",
                    "ordered:\n",
                    "    True\n",
                    "    |> WHEN {\n",
                    "        True => rows |> List/sort_by(item, key: item.rank)\n",
                    "        False => rows |> List/sort_by(item, key: item.rank, direction: Ascending)\n",
                    "    }\n",
                    "    |> List/then_by(item, key: item.rank)\n",
                ),
            ),
            (
                ProgramRole::Server,
                concat!(
                    "FUNCTION sorted(list, entry: OUT, key, direction) {\n",
                    "    list |> List/sort_by(item: entry, key: key, direction: direction)\n",
                    "}\n",
                    "rows: LIST { [rank: 1] }\n",
                    "ordered:\n",
                    "    True\n",
                    "    |> WHEN {\n",
                    "        True => rows |> sorted(entry, key: entry.rank, direction: Ascending)\n",
                    "        False => rows |> sorted(entry, key: entry.rank, direction: Descending)\n",
                    "    }\n",
                    "    |> List/then_by(item, key: item.rank)\n",
                ),
            ),
            (
                ProgramRole::Server,
                concat!(
                    "rows: LIST { [rank: 1] }\n",
                    "ordered:\n",
                    "    True\n",
                    "    |> WHEN {\n",
                    "        True => rows |> List/sort_by(item, key: item.rank, direction: Ascending)\n",
                    "        False => rows |> List/sort_by(item, key: item.rank, direction: Descending)\n",
                    "    }\n",
                    "    |> List/then_by(item, key: item.rank)\n",
                ),
            ),
            (
                ProgramRole::Server,
                concat!(
                    "rows: LIST { [rank: 1] }\n",
                    "ordered:\n",
                    "    True\n",
                    "    |> WHILE {\n",
                    "        True => rows |> List/sort_by(item, key: item.rank)\n",
                    "        False => rows |> List/sort_by(item, key: item.rank, direction: Ascending)\n",
                    "    }\n",
                    "    |> List/then_by(item, key: item.rank)\n",
                ),
            ),
            (
                ProgramRole::Server,
                concat!(
                    "rows: LIST { [rank: 1] }\n",
                    "ordered: rows |> List/sort_by(item, key: DRAIN { item.rank })\n",
                ),
            ),
            (
                ProgramRole::Server,
                concat!(
                    "FUNCTION sorted(list, item: OUT) {\n",
                    "    list |> List/sort_by(item: item, key: PASSED.key)\n",
                    "}\n",
                    "rows: LIST { [rank: 1] }\n",
                    "ordered: sorted(list: rows, item, PASS: [key: 1])\n",
                ),
            ),
            (ProgramRole::Server, "value: typo\n"),
            (
                ProgramRole::Server,
                "FUNCTION helper() {\n    1\n}\nvalue: helper\n",
            ),
            (
                ProgramRole::Server,
                "value: [item: 1, item: 2, copy: item]\n",
            ),
            (ProgramRole::Server, "value: Node[...1]\n"),
            (ProgramRole::Server, "value: Node[...[same: 1, same: 2]]\n"),
            (
                ProgramRole::Server,
                concat!(
                    "FUNCTION pass_identity(value) {\n",
                    "    context: PASSED.store\n",
                    "    value\n",
                    "}\n",
                    "result: pass_identity(value: 1, PASS: [...1])\n",
                ),
            ),
            (ProgramRole::Server, "value: PASSED.store\n"),
            (
                ProgramRole::Server,
                "value: Number/to_text(value: 1, extra: 2)\n",
            ),
            (
                ProgramRole::Server,
                concat!(
                    "FUNCTION needs(input) {\n",
                    "    input.required\n",
                    "}\n",
                    "value: needs(input: [other: 1])\n",
                ),
            ),
            (
                ProgramRole::Server,
                concat!(
                    "FUNCTION fill_both(first: OUT, second: OUT) {\n",
                    "    first\n",
                    "}\n",
                    "FUNCTION caller(existing) {\n",
                    "    fill_both(first, second: existing)\n",
                    "}\n",
                ),
            ),
            (
                ProgramRole::Server,
                concat!(
                    "FUNCTION sink(item: OUT) {\n",
                    "    item\n",
                    "}\n",
                    "FUNCTION wrapper(row: OUT) {\n",
                    "    sink(item: DRAIN { row })\n",
                    "}\n",
                ),
            ),
            (
                ProgramRole::Server,
                concat!(
                    "FUNCTION left(x: OUT) {\n",
                    "    right(x: x)\n",
                    "}\n",
                    "FUNCTION right(x: OUT) {\n",
                    "    left(x: x)\n",
                    "}\n",
                ),
            ),
            (ProgramRole::Server, "value: TEXT { no } |> Bool/not()\n"),
            (
                ProgramRole::Server,
                "value: 1 |> Number/round(to: 0, using: NearestEven, extra: 1)\n",
            ),
            (
                ProgramRole::Server,
                "value: Random/bytes(byte_count: \"invalid\")\n",
            ),
            (
                ProgramRole::Server,
                concat!(
                    "tick: SOURCE\n",
                    "value:\n",
                    "    tick\n",
                    "    |> WHILE {\n",
                    "        True => 1\n",
                    "        False => 2\n",
                    "    }\n",
                ),
            ),
            (
                ProgramRole::Server,
                concat!(
                    "value:\n",
                    "    1\n",
                    "    |> THEN {\n",
                    "        2\n",
                    "    }\n",
                ),
            ),
            (
                ProgramRole::Server,
                concat!(
                    "FUNCTION bad_out_then(signal: OUT) {\n",
                    "    rendered: Number/to_text(value: signal)\n",
                    "    signal |> THEN { 1 }\n",
                    "}\n",
                ),
            ),
            (
                ProgramRole::Server,
                concat!(
                    "FUNCTION drain_value_then(value) {\n",
                    "    DRAIN { value } |> THEN { 1 }\n",
                    "}\n",
                    "result: drain_value_then(value: 0)\n",
                ),
            ),
            (
                ProgramRole::Server,
                concat!(
                    "stored: 0 |> HOLD state {}\n",
                    "bad_drain_state: DRAIN { stored } |> THEN { 1 }\n",
                ),
            ),
            (
                ProgramRole::Server,
                concat!(
                    "FUNCTION probe_value_mode(value) {\n",
                    "    value |> WHILE { __ => 1 }\n",
                    "}\n",
                    "tick: SOURCE\n",
                    "result: probe_value_mode(value: tick)\n",
                ),
            ),
            (
                ProgramRole::Server,
                concat!(
                    "FUNCTION probe_child_value_mode(value) {\n",
                    "    BLOCK {\n",
                    "        checked: value |> WHILE { __ => 1 }\n",
                    "        checked\n",
                    "    }\n",
                    "}\n",
                    "tick: SOURCE\n",
                    "result: probe_child_value_mode(value: tick)\n",
                ),
            ),
            (
                ProgramRole::Server,
                concat!(
                    "FUNCTION probe_out_mode(list, item: OUT) {\n",
                    "    list\n",
                    "    |> List/map(\n",
                    "        item: item\n",
                    "        new: item.flag |> WHILE { __ => 1 }\n",
                    "    )\n",
                    "}\n",
                    "tick: SOURCE\n",
                    "rows: LIST { [flag: tick] }\n",
                    "result: probe_out_mode(list: rows, item)\n",
                ),
            ),
            (
                ProgramRole::Server,
                concat!(
                    "FUNCTION probe_nested_out_mode(list, item: OUT) {\n",
                    "    list\n",
                    "    |> List/map(\n",
                    "        item: item\n",
                    "        new: item.flag |> WHILE { __ => 1 }\n",
                    "    )\n",
                    "}\n",
                    "FUNCTION forward_nested_out_mode(list, row: OUT) {\n",
                    "    probe_nested_out_mode(list: list, item: row)\n",
                    "}\n",
                    "tick: SOURCE\n",
                    "rows: LIST { [flag: tick] }\n",
                    "result: forward_nested_out_mode(list: rows, row)\n",
                ),
            ),
            (
                ProgramRole::Server,
                concat!(
                    "FUNCTION probe_out_then_mode(list, item: OUT) {\n",
                    "    list\n",
                    "    |> List/map(\n",
                    "        item: item\n",
                    "        new: item.flag |> THEN { 1 }\n",
                    "    )\n",
                    "}\n",
                    "tick: SOURCE\n",
                    "rows: LIST { [flag: tick] }\n",
                    "result: probe_out_then_mode(list: rows, item)\n",
                ),
            ),
            (
                ProgramRole::Server,
                concat!(
                    "tick: SOURCE\n",
                    "projected: tick |> Field/current()\n",
                    "accepted: projected |> THEN { 1 }\n",
                    "rejected: projected |> WHILE { __ => 1 }\n",
                ),
            ),
            (
                ProgramRole::Server,
                concat!(
                    "tick: SOURCE\n",
                    "projected: tick |> Field/current(extra: 0)\n",
                    "rejected: projected |> WHILE { __ => 1 }\n",
                ),
            ),
            (
                ProgramRole::Server,
                concat!(
                    "FUNCTION stateful_record() {\n",
                    "    [\n",
                    "        stored: 0 |> HOLD state {}\n",
                    "        tick: stored |> THEN { 1 }\n",
                    "    ]\n",
                    "}\n",
                ),
            ),
            (
                ProgramRole::Server,
                concat!(
                    "tick: SOURCE\n",
                    "record: [flag: tick]\n",
                    "mixed:\n",
                    "    LATEST {\n",
                    "        record.flag\n",
                    "        []\n",
                    "    }\n",
                    "bad: mixed |> THEN { 1 }\n",
                ),
            ),
            (
                ProgramRole::Server,
                concat!(
                    "FUNCTION spread_projection_mode(value) {\n",
                    "    value.flag |> WHILE { __ => 1 }\n",
                    "}\n",
                    "tick: SOURCE\n",
                    "accepted_spread: spread_projection_mode(value: [...[flag: tick]])\n",
                ),
            ),
            (
                ProgramRole::Server,
                concat!(
                    "FUNCTION spread_projection_mode(value) {\n",
                    "    value.flag |> WHILE { __ => 1 }\n",
                    "}\n",
                    "tick: SOURCE\n",
                    "bad_spread_override: spread_projection_mode(\n",
                    "    value: [flag: tick, ...[flag: 0]]\n",
                    ")\n",
                ),
            ),
            (
                ProgramRole::Server,
                concat!(
                    "FUNCTION spread_projection_mode(value) {\n",
                    "    value.flag |> WHILE { __ => 1 }\n",
                    "}\n",
                    "tick: SOURCE\n",
                    "steady: [flag: 0]\n",
                    "accepted_named_steady: spread_projection_mode(\n",
                    "    value: [flag: tick, ...steady]\n",
                    ")\n",
                ),
            ),
            (
                ProgramRole::Server,
                concat!(
                    "FUNCTION spread_projection_mode(value) {\n",
                    "    value.flag |> WHILE { __ => 1 }\n",
                    "}\n",
                    "tick: SOURCE\n",
                    "pulse: [flag: tick]\n",
                    "rejected_named_pulse: spread_projection_mode(\n",
                    "    value: [flag: 0, ...pulse]\n",
                    ")\n",
                ),
            ),
            (
                ProgramRole::Server,
                concat!(
                    "FUNCTION spread_projection_mode(value) {\n",
                    "    value.flag |> WHILE { __ => 1 }\n",
                    "}\n",
                    "tick: SOURCE\n",
                    "bad_multi_spread_override: spread_projection_mode(\n",
                    "    value: [...[flag: tick], ...[flag: 0]]\n",
                    ")\n",
                ),
            ),
            (
                ProgramRole::Server,
                concat!(
                    "FUNCTION spread_projection_mode(value) {\n",
                    "    value.flag |> WHILE { __ => 1 }\n",
                    "}\n",
                    "tick: SOURCE\n",
                    "accepted_spread_override: spread_projection_mode(\n",
                    "    value: [...[flag: 0], ...[flag: tick]]\n",
                    ")\n",
                ),
            ),
            (
                ProgramRole::Server,
                concat!(
                    "tick: SOURCE\n",
                    "mixed:\n",
                    "    LATEST {\n",
                    "        tick |> THEN { 1 }\n",
                    "        0\n",
                    "    }\n",
                    "bad: mixed |> THEN { 1 }\n",
                ),
            ),
            (
                ProgramRole::Server,
                concat!(
                    "tick: SOURCE\n",
                    "mixed:\n",
                    "    LATEST {\n",
                    "        tick |> THEN { 1 }\n",
                    "        0\n",
                    "    }\n",
                    "value:\n",
                    "    mixed\n",
                    "    |> WHILE {\n",
                    "        True => 1\n",
                    "        False => 2\n",
                    "    }\n",
                ),
            ),
            (
                ProgramRole::Server,
                concat!(
                    "users: MAP {\n",
                    "    1 => TEXT { first }\n",
                    "    1.0 => TEXT { second }\n",
                    "}\n",
                ),
            ),
            (
                ProgramRole::Server,
                concat!(
                    "store: [\n",
                    "    press: SOURCE\n",
                    "    selected:\n",
                    "        LATEST {\n",
                    "            press |> THEN { TEXT { selected } }\n",
                    "        }\n",
                    "]\n",
                ),
            ),
            (ProgramRole::Server, "value: BITS[3] { 2u1000 }\n"),
            (
                ProgramRole::Server,
                concat!(
                    "value:\n",
                    "    TEXT { hello }\n",
                    "    |> WHEN {\n",
                    "        1 => Yes\n",
                    "        __ => No\n",
                    "    }\n",
                ),
            ),
            (
                ProgramRole::Server,
                concat!(
                    "child: MAP {\n",
                    "    TEXT { sku } => 1\n",
                    "}\n",
                    "parents: LIST {\n",
                    "    [name: A, nested: child]\n",
                    "    [name: B, nested: child]\n",
                    "}\n",
                ),
            ),
            (
                ProgramRole::Server,
                concat!(
                    "nodes: MAP {}\n",
                    "cyclic:\n",
                    "    nodes\n",
                    "    |> Map/upsert(\n",
                    "        entry: [\n",
                    "            key: TEXT { node }\n",
                    "            value: nodes\n",
                    "        ]\n",
                    "    )\n",
                ),
            ),
            (
                ProgramRole::Server,
                concat!(
                    "rows: LIST {\n",
                    "    [\n",
                    "        id: TEXT { row }\n",
                    "        child: MAP { TEXT { sku } => 1 }\n",
                    "    ]\n",
                    "}\n",
                    "escaped:\n",
                    "    rows\n",
                    "    |> List/map(item, new:\n",
                    "        MAP {\n",
                    "            TEXT { copy } => item.child\n",
                    "        }\n",
                    "    )\n",
                ),
            ),
            (
                ProgramRole::Server,
                concat!(
                    "rows: LIST {\n",
                    "    [\n",
                    "        id: TEXT { row }\n",
                    "        child: MAP { TEXT { sku } => 1 }\n",
                    "    ]\n",
                    "}\n",
                ),
            ),
        ];

        let over_budget_number = format!(
            "value: {}\n",
            "9".repeat(boon_data::MAX_NUMBER_PARSED_DIGITS + 1)
        );
        for (role, source) in fixtures.into_iter().chain(std::iter::once((
            ProgramRole::Server,
            over_budget_number.as_str(),
        ))) {
            let units = vec![CompilerSourceUnit {
                path: "RUN.bn".to_owned(),
                source: source.to_owned(),
            }];
            let mut session = CompilerSession::new();
            let project = session
                .open_project(CompilerProject::new(
                    "RUN.bn",
                    units.clone(),
                    TargetProfile::SoftwareDefault,
                    role,
                    ApplicationIdentity::compiler_default(),
                ))
                .unwrap();
            let state = session.projects.get_mut(&project).unwrap();
            let (_, aggregate, _, _, _) =
                parse_project_diagnostics_snapshot(state).unwrap_or_else(|error| {
                    panic!("lean diagnostics failed for:\n{source}\n{error:?}")
                });
            let lean = aggregate.diagnostics().to_vec();
            let deferred_style_message =
                "style field `width` must be a number, `Fill` tag, or `Auto` tag";
            if source.contains("TEXT { invalid }")
                && (source.contains("sized_box")
                    || source.contains("styled_alias")
                    || source.contains("styled_call_alias")
                    || source.contains("FUNCTION render_row(label_text, selected, row)"))
                && !source.contains("styled_alias(first: TEXT { invalid }, second: 24)")
                && !source.contains("styled_call_alias(first: TEXT { invalid }, second: 24)")
            {
                assert!(
                    lean.iter()
                        .any(|diagnostic| diagnostic.message == deferred_style_message),
                    "missing deferred style diagnostic for:\n{source}\n{lean:#?}"
                );
            }
            if source.contains("sized_box(size: 24)") {
                assert!(
                    lean.iter()
                        .all(|diagnostic| diagnostic.message != deferred_style_message),
                    "valid deferred style specialization was rejected:\n{source}\n{lean:#?}"
                );
            }
            if source.contains("row: [indent_width: 92]") {
                assert!(
                    lean.iter()
                        .all(|diagnostic| diagnostic.message != deferred_style_message),
                    "stable projected actual was confused with a sibling parameter namespace:\n{source}\n{lean:#?}"
                );
            }
            if source.contains("styled_alias(first: TEXT { invalid }, second: 24)") {
                assert!(
                    lean.iter()
                        .all(|diagnostic| !diagnostic.message.contains("style field `width`")),
                    "child alias used the wrong callable type-variable namespace:\n{lean:#?}"
                );
            }
            if source.contains("styled_alias(first: 24, second: TEXT { invalid })") {
                assert!(
                    lean.iter()
                        .any(|diagnostic| diagnostic.message.contains("style field `width`")),
                    "child alias did not retain the second parameter namespace:\n{lean:#?}"
                );
            }
            if source.contains("styled_call_alias(first: TEXT { invalid }, second: 24)") {
                assert!(
                    lean.iter()
                        .all(|diagnostic| !diagnostic.message.contains("style field `width`")),
                    "call alias used the wrong callable type-variable namespace:\n{lean:#?}"
                );
            }
            if source.contains("styled_call_alias(first: 24, second: TEXT { invalid })") {
                assert!(
                    lean.iter()
                        .any(|diagnostic| diagnostic.message.contains("style field `width`")),
                    "call alias did not retain the second parameter namespace:\n{lean:#?}"
                );
            }
            if source.contains("parents: LIST") {
                assert!(
                    lean.iter().any(|diagnostic| {
                        diagnostic.message.contains("second parent")
                            || diagnostic
                                .message
                                .contains("more than one structural parent")
                    }),
                    "missing second-parent authority diagnostic for:\n{source}\n{lean:#?}"
                );
            }
            if source.contains("cyclic:") {
                assert!(
                    lean.iter().any(|diagnostic| diagnostic
                        .message
                        .contains("attachment forms an ownership cycle")),
                    "missing cyclic authority diagnostic for:\n{source}\n{lean:#?}"
                );
            }
            if source.contains("escaped:") {
                assert!(
                    lean.iter().any(|diagnostic| {
                        diagnostic.message.contains("escapes its owner")
                            || diagnostic.message.contains("beyond its owner lifetime")
                    }),
                    "missing escaped authority diagnostic for:\n{source}\n{lean:#?}"
                );
            }
            if source.contains("FUNCTION bad_out_then") {
                assert!(
                    lean.iter()
                        .any(|diagnostic| diagnostic.message.starts_with("`THEN` requires")),
                    "OUT parameter incorrectly received the VALUE-parameter temporal exemption:\n{lean:#?}"
                );
            }
            if source.contains("FUNCTION drain_value_then") || source.contains("bad_drain_state") {
                assert!(
                    lean.iter()
                        .any(|diagnostic| diagnostic.message.starts_with("`THEN` requires")),
                    "DRAIN incorrectly inherited a read-only temporal exemption:\n{lean:#?}"
                );
            }
            if source.contains("FUNCTION probe_value_mode")
                || source.contains("FUNCTION probe_child_value_mode")
                || source.contains("FUNCTION probe_out_mode")
                || source.contains("FUNCTION probe_nested_out_mode")
            {
                assert!(
                    lean.iter()
                        .any(|diagnostic| diagnostic.message
                            == "`WHILE` requires a continuous selector"),
                    "exact parameter/output actual modes were not projected into WHILE for:\n{source}\n{lean:#?}"
                );
            }
            if source.contains("FUNCTION probe_out_then_mode") {
                assert!(
                    lean.iter()
                        .all(|diagnostic| !diagnostic.message.starts_with("`THEN` requires")),
                    "a forwarded OUT pulse was not accepted by THEN:\n{lean:#?}"
                );
            }
            if source.contains("Field/current") {
                assert!(
                    lean.iter()
                        .any(|diagnostic| diagnostic.message
                            == "`WHILE` requires a continuous selector"),
                    "Field projection lost its pulse mode before WHILE:\n{lean:#?}"
                );
                assert!(
                    lean.iter()
                        .all(|diagnostic| !diagnostic.message.starts_with("`THEN` requires")),
                    "Field projection lost its pulse mode before THEN:\n{lean:#?}"
                );
            }
            if source.contains("record: [flag: tick]") {
                assert!(
                    lean.iter()
                        .any(|diagnostic| diagnostic.message.starts_with("`THEN` requires")),
                    "LATEST statefulness used a raw rather than exact first-branch mode:\n{lean:#?}"
                );
            }
            if source.contains("accepted_spread:") || source.contains("accepted_spread_override:") {
                assert!(
                    lean.iter()
                        .any(|diagnostic| diagnostic.message
                            == "`WHILE` requires a continuous selector"),
                    "record spread projection lost its exact pulse mode:\n{lean:#?}"
                );
            }
            if source.contains("bad_spread_override:")
                || source.contains("bad_multi_spread_override:")
                || source.contains("accepted_named_steady:")
            {
                assert!(
                    lean.iter()
                        .all(|diagnostic| diagnostic.message
                            != "`WHILE` requires a continuous selector"),
                    "later record spread did not override an earlier pulse field for:\n{source}\n{lean:#?}"
                );
            }
            if source.contains("rejected_named_pulse:") {
                assert!(
                    lean.iter()
                        .any(|diagnostic| diagnostic.message
                            == "`WHILE` requires a continuous selector"),
                    "later named spread lost its pulse provider:\n{source}\n{lean:#?}"
                );
            }
            if source.contains("FUNCTION stateful_record") {
                assert!(
                    lean.iter()
                        .all(|diagnostic| !diagnostic.message.starts_with("`THEN` requires")),
                    "record-field HOLD transition was not recognized as stateful:\n{lean:#?}"
                );
            }
            if source.contains("bad: mixed |> THEN") {
                assert!(
                    lean.iter()
                        .any(|diagnostic| diagnostic.message.starts_with("`THEN` requires")),
                    "event-first LATEST incorrectly received the state-transition exemption:\n{lean:#?}"
                );
            }
            if source.contains("Number/round") {
                assert!(
                    lean.iter().any(|diagnostic| diagnostic.message
                        == "`Number/round` argument `to` must be a strictly positive exact Number"),
                    "invalid builtin shape suppressed builtin-domain diagnostics:\n{lean:#?}"
                );
            }
            if source.contains("Random/bytes") {
                assert!(
                    lean.iter().any(|diagnostic| diagnostic.message.starts_with(
                        "`Random/bytes` argument `byte_count` has incompatible type\nexpected: NUMBER\nfound:"
                    )),
                    "typed host-effect argument diagnostics were not projected from owner facts:\n{lean:#?}"
                );
            }
            if source == over_budget_number {
                assert!(
                    lean.iter().any(|diagnostic| diagnostic
                        .message
                        .contains("invalid exact Number literal")
                        && diagnostic.message.contains("digit budget")),
                    "invalid exact Number source diagnostics were not projected:\n{lean:#?}"
                );
            }
            if source.contains("value:\n    tick\n    |> WHILE") {
                assert!(
                    lean.iter()
                        .any(|diagnostic| diagnostic.message
                            == "`WHILE` requires a continuous selector"),
                    "WHILE mode validation was skipped without a callable row:\n{lean:#?}"
                );
            }
            if source.contains("mixed:\n    LATEST") && source.contains("|> WHILE") {
                assert!(
                    lean.iter()
                        .all(|diagnostic| diagnostic.message
                            != "`WHILE` requires a continuous selector"),
                    "continuous mixed LATEST received a false WHILE diagnostic:\n{lean:#?}"
                );
            }
            if source.contains("response: missing") {
                assert!(
                    lean.iter().any(|diagnostic| diagnostic.message
                        == "host port `http.response` references missing output root `missing`"),
                    "missing host output root lost its source diagnostic:\n{lean:#?}"
                );
            }
            if source == "value: Node[...1]\n" {
                assert!(
                    lean.iter()
                        .any(|diagnostic| diagnostic.message
                            == "record spread expects a record value"),
                    "tagged-object spread value was skipped by diagnostic replay:\n{lean:#?}"
                );
            }
            if source == "value: Node[...[same: 1, same: 2]]\n" {
                assert!(
                    lean.iter()
                        .any(|diagnostic| diagnostic.message
                            == "duplicate explicit record field `same`"),
                    "tagged-object spread hid nested record diagnostics:\n{lean:#?}"
                );
            }
            if source.contains("PASS: [...1]") {
                assert!(
                    lean.iter()
                        .any(|diagnostic| diagnostic.message
                            == "record spread expects a record value"),
                    "explicit PASS child diagnostics were skipped:\n{lean:#?}"
                );
            }
            if source == "document: [\n    root:\n]\n" {
                assert!(
                    lean.iter()
                        .all(|diagnostic| !diagnostic.message.contains("render slot `root`")),
                    "valueless render slot received a false type diagnostic:\n{lean:#?}"
                );
            }
            if source == "outputs: [\n    pending:\n]\n" {
                assert!(
                    lean.iter().all(|diagnostic| diagnostic.message
                        != "`outputs` must declare at least one named output root"),
                    "valueless output root was dropped from stable facts:\n{lean:#?}"
                );
            }
            assert_eq!(state.project_diagnostic_facts_requests.request_count(), 1);
            assert_eq!(state.owner_construction_abi_requests.request_count(), 0);
            assert_eq!(state.checked_owner_shard_requests.request_count(), 0);
            assert_eq!(
                state
                    .checked_owner_project_assembly_requests
                    .request_count(),
                0
            );

            let (_, assembly, _, _, _) = parse_project_snapshot(state).unwrap_or_else(|error| {
                panic!("checked assembly failed for:\n{source}\n{error:?}")
            });
            assert_eq!(
                lean,
                assembly.diagnostics(),
                "construction-independent diagnostic facts differ for:\n{source}"
            );
            if source.contains("response: missing") {
                assert_eq!(
                    assembly.fields().lowering_metadata.host_port_table,
                    boon_checked::HostPortTable::default(),
                    "diagnosed host-port relocation must fail safely to the retained default"
                );
            }
            let legacy = crate::check_diagnostics_source(
                crate::CompilerCheckRequest::source_units("RUN.bn", &units, role),
            )
            .unwrap();
            let mut owner_oracle = normalized_invalid_oracle(&lean);
            if source.contains("FUNCTION probe_out_mode")
                || source.contains("FUNCTION probe_nested_out_mode")
                || source.contains("Field/current")
            {
                // The retained walker validates a function body before later
                // call sites have populated its reverse OUT graph. The
                // project-wide owner authority intentionally closes that
                // order-dependent hole and diagnoses the event-valued row.
                owner_oracle.remove(&(0, "`WHILE` requires a continuous selector".to_owned()));
            }
            let mut legacy_oracle =
                normalized_invalid_oracle(&normalized_complete_diagnostics(&legacy));
            if source.contains("Field/current(extra: 0)") {
                // Exact owner call planning retains the resolved ABI target
                // while reporting the bad entry. The retiring checked lowerer
                // instead fails its canonical-schema projection.
                owner_oracle.remove(&(
                    0,
                    "unexpected call entry `Field/current`.`extra`".to_owned(),
                ));
                legacy_oracle.remove(&(
                    0,
                    "`Field/current` has no authoritative canonical argument schema for CheckedProgram lowering"
                        .to_owned(),
                ));
            }
            if source.contains("FUNCTION recurse_child") {
                // The retained checker loses this BLOCK-local declaration at
                // the child-owner boundary. Exact inherited lexical capture
                // keeps it local while preserving the recursion diagnostic.
                legacy_oracle.remove(&(0, "unknown identifier `next`".to_owned()));
            }
            assert_eq!(
                owner_oracle, legacy_oracle,
                "owner diagnostics differ from the normalized independent assembled checker for:\n{source}"
            );
            if let Some(program) = &legacy.output.program {
                assert_eq!(
                    assembly.fields().order_chains,
                    program.order_chains,
                    "stable project order facts relocate differently from the independent checked oracle for:\n{source}"
                );
            }
        }

        let units = vec![
            CompilerSourceUnit {
                path: "RUN.bn".to_owned(),
                source: concat!("value: helper\n", "missing: typo\n",).to_owned(),
            },
            CompilerSourceUnit {
                path: "support/invalid.bn".to_owned(),
                source: concat!(
                    "FUNCTION helper() {\n",
                    "    1\n",
                    "}\n",
                    "duplicate: [item: 1, item: 2, copy: item]\n",
                    "passed: PASSED.store\n",
                )
                .to_owned(),
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
        let token = CancellationToken::new();
        let public_lean = session
            .request(project, revision, CompileIntent::Diagnostics, &token)
            .unwrap()
            .diagnostics()
            .unwrap()
            .diagnostics()
            .to_vec();
        assert!(
            public_lean.iter().any(|diagnostic| {
                diagnostic.message
                    == "function `helper` must be called with parentheses: `helper()`"
            }),
            "multi-unit diagnostics: {public_lean:#?}"
        );
        assert!(
            public_lean
                .iter()
                .any(|diagnostic| diagnostic.message == "unknown identifier `typo`"),
            "multi-unit diagnostics: {public_lean:#?}"
        );
        {
            let state = session.projects.get(&project).unwrap();
            assert_eq!(state.project_diagnostic_facts_requests.request_count(), 1);
            assert_eq!(
                state.owner_body_inference_requests.request_count(),
                state.owner_input_requests.request_count()
            );
            assert_eq!(state.owner_construction_abi_requests.request_count(), 0);
            assert_eq!(state.checked_owner_shard_requests.request_count(), 0);
            assert_eq!(
                state
                    .checked_owner_project_assembly_requests
                    .request_count(),
                0
            );
        }
        let state = session.projects.get_mut(&project).unwrap();
        let (_, aggregate, _, _, _) = parse_project_diagnostics_snapshot(state).unwrap();
        assert_eq!(public_lean, aggregate.diagnostics());
        let (_, assembly, _, _, _) = parse_project_snapshot(state).unwrap();
        assert_eq!(public_lean, assembly.diagnostics());
        let legacy = crate::check_diagnostics_source(crate::CompilerCheckRequest::source_units(
            "RUN.bn",
            &units,
            ProgramRole::Server,
        ))
        .unwrap();
        assert_eq!(
            normalized_invalid_oracle(&public_lean),
            normalized_invalid_oracle(&normalized_complete_diagnostics(&legacy)),
            "multi-unit owner diagnostics differ from the normalized independent assembled checker"
        );
    }

    #[test]
    fn editor_report_presents_each_shared_render_failure_once() {
        let mut session = CompilerSession::new();
        let project = session
            .open_project(CompilerProject::new(
                "RUN.bn",
                vec![CompilerSourceUnit {
                    path: "RUN.bn".to_owned(),
                    source: "document: [\n    root: 1\n]\n".to_owned(),
                }],
                TargetProfile::SoftwareDefault,
                ProgramRole::Client,
                ApplicationIdentity::compiler_default(),
            ))
            .unwrap();
        let revision = session.revision(project).unwrap();
        let token = CancellationToken::new();
        let public = session
            .request(project, revision, CompileIntent::Diagnostics, &token)
            .unwrap()
            .diagnostics()
            .unwrap()
            .diagnostics()
            .to_vec();
        let editor_result = session
            .request(project, revision, CompileIntent::EditorDiagnostics, &token)
            .unwrap();
        let editor = editor_result.editor_diagnostics().unwrap();
        let slot_diagnostics = editor
            .output
            .report
            .render_slot_table
            .slots
            .iter()
            .flat_map(|slot| slot.diagnostics.iter().cloned())
            .collect::<Vec<_>>();
        assert_eq!(slot_diagnostics.len(), 1);
        assert!(editor.output.report.diagnostics.is_empty());
        assert_eq!(public, slot_diagnostics);
        assert_eq!(editor.profile.diagnostic_count, 1);
    }

    #[test]
    fn public_diagnostics_stays_lean_and_editor_rows_are_explicitly_demanded() {
        let mut session = CompilerSession::new();
        let project = session.open_project(project("value: 1")).unwrap();
        let revision = session.revision(project).unwrap();
        let token = CancellationToken::new();
        {
            let result = session
                .request(project, revision, CompileIntent::Diagnostics, &token)
                .unwrap();
            let diagnostics = result.diagnostics().unwrap();
            assert!(!diagnostics.has_errors());
            assert!(diagnostics.full_document_typecheck_coverage());
        }
        {
            let state = session.projects.get(&project).unwrap();
            assert_eq!(state.owner_construction_abi_requests.request_count(), 0);
            assert_eq!(state.checked_owner_shard_requests.request_count(), 0);
            assert_eq!(
                state
                    .checked_owner_project_assembly_requests
                    .request_count(),
                0
            );
        }
        {
            let result = session
                .request(project, revision, CompileIntent::EditorDiagnostics, &token)
                .unwrap();
            let output = &result.editor_diagnostics().unwrap().output;
            assert!(output.program.is_none());
            assert!(output.construction.is_some());
        }
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
            assembled.plan.plan().clone(),
            assembled.plan.plan_hash().to_owned(),
        );
        // The owner pipeline deliberately omits hundreds of unreachable ABI
        // declarations retained by the historical broad checker. Those dead
        // rows change its internal checked/manifest certificate while the
        // executable artifact remains byte-for-byte canonical.
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
        let request_counts = |stats: RequestEvaluationStats| {
            (
                stats.demanded,
                stats.executed,
                stats.reused,
                stats.backdated,
                stats.changed,
            )
        };
        let first_request_counts = request_counts(first_stats);
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
        // Counts include the callable-only ABI/resolution/scope SCC split in
        // addition to declaration-surface/lexical-plan, exact interface
        // provider and owner-inference requests. Public diagnostics deliberately
        // stop before construction ABI and checked-owner requests.
        // All callable-scope and unchanged interface-transfer module requests
        // are reused by this literal-only warm edit; the edited owner's
        // ordinary dependency cone remains local.
        // Every owner also publishes one current diagnostic-replay evaluation
        // and one independently backdatable normalized semantic fact.
        // One shared output-flow component is demanded after those facts; it
        // executes cold and reuses its unchanged semantic result on this edit.
        // The project diagnostic-facts request executes once per revision and
        // changes when its exact owner-body input changes. Each source unit
        // owns one local owner-diagnostic projection plus a current project-
        // diagnostic evaluation and independently backdatable project row
        // projection; unchanged local rows reuse their retained value.
        assert_eq!(
            (first_request_counts, request_counts(second_stats)),
            ((114, 112, 2, 0, 112), (228, 135, 93, 11, 124))
        );

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
        let first_left_body = session
            .owner_body_inference(project, &left)
            .unwrap()
            .unwrap();
        let first_right_body = session
            .owner_body_inference(project, &right)
            .unwrap()
            .unwrap();
        let first_output_flow = {
            let state = session.projects.get(&project).unwrap();
            Arc::clone(
                state
                    .project_output_flow_facts_requests
                    .current_value(&state.syntax_evaluator, &ProjectOutputFlowFactsKey)
                    .unwrap()
                    .unwrap(),
            )
        };
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
        let second_left_body = session
            .owner_body_inference(project, &left)
            .unwrap()
            .unwrap();
        let second_right_body = session
            .owner_body_inference(project, &right)
            .unwrap()
            .unwrap();
        let second_output_flow = {
            let state = session.projects.get(&project).unwrap();
            Arc::clone(
                state
                    .project_output_flow_facts_requests
                    .current_value(&state.syntax_evaluator, &ProjectOutputFlowFactsKey)
                    .unwrap()
                    .unwrap(),
            )
        };

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
        assert!(!Arc::ptr_eq(&first_left_body, &second_left_body));
        assert!(Arc::ptr_eq(&first_right_body, &second_right_body));
        assert_eq!(
            first_left_body.diagnostic_facts.fingerprint_v1(),
            second_left_body.diagnostic_facts.fingerprint_v1(),
            "owner-local diagnostic facts remain semantically stable inside the changed body",
        );
        assert!(Arc::ptr_eq(&first_output_flow, &second_output_flow));

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
        assert_eq!(
            state
                .owner_body_inference_requests
                .memo(&state.syntax_evaluator, &left)
                .unwrap()
                .changed_at,
            EvaluationRevision(1)
        );
    }

    #[test]
    fn owner_value_resolution_keeps_the_longest_prefix_projection() {
        let mut session = CompilerSession::new();
        let project = session
            .open_project(project("record: [value: 1]\nselected: record.value\n"))
            .unwrap();
        parse_project_snapshot(session.projects.get_mut(&project).unwrap()).unwrap();
        let unit = session
            .unit_syntax_snapshot(project, "RUN.bn")
            .unwrap()
            .unwrap();
        let selected = unit
            .stable_check_owner_keys()
            .find(|owner| {
                matches!(
                    owner,
                    StableCheckOwnerKey::Item(owner)
                        if owner.item_route.segments().last().is_some_and(|segment| segment.names == ["selected"])
                )
            })
            .unwrap();
        let summary = session
            .owner_constraint_summary(project, &selected)
            .unwrap()
            .unwrap();
        let resolved = summary.resolved_references.first().unwrap();
        assert_eq!(resolved.projection.as_ref(), ["value"]);

        let body = session
            .owner_body_inference(project, &selected)
            .unwrap()
            .unwrap();
        assert_eq!(
            body.expressions.last().unwrap().flow_type.ty,
            boon_checked::Type::Number
        );
    }

    #[test]
    fn project_value_resolution_ignores_nested_implementation_owners_at_the_same_path() {
        let source = concat!(
            "store: [\n",
            "    target:\n",
            "        LIST { 1 }\n",
            "        |> List/map(item, new: item)\n",
            "]\n",
            "selected: store.target\n",
        );
        let mut session = CompilerSession::new();
        let project = session.open_project(project(source)).unwrap();
        parse_project_snapshot(session.projects.get_mut(&project).unwrap()).unwrap();
        let unit = session
            .unit_syntax_snapshot(project, "RUN.bn")
            .unwrap()
            .unwrap();
        let selected = unit
            .stable_check_owner_keys()
            .find(|owner| {
                matches!(
                    owner,
                    StableCheckOwnerKey::Item(owner)
                        if owner.item_route.segments().last().is_some_and(|segment| {
                            segment.names.as_ref() == ["selected"]
                        })
                )
            })
            .unwrap();
        let summary = session
            .owner_constraint_summary(project, &selected)
            .unwrap()
            .unwrap();
        assert_eq!(summary.resolved_references.len(), 1, "{summary:#?}");
        assert_eq!(
            owner_route_value_path(&summary.resolved_references[0].owner, true),
            ["store", "target"]
        );
        assert!(summary.resolved_references[0].projection.is_empty());
    }

    #[test]
    fn project_value_resolution_uses_only_unique_suffixes_and_prefers_exact_paths() {
        let source = concat!(
            "nested: TEXT { exact }\n",
            "store: [\n",
            "    nested: 1\n",
            "    unique: 2\n",
            "]\n",
            "left: [\n",
            "    duplicate: 3\n",
            "]\n",
            "right: [\n",
            "    duplicate: 4\n",
            "]\n",
            "exact_value: nested\n",
            "suffix_value: unique\n",
            "ambiguous_value: duplicate\n",
        );
        let mut session = CompilerSession::new();
        let project = session.open_project(project(source)).unwrap();
        parse_project_snapshot(session.projects.get_mut(&project).unwrap()).unwrap();
        let unit = session
            .unit_syntax_snapshot(project, "RUN.bn")
            .unwrap()
            .unwrap();
        let owners = unit.stable_check_owner_keys().collect::<Vec<_>>();
        let named_owner = |name: &str| {
            owners
                .iter()
                .find(|owner| {
                    matches!(
                        owner,
                        StableCheckOwnerKey::Item(owner)
                            if owner.item_route.segments().last().is_some_and(|segment| {
                                segment.names.first().is_some_and(|candidate| candidate == name)
                            })
                    )
                })
                .cloned()
                .unwrap()
        };
        let exact_declaration = owners
            .iter()
            .find(|owner| {
                matches!(
                    owner,
                    StableCheckOwnerKey::Item(owner)
                        if owner.item_route.segments().len() == 1
                            && owner.item_route.segments()[0].names == ["nested"]
                )
            })
            .cloned()
            .unwrap();
        let unique_declaration = named_owner("unique");

        let exact_summary = session
            .owner_constraint_summary(project, &named_owner("exact_value"))
            .unwrap()
            .unwrap();
        assert_eq!(
            exact_summary.resolved_references[0].owner,
            exact_declaration
        );

        let suffix_summary = session
            .owner_constraint_summary(project, &named_owner("suffix_value"))
            .unwrap()
            .unwrap();
        assert_eq!(
            suffix_summary.resolved_references[0].owner,
            unique_declaration
        );

        let ambiguous_summary = session
            .owner_constraint_summary(project, &named_owner("ambiguous_value"))
            .unwrap()
            .unwrap();
        assert!(
            ambiguous_summary
                .symbol_resolutions
                .iter()
                .any(|resolution| {
                    matches!(
                        resolution,
                        OwnerSymbolResolution::Ambiguous { candidates, .. } if candidates.len() == 2
                    )
                }),
            "ambiguous summary: {ambiguous_summary:#?}"
        );
    }

    #[test]
    fn public_symbol_index_depends_on_declaration_surfaces_not_owner_bodies() {
        let before = "FUNCTION identity(input) {\n    input\n}\nvalue: identity(input: 1)\n";
        let after = "FUNCTION identity(input) {\n    input + 0\n}\nvalue: identity(input: 1)\n";
        let mut session = CompilerSession::new();
        let project = session.open_project(project(before)).unwrap();
        parse_project_snapshot(session.projects.get_mut(&project).unwrap()).unwrap();
        let unit = session
            .unit_syntax_snapshot(project, "RUN.bn")
            .unwrap()
            .unwrap();
        let identity = unit
            .stable_check_owner_keys()
            .find(|owner| {
                matches!(
                    owner,
                    StableCheckOwnerKey::Item(owner)
                        if owner.item_route.segments().last().is_some_and(|segment| segment.names == ["identity"])
                )
            })
            .unwrap();
        let first_surface = session
            .owner_declaration_surface(project, &identity)
            .unwrap()
            .unwrap();
        let first_seed = session
            .owner_constraint_seed(project, &identity)
            .unwrap()
            .unwrap();
        let first_symbols = {
            let state = session.projects.get(&project).unwrap();
            Arc::clone(
                state
                    .project_owner_symbol_requests
                    .current_value(&state.syntax_evaluator, &ProjectOwnerSymbolKey)
                    .unwrap()
                    .unwrap(),
            )
        };

        session
            .apply_update(project, UnitUpdate::new("RUN.bn", after))
            .unwrap();
        parse_project_snapshot(session.projects.get_mut(&project).unwrap()).unwrap();
        let second_surface = session
            .owner_declaration_surface(project, &identity)
            .unwrap()
            .unwrap();
        let second_seed = session
            .owner_constraint_seed(project, &identity)
            .unwrap()
            .unwrap();
        let second_symbols = {
            let state = session.projects.get(&project).unwrap();
            Arc::clone(
                state
                    .project_owner_symbol_requests
                    .current_value(&state.syntax_evaluator, &ProjectOwnerSymbolKey)
                    .unwrap()
                    .unwrap(),
            )
        };

        assert!(Arc::ptr_eq(&first_surface, &second_surface));
        assert!(Arc::ptr_eq(&first_symbols, &second_symbols));
        assert!(!Arc::ptr_eq(&first_seed, &second_seed));
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
        let first_body = session
            .owner_body_inference(project, &owner)
            .unwrap()
            .unwrap();
        let first_interface_evaluation = {
            let state = session.projects.get(&project).unwrap();
            Arc::clone(
                state
                    .owner_interface_scc_evaluation_requests
                    .current_value(&state.syntax_evaluator, &first_interface.key)
                    .unwrap()
                    .unwrap(),
            )
        };
        assert!(Arc::ptr_eq(
            &first_interface_evaluation.result,
            &first_interface,
        ));
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
        let second_body = session
            .owner_body_inference(project, &owner)
            .unwrap()
            .unwrap();
        let second_interface_evaluation = {
            let state = session.projects.get(&project).unwrap();
            Arc::clone(
                state
                    .owner_interface_scc_evaluation_requests
                    .current_value(&state.syntax_evaluator, &second_interface.key)
                    .unwrap()
                    .unwrap(),
            )
        };
        assert!(!Arc::ptr_eq(&first_seed, &second_seed));
        assert_eq!(first_interface.owners, second_interface.owners);
        assert_eq!(
            first_interface.type_variable_count,
            second_interface.type_variable_count,
        );
        assert_eq!(
            first_interface.fingerprint_v1(),
            second_interface.fingerprint_v1(),
            "an alpha-equivalent body edit changed the semantic interface fingerprint",
        );
        assert!(Arc::ptr_eq(&first_interface, &second_interface));
        assert!(!Arc::ptr_eq(
            &first_interface_evaluation,
            &second_interface_evaluation,
        ));
        assert_ne!(
            first_interface_evaluation.currentness.fingerprint_v1(),
            second_interface_evaluation.currentness.fingerprint_v1(),
        );
        assert_eq!(
            second_interface_evaluation
                .currentness
                .result_fingerprint_v1(),
            second_interface.fingerprint_v1(),
        );
        assert!(!Arc::ptr_eq(
            &second_interface_evaluation.result,
            &second_interface,
        ));
        assert!(!Arc::ptr_eq(&first_body, &second_body));
        let state = session.projects.get(&project).unwrap();
        let key = second_interface.key.clone();
        let evaluation_memo = state
            .owner_interface_scc_evaluation_requests
            .memo(&state.syntax_evaluator, &key)
            .unwrap();
        assert_eq!(evaluation_memo.changed_at, EvaluationRevision(1));
        assert_eq!(evaluation_memo.verified_at, EvaluationRevision(1));
        let memo = state
            .owner_interface_scc_requests
            .memo(&state.syntax_evaluator, &key)
            .unwrap();
        assert_eq!(memo.changed_at, EvaluationRevision(0));
        assert_eq!(memo.verified_at, EvaluationRevision(1));
        let body_memo = state
            .owner_body_inference_requests
            .memo(&state.syntax_evaluator, &owner)
            .unwrap();
        assert_eq!(body_memo.changed_at, EvaluationRevision(1));
        assert_eq!(body_memo.verified_at, EvaluationRevision(1));
    }

    #[test]
    fn unrelated_callable_abi_change_stays_outside_the_owner_cone() {
        let before = concat!(
            "FUNCTION keep(input) {\n",
            "    Number/to_text(value: input)\n",
            "}\n",
            "other: Field/known(input: [known: 1])\n",
        );
        let after = concat!(
            "FUNCTION keep(input) {\n",
            "    Number/to_text(value: input)\n",
            "}\n",
            "other: Field/unrelated(input: [unrelated: 2])\n",
        );
        let mut session = CompilerSession::new();
        let project = session.open_project(project(before)).unwrap();
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
        let keep = owner_named("keep");
        let other = owner_named("other");
        let first_summary = session
            .owner_constraint_summary(project, &keep)
            .unwrap()
            .unwrap();
        let first_interface = session
            .owner_interface_result(project, &keep)
            .unwrap()
            .unwrap();
        let first_body = session
            .owner_body_inference(project, &keep)
            .unwrap()
            .unwrap();
        let (
            first_provider_fingerprint,
            first_lookup,
            first_keep_abi,
            first_other_abi,
            first_keep_construction_abi,
            first_keep_shard,
        ) = {
            let state = session.projects.get(&project).unwrap();
            (
                state
                    .project_owner_callable_abi_requests
                    .current_value(&state.syntax_evaluator, &ProjectOwnerAbiKey)
                    .unwrap()
                    .unwrap()
                    .fingerprint_v1(),
                Arc::clone(
                    state
                        .owner_callable_abi_lookup_requests
                        .current_value(&state.syntax_evaluator, &"Number/to_text".to_owned())
                        .unwrap()
                        .unwrap(),
                ),
                Arc::clone(
                    state
                        .owner_inference_abi_requests
                        .current_value(&state.syntax_evaluator, &keep)
                        .unwrap()
                        .unwrap(),
                ),
                Arc::clone(
                    state
                        .owner_inference_abi_requests
                        .current_value(&state.syntax_evaluator, &other)
                        .unwrap()
                        .unwrap(),
                ),
                Arc::clone(
                    state
                        .owner_construction_abi_requests
                        .current_value(&state.syntax_evaluator, &keep)
                        .unwrap()
                        .unwrap(),
                ),
                Arc::clone(
                    state
                        .checked_owner_shard_requests
                        .current_value(&state.syntax_evaluator, &keep)
                        .unwrap()
                        .unwrap(),
                ),
            )
        };

        session
            .apply_update(project, UnitUpdate::new("RUN.bn", after))
            .unwrap();
        parse_project_snapshot(session.projects.get_mut(&project).unwrap()).unwrap();
        let second_summary = session
            .owner_constraint_summary(project, &keep)
            .unwrap()
            .unwrap();
        let second_interface = session
            .owner_interface_result(project, &keep)
            .unwrap()
            .unwrap();
        let second_body = session
            .owner_body_inference(project, &keep)
            .unwrap()
            .unwrap();
        let state = session.projects.get(&project).unwrap();
        let second_provider_fingerprint = state
            .project_owner_callable_abi_requests
            .current_value(&state.syntax_evaluator, &ProjectOwnerAbiKey)
            .unwrap()
            .unwrap()
            .fingerprint_v1();
        let second_lookup = state
            .owner_callable_abi_lookup_requests
            .current_value(&state.syntax_evaluator, &"Number/to_text".to_owned())
            .unwrap()
            .unwrap();
        let second_keep_abi = state
            .owner_inference_abi_requests
            .current_value(&state.syntax_evaluator, &keep)
            .unwrap()
            .unwrap();
        let second_other_abi = state
            .owner_inference_abi_requests
            .current_value(&state.syntax_evaluator, &other)
            .unwrap()
            .unwrap();
        let second_keep_construction_abi = state
            .owner_construction_abi_requests
            .current_value(&state.syntax_evaluator, &keep)
            .unwrap()
            .unwrap();
        let second_keep_shard = state
            .checked_owner_shard_requests
            .current_value(&state.syntax_evaluator, &keep)
            .unwrap()
            .unwrap();

        assert_ne!(first_provider_fingerprint, second_provider_fingerprint);
        assert!(Arc::ptr_eq(&first_lookup, second_lookup));
        assert!(Arc::ptr_eq(&first_keep_abi, second_keep_abi));
        assert!(!Arc::ptr_eq(&first_other_abi, second_other_abi));
        assert!(Arc::ptr_eq(&first_summary, &second_summary));
        assert!(Arc::ptr_eq(&first_interface, &second_interface));
        assert!(Arc::ptr_eq(&first_body, &second_body));
        assert!(Arc::ptr_eq(
            &first_keep_construction_abi,
            second_keep_construction_abi
        ));
        assert!(Arc::ptr_eq(&first_keep_shard, second_keep_shard));

        let lookup_memo = state
            .owner_callable_abi_lookup_requests
            .memo(&state.syntax_evaluator, &"Number/to_text".to_owned())
            .unwrap();
        assert_eq!(lookup_memo.changed_at, EvaluationRevision(0));
        assert_eq!(lookup_memo.verified_at, EvaluationRevision(1));
        let keep_abi_memo = state
            .owner_inference_abi_requests
            .memo(&state.syntax_evaluator, &keep)
            .unwrap();
        assert_eq!(keep_abi_memo.changed_at, EvaluationRevision(0));
        assert_eq!(keep_abi_memo.verified_at, EvaluationRevision(1));
        let construction_memo = state
            .owner_construction_abi_requests
            .memo(&state.syntax_evaluator, &keep)
            .unwrap();
        assert_eq!(construction_memo.changed_at, EvaluationRevision(0));
        assert_eq!(construction_memo.verified_at, EvaluationRevision(1));
        let shard_memo = state
            .checked_owner_shard_requests
            .memo(&state.syntax_evaluator, &keep)
            .unwrap();
        assert_eq!(shard_memo.changed_at, EvaluationRevision(0));
        assert_eq!(shard_memo.verified_at, EvaluationRevision(1));
    }

    #[test]
    fn role_qualified_external_value_has_an_exact_missing_lookup() {
        let mut session = CompilerSession::new();
        let project = session
            .open_project(project("value: Session/store.missing\n"))
            .unwrap();
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
                        if owner.item_route.segments().last().is_some_and(|segment| segment.names == ["value"])
                )
            })
            .unwrap();
        let summary = session
            .owner_constraint_summary(project, &owner)
            .unwrap()
            .unwrap();
        let body = session
            .owner_body_inference(project, &owner)
            .unwrap()
            .unwrap();
        assert_eq!(
            summary.authoritative_value_abi_paths().as_ref(),
            ["Session/store.missing"]
        );
        assert!(body.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "unknown_external_value"
                && diagnostic.message.contains("Session/store.missing")
        }));

        let state = session.projects.get(&project).unwrap();
        let lookup = state
            .owner_value_abi_lookup_requests
            .current_value(&state.syntax_evaluator, &"Session/store.missing".to_owned())
            .unwrap()
            .unwrap();
        assert!(matches!(
            lookup.outcome(),
            boon_typecheck::OwnerValueAbiLookupOutcome::Missing {
                allow_unresolved: false
            }
        ));
        let abi = state
            .owner_inference_abi_requests
            .current_value(&state.syntax_evaluator, &owner)
            .unwrap()
            .unwrap();
        assert_eq!(abi.value_lookups(), std::slice::from_ref(lookup.as_ref()));
    }

    #[test]
    fn cross_owner_function_used_as_value_keeps_exact_invalid_resolution() {
        let mut session = CompilerSession::new();
        let project = session
            .open_project(project("FUNCTION helper() {\n    1\n}\nvalue: helper\n"))
            .unwrap();
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
                        if owner.item_route.segments().last().is_some_and(|segment| segment.names == ["value"])
                )
            })
            .unwrap();
        let summary = session
            .owner_constraint_summary(project, &owner)
            .unwrap()
            .unwrap();
        assert!(matches!(
            summary.symbol_resolutions.as_ref(),
            [OwnerSymbolResolution::CallableAsValue { reference }]
                if reference.parts.as_ref() == ["helper"]
        ));
        let body = session
            .owner_body_inference(project, &owner)
            .unwrap()
            .unwrap();
        assert!(body.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "function_must_be_called"
                && diagnostic
                    .message
                    .contains("function `helper` must be called")
        }));
        assert!(
            session
                .checked_owner_shard(project, &owner)
                .unwrap()
                .is_some()
        );
    }

    #[test]
    fn authoritative_callable_used_as_value_has_an_exact_callable_lookup() {
        let mut session = CompilerSession::new();
        let project_id = session
            .open_project(project("value: Number.to_text\n"))
            .unwrap();
        parse_project_snapshot(session.projects.get_mut(&project_id).unwrap()).unwrap();
        let unit = session
            .unit_syntax_snapshot(project_id, "RUN.bn")
            .unwrap()
            .unwrap();
        let owner = unit
            .stable_check_owner_keys()
            .find(|owner| {
                matches!(
                    owner,
                    StableCheckOwnerKey::Item(owner)
                        if owner.item_route.segments().last().is_some_and(|segment| segment.names == ["value"])
                )
            })
            .unwrap();
        let summary = session
            .owner_constraint_summary(project_id, &owner)
            .unwrap()
            .unwrap();
        assert!(
            matches!(
                summary.symbol_resolutions.as_ref(),
                [OwnerSymbolResolution::CallableAsValue { reference }]
                    if reference.parts.as_ref() == ["Number", "to_text"]
            ),
            "{:#?}",
            summary.symbol_resolutions
        );
        let body = session
            .owner_body_inference(project_id, &owner)
            .unwrap()
            .unwrap();
        assert!(body.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "function_must_be_called"
                && diagnostic
                    .message
                    .contains("function `Number/to_text` must be called")
        }));
        let state = session.projects.get(&project_id).unwrap();
        let lookup = state
            .owner_callable_abi_lookup_requests
            .current_value(&state.syntax_evaluator, &"Number/to_text".to_owned())
            .unwrap()
            .unwrap();
        assert!(matches!(
            lookup.outcome(),
            boon_typecheck::OwnerCallableAbiLookupOutcome::Found { .. }
        ));

        let mut shadowing_session = CompilerSession::new();
        let shadowing_project = shadowing_session
            .open_project(project(concat!(
                "FUNCTION helper() {\n",
                "    1\n",
                "}\n",
                "container: [\n",
                "    helper: 7\n",
                "    value: helper\n",
                "]\n",
            )))
            .unwrap();
        parse_project_snapshot(
            shadowing_session
                .projects
                .get_mut(&shadowing_project)
                .unwrap(),
        )
        .unwrap();
        let unit = shadowing_session
            .unit_syntax_snapshot(shadowing_project, "RUN.bn")
            .unwrap()
            .unwrap();
        let owner = unit
            .stable_check_owner_keys()
            .find(|owner| {
                matches!(
                    owner,
                    StableCheckOwnerKey::Item(owner)
                        if owner.item_route.segments().last().is_some_and(|segment| segment.names == ["value"])
                )
            })
            .unwrap();
        let summary = shadowing_session
            .owner_constraint_summary(shadowing_project, &owner)
            .unwrap()
            .unwrap();
        // The record sibling is now an exact lexical import. It must never
        // fall back through the project symbol index merely because a project
        // function has the same spelling.
        assert!(summary.symbol_resolutions.is_empty());
        assert!(summary.lexical_captures.iter().any(|target| {
            matches!(
                target,
                boon_checked::OwnerLexicalTargetRef::Declaration {
                    declaration: boon_checked::OwnerDeclarationStableKey::Public,
                    capability: boon_checked::OwnerLexicalDeclarationCapability::Value,
                    ..
                }
            )
        }));
        let body = shadowing_session
            .owner_body_inference(shadowing_project, &owner)
            .unwrap()
            .unwrap();
        assert!(
            !body
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "function_must_be_called")
        );
    }

    #[test]
    fn scoped_value_lookup_must_consume_the_authored_reference() {
        let source = concat!(
            "store: [events: [press: SOURCE]]\n",
            "theme_options: [\n",
            "    mode: Light |> HOLD state {\n",
            "        store.events.press |> THEN {\n",
            "            state |> WHEN { Light => Dark, Dark => Light }\n",
            "        }\n",
            "    }\n",
            "]\n",
        );
        let mut session = CompilerSession::new();
        let project = session.open_project(project(source)).unwrap();
        let (_, diagnostics, _, _, _) =
            parse_project_diagnostics_snapshot(session.projects.get_mut(&project).unwrap())
                .unwrap();
        assert!(diagnostics.diagnostics().is_empty());

        let unit = session
            .unit_syntax_snapshot(project, "RUN.bn")
            .unwrap()
            .unwrap();
        let hold = unit
            .stable_check_owner_keys()
            .find(|owner| {
                matches!(
                    owner,
                    StableCheckOwnerKey::Item(owner)
                        if owner.item_route.segments().last().is_some_and(
                            |segment| segment.names == ["mode", "state"]
                        )
                )
            })
            .unwrap();
        let summary = session
            .owner_constraint_summary(project, &hold)
            .unwrap()
            .unwrap();
        let resolution = summary
            .symbol_resolutions
            .iter()
            .find(|resolution| {
                resolution
                    .reference()
                    .parts
                    .first()
                    .is_some_and(|part| part == "store")
            })
            .expect("the HOLD body must retain its exact store read");
        let OwnerSymbolResolution::Resolved {
            owner, projection, ..
        } = resolution
        else {
            panic!("the store read must resolve to a project value: {resolution:#?}");
        };
        assert!(
            matches!(
                owner,
                StableCheckOwnerKey::Item(owner)
                    if owner.item_route.segments().first().is_some_and(
                        |segment| segment.names.first().is_some_and(|name| name == "store")
                    )
            ),
            "the sibling read resolved through the enclosing theme_options value: {owner:?}"
        );
        assert_ne!(projection.first().map(String::as_str), Some("store"));
    }

    #[test]
    fn source_payload_has_one_exact_owner_inference_lookup() {
        let mut session = CompilerSession::new();
        let project = session
            .open_project(project("event: SOURCE\nuse: event.key\n"))
            .unwrap();
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
                        if owner.item_route.segments().last().is_some_and(|segment| segment.names == ["event"])
                )
            })
            .unwrap();
        let body = session
            .owner_body_inference(project, &owner)
            .unwrap()
            .unwrap();
        assert!(body.diagnostics.is_empty());

        let state = session.projects.get(&project).unwrap();
        let lookup = state
            .owner_source_payload_abi_lookup_requests
            .current_value(&state.syntax_evaluator, &"event".to_owned())
            .unwrap()
            .unwrap();
        assert!(matches!(
            lookup.outcome(),
            boon_typecheck::OwnerSourcePayloadAbiLookupOutcome::Found {
                payload_type: boon_checked::Type::Object(shape)
            } if shape.fields.get("key") == Some(&boon_checked::Type::Text)
        ));
        let abi = state
            .owner_inference_abi_requests
            .current_value(&state.syntax_evaluator, &owner)
            .unwrap()
            .unwrap();
        assert_eq!(
            abi.source_payload_lookups(),
            std::slice::from_ref(lookup.as_ref())
        );
    }

    #[test]
    fn function_parameter_has_one_exact_missing_requirement_lookup() {
        let mut session = CompilerSession::new();
        let project = session
            .open_project(project("FUNCTION identity(input) {\n    input\n}\n"))
            .unwrap();
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
        let key = OwnerParameterRequirementKey::new(owner.clone(), 0);
        let state = session.projects.get(&project).unwrap();
        let lookup = state
            .owner_parameter_requirement_lookup_requests
            .current_value(&state.syntax_evaluator, &key)
            .unwrap()
            .unwrap();
        assert!(matches!(
            lookup.outcome(),
            boon_typecheck::OwnerParameterRequirementLookupOutcome::Missing
        ));
        let abi = state
            .owner_inference_abi_requests
            .current_value(&state.syntax_evaluator, &owner)
            .unwrap()
            .unwrap();
        assert_eq!(
            abi.parameter_requirement_lookups(),
            std::slice::from_ref(lookup.as_ref())
        );
    }

    #[test]
    fn unrelated_source_payload_change_stays_outside_the_owner_cone() {
        let before = concat!(
            "keep: SOURCE\n",
            "keep_use: keep.key\n",
            "other: SOURCE\n",
            "other_use: other.left\n",
        );
        let after = concat!(
            "keep: SOURCE\n",
            "keep_use: keep.key\n",
            "other: SOURCE\n",
            "other_use: other.right\n",
        );
        let mut session = CompilerSession::new();
        let project = session.open_project(project(before)).unwrap();
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
        let keep = owner_named("keep");
        let other = owner_named("other");
        let first_summary = session
            .owner_constraint_summary(project, &keep)
            .unwrap()
            .unwrap();
        let first_interface = session
            .owner_interface_result(project, &keep)
            .unwrap()
            .unwrap();
        let first_body = session
            .owner_body_inference(project, &keep)
            .unwrap()
            .unwrap();
        let (first_provider_fingerprint, first_lookup, first_keep_abi, first_other_abi) = {
            let state = session.projects.get(&project).unwrap();
            (
                state
                    .project_owner_abi_requests
                    .current_value(&state.syntax_evaluator, &ProjectOwnerAbiKey)
                    .unwrap()
                    .unwrap()
                    .fingerprint_v1(),
                Arc::clone(
                    state
                        .owner_source_payload_abi_lookup_requests
                        .current_value(&state.syntax_evaluator, &"keep".to_owned())
                        .unwrap()
                        .unwrap(),
                ),
                Arc::clone(
                    state
                        .owner_inference_abi_requests
                        .current_value(&state.syntax_evaluator, &keep)
                        .unwrap()
                        .unwrap(),
                ),
                Arc::clone(
                    state
                        .owner_inference_abi_requests
                        .current_value(&state.syntax_evaluator, &other)
                        .unwrap()
                        .unwrap(),
                ),
            )
        };

        session
            .apply_update(project, UnitUpdate::new("RUN.bn", after))
            .unwrap();
        parse_project_snapshot(session.projects.get_mut(&project).unwrap()).unwrap();
        let second_summary = session
            .owner_constraint_summary(project, &keep)
            .unwrap()
            .unwrap();
        let second_interface = session
            .owner_interface_result(project, &keep)
            .unwrap()
            .unwrap();
        let second_body = session
            .owner_body_inference(project, &keep)
            .unwrap()
            .unwrap();
        let state = session.projects.get(&project).unwrap();
        let second_provider_fingerprint = state
            .project_owner_abi_requests
            .current_value(&state.syntax_evaluator, &ProjectOwnerAbiKey)
            .unwrap()
            .unwrap()
            .fingerprint_v1();
        let second_lookup = state
            .owner_source_payload_abi_lookup_requests
            .current_value(&state.syntax_evaluator, &"keep".to_owned())
            .unwrap()
            .unwrap();
        let second_keep_abi = state
            .owner_inference_abi_requests
            .current_value(&state.syntax_evaluator, &keep)
            .unwrap()
            .unwrap();
        let second_other_abi = state
            .owner_inference_abi_requests
            .current_value(&state.syntax_evaluator, &other)
            .unwrap()
            .unwrap();

        assert_ne!(first_provider_fingerprint, second_provider_fingerprint);
        assert!(Arc::ptr_eq(&first_lookup, second_lookup));
        assert!(Arc::ptr_eq(&first_keep_abi, second_keep_abi));
        assert!(!Arc::ptr_eq(&first_other_abi, second_other_abi));
        assert!(Arc::ptr_eq(&first_summary, &second_summary));
        assert!(Arc::ptr_eq(&first_interface, &second_interface));
        assert!(Arc::ptr_eq(&first_body, &second_body));

        let lookup_memo = state
            .owner_source_payload_abi_lookup_requests
            .memo(&state.syntax_evaluator, &"keep".to_owned())
            .unwrap();
        assert_eq!(lookup_memo.changed_at, EvaluationRevision(0));
        assert_eq!(lookup_memo.verified_at, EvaluationRevision(1));
        let keep_abi_memo = state
            .owner_inference_abi_requests
            .memo(&state.syntax_evaluator, &keep)
            .unwrap();
        assert_eq!(keep_abi_memo.changed_at, EvaluationRevision(0));
        assert_eq!(keep_abi_memo.verified_at, EvaluationRevision(1));
    }

    #[test]
    fn owner_body_request_shares_transitive_result_transfer_modules() {
        let source = concat!(
            "FUNCTION leaf() {\n",
            "    PASSED.store.count\n",
            "}\n",
            "FUNCTION inherited() {\n",
            "    leaf()\n",
            "}\n",
            "value: inherited(PASS: [store: [count: 1]])\n",
        );
        let mut session = CompilerSession::new();
        let project = session.open_project(project(source)).unwrap();
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
        let value = owner_named("value");
        let inherited = owner_named("inherited");
        let body = session
            .owner_body_inference(project, &value)
            .unwrap()
            .unwrap();
        let body_evaluation = {
            let state = session.projects.get(&project).unwrap();
            Arc::clone(
                state
                    .owner_body_inference_evaluation_requests
                    .current_value(&state.syntax_evaluator, &value)
                    .unwrap()
                    .unwrap(),
            )
        };
        assert!(Arc::ptr_eq(&body_evaluation.result, &body));
        let imported = body_evaluation
            .currentness
            .interface_imports()
            .iter()
            .map(|interface| interface.owner.clone())
            .collect::<BTreeSet<_>>();

        assert_eq!(imported, BTreeSet::from([value, inherited]));
        assert_eq!(body.work.interface_plan_required_owners, 2);
        assert_eq!(body.work.interface_plan_result_transfers, 2);
        assert_eq!(body.calls[0].result.ty, boon_checked::Type::Number);
    }

    #[test]
    fn explicit_pass_record_forwards_the_callers_same_named_value() {
        let source = concat!(
            "store: [count: 1]\n",
            "FUNCTION read_count() {\n",
            "    PASSED.store.count\n",
            "}\n",
            "value: read_count(PASS: [store: store])\n",
        );
        let mut session = CompilerSession::new();
        let project = session.open_project(project(source)).unwrap();
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
        let store = owner_named("store");
        let value = owner_named("value");
        let summary = session
            .owner_constraint_summary(project, &value)
            .unwrap()
            .unwrap();
        assert!(summary.symbol_resolutions.iter().any(|resolution| {
            matches!(
                resolution,
                OwnerSymbolResolution::Resolved {
                    reference,
                    owner,
                    projection,
                    ..
                } if reference.parts.as_ref() == ["store"]
                    && owner == &store
                    && projection.is_empty()
            )
        }));

        let body = session
            .owner_body_inference(project, &value)
            .unwrap()
            .unwrap();
        assert!(body.diagnostics.is_empty(), "{:#?}", body.diagnostics);
        let store_actual = body
            .expressions
            .iter()
            .find(|expression| {
                matches!(&expression.kind, boon_syntax::AstExprKind::Identifier(name) if name == "store")
            })
            .expect("explicit PASS store initializer");
        let boon_checked::Type::Object(shape) = &store_actual.flow_type.ty else {
            panic!(
                "explicit PASS must retain the caller's store object: {:?}",
                store_actual.flow_type.ty
            );
        };
        assert_eq!(shape.fields.get("count"), Some(&boon_checked::Type::Number));
        assert!(!shape.open);
    }

    #[test]
    fn unused_value_dependencies_do_not_invalidate_transfer_modules_or_callers() {
        let before = concat!(
            "source: 1\n",
            "FUNCTION identity(input) {\n",
            "    ignored: source\n",
            "    input\n",
            "}\n",
            "value: identity(input: 7)\n",
        );
        let after = before.replacen("source: 1", "source: TEXT { changed }", 1);
        let mut session = CompilerSession::new();
        let project = session.open_project(project(before)).unwrap();
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
        let identity = owner_named("identity");
        let value = owner_named("value");
        let first_identity_body = session
            .owner_body_inference(project, &identity)
            .unwrap()
            .unwrap();
        let first_value_body = session
            .owner_body_inference(project, &value)
            .unwrap()
            .unwrap();
        let first_module = {
            let state = session.projects.get(&project).unwrap();
            let provider = state
                .owner_interface_provider_requests
                .current_value(&state.syntax_evaluator, &identity)
                .unwrap()
                .unwrap();
            Arc::clone(
                state
                    .owner_interface_transfer_module_requests
                    .current_value(&state.syntax_evaluator, provider.key())
                    .unwrap()
                    .unwrap(),
            )
        };

        session
            .apply_update(project, UnitUpdate::new("RUN.bn", after))
            .unwrap();
        parse_project_snapshot(session.projects.get_mut(&project).unwrap()).unwrap();

        let second_identity_body = session
            .owner_body_inference(project, &identity)
            .unwrap()
            .unwrap();
        let second_value_body = session
            .owner_body_inference(project, &value)
            .unwrap()
            .unwrap();
        let second_module = {
            let state = session.projects.get(&project).unwrap();
            let provider = state
                .owner_interface_provider_requests
                .current_value(&state.syntax_evaluator, &identity)
                .unwrap()
                .unwrap();
            Arc::clone(
                state
                    .owner_interface_transfer_module_requests
                    .current_value(&state.syntax_evaluator, provider.key())
                    .unwrap()
                    .unwrap(),
            )
        };

        assert!(!Arc::ptr_eq(&first_identity_body, &second_identity_body));
        assert!(Arc::ptr_eq(&first_module, &second_module));
        assert!(Arc::ptr_eq(&first_value_body, &second_value_body));
        assert_eq!(
            second_value_body.calls[0].result.ty,
            boon_checked::Type::Number
        );
    }

    #[test]
    fn transitive_transfer_changes_invalidate_callers_behind_equal_interfaces() {
        let before = concat!(
            "FUNCTION leaf(kind) {\n",
            "    kind |> WHEN {\n",
            "        A => 1\n",
            "        __ => TEXT { other }\n",
            "    }\n",
            "}\n",
            "FUNCTION inherited(kind) {\n",
            "    leaf(kind: kind)\n",
            "}\n",
            "value: inherited(kind: A)\n",
        );
        let after = concat!(
            "FUNCTION leaf(kind) {\n",
            "    kind |> WHEN {\n",
            "        A => TEXT { other }\n",
            "        __ => 1\n",
            "    }\n",
            "}\n",
            "FUNCTION inherited(kind) {\n",
            "    leaf(kind: kind)\n",
            "}\n",
            "value: inherited(kind: A)\n",
        );
        let mut session = CompilerSession::new();
        let project = session.open_project(project(before)).unwrap();
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
        let leaf = owner_named("leaf");
        let inherited = owner_named("inherited");
        let value = owner_named("value");
        let first_leaf_interface = session
            .owner_interface_result(project, &leaf)
            .unwrap()
            .unwrap();
        let first_inherited_interface = session
            .owner_interface_result(project, &inherited)
            .unwrap()
            .unwrap();
        let first_value_body = session
            .owner_body_inference(project, &value)
            .unwrap()
            .unwrap();
        let (first_leaf_module, first_inherited_module, first_value_evaluation) = {
            let state = session.projects.get(&project).unwrap();
            let module = |owner: &StableCheckOwnerKey| {
                let provider = state
                    .owner_interface_provider_requests
                    .current_value(&state.syntax_evaluator, owner)
                    .unwrap()
                    .unwrap();
                Arc::clone(
                    state
                        .owner_interface_transfer_module_requests
                        .current_value(&state.syntax_evaluator, provider.key())
                        .unwrap()
                        .unwrap(),
                )
            };
            (
                module(&leaf),
                module(&inherited),
                Arc::clone(
                    state
                        .owner_body_inference_evaluation_requests
                        .current_value(&state.syntax_evaluator, &value)
                        .unwrap()
                        .unwrap(),
                ),
            )
        };
        assert_eq!(
            first_value_body.calls[0].result.ty,
            boon_checked::Type::Number
        );

        session
            .apply_update(project, UnitUpdate::new("RUN.bn", after))
            .unwrap();
        parse_project_snapshot(session.projects.get_mut(&project).unwrap()).unwrap();

        let second_leaf_interface = session
            .owner_interface_result(project, &leaf)
            .unwrap()
            .unwrap();
        let second_inherited_interface = session
            .owner_interface_result(project, &inherited)
            .unwrap()
            .unwrap();
        let second_value_body = session
            .owner_body_inference(project, &value)
            .unwrap()
            .unwrap();
        let (second_leaf_module, second_inherited_module, second_value_evaluation) = {
            let state = session.projects.get(&project).unwrap();
            let module = |owner: &StableCheckOwnerKey| {
                let provider = state
                    .owner_interface_provider_requests
                    .current_value(&state.syntax_evaluator, owner)
                    .unwrap()
                    .unwrap();
                Arc::clone(
                    state
                        .owner_interface_transfer_module_requests
                        .current_value(&state.syntax_evaluator, provider.key())
                        .unwrap()
                        .unwrap(),
                )
            };
            (
                module(&leaf),
                module(&inherited),
                Arc::clone(
                    state
                        .owner_body_inference_evaluation_requests
                        .current_value(&state.syntax_evaluator, &value)
                        .unwrap()
                        .unwrap(),
                ),
            )
        };

        assert!(Arc::ptr_eq(&first_leaf_interface, &second_leaf_interface));
        assert!(Arc::ptr_eq(
            &first_inherited_interface,
            &second_inherited_interface
        ));
        assert!(!Arc::ptr_eq(&first_leaf_module, &second_leaf_module));
        assert!(!Arc::ptr_eq(
            &first_inherited_module,
            &second_inherited_module
        ));
        assert_ne!(
            first_value_evaluation
                .currentness
                .basis()
                .interface_plan_fingerprint_v1,
            second_value_evaluation
                .currentness
                .basis()
                .interface_plan_fingerprint_v1,
        );
        assert_ne!(
            first_value_evaluation.currentness.fingerprint_v1(),
            second_value_evaluation.currentness.fingerprint_v1(),
        );
        assert!(!Arc::ptr_eq(&first_value_body, &second_value_body));
        assert_eq!(
            second_value_body.calls[0].result.ty,
            boon_checked::Type::Text
        );
    }

    #[test]
    fn transfer_module_dependencies_are_canonical_exact_and_fail_closed() {
        let source = concat!(
            "other: 1\n",
            "foreign: 2\n",
            "FUNCTION identity(input) {\n",
            "    input\n",
            "}\n",
            "FUNCTION wrapper(input) {\n",
            "    ignored: other\n",
            "    identity(input: input)\n",
            "}\n",
            "value: wrapper(input: 7)\n",
        );
        let mut session = CompilerSession::new();
        let project = session.open_project(project(source)).unwrap();
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
        let other = owner_named("other");
        let foreign = owner_named("foreign");
        let identity = owner_named("identity");
        let wrapper = owner_named("wrapper");
        let state = session.projects.get(&project).unwrap();
        let provider_key = |owner: &StableCheckOwnerKey| {
            state
                .owner_interface_provider_requests
                .current_value(&state.syntax_evaluator, owner)
                .unwrap()
                .unwrap()
                .key()
                .clone()
        };
        let wrapper_key = provider_key(&wrapper);
        let identity_key = provider_key(&identity);
        let other_key = provider_key(&other);
        let foreign_key = provider_key(&foreign);
        let module = |key: &OwnerInterfaceSccKey| {
            Arc::clone(
                state
                    .owner_interface_transfer_module_requests
                    .current_value(&state.syntax_evaluator, key)
                    .unwrap()
                    .unwrap(),
            )
        };
        let production = module(&wrapper_key);
        assert_eq!(
            production
                .direct_dependency_keys()
                .cloned()
                .collect::<Vec<_>>(),
            vec![identity_key.clone()]
        );
        assert_ne!(identity_key, other_key);
        assert_ne!(identity_key, foreign_key);
        assert!(
            production
                .direct_dependency_keys()
                .all(|key| key != &other_key && key != &foreign_key),
            "unrelated value dependencies must not enter the compiled residual module",
        );
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
        let request_delta = |after: RequestEvaluationStats, before: RequestEvaluationStats| {
            (
                after.demanded - before.demanded,
                after.executed - before.executed,
                after.reused - before.reused,
                after.backdated - before.backdated,
                after.changed - before.changed,
            )
        };
        let interface_delta = request_delta(interface_stats, cold_stats);

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
        let body_delta = request_delta(body_stats, interface_stats);
        // The callable-only ABI, resolution, scope topology/SCC, and provider
        // families are included here. An exported callable change reexecutes
        // its exact scope cone and backdates unchanged projections; the body-
        // only edit reuses 163 of 205 demanded requests and changes only 25.
        // The exact child-boundary lexical projection adds five executions to
        // the exported-interface cone. The project diagnostic-facts request
        // adds one exact execution/change to both cones. Three source-unit
        // owner presentation requests add one changed/reexecuted unit and two
        // reused units to each cone. Every source unit also publishes one
        // current project-diagnostic evaluation and one independently
        // backdatable local row projection. Each live owner publishes one
        // current diagnostic-replay evaluation and one normalized semantic fact.
        // The shared output-flow component executes once per cone and
        // backdates when the body-only edit leaves its graph unchanged.
        // The live interface-component residual transaction directly records
        // its exact transfer-module basis, so an exported interface edit also
        // reexecutes and backdates one unchanged component projection that the
        // retired post-solve transaction could reuse without that edge.
        assert_eq!(
            (interface_delta, body_delta),
            ((205, 103, 102, 47, 56), (205, 42, 163, 17, 25))
        );
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
        let warm_result = warm
            .request(
                warm_project,
                edited_revision,
                CompileIntent::Diagnostics,
                &CancellationToken::new(),
            )
            .unwrap();
        let warm_diagnostics = warm_result.diagnostics().unwrap();
        let warm_fingerprint = warm_diagnostics.fingerprint_v1();
        let warm_diagnostics = warm_diagnostics.diagnostics().to_vec();

        let mut edited_units = units;
        edited_units
            .iter_mut()
            .find(|unit| unit.path == edit_path)
            .unwrap()
            .source = edited_source;
        let mut clean = CompilerSession::new();
        let clean_project = clean.open_project(project_for(edited_units)).unwrap();
        let clean_revision = clean.revision(clean_project).unwrap();
        let clean_result = clean
            .request(
                clean_project,
                clean_revision,
                CompileIntent::Diagnostics,
                &CancellationToken::new(),
            )
            .unwrap();
        let clean_diagnostics = clean_result.diagnostics().unwrap();
        let clean_fingerprint = clean_diagnostics.fingerprint_v1();
        let clean_diagnostics = clean_diagnostics.diagnostics().to_vec();

        assert_eq!(warm_diagnostics, clean_diagnostics);
        assert_eq!(warm_fingerprint, clean_fingerprint);
        assert!(
            warm_diagnostics.is_empty(),
            "text-only TodoMVC edit produced diagnostics: {:#?}",
            warm_diagnostics,
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
        {
            let state = session.projects.get(&project).unwrap();
            assert_eq!(
                state.owner_body_inference_requests.request_count(),
                state.owner_input_requests.request_count()
            );
        }

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
