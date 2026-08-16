use boon_example_manifest::{ExampleEntry, ExampleManifest};
use boon_ir::{ErasedProgram, verify_hidden_identity, verify_static_schedule};
use boon_parser::{
    ParseError, ParseProfile, ParseWorkCounters, ParsedProgram, ProjectSyntaxSnapshot,
    parse_project, parse_project_profiled, parse_source_profiled,
};
pub use boon_plan::{
    ApplicationIdentity, MachinePlan, MigrationPredecessorBinding, PlanError, ProgramRole,
    SealedMachinePlan, TargetProfile,
};
use serde::de::DeserializeOwned;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
#[cfg(not(target_arch = "wasm32"))]
use std::time::Instant;
use unicode_segmentation::UnicodeSegmentation;
#[cfg(target_arch = "wasm32")]
use web_time::Instant;

mod distributed_compiler;
mod document_plan_backend;
mod kernel_oracle;
mod machine_plan_backend;
mod session;

pub use distributed_compiler::{
    CompiledDistributedMachinePlans, DistributedClientProjectionSource, DistributedCompilerProgram,
    compile_distributed_runtime_source_programs,
    compile_distributed_runtime_source_programs_with_client_projection,
};
#[cfg(feature = "test-kernel-oracle")]
#[doc(hidden)]
pub use kernel_oracle::{
    KernelOwnerOracleDiagnostic, KernelOwnerOracleDiagnosticSite, KernelOwnerOracleEntry,
    KernelOwnerOracleReport, KernelOwnerOracleTimings, kernel_owner_oracle,
    kernel_owner_oracle_with_source_payloads, present_kernel_source_diagnostic,
    profile_kernel_owner_oracle_with_source_payloads,
};
pub use session::{
    CancellationToken, CompileIntent, CompilerProject, CompilerSession, CompilerSessionResult,
    ProjectId, Revision, UnitChange, UnitUpdate,
};

pub type CompilerResult<T> = Result<T, Box<dyn std::error::Error>>;

pub const COMPILER_ID: &str = concat!("boon-compiler/", env!("CARGO_PKG_VERSION"));

#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct SemanticLowerProfile {
    semantic_ms: f64,
    contract_verify_ms: f64,
    ir_lower_ms: f64,
}

fn verify_and_lower_checked_profiled(
    checked: boon_checked::CheckedProgram,
    producer_requests: &[boon_semantic::ProducerMaterializationRequest],
    cancellation: &mut CancellationProbe<'_>,
) -> Result<
    (
        ErasedProgram,
        Arc<boon_compilation_db::SealedRequestGraphSnapshot>,
        SemanticLowerProfile,
    ),
    String,
> {
    cancellation.checkpoint()?;
    let semantic_started = Instant::now();
    let semantic = elaborate_checked(checked, producer_requests)?;
    let semantic_ms = elapsed_ms(semantic_started);
    cancellation.checkpoint()?;
    let contract_verify_started = Instant::now();
    let request_graph = semantic.request_graph_snapshot();
    let verified =
        boon_verify::verify_explicit_contracts(semantic).map_err(|error| error.to_string())?;
    let contract_verify_ms = elapsed_ms(contract_verify_started);
    if std::env::var_os("BOON_COMPILER_LOWER_TRACE").is_some() {
        eprintln!("boon_compiler lower semantic_verification: {contract_verify_ms:.3}ms");
    }
    cancellation.checkpoint()?;
    let ir_lower_started = Instant::now();
    let ir = boon_ir::erase_and_lower(verified)?;
    let ir_lower_ms = elapsed_ms(ir_lower_started);
    if std::env::var_os("BOON_COMPILER_LOWER_TRACE").is_some() {
        eprintln!("boon_compiler lower ir_erasure: {ir_lower_ms:.3}ms");
    }
    cancellation.checkpoint()?;
    Ok((
        ir,
        request_graph,
        SemanticLowerProfile {
            semantic_ms,
            contract_verify_ms,
            ir_lower_ms,
        },
    ))
}

struct CancellationProbe<'a> {
    token: Option<&'a CancellationToken>,
    checkpoints: usize,
}

impl<'a> CancellationProbe<'a> {
    const fn new(token: Option<&'a CancellationToken>) -> Self {
        Self {
            token,
            checkpoints: 0,
        }
    }

    fn checkpoint(&mut self) -> Result<(), String> {
        self.checkpoints = self.checkpoints.saturating_add(1);
        if self.token.is_some_and(CancellationToken::is_canceled) {
            Err("compiler request canceled".to_owned())
        } else {
            Ok(())
        }
    }
}

fn elaborate_checked(
    checked: boon_checked::CheckedProgram,
    producer_requests: &[boon_semantic::ProducerMaterializationRequest],
) -> Result<boon_semantic::SemanticProgram, String> {
    elaborate_checked_with_external_event_identities(checked, producer_requests, &[])
}

