use crate::{
    ApplicationIdentity, DocumentFrame, DocumentPatch, LiveRuntime, ProgramCapabilityProfile,
    RowId, RuntimeSourceUnit, SessionOptions, SourcePayload,
};
use boon_compiler::{
    COMPILER_ID, CompileProfile, CompilerSourceUnit, DistributedCompilerProgram,
    compile_distributed_runtime_source_programs,
    compile_runtime_source_units_to_machine_plan_for_role_with_identity,
    diagnose_runtime_source_units,
};
use boon_document_model::{
    DocumentNodeId, DocumentNodeKind, EmbeddedProgramDescriptor, ProgramArtifactRetention,
    ScrollRootId, SourceBindingId,
};
#[cfg(not(target_arch = "wasm32"))]
use boon_persistence::{
    BarrierAck, CommitAck, PersistenceControlError, PersistenceDriver, PersistenceWorkerConfig,
    PersistenceWorkerStatus, ShutdownAck,
};
use boon_persistence::{
    ContentArtifact, ContentArtifactId, ContentArtifactOwnerId, ContentArtifactRetention,
    validate_content_artifact,
};
use boon_plan::{
    DocumentConstructor, EffectBarrier, EffectReplay, MachinePlan, OutputContractKind, ProgramRole,
    TargetProfile,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::ops::Range;
use std::sync::Arc;

const MAX_DIAGNOSTIC_BYTES: usize = 4 * 1024;
const MAX_TRUSTED_PACKAGE_SOURCE_UNITS: usize = 256;
const MAX_TRUSTED_PACKAGE_SOURCE_BYTES: usize = 8 * 1024 * 1024;
const PROGRAM_ARTIFACT_FORMAT: u32 = 1;
const PROGRAM_ARTIFACT_MEDIA_TYPE: &str = "application/vnd.boon.machine-plan+cbor;version=1";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProgramLimits {
    pub max_source_units: usize,
    pub max_source_bytes: usize,
    pub max_operations: usize,
    pub max_scalar_slots: usize,
    pub max_list_slots: usize,
    pub max_source_routes: usize,
    pub max_output_roots: usize,
    pub max_effect_contracts: usize,
    pub max_document_expressions: usize,
    pub max_document_templates: usize,
    pub max_document_materializations: usize,
    pub max_declared_list_capacity: usize,
    pub max_runtime_work_units_per_transaction: u64,
}

fn program_limits(profile: ProgramCapabilityProfile) -> ProgramLimits {
    match profile {
        ProgramCapabilityProfile::PublicClient => ProgramLimits {
            max_source_units: 8,
            max_source_bytes: 64 * 1024,
            max_operations: 10_000,
            max_scalar_slots: 512,
            max_list_slots: 64,
            max_source_routes: 128,
            max_output_roots: usize::MAX,
            max_effect_contracts: 32,
            max_document_expressions: 10_000,
            max_document_templates: 2_000,
            max_document_materializations: 128,
            max_declared_list_capacity: 4_096,
            max_runtime_work_units_per_transaction: 20_000,
        },
        ProgramCapabilityProfile::TrustedSession => ProgramLimits {
            max_source_units: 16,
            max_source_bytes: 256 * 1024,
            max_operations: 50_000,
            max_scalar_slots: 2_048,
            max_list_slots: 256,
            max_source_routes: 1_024,
            max_output_roots: 128,
            max_effect_contracts: 128,
            max_document_expressions: 0,
            max_document_templates: 0,
            max_document_materializations: 0,
            max_declared_list_capacity: 32_768,
            max_runtime_work_units_per_transaction: 100_000,
        },
        ProgramCapabilityProfile::TrustedServer => ProgramLimits {
            max_source_units: 32,
            max_source_bytes: 512 * 1024,
            max_operations: 100_000,
            max_scalar_slots: 4_096,
            max_list_slots: 512,
            max_source_routes: 2_048,
            max_output_roots: 256,
            max_effect_contracts: 256,
            max_document_expressions: 0,
            max_document_templates: 0,
            max_document_materializations: 0,
            max_declared_list_capacity: 65_536,
            max_runtime_work_units_per_transaction: 200_000,
        },
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProgramCompileRequest {
    pub revision: u64,
    pub role: ProgramRole,
    pub entry_path: String,
    pub units: Vec<RuntimeSourceUnit>,
    pub application: ApplicationIdentity,
    pub capability_profile: ProgramCapabilityProfile,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProgramDiagnosticPhase {
    Request,
    Compile,
    Capability,
    Artifact,
    Start,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProgramDiagnostic {
    pub revision: u64,
    pub phase: ProgramDiagnosticPhase,
    pub source_path: String,
    pub line: usize,
    pub column: usize,
    pub message: String,
}

impl ProgramDiagnostic {
    fn new(revision: u64, phase: ProgramDiagnosticPhase, message: impl Into<String>) -> Self {
        Self {
            revision,
            phase,
            source_path: String::new(),
            line: 0,
            column: 0,
            message: bounded_diagnostic(message.into()),
        }
    }

    fn with_source_location(
        mut self,
        source_path: impl Into<String>,
        line: Option<usize>,
        column: Option<usize>,
    ) -> Self {
        self.source_path = source_path.into();
        self.line = line.unwrap_or_default();
        self.column = column.unwrap_or_default();
        self
    }

    pub fn artifact(revision: u64, message: impl Into<String>) -> Self {
        Self::new(revision, ProgramDiagnosticPhase::Artifact, message)
    }
}

impl fmt::Display for ProgramDiagnostic {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "program revision {} {:?} failed",
            self.revision, self.phase
        )?;
        if !self.source_path.is_empty() {
            write!(formatter, " at {}", self.source_path)?;
            if self.line > 0 {
                write!(formatter, ":{}", self.line)?;
                if self.column > 0 {
                    write!(formatter, ":{}", self.column)?;
                }
            }
        }
        write!(formatter, ": {}", self.message)
    }
}

impl std::error::Error for ProgramDiagnostic {}

#[derive(Clone, Debug)]
pub struct ProgramArtifact {
    id: ContentArtifactId,
    revision: u64,
    source_digest: String,
    plan_digest: String,
    capability_profile: ProgramCapabilityProfile,
    compile_profile: CompileProfile,
    plan: Arc<MachinePlan>,
    content: Arc<ContentArtifact>,
}

impl PartialEq for ProgramArtifact {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id && self.revision == other.revision
    }
}

impl Eq for ProgramArtifact {}

impl ProgramArtifact {
    pub fn id(&self) -> ContentArtifactId {
        self.id
    }

    pub fn id_text(&self) -> String {
        self.id.to_string()
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn source_digest(&self) -> &str {
        &self.source_digest
    }

    pub fn plan_digest(&self) -> &str {
        &self.plan_digest
    }

    pub fn capability_profile(&self) -> ProgramCapabilityProfile {
        self.capability_profile
    }

    pub fn compile_profile(&self) -> CompileProfile {
        self.compile_profile
    }

    pub fn plan(&self) -> &Arc<MachinePlan> {
        &self.plan
    }

    pub fn role(&self) -> ProgramRole {
        self.plan.program_role
    }

    pub fn application(&self) -> &ApplicationIdentity {
        &self.plan.application.identity
    }

    pub fn compiler_id(&self) -> &'static str {
        COMPILER_ID
    }

    pub fn target_profile_id(&self) -> &'static str {
        "software_bounded"
    }

    pub fn capability_profile_id(&self) -> &'static str {
        self.capability_profile.name()
    }

    pub fn to_content_artifact(&self) -> ContentArtifact {
        self.content.as_ref().clone()
    }

    pub fn content_bytes_len(&self) -> usize {
        self.content.bytes.len()
    }

    pub fn from_content_artifact(
        revision: u64,
        expected_capability: ProgramCapabilityProfile,
        artifact: ContentArtifact,
    ) -> Result<Self, ProgramDiagnostic> {
        decode_program_artifact(revision, expected_capability, artifact)
    }
}

#[derive(Serialize, Deserialize)]
struct StoredProgramArtifact {
    format: u32,
    source_digest: String,
    compiler_id: String,
    target_profile: TargetProfile,
    capability_profile: ProgramCapabilityProfile,
    plan_digest: String,
    plan: MachinePlan,
}

fn encode_program_artifact(
    revision: u64,
    source_digest: &str,
    capability_profile: ProgramCapabilityProfile,
    plan: &MachinePlan,
) -> Result<ContentArtifact, ProgramDiagnostic> {
    let plan_digest = boon_plan::plan_sha256(plan).map_err(|error| {
        ProgramDiagnostic::new(
            revision,
            ProgramDiagnosticPhase::Artifact,
            error.to_string(),
        )
    })?;
    let stored = StoredProgramArtifact {
        format: PROGRAM_ARTIFACT_FORMAT,
        source_digest: source_digest.to_owned(),
        compiler_id: COMPILER_ID.to_owned(),
        target_profile: plan.target_profile,
        capability_profile,
        plan_digest,
        plan: plan.clone(),
    };
    let mut bytes = Vec::new();
    ciborium::ser::into_writer(&stored, &mut bytes).map_err(|error| {
        ProgramDiagnostic::new(
            revision,
            ProgramDiagnosticPhase::Artifact,
            format!("encode immutable program artifact: {error}"),
        )
    })?;
    ContentArtifact::new(PROGRAM_ARTIFACT_MEDIA_TYPE, bytes).map_err(|error| {
        ProgramDiagnostic::new(
            revision,
            ProgramDiagnosticPhase::Artifact,
            error.to_string(),
        )
    })
}

fn decode_program_artifact(
    revision: u64,
    expected_capability: ProgramCapabilityProfile,
    artifact: ContentArtifact,
) -> Result<ProgramArtifact, ProgramDiagnostic> {
    validate_content_artifact(&artifact).map_err(|error| {
        ProgramDiagnostic::new(
            revision,
            ProgramDiagnosticPhase::Artifact,
            error.to_string(),
        )
    })?;
    if artifact.media_type != PROGRAM_ARTIFACT_MEDIA_TYPE {
        return Err(ProgramDiagnostic::new(
            revision,
            ProgramDiagnosticPhase::Artifact,
            format!(
                "unsupported program artifact media type `{}`",
                artifact.media_type
            ),
        ));
    }
    let stored: StoredProgramArtifact = ciborium::de::from_reader(artifact.bytes.as_slice())
        .map_err(|error| {
            ProgramDiagnostic::new(
                revision,
                ProgramDiagnosticPhase::Artifact,
                format!("decode immutable program artifact: {error}"),
            )
        })?;
    if stored.format != PROGRAM_ARTIFACT_FORMAT {
        return Err(ProgramDiagnostic::new(
            revision,
            ProgramDiagnosticPhase::Artifact,
            format!("unsupported program artifact format {}", stored.format),
        ));
    }
    if stored.compiler_id != COMPILER_ID {
        return Err(ProgramDiagnostic::new(
            revision,
            ProgramDiagnosticPhase::Artifact,
            format!(
                "program artifact compiler `{}` differs from host compiler `{COMPILER_ID}`",
                stored.compiler_id
            ),
        ));
    }
    if stored.target_profile != TargetProfile::SoftwareBounded
        || stored.plan.target_profile != stored.target_profile
    {
        return Err(ProgramDiagnostic::new(
            revision,
            ProgramDiagnosticPhase::Artifact,
            "program artifact target profile is not software_bounded",
        ));
    }
    if stored.capability_profile != expected_capability {
        return Err(ProgramDiagnostic::new(
            revision,
            ProgramDiagnosticPhase::Artifact,
            "program artifact capability profile differs from the requested profile",
        ));
    }
    let actual_plan_digest = boon_plan::plan_sha256(&stored.plan).map_err(|error| {
        ProgramDiagnostic::new(
            revision,
            ProgramDiagnosticPhase::Artifact,
            error.to_string(),
        )
    })?;
    if stored.plan_digest != actual_plan_digest {
        return Err(ProgramDiagnostic::new(
            revision,
            ProgramDiagnosticPhase::Artifact,
            "program artifact plan digest does not match its compiled plan",
        ));
    }
    validate_plan(revision, expected_capability, &stored.plan)?;
    Ok(ProgramArtifact {
        id: artifact.id,
        revision,
        source_digest: stored.source_digest,
        plan_digest: stored.plan_digest,
        capability_profile: stored.capability_profile,
        compile_profile: CompileProfile::default(),
        plan: Arc::new(stored.plan),
        content: Arc::new(artifact),
    })
}

pub fn compile_program_artifact(
    request: &ProgramCompileRequest,
) -> Result<ProgramArtifact, ProgramDiagnostic> {
    validate_request(request)?;
    compile_validated_program_artifact(request)
}

/// Compiles source already trusted by a package build.
///
/// This preserves the smaller runtime-authored source limits while allowing a
/// bounded production package to contain a larger multi-module application.
/// Plan capability and execution limits remain identical to runtime builds.
pub fn compile_trusted_package_program_artifact(
    request: &ProgramCompileRequest,
) -> Result<ProgramArtifact, ProgramDiagnostic> {
    validate_request_with_source_limits(
        request,
        MAX_TRUSTED_PACKAGE_SOURCE_UNITS,
        MAX_TRUSTED_PACKAGE_SOURCE_BYTES,
    )?;
    compile_validated_program_artifact(request)
}

fn compile_validated_program_artifact(
    request: &ProgramCompileRequest,
) -> Result<ProgramArtifact, ProgramDiagnostic> {
    let source_digest = crate::sha256_bytes(request.units[0].source.as_bytes());
    let units = request
        .units
        .iter()
        .map(|unit| CompilerSourceUnit {
            path: unit.path.clone(),
            source: unit.source.clone(),
        })
        .collect::<Vec<_>>();
    let compiled = compile_runtime_source_units_to_machine_plan_for_role_with_identity(
        &request.entry_path,
        &units,
        TargetProfile::SoftwareBounded,
        request.role,
        request.application.clone(),
    )
    .map_err(|error| {
        let fallback = error.to_string();
        let location = diagnose_runtime_source_units(&request.entry_path, &units)
            .into_iter()
            .next();
        let diagnostic = ProgramDiagnostic::new(
            request.revision,
            ProgramDiagnosticPhase::Compile,
            location
                .as_ref()
                .map_or(fallback, |diagnostic| diagnostic.message.clone()),
        );
        location.map_or(diagnostic.clone(), |location| {
            diagnostic.with_source_location(location.path, location.line, location.column)
        })
    })?;
    artifact_from_compiled(request, source_digest, compiled)
}

pub fn compile_distributed_program_bundle(
    requests: &[ProgramCompileRequest],
) -> Result<DistributedProgramBundle, ProgramDiagnostic> {
    for request in requests {
        validate_request(request)?;
    }
    compile_validated_distributed_program_bundle(requests)
}

pub fn compile_trusted_package_distributed_program_bundle(
    requests: &[ProgramCompileRequest],
) -> Result<DistributedProgramBundle, ProgramDiagnostic> {
    for request in requests {
        validate_request_with_source_limits(
            request,
            MAX_TRUSTED_PACKAGE_SOURCE_UNITS,
            MAX_TRUSTED_PACKAGE_SOURCE_BYTES,
        )?;
    }
    compile_validated_distributed_program_bundle(requests)
}

fn compile_validated_distributed_program_bundle(
    requests: &[ProgramCompileRequest],
) -> Result<DistributedProgramBundle, ProgramDiagnostic> {
    let revision = requests
        .iter()
        .map(|request| request.revision)
        .max()
        .unwrap_or(0);
    let compiler_programs = requests
        .iter()
        .map(|request| DistributedCompilerProgram {
            revision: request.revision,
            role: request.role,
            source_label: request.entry_path.clone(),
            units: request
                .units
                .iter()
                .map(|unit| CompilerSourceUnit {
                    path: unit.path.clone(),
                    source: unit.source.clone(),
                })
                .collect(),
            application: request.application.clone(),
            schema_version: boon_plan::DEFAULT_PERSISTENCE_SCHEMA_VERSION,
            migration_predecessors: Vec::new(),
        })
        .collect::<Vec<_>>();
    let compiled = compile_distributed_runtime_source_programs(
        &compiler_programs,
        TargetProfile::SoftwareBounded,
    )
    .map_err(|error| {
        ProgramDiagnostic::new(revision, ProgramDiagnosticPhase::Compile, error.to_string())
    })?;
    let mut artifacts = Vec::with_capacity(requests.len());
    for (role, compiled) in compiled.into_programs() {
        let request = requests
            .iter()
            .find(|request| request.role == role)
            .ok_or_else(|| {
                ProgramDiagnostic::new(
                    revision,
                    ProgramDiagnosticPhase::Artifact,
                    format!(
                        "joint compiler returned an unexpected {} role",
                        role.as_str()
                    ),
                )
            })?;
        let source_digest = crate::sha256_bytes(request.units[0].source.as_bytes());
        artifacts.push(artifact_from_compiled(request, source_digest, compiled)?);
    }
    DistributedProgramBundle::new(artifacts).map_err(|error| {
        ProgramDiagnostic::new(
            revision,
            ProgramDiagnosticPhase::Artifact,
            error.to_string(),
        )
    })
}

fn artifact_from_compiled(
    request: &ProgramCompileRequest,
    source_digest: String,
    compiled: boon_compiler::CompiledMachinePlanFromSource,
) -> Result<ProgramArtifact, ProgramDiagnostic> {
    validate_plan(request.revision, request.capability_profile, &compiled.plan)?;
    let content = encode_program_artifact(
        request.revision,
        &source_digest,
        request.capability_profile,
        &compiled.plan,
    )?;
    let plan_digest = boon_plan::plan_sha256(&compiled.plan).map_err(|error| {
        ProgramDiagnostic::new(
            request.revision,
            ProgramDiagnosticPhase::Artifact,
            error.to_string(),
        )
    })?;
    Ok(ProgramArtifact {
        id: content.id,
        revision: request.revision,
        source_digest,
        plan_digest,
        capability_profile: request.capability_profile,
        compile_profile: compiled.profile,
        plan: Arc::new(compiled.plan),
        content: Arc::new(content),
    })
}

pub struct ProgramSession {
    id: ProgramSessionId,
    artifact: ProgramArtifact,
    runtime: LiveRuntime,
    next_source_sequence: u64,
}

impl ProgramSession {
    pub fn start(artifact: ProgramArtifact) -> Result<Self, ProgramDiagnostic> {
        let limits = program_limits(artifact.capability_profile());
        let id = deterministic_program_session_id(&artifact);
        let runtime = LiveRuntime::from_shared_machine_plan(
            Arc::clone(artifact.plan()),
            SessionOptions {
                max_work_units_per_transaction: Some(limits.max_runtime_work_units_per_transaction),
                ..SessionOptions::default()
            },
        )
        .map_err(|error| {
            ProgramDiagnostic::new(
                artifact.revision(),
                ProgramDiagnosticPhase::Start,
                error.to_string(),
            )
        })?;
        Ok(Self {
            id,
            artifact,
            runtime,
            next_source_sequence: 1,
        })
    }

    pub fn id(&self) -> &ProgramSessionId {
        &self.id
    }

    pub fn artifact(&self) -> &ProgramArtifact {
        &self.artifact
    }

    pub fn runtime(&self) -> &LiveRuntime {
        &self.runtime
    }

