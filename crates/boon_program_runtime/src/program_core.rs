use boon_runtime::{
    ApplicationIdentity, DocumentFrame, DocumentPatch, LiveRuntime, MachineTemplate,
    ProgramCapabilityProfile, RowId, RuntimeSourceUnit, SessionOptions, SourcePayload,
};
use boon_compiler::{
    COMPILER_ID, CompileProfile, CompilerSourceUnit, DistributedCompilerProgram,
    compile_distributed_runtime_source_programs,
    compile_runtime_source_units_to_machine_plan_for_role_with_identity,
    diagnose_runtime_source_units,
};
use boon_contract::{CanonicalSourceBundleV1, SourceBundleDigestV1, SourceBundleUnit};
use boon_document_model::{
    DocumentNodeId, DocumentNodeKind, EMBEDDED_PROGRAM_ENTRY_PATH, EmbeddedProgramDescriptor,
    ProgramArtifactRetention, ScrollRootId, SourceBindingId,
};
use boon_persistence::{
    ContentArtifact, ContentArtifactId, ContentArtifactOwnerId, ContentArtifactRetention,
    validate_content_artifact,
};
use boon_plan::{
    DocumentConstructor, EffectBarrier, EffectReplay, MachinePlan, OutputContractKind, ProgramRole,
    SourceRouteToken, TargetProfile,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::io::Cursor;
use std::ops::Range;
use std::sync::Arc;

const MAX_DIAGNOSTIC_BYTES: usize = 4 * 1024;
const MAX_TRUSTED_PACKAGE_SOURCE_UNITS: usize = 256;
const MAX_TRUSTED_PACKAGE_SOURCE_BYTES: usize = 8 * 1024 * 1024;
const PROGRAM_ARTIFACT_FORMAT: u32 = 3;
const PROGRAM_ARTIFACT_MEDIA_TYPE: &str = "application/vnd.boon.machine-plan+cbor;version=3";

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

pub(crate) fn program_limits(profile: ProgramCapabilityProfile) -> ProgramLimits {
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

    pub fn start(revision: u64, message: impl Into<String>) -> Self {
        Self::new(revision, ProgramDiagnosticPhase::Start, message)
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

#[derive(Clone)]
pub struct ProgramArtifact {
    id: ContentArtifactId,
    revision: u64,
    source_bundle_digest_v1: SourceBundleDigestV1,
    plan_digest: String,
    capability_profile: ProgramCapabilityProfile,
    compile_profile: CompileProfile,
    plan: Arc<MachinePlan>,
    template: MachineTemplate,
    content: Arc<ContentArtifact>,
}

impl fmt::Debug for ProgramArtifact {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProgramArtifact")
            .field("id", &self.id)
            .field("revision", &self.revision)
            .field("source_bundle_digest_v1", &self.source_bundle_digest_v1)
            .field("plan_digest", &self.plan_digest)
            .field("capability_profile", &self.capability_profile)
            .field("compile_profile", &self.compile_profile)
            .field("plan", &self.plan)
            .field("content", &self.content)
            .finish()
    }
}

impl PartialEq for ProgramArtifact {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id && self.revision == other.revision
    }
}

impl Eq for ProgramArtifact {}

impl ProgramArtifact {
    pub fn session_id(&self) -> ProgramSessionId {
        deterministic_program_session_id(self)
    }

    pub fn id(&self) -> ContentArtifactId {
        self.id
    }

    pub fn id_text(&self) -> String {
        self.id.to_string()
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub const fn source_bundle_digest_v1(&self) -> SourceBundleDigestV1 {
        self.source_bundle_digest_v1
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

    pub fn machine_template(&self) -> &MachineTemplate {
        &self.template
    }

    #[doc(hidden)]
    pub fn max_runtime_work_units_per_transaction(&self) -> u64 {
        program_limits(self.capability_profile).max_runtime_work_units_per_transaction
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
#[serde(deny_unknown_fields)]
struct StoredProgramArtifact {
    format: u32,
    source_bundle_digest_v1: SourceBundleDigestV1,
    compiler_id: String,
    target_profile: TargetProfile,
    capability_profile: ProgramCapabilityProfile,
    plan_digest: String,
    plan: MachinePlan,
}

fn encode_program_artifact(
    revision: u64,
    source_bundle_digest_v1: SourceBundleDigestV1,
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
        source_bundle_digest_v1,
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
    let mut reader = Cursor::new(artifact.bytes.as_slice());
    let stored: StoredProgramArtifact =
        ciborium::de::from_reader(&mut reader).map_err(|error| {
            ProgramDiagnostic::new(
                revision,
                ProgramDiagnosticPhase::Artifact,
                format!("decode immutable program artifact: {error}"),
            )
        })?;
    if reader.position() != artifact.bytes.len() as u64 {
        return Err(ProgramDiagnostic::new(
            revision,
            ProgramDiagnosticPhase::Artifact,
            "immutable program artifact contains trailing CBOR data",
        ));
    }
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
    let plan = Arc::new(stored.plan);
    let template = MachineTemplate::new_shared(Arc::clone(&plan)).map_err(|error| {
        ProgramDiagnostic::new(
            revision,
            ProgramDiagnosticPhase::Artifact,
            error.to_string(),
        )
    })?;
    Ok(ProgramArtifact {
        id: artifact.id,
        revision,
        source_bundle_digest_v1: stored.source_bundle_digest_v1,
        plan_digest: stored.plan_digest,
        capability_profile: stored.capability_profile,
        compile_profile: CompileProfile::default(),
        plan,
        template,
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
    let source_bundle = canonical_source_bundle(request)?;
    let units = source_bundle
        .units()
        .iter()
        .map(|unit| CompilerSourceUnit {
            path: unit.path().to_owned(),
            source: unit.source().to_owned(),
        })
        .collect::<Vec<_>>();
    let compiled = compile_runtime_source_units_to_machine_plan_for_role_with_identity(
        source_bundle.entrypoint(),
        &units,
        TargetProfile::SoftwareBounded,
        request.role,
        request.application.clone(),
    )
    .map_err(|error| {
        let fallback = error.to_string();
        let location = diagnose_runtime_source_units(source_bundle.entrypoint(), &units)
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
    artifact_from_compiled(request, compiled)
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
    let source_bundles = requests
        .iter()
        .map(canonical_source_bundle)
        .collect::<Result<Vec<_>, _>>()?;
    let compiler_programs = requests
        .iter()
        .zip(&source_bundles)
        .map(|(request, source_bundle)| DistributedCompilerProgram {
            revision: request.revision,
            role: request.role,
            source_label: source_bundle.entrypoint().to_owned(),
            units: source_bundle
                .units()
                .iter()
                .map(|unit| CompilerSourceUnit {
                    path: unit.path().to_owned(),
                    source: unit.source().to_owned(),
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
        let request_index = requests
            .iter()
            .position(|request| request.role == role)
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
        let request = &requests[request_index];
        artifacts.push(artifact_from_compiled(request, compiled)?);
    }
    DistributedProgramBundle::new(artifacts).map_err(|error| {
        ProgramDiagnostic::new(
            revision,
            ProgramDiagnosticPhase::Artifact,
            error.to_string(),
        )
    })
}

fn canonical_source_bundle(
    request: &ProgramCompileRequest,
) -> Result<CanonicalSourceBundleV1<'_>, ProgramDiagnostic> {
    CanonicalSourceBundleV1::new(
        &request.entry_path,
        request
            .units
            .iter()
            .map(|unit| SourceBundleUnit::new(&unit.path, &unit.source)),
    )
    .map_err(|error| {
        ProgramDiagnostic::new(
            request.revision,
            ProgramDiagnosticPhase::Request,
            format!("invalid source bundle identity: {error}"),
        )
    })
}

fn artifact_from_compiled(
    request: &ProgramCompileRequest,
    compiled: boon_compiler::CompiledMachinePlanFromSource,
) -> Result<ProgramArtifact, ProgramDiagnostic> {
    let source_bundle_digest_v1 = compiled.ir.source_bundle_digest_v1();
    validate_plan(request.revision, request.capability_profile, &compiled.plan)?;
    let content = encode_program_artifact(
        request.revision,
        source_bundle_digest_v1,
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
    let compile_profile = compiled.profile;
    let plan = Arc::new(compiled.plan);
    let template = MachineTemplate::new_shared(Arc::clone(&plan)).map_err(|error| {
        ProgramDiagnostic::new(
            request.revision,
            ProgramDiagnosticPhase::Artifact,
            error.to_string(),
        )
    })?;
    Ok(ProgramArtifact {
        id: content.id,
        revision: request.revision,
        source_bundle_digest_v1,
        plan_digest,
        capability_profile: request.capability_profile,
        compile_profile,
        plan,
        template,
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
        let id = artifact.session_id();
        let runtime = LiveRuntime::from_machine_template(
            artifact.machine_template(),
            SessionOptions {
                program_revision: artifact.revision(),
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

    #[doc(hidden)]
    pub fn from_runtime_parts(
        id: ProgramSessionId,
        artifact: ProgramArtifact,
        runtime: LiveRuntime,
        next_source_sequence: u64,
    ) -> Self {
        Self {
            id,
            artifact,
            runtime,
            next_source_sequence,
        }
    }

    #[doc(hidden)]
    pub fn into_runtime(self) -> LiveRuntime {
        self.runtime
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
        turn: Option<&boon_runtime::RuntimeTurn>,
    ) -> Result<Self, boon_runtime::DistributedRuntimeError> {
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
        turn: Option<&boon_runtime::RuntimeTurn>,
    ) -> Result<u64, boon_runtime::DistributedRuntimeError> {
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
        turn: Option<&boon_runtime::RuntimeTurn>,
        evaluation: &Self,
    ) -> Result<(), boon_runtime::DistributedRuntimeError> {
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
    ) -> boon_runtime::RuntimeResult<ProgramSessionDispatch> {
        let source_sequence = self.next_source_sequence;
        let next_source_sequence = source_sequence
            .checked_add(1)
            .ok_or("program source sequence overflow")?;
        let event = self.runtime.source_event_for_path(
            source_sequence,
            source_path,
            target.as_slice(),
            payload,
        )?;
        let runtime_turn = self.runtime.dispatch(event)?;
        self.next_source_sequence = next_source_sequence;
        Ok(ProgramSessionDispatch {
            source_sequence,
            source_path: source_path.to_owned(),
            runtime_turn,
        })
    }

    pub fn root_value_current(&mut self, name: &str) -> boon_runtime::RuntimeResult<boon_runtime::Value> {
        self.runtime.root_value_current(name)
    }

    pub fn output_value_current(&mut self, name: &str) -> boon_runtime::RuntimeResult<boon_runtime::Value> {
        self.runtime.output_value_current(name)
    }

    pub fn update_session_context(
        &mut self,
        connection_status: boon_runtime::SessionConnectionStatus,
        principal: boon_runtime::SessionPrincipal,
    ) -> boon_runtime::RuntimeResult<Option<boon_runtime::RuntimeTurn>> {
        self.runtime
            .update_session_context(connection_status, principal)
    }

    pub fn complete_transient_effect(
        &mut self,
        call_id: boon_runtime::TransientEffectCallId,
        outcome: boon_runtime::Value,
    ) -> boon_runtime::RuntimeResult<boon_runtime::RuntimeTurn> {
        self.runtime.complete_transient_effect(call_id, outcome)
    }

    pub fn deliver_transient_effect_result(
        &mut self,
        call_id: boon_runtime::TransientEffectCallId,
        result_sequence: u64,
        outcome: boon_runtime::Value,
    ) -> boon_runtime::RuntimeResult<boon_runtime::RuntimeTurn> {
        self.runtime
            .deliver_transient_effect_result(call_id, result_sequence, outcome)
    }

    pub fn cancel_transient_effect(
        &mut self,
        call_id: boon_runtime::TransientEffectCallId,
    ) -> boon_runtime::RuntimeResult<bool> {
        self.runtime.cancel_transient_effect(call_id)
    }

    pub fn pending_transient_effect_count(&self) -> usize {
        self.runtime.pending_transient_effect_count()
    }

    pub fn pending_transient_effect_credits(
        &self,
        call_id: boon_runtime::TransientEffectCallId,
    ) -> Option<u32> {
        self.runtime.pending_transient_effect_credits(call_id)
    }

    pub fn update_distributed_import(
        &mut self,
        import_id: boon_plan::ImportId,
        content_revision: u64,
        value: boon_runtime::Value,
    ) -> boon_runtime::RuntimeResult<Option<boon_runtime::RuntimeTurn>> {
        self.runtime
            .update_distributed_import(import_id, content_revision, value)
    }

    pub fn distributed_import_revision(&self, import_id: boon_plan::ImportId) -> Option<u64> {
        self.runtime.distributed_import_revision(import_id)
    }

    pub fn distributed_export_value_current(
        &mut self,
        export_id: boon_plan::ExportId,
    ) -> boon_runtime::RuntimeResult<boon_runtime::Value> {
        self.runtime.distributed_export_value_current(export_id)
    }

    pub fn evaluate_distributed_function_instance_unsettled(
        &mut self,
        call_site_id: boon_plan::RemoteCallSiteId,
        call_instance_id: boon_plan::DistributedCallInstanceId,
        export_id: boon_plan::ExportId,
        content_revision: u64,
        arguments: BTreeMap<boon_plan::DistributedArgumentId, boon_runtime::Value>,
    ) -> boon_runtime::RuntimeResult<(boon_runtime::Value, Option<boon_runtime::RuntimeTurn>)> {
        self.runtime
            .evaluate_distributed_function_instance_unsettled(
                call_site_id,
                call_instance_id,
                export_id,
                content_revision,
                arguments,
            )
    }

    pub fn distributed_call_instances_current(
        &mut self,
        call_site_id: boon_plan::RemoteCallSiteId,
    ) -> boon_runtime::RuntimeResult<Vec<boon_runtime::DistributedCurrentCallInstance>> {
        self.runtime
            .distributed_call_instances_current(call_site_id)
    }

    pub fn distributed_producer_call_result_current(
        &mut self,
        call_site_id: boon_plan::RemoteCallSiteId,
        call_instance_id: boon_plan::DistributedCallInstanceId,
    ) -> boon_runtime::RuntimeResult<boon_runtime::Value> {
        self.runtime
            .distributed_producer_call_result_current(call_site_id, call_instance_id)
    }

    pub fn update_distributed_call_result_instance_unsettled(
        &mut self,
        call_site_id: boon_plan::RemoteCallSiteId,
        call_instance_id: boon_plan::DistributedCallInstanceId,
        content_revision: u64,
        value: boon_runtime::Value,
    ) -> boon_runtime::RuntimeResult<Option<boon_runtime::RuntimeTurn>> {
        self.runtime
            .update_distributed_call_result_instance_unsettled(
                call_site_id,
                call_instance_id,
                content_revision,
                value,
            )
    }

    pub fn drop_producer_call_instance_unsettled(
        &mut self,
        call_site_id: boon_plan::RemoteCallSiteId,
        call_instance_id: boon_plan::DistributedCallInstanceId,
    ) -> boon_runtime::RuntimeResult<Option<boon_runtime::RuntimeTurn>> {
        self.runtime
            .drop_producer_call_instance_unsettled(call_site_id, call_instance_id)
    }
}

impl boon_runtime::DistributedServerMachine for ProgramSession {
    type EvaluationMachine = ProgramSession;

    fn fork_prepared_evaluation(
        &self,
        turn: Option<&boon_runtime::RuntimeTurn>,
    ) -> Result<Self::EvaluationMachine, boon_runtime::DistributedRuntimeError> {
        self.fork_distributed_server_evaluation(turn)
    }

    fn install_evaluation(
        &mut self,
        evaluation: Self::EvaluationMachine,
    ) -> Result<(), boon_runtime::DistributedRuntimeError> {
        self.validate_distributed_server_evaluation(None, &evaluation)?;
        self.install_distributed_server_evaluation(evaluation);
        Ok(())
    }

    fn commit_prepared_evaluation(
        &mut self,
        turn: boon_runtime::RuntimeTurn,
        evaluation: Self::EvaluationMachine,
    ) -> Result<boon_runtime::RuntimeTurn, boon_runtime::DistributedRuntimeError> {
        if let Err(error) = self.validate_distributed_server_evaluation(Some(&turn), &evaluation) {
            return match self.runtime.rollback_unsettled_turn() {
                Ok(()) => Err(error),
                Err(rollback) => Err(distributed_machine_error(format!(
                    "{error}; rollback failed: {rollback}"
                ))),
            };
        }
        self.install_distributed_server_evaluation(evaluation);
        Ok(turn)
    }

    fn event_for_path(
        &self,
        path: &str,
        payload: SourcePayload,
    ) -> Result<boon_runtime::SourceEvent, boon_runtime::DistributedRuntimeError> {
        self.runtime
            .source_event_for_path(self.next_source_sequence, path, &[], payload)
            .map_err(distributed_machine_error)
    }

    fn event_for_source(
        &self,
        source: boon_plan::SourceId,
        payload: SourcePayload,
    ) -> Result<boon_runtime::SourceEvent, boon_runtime::DistributedRuntimeError> {
        self.runtime
            .source_event_by_id(self.next_source_sequence, source, payload)
            .map_err(distributed_machine_error)
    }

    fn event_for_route(
        &self,
        route: boon_plan::SourceRouteToken,
        payload: SourcePayload,
    ) -> Result<boon_runtime::SourceEvent, boon_runtime::DistributedRuntimeError> {
        self.runtime
            .source_event(self.next_source_sequence, route, payload)
            .map_err(distributed_machine_error)
    }

    fn prepare_dispatch(
        &mut self,
        event: boon_runtime::SourceEvent,
    ) -> Result<boon_runtime::RuntimeTurn, boon_runtime::DistributedRuntimeError> {
        self.next_source_sequence
            .checked_add(1)
            .ok_or_else(|| distributed_machine_error("program source sequence overflow"))?;
        self.runtime
            .dispatch_unsettled(event)
            .map_err(distributed_machine_error)
    }

    fn export_if_current(
        &mut self,
        export_id: boon_plan::ExportId,
    ) -> Result<Option<boon_runtime::Value>, boon_runtime::DistributedRuntimeError> {
        self.runtime
            .distributed_export_value_if_current(export_id)
            .map_err(distributed_machine_error)
    }

    fn current_call_instances(
        &mut self,
        call_site_id: boon_plan::RemoteCallSiteId,
    ) -> Result<Vec<boon_runtime::DistributedCurrentCallInstance>, boon_runtime::DistributedRuntimeError> {
        self.runtime
            .distributed_call_instances_current(call_site_id)
            .map_err(distributed_machine_error)
    }

    fn producer_call_result_current(
        &mut self,
        call_site_id: boon_plan::RemoteCallSiteId,
        call_instance_id: boon_plan::DistributedCallInstanceId,
    ) -> Result<boon_runtime::Value, boon_runtime::DistributedRuntimeError> {
        self.runtime
            .distributed_producer_call_result_current(call_site_id, call_instance_id)
            .map_err(distributed_machine_error)
    }

    fn evaluate_function_instance(
        &mut self,
        call_site_id: boon_plan::RemoteCallSiteId,
        call_instance_id: boon_plan::DistributedCallInstanceId,
        export_id: boon_plan::ExportId,
        demand_revision: u64,
        arguments: BTreeMap<boon_plan::DistributedArgumentId, boon_runtime::Value>,
    ) -> Result<(boon_runtime::Value, Option<boon_runtime::RuntimeTurn>), boon_runtime::DistributedRuntimeError> {
        self.runtime
            .evaluate_distributed_function_instance_unsettled(
                call_site_id,
                call_instance_id,
                export_id,
                demand_revision,
                arguments,
            )
            .map_err(distributed_machine_error)
    }

    fn update_current_call_result_instance(
        &mut self,
        call_site_id: boon_plan::RemoteCallSiteId,
        call_instance_id: boon_plan::DistributedCallInstanceId,
        content_revision: u64,
        value: boon_runtime::Value,
    ) -> Result<Option<boon_runtime::RuntimeTurn>, boon_runtime::DistributedRuntimeError> {
        self.runtime
            .update_distributed_call_result_instance_unsettled(
                call_site_id,
                call_instance_id,
                content_revision,
                value,
            )
            .map_err(distributed_machine_error)
    }

    fn drop_producer_call_instance(
        &mut self,
        call_site_id: boon_plan::RemoteCallSiteId,
        call_instance_id: boon_plan::DistributedCallInstanceId,
    ) -> Result<Option<boon_runtime::RuntimeTurn>, boon_runtime::DistributedRuntimeError> {
        self.runtime
            .drop_producer_call_instance_unsettled(call_site_id, call_instance_id)
            .map_err(distributed_machine_error)
    }

    fn replace_distributed_context(
        &mut self,
        session_context: boon_runtime::SessionContext,
        imports: Vec<boon_runtime::DistributedImportUpdate>,
    ) -> Result<Option<boon_runtime::RuntimeTurn>, boon_runtime::DistributedRuntimeError> {
        self.runtime
            .replace_distributed_execution_context(session_context, imports)
            .map_err(distributed_machine_error)
    }

    fn prepare_transient_effect_completion(
        &mut self,
        call_id: boon_runtime::TransientEffectCallId,
        outcome: boon_runtime::Value,
    ) -> Result<boon_runtime::RuntimeTurn, boon_runtime::DistributedRuntimeError> {
        self.runtime
            .complete_transient_effect_unsettled(call_id, outcome)
            .map_err(distributed_machine_error)
    }

    fn prepare_transient_effect_result(
        &mut self,
        call_id: boon_runtime::TransientEffectCallId,
        result_sequence: u64,
        outcome: boon_runtime::Value,
    ) -> Result<boon_runtime::RuntimeTurn, boon_runtime::DistributedRuntimeError> {
        self.runtime
            .deliver_transient_effect_result_unsettled(call_id, result_sequence, outcome)
            .map_err(distributed_machine_error)
    }

    fn prepare_transient_effect_cancellation(
        &mut self,
        call_ids: &[boon_runtime::TransientEffectCallId],
    ) -> Result<Option<boon_runtime::RuntimeTurn>, boon_runtime::DistributedRuntimeError> {
        self.runtime
            .cancel_transient_effects_unsettled(call_ids)
            .map_err(distributed_machine_error)
    }

    fn commit_prepared_turn(
        &mut self,
        turn: boon_runtime::RuntimeTurn,
    ) -> Result<boon_runtime::RuntimeTurn, boon_runtime::DistributedRuntimeError> {
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

    fn rollback_prepared_turn(&mut self) -> Result<(), boon_runtime::DistributedRuntimeError> {
        self.runtime
            .rollback_unsettled_turn()
            .map_err(distributed_machine_error)
    }

    fn has_pending_transient_effect(&self, call_id: boon_runtime::TransientEffectCallId) -> bool {
        self.runtime
            .pending_transient_effect_credits(call_id)
            .is_some()
    }

    fn set_transient_effect_scope(&mut self, scope: u64) {
        self.runtime.set_transient_effect_scope(scope);
    }

    fn set_machine_origin(
        &mut self,
        origin: boon_runtime::SessionOrigin,
    ) -> Result<(), boon_runtime::DistributedRuntimeError> {
        let origin = boon_runtime::MachineOrigin::new(origin.slot(), origin.generation())
            .map_err(distributed_machine_error)?;
        self.runtime
            .set_machine_origin(origin)
            .map_err(distributed_machine_error)
    }

    fn reset_machine_origin(&mut self) -> Result<(), boon_runtime::DistributedRuntimeError> {
        self.runtime
            .reset_machine_origin()
            .map_err(distributed_machine_error)
    }

    fn drop_producer_origin(
        &mut self,
        origin: boon_runtime::SessionOrigin,
    ) -> Result<Vec<boon_runtime::TransientEffectCallId>, boon_runtime::DistributedRuntimeError> {
        let origin = boon_runtime::MachineOrigin::new(origin.slot(), origin.generation())
            .map_err(distributed_machine_error)?;
        self.runtime
            .drop_producer_origin(origin)
            .map_err(distributed_machine_error)
    }

    fn root_value_current(
        &mut self,
        name: &str,
    ) -> Result<boon_runtime::Value, boon_runtime::DistributedRuntimeError> {
        self.runtime
            .root_value_current(name)
            .map_err(distributed_machine_error)
    }
}

fn distributed_machine_error(error: impl fmt::Display) -> boon_runtime::DistributedRuntimeError {
    boon_runtime::DistributedRuntimeError::Runtime(error.to_string())
}

#[derive(Debug, PartialEq)]
pub struct ProgramSessionDispatch {
    pub source_sequence: u64,
    pub source_path: String,
    pub runtime_turn: boon_runtime::RuntimeTurn,
}