fn elaborate_checked_with_external_event_identities(
    checked: boon_checked::CheckedProgram,
    producer_requests: &[boon_semantic::ProducerMaterializationRequest],
    external_event_identities: &[boon_checked::CheckedExternalDeclarationIdentityV1],
) -> Result<boon_semantic::SemanticProgram, String> {
    let started = Instant::now();
    let semantic = boon_semantic::elaborate_with_external_event_identities(
        checked,
        producer_requests,
        external_event_identities,
    )
    .map_err(|error| error.to_string())?;
    if std::env::var_os("BOON_COMPILER_LOWER_TRACE").is_some() {
        eprintln!(
            "boon_compiler lower semantic_elaboration: {:.3}ms",
            elapsed_ms(started)
        );
    }
    Ok(semantic)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompilerSourceUnit {
    /// Canonical UTF-8 project-relative path used in source identity.
    pub path: String,
    /// Exact source bytes decoded as UTF-8, without newline normalization.
    pub source: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompilerDiagnostic {
    pub path: String,
    pub line: Option<usize>,
    pub column: Option<usize>,
    pub start: Option<usize>,
    pub end: Option<usize>,
    pub message: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CheckedDiagnosticsProfile {
    pub source_unit_count: usize,
    pub expression_count: usize,
    pub diagnostic_count: usize,
    pub parse_work: ParseWorkCounters,
    pub typecheck_work: boon_typecheck::TypeCheckWorkCounters,
    pub owner_work: boon_typecheck::OwnerBodyInferenceWork,
    pub parse_ms: f64,
    pub typecheck_ms: f64,
    pub total_ms: f64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CompilerDiagnosticsProfile {
    pub source_unit_count: usize,
    pub owner_count: usize,
    /// Raw parser-arena expressions, including unreachable parser-internal rows.
    pub expression_count: usize,
    /// Reachable semantic expressions covered by owner body inference.
    pub checked_expression_count: usize,
    pub call_count: usize,
    pub diagnostic_count: usize,
    pub parse_work: ParseWorkCounters,
    /// Compatibility projection for benchmark consumers that still render
    /// the legacy owner-work columns. Kernel diagnostics populate this from
    /// their single dense compile/solve instead of running the owner solver.
    pub owner_work: boon_typecheck::OwnerBodyInferenceWork,
    pub kernel_compile_work: boon_compiler_kernel::KernelCompileWork,
    pub kernel_solve_work: boon_compiler_kernel::KernelSolveWork,
    pub parse_ms: f64,
    pub typecheck_ms: f64,
    pub total_ms: f64,
}

/// Complete construction-independent diagnostics for one compiler-session
/// revision. This artifact proves exact owner coverage without pretending that
/// checked rows, editor tables, or an executable construction were requested.
pub struct CompilerDiagnostics {
    pub syntax: ProjectSyntaxSnapshot,
    diagnostics: Box<[boon_checked::TypeDiagnostic]>,
    full_document_typecheck_coverage: bool,
    fingerprint_v1: [u8; 32],
    pub profile: CompilerDiagnosticsProfile,
}

impl CompilerDiagnostics {
    pub fn diagnostics(&self) -> &[boon_checked::TypeDiagnostic] {
        &self.diagnostics
    }

    pub fn has_errors(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|diagnostic| diagnostic.severity == boon_checked::DiagnosticSeverity::Error)
    }

    pub const fn full_document_typecheck_coverage(&self) -> bool {
        self.full_document_typecheck_coverage
    }

    pub const fn fingerprint_v1(&self) -> [u8; 32] {
        self.fingerprint_v1
    }

    pub fn source_bundle_digest_v1(&self) -> boon_contract::SourceBundleDigestV1 {
        self.syntax.source_bundle_digest_v1()
    }
}

pub enum CheckedSourceSyntax {
    Assembled(ParsedProgram),
    UnitNative(ProjectSyntaxSnapshot),
}

impl CheckedSourceSyntax {
    pub fn source_bundle_digest_v1(&self) -> boon_contract::SourceBundleDigestV1 {
        match self {
            Self::Assembled(program) => program.source_bundle_digest_v1,
            Self::UnitNative(program) => program.source_bundle_digest_v1(),
        }
    }

    pub fn assembled(&self) -> Option<&ParsedProgram> {
        match self {
            Self::Assembled(program) => Some(program),
            Self::UnitNative(_) => None,
        }
    }

    pub fn unit_native(&self) -> Option<&ProjectSyntaxSnapshot> {
        match self {
            Self::Assembled(_) => None,
            Self::UnitNative(program) => Some(program),
        }
    }
}

pub struct CheckedSourceFromSource {
    pub syntax: CheckedSourceSyntax,
    pub output: boon_checked::CheckOutput,
    pub profile: CheckedDiagnosticsProfile,
    /// Parser-issued call identities supplied by the dense kernel. Legacy
    /// checked constructions derive them from parser slots during sealing.
    checked_call_occurrences: Option<Box<[boon_syntax::StableOccurrenceKey]>>,
}

/// Produces structured parser/type diagnostics for a failed runtime compile.
/// Callers use this only on the error path, so successful compilation does not
/// repeat parsing or type checking.
///
/// `source_label` is the exact project-relative entrypoint path and must name
/// one of `units`; it is not a diagnostic-only display label.
pub fn diagnose_runtime_source_units(
    source_label: &str,
    units: &[CompilerSourceUnit],
) -> Vec<CompilerDiagnostic> {
    let parsed = parse_project(
        source_label.to_owned(),
        units
            .iter()
            .map(|unit| (unit.path.clone(), unit.source.clone())),
    );
    let parsed = match parsed {
        Ok(parsed) => parsed,
        Err(error) => {
            return vec![CompilerDiagnostic {
                path: error.path,
                line: error.line,
                column: error.column,
                start: None,
                end: None,
                message: error.message,
            }];
        }
    };
    boon_typecheck::check_runtime_profiled(&parsed)
        .0
        .diagnostics
        .into_iter()
        .filter(|diagnostic| diagnostic.severity == boon_checked::DiagnosticSeverity::Error)
        .map(|diagnostic| {
            let (path, line) = source_file_location(&parsed, diagnostic.line);
            CompilerDiagnostic {
                path,
                line: Some(line),
                column: grapheme_column(&parsed.source, diagnostic.line, diagnostic.start),
                start: Some(diagnostic.start),
                end: Some(diagnostic.end),
                message: diagnostic.message,
            }
        })
        .collect()
}

fn source_file_location(parsed: &ParsedProgram, global_line: usize) -> (String, usize) {
    parsed
        .files
        .iter()
        .filter(|file| file.start_line <= global_line)
        .max_by_key(|file| file.start_line)
        .map_or_else(
            || (parsed.path.clone(), global_line),
            |file| {
                (
                    file.path.clone(),
                    global_line
                        .saturating_sub(file.start_line)
                        .saturating_add(1),
                )
            },
        )
}

fn grapheme_column(source: &str, line: usize, byte: usize) -> Option<usize> {
    let line_start = source
        .split_inclusive('\n')
        .take(line.saturating_sub(1))
        .map(str::len)
        .sum::<usize>();
    (byte >= line_start && byte <= source.len())
        .then(|| source.get(line_start..byte.min(source.len())))
        .flatten()
        .map(|prefix| prefix.graphemes(true).count().saturating_add(1))
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CompileProfile {
    pub source_unit_count: usize,
    pub expression_count: usize,
    pub checked_expression_count: usize,
    pub checked_call_count: usize,
    pub graph_node_count: usize,
    pub cancellation_checkpoint_count: usize,
    pub parse_work: ParseWorkCounters,
    pub typecheck_work: boon_typecheck::TypeCheckWorkCounters,
    pub owner_work: boon_typecheck::OwnerBodyInferenceWork,
    pub parse_ms: f64,
    pub typecheck_ms: f64,
    pub semantic_ms: f64,
    pub contract_verify_ms: f64,
    pub ir_lower_ms: f64,
    pub lower_ms: f64,
    pub verify_ms: f64,
    pub compile_ms: f64,
    pub plan_validation_ms: f64,
    pub total_ms: f64,
}

#[derive(Clone, Debug)]
pub struct CompiledMachinePlanFromSource {
    pub ir: ErasedProgram,
    pub plan: MachinePlan,
    pub profile: CompileProfile,
    request_graph: Arc<boon_compilation_db::SealedRequestGraphSnapshot>,
}

/// Normal verified publication product.
///
/// Unlike the explicit compiler/debug product above, this artifact does not
/// retain construction IR beside the runnable plan. Its immutable plan token
/// carries the one successful public plan verification across trusted host and
/// executor handoffs.
#[derive(Clone, Debug)]
pub struct CompiledSealedMachinePlanFromSource {
    pub source_bundle_digest_v1: boon_contract::SourceBundleDigestV1,
    pub semantic_program_digest: boon_semantic::SemanticProgramDigestV1,
    pub verification_manifest_digest: boon_verify::VerificationManifestDigestV1,
    pub plan: SealedMachinePlan,
    pub profile: CompileProfile,
}

impl CompiledMachinePlanFromSource {
    pub fn request_graph_snapshot(&self) -> Arc<boon_compilation_db::SealedRequestGraphSnapshot> {
        Arc::clone(&self.request_graph)
    }

    pub fn seal(self) -> CompilerResult<CompiledSealedMachinePlanFromSource> {
        let seal_started = Instant::now();
        let source_bundle_digest_v1 = self.ir.source_bundle_digest_v1();
        let semantic_program_digest = self.ir.semantic_program_digest();
        let verification_manifest_digest = self.ir.verification_manifest_digest();
        let plan = boon_plan::seal_machine_plan(self.plan)?;
        let seal_ms = elapsed_ms(seal_started);
        let mut profile = self.profile;
        profile.plan_validation_ms = seal_ms;
        profile.total_ms += seal_ms;
        Ok(CompiledSealedMachinePlanFromSource {
            source_bundle_digest_v1,
            semantic_program_digest,
            verification_manifest_digest,
            plan,
            profile,
        })
    }
}

#[derive(Debug)]
pub struct CompileRequest<'a> {
    source: CompileSource<'a>,
    target_profile: TargetProfile,
    program_role: ProgramRole,
    application_identity: ApplicationIdentity,
    schema_version: u64,
    migration_predecessors: &'a [MigrationPredecessorBinding],
}

#[derive(Debug)]
enum CompileSource<'a> {
    Path(&'a Path),
    Text {
        source_label: &'a str,
        source_text: &'a str,
    },
    Units {
        source_label: &'a str,
        units: &'a [CompilerSourceUnit],
    },
}

#[derive(Debug)]
pub struct CompilerCheckRequest<'a> {
    source: CompileSource<'a>,
    program_role: ProgramRole,
}

impl<'a> CompilerCheckRequest<'a> {
    pub fn source_path(source_path: &'a Path, program_role: ProgramRole) -> Self {
        Self {
            source: CompileSource::Path(source_path),
            program_role,
        }
    }

    pub fn source_text(
        source_label: &'a str,
        source_text: &'a str,
        program_role: ProgramRole,
    ) -> Self {
        Self {
            source: CompileSource::Text {
                source_label,
                source_text,
            },
            program_role,
        }
    }

    pub fn source_units(
        source_label: &'a str,
        units: &'a [CompilerSourceUnit],
        program_role: ProgramRole,
    ) -> Self {
        Self {
            source: CompileSource::Units {
                source_label,
                units,
            },
            program_role,
        }
    }
}

impl<'a> CompileRequest<'a> {
    pub fn source_path(
        source_path: &'a Path,
        target_profile: TargetProfile,
        program_role: ProgramRole,
        application_identity: ApplicationIdentity,
    ) -> Self {
        Self::new(
            CompileSource::Path(source_path),
            target_profile,
            program_role,
            application_identity,
        )
    }

    pub fn source_text(
        source_label: &'a str,
        source_text: &'a str,
        target_profile: TargetProfile,
        program_role: ProgramRole,
        application_identity: ApplicationIdentity,
    ) -> Self {
        Self::new(
            CompileSource::Text {
                source_label,
                source_text,
            },
            target_profile,
            program_role,
            application_identity,
        )
    }

    /// Compiles a source bundle whose `source_label` is its exact canonical
    /// project-relative entrypoint and names one of `units`.
    pub fn source_units(
        source_label: &'a str,
        units: &'a [CompilerSourceUnit],
        target_profile: TargetProfile,
        program_role: ProgramRole,
        application_identity: ApplicationIdentity,
    ) -> Self {
        Self::new(
            CompileSource::Units {
                source_label,
                units,
            },
            target_profile,
            program_role,
            application_identity,
        )
    }

    fn new(
        source: CompileSource<'a>,
        target_profile: TargetProfile,
        program_role: ProgramRole,
        application_identity: ApplicationIdentity,
    ) -> Self {
        Self {
            source,
            target_profile,
            program_role,
            application_identity,
            schema_version: boon_plan::DEFAULT_PERSISTENCE_SCHEMA_VERSION,
            migration_predecessors: &[],
        }
    }

    pub fn with_persistence_catalog(
        mut self,
        schema_version: u64,
        migration_predecessors: &'a [MigrationPredecessorBinding],
    ) -> Self {
        self.schema_version = schema_version;
        self.migration_predecessors = migration_predecessors;
        self
    }
}

#[derive(Debug)]
pub struct CheckedCompileRequest<'a> {
    target_profile: TargetProfile,
    program_role: ProgramRole,
    application_identity: ApplicationIdentity,
    schema_version: u64,
    migration_predecessors: &'a [MigrationPredecessorBinding],
}

impl<'a> CheckedCompileRequest<'a> {
    pub fn new(
        target_profile: TargetProfile,
        program_role: ProgramRole,
        application_identity: ApplicationIdentity,
    ) -> Self {
        Self {
            target_profile,
            program_role,
            application_identity,
            schema_version: boon_plan::DEFAULT_PERSISTENCE_SCHEMA_VERSION,
            migration_predecessors: &[],
        }
    }

    pub fn with_persistence_catalog(
        mut self,
        schema_version: u64,
        migration_predecessors: &'a [MigrationPredecessorBinding],
    ) -> Self {
        self.schema_version = schema_version;
        self.migration_predecessors = migration_predecessors;
        self
    }
}

pub fn compile_erased_program(
    program: &ErasedProgram,
    target_profile: TargetProfile,
    program_role: ProgramRole,
    application_identity: &ApplicationIdentity,
    schema_version: u64,
    migration_predecessors: &[MigrationPredecessorBinding],
) -> Result<MachinePlan, PlanError> {
    machine_plan_backend::compile_erased_program(
        program,
        target_profile,
        program_role,
        application_identity,
        schema_version,
        migration_predecessors,
    )
}

pub fn compile_machine_plan(
    request: CompileRequest<'_>,
) -> CompilerResult<CompiledMachinePlanFromSource> {
    let total_started = Instant::now();
    let parse_started = Instant::now();
    let (parsed, parse_work) = parse_compile_source(request.source)?;
    let parse_ms = elapsed_ms(parse_started);
    compile_parsed_to_machine_plan(
        parsed,
        parse_work,
        parse_ms,
        total_started,
        request.target_profile,
        request.program_role,
        request.application_identity,
        request.schema_version,
        request.migration_predecessors,
    )
}

/// Compiles the normal preview/runtime artifact and drops construction IR
/// before returning it to the host.
pub fn compile_sealed_machine_plan(
    request: CompileRequest<'_>,
) -> CompilerResult<CompiledSealedMachinePlanFromSource> {
    compile_machine_plan(request)?.seal()
}

/// Retained and occurrence-specialized plans produced from one checked graph.
///
/// This type is compiled only for the differential artifact-oracle feature and
/// is not available to ordinary compiler or runtime builds.
#[cfg(feature = "test-flat-oracle")]
#[doc(hidden)]
pub struct ArtifactOraclePlanPair {
    pub retained: MachinePlan,
    pub flat_specialized: MachinePlan,
}

/// Produces a retained plan and an independently occurrence-specialized test
/// oracle from the same parsed and checked source.
///
/// The flat path is intentionally feature-gated and must never be selected by
/// production compilation or used as a fallback when retained lowering fails.
#[cfg(feature = "test-flat-oracle")]
#[doc(hidden)]
pub fn compile_artifact_oracle_pair(
    request: CompileRequest<'_>,
) -> CompilerResult<ArtifactOraclePlanPair> {
    let CompileRequest {
        source,
        target_profile,
        program_role,
        application_identity,
        schema_version,
        migration_predecessors,
    } = request;
    let (parsed, _) = parse_compile_source(source)?;
    let external_types = boon_checked::ExternalTypeEnvironment::empty(program_role);
    let (check_output, _) = boon_typecheck::check_runtime_program_profiled_with_external_types(
        &parsed,
        &external_types,
    );
    let checked =
        checked_program_from_output(CheckedSyntaxRef::Assembled(&parsed), check_output, None)?;
    let retained = compile_checked_artifact_oracle_plan(
        checked.clone(),
        false,
        target_profile,
        program_role,
        &application_identity,
        schema_version,
        migration_predecessors,
    )?;
    let flat_specialized = compile_checked_artifact_oracle_plan(
        checked,
        true,
        target_profile,
        program_role,
        &application_identity,
        schema_version,
        migration_predecessors,
    )?;
    Ok(ArtifactOraclePlanPair {
        retained,
        flat_specialized,
    })
}

#[cfg(feature = "test-flat-oracle")]
#[allow(clippy::too_many_arguments)]
fn compile_checked_artifact_oracle_plan(
    checked: boon_checked::CheckedProgram,
    flat_specialized: bool,
    target_profile: TargetProfile,
    program_role: ProgramRole,
    application_identity: &ApplicationIdentity,
    schema_version: u64,
    migration_predecessors: &[MigrationPredecessorBinding],
) -> CompilerResult<MachinePlan> {
    let semantic = if flat_specialized {
        boon_semantic::elaborate_flat_test_oracle(checked, &[])
    } else {
        boon_semantic::elaborate(checked, &[])
    }
    .map_err(|error| PlanError::new(error.to_string()))?;
    let verified = boon_verify::verify_explicit_contracts(semantic)
        .map_err(|error| PlanError::new(error.to_string()))?;
    let ir = boon_ir::erase_and_lower(verified).map_err(PlanError::new)?;
    verify_hidden_identity(&ir)?;
    verify_static_schedule(&ir)?;
    let plan = compile_erased_program(
        &ir,
        target_profile,
        program_role,
        application_identity,
        schema_version,
        migration_predecessors,
    )?;
    let verification = boon_plan::verify_plan(&plan)?;
    if verification.status != "pass" {
        return Err(PlanError::new(format!(
            "artifact-oracle MachinePlan verification returned {}",
            verification.status
        ))
        .into());
    }
    Ok(plan)
}

pub fn check_source(request: CompilerCheckRequest<'_>) -> CompilerResult<CheckedSourceFromSource> {
    check_source_with_ownership(request, CheckedSourceOwnership::Report)
}

/// Checks one compiler-service revision while retaining editor projections and
/// transferring lowering-owned tables into the checked artifact. A subsequent
/// verified request can therefore consume this exact result without retaining
/// a second copy of the runtime tables.
pub fn check_editor_source(
    request: CompilerCheckRequest<'_>,
) -> CompilerResult<CheckedSourceFromSource> {
    check_source_with_ownership(request, CheckedSourceOwnership::Editor)
}

/// Compiler-service diagnostics path. It returns the complete checked
/// diagnostic set while deferring the global editor type-hint sidecar until a
/// language client explicitly projects it.
pub fn check_diagnostics_source(
    request: CompilerCheckRequest<'_>,
) -> CompilerResult<CheckedSourceFromSource> {
    check_source_with_ownership(request, CheckedSourceOwnership::Diagnostics)
}

/// Checks source for a request that will continue through semantic sealing and
/// lowering. Successful checks keep lowering-owned tables only in the
/// `CheckedProgram`; error results retain the same complete diagnostics as
/// [`check_source`].
///
/// A compiler session uses this path when a verified request is the first
/// request for a revision. If diagnostics were already requested, it consumes
/// that exact checked artifact instead of checking the revision again.
pub fn check_runtime_source(
    request: CompilerCheckRequest<'_>,
) -> CompilerResult<CheckedSourceFromSource> {
    check_source_with_ownership(request, CheckedSourceOwnership::Runtime)
}

#[derive(Clone, Copy)]
enum CheckedSourceOwnership {
    Report,
    Editor,
    Diagnostics,
    Runtime,
}

fn check_source_with_ownership(
    request: CompilerCheckRequest<'_>,
    ownership: CheckedSourceOwnership,
) -> CompilerResult<CheckedSourceFromSource> {
    let parse_started = Instant::now();
    let (parsed, parse_work) = parse_compile_source(request.source)?;
    let parse_ms = elapsed_ms(parse_started);
    check_parsed_source_with_ownership(
        parsed,
        parse_work,
        parse_ms,
        request.program_role,
        ownership,
    )
}

pub(crate) fn checked_source_from_owner_assembly(
    syntax: ProjectSyntaxSnapshot,
    assembly: &boon_typecheck::CheckedOwnerProjectAssembly,
    parse_work: ParseWorkCounters,
    parse_ms: f64,
    typecheck_work: boon_typecheck::TypeCheckWorkCounters,
    owner_work: boon_typecheck::OwnerBodyInferenceWork,
    typecheck_ms: f64,
) -> CheckedSourceFromSource {
    checked_source_from_checked_fields(
        syntax,
        assembly.fields().clone(),
        assembly.diagnostics(),
        parse_work,
        parse_ms,
        typecheck_work,
        owner_work,
        typecheck_ms,
        None,
    )
}

pub(crate) fn checked_source_from_checked_fields(
    syntax: ProjectSyntaxSnapshot,
    fields: boon_checked::CheckedProgramFields,
    diagnostics: &[boon_checked::TypeDiagnostic],
    parse_work: ParseWorkCounters,
    parse_ms: f64,
    typecheck_work: boon_typecheck::TypeCheckWorkCounters,
    owner_work: boon_typecheck::OwnerBodyInferenceWork,
    typecheck_ms: f64,
    checked_call_occurrences: Option<Box<[boon_syntax::StableOccurrenceKey]>>,
) -> CheckedSourceFromSource {
    let metadata = &fields.lowering_metadata;
    let render_slot_failure_count = metadata
        .render_slot_table
        .slots
        .iter()
        .filter(|slot| !slot.diagnostics.is_empty())
        .count();
    let unresolved_type_variable_count = metadata.dynamic_fallback_count.saturating_sub(
        metadata
            .expr_type_table
            .entries
            .iter()
            .filter(|entry| matches!(entry.flow_type.ty, boon_checked::Type::Unknown))
            .count(),
    );
    let mut builtin_signature_coverage = syntax.operators().to_vec();
    builtin_signature_coverage.extend(syntax.functions().iter().cloned());
    builtin_signature_coverage.sort();
    builtin_signature_coverage.dedup();
    // Project facts intentionally retain render-slot failures both in the
    // complete flat diagnostic aggregate and on the render slot that owns
    // their editor presentation. A checked report historically exposes slot
    // failures through the slot table, so remove those exact rows from its
    // flat channel instead of counting and presenting one fact twice.
    let render_diagnostics = metadata
        .render_slot_table
        .slots
        .iter()
        .flat_map(|slot| &slot.diagnostics)
        .map(|diagnostic| {
            (
                match diagnostic.severity {
                    boon_checked::DiagnosticSeverity::Error => 0u8,
                    boon_checked::DiagnosticSeverity::Warning => 1u8,
                },
                diagnostic.line,
                diagnostic.start,
                diagnostic.end,
                diagnostic.message.clone(),
            )
        })
        .collect::<std::collections::BTreeSet<_>>();
    let report_diagnostics = diagnostics
        .iter()
        .filter(|diagnostic| {
            !render_diagnostics.contains(&(
                match diagnostic.severity {
                    boon_checked::DiagnosticSeverity::Error => 0u8,
                    boon_checked::DiagnosticSeverity::Warning => 1u8,
                },
                diagnostic.line,
                diagnostic.start,
                diagnostic.end,
                diagnostic.message.clone(),
            ))
        })
        .cloned()
        .collect::<Vec<_>>();
    let report = boon_checked::TypeCheckReport {
        expression_count: syntax.expression_count(),
        checked_expression_count: fields.expressions.len(),
        unresolved_type_variable_count,
        dynamic_fallback_count: metadata.dynamic_fallback_count,
        render_slot_count: metadata.render_slot_table.slots.len(),
        render_slot_failure_count,
        builtin_signature_coverage,
        source_payload_shape_coverage: metadata
            .source_payload_shape_table
            .iter()
            .map(|entry| entry.diagnostic_path.clone())
            .collect(),
        source_payload_shape_table: metadata.source_payload_shape_table.clone(),
        host_port_table: metadata.host_port_table.clone(),
        full_document_typecheck_coverage: fields.expressions.len() == syntax.expression_count(),
        output_root_types: metadata.output_root_types.clone(),
        expr_type_table: metadata.expr_type_table.clone(),
        function_type_table: metadata.function_type_table.clone(),
        named_value_type_table: metadata.named_value_type_table.clone(),
        type_hint_table: boon_checked::TypeHintTable::default(),
        resolved_constant_table: boon_checked::ResolvedConstantTable::default(),
        render_slot_table: metadata.render_slot_table.clone(),
        constraints: Vec::new(),
        diagnostics: report_diagnostics,
    };
    let construction = (!report.has_errors()).then(|| {
        // SAFETY: callers expose fields only after their checker has completed
        // dense coverage, relocation, lowering-metadata validation, and source
        // digest binding. This boundary deliberately grants diagnostics
        // construction authority but not a runtime checked image.
        unsafe {
            boon_checked::CheckedProgramConstruction::from_typechecker_fields_unchecked(fields)
        }
    });
    let diagnostic_count = report.diagnostics.len()
        + report
            .render_slot_table
            .slots
            .iter()
            .map(|slot| slot.diagnostics.len())
            .sum::<usize>();
    if std::env::var_os("BOON_OWNER_ASSEMBLY_TRACE").is_some() {
        for diagnostic in &report.diagnostics {
            eprintln!(
                "boon owner assembly diagnostic {}:{}-{} {}",
                diagnostic.line, diagnostic.start, diagnostic.end, diagnostic.message
            );
        }
    }
    CheckedSourceFromSource {
        syntax: CheckedSourceSyntax::UnitNative(syntax.clone()),
        output: boon_checked::CheckOutput {
            program: None,
            construction,
            report,
        },
        profile: CheckedDiagnosticsProfile {
            source_unit_count: syntax.units().len(),
            expression_count: syntax.expression_count(),
            diagnostic_count,
            parse_work,
            typecheck_work,
            owner_work,
            parse_ms,
            typecheck_ms,
            total_ms: parse_ms + typecheck_ms,
        },
        checked_call_occurrences,
    }
}

fn check_parsed_source_with_ownership(
    parsed: ParsedProgram,
    parse_work: ParseWorkCounters,
    parse_ms: f64,
    program_role: ProgramRole,
    ownership: CheckedSourceOwnership,
) -> CompilerResult<CheckedSourceFromSource> {
    check_syntax_source_with_ownership(
        CheckedSourceSyntax::Assembled(parsed),
        parse_work,
        parse_ms,
        program_role,
        ownership,
    )
}

fn check_syntax_source_with_ownership(
    syntax: CheckedSourceSyntax,
    parse_work: ParseWorkCounters,
    parse_ms: f64,
    program_role: ProgramRole,
    ownership: CheckedSourceOwnership,
) -> CompilerResult<CheckedSourceFromSource> {
    let check_started = Instant::now();
    let (source_unit_count, expression_count) = match &syntax {
        CheckedSourceSyntax::Assembled(program) => (program.files.len(), program.expressions.len()),
        CheckedSourceSyntax::UnitNative(program) => {
            (program.units().len(), program.expression_count())
        }
    };
    let external_types = boon_checked::ExternalTypeEnvironment::empty(program_role);
    let typecheck_started = Instant::now();
    let (output, typecheck_profile) = match (&syntax, ownership) {
        (CheckedSourceSyntax::Assembled(program), CheckedSourceOwnership::Report) => {
            boon_typecheck::check_program_profiled_with_external_types(program, &external_types)
        }
        (CheckedSourceSyntax::Assembled(program), CheckedSourceOwnership::Editor) => {
            boon_typecheck::check_editor_program_profiled_with_external_types(
                program,
                &external_types,
            )
        }
        (CheckedSourceSyntax::Assembled(program), CheckedSourceOwnership::Diagnostics) => {
            boon_typecheck::check_diagnostics_program_profiled_with_external_types(
                program,
                &external_types,
            )
        }
        (CheckedSourceSyntax::Assembled(program), CheckedSourceOwnership::Runtime) => {
            boon_typecheck::check_runtime_program_profiled_with_external_types(
                program,
                &external_types,
            )
        }
        (CheckedSourceSyntax::UnitNative(program), CheckedSourceOwnership::Report) => {
            boon_typecheck::check_project_program_profiled_with_external_types(
                program,
                &external_types,
            )
        }
        (CheckedSourceSyntax::UnitNative(program), CheckedSourceOwnership::Editor) => {
            boon_typecheck::check_project_editor_program_profiled_with_external_types(
                program,
                &external_types,
            )
        }
        (CheckedSourceSyntax::UnitNative(program), CheckedSourceOwnership::Diagnostics) => {
            boon_typecheck::check_project_diagnostics_program_profiled_with_external_types(
                program,
                &external_types,
            )
        }
        (CheckedSourceSyntax::UnitNative(program), CheckedSourceOwnership::Runtime) => {
            boon_typecheck::check_project_runtime_program_profiled_with_external_types(
                program,
                &external_types,
            )
        }
    };
    let typecheck_ms = elapsed_ms(typecheck_started);
    let diagnostic_count = output.report.diagnostics.len()
        + output
            .report
            .render_slot_table
            .slots
            .iter()
            .map(|slot| slot.diagnostics.len())
            .sum::<usize>();
    Ok(CheckedSourceFromSource {
        syntax,
        output,
        profile: CheckedDiagnosticsProfile {
            source_unit_count,
            expression_count,
            diagnostic_count,
            parse_work,
            typecheck_work: typecheck_profile.work_counters,
            owner_work: boon_typecheck::OwnerBodyInferenceWork::default(),
            parse_ms,
            typecheck_ms,
            total_ms: parse_ms + elapsed_ms(check_started),
        },
        checked_call_occurrences: None,
    })
}

fn parse_compile_source(
    source: CompileSource<'_>,
) -> CompilerResult<(ParsedProgram, ParseWorkCounters)> {
    let (parsed, profile) = match source {
        CompileSource::Path(source_path) => {
            let (entrypoint, units) = compiler_source_project_for_path(source_path)?;
            parse_source_units_profiled(&entrypoint, &units)
        }
        CompileSource::Text {
            source_label,
            source_text,
        } => parse_source_profiled(source_label.to_owned(), source_text.to_owned()),
        CompileSource::Units {
            source_label,
            units,
        } => parse_source_units_profiled(source_label, units),
    };
    Ok((parsed?, profile.work_counters))
}

#[allow(clippy::too_many_arguments)]
fn compile_parsed_to_machine_plan(
    parsed: ParsedProgram,
    parse_work: ParseWorkCounters,
    parse_ms: f64,
    total_started: Instant,
    target_profile: TargetProfile,
    program_role: ProgramRole,
    application_identity: ApplicationIdentity,
    schema_version: u64,
    migration_predecessors: &[MigrationPredecessorBinding],
) -> CompilerResult<CompiledMachinePlanFromSource> {
    let external_types = boon_checked::ExternalTypeEnvironment::empty(program_role);
    let typecheck_started = Instant::now();
    let (check_output, typecheck_profile) =
        boon_typecheck::check_runtime_program_profiled_with_external_types(
            &parsed,
            &external_types,
        );
    let typecheck_ms = elapsed_ms(typecheck_started);
    if std::env::var_os("BOON_COMPILER_LOWER_TRACE").is_some() {
        eprintln!("boon_compiler lower typecheck: {:.3}ms", typecheck_ms);
    }
    let source_unit_count = parsed.files.len();
    let parsed_expression_count = parsed.expressions.len();
    let checked =
        checked_program_from_output(CheckedSyntaxRef::Assembled(&parsed), check_output, None)?;
    finish_checked_program_to_machine_plan(
        checked,
        source_unit_count,
        parsed_expression_count,
        parse_work,
        typecheck_profile.work_counters,
        boon_typecheck::OwnerBodyInferenceWork::default(),
        parse_ms,
        typecheck_ms,
        elapsed_ms(total_started),
        target_profile,
        program_role,
        application_identity,
        schema_version,
        migration_predecessors,
        CancellationProbe::new(None),
    )
}

pub fn finish_checked_machine_plan(
    checked_source: CheckedSourceFromSource,
    request: CheckedCompileRequest<'_>,
) -> CompilerResult<CompiledMachinePlanFromSource> {
    finish_checked_machine_plan_with_cancellation(checked_source, request, None)
}

pub fn finish_checked_sealed_machine_plan(
    checked_source: CheckedSourceFromSource,
    request: CheckedCompileRequest<'_>,
) -> CompilerResult<CompiledSealedMachinePlanFromSource> {
    finish_checked_machine_plan(checked_source, request)?.seal()
}

pub(crate) fn finish_checked_machine_plan_with_cancellation(
    checked_source: CheckedSourceFromSource,
    request: CheckedCompileRequest<'_>,
    cancellation: Option<&CancellationToken>,
) -> CompilerResult<CompiledMachinePlanFromSource> {
    let mut cancellation = CancellationProbe::new(cancellation);
    cancellation.checkpoint().map_err(PlanError::new)?;
    let CheckedSourceFromSource {
        syntax,
        output,
        mut profile,
        checked_call_occurrences,
    } = checked_source;
    let deferred_runtime_handoff = output.construction.is_some();
    let runtime_handoff_started = Instant::now();
    let syntax = match &syntax {
        CheckedSourceSyntax::Assembled(program) => CheckedSyntaxRef::Assembled(program),
        CheckedSourceSyntax::UnitNative(program) => CheckedSyntaxRef::UnitNative(program),
    };
    let checked = checked_program_from_output(syntax, output, checked_call_occurrences.as_deref())?;
    if deferred_runtime_handoff {
        let runtime_handoff_ms = elapsed_ms(runtime_handoff_started);
        profile.typecheck_ms += runtime_handoff_ms;
        profile.total_ms += runtime_handoff_ms;
        if std::env::var_os("BOON_COMPILER_LOWER_TRACE").is_some() {
            eprintln!("boon_compiler deferred checked runtime handoff: {runtime_handoff_ms:.3}ms");
        }
    }
    if checked.role != request.program_role {
        return Err(PlanError::new(format!(
            "checked program role {:?} differs from requested backend role {:?}",
            checked.role, request.program_role
        ))
        .into());
    }
    finish_checked_program_to_machine_plan(
        checked,
        profile.source_unit_count,
        profile.expression_count,
        profile.parse_work,
        profile.typecheck_work,
        profile.owner_work,
        profile.parse_ms,
        profile.typecheck_ms,
        profile.total_ms,
        request.target_profile,
        request.program_role,
        request.application_identity,
        request.schema_version,
        request.migration_predecessors,
        cancellation,
    )
}

#[allow(clippy::too_many_arguments)]
fn finish_checked_program_to_machine_plan(
    checked: boon_checked::CheckedProgram,
    source_unit_count: usize,
    parsed_expression_count: usize,
    parse_work: ParseWorkCounters,
    typecheck_work: boon_typecheck::TypeCheckWorkCounters,
    owner_work: boon_typecheck::OwnerBodyInferenceWork,
    parse_ms: f64,
    typecheck_ms: f64,
    elapsed_before_finish_ms: f64,
    target_profile: TargetProfile,
    program_role: ProgramRole,
    application_identity: ApplicationIdentity,
    schema_version: u64,
    migration_predecessors: &[MigrationPredecessorBinding],
    mut cancellation: CancellationProbe<'_>,
) -> CompilerResult<CompiledMachinePlanFromSource> {
    let finish_started = Instant::now();
    let checked_expression_count = checked.expressions.len();
    let checked_call_count = checked.calls.len();
    if std::env::var_os("BOON_COMPILER_LOWER_TRACE").is_some() {
        eprintln!(
            "boon_compiler checked_program scopes={} declarations={} statements={} expressions={} callables={} calls={}",
            checked.scopes.len(),
            checked.declarations.len(),
            checked.statements.len(),
            checked.expressions.len(),
            checked.callables.len(),
            checked.calls.len(),
        );
    }
    let lower_started = Instant::now();
    let (ir, request_graph, semantic_profile) =
        verify_and_lower_checked_profiled(checked, &[], &mut cancellation)?;
    let lower_ms = elapsed_ms(lower_started);
    cancellation.checkpoint().map_err(PlanError::new)?;
    let verify_started = Instant::now();
    verify_hidden_identity(&ir)?;
    verify_static_schedule(&ir)?;
    let verify_ms = elapsed_ms(verify_started);
    cancellation.checkpoint().map_err(PlanError::new)?;
    let compile_started = Instant::now();
    let plan = compile_erased_program(
        &ir,
        target_profile,
        program_role,
        &application_identity,
        schema_version,
        migration_predecessors,
    )?;
    let compile_ms = elapsed_ms(compile_started);
    cancellation.checkpoint().map_err(PlanError::new)?;
    if std::env::var_os("BOON_COMPILER_LOWER_TRACE").is_some() {
        eprintln!("boon_compiler lower backend_compile: {compile_ms:.3}ms");
    }
    let profile = CompileProfile {
        source_unit_count,
        expression_count: parsed_expression_count,
        checked_expression_count,
        checked_call_count,
        graph_node_count: ir.graph_node_count,
        cancellation_checkpoint_count: cancellation.checkpoints,
        parse_work,
        typecheck_work,
        owner_work,
        parse_ms,
        typecheck_ms,
        semantic_ms: semantic_profile.semantic_ms,
        contract_verify_ms: semantic_profile.contract_verify_ms,
        ir_lower_ms: semantic_profile.ir_lower_ms,
        lower_ms,
        verify_ms,
        compile_ms,
        plan_validation_ms: 0.0,
        total_ms: elapsed_before_finish_ms + elapsed_ms(finish_started),
    };
    Ok(CompiledMachinePlanFromSource {
        ir,
        plan,
        profile,
        request_graph,
    })
}

#[derive(Clone, Copy)]
enum CheckedSyntaxRef<'a> {
    Assembled(&'a ParsedProgram),
    UnitNative(&'a ProjectSyntaxSnapshot),
}

impl CheckedSyntaxRef<'_> {
    fn source_file_location(self, global_line: usize) -> (String, usize) {
        match self {
            Self::Assembled(program) => source_file_location(program, global_line),
            Self::UnitNative(program) => program
                .source_layouts()
                .iter()
                .filter(|layout| layout.start_line <= global_line)
                .max_by_key(|layout| layout.start_line)
                .map_or_else(
                    || (program.path().to_owned(), global_line),
                    |layout| {
                        (
                            layout.path.clone(),
                            global_line
                                .saturating_sub(layout.start_line)
                                .saturating_add(1),
                        )
                    },
                ),
        }
    }
}

fn checked_program_from_output(
    syntax: CheckedSyntaxRef<'_>,
    output: boon_checked::CheckOutput,
    checked_call_occurrences: Option<&[boon_syntax::StableOccurrenceKey]>,
) -> CompilerResult<boon_checked::CheckedProgram> {
    if output.report.has_errors() {
        let diagnostics = output
            .report
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.severity == boon_checked::DiagnosticSeverity::Error)
            .map(|diagnostic| {
                let (path, line) = syntax.source_file_location(diagnostic.line);
                format!("{path}:{line}: {}", diagnostic.message)
            })
            .chain(
                output
                    .report
                    .render_slot_table
                    .slots
                    .iter()
                    .flat_map(|slot| {
                        slot.diagnostics
                            .iter()
                            .filter(|diagnostic| {
                                diagnostic.severity == boon_checked::DiagnosticSeverity::Error
                            })
                            .map(|diagnostic| {
                                format!(
                                    "render slot `{}` at line {}: {}",
                                    slot.slot_name, diagnostic.line, diagnostic.message
                                )
                            })
                    }),
            )
            .collect::<Vec<_>>();
        return Err(PlanError::new(format!(
            "typecheck failed with {} error diagnostic(s): {}",
            diagnostics.len(),
            diagnostics.join("; ")
        ))
        .into());
    }
    match (output.program, output.construction) {
        (Some(program), None) => Ok(program),
        (None, Some(construction)) => match syntax {
            CheckedSyntaxRef::Assembled(program) if checked_call_occurrences.is_none() => {
                boon_typecheck::seal_checked_program_construction(program, construction)
                    .map_err(|error| PlanError::new(error).into())
            }
            CheckedSyntaxRef::Assembled(_) => Err(PlanError::new(
                "assembled checked construction cannot consume project-native call identities",
            )
            .into()),
            CheckedSyntaxRef::UnitNative(program)
                if let Some(call_occurrences) = checked_call_occurrences =>
            {
                boon_typecheck::seal_project_checked_program_construction_with_call_occurrences(
                    program,
                    construction,
                    call_occurrences,
                )
                .map_err(|error| PlanError::new(error).into())
            }
            CheckedSyntaxRef::UnitNative(program) => {
                boon_typecheck::seal_project_checked_program_construction(program, construction)
                    .map_err(|error| PlanError::new(error).into())
            }
        },
        (Some(_), Some(_)) => Err(PlanError::new(
            "typecheck produced both a sealed and construction-only CheckedProgram",
        )
        .into()),
        (None, None) => {
            Err(PlanError::new("typecheck produced no CheckedProgram for valid source").into())
        }
    }
}

