use crate::owner_interface::{
    TypeUnifier, add_local_root, alpha_normalize_type, bind_projection, flow_mode_join,
    instantiate_type, merge_effects, pattern_type, true_false_type,
};
use crate::{
    OwnerAbiEvaluationScope, OwnerArgumentKind, OwnerCollectionKind, OwnerConstraintEdgeRole,
    OwnerConstraintNodeKind, OwnerConstraintSeed, OwnerConstraintSummary,
    OwnerInferenceAbiEnvironment, OwnerInterfaceEvaluationScope, OwnerInterfaceSccKey,
    OwnerInterfaceSccResult, OwnerParameterKind, OwnerPublicInterface, OwnerReferenceKind,
    OwnerResultAbiContract, OwnerResultAbiParameterContract, OwnerResultCallTarget,
    OwnerResultExpressionRef, OwnerResultTransfer, OwnerResultTransferNode, OwnerSourceAnchorRole,
    OwnerSourceAnchorSite, OwnerSourceMap, OwnerSymbolResolution, OwnerSyntaxInput,
    OwnerValueAbiForbiddenReason, OwnerValueAbiLookupOutcome, infix_returns_bool,
};
use boon_checked::{
    BytesType, CheckedCallableKind, CheckedEffectSummary, CheckedParameterKind,
    CheckedParameterRequirement, CheckedTypeSubstitution, DiagnosticSeverity, FlowMode, FlowType,
    ObjectShape, Type, TypeDiagnostic, TypeVar, Variant, apply_checked_type_substitution_lookup,
    specialize_checked_call_result, widen_structural_type,
};
use boon_data::{ExactNumber, MAX_BITS_WIDTH};
use boon_syntax::{
    AstExprKind, AstStatementKind, StableCheckOwnerKey, StableExpressionKey, StableStatementKey,
};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::error::Error;
use std::fmt;
use std::sync::Arc;

const OWNER_BODY_INFERENCE_DOMAIN_V1: &[u8] = b"boon.owner-body-inference.v1\0";
const OWNER_BODY_INFERENCE_CONTENT_DOMAIN_V1: &[u8] = b"boon.owner-body-inference-content.v1\0";
const OWNER_BODY_INTERFACE_DOMAIN_V1: &[u8] = b"boon.owner-body-interface-import.v1\0";
const OWNER_BODY_INFERENCE_CURRENTNESS_DOMAIN_V1: &[u8] =
    b"boon.owner-body-inference-currentness.v1\0";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OwnerBodyInferenceError {
    message: String,
}

