use crate::owner_interface::{
    TypeUnifier, add_local_root, alpha_normalize_type, authoritative_signature, bind_projection,
    flow_mode_join, instantiate_type, merge_effects, pattern_type, true_false_type,
};
use crate::{
    BuiltinSignatureRegistry, OwnerArgumentKind, OwnerCollectionKind, OwnerConstraintEdgeRole,
    OwnerConstraintNodeKind, OwnerConstraintSeed, OwnerConstraintSummary, OwnerDeclarationKind,
    OwnerInterfaceSccKey, OwnerInterfaceSccResult, OwnerParameterKind, OwnerPublicInterface,
    OwnerReferenceKind, OwnerSourceAnchorRole, OwnerSourceAnchorSite, OwnerSourceMap,
    OwnerSymbolResolution, OwnerSyntaxInput, RenderContractRegistry, infix_returns_bool,
};
use boon_checked::{
    BytesType, CheckedEffectSummary, CheckedParameterKind, DiagnosticSeverity, FlowMode, FlowType,
    ObjectShape, Type, TypeDiagnostic, TypeVar, Variant,
};
use boon_document_model::ProgramRole;
use boon_syntax::{AstExprKind, AstStatementKind, StableCheckOwnerKey, StableExpressionKey};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt;

const OWNER_BODY_INFERENCE_DOMAIN_V1: &[u8] = b"boon.owner-body-inference.v1\0";
const OWNER_BODY_INFERENCE_CONTENT_DOMAIN_V1: &[u8] = b"boon.owner-body-inference-content.v1\0";
const OWNER_BODY_INTERFACE_DOMAIN_V1: &[u8] = b"boon.owner-body-interface-import.v1\0";
const OWNER_BODY_AUTHORITATIVE_ABI_DOMAIN_V1: &[u8] = b"boon.owner-body-authoritative-abi.v1\0";

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

fn owner_result_expression(seed: &OwnerConstraintSeed) -> Option<u32> {
    let public = seed
        .declarations
        .iter()
        .find(|declaration| declaration.public)?;
    seed.statement_values
        .iter()
        .find(|(statement, _)| *statement == public.statement)
        .map(|(_, expression)| *expression)
        .or_else(|| {
            (public.kind == OwnerDeclarationKind::Function)
                .then(|| {
                    seed.statement_values
                        .last()
                        .map(|(_, expression)| *expression)
                })
                .flatten()
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

fn authoritative_abi_fingerprint_v1(
    program_role: ProgramRole,
    seed: &OwnerConstraintSeed,
) -> Result<[u8; 32], OwnerBodyInferenceError> {
    let builtins = BuiltinSignatureRegistry::default();
    let render = RenderContractRegistry::default();
    let signatures = seed
        .expressions
        .iter()
        .filter_map(|expression| match &expression.kind {
            OwnerConstraintNodeKind::Call { function } => Some(function),
            OwnerConstraintNodeKind::Pipe { operation } => Some(operation),
            _ => None,
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .map(|name| {
            (
                name.clone(),
                authoritative_signature(name, &builtins, &render),
            )
        })
        .collect::<Vec<_>>();
    fingerprint(
        OWNER_BODY_AUTHORITATIVE_ABI_DOMAIN_V1,
        &(program_role, signatures),
    )
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
}

fn instantiate_call_signature(
    call: &BodyCallPlan,
    interfaces: &BTreeMap<StableCheckOwnerKey, &OwnerPublicInterface>,
    unifier: &mut TypeUnifier,
    builtins: &BuiltinSignatureRegistry,
    render: &RenderContractRegistry,
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
                .map(|context| instantiate_type(&context.ty, unifier, &mut variables)),
            effect: interface.effect,
            target: InferredOwnerCallableTarget::Owner {
                owner: target.clone(),
            },
        });
    }
    if !matches!(&call.resolution, BodyCallableResolution::Authoritative) {
        return None;
    }
    authoritative_signature(&call.function, builtins, render).map(|signature| {
        InstantiatedCallSignature {
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
        }
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
    diagnostics: &mut Vec<OwnerDiagnosticTemplate>,
    work: &mut OwnerBodyInferenceWork,
) -> Vec<InferredCallDraft> {
    let builtins = BuiltinSignatureRegistry::default();
    let render = RenderContractRegistry::default();
    calls
        .into_iter()
        .map(|call| {
            work.calls = work.calls.saturating_add(1);
            let signature =
                instantiate_call_signature(&call, interfaces, unifier, &builtins, &render);
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
            } else {
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
            }
        })
        .collect()
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
    program_role: ProgramRole,
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
    let authoritative_abi_fingerprint_v1 = authoritative_abi_fingerprint_v1(program_role, seed)?;
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
        let ty = instantiate_type(&context.ty, &mut unifier, &mut own_variables);
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

    if let Some(result_reference) = owner_result_expression(seed)
        && let Some(result) =
            expression_variable(&expressions, &external_expressions, result_reference)
    {
        unifier.unify(Type::Var(result), own_result.clone());
        if let Some(mode) = modes.get_mut(result_reference as usize) {
            *mode = flow_mode_join(*mode, Some(own_interface.result.mode));
        }
    }

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
    let call_drafts = bind_calls(
        calls,
        &interfaces,
        &mut unifier,
        &expressions,
        &external_expressions,
        &mut modes,
        &mut direct_effects,
        &mut diagnostics,
        &mut work,
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
    use boon_parser::{UnitSyntaxSnapshot, parse_project_source_unit, project_unit_link_keys};

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

    fn solve(
        seeds: &[OwnerConstraintSeed],
        summaries: &[OwnerConstraintSummary],
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
            let dependencies = scc
                .dependencies
                .iter()
                .map(|dependency| results.get(dependency).unwrap())
                .collect::<Vec<_>>();
            let result = solve_owner_interface_scc(
                scc,
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
        let own_scc = results
            .iter()
            .find(|result| result.key.members.contains(&seed.owner))
            .unwrap();
        infer_owner_body(
            syntax,
            seed,
            summary,
            ProgramRole::Client,
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