    pub fn runtime_mut(&mut self) -> &mut LiveRuntime {
        &mut self.runtime
    }

    fn fork_distributed_server_evaluation(
        &self,
        turn: Option<&crate::RuntimeTurn>,
    ) -> Result<Self, crate::DistributedRuntimeError> {
        let next_source_sequence = self.evaluation_next_source_sequence(turn)?;
        let runtime = self
            .runtime
            .fork_distributed_server_evaluation(turn.is_some())
            .map_err(distributed_machine_error)?;
        Ok(Self {
            id: self.id.clone(),
            artifact: self.artifact.clone(),
            runtime,
            next_source_sequence,
        })
    }

    fn evaluation_next_source_sequence(
        &self,
        turn: Option<&crate::RuntimeTurn>,
    ) -> Result<u64, crate::DistributedRuntimeError> {
        let Some(source_sequence) = turn.and_then(|turn| turn.source_sequence) else {
            return Ok(self.next_source_sequence);
        };
        if source_sequence != self.next_source_sequence {
            return Err(distributed_machine_error(
                "prepared Server source sequence changed before evaluation",
            ));
        }
        source_sequence
            .checked_add(1)
            .ok_or_else(|| distributed_machine_error("program source sequence overflow"))
    }

    fn validate_distributed_server_evaluation(
        &self,
        turn: Option<&crate::RuntimeTurn>,
        evaluation: &Self,
    ) -> Result<(), crate::DistributedRuntimeError> {
        if self.runtime.has_unsettled_turn() != turn.is_some() {
            return Err(distributed_machine_error(
                "distributed Server authority preparation state changed before commit",
            ));
        }
        if evaluation.runtime.has_unsettled_turn() {
            return Err(distributed_machine_error(
                "distributed Server evaluation remained unsettled",
            ));
        }
        if self.id != evaluation.id
            || self.artifact.id() != evaluation.artifact.id()
            || self.artifact.plan_digest() != evaluation.artifact.plan_digest()
        {
            return Err(distributed_machine_error(
                "distributed Server evaluation belongs to another program authority",
            ));
        }
        let expected_sequence = self.evaluation_next_source_sequence(turn)?;
        if evaluation.next_source_sequence != expected_sequence {
            return Err(distributed_machine_error(
                "distributed Server evaluation source sequence is invalid",
            ));
        }
        Ok(())
    }

    fn install_distributed_server_evaluation(&mut self, evaluation: Self) {
        self.runtime = evaluation.runtime;
        self.next_source_sequence = evaluation.next_source_sequence;
    }

    pub fn frame(&self) -> Option<&DocumentFrame> {
        self.runtime.document_frame()
    }

    pub fn next_source_sequence(&self) -> u64 {
        self.next_source_sequence
    }

    pub fn dispatch(
        &mut self,
        source_path: &str,
        target: Option<RowId>,
        payload: SourcePayload,
    ) -> crate::RuntimeResult<ProgramSessionDispatch> {
        let source_sequence = self.next_source_sequence;
        let next_source_sequence = source_sequence
            .checked_add(1)
            .ok_or("program source sequence overflow")?;
        let event = self
            .runtime
            .source_event(source_sequence, source_path, target, payload)?;
        let runtime_turn = self.runtime.dispatch(event)?;
        self.next_source_sequence = next_source_sequence;
        Ok(ProgramSessionDispatch {
            source_sequence,
            source_path: source_path.to_owned(),
            runtime_turn,
        })
    }

    pub fn root_value_current(&mut self, name: &str) -> crate::RuntimeResult<crate::Value> {
        self.runtime.root_value_current(name)
    }

    pub fn output_value_current(&mut self, name: &str) -> crate::RuntimeResult<crate::Value> {
        self.runtime.output_value_current(name)
    }

    pub fn update_session_context(
        &mut self,
        connection_status: crate::SessionConnectionStatus,
        principal: crate::SessionPrincipal,
    ) -> crate::RuntimeResult<Option<crate::RuntimeTurn>> {
        self.runtime
            .update_session_context(connection_status, principal)
    }

    pub fn complete_transient_effect(
        &mut self,
        call_id: crate::TransientEffectCallId,
        outcome: crate::Value,
    ) -> crate::RuntimeResult<crate::RuntimeTurn> {
        self.runtime.complete_transient_effect(call_id, outcome)
    }

    pub fn deliver_transient_effect_result(
        &mut self,
        call_id: crate::TransientEffectCallId,
        result_sequence: u64,
        outcome: crate::Value,
    ) -> crate::RuntimeResult<crate::RuntimeTurn> {
        self.runtime
            .deliver_transient_effect_result(call_id, result_sequence, outcome)
    }

    pub fn cancel_transient_effect(
        &mut self,
        call_id: crate::TransientEffectCallId,
    ) -> crate::RuntimeResult<bool> {
        self.runtime.cancel_transient_effect(call_id)
    }

    pub fn pending_transient_effect_count(&self) -> usize {
        self.runtime.pending_transient_effect_count()
    }

    pub fn pending_transient_effect_credits(
        &self,
        call_id: crate::TransientEffectCallId,
    ) -> Option<u32> {
        self.runtime.pending_transient_effect_credits(call_id)
    }

    pub fn update_distributed_import(
        &mut self,
        import_id: boon_plan::ImportId,
        content_revision: u64,
        value: crate::Value,
    ) -> crate::RuntimeResult<Option<crate::RuntimeTurn>> {
        self.runtime
            .update_distributed_import(import_id, content_revision, value)
    }

    pub fn distributed_import_revision(&self, import_id: boon_plan::ImportId) -> Option<u64> {
        self.runtime.distributed_import_revision(import_id)
    }

    pub fn distributed_export_value_current(
        &mut self,
        export_id: boon_plan::ExportId,
    ) -> crate::RuntimeResult<crate::Value> {
        self.runtime.distributed_export_value_current(export_id)
    }

    pub fn evaluate_distributed_function(
        &mut self,
        export_id: boon_plan::ExportId,
        arguments: BTreeMap<boon_plan::DistributedArgumentId, crate::Value>,
    ) -> crate::RuntimeResult<crate::Value> {
        self.runtime
            .evaluate_distributed_function(export_id, arguments)
    }

    pub fn distributed_call_arguments_current(
        &mut self,
        call_site_id: boon_plan::RemoteCallSiteId,
    ) -> crate::RuntimeResult<BTreeMap<boon_plan::DistributedArgumentId, crate::Value>> {
        self.runtime
            .distributed_call_arguments_current(call_site_id)
    }
}

impl crate::DistributedServerMachine for ProgramSession {
    type EvaluationMachine = ProgramSession;

    fn artifact(&self) -> &ProgramArtifact {
        &self.artifact
    }

    fn fork_prepared_evaluation(
        &self,
        turn: Option<&crate::RuntimeTurn>,
    ) -> Result<Self::EvaluationMachine, crate::DistributedRuntimeError> {
        self.fork_distributed_server_evaluation(turn)
    }

    fn install_evaluation(
        &mut self,
        evaluation: Self::EvaluationMachine,
    ) -> Result<(), crate::DistributedRuntimeError> {
        self.validate_distributed_server_evaluation(None, &evaluation)?;
        self.install_distributed_server_evaluation(evaluation);
        Ok(())
    }

    fn commit_prepared_evaluation(
        &mut self,
        turn: crate::RuntimeTurn,
        evaluation: Self::EvaluationMachine,
    ) -> Result<crate::RuntimeTurn, crate::DistributedRuntimeError> {
        self.validate_distributed_server_evaluation(Some(&turn), &evaluation)?;
        self.install_distributed_server_evaluation(evaluation);
        Ok(turn)
    }

    fn event_for_path(
        &self,
        path: &str,
        payload: SourcePayload,
    ) -> Result<crate::SourceEvent, crate::DistributedRuntimeError> {
        self.runtime
            .source_event(self.next_source_sequence, path, None, payload)
            .map_err(distributed_machine_error)
    }

    fn event_for_source(
        &self,
        source: boon_plan::SourceId,
        payload: SourcePayload,
    ) -> Result<crate::SourceEvent, crate::DistributedRuntimeError> {
        self.runtime
            .source_event_by_id(self.next_source_sequence, source, None, payload)
            .map_err(distributed_machine_error)
    }

    fn prepare_dispatch(
        &mut self,
        event: crate::SourceEvent,
    ) -> Result<crate::RuntimeTurn, crate::DistributedRuntimeError> {
        self.next_source_sequence
            .checked_add(1)
            .ok_or_else(|| distributed_machine_error("program source sequence overflow"))?;
        self.runtime
            .dispatch_unsettled(event)
            .map_err(distributed_machine_error)
    }

    fn export_current(
        &mut self,
        export_id: boon_plan::ExportId,
    ) -> Result<crate::Value, crate::DistributedRuntimeError> {
        self.runtime
            .distributed_export_value_current(export_id)
            .map_err(distributed_machine_error)
    }

    fn call_arguments(
        &mut self,
        call: &boon_plan::RemoteCallSitePlan,
    ) -> Result<
        BTreeMap<boon_plan::DistributedArgumentId, crate::Value>,
        crate::DistributedRuntimeError,
    > {
        self.runtime
            .distributed_call_arguments_current(call.call_site_id)
            .map_err(distributed_machine_error)
    }

    fn evaluate_function(
        &mut self,
        export_id: boon_plan::ExportId,
        arguments: BTreeMap<boon_plan::DistributedArgumentId, crate::Value>,
    ) -> Result<crate::Value, crate::DistributedRuntimeError> {
        self.runtime
            .evaluate_distributed_function(export_id, arguments)
            .map_err(distributed_machine_error)
    }

    fn replace_distributed_context(
        &mut self,
        session_context: crate::SessionContext,
        imports: Vec<crate::DistributedImportUpdate>,
    ) -> Result<Option<crate::RuntimeTurn>, crate::DistributedRuntimeError> {
        self.runtime
            .replace_distributed_execution_context(session_context, imports)
            .map_err(distributed_machine_error)
    }

    fn prepare_transient_effect_completion(
        &mut self,
        call_id: crate::TransientEffectCallId,
        outcome: crate::Value,
    ) -> Result<crate::RuntimeTurn, crate::DistributedRuntimeError> {
        self.runtime
            .complete_transient_effect_unsettled(call_id, outcome)
            .map_err(distributed_machine_error)
    }

    fn prepare_transient_effect_result(
        &mut self,
        call_id: crate::TransientEffectCallId,
        result_sequence: u64,
        outcome: crate::Value,
    ) -> Result<crate::RuntimeTurn, crate::DistributedRuntimeError> {
        self.runtime
            .deliver_transient_effect_result_unsettled(call_id, result_sequence, outcome)
            .map_err(distributed_machine_error)
    }

    fn prepare_transient_effect_cancellation(
        &mut self,
        call_ids: &[crate::TransientEffectCallId],
    ) -> Result<Option<crate::RuntimeTurn>, crate::DistributedRuntimeError> {
        self.runtime
            .cancel_transient_effects_unsettled(call_ids)
            .map_err(distributed_machine_error)
    }

    fn commit_prepared_turn(
        &mut self,
        turn: crate::RuntimeTurn,
    ) -> Result<crate::RuntimeTurn, crate::DistributedRuntimeError> {
        if let Some(source_sequence) = turn.source_sequence {
            if source_sequence != self.next_source_sequence {
                return Err(distributed_machine_error(
                    "prepared Server source sequence changed before commit",
                ));
            }
            self.next_source_sequence = self
                .next_source_sequence
                .checked_add(1)
                .ok_or_else(|| distributed_machine_error("program source sequence overflow"))?;
        }
        self.runtime.settle_turn();
        Ok(turn)
    }

    fn rollback_prepared_turn(&mut self) -> Result<(), crate::DistributedRuntimeError> {
        self.runtime
            .rollback_unsettled_turn()
            .map_err(distributed_machine_error)
    }

    fn has_pending_transient_effect(&self, call_id: crate::TransientEffectCallId) -> bool {
        self.runtime
            .pending_transient_effect_credits(call_id)
            .is_some()
    }

    fn set_transient_effect_scope(&mut self, scope: u64) {
        self.runtime.set_transient_effect_scope(scope);
    }

    fn root_value_current(
        &mut self,
        name: &str,
    ) -> Result<crate::Value, crate::DistributedRuntimeError> {
        self.runtime
            .root_value_current(name)
            .map_err(distributed_machine_error)
    }
}

fn distributed_machine_error(error: impl fmt::Display) -> crate::DistributedRuntimeError {
    crate::DistributedRuntimeError::Runtime(error.to_string())
}

#[derive(Debug, PartialEq)]
pub struct ProgramSessionDispatch {
    pub source_sequence: u64,
    pub source_path: String,
    pub runtime_turn: crate::RuntimeTurn,
}

/// One trusted program session whose authoritative state is owned by the
/// native persistence coordinator.
///
/// Source sequencing stays host-local and is deliberately absent from the
/// durable image. Authority turns use the runtime's independent contiguous
/// turn sequence.
#[cfg(not(target_arch = "wasm32"))]
pub struct PersistentProgramSession {
    id: ProgramSessionId,
    artifact: ProgramArtifact,
    runtime: crate::PersistentRuntime,
    next_source_sequence: u64,
}

#[cfg(not(target_arch = "wasm32"))]
impl PersistentProgramSession {
    pub fn start<D>(
        artifact: ProgramArtifact,
        driver: D,
        config: PersistenceWorkerConfig,
    ) -> Result<(Self, crate::PersistentRuntimeStartup), ProgramDiagnostic>
    where
        D: PersistenceDriver + Send + 'static,
    {
        let limits = program_limits(artifact.capability_profile());
        let id = deterministic_program_session_id(&artifact);
        let (runtime, startup) = crate::PersistentRuntime::from_shared_machine_plan(
            Arc::clone(artifact.plan()),
            SessionOptions {
                max_work_units_per_transaction: Some(limits.max_runtime_work_units_per_transaction),
                ..SessionOptions::default()
            },
            driver,
            config,
        )
        .map_err(|error| {
            ProgramDiagnostic::new(
                artifact.revision(),
                ProgramDiagnosticPhase::Start,
                error.to_string(),
            )
        })?;
        Ok((
            Self {
                id,
                artifact,
                runtime,
                next_source_sequence: 1,
            },
            startup,
        ))
    }

    pub fn id(&self) -> &ProgramSessionId {
        &self.id
    }

    pub fn artifact(&self) -> &ProgramArtifact {
        &self.artifact
    }

    pub fn runtime(&self) -> &crate::PersistentRuntime {
        &self.runtime
    }

    pub fn next_source_sequence(&self) -> u64 {
        self.next_source_sequence
    }

    pub fn dispatch(
        &mut self,
        source_path: &str,
        target: Option<RowId>,
        payload: SourcePayload,
    ) -> Result<ProgramSessionDispatch, crate::PersistentDispatchError> {
        let (source_sequence, next_source_sequence, event) =
            self.prepare_dispatch(source_path, target, payload)?;
        let runtime_turn = self.runtime.dispatch(event)?;
        self.next_source_sequence = next_source_sequence;
        Ok(ProgramSessionDispatch {
            source_sequence,
            source_path: source_path.to_owned(),
            runtime_turn,
        })
    }

    pub fn dispatch_durably(
        &mut self,
        source_path: &str,
        target: Option<RowId>,
        payload: SourcePayload,
    ) -> Result<(ProgramSessionDispatch, CommitAck), crate::PersistentDispatchError> {
        let (source_sequence, next_source_sequence, event) =
            self.prepare_dispatch(source_path, target, payload)?;
        let acknowledged = self.runtime.dispatch_durably(event)?;
        self.next_source_sequence = next_source_sequence;
        Ok((
            ProgramSessionDispatch {
                source_sequence,
                source_path: source_path.to_owned(),
                runtime_turn: acknowledged.turn,
            },
            acknowledged.acknowledgement,
        ))
    }

    pub fn dispatch_prepared_durably(
        &mut self,
        event: crate::SourceEvent,
    ) -> Result<crate::DurablyAcknowledgedTurn, crate::PersistentDispatchError> {
        if event.sequence != self.next_source_sequence {
            return Err(crate::PersistentDispatchError::Runtime(format!(
                "prepared source sequence {} does not match next sequence {}",
                event.sequence, self.next_source_sequence
            )));
        }
        let next_source_sequence = self.next_source_sequence.checked_add(1).ok_or_else(|| {
            crate::PersistentDispatchError::Runtime("program source sequence overflow".to_owned())
        })?;
        let acknowledged = self.runtime.dispatch_durably(event)?;
        self.next_source_sequence = next_source_sequence;
        Ok(acknowledged)
    }

    pub fn prepare_distributed_dispatch(
        &mut self,
        event: crate::SourceEvent,
        immediate: bool,
    ) -> Result<crate::RuntimeTurn, crate::PersistentDispatchError> {
        if event.sequence != self.next_source_sequence {
            return Err(crate::PersistentDispatchError::Runtime(format!(
                "prepared source sequence {} does not match next sequence {}",
                event.sequence, self.next_source_sequence
            )));
        }
        self.next_source_sequence.checked_add(1).ok_or_else(|| {
            crate::PersistentDispatchError::Runtime("program source sequence overflow".to_owned())
        })?;
        self.runtime.prepare_distributed_dispatch(event, immediate)
    }

    pub fn prepare_distributed_effect_completion(
        &mut self,
        call_id: crate::TransientEffectCallId,
        outcome: crate::Value,
        immediate: bool,
    ) -> Result<crate::RuntimeTurn, crate::PersistentDispatchError> {
        self.runtime
            .prepare_distributed_effect_completion(call_id, outcome, immediate)
    }

    pub fn prepare_distributed_effect_result(
        &mut self,
        call_id: crate::TransientEffectCallId,
        result_sequence: u64,
        outcome: crate::Value,
        immediate: bool,
    ) -> Result<crate::RuntimeTurn, crate::PersistentDispatchError> {
        self.runtime
            .prepare_distributed_effect_result(call_id, result_sequence, outcome, immediate)
    }

    pub fn prepare_distributed_effect_cancellation(
        &mut self,
        call_ids: &[crate::TransientEffectCallId],
        immediate: bool,
    ) -> Result<Option<crate::RuntimeTurn>, crate::PersistentDispatchError> {
        self.runtime
            .prepare_distributed_effect_cancellation(call_ids, immediate)
    }

    pub fn commit_prepared_distributed_turn(
        &mut self,
        turn: crate::RuntimeTurn,
    ) -> Result<(crate::RuntimeTurn, Option<CommitAck>), crate::PersistentDispatchError> {
        self.commit_prepared_distributed_turn_with_protocol_state(turn, Vec::new())
    }

    pub fn commit_prepared_distributed_turn_with_protocol_state(
        &mut self,
        turn: crate::RuntimeTurn,
        protocol_state_changes: Vec<boon_persistence::DurableProtocolStateChange>,
    ) -> Result<(crate::RuntimeTurn, Option<CommitAck>), crate::PersistentDispatchError> {
        let source_sequence = turn.source_sequence;
        if let Some(source_sequence) = source_sequence {
            if source_sequence != self.next_source_sequence {
                return Err(crate::PersistentDispatchError::Runtime(
                    "prepared Server source sequence changed before commit".to_owned(),
                ));
            }
            self.next_source_sequence.checked_add(1).ok_or_else(|| {
                crate::PersistentDispatchError::Runtime(
                    "program source sequence overflow".to_owned(),
                )
            })?;
        }
        let committed = self
            .runtime
            .commit_prepared_distributed_turn(turn, protocol_state_changes)?;
        if source_sequence.is_some() {
            self.next_source_sequence += 1;
        }
        Ok(committed)
    }