impl OwnerBodyInferenceError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for OwnerBodyInferenceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for OwnerBodyInferenceError {}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct OwnerInferenceStatementId(pub u32);

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct OwnerInferenceExpressionId(pub u32);

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OwnerBodyRelocationKind {
    ChildValue,
    ValueRead,
    Callable,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OwnerBodyRelocationSite {
    Statement {
        statement: OwnerInferenceStatementId,
    },
    Expression {
        expression: StableExpressionKey,
    },
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct OwnerBodyRelocation {
    pub site: OwnerBodyRelocationSite,
    pub kind: OwnerBodyRelocationKind,
    pub target_owner: StableCheckOwnerKey,
    pub target_expression: Option<StableExpressionKey>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OwnerBodyInterfaceImport {
    pub owner: StableCheckOwnerKey,
    pub interface_fingerprint_v1: [u8; 32],
    pub provider_scc: OwnerInterfaceSccKey,
    pub provider_fingerprint_v1: [u8; 32],
    pub provider_type_variable_count: u32,
}

/// Frozen identity of one interface SCC consumed by owner-local inference.
///
/// `referenced_owners` is the exact subset used by this owner. The full SCC
/// result fingerprint and its alpha namespace remain attached so a cache hit
/// cannot accidentally combine same-numbered `TypeVar`s from another SCC.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct FrozenOwnerInterfaceSccRef {
    pub key: OwnerInterfaceSccKey,
    pub result_fingerprint_v1: [u8; 32],
    pub type_variable_count: u32,
    pub referenced_owners: Box<[StableCheckOwnerKey]>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OwnerBodyInferenceBasis {
    pub owner: StableCheckOwnerKey,
    pub syntax_fingerprint_v1: [u8; 32],
    pub seed_fingerprint_v1: [u8; 32],
    pub summary_fingerprint_v1: [u8; 32],
    pub own_scc: FrozenOwnerInterfaceSccRef,
    pub imports: Box<[FrozenOwnerInterfaceSccRef]>,
    pub inference_abi_fingerprint_v1: [u8; 32],
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct InferredOwnerStatement {
    pub id: OwnerInferenceStatementId,
    pub stable_key: StableStatementKey,
    pub parent: Option<OwnerInferenceStatementId>,
    pub child_index: u32,
    pub kind: AstStatementKind,
    pub expression: Option<OwnerInferenceExpressionId>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct InferredOwnerChild {
    pub owner: StableCheckOwnerKey,
    pub parent: Option<OwnerInferenceStatementId>,
    pub child_index: u32,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct InferredOwnerExpression {
    pub id: OwnerInferenceExpressionId,
    pub stable_key: StableExpressionKey,
    pub flow_type: FlowType,
    pub flush_type: Option<Type>,
    pub direct_effect: CheckedEffectSummary,
    pub kind: AstExprKind,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum InferredOwnerCallableTarget {
    Owner {
        owner: StableCheckOwnerKey,
    },
    Authoritative,
    Unresolved,
    Ambiguous {
        candidates: Box<[StableCheckOwnerKey]>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct InferredOwnerCallInput {
    pub role: OwnerConstraintEdgeRole,
    pub expression: OwnerInferenceExpressionRef,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OwnerInferenceExpressionRef {
    Local {
        expression: OwnerInferenceExpressionId,
    },
    External {
        owner: StableCheckOwnerKey,
        expression: StableExpressionKey,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct InferredOwnerCall {
    pub expression: StableExpressionKey,
    pub function: String,
    pub target: InferredOwnerCallableTarget,
    pub inputs: Box<[InferredOwnerCallInput]>,
    pub type_substitutions: Box<[CheckedTypeSubstitution]>,
    pub contextual_type_variables: Box<[TypeVar]>,
    pub syntax_discriminated_result: bool,
    pub valid: bool,
    pub result: FlowType,
    pub effect: CheckedEffectSummary,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OwnerDiagnosticTemplate {
    pub severity: DiagnosticSeverity,
    pub code: String,
    pub message: String,
    pub site: OwnerSourceAnchorSite,
    pub role: Option<OwnerSourceAnchorRole>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
pub struct OwnerBodyInferenceWork {
    pub statements: u64,
    pub expressions: u64,
    pub local_constraints: u64,
    pub interface_imports: u64,
    pub calls: u64,
    pub unification_steps: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OwnerBodyInferenceReceipt {
    pub statement_rows: u32,
    pub child_rows: u32,
    pub expression_rows: u32,
    pub call_rows: u32,
    pub relocation_rows: u32,
    pub diagnostic_rows: u32,
    pub local_content_digest_v1: [u8; 32],
}

/// Immutable, span-free constraint inference for one stable authored owner.
///
/// This artifact proves that owner-local expression constraints can be solved
/// under frozen public interfaces and reused independently. It deliberately
/// does not claim to contain the complete checked scopes, declarations,
/// resources, calls, substitutions, occurrences, or construction receipts
/// required by a production checked-owner shard. `work` is telemetry and is
/// deliberately excluded from the result fingerprint.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OwnerBodyInferenceShard {
    pub owner: StableCheckOwnerKey,
    pub statements: Box<[InferredOwnerStatement]>,
    pub children: Box<[InferredOwnerChild]>,
    pub expressions: Box<[InferredOwnerExpression]>,
    pub calls: Box<[InferredOwnerCall]>,
    pub relocations: Box<[OwnerBodyRelocation]>,
    pub diagnostics: Box<[OwnerDiagnosticTemplate]>,
    pub effect: CheckedEffectSummary,
    pub receipt: OwnerBodyInferenceReceipt,
    pub work: OwnerBodyInferenceWork,
    fingerprint_v1: [u8; 32],
}

impl OwnerBodyInferenceShard {
    pub const fn fingerprint_v1(&self) -> [u8; 32] {
        self.fingerprint_v1
    }

    pub fn expression(&self, expression: &StableExpressionKey) -> Option<&InferredOwnerExpression> {
        self.expressions
            .iter()
            .find(|candidate| &candidate.stable_key == expression)
    }
}

impl OwnerBodyInferenceShard {
    pub fn owner(&self) -> &StableCheckOwnerKey {
        &self.owner
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OwnerBodyInferenceCurrentnessReceipt {
    basis: OwnerBodyInferenceBasis,
    /// Exact latest provider/interface identities used for this evaluation.
    /// These cannot live on the backdatable semantic shard because an equal
    /// body may be retained after a provider publishes a new equivalent SCC.
    interface_imports: Box<[OwnerBodyInterfaceImport]>,
    result_fingerprint_v1: [u8; 32],
    fingerprint_v1: [u8; 32],
}

impl OwnerBodyInferenceCurrentnessReceipt {
    pub const fn basis(&self) -> &OwnerBodyInferenceBasis {
        &self.basis
    }

    pub fn interface_imports(&self) -> &[OwnerBodyInterfaceImport] {
        &self.interface_imports
    }

    pub const fn result_fingerprint_v1(&self) -> [u8; 32] {
        self.result_fingerprint_v1
    }

    pub const fn fingerprint_v1(&self) -> [u8; 32] {
        self.fingerprint_v1
    }

    fn from_current_evaluation(
        basis: OwnerBodyInferenceBasis,
        interface_imports: Box<[OwnerBodyInterfaceImport]>,
        result: &OwnerBodyInferenceShard,
    ) -> Result<Self, OwnerBodyInferenceError> {
        if basis.owner != *result.owner() {
            return Err(OwnerBodyInferenceError::new(
                "body currentness basis and semantic result name different owners",
            ));
        }
        let result_fingerprint_v1 = result.fingerprint_v1();
        let fingerprint_v1 = fingerprint(
            OWNER_BODY_INFERENCE_CURRENTNESS_DOMAIN_V1,
            &(&basis, &interface_imports, result_fingerprint_v1),
        )?;
        Ok(Self {
            basis,
            interface_imports,
            result_fingerprint_v1,
            fingerprint_v1,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OwnerBodyInferenceEvaluation {
    pub currentness: OwnerBodyInferenceCurrentnessReceipt,
    pub result: Arc<OwnerBodyInferenceShard>,
}

fn fingerprint<T: Serialize>(
    domain: &[u8],
    value: &T,
) -> Result<[u8; 32], OwnerBodyInferenceError> {
    boon_contract::canonical_serde_hash_v1(domain, value).map_err(|error| {
        OwnerBodyInferenceError::new(format!("cannot fingerprint owner body inference: {error}"))
    })
}

pub(crate) fn owner_body_interface_fingerprint_v1(
    interface: &OwnerPublicInterface,
) -> Result<[u8; 32], OwnerBodyInferenceError> {
    fingerprint(OWNER_BODY_INTERFACE_DOMAIN_V1, interface)
}

fn checked_u32(value: usize, context: &str) -> Result<u32, OwnerBodyInferenceError> {
    u32::try_from(value).map_err(|_| {
        OwnerBodyInferenceError::new(format!("{context} exceeds the owner-local u32 bound"))
    })
}

fn checked_usize(value: u64, context: &str) -> Result<usize, OwnerBodyInferenceError> {
    usize::try_from(value).map_err(|_| {
        OwnerBodyInferenceError::new(format!("{context} exceeds the host usize bound"))
    })
}

fn materialized_span(
    source_map: &OwnerSourceMap,
    diagnostic: &OwnerDiagnosticTemplate,
) -> Result<(usize, usize, usize), OwnerBodyInferenceError> {
    if let Some(role) = diagnostic.role {
        let anchor = source_map.anchor(&diagnostic.site, role).ok_or_else(|| {
            OwnerBodyInferenceError::new(format!(
                "owner {:?} diagnostic {} has no exact source anchor",
                source_map.owner, diagnostic.code
            ))
        })?;
        return Ok((
            checked_usize(anchor.line, "diagnostic line")?,
            checked_usize(anchor.start, "diagnostic start")?,
            checked_usize(anchor.end, "diagnostic end")?,
        ));
    }
    match &diagnostic.site {
        OwnerSourceAnchorSite::Statement { statement } => {
            let source = source_map
                .statements
                .get(*statement as usize)
                .filter(|source| source.statement == *statement)
                .ok_or_else(|| {
                    OwnerBodyInferenceError::new(format!(
                        "owner {:?} diagnostic {} references missing statement {}",
                        source_map.owner, diagnostic.code, statement
                    ))
                })?;
            Ok((
                checked_usize(source.line, "diagnostic line")?,
                checked_usize(source.start, "diagnostic start")?,
                checked_usize(source.end, "diagnostic end")?,
            ))
        }
        OwnerSourceAnchorSite::Expression { expression } => {
            let source = source_map
                .expressions
                .iter()
                .find(|source| &source.expression == expression)
                .ok_or_else(|| {
                    OwnerBodyInferenceError::new(format!(
                        "owner {:?} diagnostic {} references missing expression",
                        source_map.owner, diagnostic.code
                    ))
                })?;
            Ok((
                checked_usize(source.line, "diagnostic line")?,
                checked_usize(source.start, "diagnostic start")?,
                checked_usize(source.end, "diagnostic end")?,
            ))
        }
    }
}

pub fn materialize_owner_diagnostics(
    shard: &OwnerBodyInferenceShard,
    source_map: &OwnerSourceMap,
) -> Result<Vec<TypeDiagnostic>, OwnerBodyInferenceError> {
    if shard.owner() != &source_map.owner {
        return Err(OwnerBodyInferenceError::new(
            "owner body inference and source map have different owners",
        ));
    }
    shard
        .diagnostics
        .iter()
        .map(|diagnostic| {
            let (line, start, end) = materialized_span(source_map, diagnostic)?;
            Ok(TypeDiagnostic {
                severity: diagnostic.severity,
                line,
                start,
                end,
                message: diagnostic.message.clone(),
            })
        })
        .collect()
}

#[derive(Clone)]
struct BodyCallPlan {
    expression: usize,
    stable_expression: StableExpressionKey,
    resolution: BodyCallableResolution,
    function: String,
    inputs: Box<[(OwnerConstraintEdgeRole, u32)]>,
}

#[derive(Clone)]
enum BodyCallableResolution {
    Owner(StableCheckOwnerKey),
    Authoritative,
    Unresolved,
    Ambiguous(Box<[StableCheckOwnerKey]>),
}

fn expression_variable(
    expressions: &[TypeVar],
    external_expressions: &[TypeVar],
    reference: u32,
) -> Option<TypeVar> {
    let reference = reference as usize;
    expressions.get(reference).copied().or_else(|| {
        external_expressions
            .get(reference.checked_sub(expressions.len())?)
            .copied()
    })
}

fn body_expression_boundary_variable(
    expressions: &[TypeVar],
    external_expressions: &[TypeVar],
    expression_flushes: &[TypeVar],
    external_expression_flushes: &[TypeVar],
    reference: u32,
    unifier: &mut TypeUnifier,
) -> Option<TypeVar> {
    let value = expression_variable(expressions, external_expressions, reference)?;
    let flush = expression_variable(expression_flushes, external_expression_flushes, reference)?;
    let boundary = unifier.fresh();
    unifier.bind_var(
        boundary,
        boon_checked::canonical_union_type(vec![Type::Var(value), Type::Var(flush)]),
    );
    Some(boundary)
}

fn inferred_expression_ref(
    syntax: &OwnerSyntaxInput,
    reference: u32,
) -> Result<OwnerInferenceExpressionRef, OwnerBodyInferenceError> {
    let index = reference as usize;
    if index < syntax.expressions.len() {
        return Ok(OwnerInferenceExpressionRef::Local {
            expression: OwnerInferenceExpressionId(reference),
        });
    }
    let external = syntax.external_expression(index).ok_or_else(|| {
        OwnerBodyInferenceError::new(format!(
            "owner body inference expression reference {reference} is out of bounds"
        ))
    })?;
    Ok(OwnerInferenceExpressionRef::External {
        owner: external.owner.clone(),
        expression: external.expression.clone(),
    })
}

fn direct_effect_for(kind: &OwnerConstraintNodeKind) -> CheckedEffectSummary {
    match kind {
        OwnerConstraintNodeKind::Source => CheckedEffectSummary {
            emits_source: true,
            ..CheckedEffectSummary::default()
        },
        OwnerConstraintNodeKind::Hold { .. } | OwnerConstraintNodeKind::Latest => {
            CheckedEffectSummary {
                reads_state: true,
                writes_state: true,
                ..CheckedEffectSummary::default()
            }
        }
        OwnerConstraintNodeKind::Call { function }
        | OwnerConstraintNodeKind::Pipe {
            operation: function,
        } if boon_effect_schema::host_effect_spec(function).is_some() => CheckedEffectSummary {
            invokes_host: true,
            ..CheckedEffectSummary::default()
        },
        _ => CheckedEffectSummary::default(),
    }
}

fn owner_flush_payload_type_is_closed(ty: &Type) -> bool {
    match ty {
        Type::Text | Type::Number | Type::Bytes(_) | Type::Bits { .. } => true,
        Type::VariantSet(variants) => {
            !variants.is_empty()
                && variants.iter().all(|variant| match variant {
                    Variant::Tag(_) => true,
                    Variant::Tagged { fields, .. } => {
                        !fields.open
                            && fields
                                .fields
                                .values()
                                .all(owner_flush_payload_type_is_closed)
                    }
                })
        }
        Type::Object(shape) => {
            !shape.open
                && shape
                    .fields
                    .values()
                    .all(owner_flush_payload_type_is_closed)
        }
        Type::Union(members) => {
            !members.is_empty() && members.iter().all(owner_flush_payload_type_is_closed)
        }
        Type::Unknown
        | Type::Var(_)
        | Type::UnresolvedShape { .. }
        | Type::Absent
        | Type::List(_)
        | Type::Map { .. }
        | Type::Set(_)
        | Type::Function { .. }
        | Type::RenderContract => false,
    }
}

fn owner_flush_payload_is_closed_tag_algebra(ty: &Type) -> bool {
    let Type::VariantSet(variants) = ty else {
        return false;
    };
    !variants.is_empty()
        && variants.iter().all(|variant| match variant {
            Variant::Tag(_) => true,
            Variant::Tagged { fields, .. } => {
                !fields.open
                    && fields
                        .fields
                        .values()
                        .all(owner_flush_payload_type_is_closed)
            }
        })
}

fn infer_owner_expression_flush_types(
    syntax: &OwnerSyntaxInput,
    flows: &[FlowType],
    flush_types: &[Option<Type>],
    diagnostics: &mut Vec<OwnerDiagnosticTemplate>,
) -> Result<Vec<Option<Type>>, OwnerBodyInferenceError> {
    if syntax.expressions.len() != flows.len() || syntax.expressions.len() > flush_types.len() {
        return Err(OwnerBodyInferenceError::new(
            "owner FLUSH propagation inputs do not match the expression table",
        ));
    }
    for input in &syntax.expressions {
        match &input.kind {
            AstExprKind::Flush {
                payload: Some(payload),
            } => {
                let payload_flow = flows.get(*payload).cloned().unwrap_or(FlowType {
                    mode: FlowMode::Continuous,
                    ty: Type::Unknown,
                });
                if payload_flow.mode != FlowMode::Continuous
                    || !owner_flush_payload_is_closed_tag_algebra(&payload_flow.ty)
                {
                    let site = syntax
                        .expressions
                        .get(*payload)
                        .map(|payload| payload.stable_key.clone())
                        .unwrap_or_else(|| input.stable_key.clone());
                    diagnostics.push(OwnerDiagnosticTemplate {
                        severity: DiagnosticSeverity::Error,
                        code: "invalid_flush_payload".to_owned(),
                        message: format!(
                            "`FLUSH` payload must be a continuous closed Tag, tagged object, or closed union without collection, flow, or host values; found {}",
                            crate::boon_facing_type_label(&payload_flow.ty)
                        ),
                        site: OwnerSourceAnchorSite::Expression { expression: site },
                        role: None,
                    });
                }
            }
            AstExprKind::Flush { payload: None } => diagnostics.push(OwnerDiagnosticTemplate {
                severity: DiagnosticSeverity::Error,
                code: "missing_flush_payload".to_owned(),
                message: "`FLUSH` requires exactly one payload expression".to_owned(),
                site: OwnerSourceAnchorSite::Expression {
                    expression: input.stable_key.clone(),
                },
                role: None,
            }),
            AstExprKind::Hold { initial, .. } => {
                let initial = input
                    .linked_input
                    .or_else(|| u32::try_from(*initial).ok())
                    .unwrap_or(u32::MAX);
                if flush_types
                    .get(initial as usize)
                    .is_some_and(Option::is_some)
                {
                    let expression = syntax
                        .expressions
                        .get(initial as usize)
                        .map(|expression| expression.stable_key.clone())
                        .or_else(|| {
                            syntax
                                .external_expression(initial as usize)
                                .map(|external| external.expression.clone())
                        })
                        .unwrap_or_else(|| input.stable_key.clone());
                    diagnostics.push(OwnerDiagnosticTemplate {
                        severity: DiagnosticSeverity::Error,
                        code: "hold_initializer_flush".to_owned(),
                        message: "a `HOLD` initializer must produce a valid storable value and cannot `FLUSH`".to_owned(),
                        site: OwnerSourceAnchorSite::Expression { expression },
                        role: None,
                    });
                }
            }
            _ => {}
        }
    }
    Ok(flush_types[..syntax.expressions.len()].to_vec())
}

fn push_invalid_syntax_diagnostics(
    seed: &OwnerConstraintSeed,
    diagnostics: &mut Vec<OwnerDiagnosticTemplate>,
) {
    for expression in &seed.expressions {
        let (code, message) = match &expression.kind {
            OwnerConstraintNodeKind::Unknown { tokens } => (
                "invalid_expression",
                format!("invalid expression `{}`", tokens.join(" ")),
            ),
            OwnerConstraintNodeKind::MatchArm {
                pattern: crate::OwnerPatternConstraint::Invalid,
            }
            | OwnerConstraintNodeKind::Arrow {
                pattern: crate::OwnerPatternConstraint::Invalid,
            } => ("invalid_pattern", "invalid match pattern".to_owned()),
            _ => continue,
        };
        diagnostics.push(OwnerDiagnosticTemplate {
            severity: DiagnosticSeverity::Error,
            code: code.to_owned(),
            message,
            site: OwnerSourceAnchorSite::Expression {
                expression: expression.expression.clone(),
            },
            role: None,
        });
    }
}

fn push_external_value_diagnostics(
    summary: &OwnerConstraintSummary,
    abi: &OwnerInferenceAbiEnvironment,
    diagnostics: &mut Vec<OwnerDiagnosticTemplate>,
) {
    for resolution in &summary.symbol_resolutions {
        let OwnerSymbolResolution::Authoritative { reference } = resolution else {
            continue;
        };
        if reference.kind != OwnerReferenceKind::Value {
            continue;
        }
        let canonical_path = boon_syntax::canonical_value_path(&reference.parts);
        let Some(lookup) = abi.value_lookup(&canonical_path) else {
            continue;
        };
        let (code, message) = match lookup.outcome() {
            OwnerValueAbiLookupOutcome::Found { .. }
            | OwnerValueAbiLookupOutcome::Missing {
                allow_unresolved: true,
            } => continue,
            OwnerValueAbiLookupOutcome::Missing {
                allow_unresolved: false,
            } => (
                "unknown_external_value",
                format!("unknown qualified external value `{canonical_path}`"),
            ),
            OwnerValueAbiLookupOutcome::Forbidden {
                reason: OwnerValueAbiForbiddenReason::NonStoreRoot { producer },
            } => (
                "invalid_external_value_root",
                format!(
                    "qualified external value `{canonical_path}` must use `{}/store.<value>`; role outputs are host boundaries, not distributed application state",
                    producer.namespace()
                ),
            ),
            OwnerValueAbiLookupOutcome::Forbidden {
                reason: OwnerValueAbiForbiddenReason::SameRole { role },
            } => (
                "same_role_external_value",
                format!(
                    "same-role qualification `{canonical_path}` is not allowed in {}; use an unqualified local name",
                    role.namespace()
                ),
            ),
            OwnerValueAbiLookupOutcome::Forbidden {
                reason: OwnerValueAbiForbiddenReason::DependencyDirection { consumer, producer },
            } => (
                "forbidden_external_value_dependency",
                format!(
                    "{} cannot depend on {} through `{canonical_path}`",
                    consumer.namespace(),
                    producer.namespace()
                ),
            ),
        };
        diagnostics.push(OwnerDiagnosticTemplate {
            severity: DiagnosticSeverity::Error,
            code: code.to_owned(),
            message,
            site: OwnerSourceAnchorSite::Expression {
                expression: reference.expression.clone(),
            },
            role: None,
        });
    }
}

fn collect_relocations(
    seed: &OwnerConstraintSeed,
    summary: &OwnerConstraintSummary,
) -> Box<[OwnerBodyRelocation]> {
    let mut relocations = BTreeSet::new();
    for resolved in &summary.resolved_references {
        relocations.insert(OwnerBodyRelocation {
            site: OwnerBodyRelocationSite::Expression {
                expression: resolved.reference.expression.clone(),
            },
            kind: match resolved.reference.kind {
                OwnerReferenceKind::Value => OwnerBodyRelocationKind::ValueRead,
                OwnerReferenceKind::Callable => OwnerBodyRelocationKind::Callable,
            },
            target_owner: resolved.owner.clone(),
            target_expression: None,
        });
    }
    let local_count = seed.expressions.len();
    let external = |reference: u32| {
        (reference as usize)
            .checked_sub(local_count)
            .and_then(|index| seed.external_expressions.get(index))
    };
    for (statement, expression) in &seed.statement_values {
        if let Some(target) = external(*expression) {
            relocations.insert(OwnerBodyRelocation {
                site: OwnerBodyRelocationSite::Statement {
                    statement: OwnerInferenceStatementId(*statement),
                },
                kind: OwnerBodyRelocationKind::ChildValue,
                target_owner: target.owner.clone(),
                target_expression: Some(target.expression.clone()),
            });
        }
    }
    for expression in &seed.expressions {
        for input in &expression.inputs {
            if let Some(target) = external(input.expression) {
                relocations.insert(OwnerBodyRelocation {
                    site: OwnerBodyRelocationSite::Expression {
                        expression: expression.expression.clone(),
                    },
                    kind: OwnerBodyRelocationKind::ChildValue,
                    target_owner: target.owner.clone(),
                    target_expression: Some(target.expression.clone()),
                });
            }
        }
    }
    relocations
        .into_iter()
        .collect::<Vec<_>>()
        .into_boxed_slice()
}

fn insert_interface<'a>(
    interfaces: &mut BTreeMap<StableCheckOwnerKey, &'a OwnerPublicInterface>,
    interface: &'a OwnerPublicInterface,
) -> Result<(), OwnerBodyInferenceError> {
    if let Some(previous) = interfaces.insert(interface.owner.clone(), interface)
        && previous != interface
    {
        return Err(OwnerBodyInferenceError::new(format!(
            "owner body received conflicting interfaces for {:?}",
            interface.owner
        )));
    }
    Ok(())
}

fn directly_required_interface_owners(
    seed: &OwnerConstraintSeed,
    summary: &OwnerConstraintSummary,
) -> BTreeSet<StableCheckOwnerKey> {
    std::iter::once(seed.owner.clone())
        .chain(
            seed.external_expressions
                .iter()
                .map(|external| external.owner.clone()),
        )
        .chain(
            summary
                .resolved_references
                .iter()
                .map(|resolved| resolved.owner.clone()),
        )
        .collect()
}

fn collect_result_expression_ref_owner(
    reference: &OwnerResultExpressionRef,
    dependencies: &mut BTreeSet<StableCheckOwnerKey>,
) {
    if let OwnerResultExpressionRef::Child { owner, .. } = reference {
        dependencies.insert(owner.clone());
    }
}

fn owner_result_transfer_dependencies(
    transfer: &OwnerResultTransfer,
) -> BTreeSet<StableCheckOwnerKey> {
    let OwnerResultTransfer::Expression { root, nodes } = transfer else {
        return BTreeSet::new();
    };
    let mut dependencies = BTreeSet::new();
    collect_result_expression_ref_owner(root, &mut dependencies);
    for node in nodes {
        if let Some(OwnerResultCallTarget::Owner { owner }) = &node.call_target {
            dependencies.insert(owner.clone());
        }
        for input in &node.inputs {
            collect_result_expression_ref_owner(&input.expression, &mut dependencies);
        }
    }
    dependencies
}

/// Exact frozen-interface closure needed to infer one owner body.
///
/// Direct syntax imports are only the first frontier. A callable's public
/// result-transfer slice may itself invoke another callable, so callers also
/// freeze the transitive interfaces reachable through those minimal slices.
/// Unrelated calls in a callee body are absent from the transfer and therefore
/// cannot enlarge this dependency set or backdate the caller.
pub fn owner_body_required_interface_owners(
    seed: &OwnerConstraintSeed,
    summary: &OwnerConstraintSummary,
    interfaces: &BTreeMap<StableCheckOwnerKey, &OwnerPublicInterface>,
) -> Result<BTreeSet<StableCheckOwnerKey>, OwnerBodyInferenceError> {
    let mut required = directly_required_interface_owners(seed, summary);
    let mut pending = required.iter().cloned().collect::<VecDeque<_>>();
    while let Some(owner) = pending.pop_front() {
        let interface = interfaces.get(&owner).ok_or_else(|| {
            OwnerBodyInferenceError::new(format!(
                "owner body inference {:?} is missing required interface {owner:?}",
                seed.owner
            ))
        })?;
        for dependency in owner_result_transfer_dependencies(&interface.result_transfer) {
            if required.insert(dependency.clone()) {
                pending.push_back(dependency);
            }
        }
    }
    Ok(required)
}

fn frozen_scc_ref(
    result: &OwnerInterfaceSccResult,
    required: &BTreeSet<StableCheckOwnerKey>,
) -> Result<FrozenOwnerInterfaceSccRef, OwnerBodyInferenceError> {
    let referenced_owners = result
        .key
        .members
        .iter()
        .filter(|owner| required.contains(*owner))
        .cloned()
        .collect::<Vec<_>>();
    if referenced_owners.is_empty() {
        return Err(OwnerBodyInferenceError::new(format!(
            "owner body inference received unused interface SCC {:?}",
            result.key
        )));
    }
    Ok(FrozenOwnerInterfaceSccRef {
        key: result.key.clone(),
        result_fingerprint_v1: result.fingerprint_v1(),
        type_variable_count: result.type_variable_count,
        referenced_owners: referenced_owners.into_boxed_slice(),
    })
}

fn bind_local_constraints(
    seed: &OwnerConstraintSeed,
    summary: &OwnerConstraintSummary,
    abi: &OwnerInferenceAbiEnvironment,
    unifier: &mut TypeUnifier,
    expressions: &[TypeVar],
    external_expressions: &[TypeVar],
    local_roots: &BTreeMap<String, Option<TypeVar>>,
    context: Option<TypeVar>,
    modes: &mut [Option<FlowMode>],
    direct_effects: &mut [CheckedEffectSummary],
    calls: &mut Vec<BodyCallPlan>,
    work: &mut OwnerBodyInferenceWork,
) {
    let resolved = summary
        .resolved_references
        .iter()
        .map(|resolved| (resolved.reference.expression.clone(), resolved))
        .collect::<BTreeMap<_, _>>();
    let symbol_resolutions = summary
        .symbol_resolutions
        .iter()
        .map(|resolution| (resolution.reference().expression.clone(), resolution))
        .collect::<BTreeMap<_, _>>();
    for (index, expression) in seed.expressions.iter().enumerate() {
        let variable = expressions[index];
        let mut mode = Some(FlowMode::Continuous);
        match &expression.kind {
            OwnerConstraintNodeKind::Text | OwnerConstraintNodeKind::TextTemplate => {
                unifier.bind_var(variable, Type::Text);
            }
            OwnerConstraintNodeKind::Number => unifier.bind_var(variable, Type::Number),
            OwnerConstraintNodeKind::Byte => {
                unifier.bind_var(variable, Type::Bytes(BytesType::Fixed(1)));
            }
            OwnerConstraintNodeKind::Bits { width } => {
                unifier.bind_var(variable, Type::Bits { width: *width });
            }
            OwnerConstraintNodeKind::Tag { name } if name == "SKIP" => {
                unifier.bind_var(variable, Type::Absent);
                mode = Some(FlowMode::Absent);
            }
            OwnerConstraintNodeKind::Tag { name } => unifier.bind_var(
                variable,
                Type::VariantSet(vec![Variant::Tag(name.clone())].into()),
            ),
            OwnerConstraintNodeKind::Source => {
                if let Some(query) = seed
                    .source_payload_queries
                    .iter()
                    .find(|query| query.expression == expression.expression)
                    && let Some(payload_type) = abi
                        .source_payload_lookup(&query.canonical_path)
                        .and_then(crate::OwnerSourcePayloadAbiLookup::payload_type)
                {
                    let mut variables = BTreeMap::new();
                    let payload_type = instantiate_type(payload_type, unifier, &mut variables);
                    unifier.bind_var(variable, payload_type);
                }
                mode = Some(FlowMode::PresentOrAbsent);
                direct_effects[index].emits_source = true;
            }
            OwnerConstraintNodeKind::Reference { parts }
            | OwnerConstraintNodeKind::Drain { parts } => {
                if resolved.contains_key(&expression.expression) {
                    // Cross-owner value reads are wired after all interfaces
                    // have been instantiated into this body namespace.
                } else if matches!(
                    symbol_resolutions.get(&expression.expression),
                    Some(OwnerSymbolResolution::Authoritative { reference })
                        if reference.kind == OwnerReferenceKind::Value
                ) {
                    let canonical_path = boon_syntax::canonical_value_path(parts);
                    if let Some(flow_type) = abi
                        .value_lookup(&canonical_path)
                        .and_then(crate::OwnerValueAbiLookup::flow_type)
                    {
                        let mut variables = BTreeMap::new();
                        let ty = instantiate_type(&flow_type.ty, unifier, &mut variables);
                        unifier.bind_var(variable, ty);
                        mode = Some(flow_type.mode);
                    }
                } else if let Some((root, projection)) = parts.split_first() {
                    let local = if root == "PASSED" {
                        context
                    } else {
                        local_roots.get(root).copied().flatten()
                    };
                    if let Some(local) = local {
                        let projected = bind_projection(unifier, local, projection);
                        unifier.unify(Type::Var(variable), Type::Var(projected));
                    }
                }
            }
            OwnerConstraintNodeKind::Record { tag } => {
                let fields = expression.inputs.iter().filter_map(|input| {
                    let OwnerConstraintEdgeRole::RecordField {
                        name,
                        spread: false,
                    } = &input.role
                    else {
                        return None;
                    };
                    Some((
                        name.clone(),
                        Type::Var(expression_variable(
                            expressions,
                            external_expressions,
                            input.expression,
                        )?),
                    ))
                });
                let shape: ObjectShape = ObjectShape::from_ordered_fields(fields, false);
                let ty = match tag {
                    Some(tag) => Type::VariantSet(
                        vec![Variant::Tagged {
                            tag: tag.clone(),
                            fields: shape.into(),
                        }]
                        .into(),
                    ),
                    None => Type::object(shape),
                };
                unifier.bind_var(variable, ty);
            }
            OwnerConstraintNodeKind::Flush => unifier.bind_var(variable, Type::Absent),
            OwnerConstraintNodeKind::Call { function }
            | OwnerConstraintNodeKind::Pipe {
                operation: function,
            } => {
                let resolution = match symbol_resolutions.get(&expression.expression).copied() {
                    Some(OwnerSymbolResolution::Resolved { owner, .. }) => {
                        BodyCallableResolution::Owner(owner.clone())
                    }
                    Some(OwnerSymbolResolution::Authoritative { .. }) => {
                        BodyCallableResolution::Authoritative
                    }
                    Some(OwnerSymbolResolution::Ambiguous { candidates, .. }) => {
                        BodyCallableResolution::Ambiguous(
                            candidates
                                .iter()
                                .map(|candidate| candidate.owner.clone())
                                .collect::<Vec<_>>()
                                .into_boxed_slice(),
                        )
                    }
                    Some(OwnerSymbolResolution::Unresolved { .. }) | None => {
                        BodyCallableResolution::Unresolved
                    }
                };
                calls.push(BodyCallPlan {
                    expression: index,
                    stable_expression: expression.expression.clone(),
                    resolution,
                    function: function.clone(),
                    inputs: expression
                        .inputs
                        .iter()
                        .map(|input| (input.role.clone(), input.expression))
                        .collect(),
                });
                mode = None;
            }
            OwnerConstraintNodeKind::Draining => {
                if let Some(input) = expression.inputs.first().and_then(|input| {
                    expression_variable(expressions, external_expressions, input.expression)
                }) {
                    unifier.unify(Type::Var(variable), Type::Var(input));
                }
                mode = None;
            }
            OwnerConstraintNodeKind::Hold { .. } => {
                if let Some(input) = expression.inputs.first().and_then(|input| {
                    expression_variable(expressions, external_expressions, input.expression)
                }) {
                    unifier.unify(Type::Var(variable), Type::Var(input));
                }
                direct_effects[index].reads_state = true;
                direct_effects[index].writes_state = true;
            }
            OwnerConstraintNodeKind::Latest => {
                for input in &expression.inputs {
                    if let Some(input) =
                        expression_variable(expressions, external_expressions, input.expression)
                    {
                        unifier.unify(Type::Var(variable), Type::Var(input));
                    }
                }
                mode = None;
            }
            OwnerConstraintNodeKind::When => {
                for input in expression
                    .inputs
                    .iter()
                    .filter(|input| matches!(input.role, OwnerConstraintEdgeRole::WhenArm))
                {
                    if let Some(input) =
                        expression_variable(expressions, external_expressions, input.expression)
                    {
                        unifier.unify(Type::Var(variable), Type::Var(input));
                    }
                }
                mode = None;
            }
            OwnerConstraintNodeKind::Then => {
                let output = expression
                    .inputs
                    .iter()
                    .find(|input| matches!(input.role, OwnerConstraintEdgeRole::ThenOutput));
                let input = output.or_else(|| {
                    expression
                        .inputs
                        .iter()
                        .find(|input| matches!(input.role, OwnerConstraintEdgeRole::ThenInput))
                });
                if let Some(input) = input.and_then(|input| {
                    expression_variable(expressions, external_expressions, input.expression)
                }) {
                    unifier.unify(Type::Var(variable), Type::Var(input));
                }
                mode = Some(FlowMode::PresentOrAbsent);
            }
            OwnerConstraintNodeKind::Infix { operation } => {
                for input in &expression.inputs {
                    if let Some(input) =
                        expression_variable(expressions, external_expressions, input.expression)
                    {
                        unifier.bind_var(input, Type::Number);
                    }
                }
                unifier.bind_var(
                    variable,
                    if infix_returns_bool(operation) {
                        true_false_type()
                    } else {
                        Type::Number
                    },
                );
            }
            OwnerConstraintNodeKind::MatchArm { pattern } => {
                if let Some(output) = expression
                    .inputs
                    .iter()
                    .find(|input| matches!(input.role, OwnerConstraintEdgeRole::MatchOutput))
                {
                    if let Some(output) =
                        expression_variable(expressions, external_expressions, output.expression)
                    {
                        unifier.unify(Type::Var(variable), Type::Var(output));
                    }
                    mode = None;
                } else {
                    unifier.bind_var(variable, Type::Absent);
                    mode = Some(FlowMode::Absent);
                }
                let _ = pattern_type(pattern, unifier);
            }
            OwnerConstraintNodeKind::Block => {
                if let Some(result) = expression
                    .inputs
                    .iter()
                    .find(|input| matches!(input.role, OwnerConstraintEdgeRole::BlockResult))
                {
                    if let Some(result) =
                        expression_variable(expressions, external_expressions, result.expression)
                    {
                        unifier.unify(Type::Var(variable), Type::Var(result));
                    }
                    mode = None;
                } else {
                    unifier.bind_var(variable, Type::Absent);
                    mode = Some(FlowMode::Absent);
                }
            }
            OwnerConstraintNodeKind::Collection {
                collection,
                fixed_size_or_capacity,
            } => match collection {
                OwnerCollectionKind::List => {
                    let item = unifier.fresh();
                    for input in &expression.inputs {
                        if let Some(input) =
                            expression_variable(expressions, external_expressions, input.expression)
                        {
                            unifier.unify(Type::Var(item), Type::Var(input));
                        }
                    }
                    unifier.bind_var(variable, Type::List(Type::shared(Type::Var(item))));
                }
                OwnerCollectionKind::Set => {
                    let item = unifier.fresh();
                    for input in &expression.inputs {
                        if let Some(input) =
                            expression_variable(expressions, external_expressions, input.expression)
                        {
                            unifier.unify(Type::Var(item), Type::Var(input));
                        }
                    }
                    unifier.bind_var(variable, Type::Set(Type::shared(Type::Var(item))));
                }
                OwnerCollectionKind::Bytes => {
                    let size = fixed_size_or_capacity
                        .map(|size| BytesType::Fixed(size as usize))
                        .unwrap_or(BytesType::Dynamic);
                    unifier.bind_var(variable, Type::Bytes(size));
                }
                OwnerCollectionKind::Map => {
                    let key = unifier.fresh();
                    let value = unifier.fresh();
                    for input in &expression.inputs {
                        let Some(input) = expression_variable(
                            expressions,
                            external_expressions,
                            input.expression,
                        ) else {
                            continue;
                        };
                        unifier.bind_var(
                            input,
                            Type::object(ObjectShape::from_ordered_fields(
                                [
                                    ("key".to_owned(), Type::Var(key)),
                                    ("value".to_owned(), Type::Var(value)),
                                ],
                                false,
                            )),
                        );
                    }
                    unifier.bind_var(
                        variable,
                        Type::Map {
                            key: Box::new(Type::Var(key)),
                            value: Box::new(Type::Var(value)),
                        },
                    );
                }
            },
            OwnerConstraintNodeKind::Arrow { pattern } => {
                if let Some(output) = expression
                    .inputs
                    .iter()
                    .find(|input| matches!(input.role, OwnerConstraintEdgeRole::ArrowOutput))
                    .and_then(|output| {
                        expression_variable(expressions, external_expressions, output.expression)
                    })
                {
                    unifier.unify(Type::Var(variable), Type::Var(output));
                }
                let _ = pattern_type(pattern, unifier);
            }
            OwnerConstraintNodeKind::MapEntry => {
                let key = expression.inputs.iter().find_map(|input| {
                    matches!(input.role, OwnerConstraintEdgeRole::MapKey)
                        .then(|| {
                            expression_variable(expressions, external_expressions, input.expression)
                        })
                        .flatten()
                });
                let value = expression.inputs.iter().find_map(|input| {
                    matches!(input.role, OwnerConstraintEdgeRole::MapValue)
                        .then(|| {
                            expression_variable(expressions, external_expressions, input.expression)
                        })
                        .flatten()
                });
                if let (Some(key), Some(value)) = (key, value) {
                    unifier.bind_var(
                        variable,
                        Type::object(ObjectShape::from_ordered_fields(
                            [
                                ("key".to_owned(), Type::Var(key)),
                                ("value".to_owned(), Type::Var(value)),
                            ],
                            false,
                        )),
                    );
                }
            }
            OwnerConstraintNodeKind::Delimiter | OwnerConstraintNodeKind::Unknown { .. } => {}
        }
        modes[index] = flow_mode_join(modes[index], mode);
        direct_effects[index] =
            merge_effects(direct_effects[index], direct_effect_for(&expression.kind));
        work.local_constraints = work.local_constraints.saturating_add(1);
    }
}

#[derive(Clone)]
struct InstantiatedCallParameter {
    name: String,
    kind: OwnerParameterKind,
    ordinal: u32,
    flow_type: FlowType,
    requirement: CheckedParameterRequirement,
    evaluation_scope: OwnerInterfaceEvaluationScope,
}

#[derive(Clone)]
struct InstantiatedCallSignature {
    kind: CheckedCallableKind,
    parameters: Vec<InstantiatedCallParameter>,
    result: FlowType,
    result_flush_type: Option<Type>,
    context: Option<Type>,
    effect: CheckedEffectSummary,
    target: InferredOwnerCallableTarget,
}

#[derive(Clone)]
struct InferredCallDraft {
    plan: BodyCallPlan,
    target: InferredOwnerCallableTarget,
    effect: CheckedEffectSummary,
    actual_inputs: BTreeMap<u32, Type>,
    signature_result: FlowType,
    resolved_result: Option<FlowType>,
    type_substitutions: Vec<(TypeVar, Type)>,
    contextual_type_variables: Vec<TypeVar>,
    syntax_discriminated_result: bool,
    valid: bool,
}

#[derive(Clone)]
struct EvaluatedResultValue {
    flow_type: FlowType,
    parameter_derived: bool,
    syntax_selected: bool,
    static_number: Option<ExactNumber>,
}

struct EvaluatedOwnerResult {
    value: EvaluatedResultValue,
    type_substitutions: Vec<(TypeVar, Type)>,
    contextual_type_variables: Vec<TypeVar>,
}

struct OwnerResultTransferEvaluator<'a, 'unifier> {
    interfaces: &'a BTreeMap<StableCheckOwnerKey, &'a OwnerPublicInterface>,
    unifier: &'unifier mut TypeUnifier,
    active_owners: BTreeSet<StableCheckOwnerKey>,
}

impl<'a, 'unifier> OwnerResultTransferEvaluator<'a, 'unifier> {
    fn new(
        interfaces: &'a BTreeMap<StableCheckOwnerKey, &'a OwnerPublicInterface>,
        unifier: &'unifier mut TypeUnifier,
    ) -> Self {
        Self {
            interfaces,
            unifier,
            active_owners: BTreeSet::new(),
        }
    }

    fn evaluate_owner(
        &mut self,
        owner: &StableCheckOwnerKey,
        arguments: &BTreeMap<u32, EvaluatedResultValue>,
        context: Option<&EvaluatedResultValue>,
    ) -> Option<EvaluatedOwnerResult> {
        let interface = *self.interfaces.get(owner)?;
        // Interface type variables live in an SCC-local alpha namespace. Give
        // every invocation its own variables in the caller's unifier so raw
        // TypeVar ordinals from unrelated SCCs can never alias each other or
        // the caller's local variables.
        let mut variables = BTreeMap::new();
        for variable in &interface.type_variables {
            variables.insert(*variable, self.unifier.fresh());
        }
        let mut substitutions = BTreeMap::new();
        for parameter in &interface.parameters {
            if let Some(actual) = arguments.get(&parameter.ordinal) {
                let formal =
                    instantiate_type(&parameter.flow_type.ty, self.unifier, &mut variables);
                crate::unify_checked_type_pattern(
                    &formal,
                    &actual.flow_type.ty,
                    &mut substitutions,
                );
            }
        }
        if let (Some(formal), Some(actual)) = (&interface.context, context) {
            let formal = instantiate_type(&formal.flow_type.ty, self.unifier, &mut variables);
            crate::unify_checked_type_pattern(&formal, &actual.flow_type.ty, &mut substitutions);
        }
        let instantiated_result =
            instantiate_type(&interface.result.ty, self.unifier, &mut variables);
        let principal = FlowType {
            mode: interface.result.mode,
            ty: apply_checked_type_substitution_lookup(&instantiated_result, &substitutions),
        };
        let result_flush_type = interface.result_flush_type.as_ref().map(|flush_type| {
            let flush_type = instantiate_type(flush_type, self.unifier, &mut variables);
            apply_checked_type_substitution_lookup(&flush_type, &substitutions)
        });
        let fallbacks = match &interface.result_transfer {
            OwnerResultTransfer::Principal | OwnerResultTransfer::Parameter { .. } => {
                BTreeMap::new()
            }
            OwnerResultTransfer::Expression { nodes, .. } => nodes
                .iter()
                .map(|node| {
                    let ty = instantiate_type(&node.flow_type.ty, self.unifier, &mut variables);
                    (
                        node.expression.clone(),
                        FlowType {
                            mode: node.flow_type.mode,
                            ty: apply_checked_type_substitution_lookup(&ty, &substitutions),
                        },
                    )
                })
                .collect(),
        };
        let mut contextual_type_variables = BTreeSet::new();
        if let Some(context) = &interface.context {
            crate::collect_type_vars(&context.flow_type.ty, &mut contextual_type_variables);
        }
        let type_substitutions = interface
            .type_variables
            .iter()
            .filter_map(|variable| {
                let instantiated = variables.get(variable)?;
                substitutions.get(instantiated).map(|value| {
                    (
                        *variable,
                        apply_checked_type_substitution_lookup(value, &substitutions),
                    )
                })
            })
            .collect::<Vec<_>>();

        if !self.active_owners.insert(owner.clone()) {
            return Some(EvaluatedOwnerResult {
                value: EvaluatedResultValue {
                    flow_type: principal,
                    parameter_derived: arguments.values().any(|value| value.parameter_derived),
                    syntax_selected: false,
                    static_number: None,
                },
                type_substitutions,
                contextual_type_variables: contextual_type_variables.into_iter().collect(),
            });
        }
        let evaluated = match &interface.result_transfer {
            OwnerResultTransfer::Principal => None,
            OwnerResultTransfer::Parameter { read } => {
                arguments.get(&read.parameter_ordinal).and_then(|actual| {
                    let ty = if read.projection.is_empty() {
                        Some(actual.flow_type.ty.clone())
                    } else {
                        crate::type_for_nested_path(&actual.flow_type.ty, &read.projection)
                    }?;
                    Some(EvaluatedResultValue {
                        flow_type: FlowType {
                            mode: actual.flow_type.mode,
                            ty,
                        },
                        parameter_derived: true,
                        syntax_selected: actual.syntax_selected,
                        static_number: read
                            .projection
                            .is_empty()
                            .then(|| actual.static_number.clone())
                            .flatten(),
                    })
                })
            }
            OwnerResultTransfer::Expression { root, nodes } => self.evaluate_expression_ref(
                root,
                nodes,
                arguments,
                context,
                &fallbacks,
                &BTreeMap::new(),
                &mut BTreeSet::new(),
            ),
        };
        self.active_owners.remove(owner);

        let mut value = if let Some(mut evaluated) = evaluated {
            let selected = evaluated.syntax_selected
                && crate::type_has_concrete_outer_shape(&evaluated.flow_type.ty);
            evaluated.flow_type.ty = if selected {
                evaluated.flow_type.ty
            } else {
                specialize_checked_call_result(&principal.ty, &evaluated.flow_type.ty)
            };
            evaluated.syntax_selected = selected;
            evaluated
        } else {
            EvaluatedResultValue {
                flow_type: principal,
                parameter_derived: arguments.values().any(|value| value.parameter_derived),
                syntax_selected: false,
                static_number: None,
            }
        };
        if let Some(flush_type) = result_flush_type {
            value.flow_type.ty =
                boon_checked::canonical_union_type(vec![value.flow_type.ty, flush_type]);
            if value.flow_type.mode == FlowMode::Absent {
                value.flow_type.mode = FlowMode::Continuous;
            }
        }
        Some(EvaluatedOwnerResult {
            value,
            type_substitutions,
            contextual_type_variables: contextual_type_variables.into_iter().collect(),
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn evaluate_expression_ref(
        &mut self,
        reference: &OwnerResultExpressionRef,
        nodes: &[OwnerResultTransferNode],
        arguments: &BTreeMap<u32, EvaluatedResultValue>,
        context: Option<&EvaluatedResultValue>,
        fallbacks: &BTreeMap<StableExpressionKey, FlowType>,
        lexical: &BTreeMap<String, EvaluatedResultValue>,
        active: &mut BTreeSet<StableExpressionKey>,
    ) -> Option<EvaluatedResultValue> {
        match reference {
            OwnerResultExpressionRef::Child { owner, .. } => {
                let interface = *self.interfaces.get(owner)?;
                Some(EvaluatedResultValue {
                    flow_type: interface.result.clone(),
                    parameter_derived: false,
                    syntax_selected: false,
                    static_number: None,
                })
            }
            OwnerResultExpressionRef::Local { expression } => {
                let index = nodes
                    .binary_search_by(|node| node.expression.cmp(expression))
                    .ok()?;
                let node = &nodes[index];
                if !active.insert(expression.clone()) {
                    return transfer_fallback(node, fallbacks);
                }
                let value =
                    self.evaluate_node(node, nodes, arguments, context, fallbacks, lexical, active);
                active.remove(expression);
                value
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn evaluate_node(
        &mut self,
        node: &OwnerResultTransferNode,
        nodes: &[OwnerResultTransferNode],
        arguments: &BTreeMap<u32, EvaluatedResultValue>,
        context: Option<&EvaluatedResultValue>,
        fallbacks: &BTreeMap<StableExpressionKey, FlowType>,
        lexical: &BTreeMap<String, EvaluatedResultValue>,
        active: &mut BTreeSet<StableExpressionKey>,
    ) -> Option<EvaluatedResultValue> {
        let evaluate = |evaluator: &mut Self,
                        reference: &OwnerResultExpressionRef,
                        lexical: &BTreeMap<String, EvaluatedResultValue>,
                        active: &mut BTreeSet<StableExpressionKey>| {
            evaluator.evaluate_expression_ref(
                reference, nodes, arguments, context, fallbacks, lexical, active,
            )
        };
        let fallback = transfer_fallback(node, fallbacks)?;
        if let Some(read) = &node.parameter_read
            && let Some(actual) = arguments.get(&read.parameter_ordinal)
        {
            let ty = if read.projection.is_empty() {
                Some(actual.flow_type.ty.clone())
            } else {
                crate::type_for_nested_path(&actual.flow_type.ty, &read.projection)
            };
            if let Some(ty) = ty {
                return Some(EvaluatedResultValue {
                    flow_type: FlowType {
                        mode: actual.flow_type.mode,
                        ty,
                    },
                    parameter_derived: true,
                    syntax_selected: actual.syntax_selected,
                    static_number: read
                        .projection
                        .is_empty()
                        .then(|| actual.static_number.clone())
                        .flatten(),
                });
            }
        }
        if let OwnerConstraintNodeKind::Reference { parts }
        | OwnerConstraintNodeKind::Drain { parts } = &node.kind
            && let Some((name, projection)) = parts.split_first()
            && let Some(value) = lexical.get(name)
        {
            let ty = if projection.is_empty() {
                Some(value.flow_type.ty.clone())
            } else {
                crate::type_for_nested_path(&value.flow_type.ty, projection)
            };
            if let Some(ty) = ty {
                return Some(EvaluatedResultValue {
                    flow_type: FlowType {
                        mode: value.flow_type.mode,
                        ty,
                    },
                    parameter_derived: value.parameter_derived,
                    syntax_selected: value.syntax_selected,
                    static_number: projection
                        .is_empty()
                        .then(|| value.static_number.clone())
                        .flatten(),
                });
            }
        }

        match &node.kind {
            OwnerConstraintNodeKind::Call { .. } | OwnerConstraintNodeKind::Pipe { .. } => {
                self.evaluate_call_node(node, nodes, arguments, context, fallbacks, lexical, active)
            }
            OwnerConstraintNodeKind::Infix { operation } => {
                let left = node
                    .inputs
                    .iter()
                    .find(|input| matches!(input.role, OwnerConstraintEdgeRole::InfixLeft))
                    .and_then(|input| evaluate(self, &input.expression, lexical, active))?;
                let right = node
                    .inputs
                    .iter()
                    .find(|input| matches!(input.role, OwnerConstraintEdgeRole::InfixRight))
                    .and_then(|input| evaluate(self, &input.expression, lexical, active))?;
                let static_number = left
                    .static_number
                    .as_ref()
                    .zip(right.static_number.as_ref())
                    .and_then(|(left, right)| static_number_infix(left, operation, right));
                Some(EvaluatedResultValue {
                    static_number,
                    parameter_derived: left.parameter_derived || right.parameter_derived,
                    syntax_selected: left.syntax_selected || right.syntax_selected,
                    ..fallback
                })
            }
            OwnerConstraintNodeKind::Record { tag } => {
                let mut fields = Vec::new();
                let mut parameter_derived = false;
                let mut syntax_selected = false;
                for input in &node.inputs {
                    let OwnerConstraintEdgeRole::RecordField {
                        name,
                        spread: false,
                    } = &input.role
                    else {
                        continue;
                    };
                    let value = evaluate(self, &input.expression, lexical, active)?;
                    parameter_derived |= value.parameter_derived;
                    syntax_selected |= value.syntax_selected;
                    fields.push((name.clone(), value.flow_type.ty));
                }
                let shape: ObjectShape = ObjectShape::from_ordered_fields(fields, false);
                Some(EvaluatedResultValue {
                    flow_type: FlowType {
                        mode: FlowMode::Continuous,
                        ty: tag.as_ref().map_or_else(
                            || Type::object(shape.clone()),
                            |tag| {
                                Type::VariantSet(
                                    vec![Variant::Tagged {
                                        tag: tag.clone(),
                                        fields: shape.clone().into(),
                                    }]
                                    .into(),
                                )
                            },
                        ),
                    },
                    parameter_derived,
                    syntax_selected,
                    static_number: None,
                })
            }
            OwnerConstraintNodeKind::When => {
                self.evaluate_when_node(node, nodes, arguments, context, fallbacks, lexical, active)
            }
            OwnerConstraintNodeKind::MatchArm { .. } | OwnerConstraintNodeKind::Arrow { .. } => {
                let output = node.inputs.iter().find(|input| {
                    matches!(
                        input.role,
                        OwnerConstraintEdgeRole::MatchOutput | OwnerConstraintEdgeRole::ArrowOutput
                    )
                });
                output.map_or_else(
                    || Some(fallback),
                    |output| evaluate(self, &output.expression, lexical, active),
                )
            }
            OwnerConstraintNodeKind::Block => {
                let mut lexical = lexical.clone();
                let mut parameter_derived = false;
                let mut syntax_selected = false;
                for input in &node.inputs {
                    if let OwnerConstraintEdgeRole::BlockBinding { name } = &input.role {
                        let value = evaluate(self, &input.expression, &lexical, active)?;
                        parameter_derived |= value.parameter_derived;
                        syntax_selected |= value.syntax_selected;
                        lexical.insert(name.clone(), value);
                    }
                }
                if let Some(result) = node
                    .inputs
                    .iter()
                    .find(|input| matches!(input.role, OwnerConstraintEdgeRole::BlockResult))
                {
                    let mut value = evaluate(self, &result.expression, &lexical, active)?;
                    value.parameter_derived |= parameter_derived;
                    value.syntax_selected |= syntax_selected;
                    Some(value)
                } else {
                    Some(fallback)
                }
            }
            OwnerConstraintNodeKind::Collection { collection, .. } => {
                let values = node
                    .inputs
                    .iter()
                    .filter(|input| {
                        matches!(
                            input.role,
                            OwnerConstraintEdgeRole::CollectionItem
                                | OwnerConstraintEdgeRole::MapEntry
                        )
                    })
                    .map(|input| evaluate(self, &input.expression, lexical, active))
                    .collect::<Option<Vec<_>>>()?;
                let parameter_derived = values.iter().any(|value| value.parameter_derived);
                let syntax_selected = values.iter().any(|value| value.syntax_selected);
                let ty = match collection {
                    OwnerCollectionKind::List => Type::List(Type::shared(
                        values
                            .iter()
                            .map(|value| value.flow_type.ty.clone())
                            .reduce(|left, right| widen_structural_type(&left, &right))
                            .unwrap_or(Type::Unknown),
                    )),
                    OwnerCollectionKind::Set => Type::Set(Type::shared(
                        values
                            .iter()
                            .map(|value| value.flow_type.ty.clone())
                            .reduce(|left, right| widen_structural_type(&left, &right))
                            .unwrap_or(Type::Unknown),
                    )),
                    OwnerCollectionKind::Bytes | OwnerCollectionKind::Map => fallback.flow_type.ty,
                };
                Some(EvaluatedResultValue {
                    flow_type: FlowType {
                        mode: FlowMode::Continuous,
                        ty,
                    },
                    parameter_derived,
                    syntax_selected,
                    static_number: None,
                })
            }
            OwnerConstraintNodeKind::Latest => {
                let values = node
                    .inputs
                    .iter()
                    .filter(|input| matches!(input.role, OwnerConstraintEdgeRole::LatestBranch))
                    .map(|input| evaluate(self, &input.expression, lexical, active))
                    .collect::<Option<Vec<_>>>()?;
                let ty = values
                    .iter()
                    .filter(|value| !matches!(value.flow_type.ty, Type::Absent))
                    .map(|value| value.flow_type.ty.clone())
                    .reduce(|left, right| widen_structural_type(&left, &right))
                    .unwrap_or_else(|| fallback.flow_type.ty.clone());
                Some(EvaluatedResultValue {
                    flow_type: FlowType {
                        mode: crate::latest_flow_mode(
                            values.iter().map(|value| value.flow_type.mode),
                        )
                        .unwrap_or(FlowMode::Continuous),
                        ty,
                    },
                    parameter_derived: values.iter().any(|value| value.parameter_derived),
                    syntax_selected: values.iter().any(|value| value.syntax_selected),
                    static_number: None,
                })
            }
            OwnerConstraintNodeKind::Draining | OwnerConstraintNodeKind::Hold { .. } => node
                .inputs
                .first()
                .and_then(|input| evaluate(self, &input.expression, lexical, active))
                .map(|mut value| {
                    if matches!(node.kind, OwnerConstraintNodeKind::Hold { .. }) {
                        value.flow_type.mode = FlowMode::Continuous;
                    }
                    value
                })
                .or(Some(fallback)),
            OwnerConstraintNodeKind::Then => {
                let value = node
                    .inputs
                    .iter()
                    .find(|input| matches!(input.role, OwnerConstraintEdgeRole::ThenOutput))
                    .or_else(|| {
                        node.inputs
                            .iter()
                            .find(|input| matches!(input.role, OwnerConstraintEdgeRole::ThenInput))
                    })
                    .and_then(|input| evaluate(self, &input.expression, lexical, active));
                value.map(|mut value| {
                    value.flow_type.mode = FlowMode::PresentOrAbsent;
                    value
                })
            }
            OwnerConstraintNodeKind::MapEntry => {
                let key = node
                    .inputs
                    .iter()
                    .find(|input| matches!(input.role, OwnerConstraintEdgeRole::MapKey))
                    .and_then(|input| evaluate(self, &input.expression, lexical, active))?;
                let value = node
                    .inputs
                    .iter()
                    .find(|input| matches!(input.role, OwnerConstraintEdgeRole::MapValue))
                    .and_then(|input| evaluate(self, &input.expression, lexical, active))?;
                Some(EvaluatedResultValue {
                    flow_type: FlowType {
                        mode: FlowMode::Continuous,
                        ty: Type::object(ObjectShape::from_ordered_fields(
                            [
                                ("key".to_owned(), key.flow_type.ty),
                                ("value".to_owned(), value.flow_type.ty),
                            ],
                            false,
                        )),
                    },
                    parameter_derived: key.parameter_derived || value.parameter_derived,
                    syntax_selected: key.syntax_selected || value.syntax_selected,
                    static_number: None,
                })
            }
            OwnerConstraintNodeKind::Source => Some(EvaluatedResultValue {
                flow_type: FlowType {
                    mode: FlowMode::PresentOrAbsent,
                    ty: fallback.flow_type.ty,
                },
                ..fallback
            }),
            OwnerConstraintNodeKind::Tag { name } if name == "SKIP" => Some(EvaluatedResultValue {
                flow_type: FlowType {
                    mode: FlowMode::Absent,
                    ty: Type::Absent,
                },
                ..fallback
            }),
            _ => Some(fallback),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn evaluate_when_node(
        &mut self,
        node: &OwnerResultTransferNode,
        nodes: &[OwnerResultTransferNode],
        arguments: &BTreeMap<u32, EvaluatedResultValue>,
        context: Option<&EvaluatedResultValue>,
        fallbacks: &BTreeMap<StableExpressionKey, FlowType>,
        lexical: &BTreeMap<String, EvaluatedResultValue>,
        active: &mut BTreeSet<StableExpressionKey>,
    ) -> Option<EvaluatedResultValue> {
        let selector = node
            .inputs
            .iter()
            .find(|input| matches!(input.role, OwnerConstraintEdgeRole::WhenInput))?;
        let selector = self.evaluate_expression_ref(
            &selector.expression,
            nodes,
            arguments,
            context,
            fallbacks,
            lexical,
            active,
        )?;
        let selector_is_concrete =
            crate::type_is_singleton_syntax_discriminant(&selector.flow_type.ty);
        let mut outputs = Vec::new();
        for arm in node
            .inputs
            .iter()
            .filter(|input| matches!(input.role, OwnerConstraintEdgeRole::WhenArm))
        {
            let OwnerResultExpressionRef::Local { expression } = &arm.expression else {
                continue;
            };
            let arm = nodes
                .binary_search_by(|node| node.expression.cmp(expression))
                .ok()
                .and_then(|index| nodes.get(index))?;
            let pattern = match &arm.kind {
                OwnerConstraintNodeKind::MatchArm { pattern }
                | OwnerConstraintNodeKind::Arrow { pattern } => pattern,
                _ => continue,
            };
            if selector_is_concrete && !owner_pattern_accepts(&selector.flow_type.ty, pattern) {
                continue;
            }
            let mut arm_lexical = lexical.clone();
            extend_owner_pattern_bindings(&mut arm_lexical, &selector, pattern);
            let Some(output) = arm.inputs.iter().find(|input| {
                matches!(
                    input.role,
                    OwnerConstraintEdgeRole::MatchOutput | OwnerConstraintEdgeRole::ArrowOutput
                )
            }) else {
                continue;
            };
            if let Some(output) = self.evaluate_expression_ref(
                &output.expression,
                nodes,
                arguments,
                context,
                fallbacks,
                &arm_lexical,
                active,
            ) && !matches!(output.flow_type.ty, Type::Absent)
            {
                outputs.push(output);
                if selector_is_concrete {
                    // WHEN is ordered. Once a concrete selector accepts one
                    // arm, a later wildcard is unreachable and must not widen
                    // the selected occurrence result.
                    break;
                }
            }
        }
        let mut outputs = outputs.into_iter();
        let first = outputs.next()?;
        let result = outputs.fold(first, |mut result, next| {
            result.flow_type.ty = widen_structural_type(&result.flow_type.ty, &next.flow_type.ty);
            result.parameter_derived |= next.parameter_derived;
            result.syntax_selected |= next.syntax_selected;
            if result.static_number != next.static_number {
                result.static_number = None;
            }
            result
        });
        Some(EvaluatedResultValue {
            flow_type: FlowType {
                mode: selector.flow_type.mode,
                ty: result.flow_type.ty,
            },
            parameter_derived: selector.parameter_derived || result.parameter_derived,
            syntax_selected: result.syntax_selected
                || (selector.parameter_derived && selector_is_concrete),
            static_number: result.static_number,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn evaluate_call_node(
        &mut self,
        node: &OwnerResultTransferNode,
        nodes: &[OwnerResultTransferNode],
        arguments: &BTreeMap<u32, EvaluatedResultValue>,
        context: Option<&EvaluatedResultValue>,
        fallbacks: &BTreeMap<StableExpressionKey, FlowType>,
        lexical: &BTreeMap<String, EvaluatedResultValue>,
        active: &mut BTreeSet<StableExpressionKey>,
    ) -> Option<EvaluatedResultValue> {
        let fallback = transfer_fallback(node, fallbacks)?;
        match node.call_target.as_ref()? {
            OwnerResultCallTarget::Owner { owner } => {
                let interface = *self.interfaces.get(owner)?;
                let mut actuals = BTreeMap::new();
                for parameter in &interface.parameters {
                    let input = transfer_input_for_parameter(
                        &node.inputs,
                        &parameter.name,
                        parameter.kind,
                        parameter.ordinal,
                        interface.parameters.as_ref(),
                    );
                    if let Some(input) = input {
                        let value = self.evaluate_expression_ref(
                            &input.expression,
                            nodes,
                            arguments,
                            context,
                            fallbacks,
                            lexical,
                            active,
                        )?;
                        actuals.insert(parameter.ordinal, value);
                    }
                }
                let explicit_context = node
                    .inputs
                    .iter()
                    .find(|input| {
                        matches!(
                            input.role,
                            OwnerConstraintEdgeRole::CallPass { .. }
                                | OwnerConstraintEdgeRole::PipePass { .. }
                        )
                    })
                    .and_then(|input| {
                        self.evaluate_expression_ref(
                            &input.expression,
                            nodes,
                            arguments,
                            context,
                            fallbacks,
                            lexical,
                            active,
                        )
                    });
                self.evaluate_owner(owner, &actuals, explicit_context.as_ref().or(context))
                    .map(|result| result.value)
                    .or(Some(fallback))
            }
            OwnerResultCallTarget::Abi {
                canonical_name,
                contract,
            } => self
                .evaluate_abi_call(
                    node,
                    canonical_name,
                    contract,
                    nodes,
                    arguments,
                    context,
                    fallbacks,
                    lexical,
                    active,
                )
                .or(Some(fallback)),
            OwnerResultCallTarget::Unresolved | OwnerResultCallTarget::Ambiguous { .. } => {
                Some(fallback)
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn evaluate_abi_call(
        &mut self,
        node: &OwnerResultTransferNode,
        function: &str,
        contract: &OwnerResultAbiContract,
        nodes: &[OwnerResultTransferNode],
        arguments: &BTreeMap<u32, EvaluatedResultValue>,
        context: Option<&EvaluatedResultValue>,
        fallbacks: &BTreeMap<StableExpressionKey, FlowType>,
        lexical: &BTreeMap<String, EvaluatedResultValue>,
        active: &mut BTreeSet<StableExpressionKey>,
    ) -> Option<EvaluatedResultValue> {
        let mut actuals = BTreeMap::new();
        let mut instantiation = BTreeMap::new();
        for parameter in &contract.parameters {
            let input = transfer_input_for_abi_parameter(
                &node.inputs,
                &parameter.name,
                parameter.kind,
                parameter.ordinal,
                &contract.parameters,
            );
            if let Some(input) = input {
                let actual = self.evaluate_expression_ref(
                    &input.expression,
                    nodes,
                    arguments,
                    context,
                    fallbacks,
                    lexical,
                    active,
                )?;
                crate::unify_checked_type_pattern(
                    &parameter.flow_type.ty,
                    &actual.flow_type.ty,
                    &mut instantiation,
                );
                actuals.insert(parameter.ordinal, actual);
            }
        }
        let mut ty = apply_checked_type_substitution_lookup(&contract.result.ty, &instantiation);
        let named_type = |name: &str| {
            abi_actual_by_name(contract, &actuals, name).map(|value| value.flow_type.ty.clone())
        };
        let width_arg = |name: &str| {
            abi_actual_by_name(contract, &actuals, name)
                .and_then(|value| value.static_number.as_ref())
                .and_then(|value| value.to_u64_exact().ok())
                .and_then(|value| u32::try_from(value).ok())
                .filter(|width| (1..=MAX_BITS_WIDTH).contains(width))
        };
        if let Some(static_result) = crate::resolved_bits_builtin_result(
            function,
            named_type("bits"),
            &named_type,
            &width_arg,
        ) {
            ty = static_result;
        }
        if let Some(field) = function.strip_prefix("Field/")
            && let Some(input) = actuals.values().next()
        {
            ty = crate::type_for_nested_path(&input.flow_type.ty, &[field.to_owned()])
                .unwrap_or(Type::Unknown);
        }
        let mode = if function == "List/map" {
            abi_actual_by_name(contract, &actuals, "new")
                .map(|value| value.flow_type.mode)
                .unwrap_or(contract.result.mode)
        } else if function == "List/latest" {
            abi_actual_by_name(contract, &actuals, "list")
                .map(|value| value.flow_type.mode)
                .unwrap_or(contract.result.mode)
        } else if contract.kind == boon_checked::CheckedCallableKind::External {
            actuals.values().fold(contract.result.mode, |mode, actual| {
                crate::merge_flow_modes(mode, actual.flow_type.mode)
            })
        } else {
            contract.result.mode
        };
        Some(EvaluatedResultValue {
            flow_type: FlowType { mode, ty },
            parameter_derived: actuals.values().any(|value| value.parameter_derived),
            syntax_selected: actuals.values().any(|value| value.syntax_selected),
            static_number: None,
        })
    }
}

fn static_number_infix(
    left: &ExactNumber,
    operation: &str,
    right: &ExactNumber,
) -> Option<ExactNumber> {
    match operation {
        "+" => left.checked_add(right).ok(),
        "-" => left.checked_sub(right).ok(),
        "*" => left.checked_mul(right).ok(),
        "/" => left.checked_div(right).ok(),
        "%" => left.checked_rem(right).ok(),
        _ => None,
    }
}

fn transfer_fallback(
    node: &OwnerResultTransferNode,
    fallbacks: &BTreeMap<StableExpressionKey, FlowType>,
) -> Option<EvaluatedResultValue> {
    Some(EvaluatedResultValue {
        flow_type: fallbacks.get(&node.expression)?.clone(),
        parameter_derived: false,
        syntax_selected: false,
        static_number: node
            .static_number
            .as_deref()
            .and_then(|literal| ExactNumber::parse_strict(literal, None).ok()),
    })
}

fn owner_pattern_accepts(selector: &Type, pattern: &crate::OwnerPatternConstraint) -> bool {
    match pattern {
        crate::OwnerPatternConstraint::Wildcard | crate::OwnerPatternConstraint::Binding { .. } => {
            true
        }
        crate::OwnerPatternConstraint::Number => matches!(selector, Type::Number),
        crate::OwnerPatternConstraint::Text => matches!(selector, Type::Text),
        crate::OwnerPatternConstraint::Bits { width } => {
            matches!(selector, Type::Bits { width: actual } if actual == width)
        }
        crate::OwnerPatternConstraint::Tag { name, .. } => {
            matches!(selector, Type::VariantSet(variants) if variants.iter().any(|variant| match variant {
                Variant::Tag(tag) => tag == name,
                Variant::Tagged { tag, .. } => tag == name,
            }))
        }
        crate::OwnerPatternConstraint::Invalid => false,
    }
}

fn extend_owner_pattern_bindings(
    bindings: &mut BTreeMap<String, EvaluatedResultValue>,
    selector: &EvaluatedResultValue,
    pattern: &crate::OwnerPatternConstraint,
) {
    match pattern {
        crate::OwnerPatternConstraint::Binding { name } => {
            bindings.insert(name.clone(), selector.clone());
        }
        crate::OwnerPatternConstraint::Tag { name, fields } => {
            let Some(Variant::Tagged { fields: actual, .. }) = (match &selector.flow_type.ty {
                Type::VariantSet(variants) => variants
                    .iter()
                    .find(|variant| matches!(variant, Variant::Tagged { tag, .. } if tag == name)),
                _ => None,
            }) else {
                return;
            };
            for field in fields {
                if let Some(ty) = actual.fields.get(field) {
                    bindings.insert(
                        field.clone(),
                        EvaluatedResultValue {
                            flow_type: FlowType {
                                mode: selector.flow_type.mode,
                                ty: ty.clone(),
                            },
                            parameter_derived: selector.parameter_derived,
                            syntax_selected: selector.syntax_selected,
                            static_number: selector.static_number.clone(),
                        },
                    );
                }
            }
        }
        crate::OwnerPatternConstraint::Wildcard
        | crate::OwnerPatternConstraint::Number
        | crate::OwnerPatternConstraint::Text
        | crate::OwnerPatternConstraint::Invalid
        | crate::OwnerPatternConstraint::Bits { .. } => {}
    }
}

fn transfer_input_for_parameter<'a>(
    inputs: &'a [crate::OwnerResultTransferInput],
    name: &str,
    kind: OwnerParameterKind,
    ordinal: u32,
    parameters: &[crate::OwnerInterfaceParameter],
) -> Option<&'a crate::OwnerResultTransferInput> {
    inputs.iter().find(|input| match &input.role {
        OwnerConstraintEdgeRole::CallArgument {
            kind: argument_kind,
            name: actual_name,
            ..
        }
        | OwnerConstraintEdgeRole::PipeArgument {
            kind: argument_kind,
            name: actual_name,
            ..
        } => {
            actual_name == name
                && matches!(
                    (kind, argument_kind),
                    (OwnerParameterKind::Value, OwnerArgumentKind::Named)
                        | (OwnerParameterKind::Out, OwnerArgumentKind::Named)
                        | (OwnerParameterKind::Out, OwnerArgumentKind::BareBinding)
                )
        }
        OwnerConstraintEdgeRole::PipeInput => {
            kind == OwnerParameterKind::Value
                && parameters
                    .iter()
                    .filter(|parameter| parameter.kind == OwnerParameterKind::Value)
                    .min_by_key(|parameter| parameter.ordinal)
                    .is_some_and(|parameter| parameter.ordinal == ordinal)
        }
        _ => false,
    })
}

fn transfer_input_for_abi_parameter<'a>(
    inputs: &'a [crate::OwnerResultTransferInput],
    name: &str,
    kind: CheckedParameterKind,
    ordinal: u32,
    parameters: &[OwnerResultAbiParameterContract],
) -> Option<&'a crate::OwnerResultTransferInput> {
    inputs.iter().find(|input| match &input.role {
        OwnerConstraintEdgeRole::CallArgument {
            kind: argument_kind,
            name: actual_name,
            ..
        }
        | OwnerConstraintEdgeRole::PipeArgument {
            kind: argument_kind,
            name: actual_name,
            ..
        } => {
            actual_name == name
                && matches!(
                    (kind, argument_kind),
                    (CheckedParameterKind::Value, OwnerArgumentKind::Named)
                        | (CheckedParameterKind::Out, OwnerArgumentKind::Named)
                        | (CheckedParameterKind::Out, OwnerArgumentKind::BareBinding)
                )
        }
        OwnerConstraintEdgeRole::PipeInput => {
            kind == CheckedParameterKind::Value
                && parameters
                    .iter()
                    .filter(|parameter| parameter.kind == CheckedParameterKind::Value)
                    .min_by_key(|parameter| parameter.ordinal)
                    .is_some_and(|parameter| parameter.ordinal == ordinal)
        }
        _ => false,
    })
}

fn abi_actual_by_name<'a>(
    contract: &OwnerResultAbiContract,
    actuals: &'a BTreeMap<u32, EvaluatedResultValue>,
    name: &str,
) -> Option<&'a EvaluatedResultValue> {
    contract
        .parameters
        .iter()
        .find(|parameter| parameter.name == name)
        .and_then(|parameter| actuals.get(&parameter.ordinal))
}

fn instantiate_call_signature(
    call: &BodyCallPlan,
    interfaces: &BTreeMap<StableCheckOwnerKey, &OwnerPublicInterface>,
    unifier: &mut TypeUnifier,
    abi: &OwnerInferenceAbiEnvironment,
) -> Option<InstantiatedCallSignature> {
    let mut variables = BTreeMap::new();
    if let BodyCallableResolution::Owner(target) = &call.resolution {
        let interface = interfaces.get(target)?;
        return Some(InstantiatedCallSignature {
            kind: CheckedCallableKind::User,
            parameters: interface
                .parameters
                .iter()
                .map(|parameter| InstantiatedCallParameter {
                    name: parameter.name.clone(),
                    kind: parameter.kind,
                    ordinal: parameter.ordinal,
                    flow_type: FlowType {
                        mode: parameter.flow_type.mode,
                        ty: instantiate_type(&parameter.flow_type.ty, unifier, &mut variables),
                    },
                    requirement: parameter.requirement.clone(),
                    evaluation_scope: parameter.evaluation_scope,
                })
                .collect(),
            result: FlowType {
                mode: interface.result.mode,
                ty: instantiate_type(&interface.result.ty, unifier, &mut variables),
            },
            result_flush_type: interface
                .result_flush_type
                .as_ref()
                .map(|ty| instantiate_type(ty, unifier, &mut variables)),
            context: interface
                .context
                .as_ref()
                .map(|context| instantiate_type(&context.flow_type.ty, unifier, &mut variables)),
            effect: interface.effect,
            target: InferredOwnerCallableTarget::Owner {
                owner: target.clone(),
            },
        });
    }
    if !matches!(&call.resolution, BodyCallableResolution::Authoritative) {
        return None;
    }
    abi.callable(&call.function)
        .map(|signature| InstantiatedCallSignature {
            kind: signature.kind,
            parameters: signature
                .parameters
                .iter()
                .map(|parameter| InstantiatedCallParameter {
                    name: parameter.name.clone(),
                    kind: match parameter.kind {
                        CheckedParameterKind::Value => OwnerParameterKind::Value,
                        CheckedParameterKind::Out => OwnerParameterKind::Out,
                    },
                    ordinal: parameter.ordinal,
                    flow_type: FlowType {
                        mode: parameter.flow_type.mode,
                        ty: instantiate_type(&parameter.flow_type.ty, unifier, &mut variables),
                    },
                    requirement: parameter.requirement.clone(),
                    evaluation_scope: match parameter.evaluation_scope {
                        OwnerAbiEvaluationScope::Parent => OwnerInterfaceEvaluationScope::Parent,
                        OwnerAbiEvaluationScope::Output { parameter_ordinal } => {
                            OwnerInterfaceEvaluationScope::Output { parameter_ordinal }
                        }
                    },
                })
                .collect(),
            result: FlowType {
                mode: signature.result.mode,
                ty: instantiate_type(&signature.result.ty, unifier, &mut variables),
            },
            result_flush_type: None,
            context: None,
            effect: signature.effect,
            target: InferredOwnerCallableTarget::Authoritative,
        })
}

fn instantiated_body_input_for_parameter(
    call: &BodyCallPlan,
    parameter: &InstantiatedCallParameter,
    parameters: &[InstantiatedCallParameter],
) -> Option<u32> {
    call.inputs.iter().find_map(|(role, expression)| {
        let matches_parameter = match role {
            OwnerConstraintEdgeRole::CallArgument { kind, name, .. }
            | OwnerConstraintEdgeRole::PipeArgument { kind, name, .. } => {
                name == &parameter.name
                    && matches!(
                        (parameter.kind, kind),
                        (OwnerParameterKind::Value, OwnerArgumentKind::Named)
                            | (OwnerParameterKind::Out, OwnerArgumentKind::Named)
                            | (OwnerParameterKind::Out, OwnerArgumentKind::BareBinding)
                    )
            }
            OwnerConstraintEdgeRole::PipeInput => {
                parameter.kind == OwnerParameterKind::Value
                    && parameters
                        .iter()
                        .filter(|candidate| candidate.kind == OwnerParameterKind::Value)
                        .min_by_key(|candidate| candidate.ordinal)
                        .is_some_and(|candidate| candidate.ordinal == parameter.ordinal)
            }
            _ => false,
        };
        matches_parameter.then_some(*expression)
    })
}

#[allow(clippy::too_many_arguments)]
fn bind_output_scope_expression(
    reference: u32,
    output_name: &str,
    output_variable: TypeVar,
    seed: &OwnerConstraintSeed,
    unifier: &mut TypeUnifier,
    expressions: &[TypeVar],
    external_expressions: &[TypeVar],
    active: &mut BTreeSet<u32>,
) {
    let Some(variable) = expression_variable(expressions, external_expressions, reference) else {
        return;
    };
    let Some(expression) = seed.expressions.get(reference as usize) else {
        return;
    };
    if !active.insert(reference) {
        return;
    }
    if let OwnerConstraintNodeKind::Reference { parts } | OwnerConstraintNodeKind::Drain { parts } =
        &expression.kind
        && let Some((root, projection)) = parts.split_first()
        && root == output_name
    {
        let projected = bind_projection(unifier, output_variable, projection);
        unifier.unify(Type::Var(variable), Type::Var(projected));
    }

    // A block binding shadows the fresh OUT only for the following block
    // inputs. Its initializer still evaluates in the enclosing output scope.
    if matches!(expression.kind, OwnerConstraintNodeKind::Block) {
        let mut shadowed = false;
        for input in &expression.inputs {
            if !shadowed {
                bind_output_scope_expression(
                    input.expression,
                    output_name,
                    output_variable,
                    seed,
                    unifier,
                    expressions,
                    external_expressions,
                    active,
                );
            }
            if matches!(
                &input.role,
                OwnerConstraintEdgeRole::BlockBinding { name } if name == output_name
            ) {
                shadowed = true;
            }
        }
    } else {
        for input in &expression.inputs {
            bind_output_scope_expression(
                input.expression,
                output_name,
                output_variable,
                seed,
                unifier,
                expressions,
                external_expressions,
                active,
            );
        }
    }
    active.remove(&reference);
}

fn bind_output_scoped_call_inputs(
    call: &BodyCallPlan,
    parameters: &[InstantiatedCallParameter],
    seed: &OwnerConstraintSeed,
    unifier: &mut TypeUnifier,
    expressions: &[TypeVar],
    external_expressions: &[TypeVar],
) {
    let fresh_outputs = parameters
        .iter()
        .filter(|parameter| parameter.kind == OwnerParameterKind::Out)
        .filter_map(|parameter| {
            let reference = instantiated_body_input_for_parameter(call, parameter, parameters)?;
            let input = call.inputs.iter().find(|(role, expression)| {
                *expression == reference
                    && matches!(
                        role,
                        OwnerConstraintEdgeRole::CallArgument {
                            kind: OwnerArgumentKind::BareBinding,
                            ..
                        } | OwnerConstraintEdgeRole::PipeArgument {
                            kind: OwnerArgumentKind::BareBinding,
                            ..
                        }
                    )
            })?;
            let expression = seed.expressions.get(reference as usize)?;
            let OwnerConstraintNodeKind::Reference { parts } = &expression.kind else {
                return None;
            };
            let [name] = parts.as_ref() else {
                return None;
            };
            let variable = expression_variable(expressions, external_expressions, input.1)?;
            Some((parameter.ordinal, (name.clone(), variable)))
        })
        .collect::<BTreeMap<_, _>>();

    for parameter in parameters {
        let OwnerInterfaceEvaluationScope::Output { parameter_ordinal } =
            parameter.evaluation_scope
        else {
            continue;
        };
        let Some((output_name, output_variable)) = fresh_outputs.get(&parameter_ordinal) else {
            continue;
        };
        let Some(reference) = instantiated_body_input_for_parameter(call, parameter, parameters)
        else {
            continue;
        };
        bind_output_scope_expression(
            reference,
            output_name,
            *output_variable,
            seed,
            unifier,
            expressions,
            external_expressions,
            &mut BTreeSet::new(),
        );
    }
}

fn push_owner_call_diagnostic(
    diagnostics: &mut Vec<OwnerDiagnosticTemplate>,
    call: &BodyCallPlan,
    code: &str,
    message: String,
    role: Option<OwnerSourceAnchorRole>,
) {
    diagnostics.push(OwnerDiagnosticTemplate {
        severity: DiagnosticSeverity::Error,
        code: code.to_owned(),
        message,
        site: OwnerSourceAnchorSite::Expression {
            expression: call.stable_expression.clone(),
        },
        role,
    });
}

fn owner_call_argument(
    role: &OwnerConstraintEdgeRole,
) -> Option<(u32, OwnerArgumentKind, &str, OwnerSourceAnchorRole)> {
    match role {
        OwnerConstraintEdgeRole::CallArgument {
            ordinal,
            kind,
            name,
        } => Some((
            *ordinal,
            *kind,
            name,
            OwnerSourceAnchorRole::CallArgument { ordinal: *ordinal },
        )),
        OwnerConstraintEdgeRole::PipeArgument {
            ordinal,
            kind,
            name,
        } => Some((
            *ordinal,
            *kind,
            name,
            OwnerSourceAnchorRole::PipeArgument { ordinal: *ordinal },
        )),
        _ => None,
    }
}

fn validate_owner_call_shape(
    call: &BodyCallPlan,
    parameters: &[InstantiatedCallParameter],
    target: &InferredOwnerCallableTarget,
    target_kind: Option<CheckedCallableKind>,
    callee_context: Option<&Type>,
    caller_has_context: bool,
    caller_is_callable: bool,
    diagnostics: &mut Vec<OwnerDiagnosticTemplate>,
) -> bool {
    let pipe_input = call
        .inputs
        .iter()
        .any(|(role, _)| matches!(role, OwnerConstraintEdgeRole::PipeInput));
    let piped_parameter = pipe_input.then(|| {
        parameters
            .iter()
            .filter(|parameter| parameter.kind == OwnerParameterKind::Value)
            .min_by_key(|parameter| parameter.ordinal)
    });
    let piped_parameter = piped_parameter.flatten();
    let mut valid = true;
    if pipe_input && piped_parameter.is_none() {
        push_owner_call_diagnostic(
            diagnostics,
            call,
            "pipe_without_value_input",
            format!("`{}` has no ordinary input for the pipe", call.function),
            None,
        );
        valid = false;
    }

    let expected = parameters
        .iter()
        .filter(|parameter| piped_parameter.is_none_or(|piped| parameter.ordinal != piped.ordinal))
        .collect::<Vec<_>>();
    let mut expected_index = 0usize;
    for (call_index, (ordinal, kind, name, role)) in call
        .inputs
        .iter()
        .filter_map(|(role, _)| owner_call_argument(role))
        .enumerate()
    {
        while let Some(parameter) = expected.get(expected_index).copied()
            && parameter.name != name
            && parameter.requirement.is_optional()
        {
            expected_index += 1;
        }
        let Some(parameter) = expected.get(expected_index).copied() else {
            push_owner_call_diagnostic(
                diagnostics,
                call,
                "unexpected_call_entry",
                format!(
                    "`{}` has an unexpected extra call entry `{name}`",
                    call.function
                ),
                Some(role),
            );
            valid = false;
            continue;
        };
        if parameter.name != name {
            push_owner_call_diagnostic(
                diagnostics,
                call,
                "misordered_call_entry",
                format!(
                    "`{}` call entry {} must be `{}`, found `{name}`; arguments keep declaration names and order",
                    call.function,
                    call_index + 1,
                    parameter.name,
                ),
                Some(role),
            );
            expected_index += 1;
            valid = false;
            continue;
        }
        expected_index += 1;
        if parameter.kind == OwnerParameterKind::Value && kind == OwnerArgumentKind::BareBinding {
            push_owner_call_diagnostic(
                diagnostics,
                call,
                "bare_ordinary_input",
                format!(
                    "bare `{name}` cannot fill ordinary input `{}`; write `{}: expression`",
                    parameter.name, parameter.name
                ),
                Some(role),
            );
            valid = false;
        }
        debug_assert_eq!(ordinal as usize, call_index);
    }
    for parameter in expected.iter().skip(expected_index) {
        if parameter.requirement.is_optional() {
            continue;
        }
        push_owner_call_diagnostic(
            diagnostics,
            call,
            "missing_call_entry",
            format!(
                "`{}` is missing call entry `{}`",
                call.function, parameter.name
            ),
            None,
        );
        valid = false;
    }

    let explicit_pass = call.inputs.iter().find_map(|(role, _)| match role {
        OwnerConstraintEdgeRole::CallPass { .. } => Some(OwnerSourceAnchorRole::CallPass),
        OwnerConstraintEdgeRole::PipePass { .. } => Some(OwnerSourceAnchorRole::PipePass),
        _ => None,
    });
    if let Some(role) = explicit_pass
        && matches!(target, InferredOwnerCallableTarget::Authoritative)
    {
        push_owner_call_diagnostic(
            diagnostics,
            call,
            "pass_on_authoritative_callable",
            format!(
                "`PASS:` is only valid on user callable calls; `{}` is {}",
                call.function,
                match target_kind {
                    Some(CheckedCallableKind::Builtin) => "a built-in callable",
                    Some(CheckedCallableKind::External) => "an external callable",
                    Some(CheckedCallableKind::User) | None => "authoritative",
                }
            ),
            Some(role),
        );
        valid = false;
    }
    if callee_context.is_some()
        && explicit_pass.is_none()
        && !caller_has_context
        && matches!(target, InferredOwnerCallableTarget::Owner { .. })
    {
        push_owner_call_diagnostic(
            diagnostics,
            call,
            "missing_pass_context",
            format!(
                "{}",
                if caller_is_callable {
                    format!(
                        "call to `FUNCTION {}` requires explicit or inherited PASS context",
                        call.function
                    )
                } else {
                    format!(
                        "root call to `FUNCTION {}` requires a final `PASS:` clause",
                        call.function
                    )
                }
            ),
            None,
        );
    }
    valid
}

fn bind_calls(
    calls: Vec<BodyCallPlan>,
    seed: &OwnerConstraintSeed,
    interfaces: &BTreeMap<StableCheckOwnerKey, &OwnerPublicInterface>,
    unifier: &mut TypeUnifier,
    expressions: &[TypeVar],
    external_expressions: &[TypeVar],
    call_flushes: &[TypeVar],
    modes: &mut [Option<FlowMode>],
    direct_effects: &mut [CheckedEffectSummary],
    abi: &OwnerInferenceAbiEnvironment,
    pre_call_actual_types: &[Type],
    caller_has_context: bool,
    caller_is_callable: bool,
    diagnostics: &mut Vec<OwnerDiagnosticTemplate>,
    work: &mut OwnerBodyInferenceWork,
) -> Vec<InferredCallDraft> {
    calls
        .into_iter()
        .map(|call| {
            work.calls = work.calls.saturating_add(1);
            let signature = instantiate_call_signature(&call, interfaces, unifier, abi);
            let call_variable = expressions[call.expression];
            let (
                parameters,
                result,
                result_flush_type,
                context,
                effect,
                target,
                target_kind,
                mut valid,
            ) = match signature {
                Some(signature) => (
                    signature.parameters,
                    signature.result,
                    signature.result_flush_type,
                    signature.context,
                    signature.effect,
                    signature.target,
                    Some(signature.kind),
                    true,
                ),
                None => {
                    let (code, message, target) = match &call.resolution {
                        BodyCallableResolution::Ambiguous(candidates) => (
                            "ambiguous_callable",
                            format!(
                                "ambiguous function `{}` has {} equally ranked project targets",
                                call.function,
                                candidates.len()
                            ),
                            InferredOwnerCallableTarget::Ambiguous {
                                candidates: candidates.clone(),
                            },
                        ),
                        BodyCallableResolution::Authoritative => (
                            "missing_authoritative_callable",
                            format!(
                                "authoritative function `{}` has no ABI signature",
                                call.function
                            ),
                            InferredOwnerCallableTarget::Authoritative,
                        ),
                        BodyCallableResolution::Owner(owner) => (
                            "missing_owner_interface",
                            format!(
                                "function `{}` resolved to {owner:?} without a frozen interface",
                                call.function
                            ),
                            InferredOwnerCallableTarget::Owner {
                                owner: owner.clone(),
                            },
                        ),
                        BodyCallableResolution::Unresolved => (
                            "unresolved_callable",
                            format!("unknown function `{}`", call.function),
                            InferredOwnerCallableTarget::Unresolved,
                        ),
                    };
                    diagnostics.push(OwnerDiagnosticTemplate {
                        severity: DiagnosticSeverity::Error,
                        code: code.to_owned(),
                        message,
                        site: OwnerSourceAnchorSite::Expression {
                            expression: call.stable_expression.clone(),
                        },
                        role: None,
                    });
                    (
                        Vec::new(),
                        FlowType {
                            mode: FlowMode::Continuous,
                            ty: Type::Unknown,
                        },
                        None,
                        None,
                        CheckedEffectSummary::default(),
                        target,
                        None,
                        false,
                    )
                }
            };
            if valid {
                valid = validate_owner_call_shape(
                    &call,
                    &parameters,
                    &target,
                    target_kind,
                    context.as_ref(),
                    caller_has_context,
                    caller_is_callable,
                    diagnostics,
                );
            }
            let actual_inputs = call
                .inputs
                .iter()
                .filter_map(|(_, reference)| {
                    pre_call_actual_types
                        .get(*reference as usize)
                        .cloned()
                        .map(|actual| (*reference, actual))
                })
                .collect::<BTreeMap<_, _>>();
            let signature_result = result.clone();

            if valid && let Some(field) = call.function.strip_prefix("Field/") {
                if let Some(input) = call.inputs.iter().find_map(|(role, input)| {
                    matches!(role, OwnerConstraintEdgeRole::PipeInput).then_some(*input)
                }) && let Some(input) =
                    expression_variable(expressions, external_expressions, input)
                {
                    let projected = bind_projection(unifier, input, &[field.to_owned()]);
                    unifier.unify(Type::Var(call_variable), Type::Var(projected));
                }
            } else if !matches!(call.resolution, BodyCallableResolution::Owner(_)) || !valid {
                // A user callable's principal result is intentionally allowed
                // to be wider than this occurrence (for example a syntax-
                // dispatched function). Bind user results only after the
                // frozen result transfer has evaluated the actual arguments.
                unifier.bind_var(call_variable, result.ty);
            }

            for (role, input) in call.inputs.iter().filter(|_| valid) {
                let Some(input) = expression_variable(expressions, external_expressions, *input)
                else {
                    continue;
                };
                match role {
                    OwnerConstraintEdgeRole::PipeInput => {
                        if let Some(expected) = parameters
                            .iter()
                            .find(|parameter| parameter.kind == OwnerParameterKind::Value)
                        {
                            unifier.unify(Type::Var(input), expected.flow_type.ty.clone());
                        }
                    }
                    OwnerConstraintEdgeRole::CallArgument { kind, name, .. }
                    | OwnerConstraintEdgeRole::PipeArgument { kind, name, .. } => {
                        if let Some(expected) =
                            parameters.iter().find(|parameter| parameter.name == *name)
                            && matches!(
                                (expected.kind, kind),
                                (OwnerParameterKind::Value, OwnerArgumentKind::Named)
                                    | (OwnerParameterKind::Out, OwnerArgumentKind::Named)
                                    | (OwnerParameterKind::Out, OwnerArgumentKind::BareBinding)
                            )
                        {
                            unifier.unify(Type::Var(input), expected.flow_type.ty.clone());
                        }
                    }
                    OwnerConstraintEdgeRole::CallPass { .. }
                    | OwnerConstraintEdgeRole::PipePass { .. } => {
                        if let Some(context) = &context {
                            unifier.unify(Type::Var(input), context.clone());
                        }
                    }
                    _ => {}
                }
            }
            if valid {
                bind_output_scoped_call_inputs(
                    &call,
                    &parameters,
                    seed,
                    unifier,
                    expressions,
                    external_expressions,
                );
            }
            modes[call.expression] = flow_mode_join(modes[call.expression], Some(result.mode));
            if valid {
                direct_effects[call.expression] =
                    merge_effects(direct_effects[call.expression], effect);
            }
            if let Some(call_flush) = call_flushes.get(call.expression).copied() {
                unifier.bind_var(
                    call_flush,
                    if valid {
                        result_flush_type.unwrap_or(Type::Absent)
                    } else {
                        Type::Absent
                    },
                );
            }
            work.interface_imports = work.interface_imports.saturating_add(1);
            InferredCallDraft {
                plan: call,
                target,
                effect: if valid {
                    effect
                } else {
                    CheckedEffectSummary::default()
                },
                actual_inputs,
                signature_result,
                resolved_result: None,
                type_substitutions: Vec::new(),
                contextual_type_variables: Vec::new(),
                syntax_discriminated_result: false,
                valid,
            }
        })
        .collect()
}

fn body_input_for_parameter(
    call: &BodyCallPlan,
    parameter: &crate::OwnerInterfaceParameter,
    parameters: &[crate::OwnerInterfaceParameter],
) -> Option<u32> {
    call.inputs.iter().find_map(|(role, expression)| {
        let matches_parameter = match role {
            OwnerConstraintEdgeRole::CallArgument { kind, name, .. }
            | OwnerConstraintEdgeRole::PipeArgument { kind, name, .. } => {
                name == &parameter.name
                    && matches!(
                        (parameter.kind, kind),
                        (OwnerParameterKind::Value, OwnerArgumentKind::Named)
                            | (OwnerParameterKind::Out, OwnerArgumentKind::Named)
                            | (OwnerParameterKind::Out, OwnerArgumentKind::BareBinding)
                    )
            }
            OwnerConstraintEdgeRole::PipeInput => {
                parameter.kind == OwnerParameterKind::Value
                    && parameters
                        .iter()
                        .filter(|candidate| candidate.kind == OwnerParameterKind::Value)
                        .min_by_key(|candidate| candidate.ordinal)
                        .is_some_and(|candidate| candidate.ordinal == parameter.ordinal)
            }
            _ => false,
        };
        matches_parameter.then_some(*expression)
    })
}

fn body_expression_result_value(
    reference: u32,
    syntax: &OwnerSyntaxInput,
    seed: &OwnerConstraintSeed,
    interfaces: &BTreeMap<StableCheckOwnerKey, &OwnerPublicInterface>,
    unifier: &mut TypeUnifier,
    expressions: &[TypeVar],
    external_expressions: &[TypeVar],
    modes: &[Option<FlowMode>],
) -> Option<EvaluatedResultValue> {
    let variable = expression_variable(expressions, external_expressions, reference)?;
    let index = reference as usize;
    let mode = if index < expressions.len() {
        modes.get(index).copied().flatten()
    } else {
        seed.external_expressions
            .get(index.checked_sub(expressions.len())?)
            .and_then(|external| interfaces.get(&external.owner))
            .map(|interface| interface.result.mode)
    }
    .unwrap_or(FlowMode::Continuous);
    Some(EvaluatedResultValue {
        flow_type: FlowType {
            mode,
            ty: unifier.resolve(&Type::Var(variable)),
        },
        parameter_derived: false,
        syntax_selected: false,
        static_number: syntax_static_number(syntax, reference, &mut BTreeSet::new()),
    })
}

fn syntax_static_number(
    syntax: &OwnerSyntaxInput,
    reference: u32,
    active: &mut BTreeSet<u32>,
) -> Option<ExactNumber> {
    let expression = syntax.expressions.get(reference as usize)?;
    if !active.insert(reference) {
        return None;
    }
    let result = (|| match &expression.kind {
        AstExprKind::Number(literal) => ExactNumber::parse_strict(literal, None).ok(),
        AstExprKind::Infix { left, op, right } => {
            let left = syntax_static_number(syntax, u32::try_from(*left).ok()?, active)?;
            let right = syntax_static_number(syntax, u32::try_from(*right).ok()?, active)?;
            static_number_infix(&left, op, &right)
        }
        _ => None,
    })();
    active.remove(&reference);
    result
}

#[allow(clippy::too_many_arguments)]
fn refine_owner_call_at(
    call_index: usize,
    call_by_expression: &BTreeMap<usize, usize>,
    states: &mut [u8],
    drafts: &mut [InferredCallDraft],
    syntax: &OwnerSyntaxInput,
    seed: &OwnerConstraintSeed,
    interfaces: &BTreeMap<StableCheckOwnerKey, &OwnerPublicInterface>,
    unifier: &mut TypeUnifier,
    expressions: &[TypeVar],
    external_expressions: &[TypeVar],
    caller_context: Option<TypeVar>,
    modes: &mut [Option<FlowMode>],
) {
    match states.get(call_index).copied() {
        Some(2) | None => return,
        Some(1) => return,
        Some(0) | Some(_) => states[call_index] = 1,
    }
    if !drafts[call_index].valid {
        states[call_index] = 2;
        return;
    }
    let plan = drafts[call_index].plan.clone();
    for (_, input) in &plan.inputs {
        let input = *input as usize;
        if input < expressions.len()
            && let Some(dependency) = call_by_expression.get(&input).copied()
        {
            refine_owner_call_at(
                dependency,
                call_by_expression,
                states,
                drafts,
                syntax,
                seed,
                interfaces,
                unifier,
                expressions,
                external_expressions,
                caller_context,
                modes,
            );
        }
    }

    let BodyCallableResolution::Owner(target_owner) = &plan.resolution else {
        states[call_index] = 2;
        return;
    };
    let Some(target_interface) = interfaces.get(target_owner).copied() else {
        states[call_index] = 2;
        return;
    };
    let mut arguments = BTreeMap::new();
    for parameter in &target_interface.parameters {
        let Some(reference) =
            body_input_for_parameter(&plan, parameter, target_interface.parameters.as_ref())
        else {
            continue;
        };
        if let Some(actual) = body_expression_result_value(
            reference,
            syntax,
            seed,
            interfaces,
            unifier,
            expressions,
            external_expressions,
            modes,
        ) {
            arguments.insert(parameter.ordinal, actual);
        }
    }
    let explicit_context = plan
        .inputs
        .iter()
        .find(|(role, _)| {
            matches!(
                role,
                OwnerConstraintEdgeRole::CallPass { .. } | OwnerConstraintEdgeRole::PipePass { .. }
            )
        })
        .and_then(|(_, reference)| {
            body_expression_result_value(
                *reference,
                syntax,
                seed,
                interfaces,
                unifier,
                expressions,
                external_expressions,
                modes,
            )
        });
    let inherited_context = caller_context.map(|variable| EvaluatedResultValue {
        flow_type: FlowType {
            mode: FlowMode::Continuous,
            ty: unifier.resolve(&Type::Var(variable)),
        },
        parameter_derived: false,
        syntax_selected: false,
        static_number: None,
    });
    let evaluated = {
        let mut evaluator = OwnerResultTransferEvaluator::new(interfaces, unifier);
        evaluator.evaluate_owner(
            target_owner,
            &arguments,
            explicit_context.as_ref().or(inherited_context.as_ref()),
        )
    };
    if let Some(evaluated) = evaluated {
        unifier.bind_var(
            expressions[plan.expression],
            evaluated.value.flow_type.ty.clone(),
        );
        modes[plan.expression] = Some(evaluated.value.flow_type.mode);
        let draft = &mut drafts[call_index];
        draft.resolved_result = Some(evaluated.value.flow_type.clone());
        draft.type_substitutions = evaluated.type_substitutions;
        draft.contextual_type_variables = evaluated.contextual_type_variables;
        draft.syntax_discriminated_result = evaluated.value.syntax_selected;
    }
    states[call_index] = 2;
}

#[allow(clippy::too_many_arguments)]
fn refine_owner_call_transfers(
    drafts: &mut [InferredCallDraft],
    syntax: &OwnerSyntaxInput,
    seed: &OwnerConstraintSeed,
    interfaces: &BTreeMap<StableCheckOwnerKey, &OwnerPublicInterface>,
    unifier: &mut TypeUnifier,
    expressions: &[TypeVar],
    external_expressions: &[TypeVar],
    caller_context: Option<TypeVar>,
    modes: &mut [Option<FlowMode>],
) {
    let call_by_expression = drafts
        .iter()
        .enumerate()
        .map(|(index, draft)| (draft.plan.expression, index))
        .collect::<BTreeMap<_, _>>();
    let mut states = vec![0; drafts.len()];
    for call_index in 0..drafts.len() {
        refine_owner_call_at(
            call_index,
            &call_by_expression,
            &mut states,
            drafts,
            syntax,
            seed,
            interfaces,
            unifier,
            expressions,
            external_expressions,
            caller_context,
            modes,
        );
    }
}

fn body_input_for_abi_parameter(
    call: &BodyCallPlan,
    parameter: &crate::OwnerInferenceParameterContract,
    parameters: &[crate::OwnerInferenceParameterContract],
) -> Option<u32> {
    call.inputs.iter().find_map(|(role, expression)| {
        let matches_parameter = match role {
            OwnerConstraintEdgeRole::CallArgument { kind, name, .. }
            | OwnerConstraintEdgeRole::PipeArgument { kind, name, .. } => {
                name == &parameter.name
                    && matches!(
                        (parameter.kind, kind),
                        (CheckedParameterKind::Value, OwnerArgumentKind::Named)
                            | (CheckedParameterKind::Out, OwnerArgumentKind::Named)
                            | (CheckedParameterKind::Out, OwnerArgumentKind::BareBinding)
                    )
            }
            OwnerConstraintEdgeRole::PipeInput => {
                parameter.kind == CheckedParameterKind::Value
                    && parameters
                        .iter()
                        .filter(|candidate| candidate.kind == CheckedParameterKind::Value)
                        .min_by_key(|candidate| candidate.ordinal)
                        .is_some_and(|candidate| candidate.ordinal == parameter.ordinal)
            }
            _ => false,
        };
        matches_parameter.then_some(*expression)
    })
}

fn body_call_parameter_diagnostic_role(
    call: &BodyCallPlan,
    name: &str,
) -> Option<OwnerSourceAnchorRole> {
    call.inputs.iter().find_map(|(role, _)| match role {
        OwnerConstraintEdgeRole::CallArgument {
            name: candidate,
            ordinal,
            ..
        } if candidate == name => Some(OwnerSourceAnchorRole::CallArgument { ordinal: *ordinal }),
        OwnerConstraintEdgeRole::PipeArgument {
            name: candidate,
            ordinal,
            ..
        } if candidate == name => Some(OwnerSourceAnchorRole::PipeArgument { ordinal: *ordinal }),
        _ => None,
    })
}

fn body_expression_type(
    reference: u32,
    unifier: &mut TypeUnifier,
    expressions: &[TypeVar],
    external_expressions: &[TypeVar],
) -> Option<Type> {
    expression_variable(expressions, external_expressions, reference)
        .map(|variable| unifier.resolve(&Type::Var(variable)))
}

fn normalized_type_substitutions(
    substitutions: BTreeMap<TypeVar, Type>,
) -> BTreeMap<TypeVar, Type> {
    let lookup = substitutions.clone();
    substitutions
        .into_iter()
        .map(|(variable, value)| {
            (
                variable,
                apply_checked_type_substitution_lookup(&value, &lookup),
            )
        })
        .collect()
}

fn push_user_argument_type_diagnostic(
    diagnostics: &mut Vec<OwnerDiagnosticTemplate>,
    call: &BodyCallPlan,
    name: &str,
    actual: &Type,
    expected: &Type,
) {
    if crate::type_is_assignable_to(actual, expected) {
        return;
    }
    let message = if let Some(field) = crate::missing_field_name(actual, expected) {
        format!(
            "object is missing field `{field}`\nexpected: {}\nfound: {}",
            crate::boon_facing_type_label(expected),
            crate::boon_facing_type_label(actual)
        )
    } else if let Some(field) = crate::incompatible_field_name(actual, expected) {
        format!(
            "object field `{field}` has incompatible type\nexpected: {}\nfound: {}",
            crate::boon_facing_type_label(expected),
            crate::boon_facing_type_label(actual)
        )
    } else {
        format!(
            "`FUNCTION {}` argument `{name}` does not satisfy the required structural shape\nexpected: {}\nfound: {}",
            call.function,
            crate::boon_facing_type_label(expected),
            crate::boon_facing_type_label(actual)
        )
    };
    push_owner_call_diagnostic(
        diagnostics,
        call,
        "user_call_argument_type",
        message,
        body_call_parameter_diagnostic_role(call, name),
    );
}

fn push_pass_type_diagnostic(
    diagnostics: &mut Vec<OwnerDiagnosticTemplate>,
    call: &BodyCallPlan,
    actual: &Type,
    expected: &Type,
) {
    if crate::type_is_assignable_to(actual, expected) {
        return;
    }
    let detail = if let Some(field) = crate::missing_field_name(actual, expected) {
        format!("missing required field `{field}`")
    } else if let Some(field) = crate::incompatible_field_name(actual, expected) {
        format!("field `{field}` has an incompatible type")
    } else {
        "context value has an incompatible type".to_owned()
    };
    let role = call.inputs.iter().find_map(|(role, _)| match role {
        OwnerConstraintEdgeRole::CallPass { .. } => Some(OwnerSourceAnchorRole::CallPass),
        OwnerConstraintEdgeRole::PipePass { .. } => Some(OwnerSourceAnchorRole::PipePass),
        _ => None,
    });
    push_owner_call_diagnostic(
        diagnostics,
        call,
        "pass_context_type",
        format!(
            "`FUNCTION {}` PASS context {detail}\nexpected: {}\nfound: {}",
            call.function,
            crate::boon_facing_type_label(expected),
            crate::boon_facing_type_label(actual)
        ),
        role,
    );
}

fn push_contextual_argument_type_diagnostic(
    diagnostics: &mut Vec<OwnerDiagnosticTemplate>,
    call: &BodyCallPlan,
    name: &str,
    actual: &Type,
    expected: &Type,
) {
    if crate::type_is_assignable_to(actual, expected) {
        return;
    }
    push_owner_call_diagnostic(
        diagnostics,
        call,
        "contextual_call_argument_type",
        format!(
            "`{}` argument `{name}` has incompatible contextual type\nexpected: {}\nfound: {}",
            call.function,
            crate::boon_facing_type_label(expected),
            crate::boon_facing_type_label(actual)
        ),
        body_call_parameter_diagnostic_role(call, name),
    );
}

#[allow(clippy::too_many_arguments)]
fn validate_owner_call_types(
    drafts: &mut [InferredCallDraft],
    interfaces: &BTreeMap<StableCheckOwnerKey, &OwnerPublicInterface>,
    abi: &OwnerInferenceAbiEnvironment,
    unifier: &mut TypeUnifier,
    expressions: &[TypeVar],
    external_expressions: &[TypeVar],
    caller_context: Option<TypeVar>,
    diagnostics: &mut Vec<OwnerDiagnosticTemplate>,
) {
    let mut call_results = BTreeMap::new();
    for draft in drafts.iter().filter(|draft| draft.valid) {
        let result = draft.resolved_result.as_ref().map_or_else(
            || FlowType {
                mode: draft.signature_result.mode,
                ty: unifier.resolve(&draft.signature_result.ty),
            },
            Clone::clone,
        );
        call_results.insert(draft.plan.expression, result.ty);
    }
    for draft in drafts.iter_mut().filter(|draft| draft.valid) {
        let call = &draft.plan;
        let explicit_context = call.inputs.iter().find_map(|(role, reference)| {
            matches!(
                role,
                OwnerConstraintEdgeRole::CallPass { .. } | OwnerConstraintEdgeRole::PipePass { .. }
            )
            .then_some(*reference)
        });
        match &draft.target {
            InferredOwnerCallableTarget::Owner { owner } => {
                let Some(interface) = interfaces.get(owner).copied() else {
                    continue;
                };
                let substitutions = normalized_type_substitutions(
                    draft.type_substitutions.iter().cloned().collect(),
                );
                let mut actuals = BTreeMap::new();
                let mut exact_actuals = BTreeMap::new();
                for parameter in &interface.parameters {
                    let Some(reference) =
                        body_input_for_parameter(call, parameter, interface.parameters.as_ref())
                    else {
                        continue;
                    };
                    let Some(actual) =
                        body_expression_type(reference, unifier, expressions, external_expressions)
                    else {
                        continue;
                    };
                    actuals.insert(parameter.ordinal, actual);
                    if let Some(exact) = ((reference as usize) < expressions.len())
                        .then(|| call_results.get(&(reference as usize)).cloned())
                        .flatten()
                        .or_else(|| draft.actual_inputs.get(&reference).cloned())
                    {
                        exact_actuals.insert(parameter.ordinal, unifier.resolve(&exact));
                    }
                }
                let exact_context_actual = explicit_context
                    .and_then(|reference| {
                        ((reference as usize) < expressions.len())
                            .then(|| call_results.get(&(reference as usize)).cloned())
                            .flatten()
                            .or_else(|| draft.actual_inputs.get(&reference).cloned())
                    })
                    .map(|actual| unifier.resolve(&actual))
                    .or_else(|| {
                        caller_context.map(|variable| unifier.resolve(&Type::Var(variable)))
                    });
                for parameter in interface
                    .parameters
                    .iter()
                    .filter(|parameter| parameter.kind == OwnerParameterKind::Value)
                {
                    let Some(actual) = exact_actuals.get(&parameter.ordinal) else {
                        continue;
                    };
                    let expected =
                        crate::substitute_checked_type(&parameter.flow_type.ty, &substitutions);
                    push_user_argument_type_diagnostic(
                        diagnostics,
                        call,
                        &parameter.name,
                        actual,
                        &expected,
                    );
                    if matches!(
                        parameter.evaluation_scope,
                        OwnerInterfaceEvaluationScope::Output { .. }
                    ) && let Some(contextual_actual) = actuals.get(&parameter.ordinal)
                    {
                        push_contextual_argument_type_diagnostic(
                            diagnostics,
                            call,
                            &parameter.name,
                            contextual_actual,
                            &expected,
                        );
                    }
                }
                if explicit_context.is_some()
                    && let (Some(context), Some(actual)) =
                        (&interface.context, exact_context_actual)
                {
                    let expected =
                        crate::substitute_checked_type(&context.flow_type.ty, &substitutions);
                    push_pass_type_diagnostic(diagnostics, call, &actual, &expected);
                }
            }
            InferredOwnerCallableTarget::Authoritative => {
                let Some(contract) = abi.callable(&call.function) else {
                    continue;
                };
                let mut substitutions = BTreeMap::new();
                let mut actuals = BTreeMap::new();
                for parameter in &contract.parameters {
                    let Some(reference) =
                        body_input_for_abi_parameter(call, parameter, &contract.parameters)
                    else {
                        continue;
                    };
                    let Some(actual) =
                        body_expression_type(reference, unifier, expressions, external_expressions)
                    else {
                        continue;
                    };
                    crate::unify_checked_type_pattern(
                        &parameter.flow_type.ty,
                        &actual,
                        &mut substitutions,
                    );
                    actuals.insert(parameter.ordinal, actual);
                }
                substitutions = normalized_type_substitutions(substitutions);
                for parameter in contract.parameters.iter().filter(|parameter| {
                    parameter.kind == CheckedParameterKind::Value
                        && matches!(
                            parameter.evaluation_scope,
                            OwnerAbiEvaluationScope::Output { .. }
                        )
                }) {
                    let Some(actual) = actuals.get(&parameter.ordinal) else {
                        continue;
                    };
                    let expected =
                        crate::substitute_checked_type(&parameter.flow_type.ty, &substitutions);
                    push_contextual_argument_type_diagnostic(
                        diagnostics,
                        call,
                        &parameter.name,
                        actual,
                        &expected,
                    );
                }
                draft.type_substitutions = substitutions.into_iter().collect();
            }
            InferredOwnerCallableTarget::Unresolved
            | InferredOwnerCallableTarget::Ambiguous { .. } => continue,
        }
    }
}

fn validate_inputs(
    syntax: &OwnerSyntaxInput,
    seed: &OwnerConstraintSeed,
    summary: &OwnerConstraintSummary,
    own_scc: &OwnerInterfaceSccResult,
) -> Result<(), OwnerBodyInferenceError> {
    if syntax.owner != seed.owner
        || seed.owner != summary.owner
        || !own_scc.key.members.contains(&seed.owner)
    {
        return Err(OwnerBodyInferenceError::new(
            "owner body inference inputs do not name the same owner",
        ));
    }
    if summary.seed_fingerprint_v1 != seed.fingerprint_v1() {
        return Err(OwnerBodyInferenceError::new(
            "owner body inference has mismatched seed and resolved summary",
        ));
    }
    if syntax.expressions.len() != seed.expressions.len()
        || syntax
            .expressions
            .iter()
            .zip(&seed.expressions)
            .any(|(syntax, seed)| syntax.stable_key != seed.expression)
    {
        return Err(OwnerBodyInferenceError::new(
            "owner body inference syntax and constraint expression tables differ",
        ));
    }
    Ok(())
}

/// Infer one immutable owner body against exact frozen public interfaces.
///
/// This function never descends another owner body and never allocates a
/// project-global checked identity. The caller must provide exactly the own,
/// child-value, value-read, and callable interfaces named by the resolved
/// owner inputs plus the exact transitive dependencies of their public result
/// transfer slices.
pub fn evaluate_owner_body<'a>(
    syntax: &OwnerSyntaxInput,
    seed: &OwnerConstraintSeed,
    summary: &OwnerConstraintSummary,
    abi: &OwnerInferenceAbiEnvironment,
    own_scc: &'a OwnerInterfaceSccResult,
    imported_sccs: impl IntoIterator<Item = &'a OwnerInterfaceSccResult>,
) -> Result<OwnerBodyInferenceEvaluation, OwnerBodyInferenceError> {
    validate_inputs(syntax, seed, summary, own_scc)?;
    if abi.subjects() != std::slice::from_ref(&seed.owner) {
        return Err(OwnerBodyInferenceError::new(
            "owner body inference ABI does not match its exact owner",
        ));
    }
    let expected_abi_names = summary.authoritative_abi_names().into_vec();
    let actual_abi_names = abi
        .lookups()
        .iter()
        .map(|lookup| lookup.canonical_name().to_owned())
        .collect::<Vec<_>>();
    if actual_abi_names != expected_abi_names {
        return Err(OwnerBodyInferenceError::new(
            "owner body inference ABI does not match its exact callable lookup set",
        ));
    }
    let expected_value_paths = summary.authoritative_value_abi_paths().into_vec();
    let actual_value_paths = abi
        .value_lookups()
        .iter()
        .map(|lookup| lookup.canonical_path().to_owned())
        .collect::<Vec<_>>();
    if actual_value_paths != expected_value_paths {
        return Err(OwnerBodyInferenceError::new(
            "owner body inference ABI does not match its exact external value lookup set",
        ));
    }
    let expected_source_payload_paths = seed.source_payload_abi_paths().into_vec();
    let actual_source_payload_paths = abi
        .source_payload_lookups()
        .iter()
        .map(|lookup| lookup.canonical_path().to_owned())
        .collect::<Vec<_>>();
    if actual_source_payload_paths != expected_source_payload_paths {
        return Err(OwnerBodyInferenceError::new(
            "owner body inference ABI does not match its exact source payload lookup set",
        ));
    }
    for query in &seed.source_payload_queries {
        if abi
            .source_payload_lookup(&query.canonical_path)
            .and_then(crate::OwnerSourcePayloadAbiLookup::payload_type)
            .is_none()
        {
            return Err(OwnerBodyInferenceError::new(format!(
                "source `{}` has no unique payload ABI contract",
                query.canonical_path
            )));
        }
    }
    let expected_parameter_requirement_keys = seed.parameter_requirement_keys().into_vec();
    let actual_parameter_requirement_keys = abi
        .parameter_requirement_lookups()
        .iter()
        .map(|lookup| lookup.key().clone())
        .collect::<Vec<_>>();
    if actual_parameter_requirement_keys != expected_parameter_requirement_keys {
        return Err(OwnerBodyInferenceError::new(
            "owner body inference ABI does not match its exact parameter requirement lookup set",
        ));
    }
    let mut supplied_keys = BTreeSet::new();
    let mut supplied_results = Vec::new();
    let mut available_interfaces = BTreeMap::new();
    for result in std::iter::once(own_scc).chain(imported_sccs) {
        if !supplied_keys.insert(result.key.clone()) {
            return Err(OwnerBodyInferenceError::new(format!(
                "owner body inference received duplicate interface SCC {:?}",
                result.key
            )));
        }
        for interface in &result.owners {
            insert_interface(&mut available_interfaces, interface)?;
        }
        supplied_results.push(result);
    }
    let required = owner_body_required_interface_owners(seed, summary, &available_interfaces)?;
    let mut interfaces = BTreeMap::new();
    let mut providers = BTreeMap::new();
    let mut frozen_results = Vec::new();
    for result in supplied_results {
        let frozen = frozen_scc_ref(result, &required)?;
        for owner in &frozen.referenced_owners {
            let interface = result.owner(owner).ok_or_else(|| {
                OwnerBodyInferenceError::new(format!(
                    "interface SCC {:?} does not publish its member {owner:?}",
                    result.key
                ))
            })?;
            insert_interface(&mut interfaces, interface)?;
            if providers.insert(owner.clone(), result).is_some() {
                return Err(OwnerBodyInferenceError::new(format!(
                    "owner body inference received multiple provider SCCs for {owner:?}"
                )));
            }
        }
        frozen_results.push(frozen);
    }
    if interfaces.keys().cloned().collect::<BTreeSet<_>>() != required {
        return Err(OwnerBodyInferenceError::new(format!(
            "owner body inference {:?} did not receive its exact interface import set",
            seed.owner
        )));
    }
    let own_interface = interfaces[&seed.owner];
    let own_scc_index = frozen_results
        .iter()
        .position(|frozen| frozen.key == own_scc.key)
        .expect("validated own SCC is present exactly once");
    let own_scc_ref = frozen_results.remove(own_scc_index);
    frozen_results.sort_by(|left, right| left.key.cmp(&right.key));
    let inference_abi_fingerprint_v1 = abi.fingerprint_v1();
    let basis = OwnerBodyInferenceBasis {
        owner: seed.owner.clone(),
        syntax_fingerprint_v1: syntax.fingerprint_v1(),
        seed_fingerprint_v1: seed.fingerprint_v1(),
        summary_fingerprint_v1: summary.fingerprint_v1(),
        own_scc: own_scc_ref,
        imports: frozen_results.into_boxed_slice(),
        inference_abi_fingerprint_v1,
    };
    let mut interface_imports = interfaces
        .values()
        .map(|interface| {
            let provider = providers[&interface.owner];
            Ok(OwnerBodyInterfaceImport {
                owner: interface.owner.clone(),
                interface_fingerprint_v1: owner_body_interface_fingerprint_v1(interface)?,
                provider_scc: provider.key.clone(),
                provider_fingerprint_v1: provider.fingerprint_v1(),
                provider_type_variable_count: provider.type_variable_count,
            })
        })
        .collect::<Result<Vec<_>, OwnerBodyInferenceError>>()?;
    interface_imports.sort_by(|left, right| left.owner.cmp(&right.owner));

    let mut work = OwnerBodyInferenceWork {
        statements: syntax.statements.len() as u64,
        expressions: syntax.expressions.len() as u64,
        ..OwnerBodyInferenceWork::default()
    };
    let mut unifier = TypeUnifier::default();
    let expressions = (0..seed.expressions.len())
        .map(|_| unifier.fresh())
        .collect::<Vec<_>>();
    let expression_flushes = (0..seed.expressions.len())
        .map(|_| unifier.fresh())
        .collect::<Vec<_>>();
    let external_expressions = (0..seed.external_expressions.len())
        .map(|_| unifier.fresh())
        .collect::<Vec<_>>();
    let external_expression_flushes = (0..seed.external_expressions.len())
        .map(|_| unifier.fresh())
        .collect::<Vec<_>>();
    let call_flushes = (0..seed.expressions.len())
        .map(|_| unifier.fresh())
        .collect::<Vec<_>>();

    for ((external, variable), flush_variable) in seed
        .external_expressions
        .iter()
        .zip(&external_expressions)
        .zip(&external_expression_flushes)
    {
        let interface = interfaces[&external.owner];
        let mut variables = BTreeMap::new();
        let ty = instantiate_type(&interface.result.ty, &mut unifier, &mut variables);
        unifier.bind_var(*variable, ty);
        let flush_type = interface
            .result_flush_type
            .as_ref()
            .map_or(Type::Absent, |ty| {
                instantiate_type(ty, &mut unifier, &mut variables)
            });
        unifier.bind_var(*flush_variable, flush_type);
        work.interface_imports = work.interface_imports.saturating_add(1);
    }
    for (index, plan) in seed.expression_flush_plans.iter().enumerate() {
        let mut candidates = Vec::new();
        candidates.extend(
            plan.value_inputs
                .iter()
                .filter_map(|input| {
                    expression_variable(&expressions, &external_expressions, *input)
                })
                .map(Type::Var),
        );
        candidates.extend(
            plan.escape_inputs
                .iter()
                .filter_map(|input| {
                    expression_variable(&expression_flushes, &external_expression_flushes, *input)
                })
                .map(Type::Var),
        );
        if matches!(
            seed.expressions[index].kind,
            OwnerConstraintNodeKind::Call { .. } | OwnerConstraintNodeKind::Pipe { .. }
        ) {
            candidates.push(Type::Var(call_flushes[index]));
        } else {
            unifier.bind_var(call_flushes[index], Type::Absent);
        }
        unifier.bind_var(
            expression_flushes[index],
            if candidates.is_empty() {
                Type::Absent
            } else {
                boon_checked::canonical_union_type(candidates)
            },
        );
    }
    let mut local_roots = BTreeMap::new();
    let mut own_variables = BTreeMap::new();
    let mut own_parameter_variables = Vec::with_capacity(own_interface.parameters.len());
    for parameter in &own_interface.parameters {
        let variable = unifier.fresh();
        let ty = instantiate_type(&parameter.flow_type.ty, &mut unifier, &mut own_variables);
        unifier.bind_var(variable, ty);
        add_local_root(&mut local_roots, parameter.name.clone(), variable);
        own_parameter_variables.push(variable);
    }
    let context = own_interface.context.as_ref().map(|context| {
        let variable = unifier.fresh();
        let ty = instantiate_type(&context.flow_type.ty, &mut unifier, &mut own_variables);
        unifier.bind_var(variable, ty);
        variable
    });
    let own_result = instantiate_type(&own_interface.result.ty, &mut unifier, &mut own_variables);

    for declaration in &seed.declarations {
        let Some((_, expression)) = seed
            .statement_values
            .iter()
            .find(|(statement, _)| *statement == declaration.statement)
        else {
            continue;
        };
        let Some(variable) = body_expression_boundary_variable(
            &expressions,
            &external_expressions,
            &expression_flushes,
            &external_expression_flushes,
            *expression,
            &mut unifier,
        ) else {
            continue;
        };
        for name in &declaration.names {
            add_local_root(&mut local_roots, name.clone(), variable);
        }
    }
    for expression in &seed.expressions {
        if !matches!(expression.kind, OwnerConstraintNodeKind::Block) {
            continue;
        }
        for input in &expression.inputs {
            let OwnerConstraintEdgeRole::BlockBinding { name } = &input.role else {
                continue;
            };
            if let Some(variable) = body_expression_boundary_variable(
                &expressions,
                &external_expressions,
                &expression_flushes,
                &external_expression_flushes,
                input.expression,
                &mut unifier,
            ) {
                add_local_root(&mut local_roots, name.clone(), variable);
            }
        }
    }

    let mut modes = vec![None; expressions.len()];
    let mut direct_effects = vec![CheckedEffectSummary::default(); expressions.len()];
    let mut calls = Vec::new();
    bind_local_constraints(
        seed,
        summary,
        abi,
        &mut unifier,
        &expressions,
        &external_expressions,
        &local_roots,
        context,
        &mut modes,
        &mut direct_effects,
        &mut calls,
        &mut work,
    );

    let expression_by_key = seed
        .expressions
        .iter()
        .enumerate()
        .map(|(index, expression)| (expression.expression.clone(), index))
        .collect::<BTreeMap<_, _>>();
    for resolved in &summary.resolved_references {
        if resolved.reference.kind != OwnerReferenceKind::Value {
            continue;
        }
        let Some(index) = expression_by_key
            .get(&resolved.reference.expression)
            .copied()
        else {
            continue;
        };
        let interface = interfaces[&resolved.owner];
        let mut variables = BTreeMap::new();
        let ty = instantiate_type(&interface.result.ty, &mut unifier, &mut variables);
        let result = unifier.fresh();
        unifier.bind_var(result, ty);
        let result = bind_projection(&mut unifier, result, &resolved.projection);
        unifier.unify(Type::Var(expressions[index]), Type::Var(result));
        modes[index] = flow_mode_join(modes[index], Some(interface.result.mode));
        work.interface_imports = work.interface_imports.saturating_add(1);
    }
    let pre_call_actual_types = expressions
        .iter()
        .chain(&external_expressions)
        .map(|variable| unifier.resolve(&Type::Var(*variable)))
        .collect::<Vec<_>>();

    let mut diagnostics = Vec::new();
    push_invalid_syntax_diagnostics(seed, &mut diagnostics);
    push_external_value_diagnostics(summary, abi, &mut diagnostics);
    let mut call_drafts = bind_calls(
        calls,
        seed,
        &interfaces,
        &mut unifier,
        &expressions,
        &external_expressions,
        &call_flushes,
        &mut modes,
        &mut direct_effects,
        abi,
        &pre_call_actual_types,
        context.is_some(),
        own_interface.declaration_kind == Some(crate::OwnerDeclarationKind::Function),
        &mut diagnostics,
        &mut work,
    );
    refine_owner_call_transfers(
        &mut call_drafts,
        syntax,
        seed,
        &interfaces,
        &mut unifier,
        &expressions,
        &external_expressions,
        context,
        &mut modes,
    );
    validate_owner_call_types(
        &mut call_drafts,
        &interfaces,
        abi,
        &mut unifier,
        &expressions,
        &external_expressions,
        context,
        &mut diagnostics,
    );

    let mut alpha_variables = BTreeMap::new();
    let mut next_alpha = 0;
    for variable in own_parameter_variables {
        let _ = alpha_normalize_type(
            &unifier.resolve(&Type::Var(variable)),
            &mut alpha_variables,
            &mut next_alpha,
        );
    }
    let _ = alpha_normalize_type(
        &unifier.resolve(&own_result),
        &mut alpha_variables,
        &mut next_alpha,
    );
    if let Some(context) = context {
        let _ = alpha_normalize_type(
            &unifier.resolve(&Type::Var(context)),
            &mut alpha_variables,
            &mut next_alpha,
        );
    }
    let inferred_flows = syntax
        .expressions
        .iter()
        .enumerate()
        .map(|(index, _)| FlowType {
            mode: modes[index].unwrap_or(FlowMode::Continuous),
            ty: alpha_normalize_type(
                &unifier.resolve(&Type::Var(expressions[index])),
                &mut alpha_variables,
                &mut next_alpha,
            ),
        })
        .collect::<Vec<_>>();
    let normalized_flush_types = expression_flushes
        .iter()
        .chain(&external_expression_flushes)
        .map(|variable| {
            let ty = unifier.resolve(&Type::Var(*variable));
            (!matches!(
                ty,
                Type::Unknown | Type::UnresolvedShape { .. } | Type::Absent
            ))
            .then(|| alpha_normalize_type(&ty, &mut alpha_variables, &mut next_alpha))
        })
        .collect::<Vec<_>>();
    let flush_types = infer_owner_expression_flush_types(
        syntax,
        &inferred_flows,
        &normalized_flush_types,
        &mut diagnostics,
    )?;
    let inferred_expressions = syntax
        .expressions
        .iter()
        .enumerate()
        .map(|(index, expression)| {
            Ok(InferredOwnerExpression {
                id: OwnerInferenceExpressionId(checked_u32(index, "inferred expression id")?),
                stable_key: expression.stable_key.clone(),
                flow_type: inferred_flows[index].clone(),
                flush_type: flush_types[index].clone(),
                direct_effect: direct_effects[index],
                kind: expression.kind.clone(),
            })
        })
        .collect::<Result<Vec<_>, OwnerBodyInferenceError>>()?;
    let inferred_calls = call_drafts
        .into_iter()
        .map(|draft| {
            Ok(InferredOwnerCall {
                expression: draft.plan.stable_expression,
                function: draft.plan.function,
                target: draft.target,
                inputs: draft
                    .plan
                    .inputs
                    .into_vec()
                    .into_iter()
                    .map(|(role, expression)| {
                        Ok(InferredOwnerCallInput {
                            role,
                            expression: inferred_expression_ref(syntax, expression)?,
                        })
                    })
                    .collect::<Result<Vec<_>, OwnerBodyInferenceError>>()?
                    .into_boxed_slice(),
                type_substitutions: draft
                    .type_substitutions
                    .into_iter()
                    .map(|(variable, value)| CheckedTypeSubstitution {
                        variable,
                        value: alpha_normalize_type(
                            &unifier.resolve(&value),
                            &mut alpha_variables,
                            &mut next_alpha,
                        ),
                    })
                    .collect::<Vec<_>>()
                    .into_boxed_slice(),
                contextual_type_variables: draft.contextual_type_variables.into_boxed_slice(),
                syntax_discriminated_result: draft.syntax_discriminated_result,
                valid: draft.valid,
                result: inferred_expressions[draft.plan.expression]
                    .flow_type
                    .clone(),
                effect: draft.effect,
            })
        })
        .collect::<Result<Vec<_>, OwnerBodyInferenceError>>()?;
    let inferred_statements = syntax
        .statements
        .iter()
        .map(|statement| InferredOwnerStatement {
            id: OwnerInferenceStatementId(statement.id),
            stable_key: statement.stable_key.clone(),
            parent: statement.parent.map(OwnerInferenceStatementId),
            child_index: statement.child_index,
            kind: statement.kind.clone(),
            expression: statement.expression.map(OwnerInferenceExpressionId),
        })
        .collect::<Vec<_>>();
    let inferred_children = syntax
        .child_owners
        .iter()
        .map(|child| InferredOwnerChild {
            owner: child.owner.clone(),
            parent: child.parent.map(OwnerInferenceStatementId),
            child_index: child.child_index,
        })
        .collect::<Vec<_>>();
    let relocations = collect_relocations(seed, summary);

    let local_content_digest_v1 = fingerprint(
        OWNER_BODY_INFERENCE_CONTENT_DOMAIN_V1,
        &(
            &inferred_statements,
            &inferred_children,
            &inferred_expressions,
            &inferred_calls,
            &relocations,
            &diagnostics,
            own_interface.effect,
        ),
    )?;
    let receipt = OwnerBodyInferenceReceipt {
        statement_rows: checked_u32(inferred_statements.len(), "inferred statement row count")?,
        child_rows: checked_u32(inferred_children.len(), "inferred child-owner row count")?,
        expression_rows: checked_u32(inferred_expressions.len(), "inferred expression row count")?,
        call_rows: checked_u32(inferred_calls.len(), "inferred call row count")?,
        relocation_rows: checked_u32(relocations.len(), "inferred relocation row count")?,
        diagnostic_rows: checked_u32(diagnostics.len(), "inferred diagnostic row count")?,
        local_content_digest_v1,
    };
    let fingerprint_v1 = fingerprint(
        OWNER_BODY_INFERENCE_DOMAIN_V1,
        &(
            &seed.owner,
            &inferred_statements,
            &inferred_children,
            &inferred_expressions,
            &inferred_calls,
            &relocations,
            &diagnostics,
            own_interface.effect,
            &receipt,
        ),
    )?;
    work.unification_steps = unifier.steps();
    let result = Arc::new(OwnerBodyInferenceShard {
        owner: seed.owner.clone(),
        statements: inferred_statements.into_boxed_slice(),
        children: inferred_children.into_boxed_slice(),
        expressions: inferred_expressions.into_boxed_slice(),
        calls: inferred_calls.into_boxed_slice(),
        relocations,
        diagnostics: diagnostics.into_boxed_slice(),
        effect: own_interface.effect,
        receipt,
        work,
        fingerprint_v1,
    });
    let currentness = OwnerBodyInferenceCurrentnessReceipt::from_current_evaluation(
        basis,
        interface_imports.into_boxed_slice(),
        &result,
    )?;
    Ok(OwnerBodyInferenceEvaluation {
        currentness,
        result,
    })
}

/// Direct convenience projection for callers that do not retain evaluator
/// currentness. Persistent request graphs should publish evaluation and
/// semantic body as separate request families.
pub fn infer_owner_body<'a>(
    syntax: &OwnerSyntaxInput,
    seed: &OwnerConstraintSeed,
    summary: &OwnerConstraintSummary,
    abi: &OwnerInferenceAbiEnvironment,
    own_scc: &'a OwnerInterfaceSccResult,
    imported_sccs: impl IntoIterator<Item = &'a OwnerInterfaceSccResult>,
) -> Result<OwnerBodyInferenceShard, OwnerBodyInferenceError> {
    evaluate_owner_body(syntax, seed, summary, abi, own_scc, imported_sccs)
        .map(|evaluation| Arc::unwrap_or_clone(evaluation.result))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ResolvedOwnerSymbolReference, build_owner_interface_topology,
        project_owner_constraint_seed, project_owner_source_map, project_owner_syntax_input,
        resolve_owner_constraint_seed, solve_owner_interface_scc,
    };
    use boon_parser::{
        ProjectSyntaxSnapshot, UnitSyntaxSnapshot, parse_project_source_unit,
        project_unit_link_keys,
    };
    use std::sync::Arc;

    fn link(source: &str) -> UnitSyntaxSnapshot {
        let parsed = parse_project_source_unit("app/RUN.bn", source).unwrap();
        let key = project_unit_link_keys(
            "app/RUN.bn",
            [(
                parsed.source_unit_id.clone(),
                parsed.declared_functions.clone(),
            )],
        )
        .unwrap()
        .remove(&parsed.source_unit_id)
        .unwrap();
        parsed.into_unit_syntax_snapshot(key).unwrap()
    }

    fn owner_named(unit: &UnitSyntaxSnapshot, name: &str) -> StableCheckOwnerKey {
        unit.stable_check_owner_keys()
            .find(|owner| {
                matches!(
                    owner,
                    StableCheckOwnerKey::Item(owner)
                        if owner.item_route.segments().last().is_some_and(|segment| segment.names == [name])
                )
            })
            .unwrap()
    }

    fn inputs(
        unit: &UnitSyntaxSnapshot,
        owner: &StableCheckOwnerKey,
    ) -> (OwnerSyntaxInput, OwnerSourceMap, OwnerConstraintSeed) {
        let view = unit.owner_view_for_key(owner).unwrap();
        let syntax = project_owner_syntax_input(view).unwrap();
        let source_map = project_owner_source_map(unit.owner_view_for_key(owner).unwrap()).unwrap();
        let seed = project_owner_constraint_seed(&syntax).unwrap();
        (syntax, source_map, seed)
    }

    fn test_abi() -> crate::OwnerAbiEnvironment {
        let unit = link("value: 1\n");
        let project =
            ProjectSyntaxSnapshot::from_unit_snapshots("app/RUN.bn", vec![Arc::new(unit)]).unwrap();
        crate::project_owner_abi_environment(
            &project,
            &boon_checked::ExternalTypeEnvironment::empty(boon_checked::ProgramRole::Client),
        )
        .unwrap()
    }

    fn parameter_requirement_lookups<'a>(
        abi: &crate::OwnerAbiEnvironment,
        seeds: impl IntoIterator<Item = &'a OwnerConstraintSeed>,
    ) -> Vec<crate::OwnerParameterRequirementLookup> {
        seeds
            .into_iter()
            .flat_map(|seed| {
                seed.parameter_requirement_keys()
                    .into_vec()
                    .into_iter()
                    .map(|key| {
                        let (function, parameter) = seed
                            .parameter_requirement_names(key.parameter_ordinal())
                            .unwrap();
                        abi.parameter_requirement_lookup(key, function, parameter)
                            .unwrap()
                    })
            })
            .collect()
    }

    fn solve(
        seeds: &[OwnerConstraintSeed],
        summaries: &[OwnerConstraintSummary],
    ) -> Vec<crate::OwnerInterfaceSccResult> {
        let abi_provider = test_abi();
        solve_with_abi(seeds, summaries, &abi_provider)
    }

    fn solve_with_abi(
        seeds: &[OwnerConstraintSeed],
        summaries: &[OwnerConstraintSummary],
        abi_provider: &crate::OwnerAbiEnvironment,
    ) -> Vec<crate::OwnerInterfaceSccResult> {
        let topology = build_owner_interface_topology(summaries.iter()).unwrap();
        let seeds = seeds
            .iter()
            .map(|seed| (seed.owner.clone(), seed))
            .collect::<BTreeMap<_, _>>();
        let summaries = summaries
            .iter()
            .map(|summary| (summary.owner.clone(), summary))
            .collect::<BTreeMap<_, _>>();
        let mut results = BTreeMap::new();
        for scc in &topology.sccs {
            let requirements = parameter_requirement_lookups(
                abi_provider,
                scc.key.members.iter().map(|owner| seeds[owner]),
            );
            let abi = abi_provider
                .complete_inference_environment_with_requirements(
                    scc.key.members.iter().cloned(),
                    scc.key
                        .members
                        .iter()
                        .flat_map(|owner| summaries[owner].authoritative_abi_names().into_vec()),
                    scc.key.members.iter().flat_map(|owner| {
                        summaries[owner].authoritative_value_abi_paths().into_vec()
                    }),
                    scc.key
                        .members
                        .iter()
                        .flat_map(|owner| seeds[owner].source_payload_abi_paths().into_vec()),
                    requirements,
                )
                .unwrap();
            let dependencies = scc
                .dependencies
                .iter()
                .map(|dependency| results.get(dependency).unwrap())
                .collect::<Vec<_>>();
            let result = solve_owner_interface_scc(
                scc,
                &abi,
                scc.key.members.iter().map(|owner| seeds[owner]),
                scc.key.members.iter().map(|owner| summaries[owner]),
                dependencies,
            )
            .unwrap();
            results.insert(scc.key.clone(), result);
        }
        topology
            .sccs
            .iter()
            .map(|scc| results.remove(&scc.key).unwrap())
            .collect()
    }

    fn infer(
        syntax: &OwnerSyntaxInput,
        seed: &OwnerConstraintSeed,
        summary: &OwnerConstraintSummary,
        results: &[OwnerInterfaceSccResult],
    ) -> OwnerBodyInferenceShard {
        let abi_provider = test_abi();
        infer_with_abi(syntax, seed, summary, results, &abi_provider)
    }

    fn infer_with_abi(
        syntax: &OwnerSyntaxInput,
        seed: &OwnerConstraintSeed,
        summary: &OwnerConstraintSummary,
        results: &[OwnerInterfaceSccResult],
        abi_provider: &crate::OwnerAbiEnvironment,
    ) -> OwnerBodyInferenceShard {
        let requirements = parameter_requirement_lookups(abi_provider, [seed]);
        let abi = abi_provider
            .complete_inference_environment_with_requirements(
                [seed.owner.clone()],
                summary.authoritative_abi_names().into_vec(),
                summary.authoritative_value_abi_paths().into_vec(),
                seed.source_payload_abi_paths().into_vec(),
                requirements,
            )
            .unwrap();
        let own_scc = results
            .iter()
            .find(|result| result.key.members.contains(&seed.owner))
            .unwrap();
        infer_owner_body(
            syntax,
            seed,
            summary,
            &abi,
            own_scc,
            results.iter().filter(|result| result.key != own_scc.key),
        )
        .unwrap()
    }

    #[test]
    fn literal_body_changes_while_formatting_only_changes_the_source_map() {
        let original = link("value: Number/to_text(value: 1)\n");
        let formatted = link("value:   Number/to_text(value: 1)\n");
        let changed = link("value: Number/to_text(value: 2)\n");
        let original_owner = owner_named(&original, "value");
        let formatted_owner = owner_named(&formatted, "value");
        let changed_owner = owner_named(&changed, "value");
        assert_eq!(original_owner, formatted_owner);
        assert_eq!(original_owner, changed_owner);

        let (original_syntax, original_map, original_seed) = inputs(&original, &original_owner);
        let (formatted_syntax, formatted_map, formatted_seed) =
            inputs(&formatted, &formatted_owner);
        let (changed_syntax, _, changed_seed) = inputs(&changed, &changed_owner);
        assert_eq!(
            original_syntax.fingerprint_v1(),
            formatted_syntax.fingerprint_v1()
        );
        assert_ne!(
            original_map.fingerprint_v2(),
            formatted_map.fingerprint_v2()
        );
        assert_ne!(
            original_syntax.fingerprint_v1(),
            changed_syntax.fingerprint_v1()
        );

        let original_summary = resolve_owner_constraint_seed(&original_seed, []).unwrap();
        let formatted_summary = resolve_owner_constraint_seed(&formatted_seed, []).unwrap();
        let changed_summary = resolve_owner_constraint_seed(&changed_seed, []).unwrap();
        let original_interface = solve(&[original_seed.clone()], &[original_summary.clone()]);
        let formatted_interface = solve(&[formatted_seed.clone()], &[formatted_summary.clone()]);
        let changed_interface = solve(&[changed_seed.clone()], &[changed_summary.clone()]);
        assert_eq!(
            original_interface[0].fingerprint_v1(),
            changed_interface[0].fingerprint_v1()
        );

        let original_body = infer(
            &original_syntax,
            &original_seed,
            &original_summary,
            &original_interface,
        );
        let formatted_body = infer(
            &formatted_syntax,
            &formatted_seed,
            &formatted_summary,
            &formatted_interface,
        );
        let changed_body = infer(
            &changed_syntax,
            &changed_seed,
            &changed_summary,
            &changed_interface,
        );
        assert_eq!(
            original_body.fingerprint_v1(),
            formatted_body.fingerprint_v1()
        );
        assert_ne!(
            original_body.fingerprint_v1(),
            changed_body.fingerprint_v1()
        );
        assert_eq!(
            original_body.expressions.last().unwrap().flow_type.ty,
            Type::Text
        );
    }

    #[test]
    fn resolved_call_uses_frozen_interfaces_and_emits_a_stable_relocation() {
        let unit = link(
            "FUNCTION zed(input) {\n    Number/to_text(value: input)\n}\nalpha: zed(input: 1)\n",
        );
        let alpha = owner_named(&unit, "alpha");
        let zed = owner_named(&unit, "zed");
        let (alpha_syntax, _, alpha_seed) = inputs(&unit, &alpha);
        let (_, _, zed_seed) = inputs(&unit, &zed);
        let callable_reference = alpha_seed
            .references
            .iter()
            .find(|reference| reference.kind == OwnerReferenceKind::Callable)
            .cloned()
            .unwrap();
        let parameters = zed_seed
            .declarations
            .iter()
            .find(|declaration| declaration.public)
            .unwrap()
            .parameters
            .clone();
        let alpha_summary = resolve_owner_constraint_seed(
            &alpha_seed,
            [ResolvedOwnerSymbolReference {
                reference: callable_reference,
                owner: zed.clone(),
                projection: Box::new([]),
                parameters,
            }],
        )
        .unwrap();
        let zed_summary = resolve_owner_constraint_seed(&zed_seed, []).unwrap();
        let interfaces = solve(
            &[alpha_seed.clone(), zed_seed],
            &[alpha_summary.clone(), zed_summary],
        );
        let body = infer(&alpha_syntax, &alpha_seed, &alpha_summary, &interfaces);
        assert_eq!(body.calls.len(), 1);
        assert_eq!(body.calls[0].result.ty, Type::Text);
        assert_eq!(
            body.calls[0].target,
            InferredOwnerCallableTarget::Owner { owner: zed.clone() }
        );
        assert!(body.relocations.iter().any(|relocation| {
            relocation.kind == OwnerBodyRelocationKind::Callable && relocation.target_owner == zed
        }));
    }

    #[test]
    fn generic_call_transfer_matches_the_whole_checker_oracle() {
        let source = "FUNCTION identity(input) {\n    input\n}\nvalue: identity(input: 1)\n";
        let unit = link(source);
        let value = owner_named(&unit, "value");
        let identity = owner_named(&unit, "identity");
        let (value_syntax, _, value_seed) = inputs(&unit, &value);
        let (_, _, identity_seed) = inputs(&unit, &identity);
        let reference = value_seed
            .references
            .iter()
            .find(|reference| reference.kind == OwnerReferenceKind::Callable)
            .cloned()
            .unwrap();
        let parameters = identity_seed
            .declarations
            .iter()
            .find(|declaration| declaration.public)
            .unwrap()
            .parameters
            .clone();
        let value_summary = resolve_owner_constraint_seed(
            &value_seed,
            [ResolvedOwnerSymbolReference {
                reference,
                owner: identity,
                projection: Box::new([]),
                parameters,
            }],
        )
        .unwrap();
        let identity_summary = resolve_owner_constraint_seed(&identity_seed, []).unwrap();
        let interfaces = solve(
            &[value_seed.clone(), identity_seed],
            &[value_summary.clone(), identity_summary],
        );
        let body = infer(&value_syntax, &value_seed, &value_summary, &interfaces);
        let call = &body.calls[0];

        let parsed = boon_parser::parse_project(
            "app/RUN.bn",
            [("app/RUN.bn".to_owned(), source.to_owned())],
        )
        .unwrap();
        let checked = crate::check_program(&parsed);
        assert!(checked.report.diagnostics.is_empty());
        let oracle = checked
            .program
            .as_ref()
            .unwrap()
            .calls
            .iter()
            .find(|call| call.function == "identity")
            .unwrap();

        assert_eq!(call.result, oracle.result);
        assert_eq!(call.type_substitutions.len(), 1);
        assert_eq!(call.type_substitutions[0].variable, TypeVar(0));
        assert_eq!(
            call.type_substitutions[0].value,
            oracle.type_substitutions[0].value
        );
        assert_eq!(
            call.syntax_discriminated_result,
            oracle.syntax_discriminated_result
        );
    }

    #[test]
    fn syntax_selected_call_transfer_matches_the_whole_checker_oracle() {
        let source = "FUNCTION choose(kind) {\n    kind |> WHEN {\n        Record => [value: 1]\n        __ => LIST { 1 }\n    }\n}\nvalue: choose(kind: Record)\n";
        let unit = link(source);
        let value = owner_named(&unit, "value");
        let choose = owner_named(&unit, "choose");
        let (value_syntax, _, value_seed) = inputs(&unit, &value);
        let (_, _, choose_seed) = inputs(&unit, &choose);
        let reference = value_seed
            .references
            .iter()
            .find(|reference| reference.kind == OwnerReferenceKind::Callable)
            .cloned()
            .unwrap();
        let parameters = choose_seed
            .declarations
            .iter()
            .find(|declaration| declaration.public)
            .unwrap()
            .parameters
            .clone();
        let value_summary = resolve_owner_constraint_seed(
            &value_seed,
            [ResolvedOwnerSymbolReference {
                reference,
                owner: choose.clone(),
                projection: Box::new([]),
                parameters,
            }],
        )
        .unwrap();
        let choose_summary = resolve_owner_constraint_seed(&choose_seed, []).unwrap();
        let interfaces = solve(
            &[value_seed.clone(), choose_seed],
            &[value_summary.clone(), choose_summary],
        );
        let body = infer(&value_syntax, &value_seed, &value_summary, &interfaces);
        let call = &body.calls[0];

        let parsed = boon_parser::parse_project(
            "app/RUN.bn",
            [("app/RUN.bn".to_owned(), source.to_owned())],
        )
        .unwrap();
        let checked = crate::check_program(&parsed);
        assert!(
            checked.report.diagnostics.is_empty(),
            "syntax oracle diagnostics: {:#?}",
            checked.report.diagnostics
        );
        let oracle = checked
            .program
            .as_ref()
            .unwrap()
            .calls
            .iter()
            .find(|call| call.function == "choose")
            .unwrap();

        assert_eq!(
            call.result,
            oracle.result,
            "choose transfer: {:#?}",
            interfaces
                .iter()
                .find_map(|result| result.owner(&choose))
                .unwrap()
                .result_transfer
        );
        assert_eq!(
            call.syntax_discriminated_result,
            oracle.syntax_discriminated_result
        );
        assert!(call.syntax_discriminated_result);
    }

    #[test]
    fn passed_context_call_transfer_resolves_the_legacy_open_result() {
        let source = concat!(
            "FUNCTION leaf() {\n",
            "    PASSED.store.count\n",
            "}\n",
            "value: leaf(PASS: [store: [count: 1]])\n",
        );
        let unit = link(source);
        let value = owner_named(&unit, "value");
        let leaf = owner_named(&unit, "leaf");
        let (value_syntax, _, value_seed) = inputs(&unit, &value);
        let (_, _, leaf_seed) = inputs(&unit, &leaf);
        let reference = value_seed
            .references
            .iter()
            .find(|reference| reference.kind == OwnerReferenceKind::Callable)
            .cloned()
            .unwrap();
        let value_summary = resolve_owner_constraint_seed(
            &value_seed,
            [ResolvedOwnerSymbolReference {
                reference,
                owner: leaf.clone(),
                projection: Box::new([]),
                parameters: Box::new([]),
            }],
        )
        .unwrap();
        let leaf_summary = resolve_owner_constraint_seed(&leaf_seed, []).unwrap();
        let interfaces = solve(
            &[value_seed.clone(), leaf_seed],
            &[value_summary.clone(), leaf_summary],
        );
        let body = infer(&value_syntax, &value_seed, &value_summary, &interfaces);
        let call = &body.calls[0];

        let parsed = boon_parser::parse_project(
            "app/RUN.bn",
            [("app/RUN.bn".to_owned(), source.to_owned())],
        )
        .unwrap();
        let checked = crate::check_program(&parsed);
        assert!(checked.report.diagnostics.is_empty());
        let oracle = checked
            .program
            .as_ref()
            .unwrap()
            .calls
            .iter()
            .find(|call| call.function == "leaf")
            .unwrap();

        assert_eq!(
            call.result,
            FlowType {
                mode: FlowMode::Continuous,
                ty: Type::Number,
            }
        );
        assert!(!call.contextual_type_variables.is_empty());
        assert_eq!(
            call.type_substitutions
                .iter()
                .filter(|substitution| {
                    call.contextual_type_variables
                        .contains(&substitution.variable)
                })
                .map(|substitution| &substitution.value)
                .collect::<Vec<_>>(),
            vec![&Type::Number]
        );
        // The whole-project oracle never applied the explicit PASSED value to
        // this call occurrence. The owner-local replacement deliberately
        // closes that legacy hole instead of preserving an unresolved result.
        assert!(matches!(oracle.result.ty, Type::Var(_)));
        assert!(oracle.type_substitutions.is_empty());
        assert!(oracle.contextual_substitutions.is_empty());
    }

    #[test]
    fn user_argument_and_pass_diagnostics_use_pre_contract_actual_types() {
        let source = concat!(
            "FUNCTION needs(input) {\n",
            "    input.required\n",
            "}\n",
            "value: needs(input: [other: 1])\n",
        );
        let unit = link(source);
        let value = owner_named(&unit, "value");
        let needs = owner_named(&unit, "needs");
        let (value_syntax, _, value_seed) = inputs(&unit, &value);
        let (_, _, needs_seed) = inputs(&unit, &needs);
        let reference = value_seed
            .references
            .iter()
            .find(|reference| reference.kind == OwnerReferenceKind::Callable)
            .cloned()
            .unwrap();
        let parameters = needs_seed
            .declarations
            .iter()
            .find(|declaration| declaration.public)
            .unwrap()
            .parameters
            .clone();
        let value_summary = resolve_owner_constraint_seed(
            &value_seed,
            [ResolvedOwnerSymbolReference {
                reference,
                owner: needs,
                projection: Box::new([]),
                parameters,
            }],
        )
        .unwrap();
        let needs_summary = resolve_owner_constraint_seed(&needs_seed, []).unwrap();
        let interfaces = solve(
            &[value_seed.clone(), needs_seed],
            &[value_summary.clone(), needs_summary],
        );
        let body = infer(&value_syntax, &value_seed, &value_summary, &interfaces);
        assert!(body.calls[0].valid);
        assert!(body.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "user_call_argument_type"
                && diagnostic.message.contains("missing field `required`")
        }));

        let pass_source = concat!(
            "FUNCTION leaf() {\n",
            "    PASSED.store.count + 1\n",
            "}\n",
            "value: leaf(PASS: [store: [count: TEXT { wrong }]])\n",
        );
        let pass_unit = link(pass_source);
        let pass_value = owner_named(&pass_unit, "value");
        let leaf = owner_named(&pass_unit, "leaf");
        let (pass_syntax, _, pass_seed) = inputs(&pass_unit, &pass_value);
        let (_, _, leaf_seed) = inputs(&pass_unit, &leaf);
        let reference = pass_seed
            .references
            .iter()
            .find(|reference| reference.kind == OwnerReferenceKind::Callable)
            .cloned()
            .unwrap();
        let pass_summary = resolve_owner_constraint_seed(
            &pass_seed,
            [ResolvedOwnerSymbolReference {
                reference,
                owner: leaf.clone(),
                projection: Box::new([]),
                parameters: Box::new([]),
            }],
        )
        .unwrap();
        let leaf_summary = resolve_owner_constraint_seed(&leaf_seed, []).unwrap();
        let interfaces = solve(
            &[pass_seed.clone(), leaf_seed],
            &[pass_summary.clone(), leaf_summary],
        );
        let body = infer(&pass_syntax, &pass_seed, &pass_summary, &interfaces);
        assert!(body.calls[0].valid);
        assert!(
            body.diagnostics.iter().any(|diagnostic| {
                diagnostic.code == "pass_context_type"
                    && diagnostic
                        .message
                        .contains("field `store.count` has an incompatible type")
            }),
            "{:#?}",
            body.diagnostics
        );
    }

    #[test]
    fn inherited_passed_context_transfers_through_owner_calls() {
        let source = concat!(
            "FUNCTION leaf() {\n",
            "    PASSED.store.count\n",
            "}\n",
            "FUNCTION inherited() {\n",
            "    leaf()\n",
            "}\n",
            "value: inherited(PASS: [store: [count: 1]])\n",
        );
        let unit = link(source);
        let value = owner_named(&unit, "value");
        let inherited = owner_named(&unit, "inherited");
        let leaf = owner_named(&unit, "leaf");
        let (value_syntax, _, value_seed) = inputs(&unit, &value);
        let (_, _, inherited_seed) = inputs(&unit, &inherited);
        let (_, _, leaf_seed) = inputs(&unit, &leaf);

        let inherited_reference = inherited_seed
            .references
            .iter()
            .find(|reference| reference.kind == OwnerReferenceKind::Callable)
            .cloned()
            .unwrap();
        let value_reference = value_seed
            .references
            .iter()
            .find(|reference| reference.kind == OwnerReferenceKind::Callable)
            .cloned()
            .unwrap();
        let leaf_summary = resolve_owner_constraint_seed(&leaf_seed, []).unwrap();
        let inherited_summary = resolve_owner_constraint_seed(
            &inherited_seed,
            [ResolvedOwnerSymbolReference {
                reference: inherited_reference,
                owner: leaf,
                projection: Box::new([]),
                parameters: Box::new([]),
            }],
        )
        .unwrap();
        let value_summary = resolve_owner_constraint_seed(
            &value_seed,
            [ResolvedOwnerSymbolReference {
                reference: value_reference,
                owner: inherited,
                projection: Box::new([]),
                parameters: Box::new([]),
            }],
        )
        .unwrap();
        let interfaces = solve(
            &[value_seed.clone(), inherited_seed, leaf_seed],
            &[value_summary.clone(), inherited_summary, leaf_summary],
        );
        let body = infer(&value_syntax, &value_seed, &value_summary, &interfaces);
        let call = body
            .calls
            .iter()
            .find(|call| call.function == "inherited")
            .unwrap();

        assert_eq!(
            call.result,
            FlowType {
                mode: FlowMode::Continuous,
                ty: Type::Number,
            }
        );
        assert!(!call.contextual_type_variables.is_empty());
        assert!(call.type_substitutions.iter().any(|substitution| {
            call.contextual_type_variables
                .contains(&substitution.variable)
                && substitution.value == Type::Number
        }));
    }

    #[test]
    fn callable_flush_control_survives_frozen_interface_and_occurrence_transfer() {
        let source = concat!(
            "FUNCTION leaf() {\n",
            "    FLUSH { Error }\n",
            "}\n",
            "FUNCTION wrapper() {\n",
            "    leaf()\n",
            "}\n",
        );
        let unit = link(source);
        let wrapper = owner_named(&unit, "wrapper");
        let leaf = owner_named(&unit, "leaf");
        let (wrapper_syntax, _, wrapper_seed) = inputs(&unit, &wrapper);
        let (_, _, leaf_seed) = inputs(&unit, &leaf);
        let reference = wrapper_seed
            .references
            .iter()
            .find(|reference| reference.kind == OwnerReferenceKind::Callable)
            .cloned()
            .unwrap();
        let wrapper_summary = resolve_owner_constraint_seed(
            &wrapper_seed,
            [ResolvedOwnerSymbolReference {
                reference,
                owner: leaf,
                projection: Box::new([]),
                parameters: Box::new([]),
            }],
        )
        .unwrap();
        let leaf_summary = resolve_owner_constraint_seed(&leaf_seed, []).unwrap();
        let interfaces = solve(
            &[wrapper_seed.clone(), leaf_seed],
            &[wrapper_summary.clone(), leaf_summary],
        );
        let body = infer(
            &wrapper_syntax,
            &wrapper_seed,
            &wrapper_summary,
            &interfaces,
        );
        let error = Type::VariantSet(vec![Variant::Tag("Error".to_owned())].into());
        assert_eq!(body.calls[0].result.ty, error);
        let call_expression = body
            .expressions
            .iter()
            .find(|expression| expression.stable_key == body.calls[0].expression)
            .unwrap();
        assert_eq!(call_expression.flush_type, Some(error));
    }

    #[test]
    fn hold_initializer_flush_crosses_the_enclosing_owner_boundary() {
        let unit = link("state:\n    FLUSH { Error }\n    |> HOLD held {}\n");
        let state = owner_named(&unit, "state");
        let held = owner_named(&unit, "held");
        let (_, _, state_seed) = inputs(&unit, &state);
        let (held_syntax, _, held_seed) = inputs(&unit, &held);
        let state_summary = resolve_owner_constraint_seed(&state_seed, []).unwrap();
        let held_summary = resolve_owner_constraint_seed(&held_seed, []).unwrap();
        let interfaces = solve(
            &[state_seed, held_seed.clone()],
            &[state_summary, held_summary.clone()],
        );
        let body = infer(&held_syntax, &held_seed, &held_summary, &interfaces);

        assert!(body.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "hold_initializer_flush"
                && diagnostic.message
                    == "a `HOLD` initializer must produce a valid storable value and cannot `FLUSH`"
        }));
    }

    #[test]
    fn output_scoped_call_transfer_matches_the_whole_checker_oracle() {
        let source = concat!(
            "FUNCTION sorted(list, entry: OUT, key) {\n",
            "    list |> List/sort_by(item: entry, key: key, direction: Ascending)\n",
            "}\n",
            "rows: LIST { [rank: 1] }\n",
            "ordered: rows |> sorted(entry, key: entry.rank)\n",
        );
        let unit = link(source);
        let ordered = owner_named(&unit, "ordered");
        let rows = owner_named(&unit, "rows");
        let sorted = owner_named(&unit, "sorted");
        let (ordered_syntax, _, ordered_seed) = inputs(&unit, &ordered);
        let (_, _, rows_seed) = inputs(&unit, &rows);
        let (_, _, sorted_seed) = inputs(&unit, &sorted);
        let sorted_parameters = sorted_seed
            .declarations
            .iter()
            .find(|declaration| declaration.public)
            .unwrap()
            .parameters
            .clone();
        let ordered_resolutions = ordered_seed
            .references
            .iter()
            .filter_map(
                |reference| match reference.parts.first().map(String::as_str) {
                    Some("rows") => Some(ResolvedOwnerSymbolReference {
                        reference: reference.clone(),
                        owner: rows.clone(),
                        projection: Box::new([]),
                        parameters: Box::new([]),
                    }),
                    Some("sorted") => Some(ResolvedOwnerSymbolReference {
                        reference: reference.clone(),
                        owner: sorted.clone(),
                        projection: Box::new([]),
                        parameters: sorted_parameters.clone(),
                    }),
                    _ => None,
                },
            )
            .collect::<Vec<_>>();
        let ordered_summary =
            resolve_owner_constraint_seed(&ordered_seed, ordered_resolutions).unwrap();
        let rows_summary = resolve_owner_constraint_seed(&rows_seed, []).unwrap();
        let sorted_summary = resolve_owner_constraint_seed(&sorted_seed, []).unwrap();
        let interfaces = solve(
            &[ordered_seed.clone(), rows_seed, sorted_seed],
            &[ordered_summary.clone(), rows_summary, sorted_summary],
        );
        let body = infer(
            &ordered_syntax,
            &ordered_seed,
            &ordered_summary,
            &interfaces,
        );
        let call = body
            .calls
            .iter()
            .find(|call| call.function == "sorted")
            .unwrap();

        let parsed = boon_parser::parse_project(
            "app/RUN.bn",
            [("app/RUN.bn".to_owned(), source.to_owned())],
        )
        .unwrap();
        let checked = crate::check_program(&parsed);
        assert!(
            checked.report.diagnostics.is_empty(),
            "OUT oracle diagnostics: {:#?}",
            checked.report.diagnostics
        );
        let oracle = checked
            .program
            .as_ref()
            .unwrap()
            .calls
            .iter()
            .find(|call| call.function == "sorted")
            .unwrap();

        assert_eq!(call.result, oracle.result);
        assert_eq!(
            call.type_substitutions
                .iter()
                .map(|substitution| &substitution.value)
                .collect::<Vec<_>>(),
            oracle
                .type_substitutions
                .iter()
                .map(|substitution| &substitution.value)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn static_bits_arithmetic_transfers_through_user_calls() {
        let source = concat!(
            "FUNCTION take(bits) {\n",
            "    bits |> Bits/slice(from: 1, count: 2 + 2)\n",
            "}\n",
            "value: take(bits: BITS[8] { 2u10101010 })\n",
        );
        let unit = link(source);
        let value = owner_named(&unit, "value");
        let take = owner_named(&unit, "take");
        let (value_syntax, _, value_seed) = inputs(&unit, &value);
        let (_, _, take_seed) = inputs(&unit, &take);
        let reference = value_seed
            .references
            .iter()
            .find(|reference| reference.kind == OwnerReferenceKind::Callable)
            .cloned()
            .unwrap();
        let parameters = take_seed
            .declarations
            .iter()
            .find(|declaration| declaration.public)
            .unwrap()
            .parameters
            .clone();
        let value_summary = resolve_owner_constraint_seed(
            &value_seed,
            [ResolvedOwnerSymbolReference {
                reference,
                owner: take,
                projection: Box::new([]),
                parameters,
            }],
        )
        .unwrap();
        let take_summary = resolve_owner_constraint_seed(&take_seed, []).unwrap();
        let interfaces = solve(
            &[value_seed.clone(), take_seed],
            &[value_summary.clone(), take_summary],
        );
        let body = infer(&value_syntax, &value_seed, &value_summary, &interfaces);
        let call = body
            .calls
            .iter()
            .find(|call| call.function == "take")
            .unwrap();

        let parsed = boon_parser::parse_project(
            "app/RUN.bn",
            [("app/RUN.bn".to_owned(), source.to_owned())],
        )
        .unwrap();
        let checked = crate::check_program(&parsed);
        assert!(
            checked.report.diagnostics.is_empty(),
            "Bits oracle diagnostics: {:#?}",
            checked.report.diagnostics
        );
        let oracle = checked
            .program
            .as_ref()
            .unwrap()
            .calls
            .iter()
            .find(|call| call.function == "take")
            .unwrap();

        assert_eq!(
            call.result,
            FlowType {
                mode: FlowMode::Continuous,
                ty: Type::Bits { width: 4 },
            }
        );
        assert_eq!(call.result, oracle.result);
    }

    #[test]
    fn diagnostic_templates_rematerialize_against_current_source_positions() {
        let original = link("value: mystery()\n");
        let formatted = link("value:       mystery()\n");
        let owner = owner_named(&original, "value");
        let formatted_owner = owner_named(&formatted, "value");
        let (syntax, source_map, seed) = inputs(&original, &owner);
        let (formatted_syntax, formatted_source_map, formatted_seed) =
            inputs(&formatted, &formatted_owner);
        let summary = resolve_owner_constraint_seed(&seed, []).unwrap();
        let formatted_summary = resolve_owner_constraint_seed(&formatted_seed, []).unwrap();
        let interfaces = solve(&[seed.clone()], &[summary.clone()]);
        let formatted_interfaces = solve(&[formatted_seed.clone()], &[formatted_summary.clone()]);
        let body = infer(&syntax, &seed, &summary, &interfaces);
        let formatted_body = infer(
            &formatted_syntax,
            &formatted_seed,
            &formatted_summary,
            &formatted_interfaces,
        );
        assert_eq!(body.fingerprint_v1(), formatted_body.fingerprint_v1());
        assert_eq!(body.diagnostics.len(), 1);
        let diagnostics = materialize_owner_diagnostics(&body, &source_map).unwrap();
        let formatted_diagnostics =
            materialize_owner_diagnostics(&body, &formatted_source_map).unwrap();
        assert_eq!(diagnostics[0].message, "unknown function `mystery`");
        assert_ne!(diagnostics[0].start, formatted_diagnostics[0].start);
    }

    #[test]
    fn ambiguous_callable_is_retained_without_guessing_an_interface() {
        let unit = link("left: 1\nright: 2\nvalue: mystery()\n");
        let owner = owner_named(&unit, "value");
        let left = owner_named(&unit, "left");
        let right = owner_named(&unit, "right");
        let (syntax, _, seed) = inputs(&unit, &owner);
        let reference = seed.references.first().cloned().unwrap();
        let summary = crate::resolve_owner_constraint_seed_with_resolutions(
            &seed,
            [OwnerSymbolResolution::Ambiguous {
                reference,
                candidates: vec![
                    crate::AmbiguousOwnerSymbolCandidate {
                        owner: left.clone(),
                        parameters: Box::new([]),
                    },
                    crate::AmbiguousOwnerSymbolCandidate {
                        owner: right.clone(),
                        parameters: Box::new([]),
                    },
                ]
                .into_boxed_slice(),
            }],
        )
        .unwrap();
        let interfaces = solve(&[seed.clone()], &[summary.clone()]);
        let body = infer(&syntax, &seed, &summary, &interfaces);

        assert_eq!(
            body.calls[0].target,
            InferredOwnerCallableTarget::Ambiguous {
                candidates: vec![left, right].into_boxed_slice(),
            }
        );
        assert_eq!(body.diagnostics[0].code, "ambiguous_callable");
    }

    #[test]
    fn invalid_call_shapes_keep_exact_diagnostics_and_cannot_be_published() {
        for (source, code) in [
            (
                "value: Number/to_text(radix: 10, value: 1)\n",
                "misordered_call_entry",
            ),
            (
                "value: Number/to_text(value: 1, extra: 2)\n",
                "unexpected_call_entry",
            ),
            ("value: Number/to_text()\n", "missing_call_entry"),
        ] {
            let unit = link(source);
            let owner = owner_named(&unit, "value");
            let (syntax, _, seed) = inputs(&unit, &owner);
            let summary = resolve_owner_constraint_seed(&seed, []).unwrap();
            let interfaces = solve(&[seed.clone()], &[summary.clone()]);
            let body = infer(&syntax, &seed, &summary, &interfaces);

            assert_eq!(body.calls.len(), 1, "{source}");
            assert!(!body.calls[0].valid, "{source}");
            assert!(
                body.diagnostics
                    .iter()
                    .any(|diagnostic| diagnostic.code == code),
                "{source}: {:#?}",
                body.diagnostics
            );
        }

        let source = "value: Number/to_text(value: 1, PASS: [])\n";
        let unit = link(source);
        let owner = owner_named(&unit, "value");
        let (syntax, _, seed) = inputs(&unit, &owner);
        let summary = resolve_owner_constraint_seed(&seed, []).unwrap();
        let interfaces = solve(&[seed.clone()], &[summary.clone()]);
        let body = infer(&syntax, &seed, &summary, &interfaces);
        assert!(!body.calls[0].valid);
        assert!(
            body.diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "pass_on_authoritative_callable")
        );
    }

    #[test]
    fn closed_owner_expression_types_match_the_independent_whole_checker_oracle() {
        for (source, owner_name) in [
            (
                "FUNCTION increment(input) {\n    input + 1\n}\n",
                "increment",
            ),
            ("value: Number/to_text(value: 1)\n", "value"),
            ("value: [title: \"hello\", count: 1]\n", "value"),
        ] {
            let unit = link(source);
            let owner = owner_named(&unit, owner_name);
            let (syntax, _, seed) = inputs(&unit, &owner);
            let summary = resolve_owner_constraint_seed(&seed, []).unwrap();
            let interfaces = solve(&[seed.clone()], &[summary.clone()]);
            let body = infer(&syntax, &seed, &summary, &interfaces);

            let parsed = boon_parser::parse_project(
                "app/RUN.bn",
                [("app/RUN.bn".to_owned(), source.to_owned())],
            )
            .unwrap();
            let syntax_ids = parsed
                .expressions
                .iter()
                .filter_map(|expression| {
                    parsed
                        .stable_expression_key(expression.id)
                        .map(|stable| (stable, expression.id))
                })
                .collect::<BTreeMap<_, _>>();
            let oracle = crate::check_program(&parsed);
            assert!(oracle.report.diagnostics.is_empty(), "{owner_name} oracle");
            let oracle_types = oracle
                .report
                .expr_type_table
                .entries
                .iter()
                .map(|entry| (entry.expr_id, entry.flow_type.clone()))
                .collect::<BTreeMap<_, _>>();
            for expression in &body.expressions {
                let syntax_id = syntax_ids[&expression.stable_key];
                assert_eq!(
                    expression.flow_type, oracle_types[&syntax_id],
                    "{owner_name} expression {:?}",
                    expression.stable_key,
                );
            }
        }
    }

    #[test]
    fn external_value_flow_matches_the_independent_whole_checker_oracle() {
        let source = "value: Session/store.count\n";
        let unit = link(source);
        let owner = owner_named(&unit, "value");
        let (syntax, _, seed) = inputs(&unit, &owner);
        let summary = resolve_owner_constraint_seed(&seed, []).unwrap();
        assert_eq!(
            summary.authoritative_value_abi_paths().as_ref(),
            ["Session/store.count"]
        );

        let mut external =
            boon_checked::ExternalTypeEnvironment::empty(boon_checked::ProgramRole::Client);
        external.values.insert(
            "Session/store.count".to_owned(),
            FlowType {
                mode: FlowMode::Continuous,
                ty: Type::Number,
            },
        );
        let project =
            ProjectSyntaxSnapshot::from_unit_snapshots("app/RUN.bn", vec![Arc::new(unit.clone())])
                .unwrap();
        let abi = crate::project_owner_abi_environment(&project, &external).unwrap();
        let interfaces = solve_with_abi(&[seed.clone()], &[summary.clone()], &abi);
        let body = infer_with_abi(&syntax, &seed, &summary, &interfaces, &abi);
        assert!(body.diagnostics.is_empty());
        assert_eq!(interfaces[0].owners[0].result.ty, Type::Number);
        assert_eq!(body.expressions.last().unwrap().flow_type.ty, Type::Number);

        let parsed = boon_parser::parse_project(
            "app/RUN.bn",
            [("app/RUN.bn".to_owned(), source.to_owned())],
        )
        .unwrap();
        let syntax_ids = parsed
            .expressions
            .iter()
            .filter_map(|expression| {
                parsed
                    .stable_expression_key(expression.id)
                    .map(|stable| (stable, expression.id))
            })
            .collect::<BTreeMap<_, _>>();
        let oracle = crate::check_program_with_external_types(&parsed, &external);
        assert!(
            oracle.report.diagnostics.is_empty(),
            "external value oracle diagnostics: {:#?}",
            oracle.report.diagnostics
        );
        let oracle_types = oracle
            .report
            .expr_type_table
            .entries
            .iter()
            .map(|entry| (entry.expr_id, entry.flow_type.clone()))
            .collect::<BTreeMap<_, _>>();
        for expression in &body.expressions {
            assert_eq!(
                expression.flow_type,
                oracle_types[&syntax_ids[&expression.stable_key]]
            );
        }
    }

    #[test]
    fn source_payload_flow_matches_the_independent_whole_checker_oracle() {
        let source = "event: SOURCE\nuse: event.key\n";
        let unit = link(source);
        let owner = owner_named(&unit, "event");
        let (syntax, _, seed) = inputs(&unit, &owner);
        let summary = resolve_owner_constraint_seed(&seed, []).unwrap();
        assert_eq!(seed.source_payload_queries.len(), 1);
        assert_eq!(seed.source_payload_queries[0].canonical_path, "event");

        let project =
            ProjectSyntaxSnapshot::from_unit_snapshots("app/RUN.bn", vec![Arc::new(unit.clone())])
                .unwrap();
        let abi = crate::project_owner_abi_environment(
            &project,
            &boon_checked::ExternalTypeEnvironment::empty(boon_checked::ProgramRole::Client),
        )
        .unwrap();
        let interfaces = solve_with_abi(&[seed.clone()], &[summary.clone()], &abi);
        let body = infer_with_abi(&syntax, &seed, &summary, &interfaces, &abi);
        assert!(body.diagnostics.is_empty());
        let source_expression = body
            .expression(&seed.source_payload_queries[0].expression)
            .unwrap();
        assert!(matches!(
            &source_expression.flow_type.ty,
            Type::Object(shape) if shape.fields.get("key") == Some(&Type::Text)
        ));

        let parsed = boon_parser::parse_project(
            "app/RUN.bn",
            [("app/RUN.bn".to_owned(), source.to_owned())],
        )
        .unwrap();
        let syntax_ids = parsed
            .expressions
            .iter()
            .filter_map(|expression| {
                parsed
                    .stable_expression_key(expression.id)
                    .map(|stable| (stable, expression.id))
            })
            .collect::<BTreeMap<_, _>>();
        let oracle = crate::check_program(&parsed);
        assert!(
            oracle.report.diagnostics.is_empty(),
            "source payload oracle diagnostics: {:#?}",
            oracle.report.diagnostics
        );
        let oracle_types = oracle
            .report
            .expr_type_table
            .entries
            .iter()
            .map(|entry| (entry.expr_id, entry.flow_type.clone()))
            .collect::<BTreeMap<_, _>>();
        for expression in &body.expressions {
            assert_eq!(
                expression.flow_type,
                oracle_types[&syntax_ids[&expression.stable_key]]
            );
        }
    }

    #[test]
    fn interval_source_call_matches_the_oracle_without_a_payload_inference_lookup() {
        let source = concat!(
            "tick: Duration[milliseconds: 16] |> Timer/interval()\n",
            "use: tick.key\n",
        );
        let unit = link(source);
        let owner = owner_named(&unit, "tick");
        let (syntax, _, seed) = inputs(&unit, &owner);
        let summary = resolve_owner_constraint_seed(&seed, []).unwrap();
        assert!(seed.source_payload_queries.is_empty());

        let project =
            ProjectSyntaxSnapshot::from_unit_snapshots("app/RUN.bn", vec![Arc::new(unit.clone())])
                .unwrap();
        let abi = crate::project_owner_abi_environment(
            &project,
            &boon_checked::ExternalTypeEnvironment::empty(boon_checked::ProgramRole::Client),
        )
        .unwrap();
        let interfaces = solve_with_abi(&[seed.clone()], &[summary.clone()], &abi);
        let body = infer_with_abi(&syntax, &seed, &summary, &interfaces, &abi);
        assert!(body.diagnostics.is_empty());
        let source_call = body
            .calls
            .iter()
            .find(|call| call.function == "Timer/interval")
            .unwrap();
        let source_expression = body.expression(&source_call.expression).unwrap();
        assert_eq!(source_expression.flow_type.mode, FlowMode::Continuous);
        assert!(matches!(
            &source_expression.flow_type.ty,
            Type::Object(shape) if shape.open
        ));
        assert!(source_expression.direct_effect.emits_source);

        let parsed = boon_parser::parse_project(
            "app/RUN.bn",
            [("app/RUN.bn".to_owned(), source.to_owned())],
        )
        .unwrap();
        let syntax_ids = parsed
            .expressions
            .iter()
            .filter_map(|expression| {
                parsed
                    .stable_expression_key(expression.id)
                    .map(|stable| (stable, expression.id))
            })
            .collect::<BTreeMap<_, _>>();
        let oracle = crate::check_program(&parsed);
        assert!(
            oracle.report.diagnostics.is_empty(),
            "interval source oracle diagnostics: {:#?}",
            oracle.report.diagnostics
        );
        let oracle_types = oracle
            .report
            .expr_type_table
            .entries
            .iter()
            .map(|entry| (entry.expr_id, entry.flow_type.clone()))
            .collect::<BTreeMap<_, _>>();
        for expression in &body.expressions {
            assert_eq!(
                expression.flow_type,
                oracle_types[&syntax_ids[&expression.stable_key]]
            );
        }
    }
}
