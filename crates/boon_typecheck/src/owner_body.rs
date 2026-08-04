use crate::owner_interface::{
    TypeUnifier, add_local_root, alpha_normalize_type, bind_projection, flow_mode_join,
    instantiate_type, merge_effects, pattern_type, true_false_type,
};
use crate::{
    OwnerAbiCallableContract, OwnerArgumentKind, OwnerCallableAbiEnvironment, OwnerCollectionKind,
    OwnerConstraintEdgeRole, OwnerConstraintNodeKind, OwnerConstraintSeed, OwnerConstraintSummary,
    OwnerInterfaceSccKey, OwnerInterfaceSccResult, OwnerParameterKind, OwnerPublicInterface,
    OwnerReferenceKind, OwnerResultCallTarget, OwnerResultExpressionRef, OwnerResultTransfer,
    OwnerResultTransferNode, OwnerSourceAnchorRole, OwnerSourceAnchorSite, OwnerSourceMap,
    OwnerSymbolResolution, OwnerSyntaxInput, infix_returns_bool,
};
use boon_checked::{
    BytesType, CheckedEffectSummary, CheckedParameterKind, CheckedTypeSubstitution,
    DiagnosticSeverity, FlowMode, FlowType, ObjectShape, Type, TypeDiagnostic, TypeVar, Variant,
    apply_checked_type_substitution_lookup, specialize_checked_call_result, widen_structural_type,
};
use boon_syntax::{
    AstExprKind, AstStatementKind, StableCheckOwnerKey, StableExpressionKey, StableStatementKey,
};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt;

const OWNER_BODY_INFERENCE_DOMAIN_V1: &[u8] = b"boon.owner-body-inference.v1\0";
const OWNER_BODY_INFERENCE_CONTENT_DOMAIN_V1: &[u8] = b"boon.owner-body-inference-content.v1\0";
const OWNER_BODY_INTERFACE_DOMAIN_V1: &[u8] = b"boon.owner-body-interface-import.v1\0";

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
    pub authoritative_abi_fingerprint_v1: [u8; 32],
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
    pub basis: OwnerBodyInferenceBasis,
    pub interface_imports: Box<[OwnerBodyInterfaceImport]>,
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
        &self.basis.owner
    }
}

fn fingerprint<T: Serialize>(
    domain: &[u8],
    value: &T,
) -> Result<[u8; 32], OwnerBodyInferenceError> {
    boon_contract::canonical_serde_hash_v1(domain, value).map_err(|error| {
        OwnerBodyInferenceError::new(format!("cannot fingerprint owner body inference: {error}"))
    })
}

fn interface_fingerprint(
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
        OwnerConstraintNodeKind::Hold { .. } => CheckedEffectSummary {
            reads_state: true,
            writes_state: true,
            ..CheckedEffectSummary::default()
        },
        _ => CheckedEffectSummary::default(),
    }
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