    pub fn rollback_prepared_distributed_turn(
        &mut self,
    ) -> Result<(), crate::PersistentDispatchError> {
        self.runtime.rollback_prepared_distributed_turn()
    }

    pub fn fork_prepared_distributed_server_evaluation(
        &self,
        turn: Option<&crate::RuntimeTurn>,
    ) -> Result<ProgramSession, crate::DistributedRuntimeError> {
        let next_source_sequence = self.evaluation_next_source_sequence(turn)?;
        let runtime = self
            .runtime
            .runtime()
            .fork_distributed_server_evaluation(turn.is_some())
            .map_err(distributed_machine_error)?;
        Ok(ProgramSession {
            id: self.id.clone(),
            artifact: self.artifact.clone(),
            runtime,
            next_source_sequence,
        })
    }

    pub fn install_distributed_server_evaluation(
        &mut self,
        evaluation: ProgramSession,
    ) -> Result<(), crate::PersistentDispatchError> {
        self.validate_distributed_server_evaluation(None, &evaluation)
            .map_err(|error| crate::PersistentDispatchError::Runtime(error.to_string()))?;
        self.runtime
            .validate_distributed_server_evaluation(&evaluation.runtime, false)?;
        self.runtime
            .install_distributed_server_evaluation(evaluation.runtime);
        self.next_source_sequence = evaluation.next_source_sequence;
        Ok(())
    }

    pub fn prepare_protocol_checkpoint(
        &mut self,
    ) -> Result<crate::RuntimeTurn, crate::PersistentDispatchError> {
        self.runtime.prepare_distributed_protocol_checkpoint()
    }

    pub fn commit_prepared_distributed_server_evaluation(
        &mut self,
        turn: crate::RuntimeTurn,
        evaluation: ProgramSession,
        protocol_state_changes: Vec<boon_persistence::DurableProtocolStateChange>,
    ) -> Result<(crate::RuntimeTurn, Option<CommitAck>), crate::PersistentDispatchError> {
        self.validate_distributed_server_evaluation(Some(&turn), &evaluation)
            .map_err(|error| crate::PersistentDispatchError::Runtime(error.to_string()))?;
        self.runtime
            .validate_distributed_server_evaluation(&evaluation.runtime, true)?;
        let committed = self
            .runtime
            .commit_prepared_distributed_turn(turn, protocol_state_changes)?;
        self.runtime
            .install_distributed_server_evaluation(evaluation.runtime);
        self.next_source_sequence = evaluation.next_source_sequence;
        Ok(committed)
    }

    fn evaluation_next_source_sequence(
        &self,
        turn: Option<&crate::RuntimeTurn>,
    ) -> Result<u64, crate::DistributedRuntimeError> {
        let Some(source_sequence) = turn.and_then(|turn| turn.source_sequence) else {
            return Ok(self.next_source_sequence);
        };
        if source_sequence != self.next_source_sequence {
            return Err(distributed_machine_error(
                "prepared persistent Server source sequence changed before evaluation",
            ));
        }
        source_sequence
            .checked_add(1)
            .ok_or_else(|| distributed_machine_error("program source sequence overflow"))
    }

    fn validate_distributed_server_evaluation(
        &self,
        turn: Option<&crate::RuntimeTurn>,
        evaluation: &ProgramSession,
    ) -> Result<(), crate::DistributedRuntimeError> {
        if self.runtime.runtime().has_unsettled_turn() != turn.is_some() {
            return Err(distributed_machine_error(
                "persistent distributed Server authority preparation state changed before commit",
            ));
        }
        if evaluation.runtime.has_unsettled_turn() {
            return Err(distributed_machine_error(
                "distributed Server evaluation remained unsettled",
            ));
        }
        if self.id != evaluation.id
            || self.artifact.id() != evaluation.artifact.id()
            || self.artifact.plan_digest() != evaluation.artifact.plan_digest()
        {
            return Err(distributed_machine_error(
                "distributed Server evaluation belongs to another persistent authority",
            ));
        }
        if evaluation.next_source_sequence != self.evaluation_next_source_sequence(turn)? {
            return Err(distributed_machine_error(
                "persistent distributed Server evaluation source sequence is invalid",
            ));
        }
        Ok(())
    }

    fn prepare_dispatch(
        &self,
        source_path: &str,
        target: Option<RowId>,
        payload: SourcePayload,
    ) -> Result<(u64, u64, crate::SourceEvent), crate::PersistentDispatchError> {
        let source_sequence = self.next_source_sequence;
        let next_source_sequence = source_sequence.checked_add(1).ok_or_else(|| {
            crate::PersistentDispatchError::Runtime("program source sequence overflow".to_owned())
        })?;
        let event = self
            .runtime
            .runtime()
            .source_event(source_sequence, source_path, target, payload)
            .map_err(|error| crate::PersistentDispatchError::Runtime(error.to_string()))?;
        Ok((source_sequence, next_source_sequence, event))
    }

    pub fn root_value_current(
        &mut self,
        name: &str,
    ) -> Result<crate::Value, crate::PersistentDispatchError> {
        self.runtime.root_value_current(name)
    }

    pub fn output_value_current(
        &mut self,
        name: &str,
    ) -> Result<crate::Value, crate::PersistentDispatchError> {
        self.runtime.output_value_current(name)
    }

    pub fn complete_transient_effect(
        &mut self,
        call_id: crate::TransientEffectCallId,
        outcome: crate::Value,
    ) -> Result<crate::RuntimeTurn, crate::PersistentDispatchError> {
        self.runtime.complete_transient_effect(call_id, outcome)
    }

    pub fn deliver_transient_effect_result(
        &mut self,
        call_id: crate::TransientEffectCallId,
        result_sequence: u64,
        outcome: crate::Value,
    ) -> Result<crate::RuntimeTurn, crate::PersistentDispatchError> {
        self.runtime
            .deliver_transient_effect_result(call_id, result_sequence, outcome)
    }

    pub fn deliver_transient_effect_result_durably(
        &mut self,
        call_id: crate::TransientEffectCallId,
        result_sequence: u64,
        outcome: crate::Value,
    ) -> Result<crate::DurablyAcknowledgedTurn, crate::PersistentDispatchError> {
        self.runtime
            .deliver_transient_effect_result_durably(call_id, result_sequence, outcome)
    }

    pub fn complete_transient_effect_durably(
        &mut self,
        call_id: crate::TransientEffectCallId,
        outcome: crate::Value,
    ) -> Result<crate::DurablyAcknowledgedTurn, crate::PersistentDispatchError> {
        self.runtime
            .complete_transient_effect_durably(call_id, outcome)
    }

    pub fn cancel_transient_effect(
        &mut self,
        call_id: crate::TransientEffectCallId,
    ) -> Result<bool, crate::PersistentDispatchError> {
        self.runtime.cancel_transient_effect(call_id)
    }

    pub fn pending_transient_effect_count(&self) -> usize {
        self.runtime.pending_transient_effect_count()
    }

    pub fn pending_transient_effect_credits(
        &self,
        call_id: crate::TransientEffectCallId,
    ) -> Option<u32> {
        self.runtime.pending_transient_effect_credits(call_id)
    }

    pub fn persistence_status(&self) -> PersistenceWorkerStatus {
        self.runtime.status()
    }

    pub fn barrier(&self) -> Result<BarrierAck, PersistenceControlError> {
        self.runtime.barrier()
    }

