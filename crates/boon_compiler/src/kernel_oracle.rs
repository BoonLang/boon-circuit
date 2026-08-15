//! Parser-owned projection into the dense compiler kernel.
//!
//! The compact projection is the production migration boundary: it bypasses
//! legacy owner syntax, lexical, constraint-seed, interface, and body DTOs.
//! Differential report rows remain in this module temporarily so the same
//! projection can prove the flag-day cutover against the old checker without
//! becoming a selectable runtime fallback.

#![cfg_attr(not(any(test, feature = "test-kernel-oracle")), allow(dead_code))]

use boon_checked::{
    CheckedListKeyPolicy, CheckedScope, CheckedStateKind, DiagnosticSeverity, FlowMode, FlowType,
    ObjectShape, Type, TypeDiagnostic, Variant, type_is_recursively_closed,
};
use boon_compiler_kernel::{
    CheckDemand, KernelCallArgumentKind, KernelCallArgumentSource, KernelCallInputRole,
    KernelCallPassInput, KernelCallShapeArgument, KernelCallShapeInput, KernelCallShapeParameter,
    KernelCallShapeResolution, KernelCallSyntaxArgument, KernelCallSyntaxInput, KernelCallTarget,
    KernelCallTypeSubstitution, KernelCallableKind, KernelCheckProduct, KernelCheckedLinkLayout,
    KernelCollectionKind, KernelCompileWork, KernelConditionalKind, KernelDeclarationId,
    KernelDeclarationInput, KernelDeclarationKind, KernelDeclarationOrigin,
    KernelDeclarationPresentation, KernelDeclarationReference, KernelDefinitionFactsInput,
    KernelDefinitionPresentation, KernelDefinitionRelocations, KernelDiagnosticKind,
    KernelDiagnosticSeverity, KernelDiagnosticSite, KernelExecutionBlockBindingInput,
    KernelExecutionRecordFieldInput, KernelExecutionShapeInput, KernelExpressionId,
    KernelExpressionPresentation, KernelExpressionRelocation, KernelExpressionSemanticPayload,
    KernelExternalExpression, KernelExternalTarget, KernelHostEffectArtifact,
    KernelInheritedFormal, KernelLexicalAccess, KernelLexicalBindingInput,
    KernelLexicalBindingTarget, KernelLexicalBindingTargetInput, KernelListId, KernelListInput,
    KernelOwnerEdgeRole, KernelOwnerId, KernelOwnerInputEdge, KernelOwnerNode, KernelOwnerNodeKind,
    KernelOwnerProgramInput, KernelParameterEvaluationScope, KernelParameterKind, KernelPattern,
    KernelProjectInput, KernelProjectProgramInput, KernelPureBuiltinKind,
    KernelRenderConstructorKind, KernelScopeId, KernelScopeKind, KernelScopeOrigin,
    KernelScopePresentation, KernelScopeReference, KernelSession, KernelSolveWork, KernelSourceId,
    KernelSourceInput, KernelSourceSpan, KernelStateId, KernelStateInput,
    KernelStatementChildReference, KernelStatementId, KernelStatementInput, KernelStatementKind,
    KernelStatementParameter, KernelStatementPresentation, KernelStatementReference,
    KernelStructuralDeclarationInput, KernelTextTemplateSegment, KernelTypeMismatch,
    KernelValueReference, is_kernel_host_effect, is_registered_kernel_host_effect,
    project_kernel_call_shape, project_kernel_source_expression_diagnostics,
};
use boon_data::{Bits, ExactNumber};
use boon_parser::{ProjectSyntaxSnapshot, UnitOwnerSyntaxView};
use boon_syntax::{
    AstBlockBindingDeclaration, AstCallArgKind, AstExpr, AstExprKind, AstMatchPattern,
    AstParameterKind, AstStatement, AstStatementKind, AstTextSegment, StableCheckOwnerKey,
    StableExpressionKey, StableItemRouteSegment, StableStatementKey, StableStatementKind,
    UnitItemKind, UnitLocalStatementId,
};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::time::{Duration, Instant};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KernelOwnerOracleEntry {
    pub owner: StableCheckOwnerKey,
    pub result_expression: Option<StableExpressionKey>,
    pub formals: Box<[FlowType]>,
    pub result: FlowType,
    pub expressions: Box<[(StableExpressionKey, FlowType)]>,
    pub presentation_scope_count: usize,
    pub execution_shape_count: usize,
    pub statements: Box<[KernelOwnerOracleStatement]>,
    pub declarations: Box<[KernelOwnerOracleDeclaration]>,
    pub lexical_bindings: Box<[KernelOwnerOracleLexicalBinding]>,
    pub collections: Box<[KernelOwnerOracleCollection]>,
    pub sources: Box<[KernelOwnerOracleSource]>,
    pub source_resources: Box<[KernelOwnerOracleSourceResource]>,
    pub states: Box<[KernelOwnerOracleState]>,
    pub lists: Box<[KernelOwnerOracleList]>,
    pub calls: Box<[KernelOwnerOracleCall]>,
    pub effects: Box<[(StableExpressionKey, KernelHostEffectArtifact)]>,
    pub diagnostics: Box<[KernelOwnerOracleDiagnostic]>,
    pub public_child_owner_fields: Box<[(String, StableCheckOwnerKey)]>,
    pub public_child_kernel_fields: Box<[(String, FlowType)]>,
    pub exported_as_public_child: bool,
    pub generic_formal_reads: Box<[StableExpressionKey]>,
    pub structured_delimiter_dependents: Box<[StableExpressionKey]>,
    pub record_spread_dependents: Box<[StableExpressionKey]>,
    pub generic_selector_dependents: Box<[StableExpressionKey]>,
    pub detached_generic_reads: Box<[StableExpressionKey]>,
    pub legacy_no_element_dependents: Box<[StableExpressionKey]>,
    pub legacy_source_container_modes: Box<[StableExpressionKey]>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KernelOwnerOracleDiagnostic {
    pub severity: KernelDiagnosticSeverity,
    pub site: KernelOwnerOracleDiagnosticSite,
    pub kind: KernelDiagnosticKind,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum KernelOwnerOracleDiagnosticSite {
    Expression(StableExpressionKey),
    CallArgument {
        call: StableExpressionKey,
        source: KernelCallArgumentSource,
    },
    CallPass {
        call: StableExpressionKey,
        pipe: bool,
    },
    CallInput {
        call: StableExpressionKey,
        target: StableCheckOwnerKey,
        formal_ordinal: u32,
    },
}

/// Relocate and present kernel-owned source and lexical call diagnostics.
///
/// Source positions remain unit-local in parser arenas. The compiler facade
/// applies the immutable project layout exactly once; neither the kernel nor a
/// legacy checked-program database needs globalized syntax rows.
pub fn present_kernel_source_diagnostic(
    project: &ProjectSyntaxSnapshot,
    owner: &StableCheckOwnerKey,
    diagnostic: &KernelOwnerOracleDiagnostic,
) -> Result<TypeDiagnostic, String> {
    let site = match &diagnostic.site {
        KernelOwnerOracleDiagnosticSite::Expression(site)
        | KernelOwnerOracleDiagnosticSite::CallArgument { call: site, .. }
        | KernelOwnerOracleDiagnosticSite::CallPass { call: site, .. } => site,
        KernelOwnerOracleDiagnosticSite::CallInput { .. } => {
            return Err(
                "source diagnostic presentation received a solved call-input site".to_owned(),
            );
        }
    };
    let view = project
        .owner_view(owner)
        .ok_or_else(|| format!("kernel diagnostic owner has no syntax view: {owner:?}"))?;
    let expression = view
        .expressions()
        .zip(view.stable_expression_keys())
        .find_map(|(expression, stable)| (stable == *site).then_some(expression))
        .ok_or_else(|| format!("kernel diagnostic site is absent from owner {owner:?}"))?;
    let layout = project
        .source_layouts()
        .iter()
        .find(|layout| layout.source_unit_id == site.source_unit_id)
        .ok_or_else(|| {
            format!(
                "kernel diagnostic source unit has no project layout: {:?}",
                site.source_unit_id
            )
        })?;
    let message = match &diagnostic.kind {
        KernelDiagnosticKind::InvalidExpression { tokens } => {
            format!(
                "invalid expression `{}`",
                tokens
                    .iter()
                    .map(|token| token.as_ref())
                    .collect::<Vec<_>>()
                    .join(" ")
            )
        }
        KernelDiagnosticKind::InvalidPattern => "invalid match pattern".to_owned(),
        KernelDiagnosticKind::InvalidNumberLiteral {
            literal, detail, ..
        } => format!("invalid exact Number literal `{literal}`: {detail}"),
        KernelDiagnosticKind::InvalidBitsLiteral { detail, .. } => detail.to_string(),
        KernelDiagnosticKind::ByteLiteralOutsideBytes => {
            "byte literals are only valid as direct BYTES constructor items".to_owned()
        }
        KernelDiagnosticKind::DuplicateRecordField { name } => {
            format!("duplicate explicit record field `{name}`")
        }
        KernelDiagnosticKind::MissingPassedContext => {
            "`PASSED` has no enclosing callable context".to_owned()
        }
        KernelDiagnosticKind::UnresolvedValue { name } => {
            format!("unknown identifier \u{60}{name}\u{60}")
        }
        KernelDiagnosticKind::CallableUsedAsValue { function } => {
            format!(
                "function \u{60}{function}\u{60} must be called with parentheses: \u{60}{function}()\u{60}"
            )
        }
        KernelDiagnosticKind::AmbiguousValue {
            name,
            candidate_count,
        } => format!(
            "ambiguous identifier \u{60}{name}\u{60} has {candidate_count} equally ranked project targets"
        ),
        KernelDiagnosticKind::UnresolvedCallable { function } => {
            format!("unknown function `{function}`")
        }
        KernelDiagnosticKind::AmbiguousCallable {
            function,
            candidate_count,
        } => format!(
            "ambiguous function `{function}` has {candidate_count} equally ranked project targets"
        ),
        KernelDiagnosticKind::PipeWithoutValueInput { function } => {
            format!("`{function}` has no ordinary input for the pipe")
        }
        KernelDiagnosticKind::UnexpectedCallEntry { function, name } => {
            format!("`{function}` has an unexpected extra call entry `{name}`")
        }
        KernelDiagnosticKind::MisorderedCallEntry {
            function,
            position,
            expected_name,
            actual_name,
        } => format!(
            "`{function}` call entry {position} must be `{expected_name}`, found `{actual_name}`; arguments keep declaration names and order"
        ),
        KernelDiagnosticKind::MissingCallEntry { function, name } => {
            format!("`{function}` is missing call entry `{name}`")
        }
        KernelDiagnosticKind::BareOrdinaryInput { name } => {
            format!("bare `{name}` cannot fill ordinary input `{name}`; write `{name}: expression`")
        }
        KernelDiagnosticKind::PassOnAuthoritativeCallable {
            function,
            callable_kind,
        } => format!(
            "`PASS:` is only valid on user callable calls; `{function}` is {}",
            match callable_kind {
                KernelCallableKind::Builtin => "a built-in callable",
                KernelCallableKind::External => "an external callable",
                KernelCallableKind::User => "authoritative",
            }
        ),
        KernelDiagnosticKind::MissingPassContext {
            function,
            root_call,
        } => {
            if *root_call {
                format!("root call to `FUNCTION {function}` requires a final `PASS:` clause")
            } else {
                format!("call to `FUNCTION {function}` requires explicit or inherited PASS context")
            }
        }
        KernelDiagnosticKind::CallInputType { .. } => {
            return Err(
                "source-expression presentation received a call-input diagnostic".to_owned(),
            );
        }
    };
    let (line, start, end) = match &diagnostic.site {
        KernelOwnerOracleDiagnosticSite::Expression(_) => {
            (expression.line, expression.start, expression.end)
        }
        KernelOwnerOracleDiagnosticSite::CallArgument { source, .. } => {
            let argument = match (&expression.kind, source) {
                (
                    AstExprKind::Call { args, .. },
                    KernelCallArgumentSource::CallArgument { ordinal },
                )
                | (
                    AstExprKind::Pipe { args, .. },
                    KernelCallArgumentSource::PipeArgument { ordinal },
                ) => args.get(*ordinal as usize),
                (_, KernelCallArgumentSource::PipeInput) => None,
                _ => None,
            }
            .ok_or_else(|| "kernel call diagnostic has no exact argument anchor".to_owned())?;
            (
                view.physical_line_for_byte(argument.start)
                    .ok_or_else(|| "kernel call argument has no physical source line".to_owned())?,
                argument.start,
                argument.end,
            )
        }
        KernelOwnerOracleDiagnosticSite::CallPass { pipe, .. } => {
            let pass = match &expression.kind {
                AstExprKind::Call { pass, .. } if !*pipe => pass.as_ref(),
                AstExprKind::Pipe { pass, .. } if *pipe => pass.as_ref(),
                _ => None,
            }
            .ok_or_else(|| "kernel call diagnostic has no exact PASS anchor".to_owned())?;
            (
                view.physical_line_for_byte(pass.start)
                    .ok_or_else(|| "kernel call PASS has no physical source line".to_owned())?,
                pass.start,
                pass.end,
            )
        }
        KernelOwnerOracleDiagnosticSite::CallInput { .. } => unreachable!(),
    };
    Ok(TypeDiagnostic {
        severity: match diagnostic.severity {
            KernelDiagnosticSeverity::Error => DiagnosticSeverity::Error,
            KernelDiagnosticSeverity::Warning => DiagnosticSeverity::Warning,
        },
        line: layout
            .start_line
            .checked_add(line.saturating_sub(1))
            .ok_or_else(|| "kernel diagnostic global line overflowed".to_owned())?,
        start: layout
            .start_byte
            .checked_add(start)
            .ok_or_else(|| "kernel diagnostic global start overflowed".to_owned())?,
        end: layout
            .start_byte
            .checked_add(end)
            .ok_or_else(|| "kernel diagnostic global end overflowed".to_owned())?,
        message,
    })
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum KernelOwnerOracleDeclarationOrigin {
    Statement(StableStatementKey),
    Parameter {
        statement: StableStatementKey,
        ordinal: u32,
    },
    RecordField {
        object: StableExpressionKey,
        ordinal: u32,
    },
    PatternBinding {
        arm: StableExpressionKey,
        ordinal: u32,
    },
    CallbackBinding {
        call: StableExpressionKey,
        ordinal: u32,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KernelOwnerOracleDeclaration {
    pub origin: KernelOwnerOracleDeclarationOrigin,
    pub name: Box<str>,
    pub kind: KernelDeclarationKind,
    pub value: Option<KernelOwnerOracleValueReference>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum KernelOwnerOracleLexicalTarget {
    Declaration {
        owner: StableCheckOwnerKey,
        origin: KernelOwnerOracleDeclarationOrigin,
    },
    OwnerPublic(StableCheckOwnerKey),
    ContextFormal {
        owner: StableCheckOwnerKey,
        ordinal: u32,
    },
    Value(KernelOwnerOracleValueReference),
    RuntimeContext,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KernelOwnerOracleLexicalBinding {
    pub expression: StableExpressionKey,
    pub target: KernelOwnerOracleLexicalTarget,
    pub projection: Box<[Box<str>]>,
    pub access: KernelLexicalAccess,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KernelOwnerOracleSemanticPath {
    pub anchor_owner: StableCheckOwnerKey,
    pub anchor: Option<KernelOwnerOracleDeclarationOrigin>,
    pub projection: Box<[Box<str>]>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KernelOwnerOracleSourceResource {
    pub declaration: KernelOwnerOracleLexicalTarget,
    pub statement: StableStatementKey,
    pub expression: StableExpressionKey,
    pub path: KernelOwnerOracleSemanticPath,
    pub interval_ms: Option<u64>,
    pub payload_type: Type,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KernelOwnerOracleState {
    pub binding_declaration: KernelOwnerOracleLexicalTarget,
    pub declaration: KernelOwnerOracleLexicalTarget,
    pub statement: StableStatementKey,
    pub expression: StableExpressionKey,
    pub initial: KernelOwnerOracleValueReference,
    pub path: KernelOwnerOracleSemanticPath,
    pub kind: CheckedStateKind,
    pub flow_type: FlowType,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KernelOwnerOracleList {
    pub declaration: KernelOwnerOracleLexicalTarget,
    pub statement: StableStatementKey,
    pub producer: StableExpressionKey,
    pub path: KernelOwnerOracleSemanticPath,
    pub item_type: Type,
    pub capacity: Option<usize>,
    pub key_policy: CheckedListKeyPolicy,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KernelOwnerOracleCall {
    pub expression: StableExpressionKey,
    pub function: Box<str>,
    pub pipe_input: Option<KernelOwnerOracleValueReference>,
    pub arguments: Box<[KernelOwnerOracleCallSyntaxArgument]>,
    pub pass: Option<KernelOwnerOracleCallPass>,
    pub target: KernelOwnerOracleCallTarget,
    pub inputs: Box<[KernelOwnerOracleCallInput]>,
    pub type_substitutions: Box<[KernelCallTypeSubstitution]>,
    pub result: FlowType,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KernelOwnerOracleCallSyntaxArgument {
    pub kind: KernelCallArgumentKind,
    pub name: Box<str>,
    pub provider: KernelOwnerOracleValueReference,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KernelOwnerOracleCallPass {
    pub provider: KernelOwnerOracleValueReference,
    pub final_clause: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum KernelOwnerOracleCallTarget {
    User {
        target: StableCheckOwnerKey,
        inherited_formal: Option<KernelInheritedFormal>,
    },
    RenderConstructor(KernelRenderConstructorKind),
    PureBuiltin(KernelPureBuiltinKind),
    HostEffect(Box<str>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KernelOwnerOracleCallInput {
    pub role: KernelCallInputRole,
    pub provider: KernelOwnerOracleValueReference,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum KernelOwnerOracleValueReference {
    Expression(StableExpressionKey),
    OwnerResult(StableCheckOwnerKey),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KernelOwnerOracleExpressionInput {
    pub role: KernelOwnerEdgeRole,
    pub provider: KernelOwnerOracleValueReference,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KernelOwnerOracleCollection {
    pub expression: StableExpressionKey,
    pub kind: KernelCollectionKind,
    pub capacity: Option<usize>,
    pub inputs: Box<[KernelOwnerOracleExpressionInput]>,
    pub flow_type: FlowType,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KernelOwnerOracleSource {
    pub expression: StableExpressionKey,
    pub payload_type: Type,
    pub flow_type: FlowType,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KernelOwnerOracleStatement {
    pub statement: StableStatementKey,
    pub kind: KernelStatementKind,
    pub value: Option<KernelOwnerOracleValueReference>,
    pub children: Box<[KernelOwnerOracleStatementChild]>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum KernelOwnerOracleStatementChild {
    Local(StableStatementKey),
    Owner(StableCheckOwnerKey),
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct KernelOwnerOracleReport {
    pub supported: Box<[KernelOwnerOracleEntry]>,
    pub checked_scopes: Box<[CheckedScope]>,
    pub container_owners: Box<[StableCheckOwnerKey]>,
    pub unsupported: Box<[(StableCheckOwnerKey, String)]>,
    pub root_blockers: Box<[KernelOwnerBlockerImpact]>,
    pub dependency_edges: usize,
    pub reverse_consumer_edges: usize,
    pub currentness: Box<[KernelOwnerOracleCurrentness]>,
    pub work: KernelSolveWork,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KernelOwnerOracleCurrentness {
    pub owner: StableCheckOwnerKey,
    pub basis_fingerprint_v9: [u8; 32],
    pub public_result_fingerprint_v1: [u8; 32],
    pub artifact_fingerprint_v11: [u8; 32],
    pub dependency_fingerprint_v1: [u8; 32],
    pub fingerprint_v11: [u8; 32],
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KernelOwnerBlockerImpact {
    pub owner: StableCheckOwnerKey,
    pub reason: String,
    pub affected_owners: usize,
}

/// Directional phase timings for the test-only dense-kernel bridge.
///
/// These values are deliberately kept out of [`KernelOwnerOracleReport`] so
/// semantic/determinism comparisons never include wall time. They are edit-loop
/// observations, not compiler-performance acceptance evidence.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct KernelOwnerOracleTimings {
    pub total_us: u64,
    pub owner_projection_us: u64,
    pub direct_projection_us: u64,
    pub dependency_pruning_us: u64,
    pub program_compile_us: u64,
    pub graph_solve_us: u64,
    pub interface_projection_us: u64,
    pub checked_image_us: u64,
    pub checked_link_layout_us: u64,
    pub checked_link_references: u64,
    pub solve_us: u64,
    pub artifact_projection_us: u64,
    pub input_owners: usize,
    pub projected_owners: usize,
    pub solved_owners: usize,
    pub container_owners: usize,
    pub unsupported_owners: usize,
    pub compile_work: KernelCompileWork,
}

pub fn kernel_owner_oracle(project: &ProjectSyntaxSnapshot) -> KernelOwnerOracleReport {
    kernel_owner_oracle_with_source_payloads(project, &BTreeMap::new())
}

/// Runs the dense kernel over the largest closed owner subgraph supported by
/// the current migration slice.
///
/// SOURCE payloads are explicit ABI inputs rather than answers copied from
/// the legacy owner solver. All structural and cross-owner results are
/// recomputed inside one dense component.
pub fn kernel_owner_oracle_with_source_payloads(
    project: &ProjectSyntaxSnapshot,
    source_payloads: &BTreeMap<String, Type>,
) -> KernelOwnerOracleReport {
    profile_kernel_owner_oracle_with_source_payloads(project, source_payloads).0
}

struct PreparedKernelProjectProjection {
    owner_order: Vec<StableCheckOwnerKey>,
    input_owners: usize,
    prepared: Vec<PreparedOwner>,
    active: Vec<usize>,
    container_owners: Vec<StableCheckOwnerKey>,
    unsupported: BTreeMap<StableCheckOwnerKey, String>,
    root_blocker_by_owner: BTreeMap<StableCheckOwnerKey, StableCheckOwnerKey>,
    project_input: KernelProjectProgramInput,
    definition_facts: Box<[KernelDefinitionFactsInput]>,
    definition_keys: Box<[StableCheckOwnerKey]>,
    project_is_empty: bool,
    owner_projection_us: u64,
    direct_projection_elapsed: Duration,
    dependency_pruning_us: u64,
}

fn prepare_kernel_project_projection(
    project: &ProjectSyntaxSnapshot,
    source_payloads: &BTreeMap<String, Type>,
) -> PreparedKernelProjectProjection {
    let owner_order = project.stable_check_owner_keys().collect::<Vec<_>>();
    let input_owners = owner_order.len();
    let value_surfaces = project_value_surfaces(project);
    let authoritative_call_shapes = project_kernel_authoritative_call_shapes();
    let callable_surfaces = authoritative_call_shapes
        .as_ref()
        .map_err(Clone::clone)
        .and_then(|authoritative| project_callable_surfaces(project, authoritative));
    let owner_projection_started = Instant::now();
    let mut direct_projection_elapsed = Duration::ZERO;
    let mut prepared = Vec::<PreparedOwner>::new();
    let mut container_owners = Vec::<StableCheckOwnerKey>::new();
    let mut unsupported = BTreeMap::<StableCheckOwnerKey, String>::new();
    for owner in &owner_order {
        let Some(view) = project.owner_view(owner) else {
            unsupported.insert(owner.clone(), "owner has no syntax view".to_owned());
            continue;
        };
        if matches!(owner, StableCheckOwnerKey::UnitRoot(_)) && view.statement_ids().is_empty() {
            container_owners.push(owner.clone());
            continue;
        }
        let outcome = (|| {
            let direct_projection_started = Instant::now();
            let compact = compact_owner_view(
                view,
                source_payloads,
                callable_surfaces.as_ref().map_err(Clone::clone)?,
                authoritative_call_shapes.as_ref().map_err(Clone::clone)?,
                &value_surfaces,
            );
            direct_projection_elapsed += direct_projection_started.elapsed();
            compact
        })();
        match outcome {
            Ok(owner) => prepared.push(owner),
            Err(reason) => {
                unsupported.insert(owner.clone(), reason);
            }
        }
    }
    let mut resource_ordinals =
        BTreeMap::<(StableCheckOwnerKey, PreparedResourceSyntheticKind), usize>::new();
    for owner in &mut prepared {
        for synthetic in &owner.resource_synthetic_paths {
            let ordinal = resource_ordinals
                .entry((synthetic.anchor.clone(), synthetic.kind))
                .or_default();
            let projection = match synthetic.kind {
                PreparedResourceSyntheticKind::State => owner
                    .definition_facts
                    .states
                    .get_mut(synthetic.row)
                    .map(|state| &mut state.projection),
                PreparedResourceSyntheticKind::List => owner
                    .definition_facts
                    .lists
                    .get_mut(synthetic.row)
                    .map(|list| &mut list.projection),
            }
            .expect("prepared synthetic resource row is in range");
            assert!(
                projection.is_empty(),
                "a synthetic resource path must begin without a structural projection"
            );
            let prefix = match synthetic.kind {
                PreparedResourceSyntheticKind::State => "state",
                PreparedResourceSyntheticKind::List => "list",
            };
            *projection = vec![format!("{prefix}_{ordinal}").into_boxed_str()].into_boxed_slice();
            *ordinal = ordinal.saturating_add(1);
        }
    }
    let mut root_blocker_by_owner = unsupported
        .keys()
        .cloned()
        .map(|owner| (owner.clone(), owner))
        .collect::<BTreeMap<_, _>>();
    let owner_projection_us = elapsed_us(owner_projection_started.elapsed());

    let dependency_pruning_started = Instant::now();
    let prepared_by_owner = prepared
        .iter()
        .enumerate()
        .map(|(index, owner)| (owner.owner.clone(), index))
        .collect::<BTreeMap<_, _>>();
    let mut active = (0..prepared.len()).collect::<BTreeSet<_>>();
    loop {
        let rejected = active
            .iter()
            .filter_map(|index| {
                let owner = &prepared[*index];
                let external_reason = owner.external_expressions.iter().find_map(|external| {
                    let Some(target) = prepared_by_owner.get(&external.owner).copied() else {
                        return Some((
                            format!("depends on unsupported owner {:#?}", external.owner),
                            external.owner.clone(),
                        ));
                    };
                    if !active.contains(&target) {
                        return Some((
                            format!("depends on unsupported owner {:#?}", external.owner),
                            root_blocker_by_owner
                                .get(&external.owner)
                                .cloned()
                                .unwrap_or_else(|| external.owner.clone()),
                        ));
                    }
                    match &external.target {
                        PreparedExternalTarget::Result => None,
                        PreparedExternalTarget::Expression(expression) => (!prepared[target]
                            .expressions
                            .iter()
                            .any(|candidate| candidate == expression))
                        .then(|| {
                            (
                                format!(
                                    "imports missing expression {expression:#?} from owner {:#?}",
                                    external.owner
                                ),
                                owner.owner.clone(),
                            )
                        }),
                    }
                });
                let call_reason = owner.call_targets.iter().find_map(|call| {
                    let Some(target) = prepared_by_owner.get(&call.owner).copied() else {
                        return Some((
                            format!("depends on unsupported owner {:#?}", call.owner),
                            call.owner.clone(),
                        ));
                    };
                    (!active.contains(&target)).then(|| {
                        (
                            format!("depends on unsupported owner {:#?}", call.owner),
                            root_blocker_by_owner
                                .get(&call.owner)
                                .cloned()
                                .unwrap_or_else(|| call.owner.clone()),
                        )
                    })
                });
                let statement_child_reason =
                    owner.statement_child_targets.iter().find_map(|child| {
                        let Some(target) = prepared_by_owner.get(&child.owner).copied() else {
                            return Some((
                                format!("depends on unsupported owner {:#?}", child.owner),
                                child.owner.clone(),
                            ));
                        };
                        (!active.contains(&target)).then(|| {
                            (
                                format!("depends on unsupported owner {:#?}", child.owner),
                                root_blocker_by_owner
                                    .get(&child.owner)
                                    .cloned()
                                    .unwrap_or_else(|| child.owner.clone()),
                            )
                        })
                    });
                let lexical_owner_reason = owner.lexical_owner_targets.iter().find_map(|target| {
                    let Some(prepared_target) = prepared_by_owner.get(&target.owner).copied()
                    else {
                        return Some((
                            format!(
                                "lexical binding depends on unsupported owner {:#?}",
                                target.owner
                            ),
                            target.owner.clone(),
                        ));
                    };
                    (!active.contains(&prepared_target)).then(|| {
                        (
                            format!(
                                "lexical binding depends on unsupported owner {:#?}",
                                target.owner
                            ),
                            root_blocker_by_owner
                                .get(&target.owner)
                                .cloned()
                                .unwrap_or_else(|| target.owner.clone()),
                        )
                    })
                });
                let resource_owner_reason =
                    owner.resource_owner_targets.iter().find_map(|target| {
                        let Some(prepared_target) = prepared_by_owner.get(&target.owner).copied()
                        else {
                            return Some((
                                format!(
                                    "resource depends on unsupported owner {:#?}",
                                    target.owner
                                ),
                                target.owner.clone(),
                            ));
                        };
                        (!active.contains(&prepared_target)).then(|| {
                            (
                                format!(
                                    "resource depends on unsupported owner {:#?}",
                                    target.owner
                                ),
                                root_blocker_by_owner
                                    .get(&target.owner)
                                    .cloned()
                                    .unwrap_or_else(|| target.owner.clone()),
                            )
                        })
                    });
                external_reason
                    .or(call_reason)
                    .or(statement_child_reason)
                    .or(lexical_owner_reason)
                    .or(resource_owner_reason)
                    .map(|(reason, root)| (*index, reason, root))
            })
            .collect::<Vec<_>>();
        if rejected.is_empty() {
            break;
        }
        for (index, reason, root) in rejected {
            active.remove(&index);
            root_blocker_by_owner.insert(prepared[index].owner.clone(), root);
            unsupported.insert(prepared[index].owner.clone(), reason);
        }
    }

    let active = active.into_iter().collect::<Vec<_>>();
    let dependency_pruning_us = elapsed_us(dependency_pruning_started.elapsed());
    let mut dense_owner = vec![None; prepared.len()];
    for (dense, prepared_index) in active.iter().copied().enumerate() {
        dense_owner[prepared_index] = Some(KernelOwnerId(
            u32::try_from(dense).expect("kernel oracle owner count exceeds u32"),
        ));
    }
    let mut containing_placements = vec![None; prepared.len()];
    for parent in active.iter().copied() {
        for target in &prepared[parent].containing_scope_targets {
            let child = prepared_by_owner[&target.owner];
            if dense_owner[child].is_none() {
                continue;
            }
            let placement = (parent, target.scope);
            match containing_placements[child] {
                None => containing_placements[child] = Some(placement),
                Some(previous) if previous == placement => {}
                Some(previous) => panic!(
                    "kernel owner has conflicting containing-scope placements {previous:?} and {placement:?}"
                ),
            }
        }
    }
    let mut resolved_containing_scopes = vec![None; prepared.len()];
    for owner in active.iter().copied() {
        resolve_prepared_containing_scope(
            owner,
            &containing_placements,
            &dense_owner,
            &mut resolved_containing_scopes,
            &mut BTreeSet::new(),
        );
    }
    if let Some(dense) = std::env::var_os("BOON_KERNEL_ORACLE_TRACE_DENSE_OWNER")
        .and_then(|dense| dense.to_string_lossy().parse::<usize>().ok())
        && let Some(prepared_index) = active.get(dense).copied()
    {
        let compact = &prepared[prepared_index].compact;
        eprintln!(
            "kernel-owner-trace dense_owner={dense} stable_owner={:#?} formals={} result={} nodes={}",
            prepared[prepared_index].owner,
            compact.formal_count,
            compact.result.0,
            compact.nodes.len(),
        );
        for (expression, node) in compact.nodes.iter().enumerate() {
            eprintln!(
                "kernel-owner-node expression={expression} mode={:?} kind={:?} inputs={:?}",
                node.mode, node.kind, node.inputs,
            );
        }
        for call in &prepared[prepared_index].call_targets {
            let target = prepared_by_owner[&call.owner];
            eprintln!(
                "kernel-owner-call expression={} dense_target={} stable_target={:#?}",
                call.node,
                dense_owner[target]
                    .expect("active traced call target has a dense owner")
                    .0,
                call.owner,
            );
        }
    }
    let project_input = KernelProjectProgramInput {
        owners: active
            .iter()
            .map(|prepared_index| {
                let owner = &prepared[*prepared_index];
                let mut compact = owner.compact.clone();
                compact.external_expressions = owner
                    .external_expressions
                    .iter()
                    .map(|external| {
                        let target = prepared_by_owner[&external.owner];
                        let kernel_target = match &external.target {
                            PreparedExternalTarget::Result => KernelExternalTarget::Result,
                            PreparedExternalTarget::Expression(expression) => {
                                let expression = prepared[target]
                                    .expressions
                                    .iter()
                                    .position(|candidate| candidate == expression)
                                    .expect("active external expression was validated");
                                KernelExternalTarget::Expression(KernelExpressionId(
                                    u32::try_from(expression)
                                        .expect("kernel owner expression count exceeds u32"),
                                ))
                            }
                        };
                        KernelExternalExpression {
                            owner: dense_owner[target].expect("active target has a dense owner"),
                            target: kernel_target,
                        }
                    })
                    .collect::<Vec<_>>()
                    .into_boxed_slice();
                for call in &owner.call_targets {
                    let target = prepared_by_owner[&call.owner];
                    let node = compact
                        .nodes
                        .get_mut(call.node)
                        .expect("prepared user call node is local");
                    let KernelOwnerNodeKind::UserCall {
                        target: call_target,
                        ..
                    } = &mut node.kind
                    else {
                        panic!("prepared user call target references a non-call node")
                    };
                    *call_target =
                        dense_owner[target].expect("active call target has a dense owner");
                }
                compact
            })
            .collect::<Vec<_>>()
            .into_boxed_slice(),
    };
    let definition_facts = active
        .iter()
        .map(|prepared_index| {
            let owner = &prepared[*prepared_index];
            let dense_current =
                dense_owner[*prepared_index].expect("active resource owner has a dense owner ID");
            let mut facts = owner.definition_facts.clone();
            facts.presentation.containing_scope = resolved_containing_scopes[*prepared_index]
                .expect("active owner has a resolved containing scope");
            for target in &owner.statement_child_targets {
                let prepared_target = prepared_by_owner[&target.owner];
                let child = facts.statements[target.statement]
                    .children
                    .get_mut(target.child)
                    .expect("prepared statement child target is in range");
                let KernelStatementChildReference::Owner(owner) = child else {
                    panic!("prepared external statement child became local")
                };
                *owner = dense_owner[prepared_target]
                    .expect("active statement child target has a dense owner");
            }
            for target in &owner.lexical_owner_targets {
                let prepared_target = prepared_by_owner[&target.owner];
                let binding = facts
                    .lexical_bindings
                    .get_mut(target.binding)
                    .expect("prepared lexical owner target is in range");
                let KernelLexicalBindingTargetInput::Declaration(
                    KernelDeclarationReference::OwnerPublic(owner),
                ) = &mut binding.target
                else {
                    panic!("prepared lexical owner target became local")
                };
                *owner =
                    dense_owner[prepared_target].expect("active lexical target has a dense owner");
            }
            for target in &owner.resource_owner_targets {
                let prepared_target = prepared_by_owner[&target.owner];
                let dense_target =
                    dense_owner[prepared_target].expect("active resource target has a dense owner");
                match target.field {
                    PreparedResourceOwnerField::LinkagePublicDeclaration => {
                        let Some(KernelDeclarationReference::OwnerPublic(owner)) =
                            &mut facts.linkage.public_declaration
                        else {
                            panic!("prepared definition public declaration target became local")
                        };
                        *owner = dense_target;
                    }
                    PreparedResourceOwnerField::SourceDeclaration(row) => {
                        let KernelDeclarationReference::OwnerPublic(owner) =
                            &mut facts.sources[row].declaration
                        else {
                            panic!("prepared SOURCE declaration target became local")
                        };
                        *owner = dense_target;
                    }
                    PreparedResourceOwnerField::SourceStatement(row) => {
                        let KernelStatementReference::OwnerPublic(owner) =
                            &mut facts.sources[row].statement
                        else {
                            panic!("prepared SOURCE statement target became local")
                        };
                        *owner = dense_target;
                    }
                    PreparedResourceOwnerField::StateBindingDeclaration(row) => {
                        let KernelDeclarationReference::OwnerPublic(owner) =
                            &mut facts.states[row].binding_declaration
                        else {
                            panic!("prepared state binding target became local")
                        };
                        *owner = dense_target;
                    }
                    PreparedResourceOwnerField::StateDeclaration(row) => {
                        let KernelDeclarationReference::OwnerPublic(owner) =
                            &mut facts.states[row].declaration
                        else {
                            panic!("prepared state declaration target became local")
                        };
                        *owner = dense_target;
                    }
                    PreparedResourceOwnerField::StateStatement(row) => {
                        let KernelStatementReference::OwnerPublic(owner) =
                            &mut facts.states[row].statement
                        else {
                            panic!("prepared state statement target became local")
                        };
                        *owner = dense_target;
                    }
                    PreparedResourceOwnerField::ListDeclaration(row) => {
                        let KernelDeclarationReference::OwnerPublic(owner) =
                            &mut facts.lists[row].declaration
                        else {
                            panic!("prepared LIST declaration target became local")
                        };
                        *owner = dense_target;
                    }
                    PreparedResourceOwnerField::ListStatement(row) => {
                        let parent_authority =
                            project_inline_list_authority_owner(&project_input, dense_target)
                                == Some(dense_current);
                        if parent_authority {
                            facts.lists[row].statement =
                                KernelStatementReference::OwnerPublic(dense_target);
                        } else if let KernelStatementReference::OwnerPublic(owner) =
                            &mut facts.lists[row].statement
                        {
                            // This is the no-local-statement fallback. It is
                            // still an owner reference and must be relocated.
                            *owner = dense_target;
                        }
                    }
                }
            }
            facts
        })
        .collect::<Vec<_>>()
        .into_boxed_slice();

    let project_is_empty = project_input.owners.is_empty();
    let definition_keys = active
        .iter()
        .map(|prepared_index| prepared[*prepared_index].owner.clone())
        .collect::<Vec<_>>()
        .into_boxed_slice();
    PreparedKernelProjectProjection {
        owner_order,
        input_owners,
        prepared,
        active,
        container_owners,
        unsupported,
        root_blocker_by_owner,
        project_input,
        definition_facts,
        definition_keys,
        project_is_empty,
        owner_projection_us,
        direct_projection_elapsed,
        dependency_pruning_us,
    }
}

/// Profile the compatibility projection, dense compilation, and dense solve
/// independently. Production compilation never calls this bridge.
pub fn profile_kernel_owner_oracle_with_source_payloads(
    project: &ProjectSyntaxSnapshot,
    source_payloads: &BTreeMap<String, Type>,
) -> (KernelOwnerOracleReport, KernelOwnerOracleTimings) {
    let total_started = Instant::now();
    let PreparedKernelProjectProjection {
        owner_order,
        input_owners,
        prepared,
        active,
        container_owners,
        mut unsupported,
        mut root_blocker_by_owner,
        project_input,
        definition_facts,
        definition_keys,
        project_is_empty,
        owner_projection_us,
        direct_projection_elapsed,
        dependency_pruning_us,
    } = prepare_kernel_project_projection(project, source_payloads);
    let mut program_compile_us = 0;
    let mut graph_solve_us = 0;
    let mut interface_projection_us = 0;
    let mut checked_image_us = 0;
    let mut checked_link_layout_us = 0;
    let mut checked_link_references = 0;
    let mut checked_scopes: Box<[CheckedScope]> = Box::new([]);
    let mut solve_us = 0;
    let mut compile_work = KernelCompileWork::default();
    let artifact = if project_is_empty {
        None
    } else {
        let compile_started = Instant::now();
        let input = KernelProjectInput::new(project_input, definition_facts, definition_keys)
            .map_err(|error| error.to_string());
        let compiled = input
            .as_ref()
            .map_err(Clone::clone)
            .and_then(|input| input.compile().map_err(|error| error.to_string()));
        if let Ok(program) = &compiled {
            compile_work = program.compile_work();
        }
        program_compile_us = elapsed_us(compile_started.elapsed());
        let solved = compiled.and_then(|program| {
            let graph_solve_started = Instant::now();
            let solved = program.solve_graph().map_err(|error| error.to_string());
            graph_solve_us = elapsed_us(graph_solve_started.elapsed());
            solve_us = graph_solve_us;
            solved.and_then(|solved| {
                if std::env::var_os("BOON_KERNEL_MEASURE_DEMANDS").is_some() {
                    let interface_projection_started = Instant::now();
                    let interfaces = solved.interface_snapshot();
                    interface_projection_us = elapsed_us(interface_projection_started.elapsed());
                    debug_assert_eq!(interfaces.public_results.len(), active.len());
                }
                let checked_image_started = Instant::now();
                let checked = solved
                    .into_checked_snapshot()
                    .map_err(|error| error.to_string());
                checked_image_us = elapsed_us(checked_image_started.elapsed());
                solve_us = graph_solve_us.saturating_add(checked_image_us);
                checked.and_then(|checked| {
                    let checked_link_started = Instant::now();
                    let kernel_input = input
                        .as_ref()
                        .expect("a solved kernel graph retains its immutable input");
                    let layout = KernelCheckedLinkLayout::new(kernel_input, &checked)
                        .map_err(|error| error.to_string())?;
                    checked_link_layout_us = elapsed_us(checked_link_started.elapsed());
                    checked_link_references = layout.totals().resolved_references;
                    let mut materialized_scopes = layout
                        .materialize_scopes(&checked)
                        .map_err(|error| error.to_string())?
                        .into_vec();
                    for definition in layout.definitions() {
                        let key = kernel_input
                            .links()
                            .definition_key(definition.owner)
                            .ok_or_else(|| {
                                format!(
                                    "kernel checked scope linker has no stable key for definition {}",
                                    definition.owner.0,
                                )
                            })?;
                        let source = project
                            .source_layouts()
                            .iter()
                            .find(|source| &source.source_unit_id == key.source_unit_id())
                            .ok_or_else(|| {
                                format!(
                                    "kernel checked scope linker has no source layout for {:?}",
                                    key.source_unit_id(),
                                )
                            })?;
                        for row in definition.scopes.start
                            ..definition
                                .scopes
                                .start
                                .checked_add(definition.scopes.len)
                                .ok_or_else(|| {
                                    "kernel checked scope range overflowed".to_owned()
                                })?
                        {
                            let scope = materialized_scopes
                                .get_mut(row as usize)
                                .ok_or_else(|| {
                                    format!(
                                        "kernel checked scope linker references missing row {row}"
                                    )
                                })?;
                            scope.span.line = source
                                .start_line
                                .checked_add(scope.span.line.checked_sub(1).ok_or_else(|| {
                                    format!("kernel checked scope row {row} has no source line")
                                })?)
                                .ok_or_else(|| {
                                    format!("kernel checked scope row {row} line overflowed")
                                })?;
                            scope.span.start = source
                                .start_byte
                                .checked_add(scope.span.start)
                                .ok_or_else(|| {
                                    format!("kernel checked scope row {row} start overflowed")
                                })?;
                            scope.span.end = source
                                .start_byte
                                .checked_add(scope.span.end)
                                .ok_or_else(|| {
                                    format!("kernel checked scope row {row} end overflowed")
                                })?;
                        }
                    }
                    checked_scopes = materialized_scopes.into_boxed_slice();
                    Ok(checked)
                })
            })
        });
        match solved {
            Ok(artifact) => Some(artifact),
            Err(reason) => {
                for prepared_index in &active {
                    root_blocker_by_owner.insert(
                        prepared[*prepared_index].owner.clone(),
                        prepared[*prepared_index].owner.clone(),
                    );
                    unsupported.insert(
                        prepared[*prepared_index].owner.clone(),
                        format!("kernel project solve failed: {reason}"),
                    );
                }
                None
            }
        }
    };

    let artifact_projection_started = Instant::now();
    let (supported, work, dependency_edges, reverse_consumer_edges, currentness) = artifact
        .map_or_else(
        || {
            (
                Vec::new(),
                KernelSolveWork::default(),
                0,
                0,
                Vec::new(),
            )
        },
        |artifact| {
            let work = artifact.work;
            let dependency_edges = artifact.dependencies.dependency_count();
            let reverse_consumer_edges = artifact.dependencies.reverse_consumer_count();
            let currentness = active
                .iter()
                .zip(&artifact.currentness)
                .map(|(prepared_index, receipt)| KernelOwnerOracleCurrentness {
                    owner: prepared[*prepared_index].owner.clone(),
                    basis_fingerprint_v9: receipt.basis_fingerprint_v9,
                    public_result_fingerprint_v1: receipt.public_result_fingerprint_v1,
                    artifact_fingerprint_v11: receipt.artifact_fingerprint_v11,
                    dependency_fingerprint_v1: receipt.dependency_fingerprint_v1,
                    fingerprint_v11: receipt.fingerprint_v11,
                })
                .collect::<Vec<_>>();
            let definitions = artifact.definitions;
            let result_by_owner = active
                .iter()
                .zip(&definitions)
                .map(|(prepared_index, artifact)| {
                    (prepared[*prepared_index].owner.clone(), artifact.result.clone())
                })
                .collect::<BTreeMap<_, _>>();
            let exported_public_children = prepared
                .iter()
                .flat_map(|owner| {
                    owner
                        .public_child_owner_fields
                        .iter()
                        .map(|(_, child)| child.clone())
                })
                .collect::<BTreeSet<_>>();
            let supported = active
                .iter()
                .zip(definitions)
                .enumerate()
                .map(|(dense_index, (prepared_index, artifact))| {
                    let owner = &prepared[*prepared_index];
                    assert_eq!(
                        artifact.relocations.expressions,
                        owner.definition_facts.relocations.expressions,
                        "kernel definition artifacts retain every stable expression relocation"
                    );
                    assert_eq!(
                        artifact.relocations.statements,
                        owner.definition_facts.relocations.statements,
                        "kernel definition artifacts retain every stable statement relocation"
                    );
                    assert_eq!(
                        artifact.presentation.scopes,
                        owner.definition_facts.presentation.scopes,
                        "kernel definition artifacts retain every compact scope row"
                    );
                    assert_eq!(
                        artifact.presentation.expressions,
                        owner.definition_facts.presentation.expressions,
                        "kernel definition artifacts retain every checked-expression presentation row"
                    );
                    assert_eq!(
                        artifact.presentation.statements,
                        owner.definition_facts.presentation.statements,
                        "kernel definition artifacts retain every checked-statement presentation row"
                    );
                    assert_eq!(
                        artifact.presentation.declarations,
                        owner.definition_facts.presentation.declarations,
                        "kernel definition artifacts retain every checked-declaration presentation row"
                    );
                    let presentation_scope_count = artifact.presentation.scopes.len();
                    assert_eq!(
                        artifact.expression_payloads,
                        owner.definition_facts.expression_payloads,
                        "kernel definition artifacts retain every exact expression semantic payload"
                    );
                    assert_eq!(
                        artifact.call_syntax.len(),
                        owner.definition_facts.call_syntax.len(),
                        "kernel definition artifacts retain every authored call surface"
                    );
                    for (linked, authored) in artifact
                        .call_syntax
                        .iter()
                        .zip(owner.definition_facts.call_syntax.iter())
                    {
                        assert_eq!(linked.expression, authored.expression);
                        assert_eq!(linked.function, authored.function);
                        assert_eq!(linked.pipe_input.is_some(), authored.pipe_input.is_some());
                        assert_eq!(linked.arguments.len(), authored.arguments.len());
                        for (linked, authored) in
                            linked.arguments.iter().zip(authored.arguments.iter())
                        {
                            assert_eq!(linked.ordinal, authored.ordinal);
                            assert_eq!(linked.kind, authored.kind);
                            assert_eq!(linked.name, authored.name);
                        }
                        assert_eq!(
                            linked.pass.map(|pass| pass.final_clause),
                            authored.pass.map(|pass| pass.final_clause),
                        );
                    }
                    assert_eq!(
                        artifact.execution_shapes.len(),
                        owner.definition_facts.execution_shapes.len(),
                        "kernel definition artifacts retain every lossy structural execution shape"
                    );
                    for (linked, authored) in artifact
                        .execution_shapes
                        .iter()
                        .zip(owner.definition_facts.execution_shapes.iter())
                    {
                        assert_eq!(linked.expression(), authored.expression());
                    }
                    let execution_shape_count = artifact.execution_shapes.len();
                    let stable_provider = |value: KernelValueReference| match value {
                        KernelValueReference::Local(expression) => {
                            KernelOwnerOracleValueReference::Expression(
                                owner.expressions[expression.0 as usize].clone(),
                            )
                        }
                        KernelValueReference::External(external) => {
                            let target = active
                                .get(external.owner.0 as usize)
                                .and_then(|target| prepared.get(*target))
                                .unwrap_or_else(|| {
                                    panic!(
                                        "kernel artifact input targets missing dense owner {}",
                                        external.owner.0
                                    )
                                });
                            match external.target {
                                KernelExternalTarget::Expression(expression) => {
                                    KernelOwnerOracleValueReference::Expression(
                                        target.expressions[expression.0 as usize].clone(),
                                    )
                                }
                                KernelExternalTarget::Result => target
                                    .result_expression
                                    .clone()
                                    .map(KernelOwnerOracleValueReference::Expression)
                                    .unwrap_or_else(|| {
                                        KernelOwnerOracleValueReference::OwnerResult(
                                            target.owner.clone(),
                                        )
                                    }),
                            }
                        }
                    };
                    let collections = artifact
                        .expressions
                        .iter()
                        .filter_map(|expression| {
                            let KernelOwnerNodeKind::Collection { kind, capacity } = expression.kind
                            else {
                                return None;
                            };
                            Some(KernelOwnerOracleCollection {
                                expression: owner.expressions[expression.id.0 as usize].clone(),
                                kind,
                                capacity,
                                inputs: expression
                                    .inputs
                                    .iter()
                                    .map(|input| KernelOwnerOracleExpressionInput {
                                        role: input.role.clone(),
                                        provider: stable_provider(input.value),
                                    })
                                    .collect::<Vec<_>>()
                                    .into_boxed_slice(),
                                flow_type: expression.flow_type.clone(),
                            })
                        })
                        .collect::<Vec<_>>()
                        .into_boxed_slice();
                    let sources = artifact
                        .expressions
                        .iter()
                        .filter_map(|expression| {
                            let KernelOwnerNodeKind::Source(payload_type) = &expression.kind else {
                                return None;
                            };
                            Some(KernelOwnerOracleSource {
                                expression: owner.expressions[expression.id.0 as usize].clone(),
                                payload_type: payload_type.clone(),
                                flow_type: expression.flow_type.clone(),
                            })
                        })
                        .collect::<Vec<_>>()
                        .into_boxed_slice();
                    assert_eq!(
                        artifact.statements.len(),
                        owner.statements.len(),
                        "kernel statement artifacts retain one stable row each"
                    );
                    let statements = artifact
                        .statements
                        .iter()
                        .map(|statement| KernelOwnerOracleStatement {
                            statement: owner.statements[statement.id.0 as usize].clone(),
                            kind: statement.kind.clone(),
                            value: statement.value.map(&stable_provider),
                            children: statement
                                .children
                                .iter()
                                .map(|child| match child {
                                    KernelStatementChildReference::Local(child) => {
                                        KernelOwnerOracleStatementChild::Local(
                                            owner.statements[child.0 as usize].clone(),
                                        )
                                    }
                                    KernelStatementChildReference::Owner(child) => {
                                        let child = active
                                            .get(child.0 as usize)
                                            .and_then(|child| prepared.get(*child))
                                            .unwrap_or_else(|| {
                                                panic!(
                                                    "kernel statement child targets missing dense owner {}",
                                                    child.0
                                                )
                                            });
                                        KernelOwnerOracleStatementChild::Owner(child.owner.clone())
                                    }
                                })
                                .collect::<Vec<_>>()
                                .into_boxed_slice(),
                        })
                        .collect::<Vec<_>>()
                        .into_boxed_slice();
                    let declaration_origins = artifact
                        .declarations
                        .iter()
                        .map(|declaration| {
                            stable_kernel_declaration_origin(owner, &declaration.origin)
                        })
                        .collect::<Vec<_>>();
                    let declarations = artifact
                        .declarations
                        .iter()
                        .map(|declaration| KernelOwnerOracleDeclaration {
                            origin: declaration_origins[declaration.id.0 as usize].clone(),
                            name: declaration.name.clone(),
                            kind: declaration.kind,
                            value: declaration.value.map(&stable_provider),
                        })
                        .collect::<Vec<_>>()
                        .into_boxed_slice();
                    let lexical_bindings = artifact
                        .lexical_bindings
                        .iter()
                        .map(|binding| {
                            let target = match &binding.target {
                                KernelLexicalBindingTarget::Declaration(
                                    KernelDeclarationReference::Local(declaration),
                                ) => KernelOwnerOracleLexicalTarget::Declaration {
                                    owner: owner.owner.clone(),
                                    origin: declaration_origins[declaration.0 as usize].clone(),
                                },
                                KernelLexicalBindingTarget::Declaration(
                                    KernelDeclarationReference::OwnerPublic(target),
                                ) => {
                                    let target = active
                                        .get(target.0 as usize)
                                        .and_then(|target| prepared.get(*target))
                                        .unwrap_or_else(|| {
                                            panic!(
                                                "kernel lexical binding targets missing dense owner {}",
                                                target.0
                                            )
                                        });
                                    KernelOwnerOracleLexicalTarget::OwnerPublic(
                                        target.owner.clone(),
                                    )
                                }
                                KernelLexicalBindingTarget::ContextFormal { ordinal } => {
                                    KernelOwnerOracleLexicalTarget::ContextFormal {
                                        owner: owner.owner.clone(),
                                        ordinal: *ordinal,
                                    }
                                }
                                KernelLexicalBindingTarget::Value { provider } => {
                                    KernelOwnerOracleLexicalTarget::Value(stable_provider(*provider))
                                }
                                KernelLexicalBindingTarget::RuntimeContext => {
                                    KernelOwnerOracleLexicalTarget::RuntimeContext
                                }
                            };
                            KernelOwnerOracleLexicalBinding {
                                expression: owner.expressions[binding.expression.0 as usize].clone(),
                                target,
                                projection: binding.projection.clone(),
                                access: binding.access,
                            }
                        })
                        .collect::<Vec<_>>()
                        .into_boxed_slice();
                    let stable_declaration = |reference: KernelDeclarationReference| match reference {
                        KernelDeclarationReference::Local(declaration) => {
                            KernelOwnerOracleLexicalTarget::Declaration {
                                owner: owner.owner.clone(),
                                origin: declaration_origins[declaration.0 as usize].clone(),
                            }
                        }
                        KernelDeclarationReference::OwnerPublic(target) => {
                            let target = active
                                .get(target.0 as usize)
                                .and_then(|target| prepared.get(*target))
                                .unwrap_or_else(|| {
                                    panic!(
                                        "kernel resource targets missing dense owner {}",
                                        target.0
                                    )
                                });
                            KernelOwnerOracleLexicalTarget::OwnerPublic(target.owner.clone())
                        }
                    };
                    let stable_path = |path: &boon_compiler_kernel::KernelSemanticPath| {
                        match path.anchor {
                            KernelDeclarationReference::Local(declaration) => {
                                KernelOwnerOracleSemanticPath {
                                    anchor_owner: owner.owner.clone(),
                                    anchor: Some(
                                        declaration_origins[declaration.0 as usize].clone(),
                                    ),
                                    projection: path.projection.clone(),
                                }
                            }
                            KernelDeclarationReference::OwnerPublic(target) => {
                                let target = active
                                    .get(target.0 as usize)
                                    .and_then(|target| prepared.get(*target))
                                    .unwrap_or_else(|| {
                                        panic!(
                                            "kernel resource path targets missing dense owner {}",
                                            target.0
                                        )
                                    });
                                KernelOwnerOracleSemanticPath {
                                    anchor_owner: target.owner.clone(),
                                    anchor: None,
                                    projection: path.projection.clone(),
                                }
                            }
                        }
                    };
                    let stable_statement = |reference: KernelStatementReference| match reference {
                        KernelStatementReference::Local(statement) => {
                            owner.statements[statement.0 as usize].clone()
                        }
                        KernelStatementReference::OwnerPublic(target) => {
                            let target = active
                                .get(target.0 as usize)
                                .and_then(|target| prepared.get(*target))
                                .unwrap_or_else(|| {
                                    panic!(
                                        "kernel resource statement targets missing dense owner {}",
                                        target.0
                                    )
                                });
                            target
                                .statements
                                .first()
                                .cloned()
                                .expect("a public resource owner has a root statement")
                        }
                    };
                    let source_resources = artifact
                        .sources
                        .iter()
                        .map(|source| KernelOwnerOracleSourceResource {
                            declaration: stable_declaration(source.declaration),
                            statement: stable_statement(source.statement),
                            expression: owner.expressions[source.expression.0 as usize].clone(),
                            path: stable_path(&source.path),
                            interval_ms: source.interval_ms,
                            payload_type: source.payload_type.clone(),
                        })
                        .collect::<Vec<_>>()
                        .into_boxed_slice();
                    let states = artifact
                        .states
                        .iter()
                        .map(|state| KernelOwnerOracleState {
                            binding_declaration: stable_declaration(state.binding_declaration),
                            declaration: stable_declaration(state.declaration),
                            statement: stable_statement(state.statement),
                            expression: owner.expressions[state.expression.0 as usize].clone(),
                            initial: stable_provider(state.initial),
                            path: stable_path(&state.path),
                            kind: state.kind,
                            flow_type: state.flow_type.clone(),
                        })
                        .collect::<Vec<_>>()
                        .into_boxed_slice();
                    let lists = artifact
                        .lists
                        .iter()
                        .map(|list| KernelOwnerOracleList {
                            declaration: stable_declaration(list.declaration),
                            statement: stable_statement(list.statement),
                            producer: owner.expressions[list.producer.0 as usize].clone(),
                            path: stable_path(&list.path),
                            item_type: list.item_type.clone(),
                            capacity: list.capacity,
                            key_policy: list.key_policy,
                        })
                        .collect::<Vec<_>>()
                        .into_boxed_slice();
                    let diagnostics = artifact
                        .diagnostics
                        .iter()
                        .map(|diagnostic| {
                            let dense_owner = KernelOwnerId(
                                u32::try_from(dense_index)
                                .expect("kernel diagnostic owner count exceeds u32"),
                            );
                            assert_eq!(
                                diagnostic.owner, dense_owner,
                                "kernel diagnostic must remain in its definition"
                            );
                            let site = match &diagnostic.site {
                                KernelDiagnosticSite::Expression { expression } => {
                                    KernelOwnerOracleDiagnosticSite::Expression(
                                        owner.expressions[expression.0 as usize].clone(),
                                    )
                                }
                                KernelDiagnosticSite::CallArgument { call, source } => {
                                    KernelOwnerOracleDiagnosticSite::CallArgument {
                                        call: owner.expressions[call.0 as usize].clone(),
                                        source: *source,
                                    }
                                }
                                KernelDiagnosticSite::CallPass { call, pipe } => {
                                    KernelOwnerOracleDiagnosticSite::CallPass {
                                        call: owner.expressions[call.0 as usize].clone(),
                                        pipe: *pipe,
                                    }
                                }
                                KernelDiagnosticSite::CallInput {
                                    call,
                                    target,
                                    formal_ordinal,
                                } => {
                                    let target = active
                                        .get(target.0 as usize)
                                        .and_then(|target| prepared.get(*target))
                                        .unwrap_or_else(|| {
                                            panic!(
                                                "kernel diagnostic targets missing dense owner {}",
                                                target.0
                                            )
                                        });
                                    KernelOwnerOracleDiagnosticSite::CallInput {
                                        call: owner.expressions[call.0 as usize].clone(),
                                        target: target.owner.clone(),
                                        formal_ordinal: *formal_ordinal,
                                    }
                                }
                            };
                            KernelOwnerOracleDiagnostic {
                                severity: diagnostic.severity,
                                site,
                                kind: diagnostic.kind.clone(),
                            }
                        })
                        .collect::<Vec<_>>()
                        .into_boxed_slice();
                    let mut call_syntax = artifact
                        .call_syntax
                        .into_iter()
                        .map(|syntax| (syntax.expression, syntax))
                        .collect::<BTreeMap<_, _>>();
                    let calls = artifact
                        .calls
                        .into_iter()
                        .map(|call| {
                            let syntax = call_syntax.remove(&call.expression).unwrap_or_else(|| {
                                panic!(
                                    "kernel call expression {} has no authored call surface",
                                    call.expression.0
                                )
                            });
                            let target = match call.target {
                                KernelCallTarget::User {
                                    target,
                                    inherited_formal,
                                } => {
                                    let target = active
                                        .get(target.0 as usize)
                                        .and_then(|target| prepared.get(*target))
                                        .unwrap_or_else(|| {
                                            panic!(
                                                "kernel call targets missing dense owner {}",
                                                target.0
                                            )
                                        });
                                    KernelOwnerOracleCallTarget::User {
                                        target: target.owner.clone(),
                                        inherited_formal,
                                    }
                                }
                                KernelCallTarget::RenderConstructor { kind } => {
                                    KernelOwnerOracleCallTarget::RenderConstructor(kind)
                                }
                                KernelCallTarget::PureBuiltin { kind } => {
                                    KernelOwnerOracleCallTarget::PureBuiltin(kind)
                                }
                                KernelCallTarget::HostEffect { operation } => {
                                    KernelOwnerOracleCallTarget::HostEffect(operation)
                                }
                            };
                            KernelOwnerOracleCall {
                                expression: owner.expressions
                                    [call.expression.0 as usize]
                                    .clone(),
                                function: syntax.function,
                                pipe_input: syntax.pipe_input.map(&stable_provider),
                                arguments: syntax
                                    .arguments
                                    .iter()
                                    .map(|argument| KernelOwnerOracleCallSyntaxArgument {
                                        kind: argument.kind,
                                        name: argument.name.clone(),
                                        provider: stable_provider(argument.value),
                                    })
                                    .collect::<Vec<_>>()
                                    .into_boxed_slice(),
                                pass: syntax.pass.map(|pass| KernelOwnerOracleCallPass {
                                    provider: stable_provider(pass.value),
                                    final_clause: pass.final_clause,
                                }),
                                target,
                                inputs: call
                                    .inputs
                                    .iter()
                                    .map(|input| {
                                        KernelOwnerOracleCallInput {
                                            role: input.role.clone(),
                                            provider: stable_provider(input.value),
                                        }
                                    })
                                    .collect::<Vec<_>>()
                                    .into_boxed_slice(),
                                type_substitutions: call.type_substitutions,
                                result: call.result,
                            }
                        })
                        .collect::<Vec<_>>()
                        .into_boxed_slice();
                    assert!(
                        call_syntax.is_empty(),
                        "every authored call surface must link to one solved call"
                    );
                    let effects = artifact
                        .effects
                        .into_iter()
                        .map(|effect| {
                            (
                                owner.expressions[effect.expression.0 as usize].clone(),
                                effect,
                            )
                        })
                        .collect::<Vec<_>>()
                        .into_boxed_slice();
                    let expressions = owner
                        .expressions
                        .iter()
                        .zip(artifact.expressions)
                        .map(|(source, expression)| (source.clone(), expression.flow_type))
                        .collect::<Vec<_>>()
                        .into_boxed_slice();
                    KernelOwnerOracleEntry {
                        owner: owner.owner.clone(),
                        result_expression: owner.result_expression.clone(),
                        formals: artifact.formals,
                        result: artifact.result,
                        expressions,
                        presentation_scope_count,
                        execution_shape_count,
                        statements,
                        declarations,
                        lexical_bindings,
                        collections,
                        sources,
                        source_resources,
                        states,
                        lists,
                        calls,
                        effects,
                        diagnostics,
                        public_child_owner_fields: owner.public_child_owner_fields.clone(),
                        public_child_kernel_fields: owner
                            .public_child_owner_fields
                            .iter()
                            .map(|(name, child)| {
                                (
                                    name.clone(),
                                    result_by_owner
                                        .get(child)
                                        .unwrap_or_else(|| {
                                            panic!(
                                                "active public child {child:#?} has no kernel result"
                                            )
                                        })
                                        .clone(),
                                )
                            })
                            .collect::<Vec<_>>()
                            .into_boxed_slice(),
                        exported_as_public_child: exported_public_children.contains(&owner.owner),
                        generic_formal_reads: owner.generic_formal_reads.clone(),
                        structured_delimiter_dependents: owner
                            .structured_delimiter_dependents
                            .clone(),
                        record_spread_dependents: owner.record_spread_dependents.clone(),
                        generic_selector_dependents: owner.generic_selector_dependents.clone(),
                        detached_generic_reads: owner.detached_generic_reads.clone(),
                        legacy_no_element_dependents: owner.legacy_no_element_dependents.clone(),
                        legacy_source_container_modes: owner
                            .legacy_source_container_modes
                            .clone(),
                    }
                })
                .collect::<Vec<_>>();
            (
                supported,
                work,
                dependency_edges,
                reverse_consumer_edges,
                currentness,
            )
        },
    );
    let mut blocker_counts = BTreeMap::<StableCheckOwnerKey, usize>::new();
    for owner in unsupported.keys() {
        let root = root_blocker_by_owner
            .get(owner)
            .cloned()
            .unwrap_or_else(|| owner.clone());
        *blocker_counts.entry(root).or_default() += 1;
    }
    let mut root_blockers = blocker_counts
        .into_iter()
        .map(|(owner, affected_owners)| KernelOwnerBlockerImpact {
            reason: unsupported
                .get(&owner)
                .cloned()
                .unwrap_or_else(|| "unsupported dependency root".to_owned()),
            owner,
            affected_owners,
        })
        .collect::<Vec<_>>();
    root_blockers.sort_by(|left, right| {
        right
            .affected_owners
            .cmp(&left.affected_owners)
            .then_with(|| left.owner.cmp(&right.owner))
    });
    let unsupported = owner_order
        .into_iter()
        .filter_map(|owner| unsupported.remove(&owner).map(|reason| (owner, reason)))
        .collect::<Vec<_>>();
    let report = KernelOwnerOracleReport {
        supported: supported.into_boxed_slice(),
        checked_scopes,
        container_owners: container_owners.into_boxed_slice(),
        unsupported: unsupported.into_boxed_slice(),
        root_blockers: root_blockers.into_boxed_slice(),
        dependency_edges,
        reverse_consumer_edges,
        currentness: currentness.into_boxed_slice(),
        work,
    };
    let artifact_projection_us = elapsed_us(artifact_projection_started.elapsed());
    let timings = KernelOwnerOracleTimings {
        total_us: elapsed_us(total_started.elapsed()).saturating_sub(interface_projection_us),
        owner_projection_us,
        direct_projection_us: elapsed_us(direct_projection_elapsed),
        dependency_pruning_us,
        program_compile_us,
        graph_solve_us,
        interface_projection_us,
        checked_image_us,
        checked_link_layout_us,
        checked_link_references,
        solve_us,
        artifact_projection_us,
        input_owners,
        projected_owners: prepared.len(),
        solved_owners: report.supported.len(),
        container_owners: report.container_owners.len(),
        unsupported_owners: report.unsupported.len(),
        compile_work,
    };
    (report, timings)
}

/// Build the production diagnostics product from the dense kernel.
///
/// This is a flag-day adapter, not a selectable fallback: unsupported
/// authored owners are an error. The old checker remains only in differential
/// tests while this parser-owned projection is reduced to its permanent API.
pub(crate) fn compiler_diagnostics_from_kernel(
    project: ProjectSyntaxSnapshot,
    parse_work: boon_parser::ParseWorkCounters,
    parse_ms: f64,
) -> Result<crate::CompilerDiagnostics, String> {
    let typecheck_started = Instant::now();
    let (source_payloads, source_abi_diagnostics) =
        boon_typecheck::project_source_payload_abi_types_and_diagnostics(&project);
    let PreparedKernelProjectProjection {
        owner_order,
        prepared,
        active,
        container_owners,
        unsupported,
        root_blocker_by_owner,
        project_input,
        definition_facts,
        definition_keys,
        ..
    } = prepare_kernel_project_projection(&project, &source_payloads);
    if !unsupported.is_empty() {
        if std::env::var_os("BOON_KERNEL_DIAGNOSTICS_UNSUPPORTED_TRACE").is_some() {
            eprintln!(
                "kernel-diagnostics root-blockers={:#?}",
                root_blocker_by_owner.iter().take(32).collect::<Vec<_>>()
            );
        }
        return Err(format!(
            "dense kernel does not cover the complete project: {:#?}",
            unsupported
        ));
    }
    let owner_count = active.len().saturating_add(container_owners.len());
    let expected_owner_count = owner_order.len();
    if owner_count != expected_owner_count {
        return Err(format!(
            "dense kernel diagnostics cover {owner_count} of {expected_owner_count} project owners"
        ));
    }

    let input = KernelProjectInput::new(project_input, definition_facts, definition_keys)
        .map_err(|error| format!("cannot build dense kernel diagnostics input: {error}"))?;
    let mut session = KernelSession::new(input);
    let checked = session
        .check(CheckDemand::Diagnostics)
        .map_err(|error| format!("cannot solve dense kernel diagnostics: {error}"))?;
    let compile_work = checked.compile_work;
    let KernelCheckProduct::Diagnostics(interfaces) = checked.product else {
        unreachable!("diagnostics demand returns an interface snapshot")
    };
    if interfaces.public_results.len() != active.len() {
        return Err(format!(
            "dense kernel diagnostics publish {} of {} definition interfaces",
            interfaces.public_results.len(),
            active.len()
        ));
    }

    let output_types = active
        .iter()
        .zip(interfaces.public_results.iter())
        .filter_map(|(prepared_index, result)| {
            let owner = &prepared[*prepared_index].owner;
            let StableCheckOwnerKey::Item(key) = owner else {
                return None;
            };
            let segments = key.item_route.segments();
            let [.., container, output] = segments else {
                return None;
            };
            if container.names.first().map(String::as_str) == Some("outputs")
                && output.kind == UnitItemKind::Field
            {
                Some((output.names.first()?.clone(), result.ty.clone()))
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    let mut diagnostics = source_abi_diagnostics.into_vec();
    diagnostics.extend(boon_typecheck::project_host_output_abi_diagnostics(
        &project,
        &output_types,
    ));
    for diagnostic in interfaces.diagnostics.iter() {
        diagnostics.push(present_kernel_interface_diagnostic(
            &project, &prepared, &active, diagnostic,
        )?);
    }
    diagnostics.extend(project_kernel_interface_render_slot_diagnostics(
        &project,
        &prepared,
        &active,
        &interfaces.diagnostic_values,
    )?);
    diagnostics.sort_by(|left, right| {
        left.line
            .cmp(&right.line)
            .then_with(|| left.start.cmp(&right.start))
            .then_with(|| left.end.cmp(&right.end))
            .then_with(|| left.message.cmp(&right.message))
    });
    diagnostics.dedup();

    let checked_expression_count = active
        .iter()
        // Compact owners may append a synthetic structural result when an
        // authored container publishes only child-owner values. That node is
        // real solver work, but it is not a checked source expression and
        // must not inflate the document-coverage receipt.
        .map(|owner| prepared[*owner].expressions.len())
        .sum::<usize>();
    if checked_expression_count != project.check_expression_count() {
        return Err(format!(
            "dense kernel diagnostics cover {checked_expression_count} of {} reachable expressions",
            project.check_expression_count()
        ));
    }
    let call_count = active
        .iter()
        .map(|owner| {
            prepared[*owner]
                .compact
                .nodes
                .iter()
                .filter(|node| {
                    matches!(
                        node.kind,
                        KernelOwnerNodeKind::UserCall { .. }
                            | KernelOwnerNodeKind::RenderConstructor { .. }
                            | KernelOwnerNodeKind::PureBuiltin { .. }
                            | KernelOwnerNodeKind::HostEffect { .. }
                    )
                })
                .count()
        })
        .sum::<usize>();
    let fingerprint_v1 = boon_contract::canonical_serde_hash_v1_streaming(
        b"boon.compiler.kernel-diagnostics.v1\0",
        &(project.source_bundle_digest_v1().to_hex(), &diagnostics),
    )
    .map_err(|error| format!("cannot fingerprint kernel diagnostics: {error}"))?;
    let typecheck_ms = typecheck_started.elapsed().as_secs_f64() * 1_000.0;
    let owner_work = boon_typecheck::OwnerBodyInferenceWork {
        statements: active
            .iter()
            .map(|owner| {
                u64::try_from(prepared[*owner].definition_facts.statements.len())
                    .unwrap_or(u64::MAX)
            })
            .sum(),
        expressions: u64::try_from(checked_expression_count).unwrap_or(u64::MAX),
        local_constraints: compile_work.linked_operations,
        interface_imports: 0,
        interface_plan_direct_owners: u64::try_from(active.len()).unwrap_or(u64::MAX),
        interface_plan_required_owners: u64::try_from(active.len()).unwrap_or(u64::MAX),
        interface_plan_provider_sccs: 0,
        interface_plan_result_transfers: 0,
        interface_plan_transfer_nodes: compile_work.summary_definition_nodes,
        interface_plan_transfer_edges: compile_work.summary_invoke_nodes,
        calls: u64::try_from(call_count).unwrap_or(u64::MAX),
        unification_steps: interfaces.work.activations,
    };
    Ok(crate::CompilerDiagnostics {
        profile: crate::CompilerDiagnosticsProfile {
            source_unit_count: project.units().len(),
            owner_count,
            expression_count: project.expression_count(),
            checked_expression_count,
            call_count,
            diagnostic_count: diagnostics.len(),
            parse_work,
            owner_work,
            kernel_compile_work: compile_work,
            kernel_solve_work: interfaces.work,
            parse_ms,
            typecheck_ms,
            total_ms: parse_ms + typecheck_ms,
        },
        syntax: project,
        diagnostics: diagnostics.into_boxed_slice(),
        full_document_typecheck_coverage: true,
        fingerprint_v1,
    })
}

fn present_kernel_interface_diagnostic(
    project: &ProjectSyntaxSnapshot,
    prepared: &[PreparedOwner],
    active: &[usize],
    diagnostic: &boon_compiler_kernel::KernelDiagnosticArtifact,
) -> Result<TypeDiagnostic, String> {
    let prepared_index = *active
        .get(diagnostic.owner.0 as usize)
        .ok_or_else(|| format!("kernel diagnostic has missing owner {}", diagnostic.owner.0))?;
    let owner = prepared
        .get(prepared_index)
        .ok_or_else(|| format!("kernel diagnostic has missing prepared owner {prepared_index}"))?;
    let expression = |expression: KernelExpressionId| {
        owner
            .expressions
            .get(expression.0 as usize)
            .cloned()
            .ok_or_else(|| {
                format!(
                    "kernel diagnostic owner {:?} has missing expression {}",
                    owner.owner, expression.0
                )
            })
    };
    let site = match &diagnostic.site {
        KernelDiagnosticSite::Expression { expression: value } => {
            KernelOwnerOracleDiagnosticSite::Expression(expression(*value)?)
        }
        KernelDiagnosticSite::CallArgument { call, source } => {
            KernelOwnerOracleDiagnosticSite::CallArgument {
                call: expression(*call)?,
                source: *source,
            }
        }
        KernelDiagnosticSite::CallPass { call, pipe } => {
            KernelOwnerOracleDiagnosticSite::CallPass {
                call: expression(*call)?,
                pipe: *pipe,
            }
        }
        KernelDiagnosticSite::CallInput {
            call,
            target,
            formal_ordinal,
        } => {
            let target = active
                .get(target.0 as usize)
                .and_then(|target| prepared.get(*target))
                .ok_or_else(|| {
                    format!("kernel diagnostic targets missing dense owner {}", target.0)
                })?;
            KernelOwnerOracleDiagnosticSite::CallInput {
                call: expression(*call)?,
                target: target.owner.clone(),
                formal_ordinal: *formal_ordinal,
            }
        }
    };
    let diagnostic = KernelOwnerOracleDiagnostic {
        severity: diagnostic.severity,
        site,
        kind: diagnostic.kind.clone(),
    };
    let KernelOwnerOracleDiagnosticSite::CallInput {
        call,
        target,
        formal_ordinal,
    } = &diagnostic.site
    else {
        return present_kernel_source_diagnostic(project, &owner.owner, &diagnostic);
    };
    let KernelDiagnosticKind::CallInputType {
        actual,
        expected,
        mismatch,
    } = &diagnostic.kind
    else {
        return Err("kernel call-input diagnostic has a non-call-input payload".to_owned());
    };
    let target = active
        .iter()
        .copied()
        .filter_map(|target| prepared.get(target))
        .find(|candidate| &candidate.owner == target)
        .ok_or_else(|| format!("kernel diagnostic targets missing owner {target:?}"))?;
    let parameter_name = target
        .definition_facts
        .declarations
        .iter()
        .find_map(|declaration| {
            matches!(
                declaration.origin,
                KernelDeclarationOrigin::Parameter { ordinal, .. } if ordinal == *formal_ordinal
            )
            .then_some(declaration.name.as_ref())
        });
    present_kernel_call_input_diagnostic(
        project,
        &owner.owner,
        call,
        &target.owner,
        diagnostic.severity,
        actual,
        expected,
        mismatch,
        parameter_name,
    )
}

fn project_kernel_interface_render_slot_diagnostics(
    project: &ProjectSyntaxSnapshot,
    prepared: &[PreparedOwner],
    active: &[usize],
    values: &[boon_compiler_kernel::KernelDiagnosticValueArtifact],
) -> Result<Vec<TypeDiagnostic>, String> {
    let expected_values = active
        .iter()
        .map(|owner| prepared[*owner].render_slots.len())
        .sum::<usize>();
    if values.len() != expected_values {
        return Err(format!(
            "kernel diagnostics published {} of {expected_values} render-contract values",
            values.len()
        ));
    }
    let mut diagnostics = Vec::new();
    for value in values {
        let prepared_index = *active
            .get(value.owner.0 as usize)
            .ok_or_else(|| format!("kernel render value has missing owner {}", value.owner.0))?;
        let owner = prepared
            .get(prepared_index)
            .ok_or_else(|| format!("kernel render value has missing owner {prepared_index}"))?;
        let slot_name = owner
            .render_slots
            .get(value.ordinal as usize)
            .ok_or_else(|| {
                format!(
                    "kernel render owner {:?} has missing diagnostic value {}",
                    owner.owner, value.ordinal
                )
            })?;
        let Some(message) =
            boon_typecheck::project_render_slot_type_diagnostic(slot_name, &value.ty)
        else {
            continue;
        };
        let (source_owner, source_expression) = match value.value {
            KernelValueReference::Local(expression) => (
                &owner.owner,
                owner
                    .expressions
                    .get(expression.0 as usize)
                    .ok_or_else(|| {
                        format!(
                            "kernel render owner {:?} has missing expression {}",
                            owner.owner, expression.0
                        )
                    })?,
            ),
            KernelValueReference::External(external) => {
                let target_index = *active.get(external.owner.0 as usize).ok_or_else(|| {
                    format!(
                        "kernel render value targets missing owner {}",
                        external.owner.0
                    )
                })?;
                let target = prepared.get(target_index).ok_or_else(|| {
                    format!("kernel render value targets missing owner {target_index}")
                })?;
                let expression = match external.target {
                    KernelExternalTarget::Expression(expression) => target
                        .expressions
                        .get(expression.0 as usize)
                        .ok_or_else(|| {
                            format!(
                                "kernel render target {:?} has missing expression {}",
                                target.owner, expression.0
                            )
                        })?,
                    KernelExternalTarget::Result => {
                        target.result_expression.as_ref().ok_or_else(|| {
                            format!(
                                "kernel render target {:?} has no result expression",
                                target.owner
                            )
                        })?
                    }
                };
                (&target.owner, expression)
            }
        };
        let source_view = project.owner_view(source_owner).ok_or_else(|| {
            format!("kernel render value owner has no syntax view: {source_owner:?}")
        })?;
        let expression = source_view
            .expressions()
            .zip(source_view.stable_expression_keys())
            .find_map(|(syntax, stable)| (stable == *source_expression).then_some(syntax))
            .ok_or_else(|| {
                format!("kernel render value expression is absent from owner {source_owner:?}")
            })?;
        let layout = project
            .source_layouts()
            .iter()
            .find(|layout| layout.source_unit_id == source_expression.source_unit_id)
            .ok_or_else(|| "kernel render value has no source layout".to_owned())?;
        diagnostics.push(TypeDiagnostic {
            severity: DiagnosticSeverity::Error,
            line: layout
                .start_line
                .checked_add(expression.line.saturating_sub(1))
                .ok_or_else(|| "kernel render diagnostic line overflowed".to_owned())?,
            start: layout
                .start_byte
                .checked_add(expression.start)
                .ok_or_else(|| "kernel render diagnostic start overflowed".to_owned())?,
            end: layout
                .start_byte
                .checked_add(expression.end)
                .ok_or_else(|| "kernel render diagnostic end overflowed".to_owned())?,
            message,
        });
    }
    Ok(diagnostics)
}

/// Classify the parser-owned render slot without constructing the legacy
/// owner-syntax DTO. UI tags are deliberately irrelevant here: render context
/// comes from authored containment and registered constructors, never from a
/// magic value such as a particular library's empty-element tag.
fn kernel_render_slot_name<'a>(
    owner: &StableCheckOwnerKey,
    statement: &StableStatementKey,
    syntax: &'a boon_syntax::AstStatement,
) -> Option<&'a str> {
    let slot = match &syntax.kind {
        AstStatementKind::Field { name }
        | AstStatementKind::List {
            field: Some(name), ..
        } if matches!(name.as_str(), "root" | "child" | "items" | "children") => name.as_str(),
        _ => return None,
    };
    let inherited = match owner {
        StableCheckOwnerKey::UnitRoot(_) => false,
        StableCheckOwnerKey::Item(owner) => owner.item_route.segments().iter().any(|segment| {
            segment.kind == UnitItemKind::Function
                || segment.names.iter().any(|name| {
                    matches!(
                        name.as_str(),
                        "document" | "scene" | "root" | "child" | "items" | "children"
                    )
                })
        }),
    };
    let nested = statement
        .route
        .statement_route
        .iter()
        .rev()
        .skip(1)
        .flat_map(|segment| &segment.names)
        .any(|name| matches!(name.as_str(), "document" | "scene"));
    (inherited || nested).then_some(slot)
}

#[allow(clippy::too_many_arguments)]
fn present_kernel_call_input_diagnostic(
    project: &ProjectSyntaxSnapshot,
    owner: &StableCheckOwnerKey,
    call: &StableExpressionKey,
    target: &StableCheckOwnerKey,
    severity: KernelDiagnosticSeverity,
    actual: &Type,
    expected: &Type,
    mismatch: &KernelTypeMismatch,
    parameter_name: Option<&str>,
) -> Result<TypeDiagnostic, String> {
    let function = match target {
        StableCheckOwnerKey::Item(key) => key
            .item_route
            .segments()
            .last()
            .and_then(|segment| segment.names.first())
            .map(String::as_str)
            .unwrap_or("<callable>"),
        StableCheckOwnerKey::UnitRoot(_) => "<callable>",
    };
    let detail = match mismatch {
        KernelTypeMismatch::MissingField(field) => {
            format!("is missing field \u{60}{field}\u{60}")
        }
        KernelTypeMismatch::IncompatibleField(field) => {
            format!("field \u{60}{field}\u{60} has an incompatible type")
        }
        KernelTypeMismatch::Type => "has an incompatible type".to_owned(),
    };
    let expected = boon_typecheck::boon_facing_type_label(expected);
    let actual = boon_typecheck::boon_facing_type_label(actual);
    let message = parameter_name.map_or_else(
        || {
            format!(
                "\u{60}FUNCTION {function}\u{60} PASS context {detail}\nexpected: {expected}\nfound: {actual}"
            )
        },
        |name| {
            format!(
                "\u{60}FUNCTION {function}\u{60} argument \u{60}{name}\u{60} {detail}\nexpected: {expected}\nfound: {actual}"
            )
        },
    );
    let view = project
        .owner_view(owner)
        .ok_or_else(|| format!("kernel diagnostic owner has no syntax view: {owner:?}"))?;
    let expression = view
        .expressions()
        .zip(view.stable_expression_keys())
        .find_map(|(expression, stable)| (stable == *call).then_some(expression))
        .ok_or_else(|| format!("kernel call diagnostic site is absent from {:?}", owner))?;
    let layout = project
        .source_layouts()
        .iter()
        .find(|layout| layout.source_unit_id == call.source_unit_id)
        .ok_or_else(|| "kernel call diagnostic source unit has no project layout".to_owned())?;
    Ok(TypeDiagnostic {
        severity: match severity {
            KernelDiagnosticSeverity::Error => DiagnosticSeverity::Error,
            KernelDiagnosticSeverity::Warning => DiagnosticSeverity::Warning,
        },
        line: layout
            .start_line
            .checked_add(expression.line.saturating_sub(1))
            .ok_or_else(|| "kernel diagnostic global line overflowed".to_owned())?,
        start: layout
            .start_byte
            .checked_add(expression.start)
            .ok_or_else(|| "kernel diagnostic global start overflowed".to_owned())?,
        end: layout
            .start_byte
            .checked_add(expression.end)
            .ok_or_else(|| "kernel diagnostic global end overflowed".to_owned())?,
        message,
    })
}

fn elapsed_us(elapsed: Duration) -> u64 {
    u64::try_from(elapsed.as_micros()).unwrap_or(u64::MAX)
}

fn stable_kernel_declaration_origin(
    owner: &PreparedOwner,
    origin: &KernelDeclarationOrigin,
) -> KernelOwnerOracleDeclarationOrigin {
    match origin {
        KernelDeclarationOrigin::Statement { statement } => {
            KernelOwnerOracleDeclarationOrigin::Statement(
                owner.statements[statement.0 as usize].clone(),
            )
        }
        KernelDeclarationOrigin::Parameter { statement, ordinal } => {
            KernelOwnerOracleDeclarationOrigin::Parameter {
                statement: owner.statements[statement.0 as usize].clone(),
                ordinal: *ordinal,
            }
        }
        KernelDeclarationOrigin::RecordField { object, ordinal } => {
            KernelOwnerOracleDeclarationOrigin::RecordField {
                object: owner.expressions[object.0 as usize].clone(),
                ordinal: *ordinal,
            }
        }
        KernelDeclarationOrigin::PatternBinding { arm, ordinal } => {
            KernelOwnerOracleDeclarationOrigin::PatternBinding {
                arm: owner.expressions[arm.0 as usize].clone(),
                ordinal: *ordinal,
            }
        }
        KernelDeclarationOrigin::CallbackBinding { call, ordinal } => {
            KernelOwnerOracleDeclarationOrigin::CallbackBinding {
                call: owner.expressions[call.0 as usize].clone(),
                ordinal: *ordinal,
            }
        }
    }
}

struct PreparedOwner {
    owner: StableCheckOwnerKey,
    expressions: Box<[StableExpressionKey]>,
    statements: Box<[StableStatementKey]>,
    definition_facts: KernelDefinitionFactsInput,
    render_slots: Box<[Box<str>]>,
    statement_child_targets: Box<[PreparedStatementChildTarget]>,
    containing_scope_targets: Box<[PreparedContainingScopeTarget]>,
    lexical_owner_targets: Box<[PreparedLexicalOwnerTarget]>,
    resource_owner_targets: Box<[PreparedResourceOwnerTarget]>,
    resource_synthetic_paths: Box<[PreparedResourceSyntheticPath]>,
    external_expressions: Box<[PreparedExternalExpression]>,
    call_targets: Box<[PreparedCallTarget]>,
    compact: KernelOwnerProgramInput,
    result_expression: Option<StableExpressionKey>,
    public_child_owner_fields: Box<[(String, StableCheckOwnerKey)]>,
    generic_formal_reads: Box<[StableExpressionKey]>,
    structured_delimiter_dependents: Box<[StableExpressionKey]>,
    record_spread_dependents: Box<[StableExpressionKey]>,
    generic_selector_dependents: Box<[StableExpressionKey]>,
    detached_generic_reads: Box<[StableExpressionKey]>,
    legacy_no_element_dependents: Box<[StableExpressionKey]>,
    legacy_source_container_modes: Box<[StableExpressionKey]>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PreparedLexicalBinding {
    provider: PreparedLexicalProvider,
    target: PreparedLexicalTarget,
    prefix: Box<[String]>,
    directional: bool,
    /// The authored match pattern that owns this detached payload read.
    /// Keeping it explicit lets the dense kernel preserve `Tag[field]`
    /// authority instead of degrading the read to an untagged object
    /// projection.
    pattern: Option<KernelPattern>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PreparedOutputBinding {
    formal_ordinal: u32,
    name: String,
    provider: usize,
    active_inputs: Box<[usize]>,
}

type PreparedOutputBindingsByScope = BTreeMap<usize, Box<[PreparedOutputBinding]>>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PreparedResourceOwnerField {
    LinkagePublicDeclaration,
    SourceDeclaration(usize),
    SourceStatement(usize),
    StateBindingDeclaration(usize),
    StateDeclaration(usize),
    StateStatement(usize),
    ListDeclaration(usize),
    /// A child LIST can be the storage authority of its enclosing public
    /// declaration even when the parent result passes through list-preserving
    /// ABI operations. The project linker resolves this candidate after owner
    /// IDs and external result edges are dense.
    ListStatement(usize),
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PreparedResourceOwnerTarget {
    field: PreparedResourceOwnerField,
    owner: StableCheckOwnerKey,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum PreparedResourceSyntheticKind {
    State,
    List,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PreparedResourceSyntheticPath {
    kind: PreparedResourceSyntheticKind,
    row: usize,
    anchor: StableCheckOwnerKey,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum PreparedLexicalTarget {
    Declaration(KernelDeclarationOrigin),
    OwnerPublic(StableCheckOwnerKey),
    Value(PreparedInputReference),
    RuntimeContext,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum PreparedLexicalProvider {
    Input(PreparedInputReference),
    Known(Type),
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct PreparedExternalExpression {
    owner: StableCheckOwnerKey,
    target: PreparedExternalTarget,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum PreparedExternalTarget {
    Expression(StableExpressionKey),
    Result,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum PreparedInputReference {
    Syntax(usize),
    OwnerResult(StableCheckOwnerKey),
}

#[derive(Clone, Debug)]
enum PreparedSyntheticResult {
    Alias(PreparedInputReference),
    Record(Vec<PreparedRecordEntry>),
}

#[derive(Clone, Debug)]
enum PreparedRecordEntry {
    Field {
        name: String,
        value: PreparedInputReference,
    },
    Spread {
        value: PreparedInputReference,
    },
}

#[derive(Clone, Debug)]
struct PreparedCallTarget {
    node: usize,
    owner: StableCheckOwnerKey,
}

#[derive(Clone, Debug)]
struct PreparedStatementChildTarget {
    statement: usize,
    child: usize,
    owner: StableCheckOwnerKey,
}

#[derive(Clone, Debug)]
struct PreparedContainingScopeTarget {
    owner: StableCheckOwnerKey,
    scope: KernelScopeReference,
}

fn resolve_prepared_containing_scope(
    owner: usize,
    placements: &[Option<(usize, KernelScopeReference)>],
    dense_owner: &[Option<KernelOwnerId>],
    resolved: &mut [Option<KernelScopeReference>],
    active: &mut BTreeSet<usize>,
) -> KernelScopeReference {
    if let Some(scope) = resolved[owner] {
        return scope;
    }
    assert!(
        active.insert(owner),
        "kernel checked-presentation containing scopes contain a cycle"
    );
    let scope =
        placements[owner].map_or(
            KernelScopeReference::ProjectRoot,
            |(parent, scope)| match scope {
                KernelScopeReference::ProjectRoot => KernelScopeReference::ProjectRoot,
                KernelScopeReference::Containing => resolve_prepared_containing_scope(
                    parent,
                    placements,
                    dense_owner,
                    resolved,
                    active,
                ),
                KernelScopeReference::Local(scope) => KernelScopeReference::Owner {
                    owner: dense_owner[parent]
                        .expect("active containing-scope parent has a dense owner"),
                    scope,
                },
                KernelScopeReference::Owner { .. } => {
                    panic!("prepared containing scope contains a premature dense owner")
                }
            },
        );
    active.remove(&owner);
    resolved[owner] = Some(scope);
    scope
}

#[derive(Clone, Debug)]
struct PreparedLexicalOwnerTarget {
    binding: usize,
    owner: StableCheckOwnerKey,
}

#[derive(Clone, Debug)]
struct AuthoritativeCallSurface {
    kind: KernelCallableKind,
    parameters: Box<[KernelCallShapeParameter]>,
}

#[derive(Clone, Debug)]
struct CallableSurface {
    owner: StableCheckOwnerKey,
    parameters: Box<[CallableParameter]>,
    context_ordinal: Option<usize>,
}

fn project_kernel_authoritative_call_shapes()
-> Result<BTreeMap<String, AuthoritativeCallSurface>, String> {
    boon_typecheck::project_authoritative_callable_shapes_v1()?
        .into_iter()
        .map(|shape| {
            let kind = match shape.kind {
                boon_checked::CheckedCallableKind::User => KernelCallableKind::User,
                boon_checked::CheckedCallableKind::Builtin => KernelCallableKind::Builtin,
                boon_checked::CheckedCallableKind::External => KernelCallableKind::External,
            };
            let parameters = shape
                .parameters
                .into_iter()
                .map(|parameter| KernelCallShapeParameter {
                    ordinal: parameter.ordinal,
                    kind: match parameter.kind {
                        boon_checked::CheckedParameterKind::Value => KernelParameterKind::Value,
                        boon_checked::CheckedParameterKind::Out => KernelParameterKind::Out,
                    },
                    name: parameter.name.into_boxed_str(),
                    optional: parameter.optional,
                    evaluation_scope: match parameter.evaluation_scope {
                        boon_checked::CheckedAuthoritativeEvaluationScopeV1::Parent => {
                            KernelParameterEvaluationScope::Parent
                        }
                        boon_checked::CheckedAuthoritativeEvaluationScopeV1::Output {
                            parameter_ordinal,
                        } => KernelParameterEvaluationScope::Output { parameter_ordinal },
                    },
                })
                .collect::<Vec<_>>()
                .into_boxed_slice();
            Ok((shape.name, AuthoritativeCallSurface { kind, parameters }))
        })
        .collect()
}

fn dynamic_authoritative_call_surface(
    expression: &boon_syntax::AstExpr,
) -> Option<AuthoritativeCallSurface> {
    let (function, eligible) = match &expression.kind {
        AstExprKind::Pipe { op, args, arms, .. } => (op, args.is_empty() && arms.is_empty()),
        AstExprKind::Call { function, args, .. } => (
            function,
            matches!(args.as_slice(), [argument] if argument.name == "input"),
        ),
        _ => return None,
    };
    if !function.starts_with("Field/") || !eligible {
        return None;
    }
    Some(AuthoritativeCallSurface {
        kind: KernelCallableKind::Builtin,
        parameters: vec![KernelCallShapeParameter {
            ordinal: 0,
            kind: KernelParameterKind::Value,
            name: "input".into(),
            optional: false,
            evaluation_scope: KernelParameterEvaluationScope::Parent,
        }]
        .into_boxed_slice(),
    })
}

#[derive(Clone, Debug)]
struct CallableParameter {
    name: String,
    ordinal: usize,
    kind: KernelParameterKind,
    evaluation_scope: KernelParameterEvaluationScope,
}

fn project_callable_surfaces(
    project: &ProjectSyntaxSnapshot,
    authoritative: &BTreeMap<String, AuthoritativeCallSurface>,
) -> Result<BTreeMap<String, Box<[CallableSurface]>>, String> {
    let definitions = project
        .item_index()
        .definitions()
        .filter(|entry| entry.kind == UnitItemKind::Function)
        .collect::<Vec<_>>();
    let mut contexts = definitions
        .iter()
        .filter_map(|entry| {
            let owner = StableCheckOwnerKey::Item(entry.owner_key.clone());
            project
                .owner_view(&owner)
                .is_some_and(owner_uses_passed_context)
                .then_some(owner)
        })
        .collect::<BTreeSet<_>>();
    let mut callable_owner_by_name = BTreeMap::<String, Vec<StableCheckOwnerKey>>::new();
    for entry in &definitions {
        let owner = StableCheckOwnerKey::Item(entry.owner_key.clone());
        for name in &entry.names {
            callable_owner_by_name
                .entry(name.clone())
                .or_default()
                .push(owner.clone());
        }
    }
    for candidates in callable_owner_by_name.values_mut() {
        candidates.sort();
        candidates.dedup();
    }
    let mut callers_by_callee =
        BTreeMap::<StableCheckOwnerKey, BTreeSet<StableCheckOwnerKey>>::new();
    for entry in &definitions {
        let caller = StableCheckOwnerKey::Item(entry.owner_key.clone());
        let Some(view) = project.owner_view(&caller) else {
            continue;
        };
        for callee in view.expressions().filter_map(|expression| {
            let function = match &expression.kind {
                AstExprKind::Call { function, pass, .. }
                | AstExprKind::Pipe {
                    op: function, pass, ..
                } if pass.is_none() => function,
                _ => return None,
            };
            let candidates = callable_owner_by_name.get(function)?;
            let [callee] = candidates.as_slice() else {
                return None;
            };
            Some(callee.clone())
        }) {
            callers_by_callee
                .entry(callee)
                .or_default()
                .insert(caller.clone());
        }
    }
    let mut queue = contexts.iter().cloned().collect::<VecDeque<_>>();
    while let Some(callee) = queue.pop_front() {
        for caller in callers_by_callee.get(&callee).into_iter().flatten() {
            if contexts.insert(caller.clone()) {
                queue.push_back(caller.clone());
            }
        }
    }

    let mut surfaces_by_owner = BTreeMap::<StableCheckOwnerKey, CallableSurface>::new();
    for entry in &definitions {
        let owner = StableCheckOwnerKey::Item(entry.owner_key.clone());
        let context_ordinal = contexts.contains(&owner).then_some(entry.parameters.len());
        let surface = CallableSurface {
            owner: owner.clone(),
            parameters: entry
                .parameters
                .iter()
                .map(|parameter| CallableParameter {
                    name: parameter.name.clone(),
                    ordinal: parameter.ordinal,
                    kind: match parameter.kind {
                        AstParameterKind::Value => KernelParameterKind::Value,
                        AstParameterKind::Out => KernelParameterKind::Out,
                    },
                    evaluation_scope: KernelParameterEvaluationScope::Parent,
                })
                .collect::<Vec<_>>()
                .into_boxed_slice(),
            context_ordinal,
        };
        if surfaces_by_owner.insert(owner, surface).is_some() {
            return Err("project callable index repeats one function owner".to_owned());
        }
    }
    infer_callable_parameter_evaluation_scopes(
        project,
        authoritative,
        &callable_owner_by_name,
        &mut surfaces_by_owner,
    )?;

    let mut surfaces = BTreeMap::<String, Vec<CallableSurface>>::new();
    for entry in definitions {
        let owner = StableCheckOwnerKey::Item(entry.owner_key.clone());
        let surface = surfaces_by_owner
            .get(&owner)
            .ok_or_else(|| format!("callable owner {owner:?} has no compact surface"))?;
        for name in &entry.names {
            surfaces
                .entry(name.clone())
                .or_default()
                .push(surface.clone());
        }
    }
    Ok(surfaces
        .into_iter()
        .map(|(name, mut candidates)| {
            candidates.sort_by(|left, right| left.owner.cmp(&right.owner));
            candidates.dedup_by(|left, right| left.owner == right.owner);
            (name, candidates.into_boxed_slice())
        })
        .collect())
}

#[derive(Clone, Copy, Debug)]
struct OutputBindingActual {
    kind: AstCallArgKind,
    expression: usize,
}

fn infer_callable_parameter_evaluation_scopes(
    project: &ProjectSyntaxSnapshot,
    authoritative: &BTreeMap<String, AuthoritativeCallSurface>,
    callable_owner_by_name: &BTreeMap<String, Vec<StableCheckOwnerKey>>,
    surfaces_by_owner: &mut BTreeMap<StableCheckOwnerKey, CallableSurface>,
) -> Result<(), String> {
    loop {
        let mut updates = BTreeMap::<(StableCheckOwnerKey, usize), u32>::new();
        for (owner, surface) in surfaces_by_owner.iter() {
            let Some(view) = project.owner_view(owner) else {
                continue;
            };
            let raw_expressions = view.expressions().collect::<Vec<_>>();
            let stable_expressions = view.stable_expression_keys().collect::<Vec<_>>();
            if raw_expressions.len() != stable_expressions.len() {
                return Err(format!(
                    "callable scope inference has an incomplete expression table for {owner:?}"
                ));
            }
            let syntax_by_stable = stable_expressions
                .into_iter()
                .zip(raw_expressions.iter().map(|expression| expression.id))
                .collect::<BTreeMap<_, _>>();
            let parent_by_syntax = raw_expressions
                .iter()
                .filter_map(|expression| {
                    let (parent_owner, parent, _) =
                        view.stable_expression_parent_edge_for_syntax(expression.id)?;
                    (parent_owner == *owner)
                        .then(|| syntax_by_stable.get(&parent).copied())
                        .flatten()
                        .map(|parent| (expression.id, parent))
                })
                .collect::<BTreeMap<_, _>>();
            let expression_by_syntax = raw_expressions
                .iter()
                .map(|expression| (expression.id, *expression))
                .collect::<BTreeMap<_, _>>();
            let value_parameters = surface
                .parameters
                .iter()
                .filter(|parameter| parameter.kind == KernelParameterKind::Value)
                .map(|parameter| (parameter.name.as_str(), parameter))
                .collect::<BTreeMap<_, _>>();
            let out_parameters = surface
                .parameters
                .iter()
                .filter(|parameter| parameter.kind == KernelParameterKind::Out)
                .map(|parameter| (parameter.name.as_str(), parameter.ordinal))
                .collect::<BTreeMap<_, _>>();
            for expression in &raw_expressions {
                let Some(root) = syntax_binding_root(expression) else {
                    continue;
                };
                let Some(parameter) = value_parameters.get(root).copied() else {
                    continue;
                };
                let Some(output_ordinal) = inferred_public_output_scope(
                    expression.id,
                    surface,
                    &parent_by_syntax,
                    &expression_by_syntax,
                    &out_parameters,
                    authoritative,
                    callable_owner_by_name,
                    surfaces_by_owner,
                )?
                else {
                    continue;
                };
                match parameter.evaluation_scope {
                    KernelParameterEvaluationScope::Parent => {
                        match updates.entry((owner.clone(), parameter.ordinal)) {
                            std::collections::btree_map::Entry::Vacant(entry) => {
                                entry.insert(output_ordinal);
                            }
                            std::collections::btree_map::Entry::Occupied(entry)
                                if *entry.get() == output_ordinal => {}
                            std::collections::btree_map::Entry::Occupied(entry) => {
                                return Err(format!(
                                    "callable {owner:?} parameter `{}` requires incompatible OUT scopes {} and {}",
                                    parameter.name,
                                    entry.get(),
                                    output_ordinal
                                ));
                            }
                        }
                    }
                    KernelParameterEvaluationScope::Output { parameter_ordinal }
                        if parameter_ordinal == output_ordinal => {}
                    KernelParameterEvaluationScope::Output { parameter_ordinal } => {
                        return Err(format!(
                            "callable {owner:?} parameter `{}` requires incompatible OUT scopes {} and {}",
                            parameter.name, parameter_ordinal, output_ordinal
                        ));
                    }
                }
            }
        }
        if updates.is_empty() {
            return Ok(());
        }
        for ((owner, ordinal), output_ordinal) in updates {
            let surface = surfaces_by_owner
                .get_mut(&owner)
                .ok_or_else(|| format!("callable scope update lost owner {owner:?}"))?;
            let parameter = surface
                .parameters
                .iter_mut()
                .find(|parameter| parameter.ordinal == ordinal)
                .ok_or_else(|| {
                    format!("callable scope update lost parameter {ordinal} in {owner:?}")
                })?;
            parameter.evaluation_scope = KernelParameterEvaluationScope::Output {
                parameter_ordinal: output_ordinal,
            };
        }
    }
}

fn inferred_public_output_scope(
    read: usize,
    caller: &CallableSurface,
    parent_by_syntax: &BTreeMap<usize, usize>,
    expression_by_syntax: &BTreeMap<usize, &boon_syntax::AstExpr>,
    public_outputs: &BTreeMap<&str, usize>,
    authoritative: &BTreeMap<String, AuthoritativeCallSurface>,
    callable_owner_by_name: &BTreeMap<String, Vec<StableCheckOwnerKey>>,
    surfaces_by_owner: &BTreeMap<StableCheckOwnerKey, CallableSurface>,
) -> Result<Option<u32>, String> {
    let mut cursor = read;
    let mut visited = BTreeSet::new();
    while visited.insert(cursor) {
        let Some(parent) = parent_by_syntax.get(&cursor).copied() else {
            return Ok(None);
        };
        let Some(expression) = expression_by_syntax.get(&parent).copied() else {
            return Ok(None);
        };
        let Some((scope, output_actual)) = call_child_evaluation_scope(
            expression,
            cursor,
            caller,
            authoritative,
            callable_owner_by_name,
            surfaces_by_owner,
        )?
        else {
            cursor = parent;
            continue;
        };
        let KernelParameterEvaluationScope::Output { .. } = scope else {
            cursor = parent;
            continue;
        };
        let Some(actual) = output_actual else {
            return Ok(None);
        };
        match actual.kind {
            AstCallArgKind::BareBinding => {
                // A fresh nested output inherits the call's parent region.
                cursor = parent;
            }
            AstCallArgKind::Named => {
                let Some(actual_expression) = expression_by_syntax.get(&actual.expression) else {
                    return Ok(None);
                };
                let Some(name) = syntax_exact_binding_name(actual_expression) else {
                    return Ok(None);
                };
                let Some(ordinal) = public_outputs.get(name).copied() else {
                    return Ok(None);
                };
                return u32::try_from(ordinal)
                    .map(Some)
                    .map_err(|_| format!("callable {:?} OUT ordinal exceeds u32", caller.owner));
            }
        }
    }
    Ok(None)
}

fn call_child_evaluation_scope(
    expression: &boon_syntax::AstExpr,
    child: usize,
    caller: &CallableSurface,
    authoritative: &BTreeMap<String, AuthoritativeCallSurface>,
    callable_owner_by_name: &BTreeMap<String, Vec<StableCheckOwnerKey>>,
    surfaces_by_owner: &BTreeMap<StableCheckOwnerKey, CallableSurface>,
) -> Result<Option<(KernelParameterEvaluationScope, Option<OutputBindingActual>)>, String> {
    let function = match &expression.kind {
        AstExprKind::Call { function, .. } => function,
        AstExprKind::Pipe { op, .. } => op,
        _ => return Ok(None),
    };
    let (kind, parameters, context_ordinal) = if let Some(surface) = authoritative.get(function) {
        (surface.kind, surface.parameters.clone(), None)
    } else {
        let Some(candidates) = callable_owner_by_name.get(function) else {
            return Ok(None);
        };
        let [target] = candidates.as_slice() else {
            return Ok(None);
        };
        let Some(surface) = surfaces_by_owner.get(target) else {
            return Ok(None);
        };
        (
            KernelCallableKind::User,
            compact_call_shape_parameters(surface)?,
            surface
                .context_ordinal
                .map(|ordinal| checked_u32(ordinal, "call context ordinal"))
                .transpose()?,
        )
    };
    let shape = compact_call_shape_input(KernelExpressionId(0), expression)?;
    let projection = project_kernel_call_shape(
        &shape,
        &KernelCallShapeResolution::Callable {
            kind,
            parameters: parameters.clone(),
            context_ordinal,
            caller_context_ordinal: caller
                .context_ordinal
                .map(|ordinal| checked_u32(ordinal, "caller context ordinal"))
                .transpose()?,
        },
    )
    .map_err(|error| error.to_string())?;
    if !projection.valid {
        return Ok(None);
    }
    let source = call_argument_source_for_child(expression, child)?;
    let Some(matched) = projection
        .matched_inputs
        .iter()
        .find(|matched| Some(matched.source) == source)
    else {
        return Ok(None);
    };
    let parameter = parameters
        .iter()
        .find(|parameter| parameter.ordinal == matched.formal_ordinal)
        .ok_or_else(|| {
            format!(
                "call `{function}` matched missing formal {}",
                matched.formal_ordinal
            )
        })?;
    let output_actual = match parameter.evaluation_scope {
        KernelParameterEvaluationScope::Parent => None,
        KernelParameterEvaluationScope::Output { parameter_ordinal } => {
            let matched = projection
                .matched_inputs
                .iter()
                .find(|matched| matched.formal_ordinal == parameter_ordinal)
                .ok_or_else(|| {
                    format!(
                        "call `{function}` output-scoped parameter references omitted OUT formal {parameter_ordinal}"
                    )
                })?;
            let (kind, expression) = call_argument_value(expression, matched.source)?;
            Some(OutputBindingActual { kind, expression })
        }
    };
    Ok(Some((parameter.evaluation_scope, output_actual)))
}

fn call_argument_source_for_child(
    expression: &boon_syntax::AstExpr,
    child: usize,
) -> Result<Option<KernelCallArgumentSource>, String> {
    let (pipe_input, arguments) = match &expression.kind {
        AstExprKind::Call { args, .. } => (None, args.as_slice()),
        AstExprKind::Pipe { input, args, .. } => (
            Some(expression.linked_input.unwrap_or(*input)),
            args.as_slice(),
        ),
        _ => return Ok(None),
    };
    let pipe = pipe_input.is_some();
    let mut matches = Vec::new();
    if pipe_input == Some(child) {
        matches.push(KernelCallArgumentSource::PipeInput);
    }
    matches.extend(
        arguments
            .iter()
            .enumerate()
            .filter_map(|(ordinal, argument)| {
                (argument.value == child).then(|| {
                    checked_u32(ordinal, "call argument ordinal").map(|ordinal| {
                        if pipe {
                            KernelCallArgumentSource::PipeArgument { ordinal }
                        } else {
                            KernelCallArgumentSource::CallArgument { ordinal }
                        }
                    })
                })
            })
            .collect::<Result<Vec<_>, _>>()?,
    );
    match matches.as_slice() {
        [] => Ok(None),
        [source] => Ok(Some(*source)),
        _ => Err("one expression is reused by multiple direct call inputs".to_owned()),
    }
}

fn call_argument_value(
    expression: &boon_syntax::AstExpr,
    source: KernelCallArgumentSource,
) -> Result<(AstCallArgKind, usize), String> {
    let arguments = match &expression.kind {
        AstExprKind::Call { args, .. } | AstExprKind::Pipe { args, .. } => args,
        _ => return Err("call input lookup received a non-call expression".to_owned()),
    };
    match source {
        KernelCallArgumentSource::PipeInput => {
            Err("an OUT formal cannot be supplied by a pipe input".to_owned())
        }
        KernelCallArgumentSource::CallArgument { ordinal }
        | KernelCallArgumentSource::PipeArgument { ordinal } => arguments
            .get(ordinal as usize)
            .map(|argument| (argument.kind, argument.value))
            .ok_or_else(|| format!("call input lookup lost argument ordinal {ordinal}")),
    }
}

fn syntax_binding_root(expression: &boon_syntax::AstExpr) -> Option<&str> {
    match &expression.kind {
        AstExprKind::Identifier(name) => Some(name),
        AstExprKind::Path(path) => path.first().map(String::as_str),
        AstExprKind::Drain { path } => match path {
            boon_syntax::AstDrainPath::Binding { name }
            | boon_syntax::AstDrainPath::Field { binding: name, .. } => Some(name),
            boon_syntax::AstDrainPath::Passed { .. } => None,
        },
        _ => None,
    }
}

fn syntax_exact_binding_name(expression: &boon_syntax::AstExpr) -> Option<&str> {
    match &expression.kind {
        AstExprKind::Identifier(name) => Some(name),
        AstExprKind::Path(path) if path.len() == 1 => path.first().map(String::as_str),
        _ => None,
    }
}

fn owner_uses_passed_context(view: UnitOwnerSyntaxView<'_>) -> bool {
    view.expressions().any(|expression| match &expression.kind {
        AstExprKind::Identifier(name) => name == "PASSED",
        AstExprKind::Path(path) => path.first().is_some_and(|root| root == "PASSED"),
        _ => false,
    })
}

fn compact_call_syntax_input(
    index: usize,
    syntax: &boon_syntax::AstExpr,
    view: UnitOwnerSyntaxView<'_>,
    owner: &StableCheckOwnerKey,
    local_by_syntax: &BTreeMap<usize, usize>,
    node_count: usize,
    external_by_key: &mut BTreeMap<PreparedExternalExpression, usize>,
    external_expressions: &mut Vec<PreparedExternalExpression>,
) -> Result<KernelCallSyntaxInput, String> {
    let mut dense_expression = |input: usize, label: &str| {
        prepared_input_reference_index(
            PreparedInputReference::Syntax(input),
            view,
            owner,
            Some(syntax.id),
            local_by_syntax,
            node_count,
            external_by_key,
            external_expressions,
        )
        .map_err(|error| format!("authored call {label}: {error}"))
        .and_then(checked_kernel_expression)
    };
    let (function, pipe_input, arguments, pass) = match &syntax.kind {
        AstExprKind::Call {
            function,
            args,
            pass,
        } => (function, None, args, pass.as_ref()),
        AstExprKind::Pipe {
            input,
            op,
            args,
            pass,
            ..
        } => (
            op,
            Some(dense_expression(
                syntax.linked_input.unwrap_or(*input),
                "pipe input",
            )?),
            args,
            pass.as_ref(),
        ),
        _ => return Err("call syntax projection received a non-call expression".to_owned()),
    };
    Ok(KernelCallSyntaxInput {
        expression: checked_kernel_expression(index)?,
        function: function.clone().into_boxed_str(),
        pipe_input,
        arguments: arguments
            .iter()
            .enumerate()
            .map(|(ordinal, argument)| {
                Ok(KernelCallSyntaxArgument {
                    ordinal: checked_u32(ordinal, "authored call argument ordinal")?,
                    kind: match argument.kind {
                        AstCallArgKind::Named => KernelCallArgumentKind::Named,
                        AstCallArgKind::BareBinding => KernelCallArgumentKind::BareBinding,
                    },
                    name: argument.name.clone().into_boxed_str(),
                    value: dense_expression(argument.value, "argument")?,
                })
            })
            .collect::<Result<Vec<_>, String>>()?
            .into_boxed_slice(),
        pass: pass
            .map(|pass| {
                Ok::<_, String>(KernelCallPassInput {
                    value: dense_expression(pass.value, "PASS value")?,
                    final_clause: pass.final_clause,
                })
            })
            .transpose()?,
    })
}

#[allow(clippy::too_many_arguments)]
fn compact_execution_shape_inputs(
    view: UnitOwnerSyntaxView<'_>,
    owner: &StableCheckOwnerKey,
    raw_expressions: &[&boon_syntax::AstExpr],
    local_by_syntax: &BTreeMap<usize, usize>,
    nodes: &[KernelOwnerNode],
    statement_record_field_targets: &BTreeMap<(usize, String), PreparedLexicalTarget>,
    declarations: &[KernelDeclarationInput],
    node_count: usize,
    external_by_key: &mut BTreeMap<PreparedExternalExpression, usize>,
    external_expressions: &mut Vec<PreparedExternalExpression>,
) -> Result<Box<[KernelExecutionShapeInput]>, String> {
    let dense_statement_by_syntax = view
        .statement_ids()
        .iter()
        .copied()
        .zip(view.statements())
        .enumerate()
        .map(|(dense, (_, statement))| {
            Ok((
                statement.id,
                KernelStatementId(checked_u32(dense, "execution-shape statement")?),
            ))
        })
        .collect::<Result<BTreeMap<_, _>, String>>()?;
    let declaration_by_origin = declarations
        .iter()
        .map(|declaration| (declaration.origin.clone(), declaration.id))
        .collect::<BTreeMap<_, _>>();
    let structural_declaration = |target: &PreparedLexicalTarget| -> Result<Option<_>, String> {
        Ok(match target {
            PreparedLexicalTarget::Declaration(origin) => Some(
                declaration_by_origin
                    .get(origin)
                    .copied()
                    .map(KernelStructuralDeclarationInput::Local)
                    .ok_or_else(|| {
                        format!("structural execution target has no local declaration {origin:?}")
                    })?,
            ),
            PreparedLexicalTarget::OwnerPublic(_) => {
                Some(KernelStructuralDeclarationInput::ValueOwnerPublic)
            }
            PreparedLexicalTarget::Value(_) | PreparedLexicalTarget::RuntimeContext => None,
        })
    };
    let value_is_owner_result =
        |value: KernelExpressionId, external_expressions: &[PreparedExternalExpression]| {
            let index = value.0 as usize;
            index >= node_count
                && external_expressions
                    .get(index - node_count)
                    .is_some_and(|external| {
                        matches!(external.target, PreparedExternalTarget::Result)
                    })
        };
    let mut shapes = Vec::new();
    for (index, node) in nodes.iter().enumerate() {
        let expression = checked_kernel_expression(index)?;
        match &node.kind {
            KernelOwnerNodeKind::When => {
                let kind = match raw_expressions
                    .get(index)
                    .map(|expression| &expression.kind)
                {
                    Some(AstExprKind::When { .. }) => KernelConditionalKind::When,
                    Some(AstExprKind::Pipe { op, .. }) if op == "WHILE" => {
                        KernelConditionalKind::While
                    }
                    source => {
                        return Err(format!(
                            "canonical WHEN expression {index} has no exact WHEN/WHILE source: {source:?}"
                        ));
                    }
                };
                shapes.push(KernelExecutionShapeInput::Conditional { expression, kind });
            }
            KernelOwnerNodeKind::Record { .. } => {
                let syntax = raw_expressions.get(index).map(|expression| expression.id);
                let fields = node
                    .inputs
                    .iter()
                    .enumerate()
                    .map(|(ordinal, edge)| {
                        let KernelOwnerEdgeRole::RecordField { name, spread } = &edge.role else {
                            return Err(format!(
                                "canonical record expression {index} has a non-field input"
                            ));
                        };
                        let declaration = if *spread {
                            None
                        } else if let Some(target) = syntax.and_then(|syntax| {
                            statement_record_field_targets.get(&(syntax, name.to_string()))
                        }) {
                            structural_declaration(target)?
                        } else {
                            declaration_by_origin
                                .get(&KernelDeclarationOrigin::RecordField {
                                    object: expression,
                                    ordinal: checked_u32(
                                        ordinal,
                                        "execution record field ordinal",
                                    )?,
                                })
                                .copied()
                                .map(KernelStructuralDeclarationInput::Local)
                                .or_else(|| {
                                    value_is_owner_result(edge.expression, external_expressions)
                                        .then_some(
                                            KernelStructuralDeclarationInput::ValueOwnerPublic,
                                        )
                                })
                        };
                        Ok(KernelExecutionRecordFieldInput {
                            ordinal: checked_u32(ordinal, "execution record field ordinal")?,
                            declaration,
                            name: name.clone(),
                            value: edge.expression,
                            spread: *spread,
                        })
                    })
                    .collect::<Result<Vec<_>, String>>()?
                    .into_boxed_slice();
                shapes.push(KernelExecutionShapeInput::Record { expression, fields });
            }
            KernelOwnerNodeKind::Block => {
                let bindings = match raw_expressions.get(index).map(|expression| &expression.kind) {
                    Some(AstExprKind::Block { bindings, .. }) => bindings
                        .iter()
                        .enumerate()
                        .map(|(ordinal, binding)| {
                            let declaration = match binding.declaration {
                                AstBlockBindingDeclaration::Local { statement } => {
                                    let statement = dense_statement_by_syntax
                                        .get(&statement)
                                        .copied()
                                        .ok_or_else(|| {
                                            format!(
                                                "BLOCK expression {index} binding {ordinal} references statement {statement} outside its owner"
                                            )
                                        })?;
                                    let origin = KernelDeclarationOrigin::Statement {
                                        statement,
                                    };
                                    KernelStructuralDeclarationInput::Local(
                                        declaration_by_origin.get(&origin).copied().ok_or_else(
                                            || {
                                                format!(
                                                    "BLOCK expression {index} binding {ordinal} has no declaration {origin:?}"
                                                )
                                            },
                                        )?,
                                    )
                                }
                                AstBlockBindingDeclaration::Child { .. } => {
                                    KernelStructuralDeclarationInput::ValueOwnerPublic
                                }
                            };
                            let value = prepared_input_reference_index(
                                PreparedInputReference::Syntax(binding.value),
                                view,
                                owner,
                                raw_expressions.get(index).map(|expression| expression.id),
                                local_by_syntax,
                                node_count,
                                external_by_key,
                                external_expressions,
                            )
                            .and_then(checked_kernel_expression)?;
                            Ok(KernelExecutionBlockBindingInput {
                                ordinal: checked_u32(ordinal, "BLOCK binding ordinal")?,
                                declaration,
                                value,
                            })
                        })
                        .collect::<Result<Vec<_>, String>>()?
                        .into_boxed_slice(),
                    // A synthetic definition-result alias is represented by a
                    // BLOCK node without authored bindings.
                    None if index >= raw_expressions.len() => Box::new([]),
                    source => {
                        return Err(format!(
                            "canonical BLOCK expression {index} has no exact BLOCK source: {source:?}"
                        ));
                    }
                };
                let result = node
                    .inputs
                    .iter()
                    .find(|edge| edge.role == KernelOwnerEdgeRole::BlockResult)
                    .map(|edge| edge.expression);
                shapes.push(KernelExecutionShapeInput::Block {
                    expression,
                    bindings,
                    result,
                });
            }
            KernelOwnerNodeKind::MatchArm { .. } => {
                let bindings = pattern_variable_names(match raw_expressions
                    .get(index)
                    .map(|expression| &expression.kind)
                {
                    Some(AstExprKind::MatchArm { pattern, .. }) => pattern,
                    source => {
                        return Err(format!(
                            "canonical match arm {index} has no exact match source: {source:?}"
                        ));
                    }
                })
                .into_iter()
                .enumerate()
                .map(|(ordinal, _)| {
                    let origin = KernelDeclarationOrigin::PatternBinding {
                        arm: expression,
                        ordinal: checked_u32(ordinal, "execution pattern binding ordinal")?,
                    };
                    declaration_by_origin.get(&origin).copied().ok_or_else(|| {
                        format!(
                            "match expression {index} binding {ordinal} has no declaration {origin:?}"
                        )
                    })
                })
                .collect::<Result<Vec<_>, String>>()?
                .into_boxed_slice();
                shapes.push(KernelExecutionShapeInput::MatchArm {
                    expression,
                    bindings,
                });
            }
            _ => {}
        }
    }
    Ok(shapes.into_boxed_slice())
}

fn compact_call_shape_input(
    expression: KernelExpressionId,
    syntax: &boon_syntax::AstExpr,
) -> Result<KernelCallShapeInput, String> {
    let (function, pipe, arguments, pass) = match &syntax.kind {
        AstExprKind::Call {
            function,
            args,
            pass,
        } => (function, false, args, pass.as_ref()),
        AstExprKind::Pipe { op, args, pass, .. } => (op, true, args, pass.as_ref()),
        _ => return Err("call-shape projection received a non-call expression".to_owned()),
    };
    Ok(KernelCallShapeInput {
        expression,
        function: function.clone().into_boxed_str(),
        pipe,
        arguments: arguments
            .iter()
            .enumerate()
            .map(|(ordinal, argument)| {
                Ok(KernelCallShapeArgument {
                    source: if pipe {
                        KernelCallArgumentSource::PipeArgument {
                            ordinal: checked_u32(ordinal, "pipe argument ordinal")?,
                        }
                    } else {
                        KernelCallArgumentSource::CallArgument {
                            ordinal: checked_u32(ordinal, "call argument ordinal")?,
                        }
                    },
                    kind: match argument.kind {
                        AstCallArgKind::Named => KernelCallArgumentKind::Named,
                        AstCallArgKind::BareBinding => KernelCallArgumentKind::BareBinding,
                    },
                    name: argument.name.clone().into_boxed_str(),
                })
            })
            .collect::<Result<Vec<_>, String>>()?
            .into_boxed_slice(),
        pass: pass.is_some(),
    })
}

fn compact_call_shape_parameters(
    surface: &CallableSurface,
) -> Result<Box<[KernelCallShapeParameter]>, String> {
    Ok(surface
        .parameters
        .iter()
        .map(|parameter| {
            Ok(KernelCallShapeParameter {
                ordinal: checked_u32(parameter.ordinal, "callable parameter ordinal")?,
                kind: parameter.kind,
                name: parameter.name.clone().into_boxed_str(),
                optional: false,
                evaluation_scope: parameter.evaluation_scope,
            })
        })
        .collect::<Result<Vec<_>, String>>()?
        .into_boxed_slice())
}

fn prepared_call_shape_edges(
    projection: &boon_compiler_kernel::KernelCallShapeProjection,
    syntax: &boon_syntax::AstExpr,
    parameters: &[CallableParameter],
) -> Result<Vec<(KernelOwnerEdgeRole, PreparedInputReference)>, String> {
    let (pipe_input, arguments, pass) = match &syntax.kind {
        AstExprKind::Call { args, pass, .. } => (None, args, pass.as_ref()),
        AstExprKind::Pipe {
            input, args, pass, ..
        } => (
            Some(syntax.linked_input.unwrap_or(*input)),
            args,
            pass.as_ref(),
        ),
        _ => return Err("call-shape edges received a non-call expression".to_owned()),
    };
    let mut edges = projection
        .matched_inputs
        .iter()
        .map(|matched| {
            let value = match matched.source {
                KernelCallArgumentSource::PipeInput => {
                    pipe_input.ok_or_else(|| "direct call matched a pipe input".to_owned())?
                }
                KernelCallArgumentSource::CallArgument { ordinal }
                | KernelCallArgumentSource::PipeArgument { ordinal } => arguments
                    .get(ordinal as usize)
                    .map(|argument| argument.value)
                    .ok_or_else(|| {
                        format!("call-shape matched missing argument ordinal {ordinal}")
                    })?,
            };
            let parameter = parameters
                .iter()
                .find(|parameter| parameter.ordinal == matched.formal_ordinal as usize)
                .ok_or_else(|| {
                    format!(
                        "call-shape matched missing callable parameter {}",
                        matched.formal_ordinal
                    )
                })?;
            Ok((
                match parameter.kind {
                    KernelParameterKind::Value => KernelOwnerEdgeRole::CallArgument {
                        ordinal: matched.formal_ordinal,
                    },
                    KernelParameterKind::Out => KernelOwnerEdgeRole::CallOutArgument {
                        ordinal: matched.formal_ordinal,
                    },
                },
                PreparedInputReference::Syntax(value),
            ))
        })
        .collect::<Result<Vec<_>, String>>()?;
    if let Some(formal_ordinal) = projection.explicit_context_ordinal {
        let pass = pass.ok_or_else(|| "call-shape selected an absent PASS value".to_owned())?;
        edges.push((
            KernelOwnerEdgeRole::CallArgument {
                ordinal: formal_ordinal,
            },
            PreparedInputReference::Syntax(pass.value),
        ));
    }
    Ok(edges)
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ValueSurface {
    owner: StableCheckOwnerKey,
    target: PreparedExternalTarget,
    lexical_scope: Box<[StableItemRouteSegment]>,
}

fn project_value_surfaces(project: &ProjectSyntaxSnapshot) -> BTreeMap<String, Vec<ValueSurface>> {
    let mut surfaces = BTreeMap::<String, Vec<ValueSurface>>::new();
    for entry in project
        .item_index()
        .owners()
        // HOLD aliases are private capabilities for authored update bodies.
        // The enclosing field remains the public value surface; publishing
        // the nested alias here creates a second, equally ranked project
        // value for an unqualified read outside that field.
        .filter(|entry| !matches!(entry.kind, UnitItemKind::Function | UnitItemKind::Hold))
    {
        let owner = StableCheckOwnerKey::Item(entry.owner_key.clone());
        let Some(view) = project.owner_view(&owner) else {
            continue;
        };
        let Some((_, _root_statement)) = view
            .statement_ids()
            .iter()
            .copied()
            .zip(view.statements())
            .find(|(statement, _)| {
                view.stable_statement_key_local(*statement)
                    .is_some_and(|key| {
                        key.route.statement_route.is_empty()
                            && key.route.owner.as_ref() == Some(&entry.owner_key.item_route)
                    })
            })
        else {
            continue;
        };
        // A project value name denotes its declaration's finalized public
        // result. The syntax-root expression remains useful for ownership and
        // local lexical reads, but exporting it here bypasses multiline pipe
        // continuations and exposes stale seed values to sibling owners.
        let target = PreparedExternalTarget::Result;
        let lexical_scope = entry.route.segments()[..entry.route.segments().len() - 1]
            .to_vec()
            .into_boxed_slice();
        let surface = ValueSurface {
            owner,
            target,
            lexical_scope,
        };
        for name in &entry.names {
            let candidates = surfaces.entry(name.clone()).or_default();
            if !candidates.iter().any(|candidate| candidate == &surface) {
                candidates.push(surface.clone());
            }
        }
    }
    surfaces
}

fn exact_value_surface<'a>(
    name: &str,
    surfaces: &'a BTreeMap<String, Vec<ValueSurface>>,
    current_owner: &StableCheckOwnerKey,
) -> Result<&'a ValueSurface, String> {
    let current_route = match current_owner {
        StableCheckOwnerKey::Item(owner) => Some(owner.item_route.segments()),
        StableCheckOwnerKey::UnitRoot(_) => None,
    };
    let mut visible = surfaces
        .get(name)
        .into_iter()
        .flatten()
        .filter_map(|surface| {
            let same_unit = surface.owner.source_unit_id() == current_owner.source_unit_id();
            let visible = if same_unit {
                current_route.is_some_and(|route| route.starts_with(&surface.lexical_scope))
            } else {
                surface.lexical_scope.is_empty()
            };
            visible.then_some(((same_unit, surface.lexical_scope.len()), surface))
        })
        .collect::<Vec<_>>();
    let best = visible.iter().map(|(rank, _)| *rank).max();
    visible.retain(|(rank, _)| Some(*rank) == best);
    match visible.as_slice() {
        [(_, surface)] => Ok(surface),
        [] => match surfaces.get(name).map(Vec::as_slice).unwrap_or_default() {
            // Boon permits an unqualified reference to one uniquely named
            // nested root value (for example `elements` for
            // `store.elements`). This is a static declaration capture, not an
            // implicit PASSED/context formal, so preserve the exact external
            // owner rather than allocating a call-frame slot.
            [surface] => Ok(surface),
            [] => Err(format!("unresolved top-level value read `{name}`")),
            candidates => Err(format!(
                "ambiguous nested value read `{name}` has {} candidates",
                candidates.len()
            )),
        },
        candidates => Err(format!(
            "ambiguous lexical value read `{name}` has {} nearest candidates",
            candidates.len()
        )),
    }
}

/// Resolve the deepest declaration prefix of a qualified value path.
///
/// The type equation may still project from an ancestor provider, but lexical
/// identity belongs to the nested authored declaration. Keeping those two
/// facts separate avoids turning `store.elements.click` into a read of
/// `store` plus an invented semantic projection.
fn exact_value_path_surface<'a>(
    parts: &[String],
    surfaces: &'a BTreeMap<String, Vec<ValueSurface>>,
    current_owner: &StableCheckOwnerKey,
) -> Result<(&'a ValueSurface, usize), String> {
    for consumed in (2..=parts.len()).rev() {
        let name = &parts[consumed - 1];
        let mut matches = surfaces.get(name).into_iter().flatten().filter(|surface| {
            let scope = surface
                .lexical_scope
                .iter()
                .filter_map(|segment| segment.names.first())
                .map(String::as_str)
                .chain(std::iter::once(name.as_str()))
                .collect::<Vec<_>>();
            scope
                == parts[..consumed]
                    .iter()
                    .map(String::as_str)
                    .collect::<Vec<_>>()
        });
        if let Some(surface) = matches.next()
            && matches.next().is_none()
        {
            return Ok((surface, consumed));
        }
    }
    let Some(root) = parts.first() else {
        return Err("value path has no root".to_owned());
    };
    exact_value_surface(root, surfaces, current_owner).map(|surface| (surface, 1))
}

fn local_value_surface_provider(
    surface: &ValueSurface,
    current_owner: &StableCheckOwnerKey,
    stable_expressions: &[StableExpressionKey],
    raw_expressions: &[&boon_syntax::AstExpr],
    raw_result: Option<usize>,
) -> Result<Option<usize>, String> {
    if &surface.owner != current_owner {
        return Ok(None);
    }
    let provider = match &surface.target {
        PreparedExternalTarget::Expression(target) => stable_expressions
            .iter()
            .position(|expression| expression == target)
            .and_then(|index| raw_expressions.get(index))
            .map(|expression| expression.id)
            .ok_or_else(|| {
                format!("owner-local value surface {target:?} has no expression in its owning unit")
            })?,
        PreparedExternalTarget::Result => raw_result.ok_or_else(|| {
            "owner-local result read has no direct syntax result provider".to_owned()
        })?,
    };
    Ok(Some(provider))
}

/// Project the first dense slice straight from the parser-owned owner view.
///
/// This deliberately accepts only owners whose public value is one direct
/// statement expression. More elaborate statement sequencing is rejected at
/// this boundary until it has a compact residual representation; it is never
/// reconstructed through the legacy lexical/constraint graphs.
fn compact_owner_view(
    view: UnitOwnerSyntaxView<'_>,
    source_payloads: &BTreeMap<String, Type>,
    callable_surfaces: &BTreeMap<String, Box<[CallableSurface]>>,
    authoritative_call_shapes: &BTreeMap<String, AuthoritativeCallSurface>,
    value_surfaces: &BTreeMap<String, Vec<ValueSurface>>,
) -> Result<PreparedOwner, String> {
    let owner = view.stable_key();
    if !matches!(owner, StableCheckOwnerKey::Item(_)) {
        return Err("owner has no public declaration".to_owned());
    }
    let StableCheckOwnerKey::Item(owner_key) = &owner else {
        unreachable!()
    };
    let owner_callable_surface = callable_surfaces
        .values()
        .flatten()
        .find(|surface| surface.owner == owner);
    let owner_context_ordinal = owner_callable_surface.and_then(|surface| surface.context_ordinal);
    let (root_statement_id, root_statement) = view
        .statement_ids()
        .iter()
        .copied()
        .zip(view.statements())
        .find(|(statement, _)| {
            view.stable_statement_key_local(*statement)
                .is_some_and(|key| {
                    key.route.statement_route.is_empty()
                        && key.route.owner.as_ref() == Some(&owner_key.item_route)
                })
        })
        .ok_or_else(|| "owner has no public declaration".to_owned())?;
    let root_statement_dense = KernelStatementId(checked_u32(
        view.statement_ids()
            .iter()
            .position(|statement| *statement == root_statement_id)
            .ok_or_else(|| "owner root statement is absent from its dense table".to_owned())?,
        "definition root statement",
    )?);
    let result_mode = match &root_statement.kind {
        AstStatementKind::Source { .. } => FlowMode::PresentOrAbsent,
        AstStatementKind::Function { .. }
        | AstStatementKind::Field { .. }
        | AstStatementKind::Hold { .. }
        | AstStatementKind::List { .. } => FlowMode::Continuous,
        AstStatementKind::Block | AstStatementKind::Spread | AstStatementKind::Expression => {
            return Err("owner has no public declaration".to_owned());
        }
    };
    let (formal_count, formal_by_name) = match &root_statement.kind {
        AstStatementKind::Function { parameters, .. } => {
            let mut by_name = BTreeMap::new();
            for parameter in parameters {
                if parameter.ordinal >= parameters.len()
                    || by_name
                        .insert(parameter.name.clone(), parameter.ordinal)
                        .is_some()
                {
                    return Err("function formals are not a dense unique frame".to_owned());
                }
            }
            let mut formal_count = parameters.len();
            if let Some(context_ordinal) = owner_context_ordinal {
                if context_ordinal != formal_count {
                    return Err(
                        "function context ordinal is not after its value formals".to_owned()
                    );
                }
                if by_name.insert("PASSED".to_owned(), formal_count).is_some() {
                    return Err("function reserves `PASSED` for its context frame".to_owned());
                }
                formal_count = formal_count
                    .checked_add(1)
                    .ok_or_else(|| "function context frame overflows usize".to_owned())?;
            }
            (formal_count, by_name)
        }
        _ => (0, BTreeMap::<String, usize>::new()),
    };
    let statement_roots = view
        .statement_ids()
        .iter()
        .copied()
        .zip(view.statements())
        .filter_map(|(statement_id, statement)| {
            Some((
                statement.expr?,
                view.stable_statement_key_local(statement_id)?,
            ))
        })
        .collect::<Vec<_>>();
    let raw_expressions = view.expressions().collect::<Vec<_>>();
    let expressions = view.stable_expression_keys().collect::<Vec<_>>();
    if raw_expressions.len() != expressions.len() {
        return Err("owner expression identity table is incomplete".to_owned());
    }
    let mut local_by_syntax = BTreeMap::new();
    for (index, expression) in raw_expressions.iter().enumerate() {
        if local_by_syntax.insert(expression.id, index).is_some() {
            return Err("owner repeats a parser expression identity".to_owned());
        }
    }
    let (fresh_output_inputs, output_bindings_by_scope) = direct_output_callback_bindings(
        &raw_expressions,
        owner_context_ordinal,
        authoritative_call_shapes,
        callable_surfaces,
    )?;
    let raw_result = if matches!(root_statement.kind, AstStatementKind::Function { .. }) {
        view.statement_body_result_expression(root_statement_id)
    } else {
        // Public checked authority follows the finalized statement value, not
        // merely its structural opener. A LIST/FIELD followed by multiline
        // pipe continuations publishes the final call (for example the
        // List/map result), while `statement.expr` intentionally remains the
        // literal/delimiter root for syntax ownership.
        view.checked_statement_value_expression(root_statement_id)
            .or(root_statement.expr)
    }
    .filter(|result| local_by_syntax.contains_key(result));
    let child_owner_result = matches!(root_statement.kind, AstStatementKind::Field { .. })
        .then(|| direct_child_owner_result(view, root_statement_id))
        .transpose()?
        .flatten();
    let synthetic_result = raw_result
        .is_none()
        .then(|| child_owner_result.clone())
        .flatten();
    if raw_result.is_none() && synthetic_result.is_none() {
        let direct_child_boundaries = view
            .child_owners()
            .iter()
            .filter(|boundary| boundary.parent() == Some(root_statement_id))
            .count();
        return Err(format!(
            "owner has no direct or structural result: root_kind={:?} root_children={} direct_child_boundaries={direct_child_boundaries}",
            root_statement.kind,
            root_statement.children.len(),
        ));
    }
    let result_index = match raw_result {
        Some(raw_result) => local_by_syntax
            .get(&raw_result)
            .copied()
            .ok_or_else(|| "owner has no local result expression".to_owned())?,
        None => raw_expressions.len(),
    };
    let result_expression = (root_statement.expr.is_some()
        || matches!(root_statement.kind, AstStatementKind::Function { .. }))
    .then(|| {
        raw_result
            .and_then(|raw_result| local_by_syntax.get(&raw_result).copied())
            .map(|index| expressions[index].clone())
    })
    .flatten();

    let source_paths = direct_view_source_payload_paths(
        &raw_expressions,
        &expressions,
        &local_by_syntax,
        &statement_roots,
    )?;
    let mut structured_records = direct_structured_statement_records(view)?;
    if let Some(container) = root_statement.expr
        && !local_by_syntax
            .get(&container)
            .and_then(|index| raw_expressions.get(*index))
            .is_some_and(|expression| {
                matches!(
                    &expression.kind,
                    AstExprKind::Object(fields) | AstExprKind::TaggedObject { fields, .. }
                        if !fields.is_empty()
                )
            })
        && let Some(PreparedSyntheticResult::Record(fields)) = &child_owner_result
    {
        let entries = structured_records.entry(container).or_default();
        for field in fields {
            match field {
                PreparedRecordEntry::Field { name, value } => {
                    if let Some(PreparedRecordEntry::Field {
                        value: current, ..
                    }) = entries.iter_mut().find(|entry| {
                        matches!(entry, PreparedRecordEntry::Field { name: current, .. } if current == name)
                    }) {
                        *current = value.clone();
                    } else {
                        entries.push(PreparedRecordEntry::Field {
                            name: name.clone(),
                            value: value.clone(),
                        });
                    }
                }
                spread @ PreparedRecordEntry::Spread { .. } => entries.push(spread.clone()),
            }
        }
    }
    let mut child_owner_by_value = BTreeMap::<usize, StableCheckOwnerKey>::new();
    for boundary in view.child_owners() {
        let child_owner = view
            .stable_check_owner_for_local_statement(boundary.statement())
            .ok_or_else(|| "child owner boundary has no stable owner".to_owned())?;
        let values = [
            view.statement_for_local(boundary.statement())
                .and_then(|statement| statement.expr),
            view.child_owner_boundary_expression(boundary),
            view.child_owner_result_expression(boundary),
        ];
        for value in values.into_iter().flatten() {
            if let Some(previous) = child_owner_by_value.insert(value, child_owner.clone())
                && previous != child_owner
            {
                return Err(format!(
                    "child expression {value} belongs to multiple public owners: {previous:#?} and {child_owner:#?}"
                ));
            }
        }
    }
    let mut direct_child_owner_by_name = BTreeMap::<String, StableCheckOwnerKey>::new();
    if let StableCheckOwnerKey::Item(parent_owner) = &owner {
        for boundary in view.child_owners() {
            let child_owner = view
                .stable_check_owner_for_local_statement(boundary.statement())
                .ok_or_else(|| "child owner boundary has no stable owner".to_owned())?;
            let StableCheckOwnerKey::Item(child_item) = &child_owner else {
                continue;
            };
            let parent_segments = parent_owner.item_route.segments();
            let child_segments = child_item.item_route.segments();
            if child_item.source_unit_id != parent_owner.source_unit_id
                || child_segments.len() != parent_segments.len() + 1
                || !child_segments.starts_with(parent_segments)
            {
                continue;
            }
            let statement = view
                .statement_for_local(boundary.statement())
                .ok_or_else(|| "direct child owner has no parser statement".to_owned())?;
            let name = match &statement.kind {
                AstStatementKind::Field { name } => Some(name),
                AstStatementKind::Source {
                    field: Some(name), ..
                }
                | AstStatementKind::Hold {
                    field: Some(name), ..
                }
                | AstStatementKind::List {
                    field: Some(name), ..
                } => Some(name),
                _ => None,
            };
            if let Some(name) = name
                && let Some(previous) =
                    direct_child_owner_by_name.insert(name.clone(), child_owner.clone())
                && previous != child_owner
            {
                return Err(format!(
                    "direct public field `{name}` belongs to multiple child owners: {previous:#?} and {child_owner:#?}"
                ));
            }
        }
    }
    let result_record_fields = raw_result
        .and_then(|result| structured_records.get(&result))
        .or_else(|| {
            synthetic_result.as_ref().and_then(|result| match result {
                PreparedSyntheticResult::Record(fields) => Some(fields),
                PreparedSyntheticResult::Alias(_) => None,
            })
        });
    let mut public_child_owner_fields = result_record_fields
        .into_iter()
        .flatten()
        .filter_map(|field| match field {
            PreparedRecordEntry::Field { name, value } => {
                let child_owner = direct_child_owner_by_name
                    .get(name)
                    .or_else(|| match value {
                        PreparedInputReference::OwnerResult(owner) => Some(owner),
                        PreparedInputReference::Syntax(value) => child_owner_by_value.get(value),
                    })?;
                Some((name.clone(), child_owner.clone()))
            }
            PreparedRecordEntry::Spread { .. } => None,
        })
        .collect::<Vec<_>>();
    if let Some(result) = raw_result
        && let Some(expression) = local_by_syntax
            .get(&result)
            .and_then(|index| raw_expressions.get(*index))
        && let AstExprKind::Object(fields) | AstExprKind::TaggedObject { fields, .. } =
            &expression.kind
    {
        public_child_owner_fields.extend(fields.iter().filter_map(|field| {
            (!field.spread)
                .then(|| {
                    direct_child_owner_by_name
                        .get(&field.name)
                        .or_else(|| child_owner_by_value.get(&field.value))
                })
                .flatten()
                .map(|owner| (field.name.clone(), owner.clone()))
        }));
    }
    let mut public_field_names = BTreeSet::new();
    public_child_owner_fields.retain(|(name, _)| public_field_names.insert(name.clone()));
    let public_child_owner_fields = public_child_owner_fields.into_boxed_slice();
    let statement_record_field_targets =
        direct_statement_record_field_targets(view, &owner, &raw_expressions, &structured_records);
    let lexical_binding_reads = direct_lexical_binding_reads(
        view,
        &owner,
        &raw_expressions,
        &expressions,
        &local_by_syntax,
        &output_bindings_by_scope,
        &structured_records,
        &statement_record_field_targets,
    )?;
    let structured_delimiter_nodes = structured_records
        .keys()
        .filter_map(|syntax| {
            let index = local_by_syntax.get(syntax).copied()?;
            matches!(raw_expressions[index].kind, AstExprKind::Delimiter).then_some(index)
        })
        .collect::<BTreeSet<_>>();

    let mut external_by_key = BTreeMap::new();
    let mut external_expressions = Vec::new();
    let mut call_targets = Vec::new();
    let mut call_shape_diagnostics = Vec::new();
    let has_synthetic_result = synthetic_result.is_some();
    let node_count = raw_expressions.len() + usize::from(has_synthetic_result);
    let mut nodes = Vec::with_capacity(node_count);
    for (index, expression) in raw_expressions.iter().enumerate() {
        let dynamic_authoritative_surface = dynamic_authoritative_call_surface(expression);
        let authoritative_projection = match &expression.kind {
            AstExprKind::Call { function, .. } | AstExprKind::Pipe { op: function, .. } => {
                authoritative_call_shapes
                    .get(function)
                    .or(dynamic_authoritative_surface.as_ref())
                    .map(|surface| {
                        let shape = compact_call_shape_input(
                            checked_kernel_expression(index)?,
                            expression,
                        )?;
                        project_kernel_call_shape(
                            &shape,
                            &KernelCallShapeResolution::Callable {
                                kind: surface.kind,
                                parameters: surface.parameters.clone(),
                                context_ordinal: None,
                                caller_context_ordinal: None,
                            },
                        )
                        .map_err(|error| error.to_string())
                    })
            }
            _ => None,
        }
        .transpose()?;
        let compact_authoritative_name = match &expression.kind {
            AstExprKind::Call { function, .. }
                if render_constructor_kind(function).is_some()
                    || pure_builtin_kind(function)
                        .is_some_and(|kind| kind != KernelPureBuiltinKind::FieldColor)
                    || is_kernel_host_effect(function) =>
            {
                Some(function)
            }
            AstExprKind::Pipe { op, .. }
                if pure_builtin_kind(op)
                    .is_some_and(|kind| kind != KernelPureBuiltinKind::FieldColor)
                    || is_kernel_host_effect(op) =>
            {
                Some(op)
            }
            _ => None,
        };
        if let Some(function) = compact_authoritative_name
            && authoritative_projection.is_none()
        {
            return Err(format!(
                "compact authoritative callable `{function}` has no lexical shape contract"
            ));
        }
        if let Some(projection) = &authoritative_projection {
            call_shape_diagnostics.extend(projection.diagnostics.iter().cloned());
        }
        let (kind, mut raw_edges, call_target, read_target) = if let Some(fields) =
            structured_records.get(&expression.id)
        {
            (
                match &expression.kind {
                    AstExprKind::MatchArm { pattern, .. } => KernelOwnerNodeKind::MatchArm {
                        pattern: compact_pattern(pattern),
                    },
                    _ => KernelOwnerNodeKind::Record { tag: None },
                },
                fields
                    .iter()
                    .map(|entry| match entry {
                        PreparedRecordEntry::Field { name, value } => (
                            KernelOwnerEdgeRole::RecordField {
                                name: name.clone().into_boxed_str(),
                                spread: false,
                            },
                            value.clone(),
                        ),
                        PreparedRecordEntry::Spread { value } => (
                            KernelOwnerEdgeRole::RecordField {
                                name: Box::from(""),
                                spread: true,
                            },
                            value.clone(),
                        ),
                    })
                    .collect(),
                None,
                None,
            )
        } else {
            match &expression.kind {
                _ if authoritative_projection
                    .as_ref()
                    .is_some_and(|projection| !projection.valid) =>
                {
                    (KernelOwnerNodeKind::Unknown, Vec::new(), None, None)
                }
                AstExprKind::Identifier(_) if fresh_output_inputs.contains(&expression.id) => {
                    (KernelOwnerNodeKind::FreshOut, Vec::new(), None, None)
                }
                AstExprKind::Identifier(name) => {
                    if let Some(binding) = lexical_binding_reads.get(&expression.id) {
                        let (kind, edges) = prepared_lexical_read_node(binding, &[])?;
                        (kind, edges, None, None)
                    } else if let Some(formal) = formal_by_name.get(name).copied() {
                        (
                            if Some(formal) == owner_context_ordinal {
                                KernelOwnerNodeKind::ContextRead {
                                    formal: checked_u32(formal, "context formal ordinal")?,
                                    fields: Box::new([]),
                                }
                            } else {
                                KernelOwnerNodeKind::FormalRead {
                                    formal: checked_u32(formal, "formal ordinal")?,
                                    fields: Box::new([]),
                                }
                            },
                            Vec::new(),
                            None,
                            None,
                        )
                    } else {
                        match exact_value_surface(name, value_surfaces, &owner) {
                            Ok(surface) => match local_value_surface_provider(
                                surface,
                                &owner,
                                &expressions,
                                &raw_expressions,
                                raw_result,
                            )? {
                                Some(provider) => (
                                    KernelOwnerNodeKind::LexicalRead {
                                        fields: Box::new([]),
                                    },
                                    vec![(
                                        KernelOwnerEdgeRole::ReadProvider,
                                        PreparedInputReference::Syntax(provider),
                                    )],
                                    None,
                                    None,
                                ),
                                None => (
                                    KernelOwnerNodeKind::ValueRead {
                                        fields: Box::new([]),
                                        mode_narrowing: None,
                                    },
                                    Vec::new(),
                                    None,
                                    Some(surface.clone()),
                                ),
                            },
                            Err(_) => {
                                let kind = if name == "PASSED" {
                                    KernelDiagnosticKind::MissingPassedContext
                                } else if callable_surfaces.contains_key(name) {
                                    KernelDiagnosticKind::CallableUsedAsValue {
                                        function: name.clone().into_boxed_str(),
                                    }
                                } else {
                                    let candidate_count =
                                        value_surfaces.get(name).map(Vec::len).unwrap_or_default();
                                    if candidate_count > 1 {
                                        KernelDiagnosticKind::AmbiguousValue {
                                            name: name.clone().into_boxed_str(),
                                            candidate_count: checked_u32(
                                                candidate_count,
                                                "ambiguous value candidate count",
                                            )?,
                                        }
                                    } else {
                                        KernelDiagnosticKind::UnresolvedValue {
                                            name: name.clone().into_boxed_str(),
                                        }
                                    }
                                };
                                call_shape_diagnostics.push(
                                    boon_compiler_kernel::KernelDiagnosticInput {
                                        severity: KernelDiagnosticSeverity::Error,
                                        site: KernelDiagnosticSite::Expression {
                                            expression: checked_kernel_expression(index)?,
                                        },
                                        kind,
                                    },
                                );
                                (KernelOwnerNodeKind::Unknown, Vec::new(), None, None)
                            }
                        }
                    }
                }
                AstExprKind::Path(_) | AstExprKind::Drain { .. } => {
                    let parts = match &expression.kind {
                        AstExprKind::Path(path) => path.clone(),
                        AstExprKind::Drain { path } => match path {
                            boon_syntax::AstDrainPath::Binding { name } => vec![name.clone()],
                            boon_syntax::AstDrainPath::Field { binding, fields } => {
                                std::iter::once(binding.clone())
                                    .chain(fields.iter().cloned())
                                    .collect()
                            }
                            boon_syntax::AstDrainPath::Passed { fields } => {
                                std::iter::once("PASSED".to_owned())
                                    .chain(fields.iter().cloned())
                                    .collect()
                            }
                        },
                        _ => unreachable!("path/read arm only accepts lexical path syntax"),
                    };
                    let (root, fields) = parts
                        .split_first()
                        .ok_or_else(|| "value path has no root".to_owned())?;
                    let path_fields = fields
                        .iter()
                        .cloned()
                        .map(String::into_boxed_str)
                        .collect::<Vec<_>>()
                        .into_boxed_slice();
                    if let Some(binding) = lexical_binding_reads.get(&expression.id) {
                        let (kind, edges) = prepared_lexical_read_node(binding, &path_fields)?;
                        (kind, edges, None, None)
                    } else if let Some(formal) = formal_by_name.get(root).copied() {
                        (
                            if Some(formal) == owner_context_ordinal {
                                KernelOwnerNodeKind::ContextRead {
                                    formal: checked_u32(formal, "context formal ordinal")?,
                                    fields: path_fields,
                                }
                            } else {
                                KernelOwnerNodeKind::FormalRead {
                                    formal: checked_u32(formal, "formal ordinal")?,
                                    fields: path_fields,
                                }
                            },
                            Vec::new(),
                            None,
                            None,
                        )
                    } else {
                        match exact_value_surface(root, value_surfaces, &owner) {
                            Ok(surface) => match local_value_surface_provider(
                                surface,
                                &owner,
                                &expressions,
                                &raw_expressions,
                                raw_result,
                            )? {
                                Some(provider) => (
                                    KernelOwnerNodeKind::LexicalRead {
                                        fields: path_fields,
                                    },
                                    vec![(
                                        KernelOwnerEdgeRole::ReadProvider,
                                        PreparedInputReference::Syntax(provider),
                                    )],
                                    None,
                                    None,
                                ),
                                None => (
                                    KernelOwnerNodeKind::ValueRead {
                                        fields: path_fields,
                                        mode_narrowing: None,
                                    },
                                    Vec::new(),
                                    None,
                                    Some(surface.clone()),
                                ),
                            },
                            Err(_) => {
                                let kind = if root == "PASSED" {
                                    KernelDiagnosticKind::MissingPassedContext
                                } else if callable_surfaces.contains_key(root) {
                                    KernelDiagnosticKind::CallableUsedAsValue {
                                        function: root.clone().into_boxed_str(),
                                    }
                                } else {
                                    let candidate_count =
                                        value_surfaces.get(root).map(Vec::len).unwrap_or_default();
                                    if candidate_count > 1 {
                                        KernelDiagnosticKind::AmbiguousValue {
                                            name: root.clone().into_boxed_str(),
                                            candidate_count: checked_u32(
                                                candidate_count,
                                                "ambiguous value candidate count",
                                            )?,
                                        }
                                    } else {
                                        KernelDiagnosticKind::UnresolvedValue {
                                            name: root.clone().into_boxed_str(),
                                        }
                                    }
                                };
                                call_shape_diagnostics.push(
                                    boon_compiler_kernel::KernelDiagnosticInput {
                                        severity: KernelDiagnosticSeverity::Error,
                                        site: KernelDiagnosticSite::Expression {
                                            expression: checked_kernel_expression(index)?,
                                        },
                                        kind,
                                    },
                                );
                                (KernelOwnerNodeKind::Unknown, Vec::new(), None, None)
                            }
                        }
                    }
                }
                AstExprKind::Call {
                    function,
                    args,
                    pass: _,
                } if render_constructor_kind(function).is_some() => (
                    KernelOwnerNodeKind::RenderConstructor {
                        kind: render_constructor_kind(function)
                            .expect("render constructor guard resolves"),
                    },
                    args.iter()
                        .map(|argument| {
                            (
                                KernelOwnerEdgeRole::AbiArgument {
                                    name: argument.name.clone().into_boxed_str(),
                                },
                                PreparedInputReference::Syntax(argument.value),
                            )
                        })
                        .collect(),
                    None,
                    None,
                ),
                AstExprKind::Call {
                    function,
                    args,
                    pass: _,
                } if pure_builtin_kind(function).is_some_and(|kind| {
                    kind != KernelPureBuiltinKind::FieldColor || authoritative_projection.is_some()
                }) =>
                {
                    (
                        KernelOwnerNodeKind::PureBuiltin {
                            kind: pure_builtin_kind(function).expect("pure builtin guard resolves"),
                        },
                        args.iter()
                            .map(|argument| {
                                (
                                    KernelOwnerEdgeRole::AbiArgument {
                                        name: argument.name.clone().into_boxed_str(),
                                    },
                                    PreparedInputReference::Syntax(argument.value),
                                )
                            })
                            .collect(),
                        None,
                        None,
                    )
                }
                AstExprKind::Pipe {
                    input,
                    op,
                    args,
                    pass: _,
                    arms,
                } if pure_builtin_kind(op).is_some_and(|kind| {
                    kind != KernelPureBuiltinKind::FieldColor || authoritative_projection.is_some()
                }) =>
                {
                    if !arms.is_empty() {
                        return Err(format!(
                            "pure builtin pipe `{op}` cannot consume arms in the compact ABI"
                        ));
                    }
                    let mut inputs = Vec::with_capacity(args.len() + 1);
                    let input = expression.linked_input.unwrap_or(*input);
                    inputs.push((
                        KernelOwnerEdgeRole::AbiArgument {
                            name: "$pipe".into(),
                        },
                        PreparedInputReference::Syntax(input),
                    ));
                    inputs.extend(args.iter().map(|argument| {
                        (
                            KernelOwnerEdgeRole::AbiArgument {
                                name: argument.name.clone().into_boxed_str(),
                            },
                            PreparedInputReference::Syntax(argument.value),
                        )
                    }));
                    (
                        KernelOwnerNodeKind::PureBuiltin {
                            kind: pure_builtin_kind(op).expect("pure builtin pipe guard resolves"),
                        },
                        inputs,
                        None,
                        None,
                    )
                }
                AstExprKind::Pipe {
                    input,
                    op,
                    args,
                    arms,
                    ..
                } if op.starts_with("Field/") && args.is_empty() && arms.is_empty() => {
                    let field = op
                        .strip_prefix("Field/")
                        .filter(|field| !field.is_empty())
                        .ok_or_else(|| "field projection omits its field name".to_owned())?;
                    (
                        KernelOwnerNodeKind::DerivedRead {
                            fields: vec![field.into()].into_boxed_slice(),
                        },
                        vec![(
                            KernelOwnerEdgeRole::ReadProvider,
                            PreparedInputReference::Syntax(
                                expression.linked_input.unwrap_or(*input),
                            ),
                        )],
                        None,
                        None,
                    )
                }
                AstExprKind::Call { function, args, .. } if function.starts_with("Field/") => {
                    let field = function
                        .strip_prefix("Field/")
                        .filter(|field| !field.is_empty())
                        .ok_or_else(|| "field projection omits its field name".to_owned())?;
                    let input = args
                        .iter()
                        .find(|argument| argument.name == "input")
                        .ok_or_else(|| "field projection has no `input` argument".to_owned())?;
                    (
                        KernelOwnerNodeKind::DerivedRead {
                            fields: vec![field.into()].into_boxed_slice(),
                        },
                        vec![(
                            KernelOwnerEdgeRole::ReadProvider,
                            PreparedInputReference::Syntax(input.value),
                        )],
                        None,
                        None,
                    )
                }
                AstExprKind::Call {
                    function,
                    args,
                    pass: _,
                } if is_kernel_host_effect(function) => (
                    KernelOwnerNodeKind::HostEffect {
                        operation: function.clone().into_boxed_str(),
                    },
                    args.iter()
                        .map(|argument| {
                            (
                                KernelOwnerEdgeRole::AbiArgument {
                                    name: argument.name.clone().into_boxed_str(),
                                },
                                PreparedInputReference::Syntax(argument.value),
                            )
                        })
                        .collect(),
                    None,
                    None,
                ),
                AstExprKind::Pipe {
                    input,
                    op,
                    args,
                    pass: _,
                    arms,
                } if is_kernel_host_effect(op) => {
                    if !arms.is_empty() {
                        return Err(format!(
                            "host-effect pipe `{op}` cannot consume arms in the compact ABI"
                        ));
                    }
                    let mut inputs = Vec::with_capacity(args.len() + 1);
                    let input = expression.linked_input.unwrap_or(*input);
                    inputs.push((
                        KernelOwnerEdgeRole::AbiArgument {
                            name: "$pipe".into(),
                        },
                        PreparedInputReference::Syntax(input),
                    ));
                    inputs.extend(args.iter().map(|argument| {
                        (
                            KernelOwnerEdgeRole::AbiArgument {
                                name: argument.name.clone().into_boxed_str(),
                            },
                            PreparedInputReference::Syntax(argument.value),
                        )
                    }));
                    (
                        KernelOwnerNodeKind::HostEffect {
                            operation: op.clone().into_boxed_str(),
                        },
                        inputs,
                        None,
                        None,
                    )
                }
                AstExprKind::Call { function, .. } | AstExprKind::Pipe { op: function, .. }
                    if is_authoritative_callable_name(authoritative_call_shapes, function)
                        || dynamic_authoritative_surface.is_some() =>
                {
                    return Err(format!(
                        "authoritative callable `{function}` is not in the current compact ABI slice"
                    ));
                }
                AstExprKind::Call { function, .. }
                | AstExprKind::Pipe {
                    op: function,
                    arms: _,
                    ..
                } if callable_surfaces.contains_key(function) => {
                    let candidates = &callable_surfaces[function];
                    let unique = match candidates.as_ref() {
                        [surface] => Some(surface),
                        [] => {
                            return Err(format!("callable surface `{function}` has no candidates"));
                        }
                        _ => None,
                    };
                    let shape =
                        compact_call_shape_input(checked_kernel_expression(index)?, expression)?;
                    let resolution = match unique {
                        Some(surface) => KernelCallShapeResolution::Callable {
                            kind: KernelCallableKind::User,
                            parameters: compact_call_shape_parameters(surface)?,
                            context_ordinal: surface
                                .context_ordinal
                                .map(|ordinal| checked_u32(ordinal, "call context ordinal"))
                                .transpose()?,
                            caller_context_ordinal: owner_context_ordinal
                                .map(|ordinal| checked_u32(ordinal, "caller context ordinal"))
                                .transpose()?,
                        },
                        None => KernelCallShapeResolution::Ambiguous {
                            candidate_count: checked_u32(
                                candidates.len(),
                                "ambiguous callable candidate count",
                            )?,
                        },
                    };
                    let projection = project_kernel_call_shape(&shape, &resolution)
                        .map_err(|error| error.to_string())?;
                    call_shape_diagnostics.extend(projection.diagnostics.iter().cloned());
                    match (projection.valid, unique) {
                        (true, Some(surface)) => {
                            if matches!(&expression.kind, AstExprKind::Pipe { arms, .. } if !arms.is_empty())
                            {
                                return Err(
                                    "valid user callable pipes with arms are not in the current call-composition slice"
                                        .to_owned(),
                                );
                            }
                            let raw_edges = prepared_call_shape_edges(
                                &projection,
                                expression,
                                &surface.parameters,
                            )?;
                            (
                                KernelOwnerNodeKind::UserCall {
                                    target: KernelOwnerId(0),
                                    inherited_formal: projection.inherited_formal,
                                },
                                raw_edges,
                                Some(surface.owner.clone()),
                                None,
                            )
                        }
                        (true, None) => {
                            return Err(format!(
                                "ambiguous call `{function}` was accepted without one target"
                            ));
                        }
                        (false, _) => (KernelOwnerNodeKind::Unknown, Vec::new(), None, None),
                    }
                }
                AstExprKind::Call { .. } => {
                    let shape =
                        compact_call_shape_input(checked_kernel_expression(index)?, expression)?;
                    let projection =
                        project_kernel_call_shape(&shape, &KernelCallShapeResolution::Unresolved)
                            .map_err(|error| error.to_string())?;
                    call_shape_diagnostics.extend(projection.diagnostics.iter().cloned());
                    (KernelOwnerNodeKind::Unknown, Vec::new(), None, None)
                }
                AstExprKind::Pipe { op, arms, .. } if !(op == "WHILE" && !arms.is_empty()) => {
                    let shape =
                        compact_call_shape_input(checked_kernel_expression(index)?, expression)?;
                    let projection =
                        project_kernel_call_shape(&shape, &KernelCallShapeResolution::Unresolved)
                            .map_err(|error| error.to_string())?;
                    call_shape_diagnostics.extend(projection.diagnostics.iter().cloned());
                    (KernelOwnerNodeKind::Unknown, Vec::new(), None, None)
                }
                _ => (
                    compact_ast_kind(
                        &expression.kind,
                        &expressions[index],
                        &source_paths,
                        source_payloads,
                    )?,
                    compact_ast_edges(&expression.kind, expression.linked_input)?
                        .into_iter()
                        .map(|(role, reference)| (role, PreparedInputReference::Syntax(reference)))
                        .collect(),
                    None,
                    None,
                ),
            }
        };
        if matches!(kind, KernelOwnerNodeKind::Hold) {
            raw_edges.extend(
                direct_hold_update_expressions(view, expression.id, &raw_expressions)?
                    .into_iter()
                    .map(|update| {
                        (
                            KernelOwnerEdgeRole::HoldUpdate,
                            PreparedInputReference::Syntax(update),
                        )
                    }),
            );
        }
        if let Some(owner) = call_target {
            call_targets.push(PreparedCallTarget { node: index, owner });
        }
        let mode = match &kind {
            KernelOwnerNodeKind::Source(_) => FlowMode::PresentOrAbsent,
            KernelOwnerNodeKind::Then => FlowMode::PresentOrAbsent,
            KernelOwnerNodeKind::Absent => FlowMode::Absent,
            _ => FlowMode::Continuous,
        };
        let mut inputs = raw_edges
            .into_iter()
            .map(|(role, reference)| {
                let reference = prepared_input_reference_index(
                    reference,
                    view,
                    &owner,
                    Some(expression.id),
                    &local_by_syntax,
                    node_count,
                    &mut external_by_key,
                    &mut external_expressions,
                )?;
                Ok(KernelOwnerInputEdge {
                    role,
                    expression: checked_kernel_expression(reference)?,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        if let Some(read_target) = read_target {
            let key = PreparedExternalExpression {
                owner: read_target.owner,
                target: read_target.target,
            };
            let external = match external_by_key.get(&key).copied() {
                Some(external) => external,
                None => {
                    let external = external_expressions.len();
                    external_by_key.insert(key.clone(), external);
                    external_expressions.push(key);
                    external
                }
            };
            let reference = node_count
                .checked_add(external)
                .ok_or_else(|| "owner expression namespace overflowed".to_owned())?;
            inputs.push(KernelOwnerInputEdge {
                role: KernelOwnerEdgeRole::ReadProvider,
                expression: checked_kernel_expression(reference)?,
            });
        }
        nodes.push(KernelOwnerNode {
            kind,
            inputs: inputs.into_boxed_slice(),
            mode,
        });
    }
    if let Some(synthetic_result) = synthetic_result {
        let (kind, edges) = match synthetic_result {
            PreparedSyntheticResult::Alias(reference) => (
                KernelOwnerNodeKind::Block,
                vec![(KernelOwnerEdgeRole::BlockResult, reference)],
            ),
            PreparedSyntheticResult::Record(fields) => (
                KernelOwnerNodeKind::Record { tag: None },
                fields
                    .into_iter()
                    .map(|entry| match entry {
                        PreparedRecordEntry::Field { name, value } => (
                            KernelOwnerEdgeRole::RecordField {
                                name: name.into_boxed_str(),
                                spread: false,
                            },
                            value,
                        ),
                        PreparedRecordEntry::Spread { value } => (
                            KernelOwnerEdgeRole::RecordField {
                                name: Box::from(""),
                                spread: true,
                            },
                            value,
                        ),
                    })
                    .collect(),
            ),
        };
        let inputs = edges
            .into_iter()
            .map(|(role, reference)| {
                let reference = prepared_input_reference_index(
                    reference,
                    view,
                    &owner,
                    None,
                    &local_by_syntax,
                    node_count,
                    &mut external_by_key,
                    &mut external_expressions,
                )?;
                Ok(KernelOwnerInputEdge {
                    role,
                    expression: checked_kernel_expression(reference)?,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        nodes.push(KernelOwnerNode {
            kind,
            inputs: inputs.into_boxed_slice(),
            mode: result_mode,
        });
    }
    attach_tag_match_mode_narrowings(&mut nodes);
    let result_node = nodes
        .get_mut(result_index)
        .ok_or_else(|| "owner result is outside its compact node table".to_owned())?;
    if matches!(result_node.mode, FlowMode::Continuous) {
        result_node.mode = result_mode;
    }
    for (when_index, when) in nodes.iter().enumerate() {
        if !matches!(when.kind, KernelOwnerNodeKind::When) {
            continue;
        }
        for arm in &when.inputs {
            if !matches!(arm.role, KernelOwnerEdgeRole::WhenArm) {
                continue;
            }
            let arm_index = arm.expression.0 as usize;
            let Some(KernelOwnerNode {
                kind: KernelOwnerNodeKind::MatchArm { .. },
                inputs,
                ..
            }) = nodes.get(arm_index)
            else {
                continue;
            };
            let unsupported_delimiter = inputs.iter().any(|input| {
                if !matches!(input.role, KernelOwnerEdgeRole::MatchOutput) {
                    return false;
                }
                nodes
                    .get(input.expression.0 as usize)
                    .is_some_and(|output| matches!(output.kind, KernelOwnerNodeKind::Unknown))
            });
            if unsupported_delimiter {
                return Err(format!(
                    "WHEN node {when_index} has a delimiter arm whose structural record was not recovered"
                ));
            }
        }
    }
    debug_assert_eq!(nodes.len(), node_count);
    let structured_delimiter_dependents = local_dependency_cone(&nodes, structured_delimiter_nodes)
        .into_iter()
        .map(|index| expressions[index].clone())
        .collect::<Vec<_>>()
        .into_boxed_slice();
    let record_spread_nodes = nodes
        .iter()
        .enumerate()
        .filter_map(|(index, node)| {
            node.inputs
                .iter()
                .any(|input| {
                    matches!(
                        input.role,
                        KernelOwnerEdgeRole::RecordField { spread: true, .. }
                    )
                })
                .then_some(index)
        })
        .collect::<BTreeSet<_>>();
    let record_spread_dependents = local_dependency_cone(&nodes, record_spread_nodes)
        .into_iter()
        .map(|index| expressions[index].clone())
        .collect::<Vec<_>>()
        .into_boxed_slice();
    let formal_nodes = nodes
        .iter()
        .enumerate()
        .filter_map(|(index, node)| {
            matches!(
                node.kind,
                KernelOwnerNodeKind::FormalRead { .. } | KernelOwnerNodeKind::ContextRead { .. }
            )
            .then_some(index)
        })
        .collect::<BTreeSet<_>>();
    let formal_dependents = local_dependency_cone(&nodes, formal_nodes);
    let generic_selectors = nodes
        .iter()
        .enumerate()
        .filter_map(|(index, node)| {
            if !matches!(node.kind, KernelOwnerNodeKind::When) {
                return None;
            }
            node.inputs
                .iter()
                .any(|input| {
                    matches!(input.role, KernelOwnerEdgeRole::WhenInput)
                        && formal_dependents.contains(&(input.expression.0 as usize))
                })
                .then_some(index)
        })
        .collect::<BTreeSet<_>>();
    let mut generic_selector_nodes = local_input_closure(&nodes, generic_selectors);
    generic_selector_nodes.extend(nodes.iter().enumerate().filter_map(|(index, node)| {
        let KernelOwnerNodeKind::UserCall {
            inherited_formal, ..
        } = &node.kind
        else {
            return None;
        };
        (formal_dependents.contains(&index) || inherited_formal.is_some()).then_some(index)
    }));
    let generic_selector_dependents = local_dependency_cone(&nodes, generic_selector_nodes)
        .into_iter()
        .map(|index| expressions[index].clone())
        .collect::<Vec<_>>()
        .into_boxed_slice();
    let detached_generic_reads = nodes
        .iter()
        .enumerate()
        .filter_map(|(index, node)| {
            (formal_dependents.contains(&index)
                && matches!(
                    node.kind,
                    KernelOwnerNodeKind::ValueRead { .. } | KernelOwnerNodeKind::DerivedRead { .. }
                ))
            .then(|| expressions[index].clone())
        })
        .collect::<Vec<_>>()
        .into_boxed_slice();
    // `NoElement` is an ordinary tag in Boon and is intentionally ordinary in
    // the dense kernel. The legacy checker nevertheless treats that spelling
    // as the identity element of structural widening for its built-in UI ABI.
    // Mark only the local dependency cone so the migration differential can
    // recognize that old, narrower surface without teaching the new type
    // algebra the UI library's tag name.
    let no_element_nodes = nodes
        .iter()
        .enumerate()
        .filter_map(|(index, node)| {
            matches!(&node.kind, KernelOwnerNodeKind::Tag(tag) if tag.as_ref() == "NoElement")
                .then_some(index)
        })
        .collect::<BTreeSet<_>>();
    let legacy_no_element_dependents = local_dependency_cone(&nodes, no_element_nodes)
        .into_iter()
        .map(|index| expressions[index].clone())
        .collect::<Vec<_>>()
        .into_boxed_slice();
    // The legacy owner assembler inconsistently promotes an inline record
    // containing a local SOURCE to PresentOrAbsent, while the same structural
    // wrapper remains Continuous once SOURCE ownership is split into a child
    // owner. The dense kernel keeps records as continuous values and projects
    // the event mode only at the SOURCE field. This set exists solely to keep
    // that legacy mode leak out of differential parity.
    let legacy_source_container_modes = local_source_container_nodes(&nodes)
        .into_iter()
        .map(|index| expressions[index].clone())
        .collect::<Vec<_>>()
        .into_boxed_slice();
    if let Some(pattern) = std::env::var_os("BOON_KERNEL_ORACLE_TRACE_OWNER") {
        let pattern = pattern.to_string_lossy();
        if format!("{owner:?}").contains(pattern.as_ref()) {
            eprintln!("kernel-owner-trace owner={owner:#?}");
            eprintln!("kernel-owner-trace externals={external_expressions:#?}");
            eprintln!("kernel-owner-trace calls={call_targets:#?}");
            for (index, node) in nodes.iter().enumerate() {
                eprintln!(
                    "kernel-owner-trace node={index} stable={:?} value={node:?}",
                    expressions.get(index)
                );
            }
        }
    }
    // Definition formals and contextual collection items are provider roots,
    // not consumer-shaped occurrence rows. The legacy checker lets downstream
    // reads back-shape those roots; the directional kernel deliberately does
    // not. Their uses and every concrete call occurrence remain differential
    // checks, so omit only these synthetic provider expressions themselves.
    let generic_formal_reads = nodes
        .iter()
        .enumerate()
        .filter_map(|(index, node)| {
            matches!(
                node.kind,
                KernelOwnerNodeKind::FormalRead { .. }
                    | KernelOwnerNodeKind::ContextRead { .. }
                    | KernelOwnerNodeKind::CollectionItemRead
                    | KernelOwnerNodeKind::FreshOut
            )
            .then(|| expressions[index].clone())
        })
        .collect::<Vec<_>>()
        .into_boxed_slice();
    let (statements, mut definition_facts, statement_child_targets) = compact_statement_facts(
        view,
        &owner,
        owner_callable_surface,
        &local_by_syntax,
        node_count,
        &mut external_by_key,
        &mut external_expressions,
    )?;
    definition_facts.relocations = KernelDefinitionRelocations {
        expressions: expressions
            .iter()
            .cloned()
            .map(KernelExpressionRelocation::Authored)
            .chain(
                has_synthetic_result
                    .then_some(KernelExpressionRelocation::SyntheticDefinitionResult),
            )
            .collect::<Vec<_>>()
            .into_boxed_slice(),
        statements: statements.clone(),
    };
    definition_facts.expression_payloads = raw_expressions
        .iter()
        .map(|expression| compact_expression_semantic_payload(&expression.kind))
        .chain(has_synthetic_result.then_some(KernelExpressionSemanticPayload::None))
        .collect::<Vec<_>>()
        .into_boxed_slice();
    definition_facts.call_syntax = raw_expressions
        .iter()
        .zip(nodes.iter())
        .enumerate()
        .filter_map(|(index, (expression, node))| {
            matches!(
                &node.kind,
                KernelOwnerNodeKind::UserCall { .. }
                    | KernelOwnerNodeKind::RenderConstructor { .. }
                    | KernelOwnerNodeKind::PureBuiltin { .. }
                    | KernelOwnerNodeKind::HostEffect { .. }
            )
            .then(|| {
                compact_call_syntax_input(
                    index,
                    expression,
                    view,
                    &owner,
                    &local_by_syntax,
                    node_count,
                    &mut external_by_key,
                    &mut external_expressions,
                )
            })
        })
        .collect::<Result<Vec<_>, _>>()?
        .into_boxed_slice();
    let render_slots = view
        .statements()
        .zip(statements.iter())
        .zip(definition_facts.statements.iter())
        .filter_map(|((syntax, statement), fact)| {
            Some((
                kernel_render_slot_name(&owner, statement, syntax)?.into(),
                fact.value?,
            ))
        })
        .collect::<Vec<(Box<str>, KernelExpressionId)>>();
    definition_facts.diagnostic_values = render_slots
        .iter()
        .map(|(_, expression)| *expression)
        .collect::<Vec<_>>()
        .into_boxed_slice();
    let (declarations, lexical_bindings, lexical_owner_targets) =
        compact_declaration_and_lexical_facts(
            view,
            &owner,
            root_statement_id,
            &raw_expressions,
            &local_by_syntax,
            &formal_by_name,
            owner_context_ordinal,
            &lexical_binding_reads,
            &output_bindings_by_scope,
            &statement_record_field_targets,
            value_surfaces,
            node_count,
            &definition_facts.statements,
            &mut external_by_key,
            &mut external_expressions,
        )?;
    definition_facts.declarations = declarations;
    definition_facts.lexical_bindings = lexical_bindings;
    let public_declaration = definition_facts
        .declarations
        .iter()
        .find(|declaration| {
            declaration.origin
                == (KernelDeclarationOrigin::Statement {
                    statement: root_statement_dense,
                })
        })
        .map(|declaration| KernelDeclarationReference::Local(declaration.id));
    definition_facts.linkage = boon_compiler_kernel::KernelDefinitionLinkage {
        root_statement: Some(root_statement_dense),
        public_declaration,
        result_expression: Some(checked_kernel_expression(result_index)?),
        context_formal_ordinal: owner_context_ordinal
            .map(|ordinal| checked_u32(ordinal, "definition context formal ordinal"))
            .transpose()?,
    };
    definition_facts.execution_shapes = compact_execution_shape_inputs(
        view,
        &owner,
        &raw_expressions,
        &local_by_syntax,
        &nodes,
        &statement_record_field_targets,
        &definition_facts.declarations,
        node_count,
        &mut external_by_key,
        &mut external_expressions,
    )?;
    let mut diagnostics =
        project_kernel_source_expression_diagnostics(raw_expressions.iter().enumerate().map(
            |(index, expression)| {
                (
                    KernelExpressionId(
                        u32::try_from(index).expect("kernel owner expression count exceeds u32"),
                    ),
                    *expression,
                )
            },
        ))
        .map_err(|error| error.to_string())?
        .into_vec();
    diagnostics.extend(call_shape_diagnostics);
    definition_facts.diagnostics = diagnostics.into_boxed_slice();
    let (resource_owner_targets, resource_synthetic_paths) = compact_resource_facts(
        view,
        &owner,
        root_statement_id,
        &raw_expressions,
        &local_by_syntax,
        &nodes,
        checked_kernel_expression(result_index)?,
        &mut definition_facts,
    )?;
    let mut resource_owner_targets = resource_owner_targets.into_vec();
    if definition_facts.linkage.public_declaration.is_none() {
        let result = definition_facts
            .linkage
            .result_expression
            .expect("definition result linkage was populated above");
        let mut candidates = definition_facts
            .sources
            .iter()
            .enumerate()
            .filter(|(_, source)| {
                source.expression == result
                    || source.statement == KernelStatementReference::Local(root_statement_dense)
            })
            .map(|(row, source)| {
                (
                    source.declaration,
                    PreparedResourceOwnerField::SourceDeclaration(row),
                )
            })
            .chain(
                definition_facts
                    .states
                    .iter()
                    .enumerate()
                    .filter(|(_, state)| {
                        state.expression == result
                            || state.statement
                                == KernelStatementReference::Local(root_statement_dense)
                    })
                    .map(|(row, state)| {
                        (
                            state.declaration,
                            PreparedResourceOwnerField::StateDeclaration(row),
                        )
                    }),
            )
            .chain(
                definition_facts
                    .lists
                    .iter()
                    .enumerate()
                    .filter(|(_, list)| {
                        list.producer == result
                            || list.statement
                                == KernelStatementReference::Local(root_statement_dense)
                    })
                    .map(|(row, list)| {
                        (
                            list.declaration,
                            PreparedResourceOwnerField::ListDeclaration(row),
                        )
                    }),
            )
            .collect::<Vec<_>>();
        let mut unique_candidates = Vec::with_capacity(candidates.len());
        for candidate in candidates.drain(..) {
            if !unique_candidates.contains(&candidate) {
                unique_candidates.push(candidate);
            }
        }
        let candidates = unique_candidates;
        let [(public_declaration, resource_field)] = candidates.as_slice() else {
            return Err(format!(
                "owner root statement has no unique local or exact resource declaration authority: {candidates:?}"
            ));
        };
        definition_facts.linkage.public_declaration = Some(*public_declaration);
        if matches!(
            public_declaration,
            KernelDeclarationReference::OwnerPublic(_)
        ) {
            let authority = resource_owner_targets
                .iter()
                .find(|target| target.field == *resource_field)
                .map(|target| target.owner.clone())
                .ok_or_else(|| {
                    "owner root resource has an unresolved public declaration authority".to_owned()
                })?;
            resource_owner_targets.push(PreparedResourceOwnerTarget {
                field: PreparedResourceOwnerField::LinkagePublicDeclaration,
                owner: authority,
            });
        }
    }
    let (presentation, containing_scope_targets) = compact_checked_presentation(
        view,
        &raw_expressions,
        &local_by_syntax,
        &nodes,
        &definition_facts,
    )?;
    definition_facts.presentation = presentation;
    Ok(PreparedOwner {
        owner,
        expressions: expressions.into_boxed_slice(),
        statements,
        definition_facts,
        render_slots: render_slots
            .into_iter()
            .map(|(slot, _)| slot)
            .collect::<Vec<_>>()
            .into_boxed_slice(),
        statement_child_targets,
        containing_scope_targets,
        lexical_owner_targets,
        resource_owner_targets: resource_owner_targets.into_boxed_slice(),
        resource_synthetic_paths,
        external_expressions: external_expressions.into_boxed_slice(),
        call_targets: call_targets.into_boxed_slice(),
        compact: KernelOwnerProgramInput {
            nodes: nodes.into_boxed_slice(),
            formal_count: checked_u32(formal_count, "formal count")?,
            external_expressions: Box::new([]),
            result: checked_kernel_expression(result_index)?,
        },
        result_expression,
        public_child_owner_fields,
        generic_formal_reads,
        structured_delimiter_dependents,
        record_spread_dependents,
        generic_selector_dependents,
        detached_generic_reads,
        legacy_no_element_dependents,
        legacy_source_container_modes,
    })
}

/// Attach the local proof that a nested value read occurs under a matching
/// tagged `WHEN` arm. Type selection already narrows that occurrence through
/// the arm equation; modes need the same explicit relation so a retained
/// `LATEST` provider does not reintroduce event modes from historical branches
/// that the arm has ruled out.
fn attach_tag_match_mode_narrowings(nodes: &mut [KernelOwnerNode]) {
    let mut narrowings = BTreeMap::<usize, (usize, KernelExpressionId)>::new();
    for when in 0..nodes.len() {
        if !matches!(nodes[when].kind, KernelOwnerNodeKind::When) {
            continue;
        }
        let Some(selector) = nodes[when]
            .inputs
            .iter()
            .find(|edge| matches!(edge.role, KernelOwnerEdgeRole::WhenInput))
            .map(|edge| edge.expression)
        else {
            continue;
        };
        let Some(selector_node) = nodes.get(selector.0 as usize) else {
            continue;
        };
        let KernelOwnerNodeKind::ValueRead {
            fields: selector_fields,
            ..
        } = &selector_node.kind
        else {
            continue;
        };
        let Some(selector_provider) = selector_node
            .inputs
            .iter()
            .find(|edge| matches!(edge.role, KernelOwnerEdgeRole::ReadProvider))
            .map(|edge| edge.expression)
        else {
            continue;
        };

        for arm in nodes[when]
            .inputs
            .iter()
            .filter(|edge| matches!(edge.role, KernelOwnerEdgeRole::WhenArm))
        {
            let Some(arm_node) = nodes.get(arm.expression.0 as usize) else {
                continue;
            };
            if !matches!(
                arm_node.kind,
                KernelOwnerNodeKind::MatchArm {
                    pattern: KernelPattern::Tag { .. }
                }
            ) {
                continue;
            }
            let Some(output) = arm_node
                .inputs
                .iter()
                .find(|edge| matches!(edge.role, KernelOwnerEdgeRole::MatchOutput))
                .map(|edge| edge.expression.0 as usize)
                .filter(|output| *output < nodes.len())
            else {
                continue;
            };
            for candidate in local_input_closure(nodes, BTreeSet::from([output])) {
                let KernelOwnerNodeKind::ValueRead {
                    fields: candidate_fields,
                    ..
                } = &nodes[candidate].kind
                else {
                    continue;
                };
                if candidate_fields.len() <= selector_fields.len()
                    || !candidate_fields.starts_with(selector_fields)
                {
                    continue;
                }
                let same_provider = nodes[candidate].inputs.iter().any(|edge| {
                    matches!(edge.role, KernelOwnerEdgeRole::ReadProvider)
                        && edge.expression == selector_provider
                });
                if !same_provider {
                    continue;
                }
                let specificity = selector_fields.len();
                if narrowings
                    .get(&candidate)
                    .is_none_or(|(current, _)| specificity > *current)
                {
                    narrowings.insert(candidate, (specificity, selector));
                }
            }
        }
    }

    for (candidate, (_, selector)) in narrowings {
        let KernelOwnerNodeKind::ValueRead { mode_narrowing, .. } = &mut nodes[candidate].kind
        else {
            unreachable!("match mode narrowing must target a value read")
        };
        *mode_narrowing = Some(selector);
    }
}

fn local_dependency_cone(nodes: &[KernelOwnerNode], seeds: BTreeSet<usize>) -> BTreeSet<usize> {
    let mut dependents = seeds;
    loop {
        let added = nodes
            .iter()
            .enumerate()
            .filter(|(index, _)| !dependents.contains(index))
            .filter_map(|(index, node)| {
                node.inputs
                    .iter()
                    .any(|input| {
                        let input = input.expression.0 as usize;
                        input < nodes.len() && dependents.contains(&input)
                    })
                    .then_some(index)
            })
            .collect::<Vec<_>>();
        if added.is_empty() {
            return dependents;
        }
        dependents.extend(added);
    }
}

fn local_source_container_nodes(nodes: &[KernelOwnerNode]) -> BTreeSet<usize> {
    let mut providers = nodes
        .iter()
        .enumerate()
        .filter_map(|(index, node)| {
            matches!(node.kind, KernelOwnerNodeKind::Source(_)).then_some(index)
        })
        .collect::<BTreeSet<_>>();
    loop {
        let added = nodes
            .iter()
            .enumerate()
            .filter(|(index, node)| {
                !providers.contains(index)
                    && matches!(node.kind, KernelOwnerNodeKind::Record { .. })
            })
            .filter_map(|(index, node)| {
                node.inputs
                    .iter()
                    .any(|input| {
                        let input = input.expression.0 as usize;
                        input < nodes.len() && providers.contains(&input)
                    })
                    .then_some(index)
            })
            .collect::<Vec<_>>();
        if added.is_empty() {
            break;
        }
        providers.extend(added);
    }
    providers.retain(|index| matches!(nodes[*index].kind, KernelOwnerNodeKind::Record { .. }));
    providers
}

fn local_input_closure(nodes: &[KernelOwnerNode], seeds: BTreeSet<usize>) -> BTreeSet<usize> {
    let mut closure = seeds;
    let mut pending = closure.iter().copied().collect::<Vec<_>>();
    while let Some(index) = pending.pop() {
        for input in &nodes[index].inputs {
            let input = input.expression.0 as usize;
            if input < nodes.len() && closure.insert(input) {
                pending.push(input);
            }
        }
    }
    closure
}

fn prepared_input_reference_index(
    reference: PreparedInputReference,
    view: UnitOwnerSyntaxView<'_>,
    owner: &StableCheckOwnerKey,
    source_expression: Option<usize>,
    local_by_syntax: &BTreeMap<usize, usize>,
    node_count: usize,
    external_by_key: &mut BTreeMap<PreparedExternalExpression, usize>,
    external_expressions: &mut Vec<PreparedExternalExpression>,
) -> Result<usize, String> {
    let context = source_expression.map_or_else(
        || "synthetic result".to_owned(),
        |expression| format!("expression {expression}"),
    );
    let external = match reference {
        PreparedInputReference::Syntax(reference) => {
            if let Some(local) = local_by_syntax.get(&reference).copied() {
                return Ok(local);
            }
            let target_owner = view
                .stable_check_owner_for_syntax_expression(reference)
                .ok_or_else(|| {
                    format!(
                        "owner {owner:?} {context} references syntax expression {reference} with no owner"
                    )
                })?;
            if &target_owner == owner {
                return Err(format!(
                    "owner {owner:?} {context} lost local input {reference}"
                ));
            }
            let target_expression = view
                .stable_expression_key_for_syntax(reference)
                .ok_or_else(|| {
                    format!(
                        "owner {owner:?} {context} references syntax expression {reference} with no stable identity"
                    )
                })?;
            PreparedExternalExpression {
                owner: target_owner,
                target: PreparedExternalTarget::Expression(target_expression),
            }
        }
        PreparedInputReference::OwnerResult(target_owner) => {
            if &target_owner == owner {
                return Err(format!(
                    "owner {owner:?} {context} recursively imports its own public result"
                ));
            }
            PreparedExternalExpression {
                owner: target_owner,
                target: PreparedExternalTarget::Result,
            }
        }
    };
    let external_index = match external_by_key.get(&external).copied() {
        Some(external_index) => external_index,
        None => {
            let external_index = external_expressions.len();
            external_by_key.insert(external.clone(), external_index);
            external_expressions.push(external);
            external_index
        }
    };
    node_count
        .checked_add(external_index)
        .ok_or_else(|| "owner expression namespace overflowed".to_owned())
}

fn prepared_statement_lexical_target(
    view: UnitOwnerSyntaxView<'_>,
    owner: &StableCheckOwnerKey,
    statement: UnitLocalStatementId,
    dense_statement_by_syntax: &BTreeMap<usize, KernelStatementId>,
) -> Option<PreparedLexicalTarget> {
    let target_owner = view.stable_check_owner_for_local_statement(statement)?;
    if target_owner != *owner {
        return Some(PreparedLexicalTarget::OwnerPublic(target_owner));
    }
    let syntax = view.statement_for_local(statement)?;
    dense_statement_by_syntax
        .get(&syntax.id)
        .copied()
        .map(|statement| {
            PreparedLexicalTarget::Declaration(KernelDeclarationOrigin::Statement { statement })
        })
}

/// Resolve the declaration named by a HOLD update alias directly from the
/// parser-owned statement ancestry.
///
/// A field-bearing HOLD owns its statement declaration. A fieldless HOLD
/// shares the nearest enclosing Field/HOLD declaration; before another named
/// declaration boundary, the outermost fieldless HOLD owns the declaration.
/// Unlike the old owner DTO this bridge can inspect the complete finalized
/// syntax view, so the authority does not need to be copied into an
/// intermediate `containing_hold_authority` field.
fn prepared_hold_alias_lexical_target(
    view: UnitOwnerSyntaxView<'_>,
    owner: &StableCheckOwnerKey,
    statement: UnitLocalStatementId,
    dense_statement_by_syntax: &BTreeMap<usize, KernelStatementId>,
) -> Result<Option<PreparedLexicalTarget>, String> {
    let statement_input = view
        .statement_for_local(statement)
        .ok_or_else(|| "HOLD alias references a missing parser statement".to_owned())?;
    let AstStatementKind::Hold { field, name } = &statement_input.kind else {
        return Ok(None);
    };
    if name.is_none() {
        return Ok(None);
    }
    if field.is_some() {
        return Ok(prepared_statement_lexical_target(
            view,
            owner,
            statement,
            dense_statement_by_syntax,
        ));
    }

    let mut fieldless_hold = statement;
    let mut parent = view
        .statement_locator(statement)
        .and_then(|locator| locator.parent());
    let mut visited = BTreeSet::new();
    while let Some(parent_id) = parent {
        if !visited.insert(parent_id) {
            return Err("HOLD alias statement ancestry contains a cycle".to_owned());
        }
        let parent_input = view.statement_for_local(parent_id).ok_or_else(|| {
            "HOLD alias ancestry references a missing parser statement".to_owned()
        })?;
        match &parent_input.kind {
            AstStatementKind::Field { .. } | AstStatementKind::Hold { field: Some(_), .. } => {
                return Ok(prepared_statement_lexical_target(
                    view,
                    owner,
                    parent_id,
                    dense_statement_by_syntax,
                ));
            }
            AstStatementKind::Hold {
                field: None,
                name: Some(_),
            } => fieldless_hold = parent_id,
            AstStatementKind::Function { .. }
            | AstStatementKind::Source { field: Some(_), .. }
            | AstStatementKind::List { field: Some(_), .. } => {
                return Ok(prepared_statement_lexical_target(
                    view,
                    owner,
                    fieldless_hold,
                    dense_statement_by_syntax,
                ));
            }
            AstStatementKind::Source { field: None, .. }
            | AstStatementKind::Hold {
                field: None,
                name: None,
            }
            | AstStatementKind::List { field: None, .. }
            | AstStatementKind::Block
            | AstStatementKind::Spread
            | AstStatementKind::Expression => {}
        }
        parent = view
            .statement_locator(parent_id)
            .and_then(|locator| locator.parent());
    }

    Ok(prepared_statement_lexical_target(
        view,
        owner,
        fieldless_hold,
        dense_statement_by_syntax,
    ))
}

fn direct_statement_record_field_targets(
    view: UnitOwnerSyntaxView<'_>,
    owner: &StableCheckOwnerKey,
    raw_expressions: &[&boon_syntax::AstExpr],
    structured_records: &BTreeMap<usize, Vec<PreparedRecordEntry>>,
) -> BTreeMap<(usize, String), PreparedLexicalTarget> {
    let expressions = raw_expressions
        .iter()
        .map(|expression| (expression.id, *expression))
        .collect::<BTreeMap<_, _>>();
    let dense_statement_by_syntax = view
        .statement_ids()
        .iter()
        .copied()
        .zip(view.statements())
        .enumerate()
        .map(|(dense, (_, statement))| {
            (
                statement.id,
                KernelStatementId(
                    u32::try_from(dense).expect("definition statement count fits u32"),
                ),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let statement_by_placement = view
        .statement_ids()
        .iter()
        .copied()
        .filter_map(|statement| {
            let locator = view.statement_locator(statement)?;
            Some(((locator.parent(), locator.child_index()), statement))
        })
        .collect::<BTreeMap<_, _>>();
    let mut targets = BTreeMap::new();
    for (statement_id, statement) in view.statement_ids().iter().copied().zip(view.statements()) {
        if statement.children.is_empty()
            && !matches!(statement.kind, AstStatementKind::Function { .. })
        {
            continue;
        }
        let Some(direct) = statement.expr else {
            continue;
        };
        let Some(direct_expression) = expressions.get(&direct).copied() else {
            continue;
        };
        let container = match &direct_expression.kind {
            AstExprKind::Object(_) => Some(direct),
            AstExprKind::MatchArm {
                output: Some(output),
                ..
            }
            | AstExprKind::Then {
                output: Some(output),
                ..
            } if expressions.get(output).is_some_and(|output| {
                matches!(output.kind, AstExprKind::Object(_))
                    || structured_records.contains_key(&output.id)
            }) =>
            {
                Some(*output)
            }
            _ if structured_records.contains_key(&direct) => Some(direct),
            _ => None,
        };
        let Some(container) = container else {
            continue;
        };
        let fields = match expressions
            .get(&container)
            .map(|expression| &expression.kind)
        {
            Some(AstExprKind::Object(fields)) if !fields.is_empty() => fields
                .iter()
                .map(|field| {
                    (
                        field.name.as_str(),
                        field.spread,
                        PreparedInputReference::Syntax(field.value),
                    )
                })
                .collect::<Vec<_>>(),
            _ => structured_records
                .get(&container)
                .into_iter()
                .flatten()
                .map(|field| match field {
                    PreparedRecordEntry::Field { name, value } => {
                        (name.as_str(), false, value.clone())
                    }
                    PreparedRecordEntry::Spread { value } => ("", true, value.clone()),
                })
                .collect(),
        };
        for (field_name, spread, field_value) in fields {
            if spread {
                continue;
            }
            let statement_target = statement
                .children
                .iter()
                .enumerate()
                .find(|(_, child)| statement_binding_name(&child.kind) == Some(field_name))
                .and_then(|(child_index, _)| {
                    statement_by_placement
                        .get(&(Some(statement_id), child_index))
                        .copied()
                })
                .and_then(|child| {
                    prepared_statement_lexical_target(
                        view,
                        owner,
                        child,
                        &dense_statement_by_syntax,
                    )
                });
            let target = statement_target.or_else(|| match &field_value {
                PreparedInputReference::Syntax(value) => view
                    .stable_check_owner_for_syntax_expression(*value)
                    .filter(|field_owner| field_owner != owner)
                    .map(PreparedLexicalTarget::OwnerPublic),
                PreparedInputReference::OwnerResult(owner) => {
                    Some(PreparedLexicalTarget::OwnerPublic(owner.clone()))
                }
            });
            if let Some(target) = target {
                targets.insert((container, field_name.to_owned()), target);
            }
        }
    }
    targets
}

fn direct_lexical_binding_reads(
    view: UnitOwnerSyntaxView<'_>,
    owner: &StableCheckOwnerKey,
    raw_expressions: &[&boon_syntax::AstExpr],
    stable_expressions: &[StableExpressionKey],
    local_by_syntax: &BTreeMap<usize, usize>,
    output_bindings_by_scope: &PreparedOutputBindingsByScope,
    structured_records: &BTreeMap<usize, Vec<PreparedRecordEntry>>,
    statement_record_field_targets: &BTreeMap<(usize, String), PreparedLexicalTarget>,
) -> Result<BTreeMap<usize, PreparedLexicalBinding>, String> {
    let dense_statement_by_syntax = view
        .statement_ids()
        .iter()
        .copied()
        .zip(view.statements())
        .enumerate()
        .map(|(dense, (_, statement))| {
            (
                statement.id,
                KernelStatementId(
                    u32::try_from(dense).expect("definition statement count fits u32"),
                ),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let local_statement_by_syntax = view
        .statement_ids()
        .iter()
        .copied()
        .zip(view.statements())
        .map(|(local, statement)| (statement.id, local))
        .collect::<BTreeMap<_, _>>();
    let syntax_by_stable = stable_expressions
        .iter()
        .cloned()
        .zip(raw_expressions.iter().map(|expression| expression.id))
        .collect::<BTreeMap<_, _>>();
    let parent_by_syntax = raw_expressions
        .iter()
        .filter_map(|expression| {
            let (parent_owner, parent, _) =
                view.stable_expression_parent_edge_for_syntax(expression.id)?;
            (parent_owner == *owner)
                .then(|| syntax_by_stable.get(&parent).copied())
                .flatten()
                .map(|parent| (expression.id, parent))
        })
        .collect::<BTreeMap<_, _>>();
    let containing_statements =
        direct_containing_statements(view, raw_expressions, local_by_syntax);
    let statement_by_placement = view
        .statement_ids()
        .iter()
        .copied()
        .filter_map(|statement| {
            let locator = view.statement_locator(statement)?;
            Some(((locator.parent(), locator.child_index()), statement))
        })
        .collect::<BTreeMap<_, _>>();
    let mut reads = BTreeMap::new();
    for expression in raw_expressions {
        let root = match &expression.kind {
            AstExprKind::Identifier(name) => name.as_str(),
            AstExprKind::Path(path) => match path.first() {
                Some(root) => root,
                None => continue,
            },
            AstExprKind::Drain { path } => match path {
                boon_syntax::AstDrainPath::Binding { name }
                | boon_syntax::AstDrainPath::Field { binding: name, .. } => name,
                boon_syntax::AstDrainPath::Passed { .. } => "PASSED",
            },
            _ => continue,
        };
        let mut cursor = expression.id;
        let mut active = BTreeSet::new();
        while active.insert(cursor) {
            let Some(parent) = parent_by_syntax.get(&cursor).copied() else {
                break;
            };
            let Some(parent_expression) = local_by_syntax
                .get(&parent)
                .and_then(|index| raw_expressions.get(*index))
            else {
                break;
            };
            if let Some(provider) = structured_records
                .get(&parent_expression.id)
                .into_iter()
                .flatten()
                .find_map(|entry| match entry {
                    PreparedRecordEntry::Field { name, value }
                        if name == root && value != &PreparedInputReference::Syntax(cursor) =>
                    {
                        Some(value.clone())
                    }
                    PreparedRecordEntry::Field { .. } | PreparedRecordEntry::Spread { .. } => None,
                })
            {
                let target = statement_record_field_targets
                    .get(&(parent_expression.id, root.to_owned()))
                    .cloned()
                    .unwrap_or_else(|| PreparedLexicalTarget::Value(provider.clone()));
                reads.insert(
                    expression.id,
                    PreparedLexicalBinding {
                        provider: PreparedLexicalProvider::Input(provider.clone()),
                        target,
                        prefix: Box::new([]),
                        directional: false,
                        pattern: None,
                    },
                );
                break;
            }
            if let AstExprKind::Object(fields) | AstExprKind::TaggedObject { fields, .. } =
                &parent_expression.kind
                && let Some(provider) = fields
                    .iter()
                    .find(|field| !field.spread && field.name == root && field.value != cursor)
                    .map(|field| field.value)
            {
                let target = statement_record_field_targets
                    .get(&(parent_expression.id, root.to_owned()))
                    .cloned()
                    .unwrap_or_else(|| {
                        PreparedLexicalTarget::Declaration(KernelDeclarationOrigin::RecordField {
                            object: checked_kernel_expression(
                                *local_by_syntax
                                    .get(&parent_expression.id)
                                    .expect("local parent expression has a dense row"),
                            )
                            .expect("local parent expression index fits u32"),
                            ordinal: checked_u32(
                                fields
                                    .iter()
                                    .position(|field| {
                                        !field.spread
                                            && field.name == root
                                            && field.value == provider
                                    })
                                    .expect("matching record field has an ordinal"),
                                "record field ordinal",
                            )
                            .expect("record field ordinal fits u32"),
                        })
                    });
                reads.insert(
                    expression.id,
                    PreparedLexicalBinding {
                        provider: PreparedLexicalProvider::Input(PreparedInputReference::Syntax(
                            provider,
                        )),
                        target,
                        prefix: Box::new([]),
                        directional: false,
                        pattern: None,
                    },
                );
                break;
            }
            let call_surface = match &parent_expression.kind {
                AstExprKind::Call { function, args, .. } => Some((function.as_str(), args)),
                AstExprKind::Pipe { op, args, .. } => Some((op.as_str(), args)),
                _ => None,
            };
            if let Some((function, arguments)) = call_surface
                && let Some(context) = render_call_context_surface(function)
                && root == context.name
                && let Some(provider) = arguments
                    .iter()
                    .find(|argument| argument.named_name() == Some(context.provider))
                    .map(|argument| argument.value)
                && provider != cursor
            {
                reads.insert(
                    expression.id,
                    PreparedLexicalBinding {
                        provider: PreparedLexicalProvider::Known(context.flow_type),
                        target: PreparedLexicalTarget::RuntimeContext,
                        prefix: Box::new([]),
                        directional: false,
                        pattern: None,
                    },
                );
                break;
            }
            if let AstExprKind::Block { bindings, .. } = &parent_expression.kind
                && let Some(binding) = bindings.iter().find(|binding| binding.name == root)
            {
                let target = match binding.declaration {
                    AstBlockBindingDeclaration::Local { statement } => local_statement_by_syntax
                        .get(&statement)
                        .copied()
                        .and_then(|statement| {
                            prepared_statement_lexical_target(
                                view,
                                owner,
                                statement,
                                &dense_statement_by_syntax,
                            )
                        }),
                    AstBlockBindingDeclaration::Child { child } => view
                        .child_owners()
                        .get(child)
                        .and_then(|child| {
                            view.stable_check_owner_for_local_statement(child.statement())
                        })
                        .map(PreparedLexicalTarget::OwnerPublic),
                }
                .unwrap_or_else(|| {
                    PreparedLexicalTarget::Value(PreparedInputReference::Syntax(binding.value))
                });
                reads.insert(
                    expression.id,
                    PreparedLexicalBinding {
                        provider: PreparedLexicalProvider::Input(PreparedInputReference::Syntax(
                            binding.value,
                        )),
                        target,
                        prefix: Box::new([]),
                        directional: false,
                        pattern: None,
                    },
                );
                break;
            }
            if let AstExprKind::MatchArm { pattern, .. } = &parent_expression.kind
                && let Some(prefix) = match_pattern_binding_prefix(pattern, root)
                && let Some(provider) =
                    view.pattern_selector_for_syntax_expression(parent_expression.id)
            {
                reads.insert(
                    expression.id,
                    PreparedLexicalBinding {
                        provider: PreparedLexicalProvider::Input(PreparedInputReference::Syntax(
                            provider,
                        )),
                        target: PreparedLexicalTarget::Declaration(
                            KernelDeclarationOrigin::PatternBinding {
                                arm: checked_kernel_expression(
                                    *local_by_syntax
                                        .get(&parent_expression.id)
                                        .expect("local match arm has a dense row"),
                                )
                                .expect("local match arm index fits u32"),
                                ordinal: checked_u32(
                                    pattern_variable_names(pattern)
                                        .iter()
                                        .position(|name| name == root)
                                        .expect("matching pattern binding has an ordinal"),
                                    "pattern binding ordinal",
                                )
                                .expect("pattern binding ordinal fits u32"),
                            },
                        ),
                        prefix: prefix.into_boxed_slice(),
                        directional: true,
                        pattern: Some(compact_pattern(pattern)),
                    },
                );
                break;
            }
            if let Some(binding) = output_bindings_by_scope
                .get(&parent_expression.id)
                .into_iter()
                .flatten()
                .find(|binding| {
                    binding.name == root
                        && binding.provider != expression.id
                        && binding.active_inputs.contains(&cursor)
                })
            {
                reads.insert(
                    expression.id,
                    PreparedLexicalBinding {
                        provider: PreparedLexicalProvider::Input(PreparedInputReference::Syntax(
                            binding.provider,
                        )),
                        target: PreparedLexicalTarget::Declaration(
                            KernelDeclarationOrigin::CallbackBinding {
                                call: checked_kernel_expression(
                                    *local_by_syntax
                                        .get(&parent_expression.id)
                                        .expect("local callback expression has a dense row"),
                                )
                                .expect("local callback expression index fits u32"),
                                ordinal: binding.formal_ordinal,
                            },
                        ),
                        prefix: Box::new([]),
                        directional: true,
                        pattern: None,
                    },
                );
                break;
            }
            cursor = parent;
        }
        if reads.contains_key(&expression.id) {
            continue;
        }
        // Multiline record fields are sibling statements rather than child
        // expression edges of the delimiter. Follow the parser-owned
        // statement containment chain to the nearest enclosing structured
        // record instead of rebuilding a lexical scope graph.
        let Some(statement) = containing_statements.get(&expression.id).copied() else {
            continue;
        };
        let mut direct_child = statement;
        while let Some(locator) = view.statement_locator(direct_child) {
            let Some(parent) = locator.parent() else {
                break;
            };
            // A HOLD name is a private read capability available to authored
            // update statements, not to the initializer expression. Update
            // bodies are statement children of the HOLD, so statement
            // containment distinguishes the two without guessing from names
            // or source spans.
            if let Some(parent_statement) = view.statement_for_local(parent)
                && let Some(arm_expression) = parent_statement.expr
                && let Some(&arm_index) = local_by_syntax.get(&arm_expression)
                && let AstExprKind::MatchArm { pattern, .. } = &raw_expressions[arm_index].kind
                && let Some(prefix) = match_pattern_binding_prefix(pattern, root)
                && let Some(provider) = view.pattern_selector_for_syntax_expression(arm_expression)
            {
                reads.insert(
                    expression.id,
                    PreparedLexicalBinding {
                        provider: PreparedLexicalProvider::Input(PreparedInputReference::Syntax(
                            provider,
                        )),
                        target: PreparedLexicalTarget::Declaration(
                            KernelDeclarationOrigin::PatternBinding {
                                arm: checked_kernel_expression(arm_index)
                                    .expect("local match arm index fits u32"),
                                ordinal: checked_u32(
                                    pattern_variable_names(pattern)
                                        .iter()
                                        .position(|name| name == root)
                                        .expect("matching pattern binding has an ordinal"),
                                    "pattern binding ordinal",
                                )
                                .expect("pattern binding ordinal fits u32"),
                            },
                        ),
                        prefix: prefix.into_boxed_slice(),
                        directional: true,
                        pattern: Some(compact_pattern(pattern)),
                    },
                );
                break;
            }
            if let Some(parent_statement) = view.statement_for_local(parent)
                && matches!(&parent_statement.kind, AstStatementKind::Hold { name: Some(name), .. } if name == root)
                && let Some(provider) = parent_statement.expr
            {
                let target = prepared_hold_alias_lexical_target(
                    view,
                    owner,
                    parent,
                    &dense_statement_by_syntax,
                )?
                .unwrap_or_else(|| {
                    PreparedLexicalTarget::Value(PreparedInputReference::Syntax(provider))
                });
                reads.insert(
                    expression.id,
                    PreparedLexicalBinding {
                        provider: PreparedLexicalProvider::Input(PreparedInputReference::Syntax(
                            provider,
                        )),
                        target,
                        prefix: Box::new([]),
                        directional: true,
                        pattern: None,
                    },
                );
                break;
            }
            if let Some(parent_statement) = view.statement_for_local(parent)
                && let Some(container) = parent_statement.expr
                && parent_statement
                    .children
                    .get(locator.child_index())
                    .and_then(|child| statement_binding_name(&child.kind))
                    != Some(root)
                && let Some(provider) = structured_records
                    .get(&container)
                    .into_iter()
                    .flatten()
                    .find_map(|entry| match entry {
                        PreparedRecordEntry::Field { name, value } if name == root => {
                            Some(value.clone())
                        }
                        PreparedRecordEntry::Field { .. } | PreparedRecordEntry::Spread { .. } => {
                            None
                        }
                    })
                    .or_else(|| {
                        let expression = local_by_syntax
                            .get(&container)
                            .and_then(|index| raw_expressions.get(*index))?;
                        let fields = match &expression.kind {
                            AstExprKind::Object(fields)
                            | AstExprKind::TaggedObject { fields, .. } => fields,
                            _ => return None,
                        };
                        fields
                            .iter()
                            .find(|field| !field.spread && field.name == root)
                            .map(|field| PreparedInputReference::Syntax(field.value))
                    })
            {
                let target = statement_record_field_targets
                    .get(&(container, root.to_owned()))
                    .cloned()
                    .unwrap_or_else(|| PreparedLexicalTarget::Value(provider.clone()));
                reads.insert(
                    expression.id,
                    PreparedLexicalBinding {
                        provider: PreparedLexicalProvider::Input(provider),
                        target,
                        prefix: Box::new([]),
                        directional: false,
                        pattern: None,
                    },
                );
                break;
            }
            let binding = view
                .statement_for_local(parent)
                .and_then(|parent_statement| {
                    parent_statement.children[..locator.child_index()]
                        .iter()
                        .enumerate()
                        .rev()
                        .find_map(|(child_index, child)| {
                            if statement_binding_name(&child.kind) != Some(root) {
                                return None;
                            }
                            let statement = statement_by_placement
                                .get(&(Some(parent), child_index))
                                .copied()?;
                            let provider = view
                                .statement_value_expression(statement)
                                .or(child.expr)
                                .map(PreparedInputReference::Syntax)?;
                            let target = prepared_statement_lexical_target(
                                view,
                                owner,
                                statement,
                                &dense_statement_by_syntax,
                            )
                            .unwrap_or_else(|| PreparedLexicalTarget::Value(provider.clone()));
                            Some((provider, target))
                        })
                });
            if let Some((provider, target)) = binding {
                reads.insert(
                    expression.id,
                    PreparedLexicalBinding {
                        provider: PreparedLexicalProvider::Input(provider.clone()),
                        target,
                        prefix: Box::new([]),
                        directional: false,
                        pattern: None,
                    },
                );
                break;
            }
            direct_child = parent;
        }
        if !reads.contains_key(&expression.id)
            && std::env::var_os("BOON_KERNEL_ORACLE_TRACE_OWNER").is_some_and(|pattern| {
                format!("{owner:?}").contains(pattern.to_string_lossy().as_ref())
            })
        {
            let mut statement_chain = Vec::new();
            let mut statement = containing_statements.get(&expression.id).copied();
            while let Some(current) = statement {
                statement_chain.push((
                    current,
                    view.statement_for_local(current)
                        .map(|statement| (format!("{:?}", statement.kind), statement.expr)),
                ));
                statement = view
                    .statement_locator(current)
                    .and_then(|locator| locator.parent());
            }
            eprintln!(
                "kernel-owner-trace unresolved-local expression={} root={root} expression_parents={:?} statement_chain={statement_chain:?} structured_records={structured_records:?}",
                expression.id,
                parent_by_syntax.get(&expression.id),
            );
        }
    }
    Ok(reads)
}

fn statement_binding_name(kind: &AstStatementKind) -> Option<&str> {
    match kind {
        AstStatementKind::Field { name } | AstStatementKind::Function { name, .. } => Some(name),
        AstStatementKind::Source {
            field: Some(name), ..
        }
        | AstStatementKind::Hold {
            field: Some(name), ..
        }
        | AstStatementKind::List {
            field: Some(name), ..
        } => Some(name),
        AstStatementKind::Source { field: None, .. }
        | AstStatementKind::Hold { field: None, .. }
        | AstStatementKind::List { field: None, .. }
        | AstStatementKind::Block
        | AstStatementKind::Spread
        | AstStatementKind::Expression => None,
    }
}

fn compact_statement_facts(
    view: UnitOwnerSyntaxView<'_>,
    owner: &StableCheckOwnerKey,
    callable_surface: Option<&CallableSurface>,
    local_by_syntax: &BTreeMap<usize, usize>,
    node_count: usize,
    external_by_key: &mut BTreeMap<PreparedExternalExpression, usize>,
    external_expressions: &mut Vec<PreparedExternalExpression>,
) -> Result<
    (
        Box<[StableStatementKey]>,
        KernelDefinitionFactsInput,
        Box<[PreparedStatementChildTarget]>,
    ),
    String,
> {
    let statements = view
        .statement_ids()
        .iter()
        .copied()
        .zip(view.statements())
        .collect::<Vec<_>>();
    let dense_by_syntax = statements
        .iter()
        .enumerate()
        .map(|(index, (_, statement))| (statement.id, index))
        .collect::<BTreeMap<_, _>>();
    let child_owner_by_syntax = view
        .child_owners()
        .iter()
        .filter_map(|boundary| {
            Some((
                view.statement_for_local(boundary.statement())?.id,
                view.stable_check_owner_for_local_statement(boundary.statement())?,
            ))
        })
        .collect::<BTreeMap<_, _>>();
    let stable = statements
        .iter()
        .map(|(statement, _)| {
            view.stable_statement_key_local(*statement)
                .ok_or_else(|| format!("owner statement {:?} has no stable key", statement))
        })
        .collect::<Result<Vec<_>, _>>()?
        .into_boxed_slice();
    let mut child_targets = Vec::new();
    let statements = statements
        .into_iter()
        .enumerate()
        .map(|(index, (statement_id, statement))| {
            let kind = match &statement.kind {
                AstStatementKind::Function { name, parameters } => KernelStatementKind::Function {
                    name: name.clone().into_boxed_str(),
                    parameters: parameters
                        .iter()
                        .map(|parameter| {
                            Ok(KernelStatementParameter {
                                name: parameter.name.clone().into_boxed_str(),
                                kind: match parameter.kind {
                                    AstParameterKind::Value => KernelParameterKind::Value,
                                    AstParameterKind::Out => KernelParameterKind::Out,
                                },
                                ordinal: checked_u32(
                                    parameter.ordinal,
                                    "statement parameter ordinal",
                                )?,
                                evaluation_scope: callable_surface
                                    .and_then(|surface| {
                                        surface.parameters.iter().find(|candidate| {
                                            candidate.ordinal == parameter.ordinal
                                        })
                                    })
                                    .map(|parameter| parameter.evaluation_scope)
                                    .unwrap_or(KernelParameterEvaluationScope::Parent),
                            })
                        })
                        .collect::<Result<Vec<_>, String>>()?
                        .into_boxed_slice(),
                },
                AstStatementKind::Field { name } => KernelStatementKind::Field {
                    name: name.clone().into_boxed_str(),
                },
                AstStatementKind::Source { field, event } => KernelStatementKind::Source {
                    field: field.clone().map(String::into_boxed_str),
                    event: event.clone().map(String::into_boxed_str),
                },
                AstStatementKind::Hold { field, name } => KernelStatementKind::Hold {
                    field: field.clone().map(String::into_boxed_str),
                    name: name.clone().map(String::into_boxed_str),
                },
                AstStatementKind::List { field, capacity } => KernelStatementKind::List {
                    field: field.clone().map(String::into_boxed_str),
                    capacity: *capacity,
                },
                AstStatementKind::Block => KernelStatementKind::Block,
                AstStatementKind::Spread => KernelStatementKind::Spread,
                AstStatementKind::Expression => KernelStatementKind::Expression,
            };
            let value = view
                .checked_statement_value_expression(statement_id)
                .map(|value| {
                    prepared_input_reference_index(
                        PreparedInputReference::Syntax(value),
                        view,
                        owner,
                        None,
                        local_by_syntax,
                        node_count,
                        external_by_key,
                        external_expressions,
                    )
                    .and_then(checked_kernel_expression)
                })
                .transpose()?;
            let children = statement
                .children
                .iter()
                .enumerate()
                .map(|(child_index, child)| {
                    if let Some(child) = dense_by_syntax.get(&child.id).copied() {
                        return checked_u32(child, "statement child index")
                            .map(KernelStatementId)
                            .map(KernelStatementChildReference::Local);
                    }
                    let child_owner = child_owner_by_syntax.get(&child.id).cloned().ok_or_else(|| {
                        format!(
                            "owner statement {} child {} is neither local nor an owner boundary",
                            statement.id, child.id
                        )
                    })?;
                    child_targets.push(PreparedStatementChildTarget {
                        statement: index,
                        child: child_index,
                        owner: child_owner,
                    });
                    Ok(KernelStatementChildReference::Owner(KernelOwnerId(0)))
                })
                .collect::<Result<Vec<_>, _>>()?
                .into_boxed_slice();
            Ok(KernelStatementInput {
                id: KernelStatementId(checked_u32(index, "statement index")?),
                kind,
                value,
                children,
            })
        })
        .collect::<Result<Vec<_>, String>>()?
        .into_boxed_slice();
    Ok((
        stable,
        KernelDefinitionFactsInput {
            statements,
            ..KernelDefinitionFactsInput::default()
        },
        child_targets.into_boxed_slice(),
    ))
}

fn kernel_expression_span(expression: &AstExpr) -> KernelSourceSpan {
    KernelSourceSpan {
        line: expression.line,
        start: expression.start,
        end: expression.end,
    }
}

fn kernel_statement_span(statement: &AstStatement) -> KernelSourceSpan {
    KernelSourceSpan {
        line: statement.line,
        start: statement.start,
        end: statement.end,
    }
}

fn kernel_subspan(
    view: UnitOwnerSyntaxView<'_>,
    fallback_line: usize,
    start: usize,
    end: usize,
) -> KernelSourceSpan {
    KernelSourceSpan {
        line: view.physical_line_for_byte(start).unwrap_or(fallback_line),
        start,
        end,
    }
}

fn kernel_syntax_expression_children(expression: &AstExpr) -> Vec<usize> {
    let mut children = Vec::new();
    if let Some(linked) = expression.linked_input {
        children.push(linked);
    }
    match &expression.kind {
        AstExprKind::TextTemplate { segments } => {
            children.extend(segments.iter().filter_map(|segment| match segment {
                AstTextSegment::Static { .. } => None,
                AstTextSegment::Dynamic { value } => Some(*value),
            }))
        }
        AstExprKind::TaggedObject { fields, .. } | AstExprKind::Object(fields) => {
            children.extend(fields.iter().map(|field| field.value));
        }
        AstExprKind::Flush { payload } => children.extend(*payload),
        AstExprKind::Call { args, pass, .. } => {
            children.extend(args.iter().map(|argument| argument.value));
            children.extend(pass.iter().map(|pass| pass.value));
        }
        AstExprKind::Pipe {
            input,
            args,
            pass,
            arms,
            ..
        } => {
            children.push(*input);
            children.extend(args.iter().map(|argument| argument.value));
            children.extend(pass.iter().map(|pass| pass.value));
            children.extend(arms.iter().copied());
        }
        AstExprKind::Draining { input } => children.push(*input),
        AstExprKind::Hold { initial, .. } => children.push(*initial),
        AstExprKind::Latest { branches } => children.extend(branches.iter().copied()),
        AstExprKind::When { input, arms } => {
            children.push(*input);
            children.extend(arms.iter().copied());
        }
        AstExprKind::Then { input, output } => {
            children.push(*input);
            children.extend(*output);
        }
        AstExprKind::Infix { left, right, .. } => {
            children.extend([*left, *right]);
        }
        AstExprKind::MatchArm { output, .. } => children.extend(*output),
        AstExprKind::Block { bindings, result } => {
            children.extend(bindings.iter().map(|binding| binding.value));
            children.extend(*result);
        }
        AstExprKind::ListLiteral { items, .. }
        | AstExprKind::BytesLiteral { items, .. }
        | AstExprKind::SetLiteral { items } => children.extend(items.iter().copied()),
        AstExprKind::Arrow { left, output, .. } => {
            children.push(*left);
            children.extend(*output);
        }
        AstExprKind::MapEntry { key, value } => children.extend([*key, *value]),
        AstExprKind::MapLiteral { entries } => children.extend(entries.iter().copied()),
        AstExprKind::Identifier(_)
        | AstExprKind::Path(_)
        | AstExprKind::Drain { .. }
        | AstExprKind::StringLiteral(_)
        | AstExprKind::TextLiteral(_)
        | AstExprKind::Number(_)
        | AstExprKind::ByteLiteral { .. }
        | AstExprKind::Tag(_)
        | AstExprKind::Source
        | AstExprKind::Delimiter
        | AstExprKind::Unknown(_)
        | AstExprKind::BitsLiteral { .. } => {}
    }
    children.sort_unstable();
    children.dedup();
    children
}

fn compact_checked_presentation(
    view: UnitOwnerSyntaxView<'_>,
    raw_expressions: &[&AstExpr],
    local_by_syntax: &BTreeMap<usize, usize>,
    nodes: &[KernelOwnerNode],
    facts: &KernelDefinitionFactsInput,
) -> Result<
    (
        KernelDefinitionPresentation,
        Box<[PreparedContainingScopeTarget]>,
    ),
    String,
> {
    let raw_statements = view.statements().collect::<Vec<_>>();
    if raw_statements.len() != facts.statements.len()
        || raw_expressions.len() > nodes.len()
        || facts
            .declarations
            .iter()
            .enumerate()
            .any(|(index, row)| row.id.0 as usize != index)
    {
        return Err("checked presentation input is not dense".to_owned());
    }
    let dense_statement_by_local = view
        .statement_ids()
        .iter()
        .copied()
        .enumerate()
        .map(|(dense, local)| (local, dense))
        .collect::<BTreeMap<_, _>>();
    let statement_parents = view
        .statement_ids()
        .iter()
        .map(|statement| {
            view.statement_locator(*statement)
                .and_then(|locator| locator.parent())
                .and_then(|parent| dense_statement_by_local.get(&parent).copied())
        })
        .collect::<Vec<_>>();
    let declaration_by_statement = facts
        .declarations
        .iter()
        .filter_map(|declaration| match declaration.origin {
            KernelDeclarationOrigin::Statement { statement } => {
                Some((statement.0 as usize, declaration.id))
            }
            _ => None,
        })
        .collect::<BTreeMap<_, _>>();
    let declaration_by_record_field = facts
        .declarations
        .iter()
        .filter_map(|declaration| match declaration.origin {
            KernelDeclarationOrigin::RecordField { object, ordinal } => {
                Some(((object.0 as usize, ordinal as usize), declaration.id))
            }
            _ => None,
        })
        .collect::<BTreeMap<_, _>>();

    let mut scopes = Vec::<KernelScopePresentation>::new();
    let mut body_scopes = vec![None; facts.statements.len()];
    for (index, (statement, syntax)) in facts
        .statements
        .iter()
        .zip(raw_statements.iter())
        .enumerate()
    {
        if !matches!(statement.kind, KernelStatementKind::Function { .. })
            && statement.children.is_empty()
        {
            continue;
        }
        let id = KernelScopeId(checked_u32(scopes.len(), "checked presentation scope")?);
        body_scopes[index] = Some(id);
        let direct = syntax
            .expr
            .and_then(|expression| local_by_syntax.get(&expression).copied());
        let kind = if matches!(statement.kind, KernelStatementKind::Function { .. }) {
            KernelScopeKind::Function
        } else if direct.is_some_and(|expression| {
            matches!(nodes[expression].kind, KernelOwnerNodeKind::Record { .. })
        }) {
            KernelScopeKind::Record
        } else {
            KernelScopeKind::Block
        };
        scopes.push(KernelScopePresentation {
            id,
            parent: KernelScopeReference::Containing,
            owner: declaration_by_statement
                .get(&index)
                .copied()
                .map(|declaration| KernelDeclarationReference::Local(declaration)),
            kind,
            origin: KernelScopeOrigin::StatementBody {
                statement: KernelStatementId(checked_u32(index, "statement scope owner")?),
            },
            span: kernel_statement_span(syntax),
        });
    }

    fn statement_scope(
        index: usize,
        parents: &[Option<usize>],
        bodies: &[Option<KernelScopeId>],
        cache: &mut [Option<KernelScopeReference>],
        active: &mut BTreeSet<usize>,
    ) -> Result<KernelScopeReference, String> {
        if let Some(scope) = cache[index] {
            return Ok(scope);
        }
        if !active.insert(index) {
            return Err("checked presentation statement parents contain a cycle".to_owned());
        }
        let scope = if let Some(parent) = parents[index] {
            bodies[parent].map_or(
                statement_scope(parent, parents, bodies, cache, active)?,
                KernelScopeReference::Local,
            )
        } else {
            KernelScopeReference::Containing
        };
        active.remove(&index);
        cache[index] = Some(scope);
        Ok(scope)
    }

    let mut statement_scopes = vec![None; facts.statements.len()];
    for index in 0..facts.statements.len() {
        let scope = statement_scope(
            index,
            &statement_parents,
            &body_scopes,
            &mut statement_scopes,
            &mut BTreeSet::new(),
        )?;
        if let Some(body) = body_scopes[index] {
            scopes[body.0 as usize].parent = scope;
        }
    }

    let mut expression_boundaries = BTreeMap::<usize, KernelScopeId>::new();
    for (statement, syntax) in facts.statements.iter().zip(raw_statements.iter()) {
        let Some(body) = body_scopes[statement.id.0 as usize] else {
            continue;
        };
        if let Some(expression) = syntax
            .expr
            .and_then(|expression| local_by_syntax.get(&expression).copied())
            && matches!(nodes[expression].kind, KernelOwnerNodeKind::Record { .. })
        {
            expression_boundaries.insert(expression, body);
        }
    }
    for (expression, node) in nodes.iter().enumerate().take(raw_expressions.len()) {
        let (kind, origin) = match &node.kind {
            KernelOwnerNodeKind::Record { .. }
                if !expression_boundaries.contains_key(&expression) =>
            {
                (
                    KernelScopeKind::Record,
                    KernelScopeOrigin::Record {
                        expression: KernelExpressionId(checked_u32(
                            expression,
                            "record scope expression",
                        )?),
                    },
                )
            }
            KernelOwnerNodeKind::MatchArm { .. } => (
                KernelScopeKind::Block,
                KernelScopeOrigin::MatchArm {
                    expression: KernelExpressionId(checked_u32(
                        expression,
                        "match scope expression",
                    )?),
                },
            ),
            _ => continue,
        };
        let id = KernelScopeId(checked_u32(scopes.len(), "expression scope")?);
        scopes.push(KernelScopePresentation {
            id,
            parent: KernelScopeReference::Containing,
            owner: None,
            kind,
            origin,
            span: kernel_expression_span(raw_expressions[expression]),
        });
        expression_boundaries.insert(expression, id);
    }

    let root_statement = facts
        .linkage
        .root_statement
        .map(|statement| statement.0 as usize);
    if let Some(root) = root_statement
        && let KernelStatementKind::Function { parameters, .. } = &facts.statements[root].kind
        && let AstStatementKind::Function {
            parameters: syntax_parameters,
            ..
        } = &raw_statements[root].kind
    {
        let function_body = body_scopes[root]
            .ok_or_else(|| "function checked presentation has no body scope".to_owned())?;
        if parameters.len() != syntax_parameters.len() {
            return Err("function presentation parameter tables differ".to_owned());
        }
        for (parameter, syntax_parameter) in parameters.iter().zip(syntax_parameters) {
            if parameter.kind != KernelParameterKind::Out {
                continue;
            }
            let declaration = facts
                .declarations
                .iter()
                .find(|declaration| {
                    declaration.origin
                        == (KernelDeclarationOrigin::Parameter {
                            statement: KernelStatementId(
                                checked_u32(root, "function root statement")
                                    .expect("root statement already fits u32"),
                            ),
                            ordinal: parameter.ordinal,
                        })
                })
                .map(|declaration| declaration.id)
                .ok_or_else(|| "OUT scope has no declaration".to_owned())?;
            let id = KernelScopeId(checked_u32(scopes.len(), "OUT scope")?);
            scopes.push(KernelScopePresentation {
                id,
                parent: KernelScopeReference::Local(function_body),
                owner: Some(KernelDeclarationReference::Local(declaration)),
                kind: KernelScopeKind::RepeatedOutput,
                origin: KernelScopeOrigin::RepeatedOutput {
                    statement: KernelStatementId(checked_u32(root, "OUT statement")?),
                    parameter_ordinal: parameter.ordinal,
                },
                span: kernel_subspan(
                    view,
                    raw_statements[root].line,
                    syntax_parameter.start,
                    syntax_parameter.end,
                ),
            });
        }
    }

    let repeated_output_by_parameter = scopes
        .iter()
        .filter_map(|scope| match scope.origin {
            KernelScopeOrigin::RepeatedOutput {
                parameter_ordinal, ..
            } => Some((parameter_ordinal, scope.id)),
            _ => None,
        })
        .collect::<BTreeMap<_, _>>();
    let mut presentations = facts
        .relocations
        .expressions
        .iter()
        .enumerate()
        .map(|(index, relocation)| {
            let span = match relocation {
                KernelExpressionRelocation::Authored(_) => raw_expressions
                    .get(index)
                    .map(|expression| kernel_expression_span(expression))
                    .ok_or_else(|| {
                        "authored checked presentation expression has no syntax row".to_owned()
                    })?,
                KernelExpressionRelocation::SyntheticDefinitionResult => root_statement
                    .and_then(|root| raw_statements.get(root).copied())
                    .map(kernel_statement_span)
                    .ok_or_else(|| {
                        "synthetic checked presentation result has no root statement".to_owned()
                    })?,
            };
            Ok(KernelExpressionPresentation {
                expression: KernelExpressionId(checked_u32(index, "presentation expression")?),
                scope: KernelScopeReference::Containing,
                declaration: None,
                span,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let mut assigned = vec![false; presentations.len()];

    fn assign_expression_tree(
        expression: usize,
        inherited_scope: KernelScopeReference,
        declaration: Option<KernelDeclarationReference>,
        force: bool,
        raw_expressions: &[&AstExpr],
        local_by_syntax: &BTreeMap<usize, usize>,
        boundaries: &BTreeMap<usize, KernelScopeId>,
        declaration_by_record_field: &BTreeMap<(usize, usize), KernelDeclarationId>,
        scopes: &mut [KernelScopePresentation],
        presentations: &mut [KernelExpressionPresentation],
        assigned: &mut [bool],
        active: &mut BTreeSet<usize>,
    ) -> Result<(), String> {
        if expression >= raw_expressions.len() || expression >= presentations.len() {
            return Ok(());
        }
        if assigned[expression] && !force {
            return Ok(());
        }
        if !active.insert(expression) {
            return Ok(());
        }
        let scope = if let Some(boundary) = boundaries.get(&expression).copied() {
            let row = scopes
                .get_mut(boundary.0 as usize)
                .ok_or_else(|| "expression boundary references missing scope".to_owned())?;
            if row.parent == KernelScopeReference::Containing || force {
                row.parent = inherited_scope;
            } else if row.parent != inherited_scope {
                return Err(format!(
                    "expression scope has conflicting lexical parents: expression={expression} boundary={boundary:?} existing={:?} inherited={inherited_scope:?}",
                    row.parent,
                ));
            }
            KernelScopeReference::Local(boundary)
        } else {
            inherited_scope
        };
        presentations[expression].scope = scope;
        if declaration.is_some() || force {
            presentations[expression].declaration = declaration;
        }
        assigned[expression] = true;
        let syntax = raw_expressions[expression];
        let record_fields = match &syntax.kind {
            AstExprKind::Object(fields) | AstExprKind::TaggedObject { fields, .. } => Some(fields),
            _ => None,
        };
        if let Some(fields) = record_fields {
            for (ordinal, field) in fields.iter().enumerate() {
                let child = local_by_syntax.get(&field.value).copied();
                let child_declaration = if field.spread {
                    declaration
                } else {
                    declaration_by_record_field
                        .get(&(expression, ordinal))
                        .copied()
                        .map(KernelDeclarationReference::Local)
                        .or(declaration)
                };
                if let Some(child) = child {
                    assign_expression_tree(
                        child,
                        scope,
                        child_declaration,
                        true,
                        raw_expressions,
                        local_by_syntax,
                        boundaries,
                        declaration_by_record_field,
                        scopes,
                        presentations,
                        assigned,
                        active,
                    )?;
                }
            }
        } else {
            for child in kernel_syntax_expression_children(syntax) {
                if let Some(child) = local_by_syntax.get(&child).copied() {
                    assign_expression_tree(
                        child,
                        scope,
                        declaration,
                        force,
                        raw_expressions,
                        local_by_syntax,
                        boundaries,
                        declaration_by_record_field,
                        scopes,
                        presentations,
                        assigned,
                        active,
                    )?;
                }
            }
        }
        active.remove(&expression);
        Ok(())
    }

    let mut inherited_declarations = vec![None; facts.statements.len()];
    for index in 0..facts.statements.len() {
        inherited_declarations[index] = declaration_by_statement
            .get(&index)
            .copied()
            .map(KernelDeclarationReference::Local)
            .or_else(|| statement_parents[index].and_then(|parent| inherited_declarations[parent]))
            .or_else(|| {
                facts
                    .linkage
                    .public_declaration
                    .filter(|_| statement_parents[index].is_none())
            });
        if let Some(expression) = raw_statements[index]
            .expr
            .and_then(|expression| local_by_syntax.get(&expression).copied())
        {
            assign_expression_tree(
                expression,
                statement_scopes[index].expect("statement scope was assigned"),
                inherited_declarations[index],
                true,
                raw_expressions,
                local_by_syntax,
                &expression_boundaries,
                &declaration_by_record_field,
                &mut scopes,
                &mut presentations,
                &mut assigned,
                &mut BTreeSet::new(),
            )?;
        }
    }
    for expression in 0..raw_expressions.len() {
        if !assigned[expression] {
            assign_expression_tree(
                expression,
                KernelScopeReference::Containing,
                facts.linkage.public_declaration,
                false,
                raw_expressions,
                local_by_syntax,
                &expression_boundaries,
                &declaration_by_record_field,
                &mut scopes,
                &mut presentations,
                &mut assigned,
                &mut BTreeSet::new(),
            )?;
        }
    }
    if let Some(result) = facts.linkage.result_expression
        && result.0 as usize >= raw_expressions.len()
        && let Some(root) = root_statement
    {
        presentations[result.0 as usize].scope =
            statement_scopes[root].expect("root statement scope was assigned");
        presentations[result.0 as usize].declaration = inherited_declarations[root];
    }

    let statement_presentations = facts
        .statements
        .iter()
        .zip(raw_statements.iter())
        .map(|(statement, syntax)| KernelStatementPresentation {
            statement: statement.id,
            scope: statement_scopes[statement.id.0 as usize].expect("statement scope was assigned"),
            body_scope: body_scopes[statement.id.0 as usize],
            span: kernel_statement_span(syntax),
        })
        .collect::<Vec<_>>()
        .into_boxed_slice();
    let declaration_presentations = facts
        .declarations
        .iter()
        .map(|declaration| {
            let (scope, body_scope, span) = match declaration.origin {
                KernelDeclarationOrigin::Statement { statement } => {
                    let index = statement.0 as usize;
                    (
                        statement_scopes[index].expect("statement scope was assigned"),
                        body_scopes[index],
                        kernel_statement_span(raw_statements[index]),
                    )
                }
                KernelDeclarationOrigin::Parameter { statement, ordinal } => {
                    let index = statement.0 as usize;
                    let AstStatementKind::Function { parameters, .. } = &raw_statements[index].kind
                    else {
                        return Err("parameter presentation belongs to a non-function".to_owned());
                    };
                    let parameter = parameters.get(ordinal as usize).ok_or_else(|| {
                        "parameter presentation references missing ordinal".to_owned()
                    })?;
                    (
                        body_scopes[index]
                            .map(KernelScopeReference::Local)
                            .ok_or_else(|| {
                                "parameter presentation has no function scope".to_owned()
                            })?,
                        repeated_output_by_parameter.get(&ordinal).copied(),
                        kernel_subspan(
                            view,
                            raw_statements[index].line,
                            parameter.start,
                            parameter.end,
                        ),
                    )
                }
                KernelDeclarationOrigin::RecordField { object, ordinal } => {
                    let object_index = object.0 as usize;
                    let fields = match &raw_expressions[object_index].kind {
                        AstExprKind::Object(fields) | AstExprKind::TaggedObject { fields, .. } => {
                            fields
                        }
                        _ => {
                            return Err(
                                "record-field presentation belongs to a non-record".to_owned()
                            );
                        }
                    };
                    let field = fields.get(ordinal as usize).ok_or_else(|| {
                        "record-field presentation references missing ordinal".to_owned()
                    })?;
                    let value = local_by_syntax.get(&field.value).copied();
                    (
                        presentations[object_index].scope,
                        value
                            .and_then(|value| expression_boundaries.get(&value).copied())
                            .filter(|scope| {
                                scopes[scope.0 as usize].kind == KernelScopeKind::Record
                            }),
                        kernel_subspan(
                            view,
                            raw_expressions[object_index].line,
                            field.start,
                            field.end,
                        ),
                    )
                }
                KernelDeclarationOrigin::PatternBinding { arm, .. } => (
                    presentations[arm.0 as usize].scope,
                    None,
                    kernel_expression_span(raw_expressions[arm.0 as usize]),
                ),
                KernelDeclarationOrigin::CallbackBinding { call, .. } => (
                    presentations[call.0 as usize].scope,
                    None,
                    kernel_expression_span(raw_expressions[call.0 as usize]),
                ),
            };
            Ok(KernelDeclarationPresentation {
                declaration: declaration.id,
                scope,
                body_scope,
                span,
            })
        })
        .collect::<Result<Vec<_>, String>>()?
        .into_boxed_slice();

    let containing_scope_targets = view
        .child_owners()
        .iter()
        .map(|boundary| {
            let owner = view
                .stable_check_owner_for_local_statement(boundary.statement())
                .ok_or_else(|| "child scope boundary has no stable owner".to_owned())?;
            let scope = boundary
                .parent()
                .and_then(|parent| dense_statement_by_local.get(&parent).copied())
                .map(|parent| {
                    body_scopes[parent]
                        .map(KernelScopeReference::Local)
                        .unwrap_or_else(|| {
                            statement_scopes[parent].expect("parent statement scope was assigned")
                        })
                })
                .unwrap_or(KernelScopeReference::Containing);
            Ok(PreparedContainingScopeTarget { owner, scope })
        })
        .collect::<Result<Vec<_>, String>>()?
        .into_boxed_slice();

    Ok((
        KernelDefinitionPresentation {
            containing_scope: KernelScopeReference::ProjectRoot,
            scopes: scopes.into_boxed_slice(),
            expressions: presentations.into_boxed_slice(),
            statements: statement_presentations,
            declarations: declaration_presentations,
        },
        containing_scope_targets,
    ))
}

fn kernel_resource_projection(
    nodes: &[KernelOwnerNode],
    root: KernelExpressionId,
    target: KernelExpressionId,
) -> Option<Vec<Box<str>>> {
    fn visit(
        nodes: &[KernelOwnerNode],
        current: KernelExpressionId,
        target: KernelExpressionId,
        active: &mut BTreeSet<KernelExpressionId>,
    ) -> Option<Vec<Box<str>>> {
        if current == target {
            return Some(Vec::new());
        }
        if !active.insert(current) {
            return None;
        }
        let node = nodes.get(current.0 as usize)?;
        let result = node.inputs.iter().find_map(|input| {
            let child = input.expression;
            if child.0 as usize >= nodes.len() {
                return None;
            }
            let mut projection = visit(nodes, child, target, active)?;
            if let KernelOwnerEdgeRole::RecordField {
                name,
                spread: false,
            } = &input.role
            {
                projection.insert(0, name.clone());
            }
            Some(projection)
        });
        active.remove(&current);
        result
    }

    visit(nodes, root, target, &mut BTreeSet::new())
}

fn kernel_inline_list_authority_root(
    nodes: &[KernelOwnerNode],
    root: KernelExpressionId,
) -> Option<KernelExpressionId> {
    let mut current = root;
    let mut visited = BTreeSet::new();
    while visited.insert(current) {
        let node = nodes.get(current.0 as usize)?;
        match &node.kind {
            KernelOwnerNodeKind::Collection {
                kind: KernelCollectionKind::List,
                ..
            } => return Some(current),
            KernelOwnerNodeKind::Draining => {
                current = node
                    .inputs
                    .iter()
                    .find(|input| input.role == KernelOwnerEdgeRole::DrainingInput)?
                    .expression;
            }
            KernelOwnerNodeKind::Block => {
                current = node
                    .inputs
                    .iter()
                    .find(|input| input.role == KernelOwnerEdgeRole::BlockResult)?
                    .expression;
            }
            KernelOwnerNodeKind::Then => {
                current = node
                    .inputs
                    .iter()
                    .find(|input| input.role == KernelOwnerEdgeRole::ThenOutput)?
                    .expression;
            }
            KernelOwnerNodeKind::MatchArm { .. } => {
                current = node
                    .inputs
                    .iter()
                    .find(|input| input.role == KernelOwnerEdgeRole::MatchOutput)?
                    .expression;
            }
            KernelOwnerNodeKind::PureBuiltin {
                kind:
                    KernelPureBuiltinKind::ListFilter
                    | KernelPureBuiltinKind::ListMap
                    | KernelPureBuiltinKind::ListAppend
                    | KernelPureBuiltinKind::ListSort,
            } => {
                current = node
                    .inputs
                    .iter()
                    .find(|input| {
                        matches!(
                            &input.role,
                            KernelOwnerEdgeRole::AbiArgument { name }
                                if name.as_ref() == "$pipe" || name.as_ref() == "list"
                        )
                    })?
                    .expression;
            }
            _ => return None,
        }
        if current.0 as usize >= nodes.len() {
            return None;
        }
    }
    None
}

/// Return the definition whose public result supplies the persistent LIST
/// authority for `owner` after following only operations that preserve the
/// input list's occurrence identity.
///
/// This is intentionally a project-graph query. A child LIST owner can feed a
/// parent field through `List/map`, `List/filter`, `List/append`, or another
/// transparent carrier. Parser nesting alone cannot prove that relationship;
/// the dense external-result edge can.
fn project_inline_list_authority_owner(
    project: &KernelProjectProgramInput,
    owner: KernelOwnerId,
) -> Option<KernelOwnerId> {
    let input = project.owners.get(owner.0 as usize)?;
    let mut current = input.result;
    let mut visited = BTreeSet::new();
    while visited.insert(current) {
        let index = current.0 as usize;
        if index >= input.nodes.len() {
            let external = input.external_expressions.get(index - input.nodes.len())?;
            return match external.target {
                KernelExternalTarget::Result => Some(external.owner),
                KernelExternalTarget::Expression(expression) => {
                    let target = project.owners.get(external.owner.0 as usize)?;
                    kernel_inline_list_authority_root(&target.nodes, expression)
                        .map(|_| external.owner)
                }
            };
        }
        let node = &input.nodes[index];
        current = match &node.kind {
            KernelOwnerNodeKind::Collection {
                kind: KernelCollectionKind::List,
                ..
            } => return Some(owner),
            KernelOwnerNodeKind::Draining => {
                node.inputs
                    .iter()
                    .find(|input| input.role == KernelOwnerEdgeRole::DrainingInput)?
                    .expression
            }
            KernelOwnerNodeKind::Block => {
                node.inputs
                    .iter()
                    .find(|input| input.role == KernelOwnerEdgeRole::BlockResult)?
                    .expression
            }
            KernelOwnerNodeKind::Then => {
                node.inputs
                    .iter()
                    .find(|input| input.role == KernelOwnerEdgeRole::ThenOutput)?
                    .expression
            }
            KernelOwnerNodeKind::MatchArm { .. } => {
                node.inputs
                    .iter()
                    .find(|input| input.role == KernelOwnerEdgeRole::MatchOutput)?
                    .expression
            }
            KernelOwnerNodeKind::PureBuiltin {
                kind:
                    KernelPureBuiltinKind::ListFilter
                    | KernelPureBuiltinKind::ListMap
                    | KernelPureBuiltinKind::ListAppend
                    | KernelPureBuiltinKind::ListSort,
            } => {
                node.inputs
                    .iter()
                    .find(|input| {
                        matches!(
                            &input.role,
                            KernelOwnerEdgeRole::AbiArgument { name }
                                if name.as_ref() == "$pipe" || name.as_ref() == "list"
                        )
                    })?
                    .expression
            }
            _ => return None,
        };
    }
    None
}

fn compact_resource_facts(
    view: UnitOwnerSyntaxView<'_>,
    owner: &StableCheckOwnerKey,
    root_statement: UnitLocalStatementId,
    raw_expressions: &[&boon_syntax::AstExpr],
    local_by_syntax: &BTreeMap<usize, usize>,
    nodes: &[KernelOwnerNode],
    result: KernelExpressionId,
    facts: &mut KernelDefinitionFactsInput,
) -> Result<
    (
        Box<[PreparedResourceOwnerTarget]>,
        Box<[PreparedResourceSyntheticPath]>,
    ),
    String,
> {
    fn push_owner_target(
        targets: &mut Vec<PreparedResourceOwnerTarget>,
        field: PreparedResourceOwnerField,
        target: &PreparedLexicalTarget,
    ) {
        if let PreparedLexicalTarget::OwnerPublic(owner) = target {
            targets.push(PreparedResourceOwnerTarget {
                field,
                owner: owner.clone(),
            });
        }
    }

    let dense_statement_by_syntax = view
        .statement_ids()
        .iter()
        .copied()
        .zip(view.statements())
        .enumerate()
        .map(|(dense, (local, statement))| {
            (
                (local, statement.id),
                KernelStatementId(
                    u32::try_from(dense).expect("definition statement count fits u32"),
                ),
            )
        })
        .collect::<Vec<_>>();
    let dense_statement_by_local = dense_statement_by_syntax
        .iter()
        .map(|((local, _), dense)| (*local, *dense))
        .collect::<BTreeMap<_, _>>();
    let dense_statement_by_id = dense_statement_by_syntax
        .iter()
        .map(|((_, syntax), dense)| (*syntax, *dense))
        .collect::<BTreeMap<_, _>>();
    let root_statement_dense = *dense_statement_by_local
        .get(&root_statement)
        .ok_or_else(|| "kernel resource root statement is not definition-local".to_owned())?;
    let declaration_by_origin = facts
        .declarations
        .iter()
        .map(|declaration| (declaration.origin.clone(), declaration.id))
        .collect::<BTreeMap<_, _>>();
    let public_declaration = declaration_by_origin
        .get(&KernelDeclarationOrigin::Statement {
            statement: root_statement_dense,
        })
        .copied();
    let declarations_by_value = facts.declarations.iter().fold(
        BTreeMap::<KernelExpressionId, Vec<&KernelDeclarationInput>>::new(),
        |mut by_value, declaration| {
            if let Some(value) = declaration.value {
                by_value.entry(value).or_default().push(declaration);
            }
            by_value
        },
    );
    let containing_statements = resource_containing_statements(
        view,
        raw_expressions,
        local_by_syntax,
        nodes,
        &facts.statements,
    );
    let enclosing_declaration_target = {
        let mut parent = view
            .statement_locator(root_statement)
            .and_then(|locator| locator.parent());
        let mut target = None;
        while let Some(statement) = parent {
            let Some(statement_input) = view.statement_for_local(statement) else {
                break;
            };
            let declares = matches!(
                statement_input.kind,
                AstStatementKind::Function { .. } | AstStatementKind::Field { .. }
            ) || matches!(
                statement_input.kind,
                AstStatementKind::Source { field: Some(_), .. }
                    | AstStatementKind::Hold { field: Some(_), .. }
                    | AstStatementKind::List { field: Some(_), .. }
            );
            if declares {
                target = prepared_statement_lexical_target(
                    view,
                    owner,
                    statement,
                    &dense_statement_by_id,
                );
                break;
            }
            parent = view
                .statement_locator(statement)
                .and_then(|locator| locator.parent());
        }
        target
    };
    let root_hold_target =
        prepared_hold_alias_lexical_target(view, owner, root_statement, &dense_statement_by_id)?;

    let mut owner_targets = Vec::new();
    let mut synthetic_paths = Vec::new();
    let mut sources = Vec::new();
    let mut states = Vec::new();
    let mut lists = Vec::new();
    let mut state_count = 0usize;
    let mut list_count = 0usize;

    let canonical_target = |expression: KernelExpressionId,
                            exact_statement: bool,
                            exact_record_field: bool|
     -> Option<PreparedLexicalTarget> {
        let exact = declarations_by_value
            .get(&expression)
            .and_then(|declarations| {
                declarations
                    .iter()
                    .find(|declaration| {
                        exact_statement
                            && matches!(
                                declaration.origin,
                                KernelDeclarationOrigin::Statement { .. }
                            )
                    })
                    .or_else(|| {
                        declarations.iter().find(|declaration| {
                            exact_record_field
                                && matches!(
                                    declaration.origin,
                                    KernelDeclarationOrigin::RecordField { .. }
                                )
                        })
                    })
                    .copied()
            });
        exact
            .map(|declaration| PreparedLexicalTarget::Declaration(declaration.origin.clone()))
            .or_else(|| {
                public_declaration.map(|declaration| {
                    PreparedLexicalTarget::Declaration(
                        facts.declarations[declaration.0 as usize].origin.clone(),
                    )
                })
            })
            .or_else(|| root_hold_target.clone())
            .or_else(|| enclosing_declaration_target.clone())
    };
    let local_reference = |target: &PreparedLexicalTarget| -> Option<KernelDeclarationReference> {
        match target {
            PreparedLexicalTarget::Declaration(origin) => declaration_by_origin
                .get(origin)
                .copied()
                .map(KernelDeclarationReference::Local),
            PreparedLexicalTarget::OwnerPublic(_) => {
                Some(KernelDeclarationReference::OwnerPublic(KernelOwnerId(0)))
            }
            PreparedLexicalTarget::Value(_) | PreparedLexicalTarget::RuntimeContext => None,
        }
    };
    let target_owner = |target: &PreparedLexicalTarget| match target {
        PreparedLexicalTarget::OwnerPublic(owner) => owner.clone(),
        PreparedLexicalTarget::Declaration(_) => owner.clone(),
        PreparedLexicalTarget::Value(_) | PreparedLexicalTarget::RuntimeContext => owner.clone(),
    };
    let canonical_projection = |target: &PreparedLexicalTarget,
                                expression: KernelExpressionId|
     -> (Vec<Box<str>>, bool) {
        let PreparedLexicalTarget::Declaration(origin) = target else {
            return (
                Vec::new(),
                kernel_resource_projection(nodes, result, expression).is_some(),
            );
        };
        let Some(declaration) = declaration_by_origin
            .get(origin)
            .and_then(|declaration| facts.declarations.get(declaration.0 as usize))
        else {
            return (Vec::new(), false);
        };
        let root = declaration.value;
        let projection = root.and_then(|root| kernel_resource_projection(nodes, root, expression));
        (projection.clone().unwrap_or_default(), projection.is_some())
    };
    let resource_statement =
        |target: &PreparedLexicalTarget,
         syntax_expression: usize|
         -> Option<(KernelStatementReference, Option<StableCheckOwnerKey>)> {
            if let PreparedLexicalTarget::OwnerPublic(owner) = target {
                return Some((
                    KernelStatementReference::OwnerPublic(KernelOwnerId(0)),
                    Some(owner.clone()),
                ));
            }
            let declaration_statement = match target {
                PreparedLexicalTarget::Declaration(origin) => declaration_by_origin
                    .get(origin)
                    .and_then(|declaration| facts.declarations.get(declaration.0 as usize))
                    .and_then(|declaration| match declaration.origin {
                        KernelDeclarationOrigin::Statement { statement } => Some(statement),
                        _ => None,
                    }),
                _ => None,
            };
            declaration_statement
                .or_else(|| containing_statements.get(&syntax_expression).copied())
                .map(|statement| (KernelStatementReference::Local(statement), None))
        };
    for expression in raw_expressions {
        let Some(dense) = local_by_syntax.get(&expression.id).copied() else {
            continue;
        };
        let dense = KernelExpressionId(checked_u32(dense, "resource expression")?);
        match &nodes[dense.0 as usize].kind {
            KernelOwnerNodeKind::Source(_) => {
                let Some(target) = canonical_target(dense, true, true) else {
                    continue;
                };
                let Some(declaration) = local_reference(&target) else {
                    continue;
                };
                let Some((statement, statement_owner)) = resource_statement(&target, expression.id)
                else {
                    continue;
                };
                let (projection, _) = canonical_projection(&target, dense);
                let row = sources.len();
                sources.push(KernelSourceInput {
                    id: KernelSourceId(checked_u32(row, "SOURCE resource row")?),
                    declaration,
                    statement,
                    expression: dense,
                    projection: projection.into_boxed_slice(),
                    interval_ms: None,
                });
                push_owner_target(
                    &mut owner_targets,
                    PreparedResourceOwnerField::SourceDeclaration(row),
                    &target,
                );
                if let Some(owner) = statement_owner {
                    owner_targets.push(PreparedResourceOwnerTarget {
                        field: PreparedResourceOwnerField::SourceStatement(row),
                        owner,
                    });
                }
            }
            KernelOwnerNodeKind::Hold => {
                let Some(target) = canonical_target(dense, true, false) else {
                    continue;
                };
                let Some(declaration) = local_reference(&target) else {
                    continue;
                };
                let Some((statement, statement_owner)) = resource_statement(&target, expression.id)
                else {
                    continue;
                };
                let binding_target = containing_statements
                    .get(&expression.id)
                    .and_then(|statement| view.statement_ids().get(statement.0 as usize).copied())
                    .and_then(|statement| {
                        view.statement_for_local(statement)
                            .and_then(|statement_input| {
                                (statement_input.expr == Some(expression.id)
                                    && matches!(
                                        statement_input.kind,
                                        AstStatementKind::Hold { .. }
                                    ))
                                .then_some(statement)
                            })
                    })
                    .map(|statement| {
                        prepared_hold_alias_lexical_target(
                            view,
                            owner,
                            statement,
                            &dense_statement_by_id,
                        )
                    })
                    .transpose()?
                    .flatten()
                    .or_else(|| {
                        declarations_by_value.get(&dense).and_then(|declarations| {
                            declarations
                                .iter()
                                .find(|declaration| {
                                    matches!(
                                        declaration.origin,
                                        KernelDeclarationOrigin::RecordField { .. }
                                    )
                                })
                                .map(|declaration| {
                                    PreparedLexicalTarget::Declaration(declaration.origin.clone())
                                })
                        })
                    })
                    .unwrap_or_else(|| target.clone());
                let Some(binding_declaration) = local_reference(&binding_target) else {
                    continue;
                };
                let Some(initial) = nodes[dense.0 as usize]
                    .inputs
                    .iter()
                    .find(|input| input.role == KernelOwnerEdgeRole::HoldInitial)
                    .map(|input| input.expression)
                else {
                    return Err(format!(
                        "HOLD resource expression {} has no initial",
                        expression.id
                    ));
                };
                let (projection, declaration_result) = canonical_projection(&target, dense);
                let row = states.len();
                states.push(KernelStateInput {
                    id: KernelStateId(checked_u32(row, "state resource row")?),
                    binding_declaration,
                    declaration,
                    statement,
                    expression: dense,
                    initial,
                    projection: projection.into_boxed_slice(),
                    kind: CheckedStateKind::Hold,
                });
                push_owner_target(
                    &mut owner_targets,
                    PreparedResourceOwnerField::StateBindingDeclaration(row),
                    &binding_target,
                );
                push_owner_target(
                    &mut owner_targets,
                    PreparedResourceOwnerField::StateDeclaration(row),
                    &target,
                );
                if let Some(owner) = statement_owner {
                    owner_targets.push(PreparedResourceOwnerTarget {
                        field: PreparedResourceOwnerField::StateStatement(row),
                        owner,
                    });
                }
                let function_declaration = matches!(
                    target,
                    PreparedLexicalTarget::Declaration(ref origin)
                        if declaration_by_origin
                            .get(origin)
                            .and_then(|declaration| facts.declarations.get(declaration.0 as usize))
                            .is_some_and(|declaration| declaration.kind == KernelDeclarationKind::Function)
                );
                if states[row].projection.is_empty()
                    && (!declaration_result || function_declaration)
                {
                    synthetic_paths.push(PreparedResourceSyntheticPath {
                        kind: PreparedResourceSyntheticKind::State,
                        row,
                        anchor: target_owner(&target),
                    });
                }
                state_count = state_count.saturating_add(1);
            }
            KernelOwnerNodeKind::Collection {
                kind: KernelCollectionKind::List,
                capacity,
            } => {
                let Some(target) = canonical_target(dense, true, true) else {
                    continue;
                };
                let Some(declaration) = local_reference(&target) else {
                    continue;
                };
                let declaration_authority = match &target {
                    PreparedLexicalTarget::Declaration(origin) => declaration_by_origin
                        .get(origin)
                        .and_then(|declaration| facts.declarations.get(declaration.0 as usize))
                        .and_then(|declaration| declaration.value)
                        .or_else(|| {
                            matches!(
                                target,
                                PreparedLexicalTarget::Declaration(ref origin)
                                    if declaration_by_origin
                                        .get(origin)
                                        .and_then(|declaration| facts.declarations.get(declaration.0 as usize))
                                        .is_some_and(|declaration| declaration.kind == KernelDeclarationKind::Function)
                            )
                            .then_some(result)
                        })
                        .and_then(|root| kernel_inline_list_authority_root(nodes, root))
                        == Some(dense),
                    // Cross-owner authority is resolved from the dense project
                    // graph after external owner IDs have been linked. Keep
                    // the local LIST statement here as the fail-closed default.
                    PreparedLexicalTarget::OwnerPublic(_) => false,
                    _ => false,
                };
                let statement = (!declaration_authority)
                    .then(|| containing_statements.get(&expression.id).copied())
                    .flatten()
                    .map(|statement| (KernelStatementReference::Local(statement), None))
                    .or_else(|| resource_statement(&target, expression.id));
                let Some((statement, statement_owner)) = statement else {
                    continue;
                };
                let (projection, _) = canonical_projection(&target, dense);
                let row = lists.len();
                lists.push(KernelListInput {
                    id: KernelListId(checked_u32(row, "LIST resource row")?),
                    declaration,
                    statement,
                    producer: dense,
                    projection: projection.into_boxed_slice(),
                    capacity: *capacity,
                    key_policy: CheckedListKeyPolicy::GeneratedOccurrenceU64 {
                        has_generation: true,
                    },
                });
                push_owner_target(
                    &mut owner_targets,
                    PreparedResourceOwnerField::ListDeclaration(row),
                    &target,
                );
                if let PreparedLexicalTarget::OwnerPublic(authority) = &target {
                    owner_targets.push(PreparedResourceOwnerTarget {
                        field: PreparedResourceOwnerField::ListStatement(row),
                        owner: authority.clone(),
                    });
                } else if let Some(owner) = statement_owner {
                    owner_targets.push(PreparedResourceOwnerTarget {
                        field: PreparedResourceOwnerField::ListStatement(row),
                        owner,
                    });
                }
                let function_declaration = matches!(
                    target,
                    PreparedLexicalTarget::Declaration(ref origin)
                        if declaration_by_origin
                            .get(origin)
                            .and_then(|declaration| facts.declarations.get(declaration.0 as usize))
                            .is_some_and(|declaration| declaration.kind == KernelDeclarationKind::Function)
                );
                if lists[row].projection.is_empty() && function_declaration {
                    synthetic_paths.push(PreparedResourceSyntheticPath {
                        kind: PreparedResourceSyntheticKind::List,
                        row,
                        anchor: target_owner(&target),
                    });
                }
                list_count = list_count.saturating_add(1);
            }
            _ => {}
        }
    }

    debug_assert_eq!(state_count, states.len());
    debug_assert_eq!(list_count, lists.len());
    facts.sources = sources.into_boxed_slice();
    facts.states = states.into_boxed_slice();
    facts.lists = lists.into_boxed_slice();
    if std::env::var_os("BOON_KERNEL_ORACLE_TRACE_OWNER")
        .is_some_and(|pattern| format!("{owner:?}").contains(pattern.to_string_lossy().as_ref()))
    {
        eprintln!(
            "kernel-owner-trace resources sources={:#?} states={:#?} lists={:#?}",
            facts.sources, facts.states, facts.lists
        );
    }
    Ok((
        owner_targets.into_boxed_slice(),
        synthetic_paths.into_boxed_slice(),
    ))
}

#[allow(clippy::too_many_arguments)]
fn compact_declaration_and_lexical_facts(
    view: UnitOwnerSyntaxView<'_>,
    owner: &StableCheckOwnerKey,
    root_statement: UnitLocalStatementId,
    raw_expressions: &[&boon_syntax::AstExpr],
    local_by_syntax: &BTreeMap<usize, usize>,
    formal_by_name: &BTreeMap<String, usize>,
    owner_context_ordinal: Option<usize>,
    lexical_binding_reads: &BTreeMap<usize, PreparedLexicalBinding>,
    output_bindings_by_scope: &PreparedOutputBindingsByScope,
    statement_record_field_targets: &BTreeMap<(usize, String), PreparedLexicalTarget>,
    value_surfaces: &BTreeMap<String, Vec<ValueSurface>>,
    node_count: usize,
    statements: &[KernelStatementInput],
    external_by_key: &mut BTreeMap<PreparedExternalExpression, usize>,
    external_expressions: &mut Vec<PreparedExternalExpression>,
) -> Result<
    (
        Box<[KernelDeclarationInput]>,
        Box<[KernelLexicalBindingInput]>,
        Box<[PreparedLexicalOwnerTarget]>,
    ),
    String,
> {
    let dense_statement_by_syntax = view
        .statement_ids()
        .iter()
        .copied()
        .zip(view.statements())
        .enumerate()
        .map(|(dense, (_, statement))| {
            Ok((
                statement.id,
                KernelStatementId(checked_u32(dense, "declaration statement")?),
            ))
        })
        .collect::<Result<BTreeMap<_, _>, String>>()?;
    let root_statement = *dense_statement_by_syntax
        .get(
            &view
                .statement_for_local(root_statement)
                .ok_or_else(|| "kernel root declaration statement is missing".to_owned())?
                .id,
        )
        .ok_or_else(|| "kernel root declaration is not definition-local".to_owned())?;

    let mut pending = Vec::<(
        KernelDeclarationOrigin,
        Box<str>,
        KernelDeclarationKind,
        Option<KernelExpressionId>,
    )>::new();
    for statement in statements {
        let (name, kind, value) = match &statement.kind {
            KernelStatementKind::Function { name, .. } => {
                (name.clone(), KernelDeclarationKind::Function, None)
            }
            KernelStatementKind::Field { name } => {
                (name.clone(), KernelDeclarationKind::Field, statement.value)
            }
            KernelStatementKind::Source {
                field: Some(name), ..
            } => (name.clone(), KernelDeclarationKind::Source, statement.value),
            KernelStatementKind::Hold {
                field: Some(name), ..
            } => (name.clone(), KernelDeclarationKind::Hold, statement.value),
            KernelStatementKind::Hold {
                field: None,
                name: Some(name),
            } => {
                let local_statement = *view
                    .statement_ids()
                    .get(statement.id.0 as usize)
                    .ok_or_else(|| {
                        "kernel fieldless HOLD declaration has no local statement".to_owned()
                    })?;
                let owns_declaration = matches!(
                    prepared_hold_alias_lexical_target(
                        view,
                        owner,
                        local_statement,
                        &dense_statement_by_syntax,
                    )?,
                    Some(PreparedLexicalTarget::Declaration(
                        KernelDeclarationOrigin::Statement { statement: target },
                    )) if target == statement.id
                );
                if !owns_declaration {
                    continue;
                }
                (name.clone(), KernelDeclarationKind::Hold, statement.value)
            }
            KernelStatementKind::List {
                field: Some(name), ..
            } => (name.clone(), KernelDeclarationKind::List, statement.value),
            KernelStatementKind::Source { field: None, .. }
            | KernelStatementKind::Hold {
                field: None,
                name: None,
            }
            | KernelStatementKind::List { field: None, .. }
            | KernelStatementKind::Block
            | KernelStatementKind::Spread
            | KernelStatementKind::Expression => continue,
        };
        pending.push((
            KernelDeclarationOrigin::Statement {
                statement: statement.id,
            },
            name,
            kind,
            value,
        ));
        if let KernelStatementKind::Function { parameters, .. } = &statement.kind {
            pending.extend(parameters.iter().map(|parameter| {
                (
                    KernelDeclarationOrigin::Parameter {
                        statement: statement.id,
                        ordinal: parameter.ordinal,
                    },
                    parameter.name.clone(),
                    match parameter.kind {
                        KernelParameterKind::Value => KernelDeclarationKind::ValueParameter,
                        KernelParameterKind::Out => KernelDeclarationKind::OutParameter,
                    },
                    None,
                )
            }));
        }
    }

    let mut callback_origin_by_binding = BTreeMap::<usize, KernelDeclarationOrigin>::new();
    for expression in raw_expressions {
        let object = KernelExpressionId(checked_u32(
            *local_by_syntax
                .get(&expression.id)
                .ok_or_else(|| "kernel declaration expression is not local".to_owned())?,
            "declaration expression",
        )?);
        if let Some(bindings) = output_bindings_by_scope.get(&expression.id) {
            for binding in bindings {
                let origin = KernelDeclarationOrigin::CallbackBinding {
                    call: object,
                    ordinal: binding.formal_ordinal,
                };
                if callback_origin_by_binding
                    .insert(binding.provider, origin.clone())
                    .is_some()
                {
                    return Err(format!(
                        "OUT call expression {} repeats one binding occurrence",
                        expression.id
                    ));
                }
                pending.push((
                    origin,
                    binding.name.clone().into_boxed_str(),
                    KernelDeclarationKind::FreshOut,
                    None,
                ));
            }
        }
        match &expression.kind {
            AstExprKind::Object(fields) | AstExprKind::TaggedObject { fields, .. } => {
                for (ordinal, field) in fields.iter().enumerate() {
                    if field.spread
                        || statement_record_field_targets
                            .contains_key(&(expression.id, field.name.clone()))
                        || view
                            .stable_check_owner_for_syntax_expression(field.value)
                            .is_some_and(|field_owner| field_owner != *owner)
                    {
                        continue;
                    }
                    pending.push((
                        KernelDeclarationOrigin::RecordField {
                            object,
                            ordinal: checked_u32(ordinal, "record field ordinal")?,
                        },
                        field.name.clone().into_boxed_str(),
                        KernelDeclarationKind::Field,
                        Some(checked_kernel_expression(prepared_input_reference_index(
                            PreparedInputReference::Syntax(field.value),
                            view,
                            owner,
                            Some(expression.id),
                            local_by_syntax,
                            node_count,
                            external_by_key,
                            external_expressions,
                        )?)?),
                    ));
                }
            }
            AstExprKind::MatchArm { pattern, .. } => {
                for (ordinal, name) in pattern_variable_names(pattern).into_iter().enumerate() {
                    pending.push((
                        KernelDeclarationOrigin::PatternBinding {
                            arm: object,
                            ordinal: checked_u32(ordinal, "pattern binding ordinal")?,
                        },
                        name.into_boxed_str(),
                        KernelDeclarationKind::PatternBinding,
                        None,
                    ));
                }
            }
            _ => {}
        }
    }
    pending.sort_by(|left, right| left.0.cmp(&right.0));
    let mut declaration_by_origin = BTreeMap::new();
    let declarations = pending
        .into_iter()
        .enumerate()
        .map(|(index, (origin, name, kind, value))| {
            let id = KernelDeclarationId(checked_u32(index, "declaration index")?);
            if declaration_by_origin.insert(origin.clone(), id).is_some() {
                return Err(format!(
                    "kernel declaration origin {origin:?} was projected twice"
                ));
            }
            Ok(KernelDeclarationInput {
                id,
                origin,
                name,
                kind,
                value,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;

    let public_declaration = declaration_by_origin
        .get(&KernelDeclarationOrigin::Statement {
            statement: root_statement,
        })
        .copied();
    let mut owner_targets = Vec::new();
    let mut bindings = Vec::new();
    for expression in raw_expressions {
        let (parts, access) = match &expression.kind {
            AstExprKind::Identifier(root) => (vec![root.clone()], KernelLexicalAccess::Read),
            AstExprKind::Path(path) => (path.clone(), KernelLexicalAccess::Read),
            AstExprKind::Drain { path } => (
                match path {
                    boon_syntax::AstDrainPath::Binding { name } => vec![name.clone()],
                    boon_syntax::AstDrainPath::Field { binding, fields } => {
                        std::iter::once(binding.clone())
                            .chain(fields.iter().cloned())
                            .collect()
                    }
                    boon_syntax::AstDrainPath::Passed { fields } => {
                        std::iter::once("PASSED".to_owned())
                            .chain(fields.iter().cloned())
                            .collect()
                    }
                },
                KernelLexicalAccess::Drain,
            ),
            _ => continue,
        };
        let Some((root, suffix)) = parts.split_first() else {
            continue;
        };
        let expression_id = KernelExpressionId(checked_u32(
            *local_by_syntax
                .get(&expression.id)
                .ok_or_else(|| "lexical expression is not definition-local".to_owned())?,
            "lexical expression",
        )?);
        let mut residual_suffix = suffix.to_vec();
        let (target, prefix): (KernelLexicalBindingTargetInput, &[String]) = if let Some(origin) =
            callback_origin_by_binding.get(&expression.id)
        {
            (
                KernelLexicalBindingTargetInput::Declaration(KernelDeclarationReference::Local(
                    declaration_by_origin[origin],
                )),
                &[],
            )
        } else if let Some(binding) = lexical_binding_reads.get(&expression.id) {
            let target = match &binding.target {
                PreparedLexicalTarget::Declaration(origin) => declaration_by_origin
                    .get(origin)
                    .copied()
                    .map(KernelDeclarationReference::Local)
                    .map(KernelLexicalBindingTargetInput::Declaration)
                    .unwrap_or_else(|| {
                        prepared_binding_value_target(
                            &binding.provider,
                            view,
                            owner,
                            expression.id,
                            local_by_syntax,
                            node_count,
                            external_by_key,
                            external_expressions,
                        )
                        .expect("prepared lexical provider was validated")
                    }),
                PreparedLexicalTarget::OwnerPublic(target_owner) => {
                    let binding = bindings.len();
                    owner_targets.push(PreparedLexicalOwnerTarget {
                        binding,
                        owner: target_owner.clone(),
                    });
                    KernelLexicalBindingTargetInput::Declaration(
                        KernelDeclarationReference::OwnerPublic(KernelOwnerId(0)),
                    )
                }
                PreparedLexicalTarget::Value(provider) => KernelLexicalBindingTargetInput::Value {
                    provider: checked_kernel_expression(prepared_input_reference_index(
                        provider.clone(),
                        view,
                        owner,
                        Some(expression.id),
                        local_by_syntax,
                        node_count,
                        external_by_key,
                        external_expressions,
                    )?)?,
                },
                PreparedLexicalTarget::RuntimeContext => {
                    KernelLexicalBindingTargetInput::RuntimeContext
                }
            };
            let target_prefix = if matches!(
                binding.target,
                PreparedLexicalTarget::Declaration(KernelDeclarationOrigin::PatternBinding { .. })
            ) {
                &[]
            } else {
                binding.prefix.as_ref()
            };
            (target, target_prefix)
        } else if let Some(formal) = formal_by_name.get(root).copied() {
            let ordinal = checked_u32(formal, "lexical formal ordinal")?;
            if owner_context_ordinal == Some(formal) {
                (
                    KernelLexicalBindingTargetInput::ContextFormal { ordinal },
                    &[],
                )
            } else {
                let origin = KernelDeclarationOrigin::Parameter {
                    statement: root_statement,
                    ordinal,
                };
                let declaration = declaration_by_origin.get(&origin).copied().ok_or_else(|| {
                    format!("formal `{root}` has no declaration origin {origin:?}")
                })?;
                (
                    KernelLexicalBindingTargetInput::Declaration(
                        KernelDeclarationReference::Local(declaration),
                    ),
                    &[],
                )
            }
        } else {
            let mut parts = Vec::with_capacity(suffix.len() + 1);
            parts.push(root.to_owned());
            parts.extend(suffix.iter().cloned());
            let Ok((surface, consumed)) = exact_value_path_surface(&parts, value_surfaces, owner)
            else {
                // Unresolved, callable-as-value, and ambiguous reads compile
                // as explicit Unknown nodes with typed diagnostics. They have
                // no lexical target by definition, so do not turn the absence
                // of a binding row back into unsupported-owner control flow.
                continue;
            };
            residual_suffix = parts[consumed..].to_vec();
            if &surface.owner == owner {
                let declaration = public_declaration.ok_or_else(|| {
                    format!("owner-local read `{root}` has no public declaration")
                })?;
                (
                    KernelLexicalBindingTargetInput::Declaration(
                        KernelDeclarationReference::Local(declaration),
                    ),
                    &[],
                )
            } else {
                let binding = bindings.len();
                owner_targets.push(PreparedLexicalOwnerTarget {
                    binding,
                    owner: surface.owner.clone(),
                });
                (
                    KernelLexicalBindingTargetInput::Declaration(
                        KernelDeclarationReference::OwnerPublic(KernelOwnerId(0)),
                    ),
                    &[],
                )
            }
        };
        let projection = prefix
            .iter()
            .chain(residual_suffix.iter())
            .cloned()
            .map(String::into_boxed_str)
            .collect::<Vec<_>>()
            .into_boxed_slice();
        bindings.push(KernelLexicalBindingInput {
            expression: expression_id,
            target,
            projection,
            access,
        });
    }
    Ok((
        declarations.into_boxed_slice(),
        bindings.into_boxed_slice(),
        owner_targets.into_boxed_slice(),
    ))
}

#[allow(clippy::too_many_arguments)]
fn prepared_binding_value_target(
    provider: &PreparedLexicalProvider,
    view: UnitOwnerSyntaxView<'_>,
    owner: &StableCheckOwnerKey,
    consumer: usize,
    local_by_syntax: &BTreeMap<usize, usize>,
    node_count: usize,
    external_by_key: &mut BTreeMap<PreparedExternalExpression, usize>,
    external_expressions: &mut Vec<PreparedExternalExpression>,
) -> Result<KernelLexicalBindingTargetInput, String> {
    match provider {
        PreparedLexicalProvider::Input(provider) => Ok(KernelLexicalBindingTargetInput::Value {
            provider: checked_kernel_expression(prepared_input_reference_index(
                provider.clone(),
                view,
                owner,
                Some(consumer),
                local_by_syntax,
                node_count,
                external_by_key,
                external_expressions,
            )?)?,
        }),
        PreparedLexicalProvider::Known(_) => Ok(KernelLexicalBindingTargetInput::RuntimeContext),
    }
}

fn direct_containing_statements(
    view: UnitOwnerSyntaxView<'_>,
    raw_expressions: &[&boon_syntax::AstExpr],
    local_by_syntax: &BTreeMap<usize, usize>,
) -> BTreeMap<usize, UnitLocalStatementId> {
    let mut owners = BTreeMap::new();
    let mut statements = view
        .statement_ids()
        .iter()
        .copied()
        .zip(view.statements())
        .collect::<Vec<_>>();
    statements.sort_by_key(|(statement, _)| {
        let mut depth = 0usize;
        let mut cursor = *statement;
        while let Some(parent) = view
            .statement_locator(cursor)
            .and_then(|locator| locator.parent())
        {
            depth = depth.saturating_add(1);
            cursor = parent;
        }
        std::cmp::Reverse(depth)
    });
    for (statement_id, statement) in statements {
        let Some(root) = statement.expr else {
            continue;
        };
        let mut pending = vec![root];
        let mut visited = BTreeSet::new();
        while let Some(syntax) = pending.pop() {
            if !visited.insert(syntax) {
                continue;
            }
            let Some(expression) = local_by_syntax
                .get(&syntax)
                .and_then(|index| raw_expressions.get(*index))
            else {
                continue;
            };
            owners.entry(syntax).or_insert(statement_id);
            pending.extend(
                source_ast_edges(expression)
                    .unwrap_or_default()
                    .into_iter()
                    .map(|(_, input)| input),
            );
        }
    }
    owners
}

/// Reproduce the checked resource-statement ownership walk directly over the
/// compact expression graph. This deliberately follows only value-producing
/// carriers: collection items, LATEST branches, HOLD updates, and BLOCK
/// bindings belong to their own statement regions and must not be pulled into
/// an enclosing declaration merely because they are dependency children.
fn resource_containing_statements(
    view: UnitOwnerSyntaxView<'_>,
    raw_expressions: &[&boon_syntax::AstExpr],
    local_by_syntax: &BTreeMap<usize, usize>,
    nodes: &[KernelOwnerNode],
    statements: &[KernelStatementInput],
) -> BTreeMap<usize, KernelStatementId> {
    fn owned_inputs(node: &KernelOwnerNode) -> impl Iterator<Item = KernelExpressionId> + '_ {
        node.inputs.iter().filter_map(move |input| {
            let owned = match &node.kind {
                KernelOwnerNodeKind::UserCall { .. }
                | KernelOwnerNodeKind::RenderConstructor { .. }
                | KernelOwnerNodeKind::PureBuiltin { .. }
                | KernelOwnerNodeKind::HostEffect { .. } => matches!(
                    input.role,
                    KernelOwnerEdgeRole::CallArgument { .. }
                        | KernelOwnerEdgeRole::AbiArgument { .. }
                ),
                KernelOwnerNodeKind::Record { .. } => {
                    matches!(input.role, KernelOwnerEdgeRole::RecordField { .. })
                }
                KernelOwnerNodeKind::Draining => input.role == KernelOwnerEdgeRole::DrainingInput,
                KernelOwnerNodeKind::Hold => input.role == KernelOwnerEdgeRole::HoldInitial,
                KernelOwnerNodeKind::When => input.role == KernelOwnerEdgeRole::WhenInput,
                KernelOwnerNodeKind::Then => matches!(
                    input.role,
                    KernelOwnerEdgeRole::ThenInput | KernelOwnerEdgeRole::ThenOutput
                ),
                KernelOwnerNodeKind::Infix { .. } => matches!(
                    input.role,
                    KernelOwnerEdgeRole::InfixLeft | KernelOwnerEdgeRole::InfixRight
                ),
                KernelOwnerNodeKind::MatchArm { .. } => {
                    input.role == KernelOwnerEdgeRole::MatchOutput
                }
                KernelOwnerNodeKind::Known(_)
                | KernelOwnerNodeKind::Source(_)
                | KernelOwnerNodeKind::Absent
                | KernelOwnerNodeKind::Text
                | KernelOwnerNodeKind::TextTemplate
                | KernelOwnerNodeKind::Number
                | KernelOwnerNodeKind::Byte
                | KernelOwnerNodeKind::Bits(_)
                | KernelOwnerNodeKind::Tag(_)
                | KernelOwnerNodeKind::Block
                | KernelOwnerNodeKind::Collection { .. }
                | KernelOwnerNodeKind::MapEntry
                | KernelOwnerNodeKind::FormalRead { .. }
                | KernelOwnerNodeKind::ContextRead { .. }
                | KernelOwnerNodeKind::LexicalRead { .. }
                | KernelOwnerNodeKind::ValueRead { .. }
                | KernelOwnerNodeKind::DerivedRead { .. }
                | KernelOwnerNodeKind::PatternRead { .. }
                | KernelOwnerNodeKind::CollectionItemRead
                | KernelOwnerNodeKind::FreshOut
                | KernelOwnerNodeKind::Latest
                | KernelOwnerNodeKind::Arrow
                | KernelOwnerNodeKind::Delimiter
                | KernelOwnerNodeKind::Unknown => false,
            };
            owned.then_some(input.expression)
        })
    }

    let local_statement_ids = view.statement_ids();
    let mut owners = vec![None; nodes.len()];
    for statement in statements.iter().rev() {
        let Some(local_statement) = local_statement_ids.get(statement.id.0 as usize).copied()
        else {
            continue;
        };
        let mut pending = Vec::with_capacity(2);
        if let Some(value) = statement.value
            && (value.0 as usize) < nodes.len()
        {
            pending.push(value);
        }
        if let Some(expression) = view
            .statement_for_local(local_statement)
            .and_then(|statement| statement.expr)
            .and_then(|expression| local_by_syntax.get(&expression).copied())
            .and_then(|expression| u32::try_from(expression).ok())
            .map(KernelExpressionId)
            && !pending.contains(&expression)
        {
            pending.push(expression);
        }
        let mut visited = BTreeSet::new();
        while let Some(expression) = pending.pop() {
            if !visited.insert(expression) {
                continue;
            }
            let Some(node) = nodes.get(expression.0 as usize) else {
                continue;
            };
            owners[expression.0 as usize].get_or_insert(statement.id);
            pending.extend(owned_inputs(node).filter(|input| (input.0 as usize) < nodes.len()));
        }
    }

    raw_expressions
        .iter()
        .filter_map(|expression| {
            let dense = *local_by_syntax.get(&expression.id)?;
            Some((expression.id, owners.get(dense).copied().flatten()?))
        })
        .collect()
}

struct PreparedCallContextSurface {
    name: &'static str,
    provider: &'static str,
    flow_type: Type,
}

/// Return call-local values supplied by the active render-constructor ABI.
///
/// This is ordinary ABI metadata: `element` is not a Boon keyword, and other
/// render libraries may expose a different context name or no context at all.
fn render_call_context_surface(function: &str) -> Option<PreparedCallContextSurface> {
    render_constructor_kind(function)?;
    (function != "Scene/new").then(|| {
        let boolean = Type::VariantSet(
            vec![
                Variant::Tag("False".to_owned()),
                Variant::Tag("True".to_owned()),
            ]
            .into(),
        );
        PreparedCallContextSurface {
            name: "element",
            provider: "element",
            flow_type: Type::object(ObjectShape::from_ordered_fields(
                [
                    ("hovered".to_owned(), boolean.clone()),
                    ("focused".to_owned(), boolean.clone()),
                    ("pressed".to_owned(), boolean.clone()),
                    ("selected".to_owned(), boolean),
                ],
                true,
            )),
        }
    })
}

fn prepared_lexical_read_node(
    binding: &PreparedLexicalBinding,
    suffix: &[Box<str>],
) -> Result<
    (
        KernelOwnerNodeKind,
        Vec<(KernelOwnerEdgeRole, PreparedInputReference)>,
    ),
    String,
> {
    let fields = binding
        .prefix
        .iter()
        .map(String::as_str)
        .chain(suffix.iter().map(Box::<str>::as_ref))
        .collect::<Vec<_>>();
    match &binding.provider {
        PreparedLexicalProvider::Input(provider) => {
            let fields = fields
                .into_iter()
                .map(|field| field.to_owned().into_boxed_str())
                .collect();
            Ok((
                if let Some(pattern) = &binding.pattern {
                    KernelOwnerNodeKind::PatternRead {
                        pattern: pattern.clone(),
                        fields,
                    }
                } else if binding.directional {
                    KernelOwnerNodeKind::DerivedRead { fields }
                } else {
                    KernelOwnerNodeKind::LexicalRead { fields }
                },
                vec![(KernelOwnerEdgeRole::ReadProvider, provider.clone())],
            ))
        }
        PreparedLexicalProvider::Known(provider) => Ok((
            KernelOwnerNodeKind::Known(project_checked_type(provider, &fields)?),
            Vec::new(),
        )),
    }
}

fn project_checked_type(provider: &Type, fields: &[&str]) -> Result<Type, String> {
    let mut current = provider;
    for field in fields {
        let Type::Object(shape) = current else {
            return Err(format!(
                "ABI context projection `{}` crosses non-object type {current:?}",
                fields.join(".")
            ));
        };
        current = shape.fields.get(*field).ok_or_else(|| {
            format!(
                "ABI context projection `{}` has no field `{field}`",
                fields.join(".")
            )
        })?;
    }
    Ok(current.clone())
}

fn direct_output_callback_bindings(
    raw_expressions: &[&boon_syntax::AstExpr],
    caller_context_ordinal: Option<usize>,
    authoritative: &BTreeMap<String, AuthoritativeCallSurface>,
    callable_surfaces: &BTreeMap<String, Box<[CallableSurface]>>,
) -> Result<(BTreeSet<usize>, PreparedOutputBindingsByScope), String> {
    let expressions = raw_expressions
        .iter()
        .map(|expression| (expression.id, *expression))
        .collect::<BTreeMap<_, _>>();
    let mut inputs = BTreeSet::new();
    let mut scopes = BTreeMap::new();
    for expression in raw_expressions {
        let function = match &expression.kind {
            AstExprKind::Pipe { op, .. } => op,
            AstExprKind::Call { function, .. } => function,
            _ => continue,
        };
        let dynamic = dynamic_authoritative_call_surface(expression);
        let (kind, parameters, context_ordinal) =
            if let Some(surface) = authoritative.get(function).or(dynamic.as_ref()) {
                (surface.kind, surface.parameters.clone(), None)
            } else if let Some(candidates) = callable_surfaces.get(function) {
                let [surface] = candidates.as_ref() else {
                    continue;
                };
                (
                    KernelCallableKind::User,
                    compact_call_shape_parameters(surface)?,
                    surface
                        .context_ordinal
                        .map(|ordinal| checked_u32(ordinal, "call context ordinal"))
                        .transpose()?,
                )
            } else {
                continue;
            };
        let shape = compact_call_shape_input(KernelExpressionId(0), expression)?;
        let projection = project_kernel_call_shape(
            &shape,
            &KernelCallShapeResolution::Callable {
                kind,
                parameters: parameters.clone(),
                context_ordinal,
                caller_context_ordinal: caller_context_ordinal
                    .map(|ordinal| checked_u32(ordinal, "caller context ordinal"))
                    .transpose()?,
            },
        )
        .map_err(|error| error.to_string())?;
        if !projection.valid {
            continue;
        }
        let mut bindings = Vec::new();
        for matched in projection.matched_inputs.iter().filter(|matched| {
            parameters.iter().any(|parameter| {
                parameter.ordinal == matched.formal_ordinal
                    && parameter.kind == KernelParameterKind::Out
            })
        }) {
            let (kind, provider) = call_argument_value(expression, matched.source)?;
            if kind != AstCallArgKind::BareBinding {
                continue;
            }
            let name = expressions
                .get(&provider)
                .and_then(|expression| match &expression.kind {
                    AstExprKind::Identifier(name) => Some(name.clone()),
                    _ => None,
                })
                .ok_or_else(|| {
                    format!("OUT call `{function}` has a non-identifier bare binding")
                })?;
            if !inputs.insert(provider)
                || bindings
                    .iter()
                    .any(|existing: &PreparedOutputBinding| existing.name == name)
            {
                return Err(format!("OUT call `{function}` repeats binding `{name}`"));
            }
            let active_inputs = projection
                .matched_inputs
                .iter()
                .filter(|candidate| {
                    parameters.iter().any(|parameter| {
                        parameter.ordinal == candidate.formal_ordinal
                            && parameter.evaluation_scope
                                == KernelParameterEvaluationScope::Output {
                                    parameter_ordinal: matched.formal_ordinal,
                                }
                    })
                })
                .map(|candidate| {
                    call_argument_value(expression, candidate.source).map(|(_, value)| value)
                })
                .collect::<Result<Vec<_>, _>>()?;
            bindings.push(PreparedOutputBinding {
                formal_ordinal: matched.formal_ordinal,
                name,
                provider,
                active_inputs: active_inputs.into_boxed_slice(),
            });
        }
        if !bindings.is_empty() {
            bindings.sort_by_key(|binding| binding.formal_ordinal);
            scopes.insert(expression.id, bindings.into_boxed_slice());
        }
    }
    Ok((inputs, scopes))
}

fn match_pattern_binding_prefix(pattern: &AstMatchPattern, root: &str) -> Option<Vec<String>> {
    match pattern {
        AstMatchPattern::Binding { name } if name == root => Some(Vec::new()),
        AstMatchPattern::Tag { fields, .. } if fields.iter().any(|field| field == root) => {
            Some(vec![root.to_owned()])
        }
        AstMatchPattern::Wildcard
        | AstMatchPattern::Number { .. }
        | AstMatchPattern::Text { .. }
        | AstMatchPattern::Tag { .. }
        | AstMatchPattern::Binding { .. }
        | AstMatchPattern::Invalid { .. }
        | AstMatchPattern::Bits { .. } => None,
    }
}

fn pattern_variable_names(pattern: &AstMatchPattern) -> Vec<String> {
    match pattern {
        AstMatchPattern::Binding { name } => vec![name.clone()],
        AstMatchPattern::Tag { fields, .. } => fields.clone(),
        AstMatchPattern::Wildcard
        | AstMatchPattern::Number { .. }
        | AstMatchPattern::Text { .. }
        | AstMatchPattern::Invalid { .. }
        | AstMatchPattern::Bits { .. } => Vec::new(),
    }
}

fn direct_child_owner_result(
    view: UnitOwnerSyntaxView<'_>,
    root_statement: boon_syntax::UnitLocalStatementId,
) -> Result<Option<PreparedSyntheticResult>, String> {
    let mut names = BTreeSet::new();
    let mut children = Vec::new();
    for boundary in view
        .child_owners()
        .iter()
        .filter(|boundary| boundary.parent() == Some(root_statement))
    {
        let statement = view
            .statement_for_local(boundary.statement())
            .ok_or_else(|| "child owner boundary has no parser statement".to_owned())?;
        let name = match &statement.kind {
            AstStatementKind::Field { name } => Some(name),
            AstStatementKind::Source {
                field: Some(name), ..
            }
            | AstStatementKind::Hold {
                field: Some(name), ..
            }
            | AstStatementKind::List {
                field: Some(name), ..
            } => Some(name),
            _ => None,
        };
        if let Some(name) = name
            && !names.insert(name.clone())
        {
            return Err(format!(
                "structured field repeats direct child name `{name}`"
            ));
        }
        let child_owner = view
            .stable_check_owner_for_local_statement(boundary.statement())
            .ok_or_else(|| format!("structured child {name:?} has no stable owner"))?;
        if child_owner == view.stable_key() {
            return Err(format!(
                "structured child {name:?} did not cross an owner boundary"
            ));
        }
        children.push((
            name.cloned(),
            PreparedInputReference::OwnerResult(child_owner),
        ));
    }
    match children.as_slice() {
        [] => Ok(None),
        [(None, reference)] => Ok(Some(PreparedSyntheticResult::Alias(reference.clone()))),
        _ if children.iter().all(|(name, _)| name.is_some()) => {
            Ok(Some(PreparedSyntheticResult::Record(
                children
                    .into_iter()
                    .map(|(name, value)| PreparedRecordEntry::Field {
                        name: name.expect("all child names were checked"),
                        value,
                    })
                    .collect(),
            )))
        }
        _ => Ok(None),
    }
}

fn direct_structured_statement_records(
    view: UnitOwnerSyntaxView<'_>,
) -> Result<BTreeMap<usize, Vec<PreparedRecordEntry>>, String> {
    let expressions = view
        .expressions()
        .map(|expression| (expression.id, expression))
        .collect::<BTreeMap<_, _>>();
    let mut records = BTreeMap::new();
    let mut claimed = BTreeSet::new();
    for statement in view.statements() {
        let Some(direct) = statement.expr else {
            continue;
        };
        let mut delimiters = Vec::new();
        if expressions.get(&direct).is_some_and(|expression| {
            matches!(&expression.kind, AstExprKind::Delimiter)
                || matches!(&expression.kind, AstExprKind::Object(fields) if fields.is_empty())
        }) {
            delimiters.push(direct);
        }
        if let Some(expression) = expressions.get(&direct) {
            delimiters.extend(
                compact_ast_edges(&expression.kind, expression.linked_input)
                    .unwrap_or_default()
                    .into_iter()
                    .map(|(_, input)| input)
                    .filter(|input| {
                        expressions.get(input).is_some_and(|expression| {
                            matches!(&expression.kind, AstExprKind::Delimiter)
                                || matches!(&expression.kind, AstExprKind::Object(fields) if fields.is_empty())
                        })
                    }),
            );
        }
        if delimiters.is_empty() {
            continue;
        }
        let mut names = BTreeSet::new();
        let fields = statement
            .children
            .iter()
            .filter_map(|child| {
                let entry = match &child.kind {
                    AstStatementKind::Field { name } => Some(Some(name)),
                    AstStatementKind::Source {
                        field: Some(name), ..
                    }
                    | AstStatementKind::Hold {
                        field: Some(name), ..
                    }
                    | AstStatementKind::List {
                        field: Some(name), ..
                    } => Some(Some(name)),
                    AstStatementKind::Spread => Some(None),
                    _ => None,
                }?;
                let value = child.expr?;
                Some(match entry {
                    Some(name) => PreparedRecordEntry::Field {
                        name: name.clone(),
                        value: PreparedInputReference::Syntax(value),
                    },
                    None => PreparedRecordEntry::Spread {
                        value: PreparedInputReference::Syntax(value),
                    },
                })
            })
            .collect::<Vec<_>>();
        for field in &fields {
            let PreparedRecordEntry::Field { name, .. } = field else {
                continue;
            };
            if !names.insert(name.clone()) {
                return Err(format!(
                    "structured delimiter repeats direct field `{name}`"
                ));
            }
        }
        if fields.is_empty() {
            continue;
        }
        for delimiter in delimiters {
            if claimed.insert(delimiter) {
                records.insert(delimiter, fields.clone());
            }
        }
    }
    Ok(records)
}

fn direct_hold_update_expressions(
    view: UnitOwnerSyntaxView<'_>,
    hold: usize,
    expressions: &[&boon_syntax::AstExpr],
) -> Result<Vec<usize>, String> {
    let statement = view
        .statements()
        .find(|statement| statement.expr == Some(hold))
        .ok_or_else(|| format!("HOLD expression {hold} has no owning statement"))?;
    let mut updates = Vec::new();
    for child in &statement.children {
        let Some(update) = child.expr else {
            return Err("HOLD update statement has no direct expression".to_owned());
        };
        let expression = expressions
            .iter()
            .find(|expression| expression.id == update)
            .ok_or_else(|| format!("HOLD update expression {update} is not local"))?;
        if let AstExprKind::Latest { branches } = &expression.kind {
            updates.extend(branches.iter().copied());
        } else {
            updates.push(update);
        }
    }
    Ok(updates)
}

fn direct_view_source_payload_paths(
    expressions: &[&boon_syntax::AstExpr],
    stable_expressions: &[StableExpressionKey],
    local_by_syntax: &BTreeMap<usize, usize>,
    statement_roots: &[(usize, boon_syntax::StableStatementKey)],
) -> Result<BTreeMap<StableExpressionKey, String>, String> {
    fn visit(
        reference: usize,
        expressions: &[&boon_syntax::AstExpr],
        local_by_syntax: &BTreeMap<usize, usize>,
        prefix: &[String],
        projection: &mut Vec<String>,
        active: &mut BTreeSet<usize>,
        queries: &mut BTreeMap<usize, String>,
    ) -> Result<(), String> {
        let Some(index) = local_by_syntax.get(&reference).copied() else {
            return Ok(());
        };
        if !active.insert(index) {
            return Ok(());
        }
        let expression = expressions[index];
        if matches!(expression.kind, AstExprKind::Source) {
            let canonical_path = prefix
                .iter()
                .chain(projection.iter())
                .cloned()
                .collect::<Vec<_>>()
                .join(".");
            if !canonical_path.is_empty() {
                match queries.entry(index) {
                    std::collections::btree_map::Entry::Vacant(entry) => {
                        entry.insert(canonical_path);
                    }
                    std::collections::btree_map::Entry::Occupied(entry)
                        if entry.get() == &canonical_path => {}
                    std::collections::btree_map::Entry::Occupied(entry) => {
                        return Err(format!(
                            "source expression {} has conflicting stable paths `{}` and `{canonical_path}`",
                            expression.id,
                            entry.get()
                        ));
                    }
                }
            }
        }
        for (role, input) in source_ast_edges(expression)? {
            let projection_len = projection.len();
            if let KernelOwnerEdgeRole::RecordField {
                name,
                spread: false,
            } = role
            {
                projection.push(name.into());
            }
            visit(
                input,
                expressions,
                local_by_syntax,
                prefix,
                projection,
                active,
                queries,
            )?;
            projection.truncate(projection_len);
        }
        active.remove(&index);
        Ok(())
    }

    let mut by_index = BTreeMap::new();
    for (root, statement) in statement_roots {
        visit(
            *root,
            expressions,
            local_by_syntax,
            &statement_source_path_prefix(statement),
            &mut Vec::new(),
            &mut BTreeSet::new(),
            &mut by_index,
        )?;
    }
    by_index
        .into_iter()
        .map(|(index, path)| {
            stable_expressions
                .get(index)
                .cloned()
                .map(|expression| (expression, path))
                .ok_or_else(|| "source expression has no stable identity".to_owned())
        })
        .collect()
}

fn source_ast_edges(
    expression: &boon_syntax::AstExpr,
) -> Result<Vec<(KernelOwnerEdgeRole, usize)>, String> {
    match &expression.kind {
        AstExprKind::Identifier(_) | AstExprKind::Path(_) | AstExprKind::Drain { .. } => {
            Ok(Vec::new())
        }
        AstExprKind::Call { args, pass, .. } => Ok(args
            .iter()
            .map(|argument| (KernelOwnerEdgeRole::CollectionItem, argument.value))
            .chain(
                pass.iter()
                    .map(|pass| (KernelOwnerEdgeRole::CollectionItem, pass.value)),
            )
            .collect()),
        AstExprKind::Pipe {
            input,
            args,
            pass,
            arms,
            ..
        } => Ok(std::iter::once((
            KernelOwnerEdgeRole::CollectionItem,
            expression.linked_input.unwrap_or(*input),
        ))
        .chain(
            args.iter()
                .map(|argument| (KernelOwnerEdgeRole::CollectionItem, argument.value)),
        )
        .chain(
            pass.iter()
                .map(|pass| (KernelOwnerEdgeRole::CollectionItem, pass.value)),
        )
        .chain(
            arms.iter()
                .map(|arm| (KernelOwnerEdgeRole::CollectionItem, *arm)),
        )
        .collect()),
        AstExprKind::Block { bindings, result } => Ok(bindings
            .iter()
            .map(|binding| (KernelOwnerEdgeRole::CollectionItem, binding.value))
            .chain(
                result
                    .iter()
                    .map(|result| (KernelOwnerEdgeRole::BlockResult, *result)),
            )
            .collect()),
        kind => compact_ast_edges(kind, expression.linked_input),
    }
}

fn compact_expression_semantic_payload(kind: &AstExprKind) -> KernelExpressionSemanticPayload {
    match kind {
        AstExprKind::StringLiteral(value) | AstExprKind::TextLiteral(value) => {
            KernelExpressionSemanticPayload::Text(value.clone().into_boxed_str())
        }
        AstExprKind::TextTemplate { segments } => {
            let mut dynamic_ordinal = 0_u32;
            KernelExpressionSemanticPayload::TextTemplate(
                segments
                    .iter()
                    .map(|segment| match segment {
                        AstTextSegment::Static { value } => {
                            KernelTextTemplateSegment::Static(value.clone().into_boxed_str())
                        }
                        AstTextSegment::Dynamic { .. } => {
                            let ordinal = dynamic_ordinal;
                            dynamic_ordinal = dynamic_ordinal
                                .checked_add(1)
                                .expect("text template dynamic segment count exceeds u32");
                            KernelTextTemplateSegment::Dynamic(ordinal)
                        }
                    })
                    .collect::<Vec<_>>()
                    .into_boxed_slice(),
            )
        }
        AstExprKind::Number(literal) => ExactNumber::parse_strict(literal, None).map_or_else(
            |_| {
                KernelExpressionSemanticPayload::Invalid(
                    vec!["invalid_exact_number_literal".into()].into_boxed_slice(),
                )
            },
            KernelExpressionSemanticPayload::Number,
        ),
        AstExprKind::ByteLiteral { value, .. } => KernelExpressionSemanticPayload::Byte(*value),
        AstExprKind::BitsLiteral {
            width,
            radix,
            digits,
        } => Bits::parse_encoded(*width, *radix, digits).map_or_else(
            |_| {
                KernelExpressionSemanticPayload::Invalid(
                    vec!["invalid_bits_literal".into()].into_boxed_slice(),
                )
            },
            KernelExpressionSemanticPayload::Bits,
        ),
        AstExprKind::Hold { name, .. } => {
            KernelExpressionSemanticPayload::HoldName(name.clone().into_boxed_str())
        }
        AstExprKind::Unknown(tokens) => KernelExpressionSemanticPayload::Invalid(
            tokens
                .iter()
                .cloned()
                .map(String::into_boxed_str)
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        ),
        _ => KernelExpressionSemanticPayload::None,
    }
}

fn compact_ast_kind(
    kind: &AstExprKind,
    stable_key: &StableExpressionKey,
    source_paths: &BTreeMap<StableExpressionKey, String>,
    source_payloads: &BTreeMap<String, Type>,
) -> Result<KernelOwnerNodeKind, String> {
    Ok(match kind {
        AstExprKind::StringLiteral(_) | AstExprKind::TextLiteral(_) => KernelOwnerNodeKind::Text,
        AstExprKind::TextTemplate { .. } => KernelOwnerNodeKind::TextTemplate,
        AstExprKind::Number(_) => KernelOwnerNodeKind::Number,
        AstExprKind::ByteLiteral { .. } => KernelOwnerNodeKind::Byte,
        AstExprKind::BitsLiteral { width, .. } => KernelOwnerNodeKind::Bits(*width),
        AstExprKind::Tag(name) if name == "SKIP" => KernelOwnerNodeKind::Absent,
        AstExprKind::Tag(name) => KernelOwnerNodeKind::Tag(name.clone().into()),
        AstExprKind::Source => {
            let canonical_path = source_paths
                .get(stable_key)
                .ok_or_else(|| "SOURCE has no payload ABI query".to_owned())?;
            let payload = source_payloads
                .get(canonical_path)
                .ok_or_else(|| format!("SOURCE payload ABI `{canonical_path}` was not supplied"))?;
            if !type_is_recursively_closed(payload) {
                return Err(format!(
                    "SOURCE payload ABI `{canonical_path}` is not recursively closed: {payload:?}"
                ));
            }
            KernelOwnerNodeKind::Source(payload.clone())
        }
        AstExprKind::TaggedObject { tag, .. } => KernelOwnerNodeKind::Record {
            tag: Some(tag.clone().into()),
        },
        AstExprKind::Object(_) => KernelOwnerNodeKind::Record { tag: None },
        AstExprKind::Block { .. } => KernelOwnerNodeKind::Block,
        AstExprKind::ListLiteral { capacity, .. } => KernelOwnerNodeKind::Collection {
            kind: KernelCollectionKind::List,
            capacity: *capacity,
        },
        AstExprKind::BytesLiteral { size, .. } => KernelOwnerNodeKind::Collection {
            kind: KernelCollectionKind::Bytes,
            capacity: match size {
                boon_syntax::BytesSizeSyntax::Fixed(size) => Some(*size),
                boon_syntax::BytesSizeSyntax::Dynamic | boon_syntax::BytesSizeSyntax::Infer => None,
            },
        },
        AstExprKind::SetLiteral { .. } => KernelOwnerNodeKind::Collection {
            kind: KernelCollectionKind::Set,
            capacity: None,
        },
        AstExprKind::MapLiteral { .. } => KernelOwnerNodeKind::Collection {
            kind: KernelCollectionKind::Map,
            capacity: None,
        },
        AstExprKind::MapEntry { .. } => KernelOwnerNodeKind::MapEntry,
        AstExprKind::Draining { .. } => KernelOwnerNodeKind::Draining,
        AstExprKind::Hold { .. } => KernelOwnerNodeKind::Hold,
        AstExprKind::Latest { .. } => KernelOwnerNodeKind::Latest,
        AstExprKind::When { .. } => KernelOwnerNodeKind::When,
        AstExprKind::Pipe { op, arms, .. } if op == "WHILE" && !arms.is_empty() => {
            KernelOwnerNodeKind::When
        }
        AstExprKind::Then { .. } => KernelOwnerNodeKind::Then,
        AstExprKind::Infix { op, .. } => KernelOwnerNodeKind::Infix {
            operation: op.clone().into_boxed_str(),
        },
        AstExprKind::MatchArm { pattern, .. } => KernelOwnerNodeKind::MatchArm {
            pattern: compact_pattern(pattern),
        },
        AstExprKind::Arrow { .. } => KernelOwnerNodeKind::Arrow,
        AstExprKind::Unknown(_) => KernelOwnerNodeKind::Unknown,
        AstExprKind::Delimiter => KernelOwnerNodeKind::Delimiter,
        unsupported => return Err(format!("unsupported owner node {unsupported:?}")),
    })
}

fn render_constructor_kind(function: &str) -> Option<KernelRenderConstructorKind> {
    Some(match function {
        "Document/new" => KernelRenderConstructorKind::Fixed("Document".into()),
        "Element/container" => KernelRenderConstructorKind::Fixed("Stack".into()),
        "Element/stripe" => KernelRenderConstructorKind::StripeDirection,
        "Element/text" | "Element/label" | "Element/paragraph" | "Element/link" => {
            KernelRenderConstructorKind::Fixed("Text".into())
        }
        "Element/button" => KernelRenderConstructorKind::Fixed("Button".into()),
        "Element/checkbox" => KernelRenderConstructorKind::Fixed("Checkbox".into()),
        "Element/text_input" => KernelRenderConstructorKind::Fixed("TextInput".into()),
        "Element/program" => KernelRenderConstructorKind::Fixed("EmbeddedProgram".into()),
        "Element/embedded_media" => KernelRenderConstructorKind::Fixed("EmbeddedMedia".into()),
        "Element/map" => KernelRenderConstructorKind::Fixed("MapViewport".into()),
        "Scene/new" => KernelRenderConstructorKind::Fixed("Scene".into()),
        "Scene/Element/stripe" => KernelRenderConstructorKind::StripeDirection,
        "Scene/Element/block" => KernelRenderConstructorKind::Fixed("Block".into()),
        "Scene/Element/text" => KernelRenderConstructorKind::Fixed("Text".into()),
        "Scene/Element/label" => KernelRenderConstructorKind::Fixed("Label".into()),
        "Scene/Element/text_input" => KernelRenderConstructorKind::Fixed("TextInput".into()),
        "Scene/Element/button" => KernelRenderConstructorKind::Fixed("Button".into()),
        "Scene/Element/checkbox" => KernelRenderConstructorKind::Fixed("Checkbox".into()),
        "Scene/Element/paragraph" => KernelRenderConstructorKind::Fixed("Paragraph".into()),
        "Scene/Element/link" => KernelRenderConstructorKind::Fixed("Link".into()),
        "Scene/Element/program" => KernelRenderConstructorKind::Fixed("EmbeddedProgram".into()),
        "Scene/Element/embedded_media" => {
            KernelRenderConstructorKind::Fixed("EmbeddedMedia".into())
        }
        "Scene/Element/map" => KernelRenderConstructorKind::Fixed("MapViewport".into()),
        _ => return None,
    })
}

fn is_authoritative_callable_name(
    shapes: &BTreeMap<String, AuthoritativeCallSurface>,
    function: &str,
) -> bool {
    shapes.contains_key(function) || is_registered_kernel_host_effect(function)
}

fn pure_builtin_kind(function: &str) -> Option<KernelPureBuiltinKind> {
    Some(match function {
        "Text/empty" | "Text/space" => KernelPureBuiltinKind::TextConstant,
        "Text/trim" | "Text/to_lowercase" | "Text/to_uppercase" => {
            KernelPureBuiltinKind::TextTransform
        }
        "Text/slice" => KernelPureBuiltinKind::TextSlice,
        "Text/length" => KernelPureBuiltinKind::TextLength,
        "Text/concat" => KernelPureBuiltinKind::TextConcat,
        "Text/time_range_label" => KernelPureBuiltinKind::TextConcat,
        "Text/is_empty" | "Text/is_not_empty" | "Text/starts_with" | "Text/contains"
        | "Text/all_chars_in" => KernelPureBuiltinKind::TextPredicate,
        "Text/to_number" => KernelPureBuiltinKind::TextToNumber,
        "Number/to_text" | "Number/to_ascii_text" | "Number/to_codepoint_text" => {
            KernelPureBuiltinKind::NumberToText
        }
        "Number/add" | "Number/subtract" | "Number/min" | "Number/max" | "Number/bit_width"
        | "Number/ceil" | "Number/floor" | "Number/truncate" | "Number/interpolate" => {
            KernelPureBuiltinKind::NumberMath
        }
        "Number/round" => KernelPureBuiltinKind::NumberRound,
        "Number/project_offset" | "Number/project_time" | "Number/project_width" => {
            KernelPureBuiltinKind::NumberProjection
        }
        "Bool/not" | "Bool/and" | "Bool/or" | "Bool/toggle" => KernelPureBuiltinKind::Boolean,
        "Light/directional" | "Light/ambient" | "Light/spot" => {
            KernelPureBuiltinKind::RecordConstructor
        }
        "List/count" | "List/length" | "List/sum" => KernelPureBuiltinKind::ListLength,
        "List/is_not_empty" | "List/any" | "List/every" => KernelPureBuiltinKind::ListPredicate,
        "List/filter" | "List/retain" | "List/remove" => KernelPureBuiltinKind::ListFilter,
        "List/map" => KernelPureBuiltinKind::ListMap,
        "List/find" => KernelPureBuiltinKind::ListFind,
        "List/latest" => KernelPureBuiltinKind::ListLatest,
        "List/append" => KernelPureBuiltinKind::ListAppend,
        "List/sort_by" | "List/then_by" => KernelPureBuiltinKind::ListSort,
        "List/chunk" => KernelPureBuiltinKind::ListChunk,
        "Text/join" => KernelPureBuiltinKind::TextJoin,
        "Field/color" => KernelPureBuiltinKind::FieldColor,
        _ => return None,
    })
}

fn compact_pattern(pattern: &AstMatchPattern) -> KernelPattern {
    match pattern {
        AstMatchPattern::Wildcard => KernelPattern::Wildcard,
        AstMatchPattern::Number { .. } => KernelPattern::Number,
        AstMatchPattern::Text { .. } => KernelPattern::Text,
        AstMatchPattern::Bits { width, .. } => KernelPattern::Bits { width: *width },
        AstMatchPattern::Tag { name, fields } => KernelPattern::Tag {
            name: name.clone().into_boxed_str(),
            fields: fields
                .iter()
                .cloned()
                .map(String::into_boxed_str)
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        },
        AstMatchPattern::Binding { name } => KernelPattern::Binding {
            name: name.clone().into_boxed_str(),
        },
        AstMatchPattern::Invalid { .. } => KernelPattern::Invalid,
    }
}

fn compact_ast_edges(
    kind: &AstExprKind,
    linked_input: Option<usize>,
) -> Result<Vec<(KernelOwnerEdgeRole, usize)>, String> {
    let edges = match kind {
        AstExprKind::StringLiteral(_)
        | AstExprKind::TextLiteral(_)
        | AstExprKind::Number(_)
        | AstExprKind::ByteLiteral { .. }
        | AstExprKind::BitsLiteral { .. }
        | AstExprKind::Tag(_)
        | AstExprKind::Source
        | AstExprKind::Unknown(_)
        | AstExprKind::Delimiter => Vec::new(),
        AstExprKind::TextTemplate { segments } => segments
            .iter()
            .filter_map(|segment| match segment {
                AstTextSegment::Static { .. } => None,
                AstTextSegment::Dynamic { value } => {
                    Some((KernelOwnerEdgeRole::TextDynamic, *value))
                }
            })
            .collect(),
        AstExprKind::TaggedObject { fields, .. } | AstExprKind::Object(fields) => fields
            .iter()
            .map(|field| {
                (
                    KernelOwnerEdgeRole::RecordField {
                        name: field.name.clone().into(),
                        spread: field.spread,
                    },
                    field.value,
                )
            })
            .collect(),
        AstExprKind::Block { bindings, result } => {
            let _ = bindings;
            if result.is_none() {
                return Err("empty BLOCK is not in the first dense slice".to_owned());
            }
            result
                .iter()
                .map(|result| (KernelOwnerEdgeRole::BlockResult, *result))
                .collect()
        }
        AstExprKind::ListLiteral { items, .. }
        | AstExprKind::BytesLiteral { items, .. }
        | AstExprKind::SetLiteral { items } => items
            .iter()
            .map(|item| (KernelOwnerEdgeRole::CollectionItem, *item))
            .collect(),
        AstExprKind::MapEntry { key, value } => vec![
            (KernelOwnerEdgeRole::MapKey, *key),
            (KernelOwnerEdgeRole::MapValue, *value),
        ],
        AstExprKind::MapLiteral { entries } => entries
            .iter()
            .map(|entry| (KernelOwnerEdgeRole::MapEntry, *entry))
            .collect(),
        AstExprKind::Draining { input } => vec![(
            KernelOwnerEdgeRole::DrainingInput,
            linked_input.unwrap_or(*input),
        )],
        AstExprKind::Hold { initial, .. } => {
            vec![(
                KernelOwnerEdgeRole::HoldInitial,
                linked_input.unwrap_or(*initial),
            )]
        }
        AstExprKind::Latest { branches } => branches
            .iter()
            .map(|branch| (KernelOwnerEdgeRole::LatestBranch, *branch))
            .collect(),
        AstExprKind::When { input, arms }
        | AstExprKind::Pipe {
            input, op: _, arms, ..
        } => std::iter::once((
            KernelOwnerEdgeRole::WhenInput,
            linked_input.unwrap_or(*input),
        ))
        .chain(arms.iter().map(|arm| (KernelOwnerEdgeRole::WhenArm, *arm)))
        .collect(),
        AstExprKind::Then { input, output } => std::iter::once((
            KernelOwnerEdgeRole::ThenInput,
            linked_input.unwrap_or(*input),
        ))
        .chain(
            output
                .iter()
                .map(|output| (KernelOwnerEdgeRole::ThenOutput, *output)),
        )
        .collect(),
        AstExprKind::Infix { left, right, .. } => vec![
            (
                KernelOwnerEdgeRole::InfixLeft,
                linked_input.unwrap_or(*left),
            ),
            (KernelOwnerEdgeRole::InfixRight, *right),
        ],
        AstExprKind::MatchArm { output, .. } => output
            .iter()
            .map(|output| (KernelOwnerEdgeRole::MatchOutput, *output))
            .collect(),
        AstExprKind::Arrow { output, .. } => output
            .iter()
            .map(|output| (KernelOwnerEdgeRole::ArrowOutput, *output))
            .collect(),
        unsupported => return Err(format!("unsupported owner node {unsupported:?}")),
    };
    Ok(edges)
}

fn statement_source_path_prefix(statement: &boon_syntax::StableStatementKey) -> Vec<String> {
    let mut prefix = statement
        .route
        .owner
        .iter()
        .flat_map(|owner| owner.segments())
        .filter_map(|segment| {
            let name = segment.names.first()?;
            Some(match segment.kind {
                UnitItemKind::Function => format!("FUNCTION:{name}"),
                UnitItemKind::Field
                | UnitItemKind::Source
                | UnitItemKind::Hold
                | UnitItemKind::List => name.clone(),
            })
        })
        .collect::<Vec<_>>();
    prefix.extend(
        statement
            .route
            .statement_route
            .iter()
            .filter_map(|segment| {
                let name = segment.names.first()?;
                Some(match segment.kind {
                    StableStatementKind::Function => format!("FUNCTION:{name}"),
                    StableStatementKind::Field
                    | StableStatementKind::Source
                    | StableStatementKind::Hold
                    | StableStatementKind::List => name.clone(),
                    StableStatementKind::Block
                    | StableStatementKind::Spread
                    | StableStatementKind::Expression => return None,
                })
            }),
    );
    prefix
}

fn checked_kernel_expression(expression: usize) -> Result<KernelExpressionId, String> {
    checked_u32(expression, "kernel owner expression namespace").map(KernelExpressionId)
}

fn checked_u32(value: usize, context: &str) -> Result<u32, String> {
    u32::try_from(value).map_err(|_| format!("{context} exceeds u32"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use boon_checked::{
        CheckedDeclarationKind, CheckedExpressionKind, CheckedProgramFields, CheckedStatementKind,
        DeclId, ObjectShape, SharedVariantSet, TypeVar, Variant,
    };
    use boon_compiler_kernel::{KernelTypeParameterId, derive_kernel_call_type_substitutions};
    use boon_parser::{parse_project_syntax, parse_source};
    use std::collections::BTreeMap;
    use std::fs;
    use std::path::Path;
    use std::time::Instant;

    fn alpha_normalize_owner(
        result: &FlowType,
        expressions: impl IntoIterator<Item = FlowType>,
    ) -> (FlowType, Vec<FlowType>) {
        fn normalize_flow(flow: &FlowType, variables: &mut BTreeMap<TypeVar, TypeVar>) -> FlowType {
            FlowType {
                mode: flow.mode,
                ty: normalize_type(&flow.ty, variables),
            }
        }
        fn normalize_shape(
            shape: &ObjectShape,
            variables: &mut BTreeMap<TypeVar, TypeVar>,
        ) -> ObjectShape {
            ObjectShape {
                fields: shape
                    .fields
                    .iter()
                    .map(|(name, ty)| (name.clone(), normalize_type(ty, variables)))
                    .collect(),
                // Open objects are structural requirement rows. Their field
                // insertion order can reflect solver activation epochs (for
                // example, whether a collection provider arrives before a
                // nested callback requirement) and is not authored record
                // order. Canonicalize only those open rows for differential
                // comparison; closed object and tagged-record order remains
                // an exact language/output contract.
                field_order: if shape.open {
                    shape.fields.keys().cloned().collect()
                } else {
                    shape.field_order.clone()
                },
                open: shape.open,
            }
        }
        fn normalize_type(ty: &Type, variables: &mut BTreeMap<TypeVar, TypeVar>) -> Type {
            match ty {
                Type::Var(variable) => {
                    let next = TypeVar(
                        u32::try_from(variables.len()).expect("oracle alpha count exceeds u32"),
                    );
                    Type::Var(*variables.entry(*variable).or_insert(next))
                }
                Type::VariantSet(variants) => Type::VariantSet(SharedVariantSet::new(
                    variants
                        .iter()
                        .map(|variant| match variant {
                            Variant::Tag(tag) => Variant::Tag(tag.clone()),
                            Variant::Tagged { tag, fields } => {
                                Variant::tagged(tag.clone(), normalize_shape(fields, variables))
                            }
                        })
                        .collect(),
                )),
                Type::Object(shape) => Type::object(normalize_shape(shape, variables)),
                Type::List(item) => Type::List(Type::shared(normalize_type(item, variables))),
                Type::Function { args, result } => Type::Function {
                    args: args
                        .iter()
                        .map(|arg| normalize_type(arg, variables))
                        .collect(),
                    result: Box::new(normalize_flow(result, variables)),
                },
                Type::Union(members) => Type::Union(
                    members
                        .iter()
                        .map(|member| normalize_type(member, variables))
                        .collect(),
                ),
                Type::Map { key, value } => Type::Map {
                    key: Box::new(normalize_type(key, variables)),
                    value: Box::new(normalize_type(value, variables)),
                },
                Type::Set(item) => Type::Set(Type::shared(normalize_type(item, variables))),
                Type::Text
                | Type::Number
                | Type::Bytes(_)
                | Type::Absent
                | Type::RenderContract
                | Type::UnresolvedShape { .. }
                | Type::Unknown
                | Type::Bits { .. } => ty.clone(),
            }
        }

        let mut variables = BTreeMap::new();
        let result = normalize_flow(result, &mut variables);
        let expressions = expressions
            .into_iter()
            .map(|flow| normalize_flow(&flow, &mut variables))
            .collect();
        (result, expressions)
    }

    #[test]
    fn differential_canonicalizes_only_open_object_field_order() {
        let object = |order: [&str; 2], open| FlowType {
            mode: FlowMode::Continuous,
            ty: Type::object(ObjectShape::from_ordered_fields(
                order.into_iter().map(|name| (name.to_owned(), Type::Text)),
                open,
            )),
        };
        let neutral = FlowType {
            mode: FlowMode::Absent,
            ty: Type::Absent,
        };

        let open_left = alpha_normalize_owner(&neutral, [object(["left", "right"], true)]).1;
        let open_right = alpha_normalize_owner(&neutral, [object(["right", "left"], true)]).1;
        assert_eq!(open_left, open_right);

        let closed_left = alpha_normalize_owner(&neutral, [object(["left", "right"], false)]).1;
        let closed_right = alpha_normalize_owner(&neutral, [object(["right", "left"], false)]).1;
        assert_ne!(closed_left, closed_right);
    }

    fn owner_mismatch(
        owner: &KernelOwnerOracleEntry,
        checked_by_stable_key: &BTreeMap<StableExpressionKey, FlowType>,
        checked: &CheckedProgramFields,
        project: &ProjectSyntaxSnapshot,
        context: &str,
    ) -> Option<String> {
        // The old checked image loses multiline delimiter structure and the
        // fields contributed by record spreads at their local owner boundary.
        // Keep only those exact local cones out of the differential; concrete
        // downstream calls and all unaffected expressions remain oracle checks.
        let compare_result = owner.result_expression.as_ref().is_none_or(|result| {
            !owner.structured_delimiter_dependents.contains(result)
                && !owner.record_spread_dependents.contains(result)
        });
        let generic_selector_result = owner
            .result_expression
            .as_ref()
            .is_some_and(|result| owner.generic_selector_dependents.contains(result));
        let legacy_no_element_result = owner
            .result_expression
            .as_ref()
            .is_some_and(|result| owner.legacy_no_element_dependents.contains(result));
        // `DefinitionArtifact.result` is the public owner interface. A checked
        // body/root expression may legitimately be a narrower occurrence
        // surface (notably a record containing the initial epoch of a HOLD),
        // so compare that row below with the other expressions rather than
        // substituting it for declaration authority here.
        let current_result = if !compare_result {
            owner.result.clone()
        } else {
            let current = checked_public_owner_result(checked, project, &owner.owner)
                .or_else(|| {
                    owner
                        .result_expression
                        .as_ref()
                        .and_then(|result| checked_by_stable_key.get(result))
                        .cloned()
                })
                .unwrap_or_else(|| panic!("{context} has no current public owner result"));
            checked_public_child_composed_result(owner, current, checked, project, context)
        };
        let compared = owner
            .expressions
            .iter()
            .filter(|(stable_key, _)| {
                !owner.generic_formal_reads.contains(stable_key)
                    && !owner.detached_generic_reads.contains(stable_key)
                    && !owner.structured_delimiter_dependents.contains(stable_key)
                    && !owner.record_spread_dependents.contains(stable_key)
            })
            .map(|(stable_key, flow)| {
                let current = if owner.result_expression.as_ref() == Some(stable_key)
                    && !owner.public_child_owner_fields.is_empty()
                {
                    current_result.clone()
                } else {
                    checked_by_stable_key
                        .get(stable_key)
                        .unwrap_or_else(|| {
                            panic!("{context} has no current expression {stable_key:#?}")
                        })
                        .clone()
                };
                (
                    flow.clone(),
                    current,
                    owner.generic_selector_dependents.contains(stable_key),
                    owner.legacy_no_element_dependents.contains(stable_key),
                    owner.legacy_source_container_modes.contains(stable_key),
                )
            })
            .collect::<Vec<_>>();
        let mut kernel_expressions = Vec::with_capacity(compared.len());
        let mut current_expressions = Vec::with_capacity(compared.len());
        let mut generic_selector_expressions = Vec::with_capacity(compared.len());
        let mut legacy_no_element_expressions = Vec::with_capacity(compared.len());
        let mut legacy_source_container_expressions = Vec::with_capacity(compared.len());
        let mut compared_keys = Vec::with_capacity(compared.len());
        for (kernel, current, generic_selector, legacy_no_element, legacy_source_container) in
            compared
        {
            kernel_expressions.push(kernel);
            current_expressions.push(current);
            generic_selector_expressions.push(generic_selector);
            legacy_no_element_expressions.push(legacy_no_element);
            legacy_source_container_expressions.push(legacy_source_container);
        }
        compared_keys.extend(owner.expressions.iter().filter_map(|(stable_key, _)| {
            (!owner.generic_formal_reads.contains(stable_key)
                && !owner.detached_generic_reads.contains(stable_key)
                && !owner.structured_delimiter_dependents.contains(stable_key)
                && !owner.record_spread_dependents.contains(stable_key))
            .then_some(stable_key)
        }));
        let (kernel_result, mut kernel_expressions) =
            alpha_normalize_owner(&owner.result, kernel_expressions);
        let (current_result, mut current_expressions) =
            alpha_normalize_owner(&current_result, current_expressions);
        let result_exact = kernel_result == current_result;
        let result_matches = result_exact
            || flow_matches_current_or_legacy_render_projection(&kernel_result, &current_result)
            || (owner.exported_as_public_child
                && legacy_public_child_narrowing_matches(&kernel_result, &current_result))
            || (generic_selector_result
                && legacy_generic_selector_member_matches(&kernel_result, &current_result))
            || (legacy_no_element_result
                && legacy_no_element_widening_matches(&kernel_result, &current_result));
        if compare_result && !result_matches {
            return Some(format!(
                "{context} owner result mismatch (direct public child count {}): {}",
                owner.public_child_owner_fields.len(),
                first_flow_difference(&kernel_result, &current_result)
            ));
        }
        if !compare_result || !result_exact {
            // A known lossy legacy result (for example a kind-only render
            // surface) does not expose the same alpha namespace as the dense
            // result. Re-normalize only the still-comparable expression rows.
            // Generic-selector and legacy UI cones can themselves contain a
            // different number of schematic alphas, so isolate those rows;
            // correlations across every strict row remain shared.
            let isolated = generic_selector_expressions
                .iter()
                .zip(&legacy_no_element_expressions)
                .map(|(generic, no_element)| *generic || *no_element)
                .collect::<Vec<_>>();
            (kernel_expressions, current_expressions) = alpha_normalize_expression_partitions(
                kernel_expressions,
                current_expressions,
                &isolated,
            );
        }
        if kernel_expressions.len() != current_expressions.len() {
            return Some(format!(
                "{context} expression count mismatch: kernel={} current={}",
                kernel_expressions.len(),
                current_expressions.len(),
            ));
        }
        for (index, (kernel, current)) in kernel_expressions
            .iter()
            .zip(&current_expressions)
            .enumerate()
        {
            let matches = flow_matches_current_or_legacy_render_projection(kernel, current)
                || (generic_selector_expressions[index]
                    && legacy_generic_selector_member_matches(kernel, current))
                || (legacy_no_element_expressions[index]
                    && legacy_no_element_widening_matches(kernel, current))
                || (legacy_source_container_expressions[index]
                    && legacy_source_container_mode_matches(kernel, current));
            if !matches {
                return Some(format!(
                    "{context} expression {index} ({:?}) mismatch: {}",
                    compared_keys[index],
                    first_flow_difference(kernel, current)
                ));
            }
        }
        None
    }

    fn alpha_normalize_expression_partitions(
        kernel: Vec<FlowType>,
        current: Vec<FlowType>,
        isolated: &[bool],
    ) -> (Vec<FlowType>, Vec<FlowType>) {
        assert_eq!(kernel.len(), current.len());
        assert_eq!(kernel.len(), isolated.len());
        let neutral = FlowType {
            mode: FlowMode::Absent,
            ty: Type::Absent,
        };
        let strict_indices = isolated
            .iter()
            .enumerate()
            .filter_map(|(index, isolated)| (!isolated).then_some(index))
            .collect::<Vec<_>>();
        let strict_kernel = strict_indices
            .iter()
            .map(|index| kernel[*index].clone())
            .collect::<Vec<_>>();
        let strict_current = strict_indices
            .iter()
            .map(|index| current[*index].clone())
            .collect::<Vec<_>>();
        let strict_kernel = alpha_normalize_owner(&neutral, strict_kernel).1;
        let strict_current = alpha_normalize_owner(&neutral, strict_current).1;
        let mut normalized_kernel = kernel;
        let mut normalized_current = current;
        for ((index, kernel), current) in strict_indices
            .into_iter()
            .zip(strict_kernel)
            .zip(strict_current)
        {
            normalized_kernel[index] = kernel;
            normalized_current[index] = current;
        }
        for (index, isolated) in isolated.iter().copied().enumerate() {
            if !isolated {
                continue;
            }
            normalized_kernel[index] =
                alpha_normalize_owner(&neutral, vec![normalized_kernel[index].clone()])
                    .1
                    .into_iter()
                    .next()
                    .expect("one isolated kernel expression");
            normalized_current[index] =
                alpha_normalize_owner(&neutral, vec![normalized_current[index].clone()])
                    .1
                    .into_iter()
                    .next()
                    .expect("one isolated current expression");
        }
        (normalized_kernel, normalized_current)
    }

    fn assert_owner_matches_current(
        owner: &KernelOwnerOracleEntry,
        checked_by_stable_key: &BTreeMap<StableExpressionKey, FlowType>,
        checked: &CheckedProgramFields,
        project: &ProjectSyntaxSnapshot,
        context: &str,
    ) {
        if let Some(mismatch) =
            owner_mismatch(owner, checked_by_stable_key, checked, project, context)
        {
            panic!("{mismatch}");
        }
    }

    fn checked_callable_owners(
        checked: &CheckedProgramFields,
        project: &ProjectSyntaxSnapshot,
    ) -> BTreeMap<DeclId, StableCheckOwnerKey> {
        project
            .item_index()
            .owners()
            .filter(|entry| entry.kind == UnitItemKind::Function)
            .filter_map(|entry| {
                let statement = checked
                    .statements
                    .get(project.statement_slot(entry.statement_id)?)?;
                let CheckedStatementKind::Function { declaration } = statement.kind else {
                    return None;
                };
                Some((
                    declaration,
                    StableCheckOwnerKey::Item(entry.owner_key.clone()),
                ))
            })
            .collect()
    }

    fn checked_callable_interface(
        checked: &CheckedProgramFields,
        project: &ProjectSyntaxSnapshot,
        owner: &StableCheckOwnerKey,
    ) -> Option<(Box<[FlowType]>, FlowType)> {
        let StableCheckOwnerKey::Item(owner) = owner else {
            return None;
        };
        let entry = project
            .item_index()
            .owners()
            .find(|entry| entry.owner_key == *owner && entry.kind == UnitItemKind::Function)?;
        let statement = checked
            .statements
            .get(project.statement_slot(entry.statement_id)?)?;
        let CheckedStatementKind::Function { declaration } = statement.kind else {
            return None;
        };
        let signature = checked
            .callables
            .iter()
            .find(|signature| signature.decl_id == declaration)?;
        let mut parameters = signature.parameters.iter().collect::<Vec<_>>();
        parameters.sort_unstable_by_key(|parameter| parameter.ordinal);
        let mut formals = parameters
            .into_iter()
            .map(|parameter| parameter.flow_type.clone())
            .collect::<Vec<_>>();
        if let Some(context) = signature
            .context_formal
            .and_then(|formal| checked.context_formal(formal))
        {
            formals.push(context.scheme.flow_type.clone());
        }
        Some((formals.into_boxed_slice(), signature.result.clone()))
    }

    fn alpha_normalize_callable_surface(
        formals: &[FlowType],
        result: &FlowType,
    ) -> (Box<[FlowType]>, FlowType) {
        let neutral = FlowType {
            mode: FlowMode::Absent,
            ty: Type::Absent,
        };
        let (_, mut surface) = alpha_normalize_owner(
            &neutral,
            formals
                .iter()
                .cloned()
                .chain(std::iter::once(result.clone())),
        );
        let result = surface
            .pop()
            .expect("callable surface always contains its result");
        (surface.into_boxed_slice(), result)
    }

    fn callable_interface_mismatches(
        report: &KernelOwnerOracleReport,
        checked: &CheckedProgramFields,
        project: &ProjectSyntaxSnapshot,
    ) -> Vec<String> {
        let mut mismatches = Vec::new();
        for owner in &report.supported {
            let Some((checked_formals, checked_result)) =
                checked_callable_interface(checked, project, &owner.owner)
            else {
                if !owner.formals.is_empty() {
                    mismatches.push(format!(
                        "kernel owner {:?} publishes {} callable formals without a checked callable interface",
                        owner.owner,
                        owner.formals.len()
                    ));
                }
                continue;
            };
            let (kernel_formals, _) =
                alpha_normalize_callable_surface(&owner.formals, &owner.result);
            let (checked_formals, _) =
                alpha_normalize_callable_surface(&checked_formals, &checked_result);
            if kernel_formals.len() != checked_formals.len() {
                mismatches.push(format!(
                    "kernel owner {:?} callable formal count differs from checked: kernel={} checked={}",
                    owner.owner,
                    kernel_formals.len(),
                    checked_formals.len()
                ));
                continue;
            }
            for (ordinal, (kernel, checked)) in
                kernel_formals.iter().zip(&checked_formals).enumerate()
            {
                let compatible = kernel.mode == checked.mode
                    && (legacy_generic_selector_type_matches(&kernel.ty, &checked.ty)
                        || legacy_generic_selector_type_matches(&checked.ty, &kernel.ty));
                if !compatible && owner.generic_selector_dependents.is_empty() {
                    mismatches.push(format!(
                        "kernel owner {:?} callable formal {ordinal} is incompatible with checked: kernel={kernel:?} checked={checked:?}",
                        owner.owner
                    ));
                }
            }
        }
        mismatches
    }

    fn collect_callable_type_parameter_ids(
        ty: &Type,
        parameters: &mut BTreeMap<boon_checked::TypeVar, KernelTypeParameterId>,
    ) {
        match ty {
            Type::Var(variable) => {
                let next = KernelTypeParameterId(
                    u32::try_from(parameters.len())
                        .expect("checked callable type-parameter count exceeds u32"),
                );
                parameters.entry(*variable).or_insert(next);
            }
            Type::Object(shape) => {
                for (_, field) in shape.ordered_fields() {
                    collect_callable_type_parameter_ids(field, parameters);
                }
            }
            Type::List(item) | Type::Set(item) => {
                collect_callable_type_parameter_ids(item, parameters);
            }
            Type::Map { key, value } => {
                collect_callable_type_parameter_ids(key, parameters);
                collect_callable_type_parameter_ids(value, parameters);
            }
            Type::Function { args, result } => {
                for argument in args {
                    collect_callable_type_parameter_ids(argument, parameters);
                }
                collect_callable_type_parameter_ids(&result.ty, parameters);
            }
            Type::VariantSet(variants) => {
                for variant in variants {
                    if let Variant::Tagged { fields, .. } = variant {
                        for (_, field) in fields.ordered_fields() {
                            collect_callable_type_parameter_ids(field, parameters);
                        }
                    }
                }
            }
            Type::Union(members) => {
                for member in members {
                    collect_callable_type_parameter_ids(member, parameters);
                }
            }
            Type::Text
            | Type::Number
            | Type::Bytes(_)
            | Type::Bits { .. }
            | Type::Absent
            | Type::RenderContract
            | Type::UnresolvedShape { .. }
            | Type::Unknown => {}
        }
    }

    fn checked_call_type_substitutions(
        checked: &CheckedProgramFields,
        call: &boon_checked::CheckedCall,
        target_formals: &[FlowType],
        target_result: &FlowType,
    ) -> Option<Box<[KernelCallTypeSubstitution]>> {
        let signature = checked
            .callables
            .iter()
            .find(|signature| signature.decl_id == call.callable)?;
        let mut parameters = signature.parameters.iter().collect::<Vec<_>>();
        parameters.sort_unstable_by_key(|parameter| parameter.ordinal);
        let mut target_formals = target_formals.to_vec();
        let mut target_result = target_result.clone();

        let mut actuals = Vec::new();
        for entry in &call.entries {
            let boon_checked::CheckedCallEntry::Input { formal, value, .. } = entry else {
                continue;
            };
            let parameter = parameters
                .iter()
                .find(|parameter| parameter.decl_id == *formal)?;
            let actual = checked
                .expressions
                .get(value.0 as usize)?
                .flow_type
                .ty
                .clone();
            actuals.push((u32::try_from(parameter.ordinal).ok()?, actual));
        }
        let context_ordinal = u32::try_from(parameters.len()).ok()?;
        if let Some((value, _)) = call.context_binding.explicit() {
            let actual = checked
                .expressions
                .get(value.0 as usize)?
                .flow_type
                .ty
                .clone();
            actuals.push((context_ordinal, actual));
        } else if let Some(formal) = call.context_binding.inherited()
            && let Some(actual) = checked.context_formal(formal)
        {
            actuals.push((context_ordinal, actual.scheme.flow_type.ty.clone()));
        }

        // Checked scheme ordinals are definition-local and can numerically
        // collide across caller/callee rows. Isolate both namespaces before
        // asking the permanent kernel for its canonical substitution product.
        let mut target_parameters = BTreeMap::new();
        for formal in &target_formals {
            collect_callable_type_parameter_ids(&formal.ty, &mut target_parameters);
        }
        collect_callable_type_parameter_ids(&target_result.ty, &mut target_parameters);
        let target_replacements = target_parameters
            .iter()
            .map(|(variable, parameter)| (*variable, Type::Var(boon_checked::TypeVar(parameter.0))))
            .collect::<BTreeMap<_, _>>();
        for formal in &mut target_formals {
            formal.ty =
                boon_checked::apply_checked_type_environment(&formal.ty, &target_replacements);
        }
        target_result.ty =
            boon_checked::apply_checked_type_environment(&target_result.ty, &target_replacements);

        let mut actual_parameters = BTreeMap::new();
        for (_, actual) in &actuals {
            collect_callable_type_parameter_ids(actual, &mut actual_parameters);
        }
        let actual_replacements = actual_parameters
            .iter()
            .map(|(variable, parameter)| {
                (
                    *variable,
                    Type::Var(boon_checked::TypeVar(
                        u32::try_from(target_parameters.len())
                            .expect("target parameter count exceeds u32")
                            .saturating_add(parameter.0),
                    )),
                )
            })
            .collect::<BTreeMap<_, _>>();
        for (_, actual) in &mut actuals {
            *actual = boon_checked::apply_checked_type_environment(actual, &actual_replacements);
        }

        let mut substitutions =
            derive_kernel_call_type_substitutions(&target_formals, &target_result, &actuals)
                .into_vec();

        let neutral = FlowType {
            mode: FlowMode::Continuous,
            ty: Type::Absent,
        };
        let normalized = alpha_normalize_owner(
            &neutral,
            substitutions.iter().map(|substitution| FlowType {
                mode: FlowMode::Continuous,
                ty: substitution.value.clone(),
            }),
        )
        .1;
        for (substitution, flow) in substitutions.iter_mut().zip(normalized) {
            substitution.value = flow.ty;
        }
        Some(substitutions.into_boxed_slice())
    }

    fn normalized_kernel_call_type_substitutions(
        substitutions: &[KernelCallTypeSubstitution],
    ) -> Box<[KernelCallTypeSubstitution]> {
        let neutral = FlowType {
            mode: FlowMode::Continuous,
            ty: Type::Absent,
        };
        let mut substitutions = substitutions.to_vec();
        substitutions.sort_unstable_by_key(|substitution| substitution.variable);
        let normalized = alpha_normalize_owner(
            &neutral,
            substitutions.iter().map(|substitution| FlowType {
                mode: FlowMode::Continuous,
                ty: substitution.value.clone(),
            }),
        )
        .1;
        for (substitution, flow) in substitutions.iter_mut().zip(normalized) {
            substitution.value = flow.ty;
        }
        substitutions.into_boxed_slice()
    }

    fn call_substitution_mismatch(
        kernel: &[KernelCallTypeSubstitution],
        checked: &[KernelCallTypeSubstitution],
        legacy_selector_target: bool,
    ) -> Option<String> {
        let kernel = kernel
            .iter()
            .map(|substitution| (substitution.variable, &substitution.value))
            .collect::<BTreeMap<_, _>>();
        let checked = checked
            .iter()
            .map(|substitution| (substitution.variable, &substitution.value))
            .collect::<BTreeMap<_, _>>();
        for (variable, checked) in checked {
            let Some(kernel) = kernel.get(&variable) else {
                // The legacy owner checker can back-specialize an inherited
                // PASSED actual from a downstream WHEN selector. The dense
                // kernel deliberately keeps that definition-local provider
                // generic: a partial reactive selector is not an input type
                // restriction. Preserve strict missing-evidence checks for
                // ordinary callables, but do not require this known legacy
                // selector contamination to appear in the canonical call
                // substitution environment.
                if legacy_selector_target {
                    continue;
                }
                if !matches!(
                    checked,
                    Type::Var(_) | Type::Unknown | Type::UnresolvedShape { .. }
                ) {
                    return Some(format!(
                        "kernel omits concrete checked substitution {variable:?}={checked:?}"
                    ));
                }
                continue;
            };
            if !legacy_generic_selector_type_matches(kernel, checked)
                && !legacy_generic_selector_type_matches(checked, kernel)
            {
                return Some(format!(
                    "substitution {variable:?} is incompatible: kernel={kernel:?} checked={checked:?}"
                ));
            }
        }
        None
    }

    fn call_and_effect_inventory_mismatches(
        report: &KernelOwnerOracleReport,
        checked: &CheckedProgramFields,
        checked_expression_by_stable: &BTreeMap<StableExpressionKey, boon_checked::CheckedExprId>,
        stable_by_checked_expression: &BTreeMap<boon_checked::CheckedExprId, StableExpressionKey>,
        project: &ProjectSyntaxSnapshot,
    ) -> Vec<String> {
        let callable_owners = checked_callable_owners(checked, project);
        let kernel_interfaces = report
            .supported
            .iter()
            .map(|owner| (owner.owner.clone(), owner))
            .collect::<BTreeMap<_, _>>();
        let mut mismatches = Vec::new();
        let mut kernel_calls = BTreeMap::new();
        let mut kernel_effects = BTreeMap::new();
        for owner in &report.supported {
            let expression_flows = owner
                .expressions
                .iter()
                .cloned()
                .collect::<BTreeMap<_, _>>();
            for call in &owner.calls {
                if kernel_calls
                    .insert(call.expression.clone(), (&owner.owner, call))
                    .is_some()
                {
                    mismatches.push(format!(
                        "kernel repeats call occurrence {:?}",
                        call.expression
                    ));
                }
                if expression_flows.get(&call.expression) != Some(&call.result) {
                    mismatches.push(format!(
                        "kernel call {:?} result differs from its expression row",
                        call.expression
                    ));
                }
            }
            for (expression, effect) in &owner.effects {
                if kernel_effects
                    .insert(expression.clone(), (&owner.owner, effect))
                    .is_some()
                {
                    mismatches.push(format!(
                        "kernel repeats host-effect occurrence {expression:?}"
                    ));
                }
            }
        }

        let mut current_calls = BTreeMap::new();
        for call in &checked.calls {
            let Some(stable) = stable_by_checked_expression.get(&call.expression).cloned() else {
                mismatches.push(format!(
                    "checked call {:?} `{}` has no stable source expression",
                    call.id, call.function
                ));
                continue;
            };
            if current_calls.insert(stable.clone(), call).is_some() {
                mismatches.push(format!("checked image repeats call occurrence {stable:?}"));
            }
            let Some(expression) = checked.expressions.get(call.expression.0 as usize) else {
                mismatches.push(format!(
                    "checked call {:?} references missing expression {:?}",
                    call.id, call.expression
                ));
                continue;
            };
            if expression.flow_type != call.result {
                mismatches.push(format!(
                    "checked call {:?} result differs from its expression row",
                    call.id
                ));
            }
            if !matches!(expression.kind, CheckedExpressionKind::Call { call: id } if id == call.id)
            {
                mismatches.push(format!(
                    "checked call {:?} is not owned by expression {:?}",
                    call.id, call.expression
                ));
            }
        }

        for (stable, (owner, kernel)) in &kernel_calls {
            let Some(current) = current_calls.get(stable) else {
                mismatches.push(format!(
                    "kernel owner {owner:?} call {stable:?} has no checked call row"
                ));
                continue;
            };
            if current.function != kernel.function.as_ref() {
                mismatches.push(format!(
                    "kernel owner {owner:?} call {stable:?} retains spelling `{}` but checked call spells `{}`",
                    kernel.function, current.function,
                ));
            }
            let current_pipe = current.entries.iter().any(|entry| {
                matches!(
                    entry,
                    boon_checked::CheckedCallEntry::Input {
                        from_pipe: true,
                        ..
                    }
                )
            });
            if current_pipe != kernel.pipe_input.is_some() {
                mismatches.push(format!(
                    "kernel owner {owner:?} call {stable:?} pipe-input surface differs from checked"
                ));
            }
            let current_argument_names = current
                .entries
                .iter()
                .filter_map(|entry| match entry {
                    boon_checked::CheckedCallEntry::Input {
                        name,
                        from_pipe: false,
                        ..
                    }
                    | boon_checked::CheckedCallEntry::FreshOut { name, .. }
                    | boon_checked::CheckedCallEntry::ForwardOut { name, .. } => {
                        Some(name.as_str())
                    }
                    boon_checked::CheckedCallEntry::Input {
                        from_pipe: true, ..
                    } => None,
                })
                .collect::<Vec<_>>();
            let kernel_argument_names = kernel
                .arguments
                .iter()
                .map(|argument| argument.name.as_ref())
                .collect::<Vec<_>>();
            if current_argument_names != kernel_argument_names {
                mismatches.push(format!(
                    "kernel owner {owner:?} call {stable:?} authored arguments {kernel_argument_names:?} differ from checked {current_argument_names:?}"
                ));
            }
            let current_explicit_pass = matches!(
                current.context_binding,
                boon_checked::CheckedContextBinding::Explicit { .. }
            );
            if current_explicit_pass != kernel.pass.is_some() {
                mismatches.push(format!(
                    "kernel owner {owner:?} call {stable:?} explicit PASS surface differs from checked"
                ));
            }
            let target_matches = match &kernel.target {
                KernelOwnerOracleCallTarget::User {
                    target,
                    inherited_formal,
                } => {
                    let target_matches = callable_owners.get(&current.callable) == Some(target)
                        && inherited_formal.is_some()
                            == matches!(
                                current.context_binding,
                                boon_checked::CheckedContextBinding::Inherited { .. }
                            );
                    if target_matches
                        && let Some(target_interface) = kernel_interfaces.get(target)
                        && let Some(current_substitutions) = checked_call_type_substitutions(
                            checked,
                            current,
                            &target_interface.formals,
                            &target_interface.result,
                        )
                    {
                        let kernel_substitutions =
                            normalized_kernel_call_type_substitutions(&kernel.type_substitutions);
                        if let Some(reason) = call_substitution_mismatch(
                            &kernel_substitutions,
                            &current_substitutions,
                            !target_interface.generic_selector_dependents.is_empty(),
                        ) {
                            mismatches.push(format!(
                                "kernel owner {owner:?} call {stable:?} substitutions differ from checked: {reason}; kernel={kernel_substitutions:?} checked={current_substitutions:?}"
                            ));
                        }
                    }
                    target_matches
                }
                KernelOwnerOracleCallTarget::RenderConstructor(kind) => {
                    render_constructor_kind(&current.function).as_ref() == Some(kind)
                }
                KernelOwnerOracleCallTarget::PureBuiltin(kind) => {
                    pure_builtin_kind(&current.function).as_ref() == Some(kind)
                }
                KernelOwnerOracleCallTarget::HostEffect(operation) => {
                    current.function == operation.as_ref()
                }
            };
            if !target_matches {
                mismatches.push(format!(
                    "kernel owner {owner:?} call {stable:?} target {:?} differs from checked `{}` callable {:?}",
                    kernel.target, current.function, current.callable
                ));
            }
            if checked_expression_by_stable.get(stable) != Some(&current.expression) {
                mismatches.push(format!(
                    "kernel owner {owner:?} call {stable:?} maps to a different checked expression"
                ));
            }
        }
        for stable in current_calls.keys() {
            if !kernel_calls.contains_key(stable) {
                mismatches.push(format!(
                    "checked call {stable:?} has no kernel call artifact"
                ));
            }
        }

        let current_effects = current_calls
            .iter()
            .filter_map(|(stable, call)| {
                is_kernel_host_effect(&call.function).then_some((stable.clone(), *call))
            })
            .collect::<BTreeMap<_, _>>();
        for (stable, (owner, kernel)) in &kernel_effects {
            let Some(current) = current_effects.get(stable) else {
                mismatches.push(format!(
                    "kernel owner {owner:?} host effect {stable:?} has no checked effect call"
                ));
                continue;
            };
            if kernel.operation.as_ref() != current.function {
                mismatches.push(format!(
                    "kernel owner {owner:?} host effect {stable:?} operation differs from checked `{}`",
                    current.function
                ));
            }
        }
        for stable in current_effects.keys() {
            if !kernel_effects.contains_key(stable) {
                mismatches.push(format!(
                    "checked host effect {stable:?} has no kernel effect artifact"
                ));
            }
        }
        mismatches
    }

    fn collection_and_source_inventory_mismatches(
        report: &KernelOwnerOracleReport,
        checked: &CheckedProgramFields,
        checked_expression_by_stable: &BTreeMap<StableExpressionKey, boon_checked::CheckedExprId>,
        stable_by_checked_expression: &BTreeMap<boon_checked::CheckedExprId, StableExpressionKey>,
    ) -> Vec<String> {
        let mut mismatches = Vec::new();
        let mut kernel_collections = BTreeMap::new();
        let mut kernel_sources = BTreeMap::new();
        for owner in &report.supported {
            for collection in &owner.collections {
                if kernel_collections
                    .insert(collection.expression.clone(), collection)
                    .is_some()
                {
                    mismatches.push(format!(
                        "kernel repeats collection occurrence {:?}",
                        collection.expression
                    ));
                }
            }
            for source in &owner.sources {
                if kernel_sources
                    .insert(source.expression.clone(), source)
                    .is_some()
                {
                    mismatches.push(format!(
                        "kernel repeats SOURCE occurrence {:?}",
                        source.expression
                    ));
                }
            }
        }

        let stable_provider = |expression| {
            stable_by_checked_expression
                .get(&expression)
                .cloned()
                .map(KernelOwnerOracleValueReference::Expression)
        };
        let mut current_collections = BTreeMap::new();
        let mut current_sources = BTreeMap::new();
        for (stable, expression_id) in checked_expression_by_stable {
            let Some(expression) = checked.expressions.get(expression_id.0 as usize) else {
                mismatches.push(format!(
                    "stable expression {stable:?} references missing checked expression {expression_id:?}"
                ));
                continue;
            };
            let collection = match &expression.kind {
                CheckedExpressionKind::List { capacity, items } => Some((
                    KernelCollectionKind::List,
                    *capacity,
                    items
                        .iter()
                        .copied()
                        .map(|item| (KernelOwnerEdgeRole::CollectionItem, item))
                        .collect::<Vec<_>>(),
                )),
                CheckedExpressionKind::Bytes { fixed_size, items } => Some((
                    KernelCollectionKind::Bytes,
                    *fixed_size,
                    items
                        .iter()
                        .copied()
                        .map(|item| (KernelOwnerEdgeRole::CollectionItem, item))
                        .collect::<Vec<_>>(),
                )),
                CheckedExpressionKind::Set { items } => Some((
                    KernelCollectionKind::Set,
                    None,
                    items
                        .iter()
                        .copied()
                        .map(|item| (KernelOwnerEdgeRole::CollectionItem, item))
                        .collect::<Vec<_>>(),
                )),
                CheckedExpressionKind::Map { entries } => Some((
                    KernelCollectionKind::Map,
                    None,
                    entries
                        .iter()
                        .copied()
                        .map(|entry| (KernelOwnerEdgeRole::MapEntry, entry))
                        .collect::<Vec<_>>(),
                )),
                CheckedExpressionKind::Source => {
                    current_sources.insert(
                        stable.clone(),
                        KernelOwnerOracleSource {
                            expression: stable.clone(),
                            payload_type: expression.flow_type.ty.clone(),
                            flow_type: expression.flow_type.clone(),
                        },
                    );
                    None
                }
                _ => None,
            };
            let Some((kind, capacity, inputs)) = collection else {
                continue;
            };
            let mut stable_inputs = Vec::with_capacity(inputs.len());
            for (role, provider) in inputs {
                let Some(provider) = stable_provider(provider) else {
                    mismatches.push(format!(
                        "checked collection {stable:?} input {provider:?} has no stable expression"
                    ));
                    continue;
                };
                stable_inputs.push(KernelOwnerOracleExpressionInput { role, provider });
            }
            current_collections.insert(
                stable.clone(),
                KernelOwnerOracleCollection {
                    expression: stable.clone(),
                    kind,
                    capacity,
                    inputs: stable_inputs.into_boxed_slice(),
                    flow_type: expression.flow_type.clone(),
                },
            );
        }

        for (stable, kernel) in &kernel_collections {
            match current_collections.get(stable) {
                Some(current)
                    if kernel.kind == current.kind
                        && kernel.capacity == current.capacity
                        && kernel.inputs == current.inputs =>
                {}
                Some(current) => mismatches.push(format!(
                    "kernel collection {stable:?} structure differs from checked row: kernel_kind={:?} kernel_capacity={:?} kernel_inputs={:?} checked_kind={:?} checked_capacity={:?} checked_inputs={:?}",
                    kernel.kind,
                    kernel.capacity,
                    kernel.inputs,
                    current.kind,
                    current.capacity,
                    current.inputs,
                )),
                None => mismatches.push(format!(
                    "kernel collection {stable:?} has no checked collection expression"
                )),
            }
        }
        for stable in current_collections.keys() {
            if !kernel_collections.contains_key(stable) {
                mismatches.push(format!(
                    "checked collection {stable:?} has no kernel collection artifact"
                ));
            }
        }
        for (stable, kernel) in &kernel_sources {
            match current_sources.get(stable) {
                Some(current) if *kernel == current => {}
                Some(current) => mismatches.push(format!(
                    "kernel SOURCE {stable:?} differs from checked expression: kernel={kernel:?} checked={current:?}"
                )),
                None => mismatches.push(format!(
                    "kernel SOURCE {stable:?} has no checked SOURCE expression"
                )),
            }
        }
        for stable in current_sources.keys() {
            if !kernel_sources.contains_key(stable) {
                mismatches.push(format!(
                    "checked SOURCE {stable:?} has no kernel source artifact"
                ));
            }
        }
        mismatches
    }

    fn resource_inventory_mismatches(
        report: &KernelOwnerOracleReport,
        checked: &CheckedProgramFields,
        checked_expression_by_stable: &BTreeMap<StableExpressionKey, boon_checked::CheckedExprId>,
        stable_by_checked_expression: &BTreeMap<boon_checked::CheckedExprId, StableExpressionKey>,
        project: &ProjectSyntaxSnapshot,
    ) -> Vec<String> {
        fn declaration_from_statement(
            statement: &boon_checked::CheckedStatement,
        ) -> Option<DeclId> {
            match statement.kind {
                CheckedStatementKind::Function { declaration }
                | CheckedStatementKind::Field { declaration } => Some(declaration),
                CheckedStatementKind::Source { declaration, .. }
                | CheckedStatementKind::Hold { declaration, .. }
                | CheckedStatementKind::List { declaration, .. } => declaration,
                CheckedStatementKind::Block
                | CheckedStatementKind::Spread
                | CheckedStatementKind::Expression => None,
            }
        }

        fn types_alpha_equal(left: &Type, right: &Type) -> bool {
            let neutral = FlowType {
                mode: FlowMode::Continuous,
                ty: Type::Absent,
            };
            let left = alpha_normalize_owner(
                &neutral,
                [FlowType {
                    mode: FlowMode::Continuous,
                    ty: left.clone(),
                }],
            )
            .1;
            let right = alpha_normalize_owner(
                &neutral,
                [FlowType {
                    mode: FlowMode::Continuous,
                    ty: right.clone(),
                }],
            )
            .1;
            left == right
        }

        fn resource_types_compatible(left: &Type, right: &Type) -> bool {
            if types_alpha_equal(left, right) {
                return true;
            }
            let left = FlowType {
                mode: FlowMode::Continuous,
                ty: left.clone(),
            };
            let right = FlowType {
                mode: FlowMode::Continuous,
                ty: right.clone(),
            };
            flow_matches_current_or_legacy_render_projection(&left, &right)
        }

        fn type_contains_tag(ty: &Type, expected: &str) -> bool {
            match ty {
                Type::VariantSet(variants) => variants.iter().any(|variant| match variant {
                    Variant::Tag(tag) => tag == expected,
                    Variant::Tagged { tag, fields } => {
                        tag == expected
                            || fields
                                .fields
                                .values()
                                .any(|field| type_contains_tag(field, expected))
                    }
                }),
                Type::Object(shape) => shape
                    .fields
                    .values()
                    .any(|field| type_contains_tag(field, expected)),
                Type::List(item) | Type::Set(item) => type_contains_tag(item, expected),
                Type::Function { args, result } => {
                    args.iter().any(|arg| type_contains_tag(arg, expected))
                        || type_contains_tag(&result.ty, expected)
                }
                Type::Union(members) => members
                    .iter()
                    .any(|member| type_contains_tag(member, expected)),
                Type::Map { key, value } => {
                    type_contains_tag(key, expected) || type_contains_tag(value, expected)
                }
                Type::Text
                | Type::Number
                | Type::Bytes(_)
                | Type::Absent
                | Type::RenderContract
                | Type::UnresolvedShape { .. }
                | Type::Var(_)
                | Type::Unknown
                | Type::Bits { .. } => false,
            }
        }

        fn is_public_origin(
            project: &ProjectSyntaxSnapshot,
            owner: &StableCheckOwnerKey,
            origin: &KernelOwnerOracleDeclarationOrigin,
        ) -> bool {
            let KernelOwnerOracleDeclarationOrigin::Statement(statement) = origin else {
                return false;
            };
            project
                .owner_view(owner)
                .and_then(|view| {
                    view.statement_ids()
                        .first()
                        .and_then(|root| view.stable_statement_key_local(*root))
                })
                .as_ref()
                == Some(statement)
        }

        fn declaration_targets_equal(
            project: &ProjectSyntaxSnapshot,
            left: &KernelOwnerOracleLexicalTarget,
            right: &KernelOwnerOracleLexicalTarget,
        ) -> bool {
            if left == right {
                return true;
            }
            match (left, right) {
                (
                    KernelOwnerOracleLexicalTarget::OwnerPublic(owner),
                    KernelOwnerOracleLexicalTarget::Declaration {
                        owner: candidate,
                        origin,
                    },
                )
                | (
                    KernelOwnerOracleLexicalTarget::Declaration {
                        owner: candidate,
                        origin,
                    },
                    KernelOwnerOracleLexicalTarget::OwnerPublic(owner),
                ) => owner == candidate && is_public_origin(project, owner, origin),
                _ => false,
            }
        }

        fn resource_paths_equal(
            project: &ProjectSyntaxSnapshot,
            left: &KernelOwnerOracleSemanticPath,
            right: &KernelOwnerOracleSemanticPath,
        ) -> bool {
            if left.anchor_owner != right.anchor_owner || left.projection != right.projection {
                return false;
            }
            match (&left.anchor, &right.anchor) {
                (left, right) if left == right => true,
                (None, Some(origin)) | (Some(origin), None) => {
                    is_public_origin(project, &left.anchor_owner, origin)
                }
                _ => false,
            }
        }

        let mut stable_statement_by_checked = BTreeMap::new();
        let mut checked_statement_by_stable = BTreeMap::new();
        for owner in project.stable_check_owner_keys() {
            let Some(view) = project.owner_view(&owner) else {
                continue;
            };
            for statement in view.statement_ids() {
                let Some(stable) = view.stable_statement_key_local(*statement) else {
                    continue;
                };
                let Some(syntax) = view.statement_for_local(*statement) else {
                    continue;
                };
                let Some(checked_statement) = project
                    .statement_slot(syntax.id)
                    .and_then(|slot| checked.statements.get(slot))
                else {
                    continue;
                };
                stable_statement_by_checked
                    .entry(checked_statement.id)
                    .or_insert_with(|| stable.clone());
                checked_statement_by_stable
                    .entry(stable)
                    .or_insert(checked_statement);
            }
        }

        let mut stable_declaration_by_checked = BTreeMap::new();
        for owner in &report.supported {
            for declaration in &owner.declarations {
                let checked_declaration = match &declaration.origin {
                    KernelOwnerOracleDeclarationOrigin::Statement(statement) => {
                        checked_statement_by_stable
                            .get(statement)
                            .and_then(|statement| declaration_from_statement(statement))
                    }
                    KernelOwnerOracleDeclarationOrigin::RecordField { object, ordinal } => {
                        checked_expression_by_stable
                            .get(object)
                            .and_then(|expression| {
                                let expression = checked.expressions.get(expression.0 as usize)?;
                                let fields = match &expression.kind {
                                    CheckedExpressionKind::Object { fields }
                                    | CheckedExpressionKind::TaggedObject { fields, .. } => fields,
                                    _ => return None,
                                };
                                fields.get(*ordinal as usize)?.declaration
                            })
                    }
                    KernelOwnerOracleDeclarationOrigin::Parameter { statement, ordinal } => {
                        checked_statement_by_stable
                            .get(statement)
                            .and_then(|statement| declaration_from_statement(statement))
                            .and_then(|callable| {
                                checked
                                    .callables
                                    .iter()
                                    .find(|entry| entry.decl_id == callable)
                            })
                            .and_then(|callable| {
                                callable
                                    .parameters
                                    .iter()
                                    .find(|parameter| parameter.ordinal == *ordinal as usize)
                            })
                            .map(|parameter| parameter.decl_id)
                    }
                    KernelOwnerOracleDeclarationOrigin::PatternBinding { .. }
                    | KernelOwnerOracleDeclarationOrigin::CallbackBinding { .. } => None,
                };
                if let Some(checked_declaration) = checked_declaration {
                    stable_declaration_by_checked
                        .entry(checked_declaration)
                        .or_insert(KernelOwnerOracleLexicalTarget::Declaration {
                            owner: owner.owner.clone(),
                            origin: declaration.origin.clone(),
                        });
                }
            }
        }

        let stable_path = |anchor: DeclId, projection: &[String]| {
            let target = stable_declaration_by_checked.get(&anchor)?;
            match target {
                KernelOwnerOracleLexicalTarget::Declaration { owner, origin } => {
                    Some(KernelOwnerOracleSemanticPath {
                        anchor_owner: owner.clone(),
                        anchor: Some(origin.clone()),
                        projection: projection
                            .iter()
                            .cloned()
                            .map(String::into_boxed_str)
                            .collect::<Vec<_>>()
                            .into_boxed_slice(),
                    })
                }
                KernelOwnerOracleLexicalTarget::OwnerPublic(owner) => {
                    Some(KernelOwnerOracleSemanticPath {
                        anchor_owner: owner.clone(),
                        anchor: None,
                        projection: projection
                            .iter()
                            .cloned()
                            .map(String::into_boxed_str)
                            .collect::<Vec<_>>()
                            .into_boxed_slice(),
                    })
                }
                KernelOwnerOracleLexicalTarget::ContextFormal { .. }
                | KernelOwnerOracleLexicalTarget::Value(_)
                | KernelOwnerOracleLexicalTarget::RuntimeContext => None,
            }
        };
        let resource_statement_matches =
            |checked_statement: boon_checked::CheckedStatementId,
             kernel_statement: &StableStatementKey| {
                checked_statement_by_stable
                    .get(kernel_statement)
                    .is_some_and(|statement| statement.id == checked_statement)
            };

        let result_expression_by_owner = report
            .supported
            .iter()
            .filter_map(|owner| Some((owner.owner.clone(), owner.result_expression.clone()?)))
            .collect::<BTreeMap<_, _>>();
        let legacy_no_element_expressions = report
            .supported
            .iter()
            .flat_map(|owner| owner.legacy_no_element_dependents.iter().cloned())
            .collect::<BTreeSet<_>>();
        let stable_value_expression = |value: &KernelOwnerOracleValueReference| match value {
            KernelOwnerOracleValueReference::Expression(expression) => Some(expression.clone()),
            KernelOwnerOracleValueReference::OwnerResult(owner) => {
                result_expression_by_owner.get(owner).cloned()
            }
        };

        let mut mismatches = Vec::new();
        let mut kernel_sources = BTreeMap::new();
        let mut kernel_states = BTreeMap::new();
        let mut kernel_lists = BTreeMap::new();
        for owner in &report.supported {
            for source in &owner.source_resources {
                if kernel_sources
                    .insert(source.expression.clone(), source)
                    .is_some()
                {
                    mismatches.push(format!(
                        "kernel repeats SOURCE resource {:?}",
                        source.expression
                    ));
                }
            }
            for state in &owner.states {
                if kernel_states
                    .insert(state.expression.clone(), state)
                    .is_some()
                {
                    mismatches.push(format!("kernel repeats HOLD state {:?}", state.expression));
                }
            }
            for list in &owner.lists {
                if kernel_lists.insert(list.producer.clone(), list).is_some() {
                    mismatches.push(format!("kernel repeats LIST resource {:?}", list.producer));
                }
            }
        }

        let mut current_sources = BTreeMap::new();
        for source in &checked.sources {
            let Some(stable) = stable_by_checked_expression
                .get(&source.expression)
                .cloned()
            else {
                continue;
            };
            if !matches!(
                checked
                    .expressions
                    .get(source.expression.0 as usize)
                    .map(|row| &row.kind),
                Some(CheckedExpressionKind::Source)
            ) {
                continue;
            }
            current_sources.insert(stable, source);
        }
        let mut current_states = BTreeMap::new();
        for state in &checked.states {
            if state.kind != CheckedStateKind::Hold {
                continue;
            }
            let Some(stable) = stable_by_checked_expression.get(&state.expression).cloned() else {
                continue;
            };
            current_states.insert(stable, state);
        }
        let mut current_lists = BTreeMap::new();
        for list in &checked.lists {
            let Some(stable) = stable_by_checked_expression.get(&list.producer).cloned() else {
                continue;
            };
            current_lists.insert(stable, list);
        }

        for (stable, kernel) in &kernel_sources {
            let Some(current) = current_sources.get(stable) else {
                mismatches.push(format!(
                    "kernel SOURCE resource {stable:?} has no checked resource row"
                ));
                continue;
            };
            let current_declaration = stable_declaration_by_checked.get(&current.declaration);
            let current_statement = stable_statement_by_checked.get(&current.statement);
            let current_path = stable_path(current.path.anchor, &current.path.projection);
            if !current_declaration.is_some_and(|current| {
                declaration_targets_equal(project, current, &kernel.declaration)
            }) || !resource_statement_matches(current.statement, &kernel.statement)
                || !current_path
                    .as_ref()
                    .is_some_and(|current| resource_paths_equal(project, current, &kernel.path))
                || current.interval_ms != kernel.interval_ms
                || !types_alpha_equal(&current.payload_type, &kernel.payload_type)
            {
                mismatches.push(format!(
                    "kernel SOURCE resource {stable:?} differs from checked row: kernel={kernel:?} checked={current:?} checked_declaration={current_declaration:?} checked_statement={current_statement:?} checked_path={current_path:?}"
                ));
            }
        }
        for stable in current_sources.keys() {
            if !kernel_sources.contains_key(stable) {
                mismatches.push(format!(
                    "checked literal SOURCE resource {stable:?} has no kernel resource row"
                ));
            }
        }

        for (stable, kernel) in &kernel_states {
            let Some(current) = current_states.get(stable) else {
                mismatches.push(format!(
                    "kernel HOLD state {stable:?} has no checked state row"
                ));
                continue;
            };
            let current_declaration = stable_declaration_by_checked.get(&current.declaration);
            let current_binding = stable_declaration_by_checked.get(&current.binding_declaration);
            let current_statement = stable_statement_by_checked.get(&current.statement);
            let current_initial = stable_by_checked_expression.get(&current.initial);
            let current_path = stable_path(current.path.anchor, &current.path.projection);
            if !current_declaration.is_some_and(|current| {
                declaration_targets_equal(project, current, &kernel.declaration)
            }) || !current_binding.is_some_and(|current| {
                declaration_targets_equal(project, current, &kernel.binding_declaration)
            }) || !resource_statement_matches(current.statement, &kernel.statement)
                || current_initial != stable_value_expression(&kernel.initial).as_ref()
                || !current_path
                    .as_ref()
                    .is_some_and(|current| resource_paths_equal(project, current, &kernel.path))
                || current.kind != kernel.kind
            {
                mismatches.push(format!(
                    "kernel HOLD state {stable:?} differs from checked row: kernel={kernel:?} checked={current:?} checked_declaration={current_declaration:?} checked_binding={current_binding:?} checked_statement={current_statement:?} checked_initial={current_initial:?} checked_path={current_path:?}"
                ));
            }
        }
        for stable in current_states.keys() {
            if !kernel_states.contains_key(stable) {
                mismatches.push(format!(
                    "checked HOLD state {stable:?} has no kernel state row"
                ));
            }
        }

        for (stable, kernel) in &kernel_lists {
            let Some(current) = current_lists.get(stable) else {
                mismatches.push(format!(
                    "kernel LIST resource {stable:?} has no checked list row"
                ));
                continue;
            };
            let current_declaration = stable_declaration_by_checked.get(&current.declaration);
            let current_statement = stable_statement_by_checked.get(&current.statement);
            let current_path = stable_path(current.path.anchor, &current.path.projection);
            let declaration_matches = current_declaration.is_some_and(|current| {
                declaration_targets_equal(project, current, &kernel.declaration)
            });
            let statement_matches =
                resource_statement_matches(current.statement, &kernel.statement);
            let path_matches = current_path
                .as_ref()
                .is_some_and(|current| resource_paths_equal(project, current, &kernel.path));
            let capacity_matches = current.capacity == kernel.capacity;
            let key_policy_matches = current.key_policy == kernel.key_policy;
            let kernel_item_flow = FlowType {
                mode: FlowMode::Continuous,
                ty: Type::List(Type::shared(kernel.item_type.clone())),
            };
            let current_item_flow = FlowType {
                mode: FlowMode::Continuous,
                ty: Type::List(Type::shared(current.item_type.clone())),
            };
            let item_type_matches =
                resource_types_compatible(&kernel.item_type, &current.item_type)
                    || ((legacy_no_element_expressions.contains(stable)
                        || type_contains_tag(&kernel.item_type, "NoElement"))
                        && (legacy_no_element_widening_matches(
                            &kernel_item_flow,
                            &current_item_flow,
                        ) || boon_checked::resolved_type_is_assignable_to(
                            &kernel_item_flow.ty,
                            &current_item_flow.ty,
                        )));
            if !(declaration_matches
                && statement_matches
                && path_matches
                && capacity_matches
                && key_policy_matches
                && item_type_matches)
            {
                mismatches.push(format!(
                    "kernel LIST resource {stable:?} differs from checked row: declaration_matches={declaration_matches} statement_matches={statement_matches} path_matches={path_matches} capacity_matches={capacity_matches} key_policy_matches={key_policy_matches} item_type_matches={item_type_matches} kernel_statement={:?} checked_statement={current_statement:?} kernel_path={:?} checked_path={current_path:?} checked_id={:?} checked_span={:?}",
                    kernel.statement,
                    kernel.path,
                    current.id,
                    current.span,
                ));
            }
        }
        for stable in current_lists.keys() {
            if !kernel_lists.contains_key(stable) {
                let current = current_lists[stable];
                let kernel_owner = report.supported.iter().find(|owner| {
                    owner
                        .expressions
                        .iter()
                        .any(|(expression, _)| expression == stable)
                });
                let kernel_collection = kernel_owner.and_then(|owner| {
                    owner
                        .collections
                        .iter()
                        .find(|collection| &collection.expression == stable)
                });
                mismatches.push(format!(
                    "checked LIST resource {stable:?} has no kernel list row: kernel_owner={:?} kernel_collection={} checked_id={:?} declaration={:?} statement={:?} path={:?} span={:?}",
                    kernel_owner.map(|owner| &owner.owner),
                    kernel_collection.is_some(),
                    current.id,
                    current.declaration,
                    current.statement,
                    current.path,
                    current.span,
                ));
            }
        }
        mismatches
    }

    fn statement_inventory_mismatches(
        report: &KernelOwnerOracleReport,
        checked: &CheckedProgramFields,
        stable_by_checked_expression: &BTreeMap<boon_checked::CheckedExprId, StableExpressionKey>,
        project: &ProjectSyntaxSnapshot,
    ) -> Vec<String> {
        let mut mismatches = Vec::new();
        let mut kernel_statements = BTreeMap::new();
        let mut kernel_owner_by_statement = BTreeMap::new();
        let result_expression_by_owner = report
            .supported
            .iter()
            .filter_map(|owner| Some((owner.owner.clone(), owner.result_expression.clone()?)))
            .collect::<BTreeMap<_, _>>();
        for owner in &report.supported {
            for statement in &owner.statements {
                if kernel_statements
                    .insert(statement.statement.clone(), statement)
                    .is_some()
                {
                    mismatches.push(format!(
                        "kernel repeats statement artifact {:?}",
                        statement.statement
                    ));
                }
                if kernel_owner_by_statement
                    .insert(statement.statement.clone(), owner.owner.clone())
                    .is_some()
                {
                    mismatches.push(format!(
                        "kernel assigns statement {:?} to multiple definitions",
                        statement.statement
                    ));
                }
            }
        }

        let mut checked_by_stable = BTreeMap::new();
        let mut stable_by_checked = BTreeMap::new();
        let mut syntax_by_stable = BTreeMap::new();
        for owner in project.stable_check_owner_keys() {
            let Some(view) = project.owner_view(&owner) else {
                continue;
            };
            for statement in view.statement_ids() {
                let Some(stable) = view.stable_statement_key_local(*statement) else {
                    continue;
                };
                let Some(syntax_statement) = view.statement_for_local(*statement) else {
                    continue;
                };
                let Some(slot) = project.statement_slot(syntax_statement.id) else {
                    continue;
                };
                let Some(checked_statement) = checked.statements.get(slot) else {
                    continue;
                };
                syntax_by_stable
                    .entry(stable.clone())
                    .or_insert(syntax_statement);
                checked_by_stable
                    .entry(stable.clone())
                    .or_insert(checked_statement);
                stable_by_checked
                    .entry(checked_statement.id)
                    .or_insert(stable);
            }
        }

        let value_expression = |value: &KernelOwnerOracleValueReference| match value {
            KernelOwnerOracleValueReference::Expression(expression) => Some(expression.clone()),
            KernelOwnerOracleValueReference::OwnerResult(owner) => {
                result_expression_by_owner.get(owner).cloned()
            }
        };
        for (stable, kernel) in &kernel_statements {
            let Some(current) = checked_by_stable.get(stable).copied() else {
                mismatches.push(format!(
                    "kernel statement {stable:?} has no checked statement row"
                ));
                continue;
            };
            let Some(syntax) = syntax_by_stable.get(stable).copied() else {
                mismatches.push(format!(
                    "kernel statement {stable:?} has no parser statement row"
                ));
                continue;
            };
            let syntax_matches = match (&kernel.kind, &syntax.kind) {
                (
                    KernelStatementKind::Function { name, parameters },
                    AstStatementKind::Function {
                        name: syntax_name,
                        parameters: syntax_parameters,
                    },
                ) => {
                    name.as_ref() == syntax_name
                        && parameters.len() == syntax_parameters.len()
                        && parameters.iter().zip(syntax_parameters).all(
                            |(parameter, syntax_parameter)| {
                                parameter.name.as_ref() == syntax_parameter.name
                                    && Some(parameter.ordinal)
                                        == u32::try_from(syntax_parameter.ordinal).ok()
                                    && matches!(
                                        (parameter.kind, syntax_parameter.kind),
                                        (KernelParameterKind::Value, AstParameterKind::Value)
                                            | (KernelParameterKind::Out, AstParameterKind::Out)
                                    )
                            },
                        )
                }
                (
                    KernelStatementKind::Field { name },
                    AstStatementKind::Field { name: syntax_name },
                ) => name.as_ref() == syntax_name,
                (
                    KernelStatementKind::Source { field, event },
                    AstStatementKind::Source {
                        field: syntax_field,
                        event: syntax_event,
                    },
                ) => {
                    field.as_deref() == syntax_field.as_deref()
                        && event.as_deref() == syntax_event.as_deref()
                }
                (
                    KernelStatementKind::Hold { field, name },
                    AstStatementKind::Hold {
                        field: syntax_field,
                        name: syntax_name,
                    },
                ) => {
                    field.as_deref() == syntax_field.as_deref()
                        && name.as_deref() == syntax_name.as_deref()
                }
                (
                    KernelStatementKind::List { field, capacity },
                    AstStatementKind::List {
                        field: syntax_field,
                        capacity: syntax_capacity,
                    },
                ) => field.as_deref() == syntax_field.as_deref() && capacity == syntax_capacity,
                (KernelStatementKind::Block, AstStatementKind::Block)
                | (KernelStatementKind::Spread, AstStatementKind::Spread)
                | (KernelStatementKind::Expression, AstStatementKind::Expression) => true,
                _ => false,
            };
            if !syntax_matches {
                mismatches.push(format!(
                    "kernel statement {stable:?} kind {:?} differs from parser {:?}",
                    kernel.kind, syntax.kind
                ));
            }
            let kind_matches = match (&kernel.kind, &current.kind) {
                (KernelStatementKind::Function { .. }, CheckedStatementKind::Function { .. })
                | (KernelStatementKind::Field { .. }, CheckedStatementKind::Field { .. })
                | (KernelStatementKind::Block, CheckedStatementKind::Block)
                | (KernelStatementKind::Spread, CheckedStatementKind::Spread)
                | (KernelStatementKind::Expression, CheckedStatementKind::Expression) => true,
                (
                    KernelStatementKind::Source { event, .. },
                    CheckedStatementKind::Source {
                        event: checked_event,
                        ..
                    },
                ) => event.as_deref() == checked_event.as_deref(),
                (
                    KernelStatementKind::Hold { name, .. },
                    CheckedStatementKind::Hold {
                        name: checked_name, ..
                    },
                ) => name.as_deref() == checked_name.as_deref(),
                (
                    KernelStatementKind::List { capacity, .. },
                    CheckedStatementKind::List {
                        capacity: checked_capacity,
                        ..
                    },
                ) => capacity == checked_capacity,
                _ => false,
            };
            if !kind_matches {
                mismatches.push(format!(
                    "kernel statement {stable:?} kind {:?} differs from checked {:?}",
                    kernel.kind, current.kind
                ));
            }
            let kernel_value = kernel.value.as_ref().and_then(value_expression);
            let checked_value = current
                .value
                .and_then(|value| stable_by_checked_expression.get(&value).cloned());
            if kernel_value != checked_value {
                mismatches.push(format!(
                    "kernel statement {stable:?} value {kernel_value:?} differs from checked {checked_value:?}"
                ));
            }
            let mut checked_children = Vec::new();
            for child in &current.children {
                let Some(child) = stable_by_checked.get(child).cloned() else {
                    mismatches.push(format!(
                        "checked statement {stable:?} references a child with no stable identity"
                    ));
                    continue;
                };
                if kernel_owner_by_statement.get(&child) == kernel_owner_by_statement.get(stable) {
                    checked_children.push(KernelOwnerOracleStatementChild::Local(child));
                } else if let Some(owner) = kernel_owner_by_statement.get(&child) {
                    checked_children.push(KernelOwnerOracleStatementChild::Owner(owner.clone()));
                } else {
                    mismatches.push(format!(
                        "checked statement {stable:?} references child {child:?} with no kernel owner"
                    ));
                }
            }
            let checked_children = checked_children.into_boxed_slice();
            if kernel.children != checked_children {
                mismatches.push(format!(
                    "kernel statement {stable:?} children {:?} differ from checked {:?}",
                    kernel.children, checked_children
                ));
            }
        }
        for stable in checked_by_stable.keys() {
            if !kernel_statements.contains_key(stable) {
                mismatches.push(format!(
                    "checked statement {stable:?} has no kernel statement artifact"
                ));
            }
        }
        mismatches
    }

    fn lexical_plan_inventory_mismatches(
        report: &KernelOwnerOracleReport,
        project: &ProjectSyntaxSnapshot,
    ) -> Vec<String> {
        let authoritative = project_kernel_authoritative_call_shapes()
            .expect("authoritative lexical shapes project in parity audit");
        #[derive(Clone, Debug, Eq, PartialEq)]
        enum StableLexicalTarget {
            Declaration {
                owner: StableCheckOwnerKey,
                declaration: boon_checked::OwnerDeclarationStableKey,
            },
            ContextFormal {
                owner: StableCheckOwnerKey,
            },
            Value(KernelOwnerOracleValueReference),
            RuntimeContext,
            Ambiguous,
        }

        fn record_field_name(
            project: &ProjectSyntaxSnapshot,
            owner: &StableCheckOwnerKey,
            object: &StableExpressionKey,
            ordinal: u32,
        ) -> Option<String> {
            let view = project.owner_view(owner)?;
            let expression = view
                .expressions()
                .zip(view.stable_expression_keys())
                .find_map(|(expression, stable)| (stable == *object).then_some(expression))?;
            let fields = match &expression.kind {
                AstExprKind::Object(fields) | AstExprKind::TaggedObject { fields, .. } => fields,
                _ => return None,
            };
            fields.get(ordinal as usize).map(|field| field.name.clone())
        }

        fn pattern_binding_name(
            project: &ProjectSyntaxSnapshot,
            owner: &StableCheckOwnerKey,
            arm: &StableExpressionKey,
            ordinal: u32,
        ) -> Option<String> {
            let view = project.owner_view(owner)?;
            let expression = view
                .expressions()
                .zip(view.stable_expression_keys())
                .find_map(|(expression, stable)| (stable == *arm).then_some(expression))?;
            let AstExprKind::MatchArm { pattern, .. } = &expression.kind else {
                return None;
            };
            pattern_variable_names(pattern)
                .get(ordinal as usize)
                .cloned()
        }

        fn root_statement(
            project: &ProjectSyntaxSnapshot,
            owner: &StableCheckOwnerKey,
        ) -> Option<StableStatementKey> {
            let view = project.owner_view(owner)?;
            view.statement_ids()
                .first()
                .and_then(|statement| view.stable_statement_key_local(*statement))
        }

        fn kernel_target(
            project: &ProjectSyntaxSnapshot,
            target: &KernelOwnerOracleLexicalTarget,
        ) -> Option<StableLexicalTarget> {
            match target {
                KernelOwnerOracleLexicalTarget::Declaration { owner, origin } => {
                    let declaration = match origin {
                        KernelOwnerOracleDeclarationOrigin::Statement(statement) => {
                            if root_statement(project, owner).as_ref() == Some(statement) {
                                boon_checked::OwnerDeclarationStableKey::Public
                            } else {
                                boon_checked::OwnerDeclarationStableKey::Statement {
                                    statement: statement.clone(),
                                }
                            }
                        }
                        KernelOwnerOracleDeclarationOrigin::Parameter { ordinal, .. } => {
                            boon_checked::OwnerDeclarationStableKey::Parameter { ordinal: *ordinal }
                        }
                        KernelOwnerOracleDeclarationOrigin::RecordField { object, ordinal } => {
                            boon_checked::OwnerDeclarationStableKey::RecordField {
                                object: object.clone(),
                                ordinal: *ordinal,
                                name: record_field_name(project, owner, object, *ordinal)?,
                            }
                        }
                        KernelOwnerOracleDeclarationOrigin::PatternBinding { arm, ordinal } => {
                            boon_checked::OwnerDeclarationStableKey::PatternBinding {
                                selector: arm.clone(),
                                ordinal: *ordinal,
                                name: pattern_binding_name(project, owner, arm, *ordinal)?,
                            }
                        }
                        KernelOwnerOracleDeclarationOrigin::CallbackBinding { call, ordinal } => {
                            boon_checked::OwnerDeclarationStableKey::FreshOut {
                                call: call.clone(),
                                formal_ordinal: *ordinal,
                            }
                        }
                    };
                    Some(StableLexicalTarget::Declaration {
                        owner: owner.clone(),
                        declaration,
                    })
                }
                KernelOwnerOracleLexicalTarget::OwnerPublic(owner) => {
                    Some(StableLexicalTarget::Declaration {
                        owner: owner.clone(),
                        declaration: boon_checked::OwnerDeclarationStableKey::Public,
                    })
                }
                KernelOwnerOracleLexicalTarget::ContextFormal { owner, .. } => {
                    Some(StableLexicalTarget::ContextFormal {
                        owner: owner.clone(),
                    })
                }
                KernelOwnerOracleLexicalTarget::Value(value) => {
                    Some(StableLexicalTarget::Value(value.clone()))
                }
                KernelOwnerOracleLexicalTarget::RuntimeContext => {
                    Some(StableLexicalTarget::RuntimeContext)
                }
            }
        }

        fn planned_target(
            target: Option<&boon_checked::OwnerLexicalTargetRef>,
        ) -> Option<StableLexicalTarget> {
            match target? {
                boon_checked::OwnerLexicalTargetRef::Declaration {
                    owner, declaration, ..
                } => Some(StableLexicalTarget::Declaration {
                    owner: owner.clone(),
                    declaration: declaration.clone(),
                }),
                boon_checked::OwnerLexicalTargetRef::ContextFormal { owner } => {
                    Some(StableLexicalTarget::ContextFormal {
                        owner: owner.clone(),
                    })
                }
                boon_checked::OwnerLexicalTargetRef::Ambiguous { .. } => {
                    Some(StableLexicalTarget::Ambiguous)
                }
            }
        }

        let mut mismatches = Vec::new();
        for owner in &report.supported {
            let Some(view) = project.owner_view(&owner.owner) else {
                mismatches.push(format!(
                    "kernel owner {:?} has no syntax view for lexical parity",
                    owner.owner
                ));
                continue;
            };
            let input = match boon_typecheck::project_owner_syntax_input(view) {
                Ok(input) => input,
                Err(error) => {
                    mismatches.push(format!(
                        "kernel owner {:?} cannot project lexical oracle input: {error}",
                        owner.owner
                    ));
                    continue;
                }
            };
            let plan = match boon_typecheck::project_owner_lexical_plan(&input) {
                Ok(plan) => plan,
                Err(error) => {
                    mismatches.push(format!(
                        "kernel owner {:?} cannot project lexical oracle plan: {error}",
                        owner.owner
                    ));
                    continue;
                }
            };

            let dense_statement_by_syntax = view
                .statement_ids()
                .iter()
                .enumerate()
                .filter_map(|(dense, statement_id)| {
                    let statement = view.statement_for_local(*statement_id)?;
                    Some((statement.id, KernelStatementId(u32::try_from(dense).ok()?)))
                })
                .collect::<BTreeMap<_, _>>();
            let mut expected_origins = BTreeSet::new();
            for statement in &owner.statements {
                let Some((local_statement, syntax)) =
                    view.statement_ids().iter().find_map(|statement_id| {
                        let stable = view.stable_statement_key_local(*statement_id)?;
                        (stable == statement.statement)
                            .then(|| {
                                view.statement_for_local(*statement_id)
                                    .map(|statement| (*statement_id, statement))
                            })
                            .flatten()
                    })
                else {
                    continue;
                };
                let owns_fieldless_hold = matches!(
                    &syntax.kind,
                    AstStatementKind::Hold {
                        field: None,
                        name: Some(_),
                    }
                ) && matches!(
                    prepared_hold_alias_lexical_target(
                        view,
                        &owner.owner,
                        local_statement,
                        &dense_statement_by_syntax,
                    ),
                    Ok(Some(PreparedLexicalTarget::Declaration(
                        KernelDeclarationOrigin::Statement { statement: target },
                    ))) if Some(&target) == dense_statement_by_syntax.get(&syntax.id)
                );
                let declares = owns_fieldless_hold
                    || matches!(
                        syntax.kind,
                        AstStatementKind::Function { .. } | AstStatementKind::Field { .. }
                    )
                    || matches!(
                        syntax.kind,
                        AstStatementKind::Source { field: Some(_), .. }
                            | AstStatementKind::Hold { field: Some(_), .. }
                            | AstStatementKind::List { field: Some(_), .. }
                    );
                if declares {
                    expected_origins.insert(KernelOwnerOracleDeclarationOrigin::Statement(
                        statement.statement.clone(),
                    ));
                }
                if let AstStatementKind::Function { parameters, .. } = &syntax.kind {
                    expected_origins.extend(parameters.iter().filter_map(|parameter| {
                        Some(KernelOwnerOracleDeclarationOrigin::Parameter {
                            statement: statement.statement.clone(),
                            ordinal: u32::try_from(parameter.ordinal).ok()?,
                        })
                    }));
                }
            }
            expected_origins.extend(plan.record_fields().iter().filter_map(|field| {
                Some(KernelOwnerOracleDeclarationOrigin::RecordField {
                    object: input
                        .expressions
                        .get(field.object as usize)?
                        .stable_key
                        .clone(),
                    ordinal: field.ordinal,
                })
            }));
            for expression in &input.expressions {
                match &expression.kind {
                    AstExprKind::MatchArm { pattern, .. } => {
                        expected_origins.extend(
                            pattern_variable_names(pattern)
                                .into_iter()
                                .enumerate()
                                .filter_map(|(ordinal, _)| {
                                    Some(KernelOwnerOracleDeclarationOrigin::PatternBinding {
                                        arm: expression.stable_key.clone(),
                                        ordinal: u32::try_from(ordinal).ok()?,
                                    })
                                }),
                        );
                    }
                    AstExprKind::Call { function, args, .. }
                    | AstExprKind::Pipe {
                        op: function, args, ..
                    } => {
                        let Some(surface) = authoritative.get(function) else {
                            continue;
                        };
                        expected_origins.extend(args.iter().filter_map(|argument| {
                            if argument.kind != AstCallArgKind::BareBinding {
                                return None;
                            }
                            let parameter = surface.parameters.iter().find(|parameter| {
                                parameter.kind == KernelParameterKind::Out
                                    && parameter.name.as_ref() == argument.name
                            })?;
                            Some(KernelOwnerOracleDeclarationOrigin::CallbackBinding {
                                call: expression.stable_key.clone(),
                                ordinal: parameter.ordinal,
                            })
                        }));
                    }
                    _ => {}
                }
            }
            let actual_origins = owner
                .declarations
                .iter()
                .map(|declaration| declaration.origin.clone())
                .collect::<BTreeSet<_>>();
            for missing in expected_origins.difference(&actual_origins).take(64) {
                mismatches.push(format!(
                    "lexical plan declaration origin missing from kernel {:?}: {missing:?}",
                    owner.owner
                ));
            }
            for extra in actual_origins.difference(&expected_origins).take(64) {
                mismatches.push(format!(
                    "kernel declaration origin absent from lexical plan {:?}: {extra:?}",
                    owner.owner
                ));
            }

            let actual_bindings = owner
                .lexical_bindings
                .iter()
                .map(|binding| (binding.expression.clone(), binding))
                .collect::<BTreeMap<_, _>>();
            let mut expected_expressions = BTreeSet::new();
            for (index, read) in plan.reads().iter().enumerate() {
                let Some(read) = read else {
                    continue;
                };
                let Some(expression) = input.expressions.get(index) else {
                    continue;
                };
                expected_expressions.insert(expression.stable_key.clone());
                let Some(actual) = actual_bindings.get(&expression.stable_key).copied() else {
                    mismatches.push(format!(
                        "lexical plan read {:?} in {:?} has no kernel binding",
                        expression.stable_key, owner.owner
                    ));
                    continue;
                };
                let expected_access = match read.access {
                    boon_typecheck::OwnerLexicalAccess::Read => KernelLexicalAccess::Read,
                    boon_typecheck::OwnerLexicalAccess::Drain => KernelLexicalAccess::Drain,
                };
                if actual.access != expected_access
                    || actual
                        .projection
                        .iter()
                        .map(Box::as_ref)
                        .ne(read.projection.iter().map(String::as_str))
                {
                    mismatches.push(format!(
                        "kernel lexical binding {:?} projection/access {:?}/{:?} differs from lexical plan {:?}/{:?}",
                        expression.stable_key,
                        actual.projection,
                        actual.access,
                        read.projection,
                        expected_access
                    ));
                }
                let expected_target =
                    planned_target(plan.signature_regions().stable_target(&read.target));
                let actual_target = kernel_target(project, &actual.target);
                if expected_target.is_some()
                    && expected_target != actual_target
                    && expected_target != Some(StableLexicalTarget::Ambiguous)
                {
                    mismatches.push(format!(
                        "kernel lexical binding {:?} in {:?} target {actual_target:?} differs from lexical plan {expected_target:?}",
                        expression.stable_key, owner.owner
                    ));
                }
            }
            for reference in plan
                .external_candidates()
                .iter()
                .filter(|reference| reference.kind == boon_typecheck::OwnerReferenceKind::Value)
            {
                expected_expressions.insert(reference.expression.clone());
                if !actual_bindings.contains_key(&reference.expression) {
                    mismatches.push(format!(
                        "lexical external read {:?} in {:?} has no kernel binding",
                        reference.expression, owner.owner
                    ));
                }
            }
            for extra in actual_bindings
                .keys()
                .filter(|expression| !expected_expressions.contains(*expression))
                .take(64)
            {
                mismatches.push(format!(
                    "kernel lexical binding {extra:?} in {:?} has no lexical plan read",
                    owner.owner
                ));
            }
        }
        mismatches
    }

    fn checked_public_child_composed_result(
        owner: &KernelOwnerOracleEntry,
        mut result: FlowType,
        checked: &CheckedProgramFields,
        project: &ProjectSyntaxSnapshot,
        context: &str,
    ) -> FlowType {
        if owner.public_child_owner_fields.is_empty() {
            return result;
        }
        let Type::Object(shape) = result.ty.clone() else {
            panic!(
                "{context} has direct public child fields but its public result is not an object"
            );
        };
        let mut shape = shape.into_owned();
        for ((name, child_owner), (kernel_name, kernel_child)) in owner
            .public_child_owner_fields
            .iter()
            .zip(&owner.public_child_kernel_fields)
        {
            assert_eq!(name, kernel_name, "{context} child field authority drifted");
            let child = checked_public_owner_result(checked, project, child_owner).unwrap_or_else(|| {
                panic!(
                    "{context} direct child field `{name}` has no checked public owner result: {child_owner:#?}"
                )
            });
            assert!(
                child == *kernel_child
                    || legacy_public_child_narrowing_matches(kernel_child, &child),
                "{context} direct child field `{name}` is neither exact nor a checked legacy narrowing: {}",
                first_flow_difference(kernel_child, &child)
            );
            let Some(field) = shape.fields.get_mut(name) else {
                panic!(
                    "{context} checked public object omits direct child field `{name}` from {child_owner:#?}"
                );
            };
            *field = kernel_child.ty.clone();
        }
        result.ty = Type::Object(shape.into());
        result
    }

    fn first_flow_difference(kernel: &FlowType, current: &FlowType) -> String {
        if kernel.mode != current.mode {
            return format!(
                "flow mode differs: kernel={:?}, current={:?}",
                kernel.mode, current.mode
            );
        }
        first_type_difference("$", &kernel.ty, &current.ty)
            .unwrap_or_else(|| "types differ only after legacy projection rules".to_owned())
    }

    fn first_type_difference(path: &str, kernel: &Type, current: &Type) -> Option<String> {
        if kernel == current {
            return None;
        }
        match (kernel, current) {
            (Type::VariantSet(kernel), Type::VariantSet(current)) => {
                if kernel.len() != current.len() {
                    return Some(format!(
                        "{path} variant count differs: kernel={kernel:?}, current={current:?}"
                    ));
                }
                kernel
                    .iter()
                    .zip(current.iter())
                    .find_map(|(kernel, current)| match (kernel, current) {
                        (Variant::Tag(kernel), Variant::Tag(current)) if kernel == current => None,
                        (
                            Variant::Tagged {
                                tag: kernel_tag,
                                fields: kernel_fields,
                            },
                            Variant::Tagged {
                                tag: current_tag,
                                fields: current_fields,
                            },
                        ) if kernel_tag == current_tag => first_type_difference(
                            &format!("{path}<{kernel_tag}>"),
                            &Type::Object(kernel_fields.clone()),
                            &Type::Object(current_fields.clone()),
                        ),
                        _ => Some(format!(
                            "{path} variant differs: kernel={kernel:?}, current={current:?}"
                        )),
                    })
            }
            (Type::Object(kernel), Type::Object(current)) => {
                if kernel.open != current.open {
                    return Some(format!(
                        "{path} openness differs: kernel={}, current={}",
                        kernel.open, current.open
                    ));
                }
                for name in kernel.fields.keys().chain(current.fields.keys()) {
                    match (kernel.fields.get(name), current.fields.get(name)) {
                        (Some(kernel), Some(current)) => {
                            if let Some(difference) =
                                first_type_difference(&format!("{path}.{name}"), kernel, current)
                            {
                                return Some(difference);
                            }
                        }
                        (Some(_), None) => {
                            return Some(format!("{path}.{name} exists only in kernel"));
                        }
                        (None, Some(_)) => {
                            return Some(format!("{path}.{name} exists only in current"));
                        }
                        (None, None) => unreachable!(),
                    }
                }
                (kernel.field_order != current.field_order).then(|| {
                    format!(
                        "{path} field order differs: kernel={:?}, current={:?}",
                        kernel.field_order, current.field_order
                    )
                })
            }
            (Type::List(kernel), Type::List(current)) => {
                first_type_difference(&format!("{path}[]"), kernel, current)
            }
            (Type::Set(kernel), Type::Set(current)) => {
                first_type_difference(&format!("{path}{{}}"), kernel, current)
            }
            (
                Type::Map {
                    key: kernel_key,
                    value: kernel_value,
                },
                Type::Map {
                    key: current_key,
                    value: current_value,
                },
            ) => first_type_difference(&format!("{path}.key"), kernel_key, current_key).or_else(
                || first_type_difference(&format!("{path}.value"), kernel_value, current_value),
            ),
            (Type::Union(kernel), Type::Union(current)) => {
                if kernel.len() != current.len() {
                    return Some(format!(
                        "{path} union length differs: kernel={}, current={}",
                        kernel.len(),
                        current.len()
                    ));
                }
                kernel
                    .iter()
                    .zip(current)
                    .enumerate()
                    .find_map(|(index, (kernel, current))| {
                        first_type_difference(&format!("{path}|{index}"), kernel, current)
                    })
            }
            (
                Type::Function {
                    args: kernel_args,
                    result: kernel_result,
                },
                Type::Function {
                    args: current_args,
                    result: current_result,
                },
            ) => {
                if kernel_args.len() != current_args.len() {
                    return Some(format!(
                        "{path} function arity differs: kernel={}, current={}",
                        kernel_args.len(),
                        current_args.len()
                    ));
                }
                kernel_args
                    .iter()
                    .zip(current_args)
                    .enumerate()
                    .find_map(|(index, (kernel, current))| {
                        first_type_difference(&format!("{path}.arg{index}"), kernel, current)
                    })
                    .or_else(|| {
                        (kernel_result != current_result).then(|| {
                            format!(
                                "{path}.result differs: kernel={kernel_result:?}, current={current_result:?}"
                            )
                        })
                    })
            }
            _ => Some(format!(
                "{path} differs: kernel={kernel:?}, current={current:?}"
            )),
        }
    }

    /// The compatibility-assembled checked image can retain an initial-state
    /// slice where the owner interface and the dense kernel retain later HOLD
    /// epochs. Accept only that exact structural narrowing: record shape and
    /// ordering stay identical, and every legacy union/tag member must occur
    /// in the kernel authority.
    fn legacy_public_child_narrowing_matches(kernel: &FlowType, current: &FlowType) -> bool {
        fn variant_matches(kernel: &Variant, current: &Variant) -> bool {
            match (kernel, current) {
                (Variant::Tag(kernel), Variant::Tag(current)) => kernel == current,
                (
                    Variant::Tagged {
                        tag: kernel_tag,
                        fields: kernel_fields,
                    },
                    Variant::Tagged {
                        tag: current_tag,
                        fields: current_fields,
                    },
                ) => {
                    kernel_tag == current_tag
                        && type_matches(
                            &Type::Object(kernel_fields.clone()),
                            &Type::Object(current_fields.clone()),
                        )
                }
                _ => false,
            }
        }

        fn type_matches(kernel: &Type, current: &Type) -> bool {
            if kernel == current {
                return true;
            }
            match (kernel, current) {
                (Type::VariantSet(kernel), Type::VariantSet(current)) => current
                    .iter()
                    .all(|current| kernel.iter().any(|kernel| variant_matches(kernel, current))),
                (Type::Object(kernel), Type::Object(current)) => {
                    kernel.open == current.open
                        && kernel.field_order == current.field_order
                        && kernel.fields.len() == current.fields.len()
                        && kernel.fields.iter().all(|(name, kernel)| {
                            current
                                .fields
                                .get(name)
                                .is_some_and(|current| type_matches(kernel, current))
                        })
                }
                (Type::List(kernel), Type::List(current))
                | (Type::Set(kernel), Type::Set(current)) => type_matches(kernel, current),
                (
                    Type::Map {
                        key: kernel_key,
                        value: kernel_value,
                    },
                    Type::Map {
                        key: current_key,
                        value: current_value,
                    },
                ) => {
                    type_matches(kernel_key, current_key)
                        && type_matches(kernel_value, current_value)
                }
                (Type::Union(kernel), Type::Union(current)) => current
                    .iter()
                    .all(|current| kernel.iter().any(|kernel| type_matches(kernel, current))),
                _ => false,
            }
        }

        kernel.mode == current.mode
            && kernel.ty != current.ty
            && type_matches(&kernel.ty, &current.ty)
    }

    fn flow_matches_current_or_legacy_render_projection(
        kernel: &FlowType,
        current: &FlowType,
    ) -> bool {
        kernel == current
            || legacy_erased_missing_projection_matches(kernel, current)
            || (kernel.mode == current.mode
                && legacy_kind_only_render_projection_matches(&kernel.ty, &current.ty))
    }

    /// The compatibility-assembled checked image can erase both halves of a
    /// missing authoritative event projection: its diagnostic type becomes
    /// `Unknown` and its occurrence mode falls back to `Continuous`. The
    /// kernel retains the exact missing-projection diagnostic and the eventful
    /// occurrence mode. Accept only that four-part legacy tuple; concrete
    /// types, other unresolved reasons, and every other mode pairing remain
    /// strict differential checks.
    fn legacy_erased_missing_projection_matches(kernel: &FlowType, current: &FlowType) -> bool {
        matches!(
            kernel.mode,
            FlowMode::TickPresent | FlowMode::PresentOrAbsent
        ) && current.mode == FlowMode::Continuous
            && matches!(
                &kernel.ty,
                Type::UnresolvedShape { reason }
                    if reason.starts_with("authoritative provider omits projection `")
            )
            && matches!(current.ty, Type::Unknown)
    }

    #[test]
    fn legacy_missing_projection_allowance_is_exact() {
        let kernel = FlowType {
            mode: FlowMode::PresentOrAbsent,
            ty: Type::UnresolvedShape {
                reason: "authoritative provider omits projection `event.click`".to_owned(),
            },
        };
        let current = FlowType {
            mode: FlowMode::Continuous,
            ty: Type::Unknown,
        };

        assert!(legacy_erased_missing_projection_matches(&kernel, &current));
        assert!(!legacy_erased_missing_projection_matches(
            &FlowType {
                mode: FlowMode::Continuous,
                ..kernel.clone()
            },
            &current,
        ));
        assert!(!legacy_erased_missing_projection_matches(
            &FlowType {
                mode: FlowMode::PresentOrAbsent,
                ty: Type::UnresolvedShape {
                    reason: "generic selector remains unresolved".to_owned(),
                },
            },
            &current,
        ));
        assert!(!legacy_erased_missing_projection_matches(
            &kernel,
            &FlowType {
                mode: FlowMode::Continuous,
                ty: Type::Text,
            },
        ));
    }

    fn legacy_generic_selector_member_matches(kernel: &FlowType, current: &FlowType) -> bool {
        kernel.mode == current.mode
            && (matches!(&current.ty, Type::Unknown)
                || legacy_generic_selector_type_matches(&kernel.ty, &current.ty))
    }

    #[test]
    fn selector_contaminated_checked_calls_do_not_invent_kernel_substitutions() {
        let checked = [KernelCallTypeSubstitution {
            variable: KernelTypeParameterId(0),
            value: Type::VariantSet(
                vec![
                    Variant::Tag("Dark".to_owned()),
                    Variant::Tag("Light".to_owned()),
                ]
                .into(),
            ),
        }];

        assert!(call_substitution_mismatch(&[], &checked, false).is_some());
        assert_eq!(call_substitution_mismatch(&[], &checked, true), None);
        assert!(
            call_substitution_mismatch(
                &[KernelCallTypeSubstitution {
                    variable: KernelTypeParameterId(0),
                    value: Type::Number,
                }],
                &checked,
                true,
            )
            .is_some(),
            "selector compatibility may omit legacy-only evidence but may not hide contradictory evidence",
        );
    }

    #[test]
    fn legacy_generic_selector_alpha_does_not_weaken_general_flow_matching() {
        let kernel = FlowType {
            mode: FlowMode::Continuous,
            ty: Type::Number,
        };
        let current = FlowType {
            mode: FlowMode::Continuous,
            ty: Type::Var(boon_checked::TypeVar(0)),
        };

        assert!(legacy_generic_selector_member_matches(&kernel, &current));
        assert!(!flow_matches_current_or_legacy_render_projection(
            &kernel, &current
        ));
    }

    #[test]
    fn legacy_generic_selector_accepts_only_compatible_open_row_extensions() {
        let open = |fields: Vec<(String, Type)>| {
            Type::object(ObjectShape::from_ordered_fields(fields, true))
        };
        let principal = open(vec![(
            "kind".to_owned(),
            Type::Var(boon_checked::TypeVar(0)),
        )]);
        let legacy_call_specialized = open(vec![
            (
                "kind".to_owned(),
                Type::VariantSet(vec![Variant::Tag("Signal".to_owned())].into()),
            ),
            ("label".to_owned(), Type::Text),
        ]);
        let disjoint = open(vec![("label".to_owned(), Type::Text)]);

        assert!(legacy_generic_selector_type_matches(
            &principal,
            &legacy_call_specialized,
        ));
        assert!(!legacy_generic_selector_type_matches(&principal, &disjoint,));
    }

    #[test]
    fn isolated_legacy_rows_do_not_renumber_strict_expression_alphas() {
        let flow = |ty| FlowType {
            mode: FlowMode::Continuous,
            ty,
        };
        let kernel = vec![
            flow(Type::object(ObjectShape::from_ordered_fields(
                [
                    ("left".to_owned(), Type::Var(boon_checked::TypeVar(20))),
                    ("right".to_owned(), Type::Var(boon_checked::TypeVar(21))),
                ],
                true,
            ))),
            flow(Type::Var(boon_checked::TypeVar(10))),
            flow(Type::Var(boon_checked::TypeVar(10))),
        ];
        let current = vec![
            flow(Type::object(ObjectShape::from_ordered_fields(
                [
                    ("left".to_owned(), Type::Var(boon_checked::TypeVar(4))),
                    ("right".to_owned(), Type::Var(boon_checked::TypeVar(4))),
                ],
                true,
            ))),
            flow(Type::Var(boon_checked::TypeVar(3))),
            flow(Type::Var(boon_checked::TypeVar(3))),
        ];

        let (kernel, current) =
            alpha_normalize_expression_partitions(kernel, current, &[true, false, false]);

        assert_eq!(kernel[1], current[1]);
        assert_eq!(kernel[2], current[2]);
        assert_eq!(kernel[1], kernel[2]);
    }

    fn legacy_no_element_widening_matches(kernel: &FlowType, current: &FlowType) -> bool {
        kernel.mode == current.mode
            && boon_checked::resolved_type_is_assignable_to(&current.ty, &kernel.ty)
    }

    fn legacy_source_container_mode_matches(kernel: &FlowType, current: &FlowType) -> bool {
        kernel.ty == current.ty
            && kernel.mode == FlowMode::Continuous
            && current.mode == FlowMode::PresentOrAbsent
    }

    fn legacy_generic_selector_type_matches(kernel: &Type, current: &Type) -> bool {
        if kernel == current {
            return true;
        }
        // The legacy public/result surface can retain a wider render kind or
        // omit fields that the occurrence residual proves. Accept only the
        // standard checked assignability direction: dense actual -> legacy
        // expected. This stays scoped to known generic-selector/call cones.
        if boon_checked::resolved_type_is_assignable_to(kernel, current) {
            return true;
        }
        if let Type::Union(members) = kernel
            && let Some(widened) = members
                .iter()
                .cloned()
                .reduce(|left, right| boon_checked::widen_structural_type(&left, &right))
            && legacy_generic_selector_type_matches(&widened, current)
        {
            return true;
        }
        match (kernel, current) {
            // A generic WHEN's principal surface intentionally owns a broad
            // union, while each compiled invocation slices one selector arm.
            // The legacy checker numbers those arm-local alphas by a different
            // traversal and its structural widening can replace a placeholder
            // arm with a concrete sibling or back-specialize a definition
            // projection from one of its call sites. Exact occurrence calls
            // remain strict; only the already-marked generic selector cone
            // treats either definition-local alpha as that legacy schematic
            // member.
            (Type::Var(_), _) | (_, Type::Var(_)) => true,
            (Type::Union(kernel), Type::Union(current)) => current.iter().all(|current| {
                kernel
                    .iter()
                    .any(|kernel| legacy_generic_selector_type_matches(kernel, current))
            }),
            (Type::Union(kernel), Type::Object(current)) => {
                current.fields.iter().all(|(name, current_field)| {
                    let projected = kernel
                        .iter()
                        .filter_map(|member| {
                            let Type::Object(shape) = member else {
                                return None;
                            };
                            shape.fields.get(name).cloned()
                        })
                        .collect::<Vec<_>>();
                    projected.len() == kernel.len()
                        && legacy_generic_selector_type_matches(
                            &boon_checked::canonical_union_type(projected),
                            current_field,
                        )
                })
            }
            (Type::Union(kernel), current) => kernel
                .iter()
                .any(|kernel| legacy_generic_selector_type_matches(kernel, current)),
            (Type::Object(kernel), Type::Object(current)) if kernel.open && current.open => {
                // Open generic rows admit structural extension. The dense
                // principal retains only fields actually required by the
                // definition, while the legacy checker can backfill sibling
                // fields from a call site. Require one field set to be a
                // compatible subset of the other; disjoint or conflicting
                // open rows remain mismatches.
                let (subset, superset) = if kernel.fields.len() <= current.fields.len() {
                    (&kernel.fields, &current.fields)
                } else {
                    (&current.fields, &kernel.fields)
                };
                subset.iter().all(|(name, subset)| {
                    superset.get(name).is_some_and(|superset| {
                        legacy_generic_selector_type_matches(subset, superset)
                    })
                })
            }
            (Type::Object(kernel), Type::Object(current))
                if kernel.open == current.open && current.fields.len() <= kernel.fields.len() =>
            {
                current.fields.iter().all(|(name, current)| {
                    kernel
                        .fields
                        .get(name)
                        .is_some_and(|kernel| legacy_generic_selector_type_matches(kernel, current))
                })
            }
            (Type::List(kernel), Type::List(current)) | (Type::Set(kernel), Type::Set(current)) => {
                legacy_generic_selector_type_matches(kernel, current)
            }
            (
                Type::Map {
                    key: kernel_key,
                    value: kernel_value,
                },
                Type::Map {
                    key: current_key,
                    value: current_value,
                },
            ) => {
                legacy_generic_selector_type_matches(kernel_key, current_key)
                    && legacy_generic_selector_type_matches(kernel_value, current_value)
            }
            _ => false,
        }
    }

    fn legacy_kind_only_render_projection_matches(kernel: &Type, current: &Type) -> bool {
        if let (Type::List(kernel), Type::List(current)) = (kernel, current) {
            return legacy_kind_only_render_projection_matches(kernel, current);
        }
        let (Type::Object(kernel), Type::Object(current)) = (kernel, current) else {
            return false;
        };
        if current.open || current.field_order.as_ref() != ["kind"] || current.fields.len() != 1 {
            return false;
        }
        let Some(current_kind) = current.fields.get("kind") else {
            return false;
        };
        let Some(kernel_kind) = kernel.fields.get("kind") else {
            return false;
        };
        render_kind_refines_legacy_base(kernel_kind, current_kind)
    }

    fn render_kind_refines_legacy_base(kernel: &Type, current: &Type) -> bool {
        let Some(kernel_tags) = render_constructor_tags(kernel) else {
            return false;
        };
        let Some(current_tags) = render_constructor_tags(current) else {
            return false;
        };
        kernel_tags.is_subset(&current_tags)
    }

    fn render_constructor_tags(ty: &Type) -> Option<BTreeSet<&str>> {
        let Type::VariantSet(variants) = ty else {
            return None;
        };
        let tags = variants
            .iter()
            .map(|variant| {
                let Variant::Tag(tag) = variant else {
                    return None;
                };
                matches!(
                    tag.as_str(),
                    "Block"
                        | "Button"
                        | "Checkbox"
                        | "Document"
                        | "EmbeddedMedia"
                        | "EmbeddedProgram"
                        | "Label"
                        | "Link"
                        | "MapViewport"
                        | "Paragraph"
                        | "Row"
                        | "Scene"
                        | "Stack"
                        | "Text"
                        | "TextInput"
                )
                .then_some(tag.as_str())
            })
            .collect::<Option<BTreeSet<_>>>()?;
        (!tags.is_empty()).then_some(tags)
    }

    fn checked_public_owner_result(
        checked: &CheckedProgramFields,
        project: &ProjectSyntaxSnapshot,
        owner: &StableCheckOwnerKey,
    ) -> Option<FlowType> {
        let StableCheckOwnerKey::Item(owner) = owner else {
            return None;
        };
        let entry = project
            .item_index()
            .owners()
            .find(|entry| entry.owner_key == *owner)?;
        checked_public_statement_result(checked, project, entry.statement_id)
    }

    fn checked_public_statement_result(
        checked: &CheckedProgramFields,
        project: &ProjectSyntaxSnapshot,
        statement_id: usize,
    ) -> Option<FlowType> {
        let statement = checked
            .statements
            .get(project.statement_slot(statement_id)?)?;
        let declaration = match statement.kind {
            CheckedStatementKind::Function { declaration }
            | CheckedStatementKind::Field { declaration } => declaration,
            CheckedStatementKind::Source {
                declaration: Some(declaration),
                ..
            }
            | CheckedStatementKind::Hold {
                declaration: Some(declaration),
                ..
            }
            | CheckedStatementKind::List {
                declaration: Some(declaration),
                ..
            } => declaration,
            _ => return None,
        };
        checked
            .declarations
            .iter()
            .find(|candidate| candidate.id == declaration)
            .map(|declaration| match &declaration.flow_type.ty {
                Type::Function { result, .. } => (**result).clone(),
                _ => declaration.flow_type.clone(),
            })
    }

    #[test]
    fn parsed_owner_kernel_matches_current_checked_rows() {
        let source = concat!(
            "rows: LIST {\n",
            "    [kind: Header, file: TEXT { a }]\n",
            "    [kind: Empty, file: TEXT { b }]\n",
            "}\n",
        );
        let project =
            parse_project_syntax("app/RUN.bn", [("app/RUN.bn".to_owned(), source.to_owned())])
                .expect("parsed project snapshot");
        let oracle = kernel_owner_oracle(&project);
        let [owner] = oracle.supported.as_ref() else {
            panic!(
                "fixture must produce one supported owner: {:#?}",
                oracle.unsupported
            )
        };
        let [container_owner] = oracle.container_owners.as_ref() else {
            panic!(
                "fixture must classify one declaration-less unit container: {:#?}",
                oracle.container_owners
            )
        };
        assert!(matches!(container_owner, StableCheckOwnerKey::UnitRoot(_)));
        assert!(oracle.unsupported.is_empty());

        let parsed = parse_source("app/RUN.bn", source).expect("parsed current fixture");
        let checked = boon_typecheck::check_program(&parsed);
        assert!(
            checked.report.diagnostics.is_empty(),
            "current checker diagnostics: {:#?}",
            checked.report.diagnostics
        );
        let (checked, _) = checked.program.expect("fixture checks").into_parts();
        let declaration = checked
            .declarations
            .iter()
            .find(|declaration| {
                declaration.kind == CheckedDeclarationKind::List && declaration.name == "rows"
            })
            .expect("checked rows declaration");
        assert_eq!(owner.result, declaration.flow_type);
        let [collection] = owner.collections.as_ref() else {
            panic!("LIST fixture must publish one collection artifact")
        };
        assert_eq!(collection.kind, KernelCollectionKind::List);
        assert_eq!(collection.capacity, None);
        assert_eq!(collection.flow_type, owner.result);
        assert_eq!(collection.inputs.len(), 2);
        assert!(collection.inputs.iter().all(|input| {
            input.role == KernelOwnerEdgeRole::CollectionItem
                && matches!(
                    &input.provider,
                    KernelOwnerOracleValueReference::Expression(_)
                )
        }));
        let list_statement = owner
            .statements
            .iter()
            .find(|statement| {
                matches!(
                    &statement.kind,
                    KernelStatementKind::List {
                        field: Some(field),
                        capacity: None,
                    } if field.as_ref() == "rows"
                )
            })
            .expect("LIST owner publishes its authored statement row");
        assert_eq!(
            list_statement.value,
            owner
                .result_expression
                .clone()
                .map(KernelOwnerOracleValueReference::Expression)
        );
        assert_eq!(list_statement.children.len(), 2);
        assert!(
            list_statement
                .children
                .iter()
                .all(|child| matches!(child, KernelOwnerOracleStatementChild::Local(_)))
        );

        let checked_by_stable_key = parsed
            .ast
            .expressions
            .iter()
            .filter_map(|expression| {
                Some((
                    parsed.stable_expression_key(expression.id)?,
                    checked.expressions.get(expression.id)?.flow_type.clone(),
                ))
            })
            .collect::<BTreeMap<_, _>>();
        for (stable, flow_type) in owner.expressions.iter() {
            assert_eq!(
                Some(flow_type),
                checked_by_stable_key.get(stable),
                "kernel/current expression mismatch at {stable:#?}"
            );
        }
        assert_eq!(
            owner
                .result_expression
                .as_ref()
                .and_then(|result| checked_by_stable_key.get(result)),
            Some(&owner.result)
        );
        assert!(oracle.work.operations > 0);
        assert!(oracle.work.activations < oracle.work.operations.saturating_mul(8));
    }

    #[test]
    fn parsed_source_retains_payload_identity_in_the_expression_artifact() {
        let source = "signal: SOURCE\n";
        let project =
            parse_project_syntax("app/RUN.bn", [("app/RUN.bn".to_owned(), source.to_owned())])
                .expect("parse SOURCE artifact fixture");
        let payloads = boon_typecheck::project_source_payload_abi_types(&project)
            .expect("project SOURCE payload ABI");
        let oracle = kernel_owner_oracle_with_source_payloads(&project, &payloads);
        let owner = oracle
            .supported
            .iter()
            .find(|owner| {
                matches!(&owner.owner, StableCheckOwnerKey::Item(key) if key.item_route.segments().last().is_some_and(|segment| segment.names.as_ref() == ["signal"]))
            })
            .unwrap_or_else(|| panic!("SOURCE owner must compile: {:#?}", oracle.unsupported));
        let [source] = owner.sources.as_ref() else {
            panic!("SOURCE owner must publish one source expression artifact")
        };
        assert_eq!(payloads.len(), 1);
        let payload_type = payloads.values().next().unwrap();

        assert_eq!(source.expression, owner.result_expression.clone().unwrap());
        assert_eq!(&source.payload_type, payload_type);
        assert_eq!(source.flow_type, owner.result);
        assert_eq!(source.flow_type.mode, FlowMode::PresentOrAbsent);
    }

    #[test]
    fn dynamic_text_templates_keep_dependencies_but_publish_text() {
        let source = concat!(
            "FUNCTION marker(value) {\n",
            "    TEXT { M {value} }\n",
            "}\n",
            "result: marker(value: TEXT { one })\n",
        );
        let project =
            parse_project_syntax("app/RUN.bn", [("app/RUN.bn".to_owned(), source.to_owned())])
                .expect("parse dynamic text-template fixture");
        let oracle = kernel_owner_oracle(&project);
        assert!(
            oracle
                .unsupported
                .iter()
                .all(|(owner, _)| matches!(owner, StableCheckOwnerKey::UnitRoot(_))),
            "dynamic templates must not prune their owner graph: {:#?}",
            oracle.unsupported
        );
        let marker = oracle
            .supported
            .iter()
            .find(|owner| {
                matches!(&owner.owner, StableCheckOwnerKey::Item(key) if key.item_route.segments().last().is_some_and(|segment| segment.names.as_ref() == ["marker"]))
            })
            .expect("template function compiles");
        let result = oracle
            .supported
            .iter()
            .find(|owner| {
                matches!(&owner.owner, StableCheckOwnerKey::Item(key) if key.item_route.segments().last().is_some_and(|segment| segment.names.as_ref() == ["result"]))
            })
            .expect("template call compiles");
        assert_eq!(marker.result.ty, Type::Text);
        assert_eq!(result.result.ty, Type::Text);
        assert!(
            oracle.work.operations >= 2,
            "the interpolated value remains an authored dependency"
        );
    }

    #[test]
    fn unique_nested_root_values_are_static_callable_captures() {
        let source = concat!(
            "store: [\n",
            "    elements: [fire: True]\n",
            "]\n",
            "FUNCTION read_fire() {\n",
            "    elements.fire\n",
            "}\n",
            "result: read_fire()\n",
        );
        let project =
            parse_project_syntax("app/RUN.bn", [("app/RUN.bn".to_owned(), source.to_owned())])
                .expect("parse unique nested-root capture fixture");
        let oracle = kernel_owner_oracle(&project);
        let function = oracle
            .supported
            .iter()
            .find(|owner| {
                matches!(&owner.owner, StableCheckOwnerKey::Item(key) if key.item_route.segments().last().is_some_and(|segment| segment.names.as_ref() == ["read_fire"]))
            })
            .unwrap_or_else(|| {
                panic!(
                    "unique nested root must be captured by exact owner: {:#?}",
                    oracle.unsupported
                )
            });
        let result = oracle
            .supported
            .iter()
            .find(|owner| {
                matches!(&owner.owner, StableCheckOwnerKey::Item(key) if key.item_route.segments().last().is_some_and(|segment| segment.names.as_ref() == ["result"]))
            })
            .expect("capturing call compiles");
        let expected = Type::VariantSet(vec![Variant::Tag("True".to_owned())].into());
        assert_eq!(function.result.ty, expected);
        assert_eq!(result.result.ty, expected);
    }

    #[test]
    fn direct_list_builtins_accept_the_named_list_input() {
        let source = concat!(
            "chunks:\n",
            "    List/chunk(\n",
            "        list: LIST {\n",
            "            1\n",
            "            2\n",
            "        }\n",
            "        size: 1\n",
            "    )\n",
        );
        let project =
            parse_project_syntax("app/RUN.bn", [("app/RUN.bn".to_owned(), source.to_owned())])
                .expect("parse direct list builtin fixture");
        let oracle = kernel_owner_oracle(&project);
        let chunks = oracle
            .supported
            .iter()
            .find(|owner| matches!(&owner.owner, StableCheckOwnerKey::Item(key) if key.item_route.segments().last().is_some_and(|segment| segment.names.as_ref() == ["chunks"])))
            .unwrap_or_else(|| panic!("direct List/chunk must compile: {:#?}", oracle.unsupported));
        assert_eq!(
            chunks.result.ty,
            Type::List(Type::shared(Type::object(
                ObjectShape::from_ordered_fields(
                    [
                        ("label".to_owned(), Type::Text),
                        ("items".to_owned(), Type::List(Type::shared(Type::Number)),),
                    ],
                    false,
                )
            )))
        );
    }

    #[test]
    fn parsed_calls_compose_fresh_parameter_frames_without_owner_dispatch() {
        let source = concat!(
            "FUNCTION box(value) {\n",
            "    [value: value]\n",
            "}\n",
            "number_box: box(value: 1)\n",
            "text_box: box(value: TEXT { text })\n",
        );
        let parsed = parse_source("app/RUN.bn", source).expect("parse call-composition fixture");
        let checked = boon_typecheck::check_program(&parsed);
        assert!(
            !checked.report.has_errors(),
            "current checker diagnostics: {:#?}",
            checked.report.diagnostics
        );
        let (checked, _) = checked.program.expect("fixture checks").into_parts();
        let checked_by_stable_key = parsed
            .ast
            .expressions
            .iter()
            .filter_map(|expression| {
                Some((
                    parsed.stable_expression_key(expression.id)?,
                    checked.expressions.get(expression.id)?.flow_type.clone(),
                ))
            })
            .collect::<BTreeMap<_, _>>();
        let project =
            parse_project_syntax("app/RUN.bn", [("app/RUN.bn".to_owned(), source.to_owned())])
                .expect("parse unit-native call-composition fixture");
        let oracle = kernel_owner_oracle(&project);
        let function_owner = oracle
            .supported
            .iter()
            .find(|owner| {
                matches!(&owner.owner, StableCheckOwnerKey::Item(owner) if owner.item_route.segments().last().is_some_and(|segment| segment.kind == UnitItemKind::Function))
            })
            .expect("parameterized function is compiled into the dense component");
        assert_owner_matches_current(
            function_owner,
            &checked_by_stable_key,
            &checked,
            &project,
            "call-composition function",
        );
        let result_owners = oracle
            .supported
            .iter()
            .filter_map(|owner| {
                let StableCheckOwnerKey::Item(key) = &owner.owner else {
                    return None;
                };
                let name = key.item_route.segments().last()?.names.first()?;
                matches!(name.as_str(), "number_box" | "text_box").then(|| (name.clone(), owner))
            })
            .collect::<BTreeMap<_, _>>();
        assert_eq!(
            result_owners["number_box"].result.ty,
            Type::object(ObjectShape::from_ordered_fields(
                [("value".to_owned(), Type::Number)],
                false,
            ))
        );
        assert_eq!(
            result_owners["text_box"].result.ty,
            Type::object(ObjectShape::from_ordered_fields(
                [("value".to_owned(), Type::Text)],
                false,
            ))
        );
        for owner in result_owners.values() {
            let [call] = owner.calls.as_ref() else {
                panic!("parsed call owner must publish one compact call row")
            };
            assert_eq!(call.expression, owner.result_expression.clone().unwrap());
            assert_eq!(call.result, owner.result);
            assert_eq!(
                call.target,
                KernelOwnerOracleCallTarget::User {
                    target: function_owner.owner.clone(),
                    inherited_formal: None,
                }
            );
            let [input] = call.inputs.as_ref() else {
                panic!("box call must publish its one formal input edge")
            };
            assert_eq!(input.role, KernelCallInputRole::Formal { ordinal: 0 });
            assert!(matches!(
                &input.provider,
                KernelOwnerOracleValueReference::Expression(_)
            ));
            let [substitution] = call.type_substitutions.as_ref() else {
                panic!("generic box call must publish one target-local substitution")
            };
            assert_eq!(substitution.variable, KernelTypeParameterId(0));
            assert_eq!(
                substitution.value,
                if owner.result.ty
                    == Type::object(ObjectShape::from_ordered_fields(
                        [("value".to_owned(), Type::Number)],
                        false,
                    ))
                {
                    Type::Number
                } else {
                    Type::Text
                }
            );
        }
    }

    #[test]
    fn wrapped_out_calls_use_one_dense_frame_and_scoped_callback_binding() {
        let source = concat!(
            "FUNCTION doubled(list, entry: OUT, new) {\n",
            "    list\n",
            "    |> List/map(\n",
            "        item: entry\n",
            "        new: new * 2\n",
            "    )\n",
            "}\n",
            "FUNCTION wrapped(list, row: OUT, new) {\n",
            "    list\n",
            "    |> doubled(\n",
            "        entry: row\n",
            "        new: new\n",
            "    )\n",
            "}\n",
            "rows: LIST {\n",
            "    [value: 1]\n",
            "    [value: 2]\n",
            "}\n",
            "result:\n",
            "    rows\n",
            "    |> wrapped(\n",
            "        row\n",
            "        new: row.value\n",
            "    )\n",
        );
        let parsed = parse_source("app/RUN.bn", source).expect("parse wrapped OUT fixture");
        let checked = boon_typecheck::check_program(&parsed);
        assert!(
            !checked.report.has_errors(),
            "current checker diagnostics: {:#?}",
            checked.report.diagnostics
        );
        let (checked, _) = checked
            .program
            .expect("wrapped OUT fixture checks")
            .into_parts();
        let checked_by_stable_key = parsed
            .ast
            .expressions
            .iter()
            .filter_map(|expression| {
                Some((
                    parsed.stable_expression_key(expression.id)?,
                    checked.expressions.get(expression.id)?.flow_type.clone(),
                ))
            })
            .collect::<BTreeMap<_, _>>();
        let project =
            parse_project_syntax("app/RUN.bn", [("app/RUN.bn".to_owned(), source.to_owned())])
                .expect("parse unit-native wrapped OUT fixture");
        let authoritative =
            project_kernel_authoritative_call_shapes().expect("authoritative OUT shapes project");
        let surfaces = project_callable_surfaces(&project, &authoritative)
            .expect("user OUT scope effects infer");
        for function in ["doubled", "wrapped"] {
            let [surface] = surfaces[function].as_ref() else {
                panic!("{function} must have one callable surface")
            };
            assert_eq!(
                surface.parameters[2].evaluation_scope,
                KernelParameterEvaluationScope::Output {
                    parameter_ordinal: 1,
                },
                "{function}.new must be evaluated under its public OUT"
            );
        }

        let oracle = kernel_owner_oracle(&project);
        assert!(
            oracle.unsupported.is_empty(),
            "wrapped OUT project must compile entirely in the dense kernel: {:#?}",
            oracle.unsupported
        );
        assert_eq!(
            oracle
                .checked_scopes
                .iter()
                .filter(|scope| { scope.kind == boon_checked::CheckedScopeKind::RepeatedOutput })
                .count(),
            2,
            "each callable OUT parameter must retain one repeated-output scope"
        );
        let owner_named = |name: &str| {
            oracle
                .supported
                .iter()
                .find(|owner| {
                    matches!(&owner.owner, StableCheckOwnerKey::Item(key)
                    if key.item_route.segments().last().is_some_and(|segment| {
                        segment.names.first().is_some_and(|candidate| candidate == name)
                    }))
                })
                .unwrap_or_else(|| panic!("missing dense owner `{name}`"))
        };
        for function in ["doubled", "wrapped"] {
            let owner = owner_named(function);
            assert_owner_matches_current(
                owner,
                &checked_by_stable_key,
                &checked,
                &project,
                function,
            );
            let function_statement = owner
                .statements
                .iter()
                .find(|statement| matches!(statement.kind, KernelStatementKind::Function { .. }))
                .expect("function statement artifact");
            let KernelStatementKind::Function { parameters, .. } = &function_statement.kind else {
                unreachable!()
            };
            assert_eq!(
                parameters[2].evaluation_scope,
                KernelParameterEvaluationScope::Output {
                    parameter_ordinal: 1,
                }
            );
        }
        let result = owner_named("result");
        assert_eq!(result.result.ty, Type::List(Type::shared(Type::Number)));
        assert_owner_matches_current(
            result,
            &checked_by_stable_key,
            &checked,
            &project,
            "wrapped OUT result",
        );
        let fresh = result
            .declarations
            .iter()
            .find(|declaration| declaration.kind == KernelDeclarationKind::FreshOut)
            .expect("outer wrapped call publishes one fresh OUT declaration");
        assert!(matches!(
            fresh.origin,
            KernelOwnerOracleDeclarationOrigin::CallbackBinding { ordinal: 1, .. }
        ));
        assert!(result.lexical_bindings.iter().any(|binding| {
            binding.target
                == KernelOwnerOracleLexicalTarget::Declaration {
                    owner: result.owner.clone(),
                    origin: fresh.origin.clone(),
                }
                && !binding.projection.is_empty()
        }));
    }

    #[test]
    fn parsed_host_effect_publishes_one_stable_call_and_policy_row() {
        let source = "result: Clock/wall()\n";
        let project =
            parse_project_syntax("app/RUN.bn", [("app/RUN.bn".to_owned(), source.to_owned())])
                .expect("parse host-effect artifact fixture");
        let oracle = kernel_owner_oracle(&project);
        let result = oracle
            .supported
            .iter()
            .find(|owner| {
                matches!(&owner.owner, StableCheckOwnerKey::Item(key) if key.item_route.segments().last().is_some_and(|segment| segment.names.as_ref() == ["result"]))
            })
            .unwrap_or_else(|| panic!("host-effect result must compile: {:#?}", oracle.unsupported));
        let [call] = result.calls.as_ref() else {
            panic!("parsed host effect must publish one compact call row")
        };
        let [(effect_expression, effect)] = result.effects.as_ref() else {
            panic!("parsed host effect must publish one compact policy row")
        };

        assert_eq!(call.expression, *effect_expression);
        assert_eq!(call.expression, result.result_expression.clone().unwrap());
        assert_eq!(call.result, result.result);
        assert_eq!(
            call.target,
            KernelOwnerOracleCallTarget::HostEffect("Clock/wall".into())
        );
        assert!(call.inputs.is_empty());
        assert_eq!(effect.expression, KernelExpressionId(0));
        assert_eq!(effect.operation.as_ref(), "Clock/wall");
    }

    #[test]
    fn parsed_call_diagnostics_match_legacy_type_authority_before_source_presentation() {
        let source = concat!(
            "FUNCTION plus_one(value) {\n",
            "    value + 1\n",
            "}\n",
            "result: plus_one(value: TEXT { wrong })\n",
        );
        let project =
            parse_project_syntax("app/RUN.bn", [("app/RUN.bn".to_owned(), source.to_owned())])
                .expect("parse typed diagnostic fixture");
        let oracle = kernel_owner_oracle(&project);
        let plus_one = oracle
            .supported
            .iter()
            .find(|owner| {
                matches!(&owner.owner, StableCheckOwnerKey::Item(key) if key.item_route.segments().last().is_some_and(|segment| segment.names.as_ref() == ["plus_one"]))
            })
            .unwrap_or_else(|| panic!("plus_one must compile: {:#?}", oracle.unsupported));
        let result = oracle
            .supported
            .iter()
            .find(|owner| {
                matches!(&owner.owner, StableCheckOwnerKey::Item(key) if key.item_route.segments().last().is_some_and(|segment| segment.names.as_ref() == ["result"]))
            })
            .unwrap_or_else(|| panic!("result must compile: {:#?}", oracle.unsupported));
        let [call] = result.calls.as_ref() else {
            panic!("result must publish one call occurrence")
        };
        let [diagnostic] = result.diagnostics.as_ref() else {
            panic!(
                "invalid call must publish one typed diagnostic: {:#?}",
                result.diagnostics
            )
        };
        assert_eq!(diagnostic.severity, KernelDiagnosticSeverity::Error);
        assert_eq!(
            diagnostic.site,
            KernelOwnerOracleDiagnosticSite::CallInput {
                call: call.expression.clone(),
                target: plus_one.owner.clone(),
                formal_ordinal: 0,
            }
        );
        assert_eq!(
            diagnostic.kind,
            KernelDiagnosticKind::CallInputType {
                actual: Type::Text,
                expected: Type::Number,
                mismatch: boon_compiler_kernel::KernelTypeMismatch::Type,
            }
        );

        let parsed = parse_source("app/RUN.bn", source).expect("parse legacy diagnostic fixture");
        let checked = boon_typecheck::check_program(&parsed);
        assert!(
            checked.report.diagnostics.iter().any(|diagnostic| {
                diagnostic.severity == boon_checked::DiagnosticSeverity::Error
                    && diagnostic.message.contains("argument `value`")
                    && diagnostic.message.contains("expected: NUMBER")
                    && diagnostic.message.contains("found: TEXT")
            }),
            "legacy diagnostic authority must describe the same mismatch: {:#?}",
            checked.report.diagnostics
        );
    }

    #[test]
    fn source_expression_diagnostic_family_matches_legacy_authority() {
        let invalid_number = "9".repeat(boon_data::MAX_NUMBER_PARSED_DIGITS + 1);
        let source = format!(
            "bad_number: {invalid_number}\nbad_bits: BITS[4] {{ 2u11111 }}\nbad_byte: 16uFF\n"
        );
        let project =
            parse_project_syntax("app/RUN.bn", [("app/RUN.bn".to_owned(), source.clone())])
                .expect("parse source-expression diagnostic fixture");
        let oracle = kernel_owner_oracle(&project);
        let owner_named = |name: &str| {
            oracle
                .supported
                .iter()
                .find(|owner| {
                    matches!(&owner.owner, StableCheckOwnerKey::Item(key) if key.item_route.segments().last().is_some_and(|segment| segment.names.as_ref() == [name]))
                })
                .unwrap_or_else(|| {
                    panic!(
                        "diagnostic owner `{name}` must compile: {:#?}",
                        oracle.unsupported
                    )
                })
        };

        let number = owner_named("bad_number");
        let [number_diagnostic] = number.diagnostics.as_ref() else {
            panic!("invalid Number must emit one typed diagnostic")
        };
        assert_eq!(
            number_diagnostic.site,
            KernelOwnerOracleDiagnosticSite::Expression(
                number
                    .result_expression
                    .clone()
                    .expect("Number result site")
            )
        );
        let KernelDiagnosticKind::InvalidNumberLiteral {
            literal,
            reason,
            position,
            detail,
        } = &number_diagnostic.kind
        else {
            panic!("unexpected Number diagnostic: {number_diagnostic:#?}")
        };
        assert_eq!(literal.as_ref(), invalid_number);
        assert_eq!(
            *reason,
            boon_compiler_kernel::KernelNumberLiteralErrorReason::ResourceLimit
        );
        assert_eq!(*position as usize, boon_data::MAX_NUMBER_PARSED_DIGITS + 1);
        assert!(detail.contains("digit budget"));

        let bits = owner_named("bad_bits");
        let [bits_diagnostic] = bits.diagnostics.as_ref() else {
            panic!("invalid BITS must emit one typed diagnostic")
        };
        let KernelDiagnosticKind::InvalidBitsLiteral {
            width,
            radix,
            digits,
            detail: bits_detail,
        } = &bits_diagnostic.kind
        else {
            panic!("unexpected BITS diagnostic: {bits_diagnostic:#?}")
        };
        assert_eq!((*width, *radix, digits.as_ref()), (4, 2, "11111"));
        assert!(bits_detail.contains("does not fit BITS[4]"));

        let byte = owner_named("bad_byte");
        assert!(matches!(
            byte.diagnostics.as_ref(),
            [KernelOwnerOracleDiagnostic {
                kind: KernelDiagnosticKind::ByteLiteralOutsideBytes,
                ..
            }]
        ));

        let parsed = parse_source("app/RUN.bn", &source).expect("parse legacy literal fixture");
        let checked = boon_typecheck::check_program(&parsed);
        assert!(
            checked.report.diagnostics.iter().any(|diagnostic| {
                diagnostic.message.contains("invalid exact Number literal")
                    && diagnostic.message.contains(detail.as_ref())
            }),
            "legacy diagnostics must retain the same Number failure: {:#?}",
            checked.report.diagnostics
        );
        assert!(
            checked
                .report
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message == bits_detail.as_ref()),
            "legacy diagnostics must retain the same BITS failure: {:#?}",
            checked.report.diagnostics
        );
        assert!(
            checked.report.diagnostics.iter().any(|diagnostic| {
                diagnostic.message
                    == "byte literals are only valid as direct BYTES constructor items"
            }),
            "legacy diagnostics must retain the same byte-site failure: {:#?}",
            checked.report.diagnostics
        );
        for (owner, diagnostic) in [
            (number, number_diagnostic),
            (bits, bits_diagnostic),
            (
                byte,
                byte.diagnostics
                    .first()
                    .expect("byte diagnostic remains available"),
            ),
        ] {
            let presented = present_kernel_source_diagnostic(&project, &owner.owner, diagnostic)
                .expect("kernel source diagnostic presentation");
            assert!(
                checked
                    .report
                    .diagnostics
                    .iter()
                    .any(|legacy| legacy == &presented),
                "kernel diagnostic must equal one legacy diagnostic exactly\n  kernel: {presented:#?}\n  legacy: {:#?}",
                checked.report.diagnostics
            );
        }
    }

    #[test]
    fn invalid_calls_remain_supported_unknown_owners_with_exact_diagnostics() {
        let source = concat!(
            "FUNCTION needs(first, second) {\n",
            "    first\n",
            "}\n",
            "FUNCTION contextual() {\n",
            "    PASSED.value\n",
            "}\n",
            "FUNCTION out_only(item: OUT) {\n",
            "    1\n",
            "}\n",
            "missing: needs(first: 1)\n",
            "extra: needs(first: 1, second: 2, extra: 3)\n",
            "misordered: needs(second: 2, first: 1)\n",
            "unknown: mystery(value: 1)\n",
            "missing_pass: contextual()\n",
            "missing_out: out_only()\n",
            "piped: 1 |> needs(second: 2)\n",
            "builtin_missing: Text/slice(input: TEXT { value }, from: 0)\n",
            "render_missing: Scene/new()\n",
            "host_missing: Random/bytes()\n",
            "host_extra: Clock/wall(extra: 1)\n",
            "authoritative_pass: Text/empty(PASS: [])\n",
            "authoritative_unmigrated: Text/space()\n",
        );
        let project =
            parse_project_syntax("app/RUN.bn", [("app/RUN.bn".to_owned(), source.to_owned())])
                .expect("parse invalid-call diagnostic fixture");
        let oracle = kernel_owner_oracle(&project);
        let parsed = parse_source("app/RUN.bn", source).expect("parse legacy invalid-call fixture");
        let whole_checked = boon_typecheck::check_program(&parsed);
        let mut session = crate::CompilerSession::new();
        let project_id = session
            .open_project(crate::CompilerProject::new(
                "app/RUN.bn",
                vec![crate::CompilerSourceUnit {
                    path: "app/RUN.bn".to_owned(),
                    source: source.to_owned(),
                }],
                crate::TargetProfile::SoftwareDefault,
                crate::ProgramRole::Server,
                crate::ApplicationIdentity::compiler_default(),
            ))
            .expect("open invalid-call diagnostic project");
        let revision = session.revision(project_id).expect("diagnostic revision");
        let checked = session
            .request(
                project_id,
                revision,
                crate::CompileIntent::Diagnostics,
                &crate::CancellationToken::new(),
            )
            .expect("production diagnostics check invalid calls");
        let checked = checked
            .diagnostics()
            .expect("diagnostics request publishes diagnostics");
        let owner_named = |name: &str| {
            oracle
                .supported
                .iter()
                .find(|owner| {
                    matches!(&owner.owner, StableCheckOwnerKey::Item(key) if key.item_route.segments().last().is_some_and(|segment| segment.names.as_ref() == [name]))
                })
                .unwrap_or_else(|| {
                    panic!(
                        "invalid-call owner `{name}` must remain supported: {:#?}",
                        oracle.unsupported
                    )
                })
        };

        let authoritative = owner_named("authoritative_unmigrated");
        assert_eq!(authoritative.result.ty, Type::Text);
        assert!(authoritative.diagnostics.is_empty());
        assert!(matches!(authoritative.calls.as_ref(), [_]));

        for name in [
            "missing",
            "extra",
            "misordered",
            "unknown",
            "missing_pass",
            "missing_out",
            "builtin_missing",
            "render_missing",
            "host_missing",
            "host_extra",
            "authoritative_pass",
        ] {
            let owner = owner_named(name);
            assert_eq!(owner.result.ty, Type::Unknown, "{name} result");
            assert!(
                owner.calls.is_empty(),
                "invalid call must not publish a checked call artifact: {name}"
            );
            assert!(
                !owner.diagnostics.is_empty(),
                "invalid call must publish typed diagnostics: {name}"
            );
            for diagnostic in &owner.diagnostics {
                let presented =
                    present_kernel_source_diagnostic(&project, &owner.owner, diagnostic)
                        .expect("invalid call diagnostic presentation");
                assert!(
                    checked
                        .diagnostics()
                        .iter()
                        .any(|legacy| legacy == &presented),
                    "kernel call diagnostic must equal one legacy diagnostic exactly\n  owner: {name}\n  kernel: {presented:#?}\n  legacy: {:#?}",
                    checked.diagnostics()
                );
            }
        }
        assert!(
            whole_checked.report.diagnostics.iter().any(|diagnostic| {
                diagnostic.message == "`needs` is missing call entry `second`"
            }),
            "independent whole checker must retain the same missing-input authority"
        );
        assert!(matches!(
            owner_named("missing").diagnostics.as_ref(),
            [KernelOwnerOracleDiagnostic {
                kind: KernelDiagnosticKind::MissingCallEntry { function, name },
                ..
            }] if function.as_ref() == "needs" && name.as_ref() == "second"
        ));
        assert!(matches!(
            owner_named("extra").diagnostics.as_ref(),
            [KernelOwnerOracleDiagnostic {
                site: KernelOwnerOracleDiagnosticSite::CallArgument {
                    source: KernelCallArgumentSource::CallArgument { ordinal: 2 },
                    ..
                },
                kind: KernelDiagnosticKind::UnexpectedCallEntry { function, name },
                ..
            }] if function.as_ref() == "needs" && name.as_ref() == "extra"
        ));
        assert!(matches!(
            owner_named("unknown").diagnostics.as_ref(),
            [KernelOwnerOracleDiagnostic {
                kind: KernelDiagnosticKind::UnresolvedCallable { function },
                ..
            }] if function.as_ref() == "mystery"
        ));
        assert!(matches!(
            owner_named("missing_pass").diagnostics.as_ref(),
            [KernelOwnerOracleDiagnostic {
                kind: KernelDiagnosticKind::MissingPassContext {
                    function,
                    root_call: true,
                },
                ..
            }] if function.as_ref() == "contextual"
        ));
        assert!(matches!(
            owner_named("missing_out").diagnostics.as_ref(),
            [KernelOwnerOracleDiagnostic {
                kind: KernelDiagnosticKind::MissingCallEntry { function, name },
                ..
            }] if function.as_ref() == "out_only" && name.as_ref() == "item"
        ));
        assert!(matches!(
            owner_named("builtin_missing").diagnostics.as_ref(),
            [KernelOwnerOracleDiagnostic {
                kind: KernelDiagnosticKind::MissingCallEntry { function, name },
                ..
            }] if function.as_ref() == "Text/slice" && name.as_ref() == "count"
        ));
        assert!(matches!(
            owner_named("render_missing").diagnostics.as_ref(),
            [KernelOwnerOracleDiagnostic {
                kind: KernelDiagnosticKind::MissingCallEntry { function, name },
                ..
            }] if function.as_ref() == "Scene/new" && name.as_ref() == "root"
        ));
        assert!(matches!(
            owner_named("host_missing").diagnostics.as_ref(),
            [KernelOwnerOracleDiagnostic {
                kind: KernelDiagnosticKind::MissingCallEntry { function, name },
                ..
            }] if function.as_ref() == "Random/bytes" && name.as_ref() == "byte_count"
        ));
        assert!(matches!(
            owner_named("host_extra").diagnostics.as_ref(),
            [KernelOwnerOracleDiagnostic {
                site: KernelOwnerOracleDiagnosticSite::CallArgument {
                    source: KernelCallArgumentSource::CallArgument { ordinal: 0 },
                    ..
                },
                kind: KernelDiagnosticKind::UnexpectedCallEntry { function, name },
                ..
            }] if function.as_ref() == "Clock/wall" && name.as_ref() == "extra"
        ));
        assert!(matches!(
            owner_named("authoritative_pass").diagnostics.as_ref(),
            [KernelOwnerOracleDiagnostic {
                site: KernelOwnerOracleDiagnosticSite::CallPass { pipe: false, .. },
                kind: KernelDiagnosticKind::PassOnAuthoritativeCallable {
                    function,
                    callable_kind: KernelCallableKind::Builtin,
                },
                ..
            }] if function.as_ref() == "Text/empty"
        ));
        let piped = owner_named("piped");
        assert_eq!(piped.result.ty, Type::Number);
        assert!(piped.diagnostics.is_empty());
        assert!(matches!(piped.calls.as_ref(), [_]));
    }

    #[test]
    fn ambiguous_user_call_is_a_typed_unknown_result_without_target_guessing() {
        let source = concat!(
            "FUNCTION choose(value) {\n",
            "    value\n",
            "}\n",
            "FUNCTION choose(value) {\n",
            "    value\n",
            "}\n",
            "result: choose(value: 1)\n",
        );
        let project =
            parse_project_syntax("app/RUN.bn", [("app/RUN.bn".to_owned(), source.to_owned())])
                .expect("parse ambiguous callable fixture");
        let oracle = kernel_owner_oracle(&project);
        let result = oracle
            .supported
            .iter()
            .find(|owner| {
                matches!(&owner.owner, StableCheckOwnerKey::Item(key) if key.item_route.segments().last().is_some_and(|segment| segment.names.as_ref() == ["result"]))
            })
            .unwrap_or_else(|| panic!("ambiguous caller must remain supported: {:#?}", oracle.unsupported));
        assert_eq!(result.result.ty, Type::Unknown);
        assert!(result.calls.is_empty());
        let [diagnostic] = result.diagnostics.as_ref() else {
            panic!("ambiguous call must emit one typed diagnostic")
        };
        assert!(matches!(
            &diagnostic.kind,
            KernelDiagnosticKind::AmbiguousCallable {
                function,
                candidate_count: 2,
            } if function.as_ref() == "choose"
        ));

        let mut session = crate::CompilerSession::new();
        let project_id = session
            .open_project(crate::CompilerProject::new(
                "app/RUN.bn",
                vec![crate::CompilerSourceUnit {
                    path: "app/RUN.bn".to_owned(),
                    source: source.to_owned(),
                }],
                crate::TargetProfile::SoftwareDefault,
                crate::ProgramRole::Server,
                crate::ApplicationIdentity::compiler_default(),
            ))
            .expect("open ambiguous diagnostic project");
        let revision = session
            .revision(project_id)
            .expect("ambiguous diagnostic revision");
        let checked = session
            .request(
                project_id,
                revision,
                crate::CompileIntent::Diagnostics,
                &crate::CancellationToken::new(),
            )
            .expect("production ambiguous diagnostics");
        let presented = present_kernel_source_diagnostic(&project, &result.owner, diagnostic)
            .expect("ambiguous diagnostic presentation");
        assert!(
            checked
                .diagnostics()
                .expect("ambiguous diagnostics product")
                .diagnostics()
                .iter()
                .any(|legacy| legacy == &presented),
            "kernel ambiguity must equal the production diagnostic: {presented:#?}"
        );
    }

    #[test]
    fn explicit_pass_diagnostics_keep_the_context_formal_and_field_failure() {
        let source = concat!(
            "FUNCTION needs_number_pass() {\n",
            "    PASSED.value + 1\n",
            "}\n",
            "result: needs_number_pass(PASS: [value: TEXT { wrong }])\n",
        );
        let project =
            parse_project_syntax("app/RUN.bn", [("app/RUN.bn".to_owned(), source.to_owned())])
                .expect("parse PASS diagnostic fixture");
        let oracle = kernel_owner_oracle(&project);
        let target = oracle
            .supported
            .iter()
            .find(|owner| {
                matches!(&owner.owner, StableCheckOwnerKey::Item(key) if key.item_route.segments().last().is_some_and(|segment| segment.names.as_ref() == ["needs_number_pass"]))
            })
            .unwrap_or_else(|| panic!("PASS target must compile: {:#?}", oracle.unsupported));
        let result = oracle
            .supported
            .iter()
            .find(|owner| {
                matches!(&owner.owner, StableCheckOwnerKey::Item(key) if key.item_route.segments().last().is_some_and(|segment| segment.names.as_ref() == ["result"]))
            })
            .unwrap_or_else(|| panic!("PASS caller must compile: {:#?}", oracle.unsupported));
        let [diagnostic] = result.diagnostics.as_ref() else {
            panic!("invalid PASS must publish one typed diagnostic")
        };
        assert!(matches!(
            &diagnostic.site,
            KernelOwnerOracleDiagnosticSite::CallInput {
                target: diagnostic_target,
                formal_ordinal: 0,
                ..
            } if diagnostic_target == &target.owner
        ));
        assert!(matches!(
            &diagnostic.kind,
            KernelDiagnosticKind::CallInputType {
                mismatch: boon_compiler_kernel::KernelTypeMismatch::IncompatibleField(field),
                ..
            } if field.as_ref() == "value"
        ));

        let parsed = parse_source("app/RUN.bn", source).expect("parse legacy PASS fixture");
        let checked = boon_typecheck::check_program(&parsed);
        assert!(
            checked.report.diagnostics.iter().any(|diagnostic| {
                diagnostic.message.contains("PASS context")
                    && diagnostic
                        .message
                        .contains("field `value` has an incompatible type")
                    && diagnostic.message.contains("expected:")
                    && diagnostic.message.contains("found:")
            }),
            "legacy PASS diagnostic must present the same typed fact: {:#?}",
            checked.report.diagnostics
        );
    }

    #[test]
    fn infix_residuals_constrain_operands_and_publish_fixed_results() {
        let source = concat!(
            "FUNCTION numerator(value) {\n",
            "    value + 2\n",
            "}\n",
            "FUNCTION digits(value) {\n",
            "    numerator(value: value) / 3\n",
            "}\n",
            "sum: 1 + 2\n",
            "ordered: 1 <= 2\n",
            "same: Left == Right\n",
            "answer: digits(value: 9)\n",
        );
        let parsed = parse_source("app/RUN.bn", source).expect("parse infix fixture");
        let checked = boon_typecheck::check_program(&parsed);
        assert!(
            !checked.report.has_errors(),
            "current checker diagnostics: {:#?}",
            checked.report.diagnostics
        );
        let (checked, _) = checked.program.expect("fixture checks").into_parts();
        let checked_by_stable_key = parsed
            .ast
            .expressions
            .iter()
            .filter_map(|expression| {
                Some((
                    parsed.stable_expression_key(expression.id)?,
                    checked.expressions.get(expression.id)?.flow_type.clone(),
                ))
            })
            .collect::<BTreeMap<_, _>>();
        let project =
            parse_project_syntax("app/RUN.bn", [("app/RUN.bn".to_owned(), source.to_owned())])
                .expect("parse unit-native infix fixture");
        let oracle = kernel_owner_oracle(&project);
        for owner in &oracle.supported {
            assert_owner_matches_current(
                owner,
                &checked_by_stable_key,
                &checked,
                &project,
                "infix residual",
            );
        }
        let results = oracle
            .supported
            .iter()
            .filter_map(|owner| {
                let StableCheckOwnerKey::Item(key) = &owner.owner else {
                    return None;
                };
                let name = key.item_route.segments().last()?.names.first()?;
                matches!(name.as_str(), "sum" | "ordered" | "same" | "answer")
                    .then(|| (name.clone(), owner.result.ty.clone()))
            })
            .collect::<BTreeMap<_, _>>();
        let boolean = Type::VariantSet(
            vec![
                Variant::Tag("False".to_owned()),
                Variant::Tag("True".to_owned()),
            ]
            .into(),
        );
        assert_eq!(results["sum"], Type::Number);
        assert_eq!(results["answer"], Type::Number);
        assert_eq!(results["ordered"], boolean);
        assert_eq!(results["same"], boolean);
    }

    #[test]
    fn pure_builtin_calls_compile_to_fixed_residual_equations() {
        let source = concat!(
            "FUNCTION format(value) {\n",
            "    value |> Number/to_text() |> Text/trim()\n",
            "}\n",
            "text: format(value: 9)\n",
            "items:\n",
            "    LIST {\n",
            "        1\n",
            "        2\n",
            "    }\n",
            "count: items |> List/length()\n",
            "empty: TEXT { value } |> Text/is_empty()\n",
            "minimum: Number/min(left: 1, right: 2)\n",
        );
        let parsed = parse_source("app/RUN.bn", source).expect("parse pure builtin fixture");
        let checked = boon_typecheck::check_program(&parsed);
        assert!(
            !checked.report.has_errors(),
            "current checker diagnostics: {:#?}",
            checked.report.diagnostics
        );
        let (checked, _) = checked.program.expect("fixture checks").into_parts();
        let checked_by_stable_key = parsed
            .ast
            .expressions
            .iter()
            .filter_map(|expression| {
                Some((
                    parsed.stable_expression_key(expression.id)?,
                    checked.expressions.get(expression.id)?.flow_type.clone(),
                ))
            })
            .collect::<BTreeMap<_, _>>();
        let project =
            parse_project_syntax("app/RUN.bn", [("app/RUN.bn".to_owned(), source.to_owned())])
                .expect("parse unit-native pure builtin fixture");
        let oracle = kernel_owner_oracle(&project);
        for owner in &oracle.supported {
            assert_owner_matches_current(
                owner,
                &checked_by_stable_key,
                &checked,
                &project,
                "pure builtin residual",
            );
        }
        let results = oracle
            .supported
            .iter()
            .filter_map(|owner| {
                let StableCheckOwnerKey::Item(key) = &owner.owner else {
                    return None;
                };
                let name = key.item_route.segments().last()?.names.first()?;
                matches!(name.as_str(), "text" | "count" | "empty" | "minimum")
                    .then(|| (name.clone(), owner.result.ty.clone()))
            })
            .collect::<BTreeMap<_, _>>();
        assert_eq!(
            results.keys().cloned().collect::<BTreeSet<_>>(),
            BTreeSet::from([
                "count".to_owned(),
                "empty".to_owned(),
                "minimum".to_owned(),
                "text".to_owned(),
            ]),
            "every pure builtin result must compile: {:#?}",
            oracle.unsupported
        );
        let boolean = Type::VariantSet(
            vec![
                Variant::Tag("False".to_owned()),
                Variant::Tag("True".to_owned()),
            ]
            .into(),
        );
        assert_eq!(results["text"], Type::Text);
        assert_eq!(results["count"], Type::Number);
        assert_eq!(results["empty"], boolean);
        assert_eq!(results["minimum"], Type::Number);
    }

    #[test]
    fn singleton_selectors_choose_one_compiled_match_arm() {
        let source = concat!(
            "FUNCTION choose(kind) {\n",
            "    kind |> WHEN {\n",
            "        A => SelectedA\n",
            "        B => SelectedB\n",
            "        __ => Fallback\n",
            "    }\n",
            "}\n",
            "number: choose(kind: A)\n",
            "text: choose(kind: B)\n",
        );
        let parsed = parse_source("app/RUN.bn", source).expect("parse selector fixture");
        let checked = boon_typecheck::check_program(&parsed);
        assert!(
            !checked.report.has_errors(),
            "current checker diagnostics: {:#?}",
            checked.report.diagnostics
        );
        let (checked, _) = checked.program.expect("fixture checks").into_parts();
        let checked_by_stable_key = parsed
            .ast
            .expressions
            .iter()
            .filter_map(|expression| {
                Some((
                    parsed.stable_expression_key(expression.id)?,
                    checked.expressions.get(expression.id)?.flow_type.clone(),
                ))
            })
            .collect::<BTreeMap<_, _>>();
        let project =
            parse_project_syntax("app/RUN.bn", [("app/RUN.bn".to_owned(), source.to_owned())])
                .expect("parse unit-native selector fixture");
        let oracle = kernel_owner_oracle(&project);
        for owner in &oracle.supported {
            if matches!(&owner.owner, StableCheckOwnerKey::Item(key) if key.item_route.segments().last().is_some_and(|segment| segment.kind == UnitItemKind::Function))
            {
                continue;
            }
            let context = format!("selector residual {:#?}", owner.owner);
            assert_owner_matches_current(
                owner,
                &checked_by_stable_key,
                &checked,
                &project,
                &context,
            );
        }
        let results = oracle
            .supported
            .iter()
            .filter_map(|owner| {
                let StableCheckOwnerKey::Item(key) = &owner.owner else {
                    return None;
                };
                let name = key.item_route.segments().last()?.names.first()?;
                matches!(name.as_str(), "number" | "text")
                    .then(|| (name.clone(), owner.result.ty.clone()))
            })
            .collect::<BTreeMap<_, _>>();
        let function = oracle
            .supported
            .iter()
            .find(|owner| matches!(&owner.owner, StableCheckOwnerKey::Item(key) if key.item_route.segments().last().is_some_and(|segment| segment.kind == UnitItemKind::Function)))
            .expect("selector function is compiled");
        assert_eq!(
            function.result.ty,
            Type::VariantSet(
                vec![
                    Variant::Tag("Fallback".to_owned()),
                    Variant::Tag("SelectedA".to_owned()),
                    Variant::Tag("SelectedB".to_owned()),
                ]
                .into(),
            )
        );
        assert_eq!(
            results["number"],
            Type::VariantSet(vec![Variant::Tag("SelectedA".to_owned())].into())
        );
        assert_eq!(
            results["text"],
            Type::VariantSet(vec![Variant::Tag("SelectedB".to_owned())].into())
        );
    }

    #[test]
    fn project_value_reads_use_the_finalized_piped_owner_result() {
        let source = concat!(
            "FUNCTION enrich(row) {\n",
            "    [value: row.value, extra: TEXT { ready }]\n",
            "}\n",
            "rows:\n",
            "    LIST {\n",
            "        [value: 1]\n",
            "        [value: 2]\n",
            "    }\n",
            "    |> List/map(item, new: enrich(row: item))\n",
            "result:\n",
            "    rows\n",
            "    |> List/map(item, new: item.extra)\n",
        );
        let project =
            parse_project_syntax("app/RUN.bn", [("app/RUN.bn".to_owned(), source.to_owned())])
                .expect("parse piped public-result fixture");
        let oracle = kernel_owner_oracle(&project);
        assert!(
            oracle.unsupported.is_empty(),
            "piped public-result fixture must stay entirely in the kernel: {:#?}",
            oracle.unsupported
        );
        let diagnostics = oracle
            .supported
            .iter()
            .filter(|owner| !owner.diagnostics.is_empty())
            .map(|owner| (&owner.owner, &owner.diagnostics))
            .collect::<Vec<_>>();
        assert!(
            diagnostics.is_empty(),
            "piped public-result fixture emitted diagnostics: {diagnostics:#?}"
        );
        let result_named = |name: &str| {
            oracle
                .supported
                .iter()
                .find(|owner| {
                    matches!(&owner.owner, StableCheckOwnerKey::Item(key)
                        if key.item_route.segments().last().is_some_and(|segment| segment.names.as_ref() == [name]))
                })
                .unwrap_or_else(|| panic!("missing `{name}` owner"))
        };
        let expected_item = Type::object(ObjectShape::from_ordered_fields(
            [
                ("value".to_owned(), Type::Number),
                ("extra".to_owned(), Type::Text),
            ],
            false,
        ));
        assert_eq!(
            result_named("rows").result.ty,
            Type::List(Type::shared(expected_item))
        );
        assert_eq!(
            result_named("result").result.ty,
            Type::List(Type::shared(Type::Text))
        );
    }

    #[test]
    fn unqualified_project_reads_ignore_private_hold_aliases() {
        let source = concat!(
            "store: [\n",
            "    selected:\n",
            "        A |> HOLD selected {\n",
            "            B\n",
            "        }\n",
            "]\n",
            "FUNCTION current() {\n",
            "    selected\n",
            "}\n",
            "result: current()\n",
        );
        let project =
            parse_project_syntax("app/RUN.bn", [("app/RUN.bn".to_owned(), source.to_owned())])
                .expect("parse HOLD alias visibility fixture");
        let oracle = kernel_owner_oracle(&project);
        assert!(
            oracle.unsupported.is_empty(),
            "HOLD alias visibility fixture must stay entirely in the kernel: {:#?}",
            oracle.unsupported
        );
        let diagnostics = oracle
            .supported
            .iter()
            .filter(|owner| !owner.diagnostics.is_empty())
            .map(|owner| (&owner.owner, &owner.diagnostics))
            .collect::<Vec<_>>();
        assert!(
            diagnostics.is_empty(),
            "a private HOLD alias must not compete with its public field: {diagnostics:#?}"
        );
        let expected = Type::VariantSet(
            vec![Variant::Tag("A".to_owned()), Variant::Tag("B".to_owned())].into(),
        );
        for name in ["current", "result"] {
            let owner = oracle
                .supported
                .iter()
                .find(|owner| {
                    matches!(&owner.owner, StableCheckOwnerKey::Item(key)
                        if key.item_route.segments().last().is_some_and(|segment| segment.names.as_ref() == [name]))
                })
                .unwrap_or_else(|| panic!("missing `{name}` owner"));
            assert_eq!(owner.result.ty, expected, "{name} public result");
        }
    }

    #[test]
    fn multiline_match_records_compile_as_structural_arm_outputs() {
        let source = concat!(
            "FUNCTION choose(kind) {\n",
            "    kind |> WHEN {\n",
            "        Record => [\n",
            "            value: 1\n",
            "            label: TEXT { chosen }\n",
            "        ]\n",
            "        __ => [\n",
            "            value: 2\n",
            "        ]\n",
            "    }\n",
            "}\n",
            "selected: choose(kind: Record)\n",
        );
        let checked = crate::check_diagnostics_source(crate::CompilerCheckRequest::source_text(
            "app/RUN.bn",
            source,
            crate::ProgramRole::Client,
        ))
        .expect("check multiline record fixture through the owner pipeline");
        assert!(
            checked.output.report.diagnostics.is_empty(),
            "current checker diagnostics: {:#?}",
            checked.output.report.diagnostics
        );
        let project =
            parse_project_syntax("app/RUN.bn", [("app/RUN.bn".to_owned(), source.to_owned())])
                .expect("parse unit-native multiline record fixture");
        let checked = checked
            .output
            .checked_program_fields()
            .expect("multiline record diagnostics own checked fields");
        let mut checked_by_stable_key = BTreeMap::new();
        for owner in project.stable_check_owner_keys() {
            let view = project
                .owner_view(&owner)
                .expect("multiline record owner has a view");
            for (expression, stable_key) in view.expressions().zip(view.stable_expression_keys()) {
                let Some(flow_type) = project
                    .expression_slot(expression.id)
                    .and_then(|slot| checked.expressions.get(slot))
                    .map(|expression| expression.flow_type.clone())
                else {
                    continue;
                };
                checked_by_stable_key.insert(stable_key, flow_type);
            }
        }
        let oracle = kernel_owner_oracle(&project);
        for owner in &oracle.supported {
            assert_owner_matches_current(
                owner,
                &checked_by_stable_key,
                &checked,
                &project,
                "multiline record residual",
            );
        }
        let function = oracle
            .supported
            .iter()
            .find(|owner| matches!(&owner.owner, StableCheckOwnerKey::Item(key) if key.item_route.segments().last().is_some_and(|segment| segment.names.as_ref() == ["choose"])))
            .expect("generic multiline selector function compiles");
        assert_eq!(
            function.result.ty,
            Type::object(ObjectShape::from_ordered_fields(
                [
                    ("value".to_owned(), Type::Number),
                    ("label".to_owned(), Type::Text),
                ],
                false,
            )),
            "the generic principal must structurally widen compatible record arms"
        );
        let selected = oracle
            .supported
            .iter()
            .find(|owner| matches!(&owner.owner, StableCheckOwnerKey::Item(key) if key.item_route.segments().last().is_some_and(|segment| segment.names.as_ref() == ["selected"])))
            .unwrap_or_else(|| {
                panic!(
                    "selected multiline record must compile: {:#?}",
                    oracle.unsupported
                )
            });
        assert_eq!(
            selected.result.ty,
            Type::object(ObjectShape::from_ordered_fields(
                [
                    ("value".to_owned(), Type::Number),
                    ("label".to_owned(), Type::Text),
                ],
                false,
            ))
        );
    }

    #[test]
    fn record_spreads_compile_as_ordered_residual_overlays() {
        let source = concat!(
            "FUNCTION base() {\n",
            "    [family: 1, size: 12]\n",
            "}\n",
            "style: [\n",
            "    ...base()\n",
            "    family: TEXT { Mono }\n",
            "    color: TEXT { #ffffff }\n",
            "]\n",
        );
        let parsed = parse_source("app/RUN.bn", source).expect("parse record-spread fixture");
        let checked = boon_typecheck::check_program(&parsed);
        assert!(
            !checked.report.has_errors(),
            "current checker diagnostics: {:#?}",
            checked.report.diagnostics
        );
        let (checked, _) = checked.program.expect("fixture checks").into_parts();
        let checked_by_stable_key = parsed
            .ast
            .expressions
            .iter()
            .filter_map(|expression| {
                Some((
                    parsed.stable_expression_key(expression.id)?,
                    checked.expressions.get(expression.id)?.flow_type.clone(),
                ))
            })
            .collect::<BTreeMap<_, _>>();
        let project =
            parse_project_syntax("app/RUN.bn", [("app/RUN.bn".to_owned(), source.to_owned())])
                .expect("parse unit-native record-spread fixture");
        let oracle = kernel_owner_oracle(&project);
        for owner in &oracle.supported {
            assert_owner_matches_current(
                owner,
                &checked_by_stable_key,
                &checked,
                &project,
                "record-spread residual",
            );
        }
        let style = oracle
            .supported
            .iter()
            .find(|owner| matches!(&owner.owner, StableCheckOwnerKey::Item(key) if key.item_route.segments().last().is_some_and(|segment| segment.names.as_ref() == ["style"])))
            .unwrap_or_else(|| panic!("style spread must compile: {:#?}", oracle.unsupported));
        assert_eq!(
            style.result.ty,
            Type::object(ObjectShape::from_ordered_fields(
                [
                    ("family".to_owned(), Type::Text),
                    ("size".to_owned(), Type::Number),
                    ("color".to_owned(), Type::Text),
                ],
                false,
            ))
        );
    }

    #[test]
    fn render_constructors_compile_named_fields_and_kind_without_abi_replay() {
        let source = concat!(
            "FUNCTION make_button(label) {\n",
            "    Scene/Element/button(\n",
            "        element: [event: [press: False], hovered: False]\n",
            "        label: label\n",
            "    )\n",
            "}\n",
            "button: make_button(label: TEXT { Go })\n",
        );
        let checked = crate::check_diagnostics_source(crate::CompilerCheckRequest::source_text(
            "app/RUN.bn",
            source,
            crate::ProgramRole::Client,
        ))
        .expect("check render residual fixture through the owner pipeline");
        assert!(
            checked.output.report.diagnostics.is_empty(),
            "current checker diagnostics: {:#?}",
            checked.output.report.diagnostics
        );
        let project =
            parse_project_syntax("app/RUN.bn", [("app/RUN.bn".to_owned(), source.to_owned())])
                .expect("parse unit-native render residual fixture");
        let oracle = kernel_owner_oracle(&project);
        assert!(
            oracle
                .unsupported
                .iter()
                .all(|(owner, _)| matches!(owner, StableCheckOwnerKey::UnitRoot(_))),
            "every declared render owner must compile: {:#?}",
            oracle.unsupported
        );
        // The independent dense checker still exposes the constructor's
        // kind-only base record here. The production owner ABI already makes
        // supplied fields part of the result contract, so assert that contract
        // directly instead of preserving the lossy oracle surface.
        let button = oracle
            .supported
            .iter()
            .find(|owner| matches!(&owner.owner, StableCheckOwnerKey::Item(key) if key.item_route.segments().last().is_some_and(|segment| segment.names == ["button"])))
            .unwrap_or_else(|| panic!("render result must be supported: {:#?}", oracle.unsupported));
        let Type::Object(shape) = &button.result.ty else {
            panic!("render constructor result must be an object")
        };
        assert_eq!(shape.field_order, ["element", "label", "kind"]);
        assert!(matches!(shape.fields["element"], Type::Object(_)));
        assert_eq!(shape.fields["label"], Type::Text);
        assert_eq!(
            shape.fields["kind"],
            Type::VariantSet(vec![Variant::Tag("Button".to_owned())].into())
        );
    }

    #[test]
    fn explicit_pass_contexts_compose_as_fresh_call_frames() {
        let source = concat!(
            "FUNCTION read() {\n",
            "    PASSED.value\n",
            "}\n",
            "FUNCTION inherited() {\n",
            "    read()\n",
            "}\n",
            "number: inherited(PASS: [value: 1])\n",
            "text: inherited(PASS: [value: TEXT { text }])\n",
        );
        let project =
            parse_project_syntax("app/RUN.bn", [("app/RUN.bn".to_owned(), source.to_owned())])
                .expect("parse explicit PASS fixture");
        let oracle = kernel_owner_oracle(&project);
        let results = oracle
            .supported
            .iter()
            .filter_map(|owner| {
                let StableCheckOwnerKey::Item(key) = &owner.owner else {
                    return None;
                };
                let name = key.item_route.segments().last()?.names.first()?;
                matches!(name.as_str(), "number" | "text")
                    .then(|| (name.clone(), owner.result.ty.clone()))
            })
            .collect::<BTreeMap<_, _>>();
        assert_eq!(results["number"], Type::Number);
        assert_eq!(results["text"], Type::Text);
    }

    #[test]
    fn repeated_passed_reads_share_one_projection_alpha() {
        let source = concat!(
            "FUNCTION pair() {\n",
            "    [\n",
            "        event: [\n",
            "            first: PASSED.store.elements.first\n",
            "            repeated: PASSED.store.elements.repeated\n",
            "            third: PASSED.store.elements.third\n",
            "        ]\n",
            "        second: PASSED.store.elements.repeated\n",
            "    ]\n",
            "}\n",
            "unrelated: 1\n",
        );
        let project =
            parse_project_syntax("app/RUN.bn", [("app/RUN.bn".to_owned(), source.to_owned())])
                .expect("parse repeated PASSED fixture");
        let oracle = kernel_owner_oracle(&project);
        let function = oracle
            .supported
            .iter()
            .find(|owner| matches!(&owner.owner, StableCheckOwnerKey::Item(key) if key.item_route.segments().last().is_some_and(|segment| segment.kind == UnitItemKind::Function)))
            .unwrap_or_else(|| panic!("pair function must compile: {:#?}", oracle.unsupported));
        let Type::Object(shape) = &function.result.ty else {
            panic!("pair function must return an object")
        };
        let Type::Object(event) = &shape.fields["event"] else {
            panic!("pair function event must be an object")
        };
        assert_eq!(event.fields["repeated"], shape.fields["second"]);
    }

    #[test]
    fn nonempty_unit_roots_are_not_classified_as_inert_containers() {
        let source = "1\n";
        let project =
            parse_project_syntax("app/RUN.bn", [("app/RUN.bn".to_owned(), source.to_owned())])
                .expect("parse nonempty unit-root fixture");
        let oracle = kernel_owner_oracle(&project);
        assert!(oracle.container_owners.is_empty());
        let [(owner, reason)] = oracle.unsupported.as_ref() else {
            panic!(
                "a nonempty unit root must remain an explicit unsupported surface: {:#?}",
                oracle.unsupported
            )
        };
        assert!(matches!(owner, StableCheckOwnerKey::UnitRoot(_)));
        assert_eq!(reason, "owner has no public declaration");
    }

    #[test]
    fn multiline_record_fields_can_read_later_siblings() {
        let source = concat!(
            "FUNCTION forward() {\n",
            "    [\n",
            "        first:\n",
            "            Initial |> HOLD first {\n",
            "                later\n",
            "            }\n",
            "        later: Updated\n",
            "    ]\n",
            "}\n",
            "result: forward()\n",
        );
        let project =
            parse_project_syntax("app/RUN.bn", [("app/RUN.bn".to_owned(), source.to_owned())])
                .expect("parse forward record-field fixture");
        let oracle = kernel_owner_oracle(&project);
        let function = oracle
            .supported
            .iter()
            .find(|owner| {
                matches!(&owner.owner, StableCheckOwnerKey::Item(key) if key.item_route.segments().last().is_some_and(|segment| segment.kind == UnitItemKind::Function && segment.names == ["forward"]))
            })
            .unwrap_or_else(|| panic!("forward function must compile: {:#?}", oracle.unsupported));
        let Type::Object(shape) = &function.result.ty else {
            panic!("forward function must return an object")
        };
        assert_eq!(
            shape.fields["first"],
            Type::VariantSet(
                vec![
                    Variant::Tag("Initial".to_owned()),
                    Variant::Tag("Updated".to_owned()),
                ]
                .into(),
            )
        );
        assert_eq!(
            shape.fields["later"],
            Type::VariantSet(vec![Variant::Tag("Updated".to_owned())].into())
        );
        assert!(function.diagnostics.is_empty());
        assert!(function.lexical_bindings.iter().any(|binding| {
            binding.projection.is_empty()
                && matches!(
                    &binding.target,
                    KernelOwnerOracleLexicalTarget::Declaration { owner, .. }
                        if owner == &function.owner
                )
        }));
    }

    #[test]
    fn block_bindings_compile_as_lexical_alias_edges() {
        let source = concat!(
            "FUNCTION duplicate(value) {\n",
            "    BLOCK {\n",
            "        first: value\n",
            "        second: first\n",
            "        [left: first, right: second]\n",
            "    }\n",
            "}\n",
            "number: duplicate(value: 1)\n",
            "text: duplicate(value: TEXT { value })\n",
        );
        let parsed = parse_source("app/RUN.bn", source).expect("parse BLOCK lexical fixture");
        let checked = boon_typecheck::check_program(&parsed);
        assert!(
            !checked.report.has_errors(),
            "current checker diagnostics: {:#?}",
            checked.report.diagnostics
        );
        let (checked, _) = checked.program.expect("fixture checks").into_parts();
        let checked_by_stable_key = parsed
            .ast
            .expressions
            .iter()
            .filter_map(|expression| {
                Some((
                    parsed.stable_expression_key(expression.id)?,
                    checked.expressions.get(expression.id)?.flow_type.clone(),
                ))
            })
            .collect::<BTreeMap<_, _>>();
        let project =
            parse_project_syntax("app/RUN.bn", [("app/RUN.bn".to_owned(), source.to_owned())])
                .expect("parse unit-native BLOCK lexical fixture");
        let oracle = kernel_owner_oracle(&project);
        for owner in &oracle.supported {
            assert_owner_matches_current(
                owner,
                &checked_by_stable_key,
                &checked,
                &project,
                "BLOCK lexical residual",
            );
        }
        let results = oracle
            .supported
            .iter()
            .filter_map(|owner| {
                let StableCheckOwnerKey::Item(key) = &owner.owner else {
                    return None;
                };
                let name = key.item_route.segments().last()?.names.first()?;
                matches!(name.as_str(), "number" | "text")
                    .then(|| (name.clone(), owner.result.ty.clone()))
            })
            .collect::<BTreeMap<_, _>>();
        let pair = |ty: Type| {
            Type::object(ObjectShape::from_ordered_fields(
                [("left".to_owned(), ty.clone()), ("right".to_owned(), ty)],
                false,
            ))
        };
        assert_eq!(results["number"], pair(Type::Number));
        assert_eq!(results["text"], pair(Type::Text));
    }

    #[test]
    fn declaration_and_lexical_artifacts_match_the_parser_owned_plan() {
        let fixtures = [
            (
                "block-pattern-callback-drain",
                concat!(
                    "FUNCTION project(value) {\n",
                    "    BLOCK {\n",
                    "        local: [entry: Found[value: value]]\n",
                    "        selected:\n",
                    "            local.entry |> WHEN {\n",
                    "                Found[payload] => payload\n",
                    "                __ => value\n",
                    "            }\n",
                    "        rows: LIST { [rank: selected] }\n",
                    "        drained: rows |> List/map(item, new: DRAIN { item.rank })\n",
                    "        [value: selected, drained: drained]\n",
                    "    }\n",
                    "}\n",
                    "result: project(value: 1)\n",
                ),
            ),
            (
                "statement-record-hold-alias",
                concat!(
                    "store: [\n",
                    "    state:\n",
                    "        0 |> HOLD state {\n",
                    "            True |> THEN { state }\n",
                    "        }\n",
                    "]\n",
                ),
            ),
        ];

        let mut declarations = Vec::new();
        let mut has_record_field = false;
        let mut lexical_targets = Vec::new();
        let mut lexical_accesses = Vec::new();
        for (name, source) in fixtures {
            let project = parse_project_syntax(
                format!("app/{name}.bn"),
                [(format!("app/{name}.bn"), source.to_owned())],
            )
            .unwrap_or_else(|error| panic!("parse {name} lexical fixture: {error}"));
            let oracle = kernel_owner_oracle(&project);
            assert!(
                !oracle.supported.is_empty(),
                "{name} must exercise kernel definition artifacts: {:#?}",
                oracle.unsupported
            );
            assert!(
                oracle
                    .unsupported
                    .iter()
                    .all(|(owner, _)| matches!(owner, StableCheckOwnerKey::UnitRoot(_))),
                "every declared {name} owner must compile: {:#?}",
                oracle.unsupported
            );
            let mismatches = lexical_plan_inventory_mismatches(&oracle, &project);
            assert!(
                mismatches.is_empty(),
                "{name} declaration/lexical parity: {mismatches:#?}"
            );
            declarations.extend(
                oracle
                    .supported
                    .iter()
                    .flat_map(|owner| owner.declarations.iter())
                    .map(|declaration| declaration.kind),
            );
            has_record_field |= oracle.supported.iter().any(|owner| {
                owner.declarations.iter().any(|declaration| {
                    matches!(
                        declaration.origin,
                        KernelOwnerOracleDeclarationOrigin::RecordField { .. }
                    )
                })
            });
            lexical_targets.extend(
                oracle
                    .supported
                    .iter()
                    .flat_map(|owner| owner.lexical_bindings.iter())
                    .map(|binding| binding.target.clone()),
            );
            lexical_accesses.extend(
                oracle
                    .supported
                    .iter()
                    .flat_map(|owner| owner.lexical_bindings.iter())
                    .map(|binding| binding.access),
            );
        }

        assert!(declarations.contains(&KernelDeclarationKind::ValueParameter));
        assert!(declarations.contains(&KernelDeclarationKind::Field));
        assert!(has_record_field);
        assert!(declarations.contains(&KernelDeclarationKind::PatternBinding));
        assert!(declarations.contains(&KernelDeclarationKind::FreshOut));
        assert!(
            lexical_targets
                .iter()
                .any(|target| matches!(target, KernelOwnerOracleLexicalTarget::OwnerPublic(_)))
        );
        assert!(lexical_accesses.contains(&KernelLexicalAccess::Drain));
    }

    #[test]
    fn multiline_record_siblings_are_visible_inside_a_later_hold_field() {
        let source = concat!(
            "FUNCTION stateful(value) {\n",
            "    [\n",
            "        controls: [fire: value]\n",
            "        state:\n",
            "            False |> HOLD state {\n",
            "                controls.fire |> THEN { True }\n",
            "            }\n",
            "    ]\n",
            "}\n",
            "result: stateful(value: 1)\n",
        );
        let project =
            parse_project_syntax("app/RUN.bn", [("app/RUN.bn".to_owned(), source.to_owned())])
                .expect("parse multiline sibling fixture");
        let oracle = kernel_owner_oracle(&project);
        assert!(
            oracle
                .unsupported
                .iter()
                .all(|(owner, _)| matches!(owner, StableCheckOwnerKey::UnitRoot(_))),
            "every declared multiline sibling owner must compile: {:#?}",
            oracle.unsupported
        );
        let result = oracle
            .supported
            .iter()
            .find(|owner| matches!(&owner.owner, StableCheckOwnerKey::Item(key) if key.item_route.segments().last().is_some_and(|segment| segment.names == ["result"])))
            .expect("stateful call result must compile");
        let Type::Object(shape) = &result.result.ty else {
            panic!("stateful result must be an object")
        };
        assert_eq!(
            shape.fields["state"],
            Type::VariantSet(
                vec![
                    Variant::Tag("False".to_owned()),
                    Variant::Tag("True".to_owned())
                ]
                .into()
            )
        );
    }

    #[test]
    fn nested_owner_presentation_inherits_one_compact_enclosing_scope() {
        let source = concat!(
            "store: [\n",
            "    state:\n",
            "        0 |> HOLD state {\n",
            "            True |> THEN { state }\n",
            "        }\n",
            "]\n",
        );
        let project =
            parse_project_syntax("app/RUN.bn", [("app/RUN.bn".to_owned(), source.to_owned())])
                .expect("parse nested checked-presentation fixture");
        let prepared = prepare_kernel_project_projection(&project, &BTreeMap::new());
        assert!(
            prepared
                .unsupported
                .keys()
                .all(|owner| matches!(owner, StableCheckOwnerKey::UnitRoot(_))),
            "every declared nested owner must project: {:#?}",
            prepared.unsupported
        );
        let hold = prepared
            .definition_keys
            .iter()
            .position(|owner| {
                matches!(owner, StableCheckOwnerKey::Item(key) if key
                    .item_route
                    .segments()
                    .last()
                    .is_some_and(|segment| segment.kind == UnitItemKind::Hold))
            })
            .expect("fixture has one nested HOLD owner");
        let KernelScopeReference::Owner {
            owner: enclosing_owner,
            scope: enclosing_scope,
        } = prepared.definition_facts[hold]
            .presentation
            .containing_scope
        else {
            panic!(
                "nested HOLD must retain an enclosing owner scope: {:?}",
                prepared.definition_facts[hold]
                    .presentation
                    .containing_scope
            )
        };
        let enclosing_scope_row = prepared.definition_facts[enclosing_owner.0 as usize]
            .presentation
            .scopes
            .get(enclosing_scope.0 as usize)
            .expect("nested HOLD enclosing scope exists");
        assert!(
            matches!(
                enclosing_scope_row.origin,
                KernelScopeOrigin::StatementBody { .. }
            ) && enclosing_scope_row.owner.is_some(),
            "nested HOLD must be anchored in its authored field-body scope: {enclosing_scope_row:?}"
        );

        let input = KernelProjectInput::new(
            prepared.project_input,
            prepared.definition_facts,
            prepared.definition_keys,
        )
        .expect("nested checked-presentation input validates");
        let mut session = KernelSession::new(input.clone());
        let checked = session
            .check(CheckDemand::CheckedImage)
            .expect("nested checked-presentation project solves");
        let KernelCheckProduct::CheckedImage(snapshot) = checked.product else {
            unreachable!()
        };
        let layout = KernelCheckedLinkLayout::new(&input, &snapshot)
            .expect("nested checked-presentation scopes link");
        let expected = layout
            .scope(
                enclosing_owner,
                KernelScopeReference::Local(enclosing_scope),
            )
            .unwrap();
        assert_eq!(layout.definitions()[hold].containing_scope, expected);
        assert_ne!(
            expected.0, 0,
            "nested owner must not collapse to project root"
        );
    }

    #[test]
    fn hold_update_statements_read_the_private_state_capability() {
        let source = concat!(
            "FUNCTION toggle(trigger) {\n",
            "    False |> HOLD state {\n",
            "        trigger |> THEN {\n",
            "            state |> WHEN {\n",
            "                False => True\n",
            "                True => False\n",
            "            }\n",
            "        }\n",
            "    }\n",
            "}\n",
            "result: toggle(trigger: True)\n",
        );
        let project =
            parse_project_syntax("app/RUN.bn", [("app/RUN.bn".to_owned(), source.to_owned())])
                .expect("parse HOLD self-read fixture");
        let oracle = kernel_owner_oracle(&project);
        let function = oracle
            .supported
            .iter()
            .find(|owner| matches!(&owner.owner, StableCheckOwnerKey::Item(key) if key.item_route.segments().last().is_some_and(|segment| segment.names == ["toggle"])))
            .unwrap_or_else(|| panic!("HOLD self-read must compile: {:#?}", oracle.unsupported));
        let expected = Type::VariantSet(
            vec![
                Variant::Tag("False".to_owned()),
                Variant::Tag("True".to_owned()),
            ]
            .into(),
        );
        assert_eq!(function.result.ty, expected);
    }

    #[test]
    fn linked_hold_initial_uses_the_parser_owned_pipeline_input() {
        let source = concat!(
            "FUNCTION hold_proof() {\n",
            "    0\n",
            "    |> HOLD count {\n",
            "        LATEST {}\n",
            "    }\n",
            "}\n",
        );
        let project =
            parse_project_syntax("app/RUN.bn", [("app/RUN.bn".to_owned(), source.to_owned())])
                .expect("parse linked HOLD input fixture");
        let oracle = kernel_owner_oracle(&project);
        let function = oracle
            .supported
            .iter()
            .find(|owner| matches!(&owner.owner, StableCheckOwnerKey::Item(key) if key.item_route.segments().last().is_some_and(|segment| segment.names == ["hold_proof"])))
            .unwrap_or_else(|| {
                panic!(
                    "linked HOLD input must remain in its parser-owned expression tree: {:#?}",
                    oracle.unsupported
                )
            });
        assert_eq!(function.result.ty, Type::Number);
        let [state] = function.states.as_ref() else {
            panic!(
                "fieldless function HOLD must own exactly one persistent state row: {:#?}",
                function.states
            )
        };
        assert_eq!(state.kind, CheckedStateKind::Hold);
        assert_eq!(state.flow_type.ty, Type::Number);
        assert_eq!(state.binding_declaration, state.declaration);
        assert!(state.path.projection.is_empty());
        assert!(
            oracle
                .unsupported
                .iter()
                .all(|(owner, _)| matches!(owner, StableCheckOwnerKey::UnitRoot(_))),
            "only the declaration-less unit root may be non-executable: {:#?}",
            oracle.unsupported
        );
    }

    #[test]
    fn source_hold_and_mapped_list_resources_match_checked_authorities() {
        let source = concat!(
            "store: [\n",
            "    pulse: SOURCE\n",
            "    state:\n",
            "        0 |> HOLD state {\n",
            "            pulse |> THEN { 1 }\n",
            "        }\n",
            "    rows:\n",
            "        LIST {\n",
            "            1\n",
            "            2\n",
            "        }\n",
            "        |> List/map(item, new: item)\n",
            "]\n",
        );
        let parsed = parse_source("app/RUN.bn", source).expect("parse resource fixture");
        let checked = boon_typecheck::check_program(&parsed);
        assert!(
            !checked.report.has_errors(),
            "current checker diagnostics: {:#?}",
            checked.report.diagnostics
        );
        let (checked, _) = checked
            .program
            .expect("resource fixture checks")
            .into_parts();
        let project =
            parse_project_syntax("app/RUN.bn", [("app/RUN.bn".to_owned(), source.to_owned())])
                .expect("parse project resource fixture");
        let [checked_source] = checked.sources.as_slice() else {
            panic!("resource fixture must expose one checked SOURCE")
        };
        let report = kernel_owner_oracle_with_source_payloads(
            &project,
            &BTreeMap::from([(
                "store.pulse".to_owned(),
                checked_source.payload_type.clone(),
            )]),
        );
        assert!(
            report
                .unsupported
                .iter()
                .all(|(owner, _)| matches!(owner, StableCheckOwnerKey::UnitRoot(_))),
            "every declared resource owner must compile: {:#?}",
            report.unsupported
        );

        let mut checked_expression_by_stable = BTreeMap::new();
        let mut stable_by_checked_expression = BTreeMap::new();
        for owner in project.stable_check_owner_keys() {
            let view = project
                .owner_view(&owner)
                .expect("resource owner has a view");
            for (expression, stable) in view.expressions().zip(view.stable_expression_keys()) {
                let Some(checked_expression) = project
                    .expression_slot(expression.id)
                    .and_then(|slot| checked.expressions.get(slot))
                else {
                    continue;
                };
                checked_expression_by_stable.insert(stable.clone(), checked_expression.id);
                stable_by_checked_expression.insert(checked_expression.id, stable);
            }
        }
        let mismatches = resource_inventory_mismatches(
            &report,
            &checked,
            &checked_expression_by_stable,
            &stable_by_checked_expression,
            &project,
        );
        assert!(mismatches.is_empty(), "resource parity: {mismatches:#?}");
        assert_eq!(
            report
                .supported
                .iter()
                .map(|owner| owner.source_resources.len())
                .sum::<usize>(),
            1
        );
        assert_eq!(
            report
                .supported
                .iter()
                .map(|owner| owner.states.len())
                .sum::<usize>(),
            1
        );
        assert_eq!(
            report
                .supported
                .iter()
                .map(|owner| owner.lists.len())
                .sum::<usize>(),
            1
        );
        let lists = report
            .supported
            .iter()
            .flat_map(|owner| owner.lists.iter())
            .collect::<Vec<_>>();
        let [list] = lists.as_slice() else {
            panic!("resource fixture must expose one persistent LIST")
        };
        assert!(
            matches!(
                list.declaration,
                KernelOwnerOracleLexicalTarget::OwnerPublic(_)
            ),
            "the child LIST must retain its parent field declaration authority"
        );
        assert_eq!(list.item_type, Type::Number);
    }

    #[test]
    fn hold_capability_reaches_nested_pipe_callback_arguments() {
        let source = concat!(
            "FUNCTION preserve(rows) {\n",
            "    False |> HOLD state {\n",
            "        rows\n",
            "        |> List/map(item, new: item |> THEN { state })\n",
            "        |> List/latest()\n",
            "    }\n",
            "}\n",
            "result: preserve(rows: LIST { True })\n",
        );
        let project =
            parse_project_syntax("app/RUN.bn", [("app/RUN.bn".to_owned(), source.to_owned())])
                .expect("parse nested HOLD callback fixture");
        let oracle = kernel_owner_oracle(&project);
        let function = oracle
            .supported
            .iter()
            .find(|owner| matches!(&owner.owner, StableCheckOwnerKey::Item(key) if key.item_route.segments().last().is_some_and(|segment| segment.names == ["preserve"])))
            .unwrap_or_else(|| {
                panic!(
                    "HOLD capability must reach pipe arguments: {:#?}",
                    oracle.unsupported
                )
            });
        assert!(
            !matches!(function.result.ty, Type::UnresolvedShape { .. }),
            "the nested callback must retain a resolved HOLD dependency"
        );
    }

    #[test]
    fn parsed_value_reads_share_one_component_without_owner_reconstruction() {
        let source = concat!(
            "base: [\n",
            "    value: 1\n",
            "    label: TEXT { base }\n",
            "]\n",
            "copy: base.value\n",
        );
        let project =
            parse_project_syntax("app/RUN.bn", [("app/RUN.bn".to_owned(), source.to_owned())])
                .expect("parse cross-owner value-read fixture");
        let oracle = kernel_owner_oracle(&project);
        let results = oracle
            .supported
            .iter()
            .filter_map(|owner| {
                let StableCheckOwnerKey::Item(key) = &owner.owner else {
                    return None;
                };
                let name = key.item_route.segments().last()?.names.first()?;
                matches!(name.as_str(), "base" | "copy")
                    .then(|| (name.clone(), owner.result.ty.clone()))
            })
            .collect::<BTreeMap<_, _>>();
        assert_eq!(
            results["base"],
            Type::object(ObjectShape::from_ordered_fields(
                [
                    ("value".to_owned(), Type::Number),
                    ("label".to_owned(), Type::Text),
                ],
                false,
            ))
        );
        assert_eq!(results["copy"], Type::Number);
        assert!(
            oracle
                .unsupported
                .iter()
                .all(|(owner, _)| matches!(owner, StableCheckOwnerKey::UnitRoot(_))),
            "both value owners must solve in one component: {:#?}",
            oracle.unsupported
        );
    }

    #[test]
    fn multiline_fields_alias_child_owner_results_without_expression_guessing() {
        let source = concat!(
            "store: [\n",
            "    rows:\n",
            "        LIST {\n",
            "            1\n",
            "            2\n",
            "        }\n",
            "]\n",
        );
        let project =
            parse_project_syntax("app/RUN.bn", [("app/RUN.bn".to_owned(), source.to_owned())])
                .expect("parse multiline child-owner fixture");
        let oracle = kernel_owner_oracle(&project);
        let rows = oracle
            .supported
            .iter()
            .find(|owner| {
                matches!(&owner.owner, StableCheckOwnerKey::Item(owner) if owner.item_route.segments().last().is_some_and(|segment| segment.names == ["rows"]))
            })
            .unwrap_or_else(|| {
                panic!(
                    "multiline rows field must alias its LIST owner result: {:#?}",
                    oracle.unsupported
                )
            });
        assert!(
            rows.result_expression.is_none(),
            "the public multiline field result is a declaration authority, not a guessed expression"
        );
        assert_eq!(rows.result.ty, Type::List(Type::shared(Type::Number)));
        assert!(
            oracle
                .unsupported
                .iter()
                .all(|(owner, _)| matches!(owner, StableCheckOwnerKey::UnitRoot(_))),
            "the store record, rows field, and LIST owner must share one component: {:#?}",
            oracle.unsupported
        );
    }

    #[test]
    fn hold_updates_widen_in_one_component_without_recursive_replay() {
        let source = concat!(
            "state:\n",
            "    NotStarted |> HOLD state {\n",
            "        LATEST {\n",
            "            True |> THEN { WaveformOpened[timescale: TEXT { ns }] }\n",
            "            False |> THEN { Failed }\n",
            "        }\n",
            "    }\n",
        );
        let project =
            parse_project_syntax("app/RUN.bn", [("app/RUN.bn".to_owned(), source.to_owned())])
                .expect("parse HOLD residual fixture");
        let oracle = kernel_owner_oracle(&project);
        let state = oracle
            .supported
            .iter()
            .find(|owner| {
                matches!(&owner.owner, StableCheckOwnerKey::Item(owner) if owner.item_route.segments().last().is_some_and(|segment| segment.kind == UnitItemKind::Field && segment.names == ["state"]))
            })
            .unwrap_or_else(|| panic!("state field must consume its HOLD result: {:#?}", oracle.unsupported));
        assert_eq!(
            state.result.ty,
            Type::VariantSet(
                vec![
                    Variant::Tag("Failed".to_owned()),
                    Variant::Tag("NotStarted".to_owned()),
                    Variant::tagged(
                        "WaveformOpened".to_owned(),
                        ObjectShape::from_ordered_fields(
                            [("timescale".to_owned(), Type::Text)],
                            false,
                        ),
                    ),
                ]
                .into(),
            )
        );
        assert!(
            oracle
                .unsupported
                .iter()
                .all(|(owner, _)| matches!(owner, StableCheckOwnerKey::UnitRoot(_))),
            "the public field and HOLD owner must solve together: {:#?}",
            oracle.unsupported
        );
    }

    #[test]
    fn owner_local_state_reads_compile_as_explicit_lexical_cycles() {
        let source = concat!(
            "state:\n",
            "    0 |> HOLD state {\n",
            "        True |> THEN { state }\n",
            "    }\n",
        );
        let project =
            parse_project_syntax("app/RUN.bn", [("app/RUN.bn".to_owned(), source.to_owned())])
                .expect("parse owner-local state cycle fixture");
        let oracle = kernel_owner_oracle(&project);
        let state = oracle
            .supported
            .iter()
            .find(|owner| {
                matches!(&owner.owner, StableCheckOwnerKey::Item(owner) if owner.item_route.segments().last().is_some_and(|segment| segment.kind == UnitItemKind::Field && segment.names == ["state"]))
            })
            .unwrap_or_else(|| {
                panic!(
                    "state field must compile its self read as a lexical cycle: {:#?}",
                    oracle.unsupported
                )
            });
        assert_eq!(state.result.ty, Type::Number);
        assert!(
            oracle
                .unsupported
                .iter()
                .all(|(owner, _)| matches!(owner, StableCheckOwnerKey::UnitRoot(_))),
            "no owner-local value read may be rejected: {:#?}",
            oracle.unsupported
        );
    }

    #[test]
    fn real_example_coverage_is_deterministic_and_explicit() {
        for (disk_relative, project_path) in [
            ("../../examples/counter.bn", "examples/counter.bn"),
            ("../../examples/todomvc.bn", "examples/todomvc.bn"),
        ] {
            let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(disk_relative);
            let source = fs::read_to_string(&path).expect("read example source");
            let parsed = parse_source(project_path, &source).expect("parse checked example");
            let checked = boon_typecheck::check_program(&parsed);
            assert!(
                !checked.report.has_errors(),
                "current checker diagnostics for {project_path}: {:#?}",
                checked.report.diagnostics
            );
            let (checked, _) = checked.program.expect("example checks").into_parts();
            let checked_by_stable_key = parsed
                .ast
                .expressions
                .iter()
                .filter_map(|expression| {
                    Some((
                        parsed.stable_expression_key(expression.id)?,
                        checked.expressions.get(expression.id)?.flow_type.clone(),
                    ))
                })
                .collect::<BTreeMap<_, _>>();
            let project = parse_project_syntax(project_path, [(project_path.to_owned(), source)])
                .expect("parse example project");
            let source_payloads = boon_typecheck::project_source_payload_abi_types(&project)
                .expect("project closed SOURCE ABI without running the checker");
            let kernel_started = Instant::now();
            let first = kernel_owner_oracle_with_source_payloads(&project, &source_payloads);
            let kernel_elapsed = kernel_started.elapsed();
            let second = kernel_owner_oracle_with_source_payloads(&project, &source_payloads);
            assert_eq!(
                first, second,
                "kernel oracle must be deterministic for {project_path}"
            );
            assert!(
                !first.supported.is_empty(),
                "first kernel slice must cover real owners in {project_path}: {:#?}",
                first.unsupported
            );
            assert_eq!(
                first.supported.len() + first.container_owners.len() + first.unsupported.len(),
                project.stable_check_owner_keys().count(),
                "every example owner must be classified explicitly"
            );
            assert_eq!(
                first.currentness.len(),
                first.supported.len(),
                "every solved definition must publish one exact currentness receipt"
            );
            assert!(
                first
                    .currentness
                    .iter()
                    .zip(&first.supported)
                    .all(|(receipt, owner)| receipt.owner == owner.owner
                        && receipt.fingerprint_v11 != [0; 32]),
                "receipt order and ownership must match the dense definition table"
            );
            assert!(
                first.dependency_edges >= first.reverse_consumer_edges,
                "reverse definition consumers are a deduplicated dependency projection"
            );
            for owner in &first.supported {
                assert_owner_matches_current(
                    owner,
                    &checked_by_stable_key,
                    &checked,
                    &project,
                    &format!("kernel/current {project_path} owner {:#?}", owner.owner),
                );
            }
            assert!(first.work.operations > 0);
            assert!(first.work.activations < first.work.operations.saturating_mul(8));
            if std::env::var_os("BOON_KERNEL_ORACLE_TRACE").is_some() {
                eprintln!(
                    "kernel-oracle {project_path}: supported={}/{} operations={} activations={} mutations={} dynamic_edges={} elapsed_us={}",
                    first.supported.len(),
                    first.supported.len() + first.container_owners.len() + first.unsupported.len(),
                    first.work.operations,
                    first.work.activations,
                    first.work.mutations,
                    first.work.dynamic_dependency_edges,
                    kernel_elapsed.as_micros(),
                );
            }
        }
    }

    #[test]
    #[ignore = "directional NovyWave kernel timing probe"]
    fn novywave_kernel_timing_probe() {
        let source_path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/novywave/RUN.bn");
        let bundle_started = Instant::now();
        let (entrypoint, units) = crate::compiler_source_project_for_path(&source_path)
            .expect("load NovyWave source bundle");
        let bundle_us = elapsed_us(bundle_started.elapsed());

        let parse_started = Instant::now();
        let project = parse_project_syntax(
            entrypoint.clone(),
            units
                .iter()
                .map(|unit| (unit.path.clone(), unit.source.clone())),
        )
        .expect("parse NovyWave unit-native project");
        let parse_us = elapsed_us(parse_started.elapsed());

        let source_abi_started = Instant::now();
        let source_payloads = boon_typecheck::project_source_payload_abi_types(&project)
            .expect("project NovyWave SOURCE ABI without running the checker");
        let source_abi_us = elapsed_us(source_abi_started.elapsed());

        let (report, timings) =
            profile_kernel_owner_oracle_with_source_payloads(&project, &source_payloads);
        if std::env::var_os("BOON_KERNEL_PRODUCTION_DIAGNOSTICS").is_some() {
            let diagnostics_started = Instant::now();
            let diagnostics = compiler_diagnostics_from_kernel(
                project.clone(),
                boon_parser::ParseWorkCounters::default(),
                parse_us as f64 / 1_000.0,
            )
            .expect("compile NovyWave production diagnostics through KernelSession");
            let wall_us = elapsed_us(diagnostics_started.elapsed());
            assert!(
                diagnostics.diagnostics().is_empty(),
                "NovyWave production diagnostics: {:#?}",
                diagnostics.diagnostics()
            );
            eprintln!(
                "kernel-novywave production_diagnostics=true profile={} wall_us={} parse_us={} typecheck_us={} total_us={} materialized_definitions=0 sealed_definitions=0 operations={} activations={}",
                if cfg!(debug_assertions) {
                    "debug"
                } else {
                    "release"
                },
                wall_us,
                parse_us,
                (diagnostics.profile.typecheck_ms * 1_000.0) as u64,
                (diagnostics.profile.total_ms * 1_000.0) as u64,
                diagnostics.profile.kernel_solve_work.operations,
                diagnostics.profile.kernel_solve_work.activations,
            );
        }
        eprintln!(
            "kernel-novywave definition_artifacts expression_rows={} scope_rows={} execution_shape_rows={} statement_rows={} declaration_rows={} lexical_binding_rows={} source_resource_rows={} hold_state_rows={} persistent_list_rows={} collection_rows={} source_expression_rows={} call_rows={} host_effect_rows={} dependency_edges={} reverse_consumer_edges={}",
            report
                .supported
                .iter()
                .map(|owner| owner.expressions.len())
                .sum::<usize>(),
            report
                .supported
                .iter()
                .map(|owner| owner.presentation_scope_count)
                .sum::<usize>(),
            report
                .supported
                .iter()
                .map(|owner| owner.execution_shape_count)
                .sum::<usize>(),
            report
                .supported
                .iter()
                .map(|owner| owner.statements.len())
                .sum::<usize>(),
            report
                .supported
                .iter()
                .map(|owner| owner.declarations.len())
                .sum::<usize>(),
            report
                .supported
                .iter()
                .map(|owner| owner.lexical_bindings.len())
                .sum::<usize>(),
            report
                .supported
                .iter()
                .map(|owner| owner.source_resources.len())
                .sum::<usize>(),
            report
                .supported
                .iter()
                .map(|owner| owner.states.len())
                .sum::<usize>(),
            report
                .supported
                .iter()
                .map(|owner| owner.lists.len())
                .sum::<usize>(),
            report
                .supported
                .iter()
                .map(|owner| owner.collections.len())
                .sum::<usize>(),
            report
                .supported
                .iter()
                .map(|owner| owner.sources.len())
                .sum::<usize>(),
            report
                .supported
                .iter()
                .map(|owner| owner.calls.len())
                .sum::<usize>(),
            report
                .supported
                .iter()
                .map(|owner| owner.effects.len())
                .sum::<usize>(),
            report.dependency_edges,
            report.reverse_consumer_edges,
        );

        if timings.interface_projection_us > 0 {
            let diagnostics_kernel_us = timings
                .total_us
                .saturating_sub(timings.checked_image_us)
                .saturating_sub(timings.checked_link_layout_us)
                .saturating_sub(timings.artifact_projection_us)
                .saturating_add(timings.interface_projection_us);
            let diagnostics_retained_snapshot_us =
                source_abi_us.saturating_add(diagnostics_kernel_us);
            let diagnostics_candidate_us =
                parse_us.saturating_add(diagnostics_retained_snapshot_us);
            eprintln!(
                "kernel-novywave demand=diagnostics profile={} candidate_total_us={} retained_snapshot_total_us={} kernel_total_us={} graph_solve_us={} interface_projection_us={} materialized_definitions=0 sealed_definitions=0",
                if cfg!(debug_assertions) {
                    "debug"
                } else {
                    "release"
                },
                diagnostics_candidate_us,
                diagnostics_retained_snapshot_us,
                diagnostics_kernel_us,
                timings.graph_solve_us,
                timings.interface_projection_us,
            );
        }

        if std::env::var_os("BOON_KERNEL_CANDIDATE_ONLY").is_some() {
            if let Some((owner, reason)) = report
                .unsupported
                .iter()
                .find(|(_, reason)| reason.starts_with("kernel project solve failed:"))
                .or_else(|| report.unsupported.first())
            {
                eprintln!("kernel-novywave first_unsupported_owner={owner:?} reason={reason}");
            }
            let retained_snapshot_total_us = source_abi_us.saturating_add(timings.total_us);
            let candidate_total_us = parse_us.saturating_add(retained_snapshot_total_us);
            eprintln!(
                "kernel-novywave candidate_only=true parity=not_run profile={} bundle_us={} parse_us={} source_abi_us={} retained_snapshot_total_us={} candidate_total_us={} kernel_total_us={} compile_us={} solve_us={} graph_solve_us={} interface_projection_us={} checked_image_us={} checked_link_layout_us={} checked_link_references={} solved_owners={} container_owners={} unsupported_owners={} residual_modules={} residual_frames={} acyclic_residual_frames={} invocation_frames={} direct_result_summaries={} summary_definition_nodes={} summary_constant_folded_nodes={} summary_selector_fused_records={} summary_deduplicated_nodes={} summary_pruned_nodes={} summary_pruned_inputs={} summary_invoke_nodes={} linked_operations={} scheduled_work_items={} acyclic_initial_work_items={} dominant_module_owner={} dominant_module_operations={} dominant_module_frames={} dominant_module_linked_operations={} variables={} activations={} unify_activations={} publish_activations={} projection_activations={} select_activations={} record_activations={} summary_call_activations={} summary_node_evaluations={} mutations={} term_materializations={} term_intern_requests={} term_intern_hits={} term_intern_requests_by_kind={:?} term_intern_hits_by_kind={:?} structural_widen_requests={} structural_widen_hits={} dynamic_edges={}",
                if cfg!(debug_assertions) {
                    "debug"
                } else {
                    "release"
                },
                bundle_us,
                parse_us,
                source_abi_us,
                retained_snapshot_total_us,
                candidate_total_us,
                timings.total_us,
                timings.program_compile_us,
                timings.solve_us,
                timings.graph_solve_us,
                timings.interface_projection_us,
                timings.checked_image_us,
                timings.checked_link_layout_us,
                timings.checked_link_references,
                timings.solved_owners,
                timings.container_owners,
                timings.unsupported_owners,
                timings.compile_work.residual_type_modules,
                timings.compile_work.residual_frames,
                timings.compile_work.acyclic_residual_frames,
                timings.compile_work.invocation_frames,
                timings.compile_work.direct_result_summaries,
                timings.compile_work.summary_definition_nodes,
                timings.compile_work.summary_constant_folded_nodes,
                timings.compile_work.summary_selector_fused_records,
                timings.compile_work.summary_deduplicated_nodes,
                timings.compile_work.summary_pruned_nodes,
                timings.compile_work.summary_pruned_inputs,
                timings.compile_work.summary_invoke_nodes,
                timings.compile_work.linked_operations,
                timings.compile_work.scheduled_work_items,
                timings.compile_work.acyclic_initial_operations,
                timings.compile_work.dominant_module_owner,
                timings.compile_work.dominant_module_operations,
                timings.compile_work.dominant_module_frames,
                timings.compile_work.dominant_module_linked_operations,
                report.work.variables,
                report.work.activations,
                report.work.unify_activations,
                report.work.publish_activations,
                report.work.projection_activations,
                report.work.select_activations,
                report.work.record_activations,
                report.work.summary_call_activations,
                report.work.summary_node_evaluations,
                report.work.mutations,
                report.work.term_materializations,
                report.work.term_intern_requests,
                report.work.term_intern_hits,
                report.work.term_intern_requests_by_kind,
                report.work.term_intern_hits_by_kind,
                report.work.structural_widen_requests,
                report.work.structural_widen_hits,
                report.work.dynamic_dependency_edges,
            );
            eprintln!(
                "kernel-novywave residual_module_ranking={:?}",
                timings
                    .compile_work
                    .residual_module_ranking
                    .iter()
                    .filter(|module| module.linked_operations > 0)
                    .map(|module| (
                        report
                            .supported
                            .get(module.owner as usize)
                            .map(|owner| &owner.owner),
                        module.operations,
                        module.frames,
                        module.linked_operations,
                    ))
                    .collect::<Vec<_>>()
            );
            eprintln!(
                "kernel-novywave summary_definition_ranking={:?}",
                report
                    .work
                    .summary_definition_ranking
                    .iter()
                    .filter(|definition| definition.node_evaluations > 0)
                    .map(|definition| (
                        definition.definition,
                        report
                            .supported
                            .get(definition.definition as usize)
                            .map(|owner| &owner.owner),
                        definition.program_evaluations,
                        definition.node_evaluations,
                    ))
                    .collect::<Vec<_>>()
            );
            if std::env::var_os("BOON_KERNEL_ORACLE_UNSUPPORTED_TRACE").is_some() {
                for (owner, reason) in &report.unsupported {
                    eprintln!("kernel-novywave unsupported owner={owner:?} reason={reason}");
                }
            }
            return;
        }

        // The old compiler runs only as the differential oracle. It is timed
        // outside the candidate parse + ABI + kernel path and contributes no
        // input to the dense solve.
        let oracle_check_started = Instant::now();
        let checked = crate::check_diagnostics_source(crate::CompilerCheckRequest::source_units(
            &entrypoint,
            &units,
            crate::ProgramRole::Client,
        ))
        .expect("check NovyWave differential oracle");
        let oracle_check_us = elapsed_us(oracle_check_started.elapsed());
        assert!(
            checked.output.report.diagnostics.is_empty(),
            "NovyWave timing fixture diagnostics: {:#?}",
            checked.output.report.diagnostics
        );
        let fields = checked
            .output
            .checked_program_fields()
            .expect("NovyWave diagnostics own checked fields");
        assert_eq!(
            report.supported.len() + report.container_owners.len() + report.unsupported.len(),
            project.stable_check_owner_keys().count(),
            "every NovyWave owner must be classified"
        );
        assert!(
            !report.supported.is_empty(),
            "NovyWave must exercise at least one dense owner: {:#?}",
            report.unsupported
        );
        assert_eq!(timings.solved_owners, report.supported.len());
        assert_eq!(timings.container_owners, report.container_owners.len());
        assert_eq!(timings.unsupported_owners, report.unsupported.len());
        let mut checked_by_stable_key = BTreeMap::new();
        let mut checked_expression_by_stable = BTreeMap::new();
        let mut stable_by_checked_expression = BTreeMap::new();
        for owner in project.stable_check_owner_keys() {
            let view = project
                .owner_view(&owner)
                .expect("NovyWave owner has a view");
            for (expression, stable_key) in view.expressions().zip(view.stable_expression_keys()) {
                let Some(checked_expression) = project
                    .expression_slot(expression.id)
                    .and_then(|slot| fields.expressions.get(slot))
                else {
                    continue;
                };
                checked_by_stable_key
                    .insert(stable_key.clone(), checked_expression.flow_type.clone());
                checked_expression_by_stable.insert(stable_key.clone(), checked_expression.id);
                stable_by_checked_expression.insert(checked_expression.id, stable_key);
            }
        }
        if let Some(pattern) = std::env::var_os("BOON_KERNEL_ORACLE_TRACE_OWNER") {
            let pattern = pattern.to_string_lossy();
            for owner in report
                .supported
                .iter()
                .filter(|owner| format!("{:?}", owner.owner).contains(pattern.as_ref()))
            {
                eprintln!(
                    "kernel-owner-trace solved owner={:?} formals={:?} result={:?}",
                    owner.owner, owner.formals, owner.result
                );
                for (index, (stable, flow)) in owner.expressions.iter().enumerate() {
                    let current = checked_by_stable_key.get(stable);
                    let current_mode = current.map(|current| current.mode);
                    eprintln!(
                        "kernel-owner-trace solved node={index} mode={:?} current_mode={current_mode:?} stable={stable:?} type={:?} current_type={:?}",
                        flow.mode,
                        flow.ty,
                        current.map(|current| &current.ty),
                    );
                }
                for call in &owner.calls {
                    eprintln!(
                        "kernel-owner-trace call expression={:?} target={:?} substitutions={:?} result={:?}",
                        call.expression, call.target, call.type_substitutions, call.result,
                    );
                }
            }
        }
        let mut mismatches = report
            .supported
            .iter()
            .filter_map(|owner| {
                owner_mismatch(
                    owner,
                    &checked_by_stable_key,
                    fields,
                    &project,
                    &format!("NovyWave kernel/current owner {:?}", owner.owner),
                )
            })
            .collect::<Vec<_>>();
        mismatches.extend(callable_interface_mismatches(&report, fields, &project));
        mismatches.extend(call_and_effect_inventory_mismatches(
            &report,
            fields,
            &checked_expression_by_stable,
            &stable_by_checked_expression,
            &project,
        ));
        mismatches.extend(collection_and_source_inventory_mismatches(
            &report,
            fields,
            &checked_expression_by_stable,
            &stable_by_checked_expression,
        ));
        mismatches.extend(resource_inventory_mismatches(
            &report,
            fields,
            &checked_expression_by_stable,
            &stable_by_checked_expression,
            &project,
        ));
        mismatches.extend(statement_inventory_mismatches(
            &report,
            fields,
            &stable_by_checked_expression,
            &project,
        ));
        mismatches.extend(lexical_plan_inventory_mismatches(&report, &project));
        if !mismatches.is_empty() {
            eprintln!("kernel-novywave parity_mismatch_count={}", mismatches.len());
            let mut mismatch_classes = BTreeMap::<&str, usize>::new();
            for mismatch in &mismatches {
                let class = if mismatch.contains("declaration origin missing from kernel") {
                    "declaration-missing"
                } else if mismatch.contains("declaration origin absent from lexical plan") {
                    "declaration-extra"
                } else if mismatch.contains("kernel declaration") {
                    "declaration-row"
                } else if mismatch.contains("lexical binding")
                    && mismatch.contains("projection/access")
                {
                    "lexical-projection"
                } else if mismatch.contains("lexical binding") && mismatch.contains("target") {
                    "lexical-target"
                } else if mismatch.contains("has no kernel binding") {
                    "lexical-missing"
                } else if mismatch.contains("has no lexical plan read") {
                    "lexical-extra"
                } else if mismatch.contains("lexical") {
                    "lexical-other"
                } else if mismatch.contains("callable interface") {
                    "callable-interface"
                } else if mismatch.contains("call") {
                    "call"
                } else if mismatch.contains("SOURCE resource") {
                    "resource-source"
                } else if mismatch.contains("HOLD state") {
                    "resource-state"
                } else if mismatch.contains("LIST resource") {
                    "resource-list"
                } else if mismatch.contains("statement") {
                    "statement"
                } else if mismatch.contains("collection") || mismatch.contains("source") {
                    "collection-or-source"
                } else {
                    "owner-flow"
                };
                *mismatch_classes.entry(class).or_default() += 1;
            }
            eprintln!("kernel-novywave parity_mismatch_classes={mismatch_classes:?}");
            const MISMATCH_SAMPLE_LIMIT: usize = 24;
            for mismatch in mismatches.iter().take(MISMATCH_SAMPLE_LIMIT) {
                eprintln!("kernel-novywave parity_mismatch={mismatch}");
            }
            if mismatches.len() > MISMATCH_SAMPLE_LIMIT {
                eprintln!(
                    "kernel-novywave parity_mismatch_omitted={}",
                    mismatches.len() - MISMATCH_SAMPLE_LIMIT
                );
            }
            panic!(
                "NovyWave kernel/current differential found {} owner mismatches",
                mismatches.len()
            );
        }
        let unsupported_classes = report.unsupported.iter().fold(
            BTreeMap::<String, usize>::new(),
            |mut classes, (_, reason)| {
                let class = unsupported_reason_class(reason);
                *classes.entry(class).or_default() += 1;
                classes
            },
        );
        let candidate_total_us = parse_us
            .saturating_add(source_abi_us)
            .saturating_add(timings.total_us);
        let retained_snapshot_total_us = source_abi_us.saturating_add(timings.total_us);
        let candidate_with_bundle_us = bundle_us.saturating_add(candidate_total_us);
        eprintln!(
            "kernel-novywave profile={} bundle_us={} parse_us={} source_abi_us={} retained_snapshot_total_us={} candidate_total_us={} candidate_with_bundle_us={} oracle_check_us={} legacy_parse_ms={:.3} legacy_typecheck_ms={:.3} kernel_total_us={} owner_projection_us={} direct_projection_us={} dependency_pruning_us={} program_compile_us={} solve_us={} graph_solve_us={} interface_projection_us={} checked_image_us={} checked_link_layout_us={} checked_link_references={} artifact_projection_us={} projected_owners={} solved_owners={} container_owners={} unsupported_owners={} definition_modules={} principal_expressions={} residual_type_modules={} residual_module_operations={} residual_module_terms={} residual_frames={} linked_operations={} scheduled_work_items={} linked_terms={} acyclic_initial_operations={} compiled_call_sites={} invocation_frames={} reused_invocation_frames={} principal_result_reuses={} principal_expression_reuses={} pruned_invocation_expressions={} specialization_plans={} reused_specialization_plans={} max_call_depth={} variables={} operations={} activations={} unify_activations={} publish_activations={} projection_activations={} select_activations={} record_activations={} mutations={} dynamic_edges={}",
            if cfg!(debug_assertions) {
                "debug"
            } else {
                "release"
            },
            bundle_us,
            parse_us,
            source_abi_us,
            retained_snapshot_total_us,
            candidate_total_us,
            candidate_with_bundle_us,
            oracle_check_us,
            checked.profile.parse_ms,
            checked.profile.typecheck_ms,
            timings.total_us,
            timings.owner_projection_us,
            timings.direct_projection_us,
            timings.dependency_pruning_us,
            timings.program_compile_us,
            timings.solve_us,
            timings.graph_solve_us,
            timings.interface_projection_us,
            timings.checked_image_us,
            timings.checked_link_layout_us,
            timings.checked_link_references,
            timings.artifact_projection_us,
            timings.projected_owners,
            timings.solved_owners,
            timings.container_owners,
            timings.unsupported_owners,
            timings.compile_work.definition_modules,
            timings.compile_work.principal_expressions,
            timings.compile_work.residual_type_modules,
            timings.compile_work.residual_module_operations,
            timings.compile_work.residual_module_terms,
            timings.compile_work.residual_frames,
            timings.compile_work.linked_operations,
            timings.compile_work.scheduled_work_items,
            timings.compile_work.linked_terms,
            timings.compile_work.acyclic_initial_operations,
            timings.compile_work.compiled_call_sites,
            timings.compile_work.invocation_frames,
            timings.compile_work.reused_invocation_frames,
            timings.compile_work.principal_result_reuses,
            timings.compile_work.principal_expression_reuses,
            timings.compile_work.pruned_invocation_expressions,
            timings.compile_work.specialization_plans,
            timings.compile_work.reused_specialization_plans,
            timings.compile_work.max_call_depth,
            report.work.variables,
            report.work.operations,
            report.work.activations,
            report.work.unify_activations,
            report.work.publish_activations,
            report.work.projection_activations,
            report.work.select_activations,
            report.work.record_activations,
            report.work.mutations,
            report.work.dynamic_dependency_edges,
        );
        eprintln!("kernel-novywave unsupported_classes={unsupported_classes:?}");
        eprintln!(
            "kernel-novywave root_blockers={:?}",
            report
                .root_blockers
                .iter()
                .take(16)
                .map(|blocker| (
                    blocker.affected_owners,
                    unsupported_reason_class(&blocker.reason),
                    &blocker.owner,
                ))
                .collect::<Vec<_>>()
        );
        if std::env::var_os("BOON_KERNEL_ORACLE_UNSUPPORTED_TRACE").is_some() {
            for (owner, reason) in &report.unsupported {
                eprintln!("kernel-novywave unsupported owner={owner:?} reason={reason}");
            }
        }
    }

    fn unsupported_reason_class(reason: &str) -> String {
        if reason.starts_with("unresolved top-level value read") {
            return "unresolved_top_level_value".to_owned();
        }
        if reason.starts_with("owner has no direct or structural result") {
            return "owner has no direct or structural result".to_owned();
        }
        if reason.starts_with("ambiguous top-level value read") {
            return "ambiguous_top_level_value".to_owned();
        }
        if reason.contains("needs a lexical equation") {
            return "owner_local_value_read".to_owned();
        }
        if reason.contains("requires an explicit PASS context") {
            return "missing_pass_context".to_owned();
        }
        if let Some(kind) = reason.strip_prefix("unsupported owner node ") {
            let end = kind.find([' ', '{', '(']).unwrap_or(kind.len());
            return format!("unsupported_node:{}", &kind[..end]);
        }
        if reason.starts_with("depends on unsupported owner") {
            return "dependency_pruned".to_owned();
        }
        if reason.starts_with("imports missing expression") {
            return "missing_import".to_owned();
        }
        reason.to_owned()
    }
}