fn required_interface_owners(
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
                mode = Some(FlowMode::PresentOrAbsent);
                direct_effects[index].emits_source = true;
            }
            OwnerConstraintNodeKind::Reference { parts }
            | OwnerConstraintNodeKind::Drain { parts } => {
                if !resolved.contains_key(&expression.expression)
                    && let Some((root, projection)) = parts.split_first()
                {
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
struct InstantiatedCallSignature {
    parameters: Vec<(String, OwnerParameterKind, FlowType)>,
    result: FlowType,
    context: Option<Type>,
    effect: CheckedEffectSummary,
    target: InferredOwnerCallableTarget,
}

#[derive(Clone)]
struct InferredCallDraft {
    plan: BodyCallPlan,
    target: InferredOwnerCallableTarget,
    effect: CheckedEffectSummary,
    type_substitutions: Vec<(TypeVar, Type)>,
    contextual_type_variables: Vec<TypeVar>,
    syntax_discriminated_result: bool,
}

#[derive(Clone)]
struct EvaluatedResultValue {
    flow_type: FlowType,
    parameter_derived: bool,
    syntax_selected: bool,
}

struct EvaluatedOwnerResult {
    value: EvaluatedResultValue,
    type_substitutions: Vec<(TypeVar, Type)>,
    contextual_type_variables: Vec<TypeVar>,
}

struct OwnerResultTransferEvaluator<'a, 'unifier> {
    interfaces: &'a BTreeMap<StableCheckOwnerKey, &'a OwnerPublicInterface>,
    abi: &'a OwnerCallableAbiEnvironment,
    unifier: &'unifier mut TypeUnifier,
    active_owners: BTreeSet<StableCheckOwnerKey>,
}

impl<'a, 'unifier> OwnerResultTransferEvaluator<'a, 'unifier> {
    fn new(
        interfaces: &'a BTreeMap<StableCheckOwnerKey, &'a OwnerPublicInterface>,
        abi: &'a OwnerCallableAbiEnvironment,
        unifier: &'unifier mut TypeUnifier,
    ) -> Self {
        Self {
            interfaces,
            abi,
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

        let value = if let Some(mut evaluated) = evaluated {
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
            }
        };
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
                });
            }
        }

        match &node.kind {
            OwnerConstraintNodeKind::Call { function }
            | OwnerConstraintNodeKind::Pipe {
                operation: function,
            } => self.evaluate_call_node(
                node, function, nodes, arguments, context, fallbacks, lexical, active,
            ),
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
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn evaluate_call_node(
        &mut self,
        node: &OwnerResultTransferNode,
        function: &str,
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
                contract_fingerprint_v1,
            } => {
                let contract = self.abi.callable(canonical_name)?;
                let current_fingerprint = boon_contract::canonical_serde_hash_v1(
                    b"boon.owner-result-abi-call.v1\0",
                    contract,
                )
                .ok()?;
                if &current_fingerprint != contract_fingerprint_v1 {
                    return None;
                }
                self.evaluate_abi_call(
                    node, function, contract, nodes, arguments, context, fallbacks, lexical, active,
                )
                .or(Some(fallback))
            }
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
        contract: &OwnerAbiCallableContract,
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
        })
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
        }
        | OwnerConstraintEdgeRole::PipeArgument {
            kind: argument_kind,
            name: actual_name,
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
    parameters: &[crate::OwnerAbiParameterContract],
) -> Option<&'a crate::OwnerResultTransferInput> {
    inputs.iter().find(|input| match &input.role {
        OwnerConstraintEdgeRole::CallArgument {
            kind: argument_kind,
            name: actual_name,
        }
        | OwnerConstraintEdgeRole::PipeArgument {
            kind: argument_kind,
            name: actual_name,
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
    contract: &OwnerAbiCallableContract,
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
    abi: &OwnerCallableAbiEnvironment,
) -> Option<InstantiatedCallSignature> {
    let mut variables = BTreeMap::new();
    if let BodyCallableResolution::Owner(target) = &call.resolution {
        let interface = interfaces.get(target)?;
        return Some(InstantiatedCallSignature {
            parameters: interface
                .parameters
                .iter()
                .map(|parameter| {
                    (
                        parameter.name.clone(),
                        parameter.kind,
                        FlowType {
                            mode: parameter.flow_type.mode,
                            ty: instantiate_type(&parameter.flow_type.ty, unifier, &mut variables),
                        },
                    )
                })
                .collect(),
            result: FlowType {
                mode: interface.result.mode,
                ty: instantiate_type(&interface.result.ty, unifier, &mut variables),
            },
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
            parameters: signature
                .parameters
                .iter()
                .map(|parameter| {
                    (
                        parameter.name.clone(),
                        match parameter.kind {
                            CheckedParameterKind::Value => OwnerParameterKind::Value,
                            CheckedParameterKind::Out => OwnerParameterKind::Out,
                        },
                        FlowType {
                            mode: parameter.flow_type.mode,
                            ty: instantiate_type(&parameter.flow_type.ty, unifier, &mut variables),
                        },
                    )
                })
                .collect(),
            result: FlowType {
                mode: signature.result.mode,
                ty: instantiate_type(&signature.result.ty, unifier, &mut variables),
            },
            context: None,
            effect: signature.effect,
            target: InferredOwnerCallableTarget::Authoritative,
        })
}

fn bind_calls(
    calls: Vec<BodyCallPlan>,
    interfaces: &BTreeMap<StableCheckOwnerKey, &OwnerPublicInterface>,
    unifier: &mut TypeUnifier,
    expressions: &[TypeVar],
    external_expressions: &[TypeVar],
    modes: &mut [Option<FlowMode>],
    direct_effects: &mut [CheckedEffectSummary],
    abi: &OwnerCallableAbiEnvironment,
    diagnostics: &mut Vec<OwnerDiagnosticTemplate>,
    work: &mut OwnerBodyInferenceWork,
) -> Vec<InferredCallDraft> {
    calls
        .into_iter()
        .map(|call| {
            work.calls = work.calls.saturating_add(1);
            let signature = instantiate_call_signature(&call, interfaces, unifier, abi);
            let call_variable = expressions[call.expression];
            let (parameters, result, context, effect, target) = match signature {
                Some(signature) => (
                    signature.parameters,
                    signature.result,
                    signature.context,
                    signature.effect,
                    signature.target,
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
                        CheckedEffectSummary::default(),
                        target,
                    )
                }
            };

            if let Some(field) = call.function.strip_prefix("Field/") {
                if let Some(input) = call.inputs.iter().find_map(|(role, input)| {
                    matches!(role, OwnerConstraintEdgeRole::PipeInput).then_some(*input)
                }) && let Some(input) =
                    expression_variable(expressions, external_expressions, input)
                {
                    let projected = bind_projection(unifier, input, &[field.to_owned()]);
                    unifier.unify(Type::Var(call_variable), Type::Var(projected));
                }
            } else if !matches!(call.resolution, BodyCallableResolution::Owner(_)) {
                // A user callable's principal result is intentionally allowed
                // to be wider than this occurrence (for example a syntax-
                // dispatched function). Bind user results only after the
                // frozen result transfer has evaluated the actual arguments.
                unifier.bind_var(call_variable, result.ty);
            }

            for (role, input) in &call.inputs {
                let Some(input) = expression_variable(expressions, external_expressions, *input)
                else {
                    continue;
                };
                match role {
                    OwnerConstraintEdgeRole::PipeInput => {
                        if let Some((_, _, expected)) = parameters
                            .iter()
                            .find(|(_, kind, _)| *kind == OwnerParameterKind::Value)
                        {
                            unifier.unify(Type::Var(input), expected.ty.clone());
                        }
                    }
                    OwnerConstraintEdgeRole::CallArgument { kind, name }
                    | OwnerConstraintEdgeRole::PipeArgument { kind, name } => {
                        if let Some((_, parameter_kind, expected)) = parameters
                            .iter()
                            .find(|(parameter, _, _)| parameter == name)
                            && matches!(
                                (parameter_kind, kind),
                                (OwnerParameterKind::Value, OwnerArgumentKind::Named)
                                    | (OwnerParameterKind::Out, OwnerArgumentKind::Named)
                            )
                        {
                            unifier.unify(Type::Var(input), expected.ty.clone());
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
            modes[call.expression] = flow_mode_join(modes[call.expression], Some(result.mode));
            direct_effects[call.expression] =
                merge_effects(direct_effects[call.expression], effect);
            work.interface_imports = work.interface_imports.saturating_add(1);
            InferredCallDraft {
                plan: call,
                target,
                effect,
                type_substitutions: Vec::new(),
                contextual_type_variables: Vec::new(),
                syntax_discriminated_result: false,
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
            OwnerConstraintEdgeRole::CallArgument { kind, name }
            | OwnerConstraintEdgeRole::PipeArgument { kind, name } => {
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
    })
}

#[allow(clippy::too_many_arguments)]
fn refine_owner_call_at(
    call_index: usize,
    call_by_expression: &BTreeMap<usize, usize>,
    states: &mut [u8],
    drafts: &mut [InferredCallDraft],
    seed: &OwnerConstraintSeed,
    interfaces: &BTreeMap<StableCheckOwnerKey, &OwnerPublicInterface>,
    abi: &OwnerCallableAbiEnvironment,
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
                seed,
                interfaces,
                abi,
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
    });
    let evaluated = {
        let mut evaluator = OwnerResultTransferEvaluator::new(interfaces, abi, unifier);
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
        draft.type_substitutions = evaluated.type_substitutions;
        draft.contextual_type_variables = evaluated.contextual_type_variables;
        draft.syntax_discriminated_result = evaluated.value.syntax_selected;
    }
    states[call_index] = 2;
}

#[allow(clippy::too_many_arguments)]
fn refine_owner_call_transfers(
    drafts: &mut [InferredCallDraft],
    seed: &OwnerConstraintSeed,
    interfaces: &BTreeMap<StableCheckOwnerKey, &OwnerPublicInterface>,
    abi: &OwnerCallableAbiEnvironment,
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
            seed,
            interfaces,
            abi,
            unifier,
            expressions,
            external_expressions,
            caller_context,
            modes,
        );
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
/// owner inputs.
pub fn infer_owner_body<'a>(
    syntax: &OwnerSyntaxInput,
    seed: &OwnerConstraintSeed,
    summary: &OwnerConstraintSummary,
    abi: &OwnerCallableAbiEnvironment,
    own_scc: &'a OwnerInterfaceSccResult,
    imported_sccs: impl IntoIterator<Item = &'a OwnerInterfaceSccResult>,
) -> Result<OwnerBodyInferenceShard, OwnerBodyInferenceError> {
    validate_inputs(syntax, seed, summary, own_scc)?;
    let required = required_interface_owners(seed, summary);
    let mut interfaces = BTreeMap::new();
    let mut providers = BTreeMap::new();
    let mut supplied_keys = BTreeSet::new();
    let mut frozen_results = Vec::new();
    for result in std::iter::once(own_scc).chain(imported_sccs) {
        if !supplied_keys.insert(result.key.clone()) {
            return Err(OwnerBodyInferenceError::new(format!(
                "owner body inference received duplicate interface SCC {:?}",
                result.key
            )));
        }
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
    let authoritative_abi_fingerprint_v1 = abi.fingerprint_v1();
    let basis = OwnerBodyInferenceBasis {
        owner: seed.owner.clone(),
        syntax_fingerprint_v1: syntax.fingerprint_v1(),
        seed_fingerprint_v1: seed.fingerprint_v1(),
        summary_fingerprint_v1: summary.fingerprint_v1(),
        own_scc: own_scc_ref,
        imports: frozen_results.into_boxed_slice(),
        authoritative_abi_fingerprint_v1,
    };

    let mut interface_imports = interfaces
        .values()
        .map(|interface| {
            let provider = providers[&interface.owner];
            Ok(OwnerBodyInterfaceImport {
                owner: interface.owner.clone(),
                interface_fingerprint_v1: interface_fingerprint(interface)?,
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
    let external_expressions = (0..seed.external_expressions.len())
        .map(|_| unifier.fresh())
        .collect::<Vec<_>>();
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
        let Some(variable) = expression_variable(&expressions, &external_expressions, *expression)
        else {
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
            if let Some(variable) =
                expression_variable(&expressions, &external_expressions, input.expression)
            {
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

    for (external, variable) in seed.external_expressions.iter().zip(&external_expressions) {
        let interface = interfaces[&external.owner];
        let mut variables = BTreeMap::new();
        let ty = instantiate_type(&interface.result.ty, &mut unifier, &mut variables);
        unifier.bind_var(*variable, ty);
        work.interface_imports = work.interface_imports.saturating_add(1);
    }
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
        unifier.bind_var(expressions[index], ty);
        modes[index] = flow_mode_join(modes[index], Some(interface.result.mode));
        direct_effects[index] = merge_effects(direct_effects[index], interface.effect);
        work.interface_imports = work.interface_imports.saturating_add(1);
    }

    let mut diagnostics = Vec::new();
    push_invalid_syntax_diagnostics(seed, &mut diagnostics);
    let mut call_drafts = bind_calls(
        calls,
        &interfaces,
        &mut unifier,
        &expressions,
        &external_expressions,
        &mut modes,
        &mut direct_effects,
        abi,
        &mut diagnostics,
        &mut work,
    );
    refine_owner_call_transfers(
        &mut call_drafts,
        seed,
        &interfaces,
        abi,
        &mut unifier,
        &expressions,
        &external_expressions,
        context,
        &mut modes,
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
    let inferred_expressions = syntax
        .expressions
        .iter()
        .enumerate()
        .map(|(index, expression)| {
            Ok(InferredOwnerExpression {
                id: OwnerInferenceExpressionId(checked_u32(index, "inferred expression id")?),
                stable_key: expression.stable_key.clone(),
                flow_type: FlowType {
                    mode: modes[index].unwrap_or(FlowMode::Continuous),
                    ty: alpha_normalize_type(
                        &unifier.resolve(&Type::Var(expressions[index])),
                        &mut alpha_variables,
                        &mut next_alpha,
                    ),
                },
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
            &interface_imports,
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
            &basis,
            &interface_imports,
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
    Ok(OwnerBodyInferenceShard {
        basis,
        interface_imports: interface_imports.into_boxed_slice(),
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
    })
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

    fn test_abi() -> OwnerCallableAbiEnvironment {
        let unit = link("value: 1\n");
        let project =
            ProjectSyntaxSnapshot::from_unit_snapshots("app/RUN.bn", vec![Arc::new(unit)]).unwrap();
        crate::project_owner_abi_environment(
            &project,
            &boon_checked::ExternalTypeEnvironment::empty(boon_checked::ProgramRole::Client),
        )
        .unwrap()
        .callable_environment()
        .unwrap()
    }

    fn solve(
        seeds: &[OwnerConstraintSeed],
        summaries: &[OwnerConstraintSummary],
    ) -> Vec<crate::OwnerInterfaceSccResult> {
        let abi = test_abi();
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
        let abi = test_abi();
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
}