    pub fn shutdown(&self) -> Result<ShutdownAck, PersistenceControlError> {
        self.runtime.shutdown()
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl crate::DistributedServerMachine for PersistentProgramSession {
    type EvaluationMachine = ProgramSession;

    fn artifact(&self) -> &ProgramArtifact {
        &self.artifact
    }

    fn fork_prepared_evaluation(
        &self,
        turn: Option<&crate::RuntimeTurn>,
    ) -> Result<Self::EvaluationMachine, crate::DistributedRuntimeError> {
        self.fork_prepared_distributed_server_evaluation(turn)
    }

    fn install_evaluation(
        &mut self,
        evaluation: Self::EvaluationMachine,
    ) -> Result<(), crate::DistributedRuntimeError> {
        self.install_distributed_server_evaluation(evaluation)
            .map_err(distributed_machine_error)
    }

    fn commit_prepared_evaluation(
        &mut self,
        turn: crate::RuntimeTurn,
        evaluation: Self::EvaluationMachine,
    ) -> Result<crate::RuntimeTurn, crate::DistributedRuntimeError> {
        self.commit_prepared_distributed_server_evaluation(turn, evaluation, Vec::new())
            .map(|(turn, _)| turn)
            .map_err(distributed_machine_error)
    }

    fn commit_prepared_evaluation_with_protocol_state(
        &mut self,
        turn: crate::RuntimeTurn,
        evaluation: Self::EvaluationMachine,
        protocol_state_changes: Vec<boon_persistence::DurableProtocolStateChange>,
    ) -> Result<crate::RuntimeTurn, crate::DistributedRuntimeError> {
        self.commit_prepared_distributed_server_evaluation(turn, evaluation, protocol_state_changes)
            .map(|(turn, _)| turn)
            .map_err(distributed_machine_error)
    }

    fn event_for_path(
        &self,
        path: &str,
        payload: SourcePayload,
    ) -> Result<crate::SourceEvent, crate::DistributedRuntimeError> {
        self.runtime
            .runtime()
            .source_event(self.next_source_sequence, path, None, payload)
            .map_err(distributed_machine_error)
    }

    fn event_for_source(
        &self,
        source: boon_plan::SourceId,
        payload: SourcePayload,
    ) -> Result<crate::SourceEvent, crate::DistributedRuntimeError> {
        self.runtime
            .runtime()
            .source_event_by_id(self.next_source_sequence, source, None, payload)
            .map_err(distributed_machine_error)
    }

    fn prepare_dispatch(
        &mut self,
        event: crate::SourceEvent,
    ) -> Result<crate::RuntimeTurn, crate::DistributedRuntimeError> {
        self.prepare_distributed_dispatch(event, false)
            .map_err(distributed_machine_error)
    }

    fn export_current(
        &mut self,
        export_id: boon_plan::ExportId,
    ) -> Result<crate::Value, crate::DistributedRuntimeError> {
        self.runtime
            .distributed_export_value_current(export_id)
            .map_err(distributed_machine_error)
    }

    fn call_arguments(
        &mut self,
        call: &boon_plan::RemoteCallSitePlan,
    ) -> Result<
        BTreeMap<boon_plan::DistributedArgumentId, crate::Value>,
        crate::DistributedRuntimeError,
    > {
        self.runtime
            .distributed_call_arguments_current(call.call_site_id)
            .map_err(distributed_machine_error)
    }

    fn evaluate_function(
        &mut self,
        export_id: boon_plan::ExportId,
        arguments: BTreeMap<boon_plan::DistributedArgumentId, crate::Value>,
    ) -> Result<crate::Value, crate::DistributedRuntimeError> {
        self.runtime
            .evaluate_distributed_function(export_id, arguments)
            .map_err(distributed_machine_error)
    }

    fn replace_distributed_context(
        &mut self,
        session_context: crate::SessionContext,
        imports: Vec<crate::DistributedImportUpdate>,
    ) -> Result<Option<crate::RuntimeTurn>, crate::DistributedRuntimeError> {
        self.runtime
            .replace_distributed_context(session_context, imports)
            .map_err(distributed_machine_error)
    }

    fn prepare_transient_effect_completion(
        &mut self,
        call_id: crate::TransientEffectCallId,
        outcome: crate::Value,
    ) -> Result<crate::RuntimeTurn, crate::DistributedRuntimeError> {
        self.prepare_distributed_effect_completion(call_id, outcome, false)
            .map_err(distributed_machine_error)
    }

    fn prepare_transient_effect_result(
        &mut self,
        call_id: crate::TransientEffectCallId,
        result_sequence: u64,
        outcome: crate::Value,
    ) -> Result<crate::RuntimeTurn, crate::DistributedRuntimeError> {
        self.prepare_distributed_effect_result(call_id, result_sequence, outcome, false)
            .map_err(distributed_machine_error)
    }

    fn prepare_transient_effect_cancellation(
        &mut self,
        call_ids: &[crate::TransientEffectCallId],
    ) -> Result<Option<crate::RuntimeTurn>, crate::DistributedRuntimeError> {
        self.prepare_distributed_effect_cancellation(call_ids, false)
            .map_err(distributed_machine_error)
    }

    fn commit_prepared_turn(
        &mut self,
        turn: crate::RuntimeTurn,
    ) -> Result<crate::RuntimeTurn, crate::DistributedRuntimeError> {
        self.commit_prepared_distributed_turn(turn)
            .map(|(turn, _)| turn)
            .map_err(distributed_machine_error)
    }

    fn commit_prepared_turn_with_protocol_state(
        &mut self,
        turn: crate::RuntimeTurn,
        protocol_state_changes: Vec<boon_persistence::DurableProtocolStateChange>,
    ) -> Result<crate::RuntimeTurn, crate::DistributedRuntimeError> {
        self.commit_prepared_distributed_turn_with_protocol_state(turn, protocol_state_changes)
            .map(|(turn, _)| turn)
            .map_err(distributed_machine_error)
    }

    fn prepare_protocol_checkpoint(
        &mut self,
    ) -> Result<crate::RuntimeTurn, crate::DistributedRuntimeError> {
        PersistentProgramSession::prepare_protocol_checkpoint(self)
            .map_err(distributed_machine_error)
    }

    fn supports_protocol_state(&self) -> bool {
        true
    }

    fn rollback_prepared_turn(&mut self) -> Result<(), crate::DistributedRuntimeError> {
        self.rollback_prepared_distributed_turn()
            .map_err(distributed_machine_error)
    }

    fn has_pending_transient_effect(&self, call_id: crate::TransientEffectCallId) -> bool {
        self.runtime
            .pending_transient_effect_credits(call_id)
            .is_some()
    }

    fn set_transient_effect_scope(&mut self, scope: u64) {
        self.runtime.set_transient_effect_scope(scope);
    }

    fn root_value_current(
        &mut self,
        name: &str,
    ) -> Result<crate::Value, crate::DistributedRuntimeError> {
        self.runtime
            .root_value_current(name)
            .map_err(distributed_machine_error)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DistributedProgramBundle {
    artifacts: Vec<ProgramArtifact>,
}

impl DistributedProgramBundle {
    pub fn new(mut artifacts: Vec<ProgramArtifact>) -> crate::RuntimeResult<Self> {
        if artifacts.len() != 3 {
            return Err(
                "distributed program requires exactly one client, one session, and one server artifact"
                    .into(),
            );
        }
        let package_id = artifacts[0].application().package_id.clone();
        let deployment_domain = artifacts[0].application().deployment_domain.clone();
        let mut roles = BTreeSet::new();
        let mut namespaces = BTreeSet::new();
        for artifact in &artifacts {
            if !roles.insert(artifact.role().as_str()) {
                return Err(
                    format!("program bundle repeats role `{}`", artifact.role().as_str()).into(),
                );
            }
            let application = artifact.application();
            if application.package_id != package_id
                || application.deployment_domain != deployment_domain
            {
                return Err(format!(
                    "{} program is outside bundle application `{package_id}` in deployment `{deployment_domain}`",
                    artifact.role().as_str()
                )
                .into());
            }
            if !namespaces.insert(application.state_namespace.clone()) {
                return Err(format!(
                    "program bundle repeats state namespace `{}`",
                    application.state_namespace
                )
                .into());
            }
        }
        let required_roles = BTreeSet::from([
            ProgramRole::Client.as_str(),
            ProgramRole::Session.as_str(),
            ProgramRole::Server.as_str(),
        ]);
        if roles != required_roles {
            return Err(
                "distributed program requires client, session, and server artifacts".into(),
            );
        }
        let graph_identity = artifacts[0]
            .plan()
            .distributed_endpoint
            .as_ref()
            .ok_or("distributed artifact has no linked endpoint contract")?
            .graph
            .clone();
        let mut endpoint_contracts = Vec::with_capacity(artifacts.len());
        for artifact in &artifacts {
            let endpoint = artifact
                .plan()
                .distributed_endpoint
                .as_ref()
                .ok_or_else(|| {
                    format!(
                        "{} artifact has no linked distributed endpoint",
                        artifact.role().as_str()
                    )
                })?;
            if endpoint.graph != graph_identity || endpoint.endpoint.role != artifact.role() {
                return Err(format!(
                    "{} artifact does not match the bundle distributed graph",
                    artifact.role().as_str()
                )
                .into());
            }
            endpoint_contracts.push(endpoint.endpoint.clone());
        }
        boon_plan::DistributedGraphPlan::new(
            artifacts[0].application(),
            graph_identity,
            endpoint_contracts,
        )?;
        artifacts.sort_by_key(|artifact| program_role_rank(artifact.role()));
        Ok(Self { artifacts })
    }

    pub fn artifacts(&self) -> &[ProgramArtifact] {
        &self.artifacts
    }

    pub fn artifact(&self, role: ProgramRole) -> Option<&ProgramArtifact> {
        self.artifacts
            .iter()
            .find(|artifact| artifact.role() == role)
    }
}

fn program_role_rank(role: ProgramRole) -> u8 {
    match role {
        ProgramRole::Client => 0,
        ProgramRole::Session => 1,
        ProgramRole::Server => 2,
    }
}

fn deterministic_program_session_id(artifact: &ProgramArtifact) -> ProgramSessionId {
    let application = artifact.application();
    let mut hasher = Sha256::new();
    hasher.update(b"boon.program-session.v1");
    for component in [
        application.package_id.as_str(),
        application.state_namespace.as_str(),
        application.deployment_domain.as_str(),
        artifact.role().as_str(),
    ] {
        hasher.update((component.len() as u64).to_be_bytes());
        hasher.update(component.as_bytes());
    }
    ProgramSessionId(format!("program-session:{:x}", hasher.finalize()))
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProgramCompletion {
    Activated {
        revision: u64,
    },
    Rejected {
        diagnostic: ProgramDiagnostic,
    },
    Stale {
        revision: u64,
        latest_requested_revision: u64,
    },
}

pub struct ProgramController {
    capability_profile: ProgramCapabilityProfile,
    latest_requested_revision: u64,
    active: Option<ProgramSession>,
    diagnostic: Option<ProgramDiagnostic>,
}

impl ProgramController {
    pub fn new(capability_profile: ProgramCapabilityProfile) -> Self {
        Self {
            capability_profile,
            latest_requested_revision: 0,
            active: None,
            diagnostic: None,
        }
    }

    pub fn request(&mut self, revision: u64) -> Result<(), ProgramDiagnostic> {
        if revision <= self.latest_requested_revision {
            return Err(ProgramDiagnostic::new(
                revision,
                ProgramDiagnosticPhase::Request,
                format!(
                    "revision must increase beyond {}",
                    self.latest_requested_revision
                ),
            ));
        }
        self.latest_requested_revision = revision;
        Ok(())
    }

    pub fn complete(
        &mut self,
        result: Result<ProgramArtifact, ProgramDiagnostic>,
    ) -> ProgramCompletion {
        let revision = match &result {
            Ok(artifact) => artifact.revision(),
            Err(diagnostic) => diagnostic.revision,
        };
        if revision != self.latest_requested_revision {
            return ProgramCompletion::Stale {
                revision,
                latest_requested_revision: self.latest_requested_revision,
            };
        }
        match result {
            Ok(artifact) if artifact.capability_profile() != self.capability_profile => self
                .reject(ProgramDiagnostic::new(
                    revision,
                    ProgramDiagnosticPhase::Capability,
                    format!(
                        "artifact profile `{}` does not match controller profile `{}`",
                        artifact.capability_profile().name(),
                        self.capability_profile.name()
                    ),
                )),
            Ok(artifact) => match ProgramSession::start(artifact) {
                Ok(session) => {
                    self.active = Some(session);
                    self.diagnostic = None;
                    ProgramCompletion::Activated { revision }
                }
                Err(diagnostic) => self.reject(diagnostic),
            },
            Err(diagnostic) => self.reject(diagnostic),
        }
    }

    pub fn latest_requested_revision(&self) -> u64 {
        self.latest_requested_revision
    }

    pub fn active(&self) -> Option<&ProgramSession> {
        self.active.as_ref()
    }

    pub fn active_mut(&mut self) -> Option<&mut ProgramSession> {
        self.active.as_mut()
    }

    pub fn diagnostic(&self) -> Option<&ProgramDiagnostic> {
        self.diagnostic.as_ref()
    }

    fn reject(&mut self, diagnostic: ProgramDiagnostic) -> ProgramCompletion {
        self.diagnostic = Some(diagnostic.clone());
        ProgramCompletion::Rejected { diagnostic }
    }
}

fn validate_request(request: &ProgramCompileRequest) -> Result<(), ProgramDiagnostic> {
    let limits = program_limits(request.capability_profile);
    validate_request_with_source_limits(request, limits.max_source_units, limits.max_source_bytes)
}

fn validate_request_with_source_limits(
    request: &ProgramCompileRequest,
    max_source_units: usize,
    max_source_bytes: usize,
) -> Result<(), ProgramDiagnostic> {
    let required_profile = match request.role {
        ProgramRole::Client => ProgramCapabilityProfile::PublicClient,
        ProgramRole::Session => ProgramCapabilityProfile::TrustedSession,
        ProgramRole::Server => ProgramCapabilityProfile::TrustedServer,
    };
    if request.capability_profile != required_profile {
        return Err(ProgramDiagnostic::new(
            request.revision,
            ProgramDiagnosticPhase::Request,
            format!(
                "{} programs require capability profile `{}`, found `{}`",
                request.role.as_str(),
                required_profile.name(),
                request.capability_profile.name()
            ),
        ));
    }
    if request.revision == 0 {
        return Err(ProgramDiagnostic::new(
            request.revision,
            ProgramDiagnosticPhase::Request,
            "revision zero is reserved for an uninitialized program",
        ));
    }
    if request.units.is_empty() || request.units.len() > max_source_units {
        return Err(ProgramDiagnostic::new(
            request.revision,
            ProgramDiagnosticPhase::Request,
            format!(
                "source unit count {} is outside 1..={}",
                request.units.len(),
                max_source_units
            ),
        ));
    }
    let source_bytes = request
        .units
        .iter()
        .map(|unit| unit.path.len().saturating_add(unit.source.len()))
        .sum::<usize>();
    if source_bytes > max_source_bytes {
        return Err(ProgramDiagnostic::new(
            request.revision,
            ProgramDiagnosticPhase::Request,
            format!(
                "source bundle uses {source_bytes} bytes, limit is {}",
                max_source_bytes
            ),
        ));
    }
    let mut paths = BTreeSet::new();
    for unit in &request.units {
        if unit.path.trim().is_empty()
            || unit.path.trim() != unit.path
            || unit.path.starts_with('/')
            || unit.path.split('/').any(|part| part == "..")
        {
            return Err(ProgramDiagnostic::new(
                request.revision,
                ProgramDiagnosticPhase::Request,
                format!(
                    "source unit path `{}` is not a relative canonical path",
                    unit.path
                ),
            ));
        }
        if !paths.insert(unit.path.as_str()) {
            return Err(ProgramDiagnostic::new(
                request.revision,
                ProgramDiagnosticPhase::Request,
                format!("source unit path `{}` is duplicated", unit.path),
            ));
        }
    }
    if !request
        .units
        .iter()
        .any(|unit| unit.path == request.entry_path)
    {
        return Err(ProgramDiagnostic::new(
            request.revision,
            ProgramDiagnosticPhase::Request,
            format!(
                "entry source path `{}` is not present in the source units",
                request.entry_path
            ),
        ));
    }
    if !request.application.is_valid() {
        return Err(ProgramDiagnostic::new(
            request.revision,
            ProgramDiagnosticPhase::Request,
            "application identity is invalid",
        ));
    }
    Ok(())
}

fn validate_plan(
    revision: u64,
    profile: ProgramCapabilityProfile,
    plan: &MachinePlan,
) -> Result<(), ProgramDiagnostic> {
    let limits = program_limits(profile);
    let capabilities = &plan.capability_summary;
    let document = plan.document_plan();
    let retained_outputs = plan
        .outputs
        .iter()
        .filter(|output| {
            matches!(
                output.contract,
                OutputContractKind::Document | OutputContractKind::Scene
            )
        })
        .count();
    let mut failures = Vec::new();
    let expected_role = match profile {
        ProgramCapabilityProfile::PublicClient => ProgramRole::Client,
        ProgramCapabilityProfile::TrustedSession => ProgramRole::Session,
        ProgramCapabilityProfile::TrustedServer => ProgramRole::Server,
    };
    if plan.program_role != expected_role {
        failures.push(format!(
            "profile `{}` requires ProgramRole::{}, compiled plan declares ProgramRole::{}",
            profile.name(),
            title_case_role(expected_role),
            title_case_role(plan.program_role)
        ));
    }
    if plan.target_profile != TargetProfile::SoftwareBounded {
        failures.push(format!(
            "profile `{}` requires target profile `software_bounded`, found `{}`",
            profile.name(),
            plan.target_profile.as_str()
        ));
    }
    if !plan.application.identity.is_valid() {
        failures.push("plan application identity is invalid".to_owned());
    }
    if !capabilities.typed_lowering_executable
        || !capabilities.executable
        || !capabilities.cpu_plan_executor_complete
    {
        let unsupported = boon_plan::cpu_plan_executor_unsupported_ops(plan)
            .into_iter()
            .take(16)
            .map(|op| format!("{}:{:?}", op.id.0, op.kind))
            .collect::<Vec<_>>()
            .join(", ");
        failures.push(format!(
            "plan is not executable for profile `{}` (unresolved refs {}, unknown ops {}, unsupported CPU ops {} [{}])",
            profile.name(),
            capabilities.unresolved_executable_ref_count,
            capabilities.unknown_plan_op_count,
            capabilities.cpu_plan_executor_unsupported_op_count,
            unsupported,
        ));
    }
    match profile {
        ProgramCapabilityProfile::PublicClient => {
            if document.is_none() {
                failures.push("program has no retained document or scene output".to_owned());
            }
            if retained_outputs != 1 {
                failures.push(format!(
                    "program must expose exactly one retained visual output, found {retained_outputs}"
                ));
            }
            let denied_effects = plan
                .effects
                .iter()
                .filter(|effect| {
                    !matches!(
                        effect.replay,
                        EffectReplay::ReadOnly | EffectReplay::ProcessScoped
                    ) || effect.barrier != EffectBarrier::None
                        || effect.schema.is_none()
                })
                .count();
            if denied_effects > 0 {
                failures.push(format!(
                    "profile `{}` forbids {denied_effects} host effect contract(s) that are not typed process-local operations without persistence barriers",
                    profile.name(),
                ));
            }
            if document.is_some_and(|document| {
                document.templates.iter().any(|template| {
                    matches!(
                        template.constructor,
                        DocumentConstructor::ElementProgram
                            | DocumentConstructor::SceneElementProgram
                    )
                })
            }) {
                failures.push(format!(
                    "profile `{}` forbids nested program hosts",
                    profile.name()
                ));
            }
        }
        ProgramCapabilityProfile::TrustedSession | ProgramCapabilityProfile::TrustedServer => {
            if document.is_some() || retained_outputs != 0 {
                failures.push(format!(
                    "{} program must not contain a retained visual output",
                    profile.name()
                ));
            }
            if plan
                .outputs
                .iter()
                .any(|output| !matches!(output.contract, OutputContractKind::HostValue { .. }))
            {
                failures.push(format!(
                    "{} program outputs must be typed host values",
                    profile.name()
                ));
            }
            if profile == ProgramCapabilityProfile::TrustedSession && !plan.host_ports.is_empty() {
                failures.push(
                    "trusted session programs cannot own process-level host ports".to_owned(),
                );
            }
        }
    }
    check_limit(
        &mut failures,
        "operations",
        capabilities.operation_count,
        limits.max_operations,
    );
    check_limit(
        &mut failures,
        "scalar slots",
        plan.storage_layout.scalar_slots.len(),
        limits.max_scalar_slots,
    );
    check_limit(
        &mut failures,
        "list slots",
        plan.storage_layout.list_slots.len(),
        limits.max_list_slots,
    );
    check_limit(
        &mut failures,
        "source routes",
        plan.source_routes.len(),
        limits.max_source_routes,
    );
    if matches!(
        profile,
        ProgramCapabilityProfile::TrustedSession | ProgramCapabilityProfile::TrustedServer
    ) {
        check_limit(
            &mut failures,
            "output roots",
            plan.outputs.len(),
            limits.max_output_roots,
        );
    }
    check_limit(
        &mut failures,
        "effect contracts",
        plan.effects.len(),
        limits.max_effect_contracts,
    );
    check_limit(
        &mut failures,
        "document expressions",
        document.map_or(0, |document| document.expressions.len()),
        limits.max_document_expressions,
    );
    check_limit(
        &mut failures,
        "document templates",
        document.map_or(0, |document| document.templates.len()),
        limits.max_document_templates,
    );
    check_limit(
        &mut failures,
        "document materializations",
        document.map_or(0, |document| document.materializations.len()),
        limits.max_document_materializations,
    );
    if let Some(capacity) = plan
        .storage_layout
        .list_slots
        .iter()
        .filter_map(|slot| slot.capacity)
        .find(|capacity| *capacity > limits.max_declared_list_capacity)
    {
        failures.push(format!(
            "declared list capacity {capacity} exceeds limit {}",
            limits.max_declared_list_capacity
        ));
    }
    match boon_plan::verify_plan(plan) {
        Ok(verification) => {
            let failed = verification
                .checks
                .iter()
                .filter(|check| !check.pass)
                .map(|check| check.id.as_str())
                .collect::<Vec<_>>();
            if verification.status != "pass" || verification.error_count != 0 || !failed.is_empty()
            {
                failures.push(format!("plan verification failed: {}", failed.join(", ")));
            }
        }
        Err(error) => failures.push(format!("plan verification failed: {error}")),
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(ProgramDiagnostic::new(
            revision,
            ProgramDiagnosticPhase::Capability,
            failures.join("; "),
        ))
    }
}

fn title_case_role(role: ProgramRole) -> &'static str {
    match role {
        ProgramRole::Client => "Client",
        ProgramRole::Session => "Session",
        ProgramRole::Server => "Server",
    }
}

fn check_limit(failures: &mut Vec<String>, label: &str, actual: usize, limit: usize) {
    if actual > limit {
        failures.push(format!("{label} count {actual} exceeds limit {limit}"));
    }
}

fn bounded_diagnostic(mut message: String) -> String {
    if message.len() <= MAX_DIAGNOSTIC_BYTES {
        return message;
    }
    let mut end = MAX_DIAGNOSTIC_BYTES;
    while !message.is_char_boundary(end) {
        end -= 1;
    }
    message.truncate(end);
    message.push_str("...");
    message
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProgramRequestId(pub String);

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct ProgramSessionId(pub String);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProgramArtifactOwnership {
    pub owner: ContentArtifactOwnerId,
    pub retention: ContentArtifactRetention,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProgramHostRequest {
    pub request_id: ProgramRequestId,
    pub session: ProgramSessionId,
    pub host: DocumentNodeId,
    pub compile: ProgramCompileRequest,
    pub artifact_id: Option<ContentArtifactId>,
    pub artifact_ownership: Option<ProgramArtifactOwnership>,
}

impl ProgramHostRequest {
    pub const fn is_artifact_load(&self) -> bool {
        self.artifact_id.is_some()
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ProgramHostUpdate {
    pub patches: Vec<DocumentPatch>,
    pub requests: Vec<ProgramHostRequest>,
    pub rejections: Vec<ProgramRejection>,
    pub bootstrap: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProgramRejection {
    pub session: ProgramSessionId,
    pub diagnostic: ProgramDiagnostic,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProgramHostDiagnostic {
    pub session: ProgramSessionId,
    pub hosts: Vec<DocumentNodeId>,
    pub diagnostic: ProgramDiagnostic,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProgramHostCompletion {
    Program(ProgramCompletion),
    Superseded {
        session: ProgramSessionId,
        request_id: ProgramRequestId,
    },
    Removed {
        session: ProgramSessionId,
    },
}

#[derive(Clone)]
struct ProgramSourceRoute {
    session: ProgramSessionId,
    source_path: String,
}

#[derive(Clone)]
struct ProgramMaterializationRoute {
    session: ProgramSessionId,
    materialization: u64,
}

#[derive(Clone)]
struct ProgramProjection {
    session: ProgramSessionId,
    descriptor: EmbeddedProgramDescriptor,
    mount: bool,
    parent_children: Vec<DocumentNodeId>,
    projected: Option<ProjectedProgram>,
}

#[derive(Clone)]
struct ProjectedProgram {
    frame: DocumentFrame,
    source_routes: BTreeMap<String, ProgramSourceRoute>,
    materialization_routes: BTreeMap<u64, ProgramMaterializationRoute>,
}

struct HostedProgram {
    descriptor: EmbeddedProgramDescriptor,
    controller: ProgramController,
    request_diagnostic: Option<ProgramDiagnostic>,
    latest_request_id: Option<ProgramRequestId>,
    latest_request_artifact_id: Option<ContentArtifactId>,
    latest_request_artifact_ownership: Option<ProgramArtifactOwnership>,
    bootstrapping: bool,
}

fn reject_program_request(
    program: &mut HostedProgram,
    session: &ProgramSessionId,
    diagnostic: ProgramDiagnostic,
    rejections: &mut Vec<ProgramRejection>,
) {
    if program.request_diagnostic.as_ref() != Some(&diagnostic) {
        rejections.push(ProgramRejection {
            session: session.clone(),
            diagnostic: diagnostic.clone(),
        });
    }
    program.request_diagnostic = Some(diagnostic);
    program.latest_request_id = None;
    program.latest_request_artifact_id = None;
    program.latest_request_artifact_ownership = None;
    program.bootstrapping = false;
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ProgramDocumentHostStats {
    pub full_reconcile_count: u64,
    pub scoped_parent_patch_count: u64,
    pub scoped_projection_refresh_count: u64,
}

/// Owns restricted child Sessions and projects them into one retained document.
/// Compilation is deliberately caller-scheduled so no compiler work can block an
/// input or rendering transaction.
pub struct ProgramDocumentHost {
    parent_application: ApplicationIdentity,
    programs: BTreeMap<ProgramSessionId, HostedProgram>,
    projections: BTreeMap<DocumentNodeId, ProgramProjection>,
    frame: DocumentFrame,
    parent_focus: Option<DocumentNodeId>,
    parent_scroll_roots: BTreeMap<ScrollRootId, boon_document_model::ScrollState>,
    parent_materializations: BTreeSet<u64>,
    source_routes: BTreeMap<String, ProgramSourceRoute>,
    materialization_routes: BTreeMap<u64, ProgramMaterializationRoute>,
    stats: ProgramDocumentHostStats,
}

impl ProgramDocumentHost {
    pub fn mount(
        parent_application: ApplicationIdentity,
        parent: &DocumentFrame,
    ) -> (Self, Vec<ProgramHostRequest>) {
        let mut host = Self {
            parent_application,
            programs: BTreeMap::new(),
            projections: BTreeMap::new(),
            frame: parent.clone(),
            parent_focus: parent.focus.clone(),
            parent_scroll_roots: parent.scroll_roots.clone(),
            parent_materializations: frame_materializations(parent),
            source_routes: BTreeMap::new(),
            materialization_routes: BTreeMap::new(),
            stats: ProgramDocumentHostStats::default(),
        };
        let update = host.reconcile(parent);
        (host, update.requests)
    }

    pub fn frame(&self) -> &DocumentFrame {
        &self.frame
    }

    pub fn stats(&self) -> ProgramDocumentHostStats {
        self.stats
    }

    pub fn reconcile(&mut self, parent: &DocumentFrame) -> ProgramHostUpdate {
        self.reconcile_full(parent)
    }

    pub fn reconcile_with_parent_patches(
        &mut self,
        parent: &DocumentFrame,
        parent_patches: Vec<DocumentPatch>,
    ) -> ProgramHostUpdate {
        if !parent_patches.iter().all(parent_patch_is_nonstructural) {
            return self.reconcile_full(parent);
        }
        self.stats.scoped_parent_patch_count = self
            .stats
            .scoped_parent_patch_count
            .saturating_add(parent_patches.len().try_into().unwrap_or(u64::MAX));

        self.parent_focus.clone_from(&parent.focus);
        self.parent_scroll_roots.clone_from(&parent.scroll_roots);
        if parent_patches
            .iter()
            .any(|patch| matches!(patch, DocumentPatch::SetListMaterialization { .. }))
        {
            self.parent_materializations = frame_materializations(parent);
        }
        let mut changed_projection_hosts = BTreeSet::new();
        let mut program_definition_changed = false;
        let touched = parent_patches
            .iter()
            .filter_map(parent_patch_target)
            .cloned()
            .collect::<BTreeSet<_>>();
        for id in touched {
            let Some(mut node) = parent.nodes.get(&id).cloned() else {
                return self.reconcile_full(parent);
            };
            if let Some(projection) = self.projections.get_mut(&id) {
                if node.kind != DocumentNodeKind::EmbeddedProgram {
                    return self.reconcile_full(parent);
                }
                let Some(descriptor) = node.embedded_program.clone() else {
                    return self.reconcile_full(parent);
                };
                let session = program_session_id(&id, &descriptor);
                let projection_identity_changed = projection.session != session
                    || projection.mount != descriptor.mount
                    || projection.descriptor.capability_profile != descriptor.capability_profile;
                program_definition_changed |= projection_identity_changed
                    || !same_current_program_definition(&projection.descriptor, &descriptor);
                projection.session = session;
                projection.descriptor = descriptor.clone();
                projection.mount = descriptor.mount;
                projection.parent_children = node.children.clone();
                node.children.extend(projected_root_children(projection));
                if projection_identity_changed {
                    changed_projection_hosts.insert(id.clone());
                }
            }
            self.frame.nodes.insert(id, node);
        }

        let (requests, rejections) = if program_definition_changed {
            self.schedule_requests()
        } else {
            (Vec::new(), Vec::new())
        };
        let mut patches = parent_patches;
        if !changed_projection_hosts.is_empty() {
            patches.extend(self.refresh_projections(Some(&changed_projection_hosts)));
        }
        self.refresh_metadata_and_routes();
        ProgramHostUpdate {
            patches,
            requests,
            rejections,
            bootstrap: false,
        }
    }

    fn reconcile_full(&mut self, parent: &DocumentFrame) -> ProgramHostUpdate {
        self.stats.full_reconcile_count = self.stats.full_reconcile_count.saturating_add(1);
        let previous = self.frame.clone();
        self.parent_focus.clone_from(&parent.focus);
        self.parent_scroll_roots.clone_from(&parent.scroll_roots);
        self.parent_materializations = frame_materializations(parent);
        self.install_projections(parent);
        let (requests, rejections) = self.schedule_requests();
        self.rebuild_composed_frame(parent);
        ProgramHostUpdate {
            patches: crate::document::diff_frames(&previous, &self.frame),
            requests,
            rejections,
            bootstrap: false,
        }
    }

    fn install_projections(&mut self, parent: &DocumentFrame) {
        let mut projections = BTreeMap::new();
        for node in parent.nodes.values() {
            if node.kind != DocumentNodeKind::EmbeddedProgram {
                continue;
            }
            let Some(descriptor) = node.embedded_program.clone() else {
                continue;
            };
            let session = program_session_id(&node.id, &descriptor);
            projections.insert(
                node.id.clone(),
                ProgramProjection {
                    session,
                    descriptor: descriptor.clone(),
                    mount: descriptor.mount,
                    parent_children: node.children.clone(),
                    projected: None,
                },
            );
        }
        self.projections = projections;
    }

    fn schedule_requests(&mut self) -> (Vec<ProgramHostRequest>, Vec<ProgramRejection>) {
        let mut descriptors =
            BTreeMap::<ProgramSessionId, Vec<(DocumentNodeId, EmbeddedProgramDescriptor)>>::new();
        for (host, projection) in &self.projections {
            descriptors
                .entry(projection.session.clone())
                .or_default()
                .push((host.clone(), projection.descriptor.clone()));
        }
        self.programs
            .retain(|session, _| descriptors.contains_key(session));

        let mut requests = Vec::new();
        let mut rejections = Vec::new();
        for (session, descriptors) in descriptors {
            let (host, descriptor) = descriptors
                .first()
                .cloned()
                .expect("grouped embedded program descriptors are nonempty");
            let program = self
                .programs
                .entry(session.clone())
                .or_insert_with(|| HostedProgram {
                    controller: ProgramController::new(descriptor.capability_profile),
                    descriptor: descriptor.clone(),
                    request_diagnostic: None,
                    latest_request_id: None,
                    latest_request_artifact_id: None,
                    latest_request_artifact_ownership: None,
                    bootstrapping: false,
                });
            if let Some((conflicting_host, conflicting)) = descriptors
                .iter()
                .skip(1)
                .find(|(_, other)| !same_program_definition(&descriptor, other))
            {
                reject_program_request(
                    program,
                    &session,
                    ProgramDiagnostic::new(
                        descriptor.revision.max(conflicting.revision),
                        ProgramDiagnosticPhase::Request,
                        format!(
                            "logical session `{}` has conflicting descriptors at `{}` and `{}`",
                            session.0, host.0, conflicting_host.0
                        ),
                    ),
                    &mut rejections,
                );
                continue;
            }
            if same_current_program_definition(&program.descriptor, &descriptor)
                && program.controller.latest_requested_revision() >= descriptor.revision
            {
                continue;
            }
            if program.controller.capability_profile != descriptor.capability_profile {
                program.controller = ProgramController::new(descriptor.capability_profile);
                program.bootstrapping = false;
            }
            program.descriptor = descriptor.clone();
            if program.bootstrapping {
                continue;
            }
            let request_descriptor = match bootstrap_descriptor(&descriptor) {
                Ok(Some(bootstrap))
                    if program.controller.active().is_none()
                        && program.controller.latest_requested_revision() == 0 =>
                {
                    program.bootstrapping = true;
                    bootstrap
                }
                Ok(_) => descriptor.clone(),
                Err(diagnostic) => {
                    reject_program_request(program, &session, diagnostic, &mut rejections);
                    continue;
                }
            };
            let artifact_id = match descriptor_artifact_id(&request_descriptor) {
                Ok(artifact_id) => artifact_id,
                Err(diagnostic) => {
                    reject_program_request(program, &session, diagnostic, &mut rejections);
                    continue;
                }
            };
            match program.controller.request(request_descriptor.revision) {
                Ok(()) => {
                    program.request_diagnostic = None;
                    let request_id =
                        program_request_id(&self.parent_application, &session, &request_descriptor);
                    let artifact_ownership = program_artifact_ownership(
                        &self.parent_application,
                        &session,
                        &request_id,
                        request_descriptor.artifact_retention,
                    );
                    program.latest_request_id = Some(request_id.clone());
                    program.latest_request_artifact_id = artifact_id;
                    program.latest_request_artifact_ownership = artifact_ownership;
                    let units = artifact_id.map_or_else(
                        || {
                            let mut units = vec![RuntimeSourceUnit {
                                path: "RUN.bn".to_owned(),
                                source: request_descriptor.source.clone(),
                            }];
                            units.extend(request_descriptor.support_sources.iter().map(|unit| {
                                RuntimeSourceUnit {
                                    path: unit.path.clone(),
                                    source: unit.source.clone(),
                                }
                            }));
                            units
                        },
                        |_| Vec::new(),
                    );
                    requests.push(ProgramHostRequest {
                        request_id,
                        session: session.clone(),
                        host: host.clone(),
                        compile: ProgramCompileRequest {
                            revision: request_descriptor.revision,
                            role: request_descriptor.role,
                            entry_path: "RUN.bn".to_owned(),
                            units,
                            application: child_application(&self.parent_application, &session),
                            capability_profile: request_descriptor.capability_profile,
                        },
                        artifact_id,
                        artifact_ownership,
                    });
                }
                Err(diagnostic) => {
                    reject_program_request(program, &session, diagnostic, &mut rejections);
                }
            }
        }
        (requests, rejections)
    }

    fn rebuild_composed_frame(&mut self, parent: &DocumentFrame) {
        self.frame = parent.clone();
        for projection in self.projections.values_mut() {
            projection.projected = None;
        }
        let hosts = self.projections.keys().cloned().collect::<Vec<_>>();
        for host in hosts {
            let projected = self.project_for_host(&host);
            self.install_projection(&host, projected);
        }
        self.refresh_metadata_and_routes();
    }

    fn refresh_projections(
        &mut self,
        only: Option<&BTreeSet<DocumentNodeId>>,
    ) -> Vec<DocumentPatch> {
        let hosts = self
            .projections
            .keys()
            .filter(|host| only.is_none_or(|only| only.contains(*host)))
            .cloned()
            .collect::<Vec<_>>();
        self.stats.scoped_projection_refresh_count = self
            .stats
            .scoped_projection_refresh_count
            .saturating_add(hosts.len().try_into().unwrap_or(u64::MAX));
        let mut patches = Vec::new();
        for host in hosts {
            let previous = self
                .projections
                .get(&host)
                .and_then(|projection| projection.projected.clone());
            let next = self.project_for_host(&host);
            let previous_frame = previous.as_ref().map_or_else(
                || empty_projection_frame(&host),
                |projected| projected.frame.clone(),
            );
            let next_frame = next.as_ref().map_or_else(
                || empty_projection_frame(&host),
                |projected| projected.frame.clone(),
            );
            let parent_child_count = self
                .projections
                .get(&host)
                .map_or(0, |projection| projection.parent_children.len());
            patches.extend(
                crate::document::diff_frames(&previous_frame, &next_frame)
                    .into_iter()
                    .map(|patch| offset_projection_root_patch(patch, &host, parent_child_count)),
            );
            self.install_projection(&host, next);
        }
        self.refresh_metadata_and_routes();
        patches
    }

    fn project_for_host(&self, host: &DocumentNodeId) -> Option<ProjectedProgram> {
        let projection = self.projections.get(host)?;
        if !projection.mount {
            return None;
        }
        let session = self
            .programs
            .get(&projection.session)?
            .controller
            .active()?;
        let frame = session.frame()?;
        let mut used_materializations = self.parent_materializations.clone();
        for (other_host, other) in &self.projections {
            if other_host == host {
                continue;
            }
            if let Some(projected) = &other.projected {
                used_materializations.extend(projected.materialization_routes.keys().copied());
            }
        }
        Some(project_program(
            host,
            &projection.session,
            frame,
            &mut used_materializations,
        ))
    }

    fn install_projection(&mut self, host: &DocumentNodeId, next: Option<ProjectedProgram>) {
        let Some(projection) = self.projections.get_mut(host) else {
            return;
        };
        if let Some(previous) = projection.projected.take() {
            for id in previous
                .frame
                .nodes
                .keys()
                .filter(|id| **id != previous.frame.root)
            {
                self.frame.nodes.remove(id);
            }
        }
        if let Some(projected) = &next {
            for (id, node) in &projected.frame.nodes {
                if *id != projected.frame.root {
                    self.frame.nodes.insert(id.clone(), node.clone());
                }
            }
        }
        projection.projected = next;
        if let Some(host_node) = self.frame.nodes.get_mut(host) {
            host_node.children = projection.parent_children.clone();
            host_node
                .children
                .extend(projected_root_children(projection));
        }
    }

    fn refresh_metadata_and_routes(&mut self) {
        self.frame.focus.clone_from(&self.parent_focus);
        self.frame
            .scroll_roots
            .clone_from(&self.parent_scroll_roots);
        self.source_routes.clear();
        self.materialization_routes.clear();
        for projection in self.projections.values() {
            let Some(projected) = &projection.projected else {
                continue;
            };
            self.source_routes.extend(projected.source_routes.clone());
            self.materialization_routes
                .extend(projected.materialization_routes.clone());
            self.frame
                .scroll_roots
                .extend(projected.frame.scroll_roots.clone());
            if projected.frame.focus.is_some() {
                self.frame.focus.clone_from(&projected.frame.focus);
            }
        }
    }

    pub fn complete(
        &mut self,
        session: &ProgramSessionId,
        request_id: &ProgramRequestId,
        result: Result<ProgramArtifact, ProgramDiagnostic>,
    ) -> (ProgramHostCompletion, ProgramHostUpdate) {
        let Some(program) = self.programs.get_mut(session) else {
            return (
                ProgramHostCompletion::Removed {
                    session: session.clone(),
                },
                ProgramHostUpdate::default(),
            );
        };
        if program.latest_request_id.as_ref() != Some(request_id) {
            return (
                ProgramHostCompletion::Superseded {
                    session: session.clone(),
                    request_id: request_id.clone(),
                },
                ProgramHostUpdate::default(),
            );
        }
        let bootstrap = program.bootstrapping;
        program.bootstrapping = false;
        let completion = ProgramHostCompletion::Program(program.controller.complete(result));
        let hosts = self
            .projections
            .iter()
            .filter_map(|(host, projection)| {
                (projection.session == *session).then_some(host.clone())
            })
            .collect::<BTreeSet<_>>();
        let patches = self.refresh_projections(Some(&hosts));
        let (requests, rejections) = if bootstrap {
            self.schedule_requests()
        } else {
            (Vec::new(), Vec::new())
        };
        (
            completion,
            ProgramHostUpdate {
                patches,
                requests,
                rejections,
                bootstrap,
            },
        )
    }

    pub fn diagnostics(&self) -> Vec<ProgramHostDiagnostic> {
        self.programs
            .iter()
            .filter_map(|(session, program)| {
                program
                    .request_diagnostic
                    .as_ref()
                    .or_else(|| program.controller.diagnostic())
                    .cloned()
                    .map(|diagnostic| ProgramHostDiagnostic {
                        session: session.clone(),
                        hosts: self
                            .projections
                            .iter()
                            .filter_map(|(host, projection)| {
                                (projection.session == *session).then_some(host.clone())
                            })
                            .collect(),
                        diagnostic,
                    })
            })
            .collect()
    }

    pub fn active_artifact(&self, session: &ProgramSessionId) -> Option<&ProgramArtifact> {
        self.programs
            .get(session)?
            .controller
            .active()
            .map(ProgramSession::artifact)
    }

    pub fn request_artifact_ownership(
        &self,
        session: &ProgramSessionId,
        request_id: &ProgramRequestId,
    ) -> Option<ProgramArtifactOwnership> {
        self.programs.get(session).and_then(|program| {
            (program.latest_request_id.as_ref() == Some(request_id))
                .then_some(program.latest_request_artifact_ownership)
                .flatten()
        })
    }

    pub fn request_is_artifact_load(
        &self,
        session: &ProgramSessionId,
        request_id: &ProgramRequestId,
    ) -> bool {
        self.programs.get(session).is_some_and(|program| {
            program.latest_request_id.as_ref() == Some(request_id)
                && program.latest_request_artifact_id.is_some()
        })
    }

    pub fn request_is_current(
        &self,
        session: &ProgramSessionId,
        request_id: &ProgramRequestId,
    ) -> bool {
        self.programs
            .get(session)
            .is_some_and(|program| program.latest_request_id.as_ref() == Some(request_id))
    }

    pub fn lifecycle_source_paths(&self, session: &ProgramSessionId, intent: &str) -> Vec<String> {
        self.projections
            .iter()
            .filter(|(_, projection)| projection.session == *session)
            .filter_map(|(host, _)| self.frame.nodes.get(host))
            .flat_map(|node| node.source_bindings())
            .filter(|binding| binding.intent == intent)
            .map(|binding| binding.source_path.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }

    pub fn source_is_row_scoped(&self, route: &str) -> Option<bool> {
        let route = self.source_routes.get(route)?;
        self.programs
            .get(&route.session)?
            .controller
            .active()?
            .runtime()
            .source_is_row_scoped(&route.source_path)
    }

    pub fn row_target_for_source_text(
        &self,
        route: &str,
        text: &str,
        occurrence: usize,
    ) -> crate::RuntimeResult<Option<RowId>> {
        let route = self
            .source_routes
            .get(route)
            .ok_or_else(|| format!("embedded program has no source route `{route}`"))?;
        let program = self
            .programs
            .get(&route.session)
            .and_then(|program| program.controller.active())
            .ok_or_else(|| format!("embedded program `{}` is not active", route.session.0))?;
        program
            .runtime()
            .row_target_for_source_text(&route.source_path, text, occurrence)
    }

    pub fn row_target_for_source_path(
        &self,
        route: &str,
        key: u64,
        generation: u64,
    ) -> crate::RuntimeResult<RowId> {
        let route = self
            .source_routes
            .get(route)
            .ok_or_else(|| format!("embedded program has no source route `{route}`"))?;
        let program = self
            .programs
            .get(&route.session)
            .and_then(|program| program.controller.active())
            .ok_or_else(|| format!("embedded program `{}` is not active", route.session.0))?;
        program
            .runtime()
            .row_target_for_source_path(&route.source_path, key, generation)
    }

    pub fn dispatch(
        &mut self,
        sequence: u64,
        route: &str,
        target: Option<RowId>,
        payload: SourcePayload,
    ) -> crate::RuntimeResult<(crate::RuntimeTurn, Vec<DocumentPatch>)> {
        let route = self
            .source_routes
            .get(route)
            .cloned()
            .ok_or_else(|| format!("embedded program has no source route `{route}`"))?;
        let program = self
            .programs
            .get_mut(&route.session)
            .and_then(|program| program.controller.active_mut())
            .ok_or_else(|| format!("embedded program `{}` is not active", route.session.0))?;
        let event =
            program
                .runtime()
                .source_event(sequence, &route.source_path, target, payload)?;
        let turn = program.runtime_mut().dispatch(event)?;
        let hosts = self.hosts_for_session(&route.session);
        let patches = self.refresh_projections(Some(&hosts));
        Ok((turn, patches))
    }

    pub fn demand_document_window(
        &mut self,
        materialization: u64,
        visible: Range<u64>,
        overscan: Range<u64>,
    ) -> crate::RuntimeResult<Vec<DocumentPatch>> {
        let route = self
            .materialization_routes
            .get(&materialization)
            .cloned()
            .ok_or_else(|| {
                format!("embedded program has no materialization `{materialization}`")
            })?;
        let program = self
            .programs
            .get_mut(&route.session)
            .and_then(|program| program.controller.active_mut())
            .ok_or_else(|| format!("embedded program `{}` is not active", route.session.0))?;
        program.runtime_mut().demand_document_window_by_id(
            route.materialization,
            visible,
            overscan,
        )?;
        let hosts = self.hosts_for_session(&route.session);
        Ok(self.refresh_projections(Some(&hosts)))
    }

    pub fn owns_source_route(&self, route: &str) -> bool {
        self.source_routes.contains_key(route)
    }

    pub fn owns_materialization(&self, materialization: u64) -> bool {
        self.materialization_routes.contains_key(&materialization)
    }

    fn hosts_for_session(&self, session: &ProgramSessionId) -> BTreeSet<DocumentNodeId> {
        self.projections
            .iter()
            .filter_map(|(host, projection)| {
                (projection.session == *session).then_some(host.clone())
            })
            .collect()
    }
}

fn project_program(
    host: &DocumentNodeId,
    session: &ProgramSessionId,
    child: &DocumentFrame,
    used_materializations: &mut BTreeSet<u64>,
) -> ProjectedProgram {
    let mut frame = empty_projection_frame(host);
    let mut source_routes = BTreeMap::new();
    let mut materialization_routes = BTreeMap::new();
    let root_children = child
        .nodes
        .get(&child.root)
        .map(|root| root.children.clone())
        .unwrap_or_default();
    let child_ids = child
        .nodes
        .keys()
        .filter(|id| **id != child.root)
        .cloned()
        .collect::<Vec<_>>();

    for child_id in child_ids {
        let Some(mut node) = child.nodes.get(&child_id).cloned() else {
            continue;
        };
        node.id = namespaced_node(host, &node.id);
        node.parent = node.parent.as_ref().map(|parent_id| {
            if *parent_id == child.root {
                host.clone()
            } else {
                namespaced_node(host, parent_id)
            }
        });
        node.children = node
            .children
            .iter()
            .map(|child_id| namespaced_node(host, child_id))
            .collect();
        for binding in &mut node.source_bindings {
            let original_path = binding.source_path.clone();
            let route_key = source_route_key(host, &original_path);
            source_routes.insert(
                route_key.clone(),
                ProgramSourceRoute {
                    session: session.clone(),
                    source_path: original_path,
                },
            );
            binding.id =
                SourceBindingId(format!("embedded/{}/{}", namespace(&host.0), binding.id.0));
            binding.source_path = route_key;
        }
        for range in &mut node.materialized {
            let Some(original) = range.materialization else {
                continue;
            };
            let mapped = namespaced_materialization(host, original, used_materializations);
            materialization_routes.insert(
                mapped,
                ProgramMaterializationRoute {
                    session: session.clone(),
                    materialization: original,
                },
            );
            range.materialization = Some(mapped);
        }
        frame.nodes.insert(node.id.clone(), node);
    }

    if let Some(root) = frame.nodes.get_mut(&frame.root) {
        root.children = root_children
            .iter()
            .map(|child_id| namespaced_node(host, child_id))
            .collect();
    }
    frame.focus = child
        .focus
        .as_ref()
        .map(|focus| namespaced_node(host, focus));
    for (scroll_root, state) in &child.scroll_roots {
        frame.scroll_roots.insert(
            ScrollRootId(format!("embedded/{}/{}", namespace(&host.0), scroll_root.0)),
            *state,
        );
    }

    ProjectedProgram {
        frame,
        source_routes,
        materialization_routes,
    }
}

fn empty_projection_frame(host: &DocumentNodeId) -> DocumentFrame {
    DocumentFrame::empty(host.0.clone())
}

fn projected_root_children(projection: &ProgramProjection) -> Vec<DocumentNodeId> {
    projection
        .projected
        .as_ref()
        .and_then(|projected| projected.frame.nodes.get(&projected.frame.root))
        .map(|root| root.children.clone())
        .unwrap_or_default()
}

fn frame_materializations(frame: &DocumentFrame) -> BTreeSet<u64> {
    frame
        .nodes
        .values()
        .flat_map(|node| node.materialized.iter())
        .filter_map(|range| range.materialization)
        .collect()
}

fn parent_patch_is_nonstructural(patch: &DocumentPatch) -> bool {
    matches!(
        patch,
        DocumentPatch::SetText { .. }
            | DocumentPatch::SetStyle { .. }
            | DocumentPatch::SetEmbeddedProgram { .. }
            | DocumentPatch::SetBinding { .. }
            | DocumentPatch::SetBindingAt { .. }
            | DocumentPatch::SetTextInputFocus { .. }
            | DocumentPatch::SetScroll { .. }
            | DocumentPatch::SetListMaterialization { .. }
    )
}

fn parent_patch_target(patch: &DocumentPatch) -> Option<&DocumentNodeId> {
    match patch {
        DocumentPatch::SetText { id, .. }
        | DocumentPatch::SetStyle { id, .. }
        | DocumentPatch::SetEmbeddedProgram { id, .. }
        | DocumentPatch::SetBinding { id, .. }
        | DocumentPatch::SetBindingAt { id, .. }
        | DocumentPatch::SetTextInputFocus { id, .. }
        | DocumentPatch::SetScroll { id, .. }
        | DocumentPatch::SetListMaterialization { id, .. } => Some(id),
        DocumentPatch::UpsertNode(_)
        | DocumentPatch::RemoveNode { .. }
        | DocumentPatch::InsertChild { .. }
        | DocumentPatch::RemoveChild { .. }
        | DocumentPatch::MoveChild { .. } => None,
    }
}

fn offset_projection_root_patch(
    patch: DocumentPatch,
    host: &DocumentNodeId,
    parent_child_count: usize,
) -> DocumentPatch {
    match patch {
        DocumentPatch::InsertChild {
            parent,
            child,
            index,
        } if parent == *host => DocumentPatch::InsertChild {
            parent,
            child,
            index: index.saturating_add(parent_child_count),
        },
        DocumentPatch::MoveChild {
            child,
            new_parent,
            index,
        } if new_parent == *host => DocumentPatch::MoveChild {
            child,
            new_parent,
            index: index.saturating_add(parent_child_count),
        },
        patch => patch,
    }
}

fn child_application(
    parent: &ApplicationIdentity,
    session: &ProgramSessionId,
) -> ApplicationIdentity {
    ApplicationIdentity::new(
        format!("{}.embedded", parent.package_id),
        format!("{}.{}", parent.state_namespace, namespace(&session.0)),
        parent.deployment_domain.clone(),
    )
}

fn program_request_id(
    parent: &ApplicationIdentity,
    session: &ProgramSessionId,
    descriptor: &EmbeddedProgramDescriptor,
) -> ProgramRequestId {
    let revision = descriptor.revision.to_string();
    let support_parts = descriptor
        .support_sources
        .iter()
        .map(|unit| (unit.path.as_str(), unit.source.as_str()))
        .collect::<Vec<_>>();
    let support_sources = crate::source_unit_parts_hash(&support_parts);
    ProgramRequestId(crate::source_unit_parts_hash(&[
        ("parent.package_id", parent.package_id.as_str()),
        ("parent.state_namespace", parent.state_namespace.as_str()),
        (
            "parent.deployment_domain",
            parent.deployment_domain.as_str(),
        ),
        ("session", session.0.as_str()),
        ("revision", revision.as_str()),
        ("source", descriptor.source_digest.as_str()),
        ("support_sources", support_sources.as_str()),
        ("artifact", descriptor.artifact_id.as_str()),
    ]))
}

fn program_artifact_ownership(
    parent: &ApplicationIdentity,
    session: &ProgramSessionId,
    request: &ProgramRequestId,
    retention: ProgramArtifactRetention,
) -> Option<ProgramArtifactOwnership> {
    let (domain, retention, include_request) = match retention {
        ProgramArtifactRetention::Ephemeral => return None,
        ProgramArtifactRetention::Replaceable => (
            b"boon.program.slot.v1".as_slice(),
            ContentArtifactRetention::Replaceable,
            false,
        ),
        ProgramArtifactRetention::Archive => (
            b"boon.program.archive.v1".as_slice(),
            ContentArtifactRetention::Immutable,
            true,
        ),
    };
    let mut hasher = Sha256::new();
    hash_owner_part(&mut hasher, domain);
    for part in [
        parent.package_id.as_bytes(),
        parent.state_namespace.as_bytes(),
        parent.deployment_domain.as_bytes(),
        session.0.as_bytes(),
    ] {
        hash_owner_part(&mut hasher, part);
    }
    if include_request {
        hash_owner_part(&mut hasher, request.0.as_bytes());
    }
    Some(ProgramArtifactOwnership {
        owner: ContentArtifactOwnerId(hasher.finalize().into()),
        retention,
    })
}

fn hash_owner_part(hasher: &mut Sha256, bytes: &[u8]) {
    hasher.update((bytes.len() as u64).to_be_bytes());
    hasher.update(bytes);
}

fn program_session_id(
    host: &DocumentNodeId,
    descriptor: &EmbeddedProgramDescriptor,
) -> ProgramSessionId {
    let explicit = descriptor.session_key.trim();
    if explicit.is_empty() {
        ProgramSessionId(host.0.clone())
    } else {
        ProgramSessionId(explicit.to_owned())
    }
}

fn same_program_definition(
    left: &EmbeddedProgramDescriptor,
    right: &EmbeddedProgramDescriptor,
) -> bool {
    left.source_digest == right.source_digest
        && left.support_sources == right.support_sources
        && left.artifact_id == right.artifact_id
        && left.artifact_retention == right.artifact_retention
        && left.revision == right.revision
        && left.bootstrap_source_digest == right.bootstrap_source_digest
        && left.bootstrap_artifact_id == right.bootstrap_artifact_id
        && left.bootstrap_revision == right.bootstrap_revision
        && left.role == right.role
        && left.capability_profile == right.capability_profile
}

fn same_current_program_definition(
    left: &EmbeddedProgramDescriptor,
    right: &EmbeddedProgramDescriptor,
) -> bool {
    left.source_digest == right.source_digest
        && left.support_sources == right.support_sources
        && left.artifact_id == right.artifact_id
        && left.artifact_retention == right.artifact_retention
        && left.revision == right.revision
        && left.role == right.role
        && left.capability_profile == right.capability_profile
}

fn bootstrap_descriptor(
    descriptor: &EmbeddedProgramDescriptor,
) -> Result<Option<EmbeddedProgramDescriptor>, ProgramDiagnostic> {
    let has_source = !descriptor.bootstrap_source.is_empty();
    let has_artifact = !descriptor.bootstrap_artifact_id.trim().is_empty();
    if has_source && has_artifact {
        return Err(ProgramDiagnostic::new(
            descriptor.revision,
            ProgramDiagnosticPhase::Request,
            "embedded program bootstrap cannot provide both source and artifact_id",
        ));
    }
    let has_bootstrap = descriptor.bootstrap_revision > 0 && (has_source || has_artifact);
    let differs = descriptor.bootstrap_revision != descriptor.revision
        || descriptor.bootstrap_source_digest != descriptor.source_digest
        || descriptor.bootstrap_artifact_id != descriptor.artifact_id;
    if !has_bootstrap || !differs {
        return Ok(None);
    }
    if descriptor.bootstrap_revision > descriptor.revision {
        return Err(ProgramDiagnostic::new(
            descriptor.revision,
            ProgramDiagnosticPhase::Request,
            "bootstrap_revision must not exceed the current program revision",
        ));
    }
    let mut bootstrap = descriptor.clone();
    bootstrap.source = if has_artifact {
        String::new()
    } else {
        descriptor.bootstrap_source.clone()
    };
    bootstrap.source_digest = if has_artifact {
        String::new()
    } else {
        descriptor.bootstrap_source_digest.clone()
    };
    if has_artifact {
        bootstrap.support_sources.clear();
    }
    bootstrap.artifact_id = descriptor.bootstrap_artifact_id.clone();
    bootstrap.artifact_retention = ProgramArtifactRetention::Ephemeral;
    bootstrap.revision = descriptor.bootstrap_revision;
    bootstrap.bootstrap_source.clear();
    bootstrap.bootstrap_source_digest.clear();
    bootstrap.bootstrap_artifact_id.clear();
    bootstrap.bootstrap_revision = 0;
    Ok(Some(bootstrap))
}

fn descriptor_artifact_id(
    descriptor: &EmbeddedProgramDescriptor,
) -> Result<Option<ContentArtifactId>, ProgramDiagnostic> {
    let artifact_id = descriptor.artifact_id.trim();
    if artifact_id.is_empty() {
        if descriptor.source.is_empty() {
            return Err(ProgramDiagnostic::new(
                descriptor.revision,
                ProgramDiagnosticPhase::Request,
                "embedded program requires source or artifact_id",
            ));
        }
        return Ok(None);
    }
    if !descriptor.source.is_empty() {
        return Err(ProgramDiagnostic::new(
            descriptor.revision,
            ProgramDiagnosticPhase::Request,
            "artifact-backed embedded program cannot also provide source",
        ));
    }
    if !descriptor.support_sources.is_empty() {
        return Err(ProgramDiagnostic::new(
            descriptor.revision,
            ProgramDiagnosticPhase::Request,
            "artifact-backed embedded program cannot also provide support sources",
        ));
    }
    if descriptor.artifact_retention != ProgramArtifactRetention::Ephemeral {
        return Err(ProgramDiagnostic::new(
            descriptor.revision,
            ProgramDiagnosticPhase::Request,
            "artifact-backed embedded program cannot retain an already stored artifact",
        ));
    }
    ContentArtifactId::from_hex(artifact_id)
        .map(Some)
        .map_err(|error| {
            ProgramDiagnostic::new(descriptor.revision, ProgramDiagnosticPhase::Request, error)
        })
}

fn namespaced_node(host: &DocumentNodeId, child: &DocumentNodeId) -> DocumentNodeId {
    DocumentNodeId(format!("embedded/{}/{}", namespace(&host.0), child.0))
}

fn source_route_key(host: &DocumentNodeId, source_path: &str) -> String {
    format!(
        "embedded-source/{}/{}",
        namespace(&host.0),
        crate::sha256_bytes(source_path.as_bytes())
    )
}

fn namespaced_materialization(
    host: &DocumentNodeId,
    materialization: u64,
    used: &mut BTreeSet<u64>,
) -> u64 {
    let digest = crate::sha256_bytes(format!("{}:{materialization}", host.0).as_bytes());
    let mut candidate = u64::from_str_radix(&digest[..16], 16).unwrap_or(materialization);
    while !used.insert(candidate) {
        candidate = candidate.wrapping_add(1);
    }
    candidate
}

fn namespace(value: &str) -> String {
    crate::sha256_bytes(value.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use boon_document::{DocumentChangeBatch, DocumentState};
    use boon_document_model::{
        DocumentNode, DocumentNodeKind, EmbeddedProgramSourceUnit, TextValue,
    };

    fn request(revision: u64, source: &str) -> ProgramCompileRequest {
        ProgramCompileRequest {
            revision,
            entry_path: "Child.bn".to_owned(),
            units: vec![RuntimeSourceUnit {
                path: "Child.bn".to_owned(),
                source: source.to_owned(),
            }],
            application: ApplicationIdentity::new(
                "dev.boon.test-child",
                "test-child",
                "runtime-test",
            ),
            role: boon_plan::ProgramRole::Client,
            capability_profile: ProgramCapabilityProfile::PublicClient,
        }
    }

    fn server_request(revision: u64, source: &str) -> ProgramCompileRequest {
        ProgramCompileRequest {
            revision,
            entry_path: "RUN.bn".to_owned(),
            units: vec![RuntimeSourceUnit {
                path: "RUN.bn".to_owned(),
                source: source.to_owned(),
            }],
            application: ApplicationIdentity::new(
                "dev.boon.test-child",
                "test-server",
                "runtime-test",
            ),
            role: boon_plan::ProgramRole::Server,
            capability_profile: ProgramCapabilityProfile::TrustedServer,
        }
    }

    fn session_request(revision: u64, source: &str) -> ProgramCompileRequest {
        ProgramCompileRequest {
            revision,
            entry_path: "RUN.bn".to_owned(),
            units: vec![RuntimeSourceUnit {
                path: "RUN.bn".to_owned(),
                source: source.to_owned(),
            }],
            application: ApplicationIdentity::new(
                "dev.boon.test-child",
                "test-session",
                "runtime-test",
            ),
            role: boon_plan::ProgramRole::Session,
            capability_profile: ProgramCapabilityProfile::TrustedSession,
        }
    }

    fn server_source() -> &'static str {
        r#"
store: [
    request_received: SOURCE
    request_count:
        0 |> HOLD request_count {
            LATEST {
                store.request_received |> THEN { request_count + 1 }
            }
        }
]

outputs: [
    response: [status: TEXT { accepted }, count: store.request_count]
]
"#
    }

    #[test]
    fn trusted_package_sources_have_a_separate_bounded_build_limit() {
        let mut request = request(1, "document: Document/new(root: [])");
        request.units[0].source.push_str(&" ".repeat(128 * 1024));
        assert!(
            validate_request(&request)
                .unwrap_err()
                .to_string()
                .contains("source bundle uses")
        );
        validate_request_with_source_limits(
            &request,
            MAX_TRUSTED_PACKAGE_SOURCE_UNITS,
            MAX_TRUSTED_PACKAGE_SOURCE_BYTES,
        )
        .unwrap();
    }

    fn outbound_document_source() -> &'static str {
        r#"
store: [
    request: SOURCE
    response:
        NotRequested |> HOLD response {
            store.request |> THEN {
                Http/request(
                    endpoint: store.request.endpoint
                    method: store.request.method
                    path_segments: store.request.path_segments
                    query: store.request.query
                    headers: store.request.headers
                    body: store.request.body
                    connect_timeout_ms: store.request.connect_timeout_ms
                    overall_timeout_ms: store.request.overall_timeout_ms
                )
            }
        }
]

scene: Scene/Element/text(
    element: []
    style: [width: Fill]
    text: TEXT { Outbound HTTP document }
)
"#
    }

    fn parent_frame(revision: u64, source: &str) -> DocumentFrame {
        let mut frame = DocumentFrame::empty("parent");
        let mut program = DocumentNode::new("program", DocumentNodeKind::EmbeddedProgram);
        program.parent = Some(frame.root.clone());
        program.embedded_program = Some(EmbeddedProgramDescriptor {
            source: source.to_owned(),
            source_digest: crate::sha256_bytes(source.as_bytes()),
            revision,
            role: boon_plan::ProgramRole::Client,
            capability_profile: ProgramCapabilityProfile::PublicClient,
            session_key: String::new(),
            mount: true,
            ..EmbeddedProgramDescriptor::default()
        });
        frame
            .nodes
            .get_mut(&frame.root)
            .unwrap()
            .children
            .push(program.id.clone());
        frame.nodes.insert(program.id.clone(), program);
        frame
    }

    fn child_source(label: &str) -> String {
        format!(
            "scene: Scene/Element/text(element: [], style: [width: Fill], text: TEXT {{ {label} }})\n"
        )
    }

    fn frame_has_text(frame: &DocumentFrame, expected: &str) -> bool {
        frame
            .nodes
            .values()
            .any(|node| node.text.as_ref().is_some_and(|text| text.text == expected))
    }

    #[test]
    fn replaceable_artifact_owner_is_stable_per_parent_session() {
        let parent = ApplicationIdentity::new("dev.boon.parent", "owner-test", "local");
        let session = ProgramSessionId("public-page".to_owned());
        let first = program_artifact_ownership(
            &parent,
            &session,
            &ProgramRequestId("first".to_owned()),
            ProgramArtifactRetention::Replaceable,
        )
        .unwrap();
        let second = program_artifact_ownership(
            &parent,
            &session,
            &ProgramRequestId("second".to_owned()),
            ProgramArtifactRetention::Replaceable,
        )
        .unwrap();
        let other_session = program_artifact_ownership(
            &parent,
            &ProgramSessionId("other-page".to_owned()),
            &ProgramRequestId("first".to_owned()),
            ProgramArtifactRetention::Replaceable,
        )
        .unwrap();

        assert_eq!(first, second);
        assert_eq!(first.retention, ContentArtifactRetention::Replaceable);
        assert_ne!(first.owner, other_session.owner);
    }

    #[test]
    fn archived_artifact_owner_is_stable_per_request() {
        let parent = ApplicationIdentity::new("dev.boon.parent", "archive-test", "local");
        let session = ProgramSessionId("public-page".to_owned());
        let request = ProgramRequestId("revision-7".to_owned());
        let first = program_artifact_ownership(
            &parent,
            &session,
            &request,
            ProgramArtifactRetention::Archive,
        )
        .unwrap();
        let repeated = program_artifact_ownership(
            &parent,
            &session,
            &request,
            ProgramArtifactRetention::Archive,
        )
        .unwrap();
        let next = program_artifact_ownership(
            &parent,
            &session,
            &ProgramRequestId("revision-8".to_owned()),
            ProgramArtifactRetention::Archive,
        )
        .unwrap();

        assert_eq!(first, repeated);
        assert_eq!(first.retention, ContentArtifactRetention::Immutable);
        assert_ne!(first.owner, next.owner);
    }

    fn interactive_child_source() -> &'static str {
        r#"
store: [
    button: [events: [press: SOURCE]]
    label:
        TEXT { Before } |> HOLD label {
            store.button.events.press |> THEN { TEXT { After } }
        }
]

document: Document/new(
    root: Element/button(
        element: [events: store.button.events]
        style: [width: 120, height: 40]
        label: store.label
    )
)
"#
    }

    fn replay_patches(frame: DocumentFrame, patches: Vec<DocumentPatch>) -> DocumentFrame {
        DocumentState::apply_batch_to_owned_frame(frame, DocumentChangeBatch { patches })
            .unwrap()
            .0
    }

    #[test]
    fn bounded_program_artifact_starts_one_retained_session() {
        let artifact = compile_program_artifact(&request(1, &child_source("First"))).unwrap();
        assert_eq!(artifact.revision(), 1);
        assert_eq!(
            artifact.plan().target_profile,
            TargetProfile::SoftwareBounded
        );
        let mut controller = ProgramController::new(ProgramCapabilityProfile::PublicClient);
        controller.request(1).unwrap();
        assert_eq!(
            controller.complete(Ok(artifact)),
            ProgramCompletion::Activated { revision: 1 }
        );
        let frame = controller.active().unwrap().frame().unwrap();
        assert!(
            frame
                .nodes
                .values()
                .any(|node| { node.text.as_ref().is_some_and(|text| text.text == "First") })
        );
    }

    #[test]
    fn program_artifact_is_deterministic_round_trips_and_rejects_tampering() {
        let source = child_source("Immutable");
        let first = compile_program_artifact(&request(1, &source)).unwrap();
        let second = compile_program_artifact(&request(99, &source)).unwrap();
        assert_eq!(first.id(), second.id());
        assert_eq!(first.plan_digest(), second.plan_digest());

        let content = first.to_content_artifact();
        let restored = ProgramArtifact::from_content_artifact(
            7,
            ProgramCapabilityProfile::PublicClient,
            content.clone(),
        )
        .unwrap();
        assert_eq!(restored.id(), first.id());
        assert_eq!(restored.plan_digest(), first.plan_digest());
        assert_eq!(restored.plan().as_ref(), first.plan().as_ref());

        let mut corrupt = content.clone();
        corrupt.bytes[0] ^= 0xff;
        let diagnostic = ProgramArtifact::from_content_artifact(
            7,
            ProgramCapabilityProfile::PublicClient,
            corrupt,
        )
        .unwrap_err();
        assert_eq!(diagnostic.phase, ProgramDiagnosticPhase::Artifact);
        assert!(diagnostic.message.contains("digest"));

        let mut stored: StoredProgramArtifact =
            ciborium::de::from_reader(content.bytes.as_slice()).unwrap();
        stored.compiler_id = "boon-compiler-future".to_owned();
        let mut bytes = Vec::new();
        ciborium::ser::into_writer(&stored, &mut bytes).unwrap();
        let incompatible = ContentArtifact::new(PROGRAM_ARTIFACT_MEDIA_TYPE, bytes).unwrap();
        let diagnostic = ProgramArtifact::from_content_artifact(
            7,
            ProgramCapabilityProfile::PublicClient,
            incompatible,
        )
        .unwrap_err();
        assert_eq!(diagnostic.phase, ProgramDiagnosticPhase::Artifact);
        assert!(diagnostic.message.contains("differs from host compiler"));

        stored.compiler_id = COMPILER_ID.to_owned();
        stored
            .plan
            .application
            .identity
            .state_namespace
            .push_str("-tampered");
        let mut bytes = Vec::new();
        ciborium::ser::into_writer(&stored, &mut bytes).unwrap();
        let stale_digest = ContentArtifact::new(PROGRAM_ARTIFACT_MEDIA_TYPE, bytes).unwrap();
        let diagnostic = ProgramArtifact::from_content_artifact(
            7,
            ProgramCapabilityProfile::PublicClient,
            stale_digest,
        )
        .unwrap_err();
        assert_eq!(diagnostic.phase, ProgramDiagnosticPhase::Artifact);
        assert!(diagnostic.message.contains("plan digest"));
    }

    #[test]
    fn trusted_server_artifact_round_trips_and_runs_without_a_document_frame() {
        let artifact = compile_program_artifact(&server_request(1, server_source())).unwrap();
        assert_eq!(artifact.role(), ProgramRole::Server);
        assert_eq!(
            artifact.capability_profile(),
            ProgramCapabilityProfile::TrustedServer
        );
        assert_eq!(artifact.application().state_namespace, "test-server");
        assert!(artifact.plan().document.is_none());

        let restored = ProgramArtifact::from_content_artifact(
            7,
            ProgramCapabilityProfile::TrustedServer,
            artifact.to_content_artifact(),
        )
        .unwrap();
        assert_eq!(restored.id(), artifact.id());
        assert_eq!(restored.role(), ProgramRole::Server);

        let mut session = ProgramSession::start(restored).unwrap();
        assert!(session.frame().is_none());
        assert_eq!(
            session.root_value_current("store.request_count").unwrap(),
            crate::Value::integer(0).unwrap()
        );
        let dispatched = session
            .dispatch("store.request_received", None, SourcePayload::default())
            .unwrap();
        assert_eq!(dispatched.source_sequence, 1);
        assert_eq!(session.next_source_sequence(), 2);
        let response = session.output_value_current("response").unwrap();
        assert!(matches!(response, crate::Value::Record(fields)
            if fields.get("count") == Some(&crate::Value::integer(1).unwrap())));
    }

    #[test]
    fn public_client_may_emit_typed_read_only_effect_without_an_outbox() {
        let artifact = compile_program_artifact(&request(1, outbound_document_source())).unwrap();
        let [effect] = artifact.plan().effects.as_slice() else {
            panic!("document plan must own one outbound HTTP effect contract");
        };
        assert!(matches!(effect.replay, EffectReplay::ReadOnly));
        assert_eq!(effect.barrier, EffectBarrier::None);
        assert!(effect.schema.is_some());

        let mut session = ProgramSession::start(artifact).unwrap();
        let dispatched = session
            .dispatch(
                "store.request",
                None,
                SourcePayload {
                    fields: BTreeMap::from([
                        (
                            "endpoint".to_owned(),
                            crate::Value::Text("catalog".to_owned()),
                        ),
                        ("method".to_owned(), crate::Value::Text("Get".to_owned())),
                        ("path_segments".to_owned(), crate::Value::List(Vec::new())),
                        ("query".to_owned(), crate::Value::List(Vec::new())),
                        ("headers".to_owned(), crate::Value::List(Vec::new())),
                        ("body".to_owned(), crate::Value::Bytes(Vec::new().into())),
                        (
                            "connect_timeout_ms".to_owned(),
                            crate::Value::integer(500).unwrap(),
                        ),
                        (
                            "overall_timeout_ms".to_owned(),
                            crate::Value::integer(2_000).unwrap(),
                        ),
                        (
                            "cancellation".to_owned(),
                            crate::Value::Text("Independent".to_owned()),
                        ),
                    ]),
                    ..SourcePayload::default()
                },
            )
            .unwrap();
        assert!(dispatched.runtime_turn.outbox_changes.is_empty());
        assert_eq!(dispatched.runtime_turn.transient_effects.len(), 1);
    }

    #[test]
    fn persistent_transient_completion_is_an_acknowledged_authority_turn() {
        let artifact = compile_program_artifact(&server_request(
            1,
            include_str!("../../../examples/outbound_http_effect.bn"),
        ))
        .unwrap();
        let (mut session, _) = PersistentProgramSession::start(
            artifact,
            boon_persistence::InMemoryDriver::default(),
            PersistenceWorkerConfig::default(),
        )
        .unwrap();
        let request = SourcePayload {
            fields: BTreeMap::from([
                (
                    "endpoint".to_owned(),
                    crate::Value::Text("catalog".to_owned()),
                ),
                ("method".to_owned(), crate::Value::Text("Get".to_owned())),
                ("path_segments".to_owned(), crate::Value::List(Vec::new())),
                ("query".to_owned(), crate::Value::List(Vec::new())),
                ("headers".to_owned(), crate::Value::List(Vec::new())),
                ("body".to_owned(), crate::Value::Bytes(Vec::new().into())),
                (
                    "connect_timeout_ms".to_owned(),
                    crate::Value::integer(500).unwrap(),
                ),
                (
                    "overall_timeout_ms".to_owned(),
                    crate::Value::integer(2_000).unwrap(),
                ),
                (
                    "cancellation".to_owned(),
                    crate::Value::Text("Independent".to_owned()),
                ),
            ]),
            ..SourcePayload::default()
        };
        let (dispatched, first_acknowledgement) = session
            .dispatch_durably("store.request", None, request)
            .unwrap();
        let [invocation] = dispatched.runtime_turn.transient_effects.as_slice() else {
            panic!("request turn must emit one transient effect");
        };
        let call_id = invocation.call_id;
        assert_eq!(session.pending_transient_effect_count(), 1);
        assert_eq!(
            first_acknowledgement.through_turn_sequence,
            dispatched.runtime_turn.sequence
        );

        let outcome = crate::Value::Record(BTreeMap::from([
            (
                "$tag".to_owned(),
                crate::Value::Text("HttpSucceeded".to_owned()),
            ),
            (
                "endpoint".to_owned(),
                crate::Value::Text("catalog".to_owned()),
            ),
            ("status".to_owned(), crate::Value::integer(207).unwrap()),
            ("headers".to_owned(), crate::Value::List(Vec::new())),
            ("body".to_owned(), crate::Value::Bytes(Vec::new().into())),
            (
                "redirects_followed".to_owned(),
                crate::Value::integer(0).unwrap(),
            ),
        ]));
        let completed = session
            .complete_transient_effect_durably(call_id, outcome)
            .unwrap();
        assert_eq!(
            completed.acknowledgement.through_turn_sequence,
            completed.turn.sequence
        );
        assert!(completed.turn.sequence > first_acknowledgement.through_turn_sequence);
        assert_eq!(session.pending_transient_effect_count(), 0);
        assert_eq!(
            session.output_value_current("last_status").unwrap(),
            crate::Value::integer(207).unwrap()
        );
        session.shutdown().unwrap();
    }

    #[test]
    fn trusted_server_load_rejects_hash_tampering_and_a_rehashed_role_mismatch() {
        let artifact = compile_program_artifact(&server_request(1, server_source())).unwrap();
        let content = artifact.to_content_artifact();

        let mut corrupt = content.clone();
        corrupt.bytes[0] ^= 0xff;
        let diagnostic = ProgramArtifact::from_content_artifact(
            2,
            ProgramCapabilityProfile::TrustedServer,
            corrupt,
        )
        .unwrap_err();
        assert_eq!(diagnostic.phase, ProgramDiagnosticPhase::Artifact);
        assert!(diagnostic.message.contains("digest"));

        let mut stored: StoredProgramArtifact =
            ciborium::de::from_reader(content.bytes.as_slice()).unwrap();
        stored.plan.program_role = ProgramRole::Client;
        stored.plan_digest = boon_plan::plan_sha256(&stored.plan).unwrap();
        let mut bytes = Vec::new();
        ciborium::ser::into_writer(&stored, &mut bytes).unwrap();
        let rehashed = ContentArtifact::new(PROGRAM_ARTIFACT_MEDIA_TYPE, bytes).unwrap();
        let diagnostic = ProgramArtifact::from_content_artifact(
            3,
            ProgramCapabilityProfile::TrustedServer,
            rehashed,
        )
        .unwrap_err();
        assert_eq!(diagnostic.phase, ProgramDiagnosticPhase::Capability);
        assert!(diagnostic.message.contains("requires ProgramRole::Server"));
    }

    #[test]
    fn distributed_program_requires_distinct_namespaces_and_orders_role_artifacts() {
        let client_request = request(
            1,
            r#"
store: [
    increment: SOURCE
]

scene: Scene/Element/text(
    element: []
    style: [width: Fill]
    text: TEXT { Distributed }
)
"#,
        );
        let session_request = session_request(
            1,
            r#"
store: [
    increment: Client/store.increment
    count:
        0 |> HOLD count {
            LATEST {
                increment |> THEN { count + 1 }
            }
        }
]

outputs: [
    count: store.count
]
"#,
        );
        let server_request = server_request(
            1,
            "store: [\n    ready: True\n]\n\noutputs: [\n    ready: store.ready\n]\n",
        );
        let bundle = compile_distributed_program_bundle(&[
            server_request.clone(),
            client_request.clone(),
            session_request.clone(),
        ])
        .unwrap();
        assert_eq!(bundle.artifacts()[0].role(), ProgramRole::Client);
        assert_eq!(bundle.artifacts()[1].role(), ProgramRole::Session);
        assert_eq!(bundle.artifacts()[2].role(), ProgramRole::Server);

        let mut duplicate_namespace_request = server_request;
        duplicate_namespace_request.revision = 2;
        duplicate_namespace_request.application.state_namespace =
            client_request.application.state_namespace.clone();
        let error = compile_distributed_program_bundle(&[
            client_request,
            session_request,
            duplicate_namespace_request,
        ])
        .unwrap_err();
        assert!(error.to_string().contains("distinct state namespaces"));
    }

    #[test]
    fn distributed_client_document_compiles_a_session_value_import() {
        let client = request(
            1,
            r#"
store: [
    increment: SOURCE
]

document: Document/new(
    root: Element/label(
        element: []
        style: []
        label: Session/store.adjusted_count
    )
)
"#,
        );
        let session = session_request(
            1,
            r#"
store: [
    increment: Client/store.increment
    count:
        40 |> HOLD count {
            LATEST {
                increment |> THEN { count + 1 }
            }
        }
    adjusted_count: count + 1
]

outputs: [
    adjusted_count: store.adjusted_count
]
"#,
        );
        let server = server_request(
            1,
            "store: [\n    ready: True\n]\n\noutputs: [\n    ready: store.ready\n]\n",
        );
        let bundle = compile_distributed_program_bundle(&[client, session, server]).unwrap();
        let client_endpoint = bundle
            .artifact(ProgramRole::Client)
            .unwrap()
            .plan()
            .distributed_endpoint
            .as_ref()
            .unwrap();
        assert_eq!(client_endpoint.endpoint.value_imports.len(), 1);
        assert_eq!(
            client_endpoint.endpoint.value_imports[0].producer_role,
            ProgramRole::Session
        );
        assert!(client_endpoint.wire_schema.value_edges.iter().any(|edge| {
            edge.producer_role == ProgramRole::Session && edge.consumer_role == ProgramRole::Client
        }));
    }

    #[test]
    fn document_host_mounts_an_artifact_without_compiling_source() {
        let artifact =
            compile_program_artifact(&request(1, &child_source("Stored child"))).unwrap();
        let mut parent = DocumentFrame::empty("parent");
        let mut program = DocumentNode::new("program", DocumentNodeKind::EmbeddedProgram);
        program.parent = Some(parent.root.clone());
        program.embedded_program = Some(EmbeddedProgramDescriptor {
            artifact_id: artifact.id_text(),
            revision: 1,
            role: boon_plan::ProgramRole::Client,
            capability_profile: ProgramCapabilityProfile::PublicClient,
            session_key: "stored-child".to_owned(),
            mount: true,
            ..EmbeddedProgramDescriptor::default()
        });
        parent
            .nodes
            .get_mut(&parent.root)
            .unwrap()
            .children
            .push(program.id.clone());
        parent.nodes.insert(program.id.clone(), program);

        let (mut host, requests) = ProgramDocumentHost::mount(
            ApplicationIdentity::new("dev.boon.parent", "artifact", "local"),
            &parent,
        );
        assert_eq!(requests.len(), 1);
        assert!(requests[0].is_artifact_load());
        assert!(requests[0].compile.units.is_empty());
        let loaded = ProgramArtifact::from_content_artifact(
            requests[0].compile.revision,
            requests[0].compile.capability_profile,
            artifact.to_content_artifact(),
        );
        let (completion, _) = host.complete(&requests[0].session, &requests[0].request_id, loaded);
        assert_eq!(
            completion,
            ProgramHostCompletion::Program(ProgramCompletion::Activated { revision: 1 })
        );
        assert!(frame_has_text(host.frame(), "Stored child"));
    }

    #[test]
    fn document_host_compiles_bounded_support_source_units() {
        let source = "scene: ProfilePage/render()\n";
        let support_source = r#"
FUNCTION render() {
    Scene/Element/text(
        element: []
        style: [width: Fill]
        text: TEXT { Typed profile }
    )
}
"#;
        let mut parent = parent_frame(1, source);
        parent
            .nodes
            .get_mut(&DocumentNodeId("program".to_owned()))
            .unwrap()
            .embedded_program
            .as_mut()
            .unwrap()
            .support_sources = vec![EmbeddedProgramSourceUnit {
            path: "ProfilePage.bn".to_owned(),
            source: support_source.to_owned(),
            source_digest: crate::sha256_bytes(support_source.as_bytes()),
        }];

        let (mut host, requests) = ProgramDocumentHost::mount(
            ApplicationIdentity::new("dev.boon.parent", "support-source", "local"),
            &parent,
        );
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].compile.units.len(), 2);
        assert_eq!(requests[0].compile.units[0].path, "RUN.bn");
        assert_eq!(requests[0].compile.units[1].path, "ProfilePage.bn");
        let artifact = compile_program_artifact(&requests[0].compile).unwrap();
        assert_eq!(
            artifact.source_digest(),
            crate::sha256_bytes(source.as_bytes())
        );

        let mut changed_support = requests[0].compile.clone();
        changed_support.units[1].source =
            support_source.replace("Typed profile", "Changed profile");
        let changed_artifact = compile_program_artifact(&changed_support).unwrap();
        assert_eq!(changed_artifact.source_digest(), artifact.source_digest());
        assert_ne!(changed_artifact.id(), artifact.id());

        let mut invalid_support = requests[0].compile.clone();
        invalid_support.units[1].source = "FUNCTION render() {\n".to_owned();
        let diagnostic = compile_program_artifact(&invalid_support).unwrap_err();
        assert_eq!(diagnostic.source_path, "ProfilePage.bn");
        assert!(diagnostic.line > 0);

        let (completion, _) =
            host.complete(&requests[0].session, &requests[0].request_id, Ok(artifact));
        assert_eq!(
            completion,
            ProgramHostCompletion::Program(ProgramCompletion::Activated { revision: 1 })
        );
        assert!(frame_has_text(host.frame(), "Typed profile"));
    }

    #[test]
    fn document_host_rejects_source_plus_artifact_descriptors() {
        let source = child_source("Ambiguous child");
        let artifact = compile_program_artifact(&request(1, &source)).unwrap();
        let mut parent = parent_frame(1, &source);
        parent
            .nodes
            .get_mut(&DocumentNodeId("program".to_owned()))
            .unwrap()
            .embedded_program
            .as_mut()
            .unwrap()
            .artifact_id = artifact.id_text();
        let (host, requests) = ProgramDocumentHost::mount(
            ApplicationIdentity::new("dev.boon.parent", "ambiguous", "local"),
            &parent,
        );
        assert!(requests.is_empty());
        assert_eq!(host.diagnostics().len(), 1);
        assert!(
            host.diagnostics()[0]
                .diagnostic
                .message
                .contains("cannot also provide source")
        );
    }

    #[test]
    fn rejected_revision_preserves_the_last_valid_session() {
        let mut controller = ProgramController::new(ProgramCapabilityProfile::PublicClient);
        controller.request(1).unwrap();
        controller.complete(compile_program_artifact(&request(
            1,
            &child_source("Valid"),
        )));
        let valid_digest = controller
            .active()
            .unwrap()
            .artifact()
            .source_digest()
            .to_owned();

        controller.request(2).unwrap();
        let completion = controller.complete(compile_program_artifact(&request(
            2,
            "scene: Scene/Element/text(",
        )));
        assert!(matches!(completion, ProgramCompletion::Rejected { .. }));
        assert_eq!(
            controller.active().unwrap().artifact().source_digest(),
            valid_digest
        );
        assert_eq!(controller.diagnostic().unwrap().revision, 2);
        assert_eq!(controller.diagnostic().unwrap().source_path, "Child.bn");
        assert_eq!(controller.diagnostic().unwrap().line, 1);
        assert!(controller.diagnostic().unwrap().column > 0);
    }

    #[test]
    fn stale_completion_cannot_replace_a_newer_requested_revision() {
        let older = compile_program_artifact(&request(1, &child_source("Older"))).unwrap();
        let newer = compile_program_artifact(&request(2, &child_source("Newer"))).unwrap();
        let mut controller = ProgramController::new(ProgramCapabilityProfile::PublicClient);
        controller.request(1).unwrap();
        controller.request(2).unwrap();
        assert_eq!(
            controller.complete(Ok(older)),
            ProgramCompletion::Stale {
                revision: 1,
                latest_requested_revision: 2,
            }
        );
        assert!(controller.active().is_none());
        assert_eq!(
            controller.complete(Ok(newer)),
            ProgramCompletion::Activated { revision: 2 }
        );
        assert_eq!(controller.active().unwrap().artifact().revision(), 2);
    }

    #[test]
    fn public_client_profile_rejects_host_effects_and_oversized_source() {
        let effectful = compile_program_artifact(&request(
            1,
            r#"
path: TEXT { profile.txt }
contents: File/read_text(path: path)
scene: Scene/Element/text(element: [], style: [], text: contents)
"#,
        ))
        .unwrap_err();
        assert_eq!(effectful.phase, ProgramDiagnosticPhase::Capability);
        assert!(effectful.message.contains("forbids"));

        let oversized =
            "x".repeat(program_limits(ProgramCapabilityProfile::PublicClient).max_source_bytes + 1);
        let diagnostic = compile_program_artifact(&request(2, &oversized)).unwrap_err();
        assert_eq!(diagnostic.phase, ProgramDiagnosticPhase::Request);
        assert!(diagnostic.message.contains("source bundle uses"));
    }

    #[test]
    fn embedded_program_constructor_keeps_typed_private_descriptor() {
        let source = r#"
scene: Scene/Element/program(
    element: []
    style: [width: Fill, height: Fill]
    source: TEXT { child source }
    revision: 7
    capability_profile: PublicClient
)
"#;
        let runtime = LiveRuntime::from_source("embedded-program.bn", source).unwrap();
        let frame = runtime.primary_retained_output_frame().unwrap();
        let node = frame
            .nodes
            .values()
            .find(|node| node.kind == DocumentNodeKind::EmbeddedProgram)
            .expect("embedded program node");
        let descriptor = node.embedded_program.as_ref().expect("program descriptor");

        assert_eq!(descriptor.source, "child source");
        assert_eq!(descriptor.revision, 7);
        assert_eq!(
            descriptor.capability_profile,
            ProgramCapabilityProfile::PublicClient
        );
        assert!(descriptor.session_key.is_empty());
        assert!(descriptor.mount);
        assert_eq!(
            descriptor.source_digest,
            crate::sha256_bytes(b"child source")
        );
        assert!(!format!("{node:?}").contains("child source"));
    }

    #[test]
    fn one_logical_session_can_project_into_multiple_retained_hosts() {
        let source = child_source("Shared child");
        let mut parent = DocumentFrame::empty("parent");
        for host in ["desktop-preview", "mobile-preview"] {
            let mut node = DocumentNode::new(host, DocumentNodeKind::EmbeddedProgram);
            node.parent = Some(parent.root.clone());
            node.embedded_program = Some(EmbeddedProgramDescriptor {
                source: source.clone(),
                source_digest: crate::sha256_bytes(source.as_bytes()),
                revision: 1,
                role: boon_plan::ProgramRole::Client,
                capability_profile: ProgramCapabilityProfile::PublicClient,
                session_key: "public-page".to_owned(),
                mount: true,
                ..EmbeddedProgramDescriptor::default()
            });
            parent
                .nodes
                .get_mut(&parent.root)
                .unwrap()
                .children
                .push(node.id.clone());
            parent.nodes.insert(node.id.clone(), node);
        }

        let (mut host, requests) = ProgramDocumentHost::mount(
            ApplicationIdentity::new("dev.boon.parent", "shared", "local"),
            &parent,
        );
        assert_eq!(requests.len(), 1);
        assert_eq!(
            requests[0].session,
            ProgramSessionId("public-page".to_owned())
        );
        assert_eq!(host.programs.len(), 1);
        assert_eq!(host.projections.len(), 2);

        host.complete(
            &requests[0].session,
            &requests[0].request_id,
            compile_program_artifact(&requests[0].compile),
        );
        assert_eq!(
            host.frame()
                .nodes
                .values()
                .filter(|node| node
                    .text
                    .as_ref()
                    .is_some_and(|text| text.text == "Shared child"))
                .count(),
            2
        );
    }

    #[test]
    fn document_host_composes_child_and_preserves_it_across_invalid_source() {
        let first_parent = parent_frame(1, &child_source("first"));
        let (mut host, requests) = ProgramDocumentHost::mount(
            ApplicationIdentity::new("dev.boon.parent", "test", "local"),
            &first_parent,
        );
        assert_eq!(requests.len(), 1);
        let first = compile_program_artifact(&requests[0].compile);
        let (completion, update) =
            host.complete(&requests[0].session, &requests[0].request_id, first);
        assert_eq!(
            completion,
            ProgramHostCompletion::Program(ProgramCompletion::Activated { revision: 1 })
        );
        assert!(!update.patches.is_empty());
        assert!(frame_has_text(host.frame(), "first"));

        let invalid_parent = parent_frame(2, "scene: Missing/constructor(");
        let invalid = host.reconcile(&invalid_parent);
        assert_eq!(invalid.requests.len(), 1);
        let failed = compile_program_artifact(&invalid.requests[0].compile);
        let (completion, _) = host.complete(
            &invalid.requests[0].session,
            &invalid.requests[0].request_id,
            failed,
        );
        assert!(matches!(
            completion,
            ProgramHostCompletion::Program(ProgramCompletion::Rejected { .. })
        ));
        assert_eq!(host.diagnostics().len(), 1);
        assert!(frame_has_text(host.frame(), "first"));
        let empty = host.reconcile(&parent_frame(3, ""));
        assert!(empty.requests.is_empty());
        assert_eq!(empty.rejections.len(), 1);
        assert!(
            empty.rejections[0]
                .diagnostic
                .message
                .contains("requires source")
        );
    }

    #[test]
    fn document_host_routes_namespaced_input_into_the_child_session() {
        let parent = parent_frame(1, interactive_child_source());
        let (mut host, requests) = ProgramDocumentHost::mount(
            ApplicationIdentity::new("dev.boon.parent", "input", "local"),
            &parent,
        );
        host.complete(
            &requests[0].session,
            &requests[0].request_id,
            compile_program_artifact(&requests[0].compile),
        );
        let route = host
            .frame()
            .nodes
            .values()
            .find(|node| node.text.as_ref().is_some_and(|text| text.text == "Before"))
            .and_then(|node| node.primary_source_binding())
            .map(|binding| binding.source_path.clone())
            .expect("composed child button source route");
        assert!(host.owns_source_route(&route));

        let (_, patches) = host
            .dispatch(1, &route, None, SourcePayload::default())
            .unwrap();
        assert!(!patches.is_empty());
        assert!(
            host.frame()
                .nodes
                .values()
                .any(|node| { node.text.as_ref().is_some_and(|text| text.text == "After") })
        );
    }

    #[test]
    fn document_host_rejects_stale_completion_then_activates_latest() {
        let first_parent = parent_frame(1, &child_source("first"));
        let (mut host, first_requests) = ProgramDocumentHost::mount(
            ApplicationIdentity::new("dev.boon.parent", "stale", "local"),
            &first_parent,
        );
        let first = compile_program_artifact(&first_requests[0].compile);
        host.complete(
            &first_requests[0].session,
            &first_requests[0].request_id,
            first,
        );

        let second_parent = parent_frame(2, &child_source("second"));
        let second = host.reconcile(&second_parent).requests.remove(0);
        let second_artifact = compile_program_artifact(&second.compile);
        let third_parent = parent_frame(3, &child_source("third"));
        let third = host.reconcile(&third_parent).requests.remove(0);
        let third_artifact = compile_program_artifact(&third.compile);

        let (stale, stale_update) =
            host.complete(&second.session, &second.request_id, second_artifact);
        assert_eq!(
            stale,
            ProgramHostCompletion::Superseded {
                session: second.session.clone(),
                request_id: second.request_id.clone(),
            }
        );
        assert!(stale_update.patches.is_empty());
        let (latest, _) = host.complete(&third.session, &third.request_id, third_artifact);
        assert_eq!(
            latest,
            ProgramHostCompletion::Program(ProgramCompletion::Activated { revision: 3 })
        );
        assert!(frame_has_text(host.frame(), "third"));
    }

    #[test]
    fn incremental_parent_descriptor_patch_schedules_one_scoped_request() {
        let first_parent = parent_frame(1, &child_source("first"));
        let (mut host, requests) = ProgramDocumentHost::mount(
            ApplicationIdentity::new("dev.boon.parent", "incremental", "local"),
            &first_parent,
        );
        host.complete(
            &requests[0].session,
            &requests[0].request_id,
            compile_program_artifact(&requests[0].compile),
        );

        let next_parent = parent_frame(2, &child_source("second"));
        let parent_patches = crate::document::diff_frames(&first_parent, &next_parent);
        assert!(
            parent_patches
                .iter()
                .any(|patch| matches!(patch, DocumentPatch::SetEmbeddedProgram { .. }))
        );
        let stats_before = host.stats();
        let previous = host.frame().clone();
        let update = host.reconcile_with_parent_patches(&next_parent, parent_patches.clone());

        assert_eq!(update.requests.len(), 1);
        assert_eq!(update.requests[0].compile.revision, 2);
        assert_eq!(update.patches, parent_patches);
        assert_eq!(replay_patches(previous, update.patches), *host.frame());
        let stats_after = host.stats();
        assert_eq!(
            stats_after.scoped_parent_patch_count,
            stats_before
                .scoped_parent_patch_count
                .saturating_add(parent_patches.len() as u64)
        );
        assert_eq!(
            stats_after.scoped_projection_refresh_count,
            stats_before.scoped_projection_refresh_count
        );
        assert_eq!(
            stats_after.full_reconcile_count,
            stats_before.full_reconcile_count
        );

        let request = &update.requests[0];
        host.complete(
            &request.session,
            &request.request_id,
            compile_program_artifact(&request.compile),
        );
        assert_eq!(
            host.stats().scoped_projection_refresh_count,
            stats_after
                .scoped_projection_refresh_count
                .saturating_add(1)
        );
    }

    #[test]
    fn projection_patch_replay_preserves_parent_owned_host_children() {
        let mut parent = parent_frame(1, &child_source("child"));
        let host_id = DocumentNodeId("program".to_owned());
        let mut owned = DocumentNode::new("owned", DocumentNodeKind::Text);
        owned.parent = Some(host_id.clone());
        owned.text = Some(TextValue {
            text: "parent owned".to_owned(),
        });
        parent.nodes.insert(owned.id.clone(), owned);
        parent
            .nodes
            .get_mut(&host_id)
            .unwrap()
            .children
            .push(DocumentNodeId("owned".to_owned()));

        let (mut host, requests) = ProgramDocumentHost::mount(
            ApplicationIdentity::new("dev.boon.parent", "owned-child", "local"),
            &parent,
        );
        let previous = host.frame().clone();
        let (_, update) = host.complete(
            &requests[0].session,
            &requests[0].request_id,
            compile_program_artifact(&requests[0].compile),
        );
        assert!(update.patches.iter().any(|patch| matches!(
            patch,
            DocumentPatch::MoveChild {
                new_parent,
                index: 1,
                ..
            } if *new_parent == host_id
        )));
        assert_eq!(replay_patches(previous, update.patches), *host.frame());
        assert_eq!(
            host.frame().nodes[&host_id].children[0],
            DocumentNodeId("owned".to_owned())
        );
    }

    #[test]
    fn bootstrap_activates_before_invalid_current_and_remains_last_valid() {
        let bootstrap_source = child_source("bootstrap");
        let mut parent = parent_frame(2, "scene: Missing/constructor(");
        let descriptor = parent
            .nodes
            .get_mut(&DocumentNodeId("program".to_owned()))
            .unwrap()
            .embedded_program
            .as_mut()
            .unwrap();
        descriptor.bootstrap_source = bootstrap_source.clone();
        descriptor.bootstrap_source_digest = crate::sha256_bytes(bootstrap_source.as_bytes());
        descriptor.bootstrap_revision = 1;

        let (mut host, requests) = ProgramDocumentHost::mount(
            ApplicationIdentity::new("dev.boon.parent", "bootstrap", "local"),
            &parent,
        );
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].compile.revision, 1);
        let (completion, update) = host.complete(
            &requests[0].session,
            &requests[0].request_id,
            compile_program_artifact(&requests[0].compile),
        );
        assert_eq!(
            completion,
            ProgramHostCompletion::Program(ProgramCompletion::Activated { revision: 1 })
        );
        assert!(update.bootstrap);

        assert_eq!(update.requests.len(), 1);
        assert_eq!(update.requests[0].compile.revision, 2);
        let (completion, update) = host.complete(
            &update.requests[0].session,
            &update.requests[0].request_id,
            compile_program_artifact(&update.requests[0].compile),
        );
        assert!(matches!(
            completion,
            ProgramHostCompletion::Program(ProgramCompletion::Rejected { .. })
        ));
        assert!(update.patches.is_empty());
        assert_eq!(
            host.active_artifact(&ProgramSessionId("program".to_owned()))
                .unwrap()
                .revision(),
            1
        );
    }

    #[test]
    fn stored_bootstrap_artifact_mounts_before_invalid_current_without_recompile() {
        let bootstrap_source = child_source("stored bootstrap");
        let bootstrap_parent = parent_frame(1, &bootstrap_source);
        let (_, bootstrap_requests) = ProgramDocumentHost::mount(
            ApplicationIdentity::new("dev.boon.parent", "bootstrap-build", "local"),
            &bootstrap_parent,
        );
        let artifact = compile_program_artifact(&bootstrap_requests[0].compile).unwrap();

        let mut parent = parent_frame(2, "scene: Missing/constructor(");
        let descriptor = parent
            .nodes
            .get_mut(&DocumentNodeId("program".to_owned()))
            .unwrap()
            .embedded_program
            .as_mut()
            .unwrap();
        descriptor.bootstrap_artifact_id = artifact.id_text();
        descriptor.bootstrap_revision = 1;

        let (mut host, requests) = ProgramDocumentHost::mount(
            ApplicationIdentity::new("dev.boon.parent", "artifact-bootstrap", "local"),
            &parent,
        );
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].artifact_id, Some(artifact.id()));
        assert!(requests[0].compile.units.is_empty());
        assert!(host.request_is_artifact_load(&requests[0].session, &requests[0].request_id));
        assert!(
            host.request_artifact_ownership(&requests[0].session, &requests[0].request_id)
                .is_none()
        );
        let (completion, update) =
            host.complete(&requests[0].session, &requests[0].request_id, Ok(artifact));
        assert_eq!(
            completion,
            ProgramHostCompletion::Program(ProgramCompletion::Activated { revision: 1 })
        );
        assert!(update.bootstrap);
        assert_eq!(update.requests.len(), 1);
        assert_eq!(update.requests[0].compile.revision, 2);
        assert!(update.requests[0].artifact_id.is_none());

        let (completion, update) = host.complete(
            &update.requests[0].session,
            &update.requests[0].request_id,
            compile_program_artifact(&update.requests[0].compile),
        );
        assert!(matches!(
            completion,
            ProgramHostCompletion::Program(ProgramCompletion::Rejected { .. })
        ));
        assert!(update.patches.is_empty());
        assert_eq!(
            host.active_artifact(&ProgramSessionId("program".to_owned()))
                .unwrap()
                .revision(),
            1
        );
    }

    #[test]
    fn stored_bootstrap_artifact_can_restore_the_exact_current_revision() {
        let source = child_source("exact current");
        let parent = parent_frame(2, &source);
        let application =
            ApplicationIdentity::new("dev.boon.parent", "exact-artifact-bootstrap", "local");
        let (_, build_requests) = ProgramDocumentHost::mount(application.clone(), &parent);
        let artifact = compile_program_artifact(&build_requests[0].compile).unwrap();

        let mut restored_parent = parent;
        let descriptor = restored_parent
            .nodes
            .get_mut(&DocumentNodeId("program".to_owned()))
            .unwrap()
            .embedded_program
            .as_mut()
            .unwrap();
        descriptor.bootstrap_artifact_id = artifact.id_text();
        descriptor.bootstrap_revision = 2;

        let (mut host, requests) = ProgramDocumentHost::mount(application, &restored_parent);
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].artifact_id, Some(artifact.id()));
        let (completion, update) =
            host.complete(&requests[0].session, &requests[0].request_id, Ok(artifact));
        assert_eq!(
            completion,
            ProgramHostCompletion::Program(ProgramCompletion::Activated { revision: 2 })
        );
        assert!(update.bootstrap);
        assert!(update.requests.is_empty());
        assert_eq!(
            host.active_artifact(&ProgramSessionId("program".to_owned()))
                .unwrap()
                .revision(),
            2
        );
        assert!(frame_has_text(host.frame(), "exact current"));
    }

    #[test]
    fn bootstrap_metadata_change_does_not_recompile_current_program() {
        let source = child_source("current");
        let first_parent = parent_frame(2, &source);
        let (mut host, requests) = ProgramDocumentHost::mount(
            ApplicationIdentity::new("dev.boon.parent", "metadata", "local"),
            &first_parent,
        );
        host.complete(
            &requests[0].session,
            &requests[0].request_id,
            compile_program_artifact(&requests[0].compile),
        );

        let mut next_parent = first_parent.clone();
        let bootstrap_source = child_source("older");
        let descriptor = next_parent
            .nodes
            .get_mut(&DocumentNodeId("program".to_owned()))
            .unwrap()
            .embedded_program
            .as_mut()
            .unwrap();
        descriptor.bootstrap_source = bootstrap_source.clone();
        descriptor.bootstrap_source_digest = crate::sha256_bytes(bootstrap_source.as_bytes());
        descriptor.bootstrap_revision = 1;
        let patches = crate::document::diff_frames(&first_parent, &next_parent);
        let update = host.reconcile_with_parent_patches(&next_parent, patches);

        assert!(update.requests.is_empty());
        assert_eq!(
            host.active_artifact(&ProgramSessionId("program".to_owned()))
                .unwrap()
                .revision(),
            2
        );
    }

    #[test]
    fn invalid_bootstrap_revision_fails_closed_without_request() {
        let source = child_source("current");
        let mut parent = parent_frame(2, &source);
        let descriptor = parent
            .nodes
            .get_mut(&DocumentNodeId("program".to_owned()))
            .unwrap()
            .embedded_program
            .as_mut()
            .unwrap();
        descriptor.bootstrap_source = child_source("not older");
        descriptor.bootstrap_source_digest =
            crate::sha256_bytes(descriptor.bootstrap_source.as_bytes());
        descriptor.bootstrap_revision = 3;

        let (host, requests) = ProgramDocumentHost::mount(
            ApplicationIdentity::new("dev.boon.parent", "bad-bootstrap", "local"),
            &parent,
        );
        assert!(requests.is_empty());
        assert!(
            host.diagnostics()[0]
                .diagnostic
                .message
                .contains("must not exceed")
        );
    }
}