fn parse_source_units(
    source_label: &str,
    units: &[CompilerSourceUnit],
) -> CompilerResult<ParsedProgram> {
    Ok(parse_project(
        source_label.to_owned(),
        units
            .iter()
            .map(|unit| (unit.path.clone(), unit.source.clone())),
    )?)
}

fn parse_source_units_profiled(
    source_label: &str,
    units: &[CompilerSourceUnit],
) -> (Result<ParsedProgram, ParseError>, ParseProfile) {
    parse_project_profiled(
        source_label.to_owned(),
        units
            .iter()
            .map(|unit| (unit.path.clone(), unit.source.clone())),
    )
}

pub fn compiler_source_units_for_path(path: &Path) -> CompilerResult<Vec<CompilerSourceUnit>> {
    compiler_source_project_for_path(path).map(|(_, units)| units)
}

pub fn compiler_source_project_for_path(
    path: &Path,
) -> CompilerResult<(String, Vec<CompilerSourceUnit>)> {
    let entrypoint = resolve_repo_file(path);
    let files = compiler_source_files_for_path(path)?;
    let entrypoint_index = files
        .iter()
        .position(|candidate| paths_match(candidate, &entrypoint))
        .ok_or_else(|| {
            format!(
                "source entrypoint `{}` is absent from its compiler source bundle",
                path.display()
            )
        })?;
    let units = compiler_source_units_for_files(files)?;
    let entrypoint = units
        .get(entrypoint_index)
        .map(|unit| unit.path.clone())
        .ok_or_else(|| "compiler source entrypoint index is stale".to_owned())?;
    Ok((entrypoint, units))
}

pub fn compiler_source_units_for_manifest_source(
    source: &str,
    source_files: &[String],
) -> CompilerResult<Vec<CompilerSourceUnit>> {
    compiler_source_units_for_files(compiler_source_files_for_manifest_source(
        source,
        source_files,
    ))
}

pub fn compiler_source_files_for_path(path: &Path) -> CompilerResult<Vec<PathBuf>> {
    source_files_for_path(path)
}

pub fn compiler_source_files_for_manifest_source(
    source: &str,
    source_files: &[String],
) -> Vec<PathBuf> {
    source_files_for_manifest_source(source, source_files)
}

pub fn compiler_source_text_for_path(path: &Path) -> CompilerResult<String> {
    Ok(fs::read_to_string(resolve_repo_file(path))?)
}

pub fn compiler_source_text_for_manifest_source(source: &str) -> CompilerResult<String> {
    Ok(fs::read_to_string(resolve_repo_file(source))?)
}

pub fn parse_scenario_file<T>(path: &Path) -> CompilerResult<T>
where
    T: DeserializeOwned,
{
    let text = fs::read_to_string(resolve_repo_file(path))?;
    Ok(toml::from_str(&text)?)
}

fn compiler_source_units_for_files(files: Vec<PathBuf>) -> CompilerResult<Vec<CompilerSourceUnit>> {
    let logical_paths = compiler_logical_source_paths(&files)?;
    files
        .into_iter()
        .zip(logical_paths)
        .map(|(path, logical_path)| {
            let source = fs::read_to_string(&path)?;
            Ok(CompilerSourceUnit {
                path: logical_path,
                source,
            })
        })
        .collect()
}

fn compiler_logical_source_paths(files: &[PathBuf]) -> CompilerResult<Vec<String>> {
    if files.is_empty() {
        return Err("compiler source bundle has no files".into());
    }
    let canonical_files = files
        .iter()
        .map(|path| {
            path.canonicalize()
                .map_err(|error| format!("cannot canonicalize `{}`: {error}", path.display()))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .ok();
    let logical_root = workspace_root
        .filter(|root| canonical_files.iter().all(|path| path.starts_with(root)))
        .or_else(|| common_source_parent(&canonical_files))
        .ok_or_else(|| {
            "compiler source files do not share a non-root logical project directory".to_owned()
        })?;

    canonical_files
        .iter()
        .map(|path| {
            let relative = path.strip_prefix(&logical_root).map_err(|_| {
                format!(
                    "compiler source `{}` is outside logical root `{}`",
                    path.display(),
                    logical_root.display()
                )
            })?;
            let mut components = Vec::new();
            for component in relative.components() {
                let Component::Normal(component) = component else {
                    return Err(format!(
                        "compiler source `{}` has a non-project-relative component",
                        path.display()
                    ));
                };
                components.push(
                    component
                        .to_str()
                        .ok_or_else(|| {
                            format!("compiler source `{}` is not UTF-8", path.display())
                        })?
                        .to_owned(),
                );
            }
            if components.is_empty() {
                return Err(format!(
                    "compiler source `{}` has an empty logical path",
                    path.display()
                ));
            }
            Ok(components.join("/"))
        })
        .collect::<Result<Vec<_>, _>>()
        .map_err(Into::into)
}

fn common_source_parent(paths: &[PathBuf]) -> Option<PathBuf> {
    let mut common = paths.first()?.parent()?.to_path_buf();
    while !paths.iter().all(|path| path.starts_with(&common)) {
        if !common.pop() {
            return None;
        }
    }
    common.parent().is_some().then_some(common)
}

fn source_files_for_path(source_path: &Path) -> CompilerResult<Vec<PathBuf>> {
    let source_path = resolve_repo_file(source_path);
    for entry in example_manifest_entries().unwrap_or_default() {
        if paths_match(&resolve_repo_file(&entry.source), &source_path) {
            return Ok(source_files_for_manifest_source(
                &entry.source,
                &entry.source_files,
            ));
        }
        for program in &entry.programs {
            if paths_match(&resolve_repo_file(&program.source), &source_path) {
                return Ok(source_files_for_manifest_source(
                    &program.source,
                    &program.source_files,
                ));
            }
        }
    }
    Ok(vec![source_path])
}

fn example_manifest_entries() -> CompilerResult<Vec<ExampleEntry>> {
    let path = resolve_repo_file("examples/manifest.toml");
    let manifest = ExampleManifest::from_path(path)?;
    Ok(manifest.example)
}

fn source_files_for_manifest_source(source: &str, source_files: &[String]) -> Vec<PathBuf> {
    let source_path = resolve_repo_file(source);
    let mut files = source_files
        .iter()
        .map(resolve_repo_file)
        .collect::<Vec<_>>();
    files.retain(|path| !paths_match(path, &source_path));
    files.push(source_path);
    files
}

fn paths_match(left: &Path, right: &Path) -> bool {
    left == right
        || left
            .canonicalize()
            .ok()
            .zip(right.canonicalize().ok())
            .is_some_and(|(left, right)| left == right)
}

fn resolve_repo_file(relative: impl AsRef<Path>) -> PathBuf {
    let relative = relative.as_ref();
    if relative.exists() {
        return relative.to_path_buf();
    }
    if let Ok(cwd) = std::env::current_dir() {
        for ancestor in cwd.ancestors() {
            let candidate = ancestor.join(relative);
            if candidate.exists() {
                return candidate;
            }
        }
    }
    relative.to_path_buf()
}

fn elapsed_ms(started: Instant) -> f64 {
    started.elapsed().as_secs_f64() * 1000.0
}
