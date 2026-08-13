use crate::owner_interface::{
    OwnerFlowConstraint, OwnerInheritedPatternNarrowing, OwnerInterfaceSccProjection,
    OwnerPatternNarrowing, TypeUnifier, alpha_normalize_type, bind_and_record_flow_variables,
    bind_and_record_structural_flow_variables, bind_projection, flow_mode_join,
    inherited_pattern_read_plans, initialize_owner_hold_constraints,
    instantiate_owner_inherited_pattern_narrowings, instantiate_type, mark_owner_derived_providers,
    merge_effects, pattern_binding_type_from_pattern, pattern_type,
    refine_owner_inherited_pattern_narrowings, refine_owner_pattern_narrowings,
    replay_flow_constraints, replay_owner_hold_constraints, signature_dynamic_expression_index,
    true_false_type,
};
use crate::owner_signature_lexical::effective_narrowed_selector_read_matches;
use crate::{
    OwnerAbiEvaluationScope, OwnerCallableLexicalSignature, OwnerCollectionKind,
    OwnerConstraintEdgeRole, OwnerConstraintNodeKind, OwnerConstraintSeed, OwnerConstraintSummary,
    OwnerEffectiveLexicalTarget, OwnerInferenceAbiEnvironment, OwnerInterfaceEvaluationScope,
    OwnerInterfaceScc, OwnerInterfaceSccKey, OwnerInterfaceSccResult,
    OwnerLexicalDeclarationTarget, OwnerLexicalPlan, OwnerParameterKind, OwnerPublicInterface,
    OwnerReferenceKind, OwnerSignatureCallLexicalError, OwnerSignatureCallPlan,
    OwnerSignatureDeclarationTarget, OwnerSignatureLexicalPlan, OwnerSignatureMatchedInputSource,
    OwnerSignaturePassSource, OwnerSourceAnchorRole, OwnerSourceAnchorSite, OwnerSourceMap,
    OwnerSymbolResolution, OwnerSyntaxInput, OwnerValueAbiForbiddenReason,
    OwnerValueAbiLookupOutcome, infix_requires_number_operands, infix_returns_bool,
    project_owner_signature_lexical_plan,
};
use boon_checked::{
    BytesType, CheckedCallableKind, CheckedEffectSummary, CheckedParameterKind,
    CheckedTypeSubstitution, CheckedTypeSubstitutionLookup, DiagnosticSeverity, FlowMode, FlowType,
    ObjectShape, OwnerLexicalTargetRef, Type, TypeDiagnostic, TypeVar, Variant,
    apply_checked_type_substitution_lookup, specialize_checked_call_result, widen_structural_type,
};
use boon_contract::SourceBundleDigestV1;
use boon_data::{ExactNumber, MAX_BITS_WIDTH};
use boon_parser::ProjectSyntaxSnapshot;
use boon_syntax::{
    AstExprKind, AstStatementKind, SourceUnitId, StableCheckOwnerKey, StableExpressionKey,
    StableStatementKey,
};
use serde::Serialize;
use smallvec::SmallVec;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::error::Error;
use std::fmt;
use std::sync::Arc;
#[cfg(not(target_arch = "wasm32"))]
use std::time::Instant;
#[cfg(target_arch = "wasm32")]
use web_time::Instant;

const OWNER_BODY_INFERENCE_DOMAIN_V10: &[u8] = b"boon.owner-body-inference.v10\0";
const OWNER_BODY_INFERENCE_CONTENT_DOMAIN_V8: &[u8] = b"boon.owner-body-inference-content.v8\0";
const OWNER_BODY_INFERENCE_CURRENTNESS_DOMAIN_V13: &[u8] =
    b"boon.owner-body-inference-currentness.v13\0";
const OWNER_BODY_INTERFACE_PLAN_DOMAIN_V5: &[u8] = b"boon.owner-body-interface-plan.v5\0";
const OWNER_INTERFACE_TRANSFER_MODULE_DOMAIN_V5: &[u8] =
    b"boon.owner-interface-transfer-module.v5\0";
const OWNER_RESIDUAL_PROGRAM_DOMAIN_V3: &[u8] = b"boon.owner-residual-program.v3\0";
const SOURCE_UNIT_OWNER_DIAGNOSTICS_DOMAIN_V1: &[u8] = b"boon.source-unit-owner-diagnostics.v1\0";
const OWNER_DIAGNOSTICS_AGGREGATE_DOMAIN_V8: &[u8] = b"boon.owner-diagnostics-aggregate.v8\0";

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

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum OwnerResidualExpressionRef {
    Local {
        expression: StableExpressionKey,
    },
    Child {
        owner: StableCheckOwnerKey,
        expression: StableExpressionKey,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct OwnerResidualInput {
    pub(crate) role: OwnerConstraintEdgeRole,
    pub(crate) expression: OwnerResidualExpressionRef,
    pub(crate) formal_ordinal: Option<u32>,
    pub(crate) explicit_pass: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct OwnerResidualParameterRead {
    pub(crate) parameter_ordinal: u32,
    pub(crate) projection: Box<[String]>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct OwnerResidualAbiParameter {
    pub(crate) name: String,
    pub(crate) kind: CheckedParameterKind,
    pub(crate) ordinal: u32,
    pub(crate) flow_type: FlowType,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct OwnerResidualAbiContract {
    pub(crate) kind: CheckedCallableKind,
    pub(crate) parameters: Box<[OwnerResidualAbiParameter]>,
    pub(crate) result: FlowType,
    pub(crate) result_specialization: crate::OwnerAbiResultSpecialization,
}

impl From<&crate::OwnerInferenceCallableContract> for OwnerResidualAbiContract {
    fn from(contract: &crate::OwnerInferenceCallableContract) -> Self {
        Self {
            kind: contract.kind,
            parameters: contract
                .parameters
                .iter()
                .map(|parameter| OwnerResidualAbiParameter {
                    name: parameter.name.clone(),
                    kind: parameter.kind,
                    ordinal: parameter.ordinal,
                    flow_type: parameter.flow_type.clone(),
                })
                .collect::<Vec<_>>()
                .into_boxed_slice(),
            result: contract.result.clone(),
            result_specialization: contract.result_specialization,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum OwnerResidualCallTarget {
    Owner {
        owner: StableCheckOwnerKey,
    },
    Abi {
        canonical_name: String,
        contract: OwnerResidualAbiContract,
    },
    Unresolved,
    Ambiguous {
        candidates: Box<[StableCheckOwnerKey]>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct OwnerResidualNode {
    pub(crate) expression: StableExpressionKey,
    pub(crate) flow_type: FlowType,
    pub(crate) static_number: Option<String>,
    pub(crate) kind: OwnerConstraintNodeKind,
    pub(crate) inputs: Box<[OwnerResidualInput]>,
    pub(crate) parameter_read: Option<OwnerResidualParameterRead>,
    pub(crate) call_target: Option<OwnerResidualCallTarget>,
}

/// Unsealed result-semantics draft produced beside one public interface.
/// Stable syntax identities and generic constraint roles are consumed by the
/// residual compiler and never survive in a published transfer module.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum OwnerResidualDraft {
    Principal,
    Parameter {
        read: OwnerResidualParameterRead,
    },
    Expression {
        root: OwnerResidualExpressionRef,
        nodes: Box<[OwnerResidualNode]>,
    },
}

type OwnerResidualOpId = u32;
type OwnerResidualFrameId = u32;
type OwnerResidualNamespaceId = u32;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct OwnerResidualFrameParameter {
    ordinal: u32,
    flow_type: FlowType,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct OwnerResidualFrameContext {
    flow_type: FlowType,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct OwnerResidualFrame {
    namespace: OwnerResidualNamespaceId,
    parameters: Box<[OwnerResidualFrameParameter]>,
    context: Option<OwnerResidualFrameContext>,
    result: FlowType,
    result_flush_type: Option<Type>,
    type_variables: Box<[TypeVar]>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum OwnerResidualRoot {
    Principal {
        frame: OwnerResidualFrameId,
    },
    Parameter {
        frame: OwnerResidualFrameId,
        read: OwnerResidualParameterRead,
    },
    Value {
        frame: OwnerResidualFrameId,
        op: OwnerResidualOpId,
    },
}

impl OwnerResidualRoot {
    const fn frame(&self) -> OwnerResidualFrameId {
        match self {
            Self::Principal { frame }
            | Self::Parameter { frame, .. }
            | Self::Value { frame, .. } => *frame,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum OwnerResidualCompiledCallTarget {
    Own { owner: u32 },
    Dependency { dependency: u32, owner: u32 },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum OwnerResidualCallContext {
    Inherited,
    Explicit { value: OwnerResidualOpId },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct OwnerResidualActual {
    formal_ordinal: u32,
    value: OwnerResidualOpId,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct OwnerResidualRecordField {
    name: String,
    value: OwnerResidualOpId,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct OwnerResidualWhenArm {
    pattern: crate::OwnerPatternConstraint,
    output: OwnerResidualOpId,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct OwnerResidualBlockBinding {
    name: String,
    value: OwnerResidualOpId,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum OwnerResidualOpKind {
    Fallback,
    Surface {
        namespace: OwnerResidualNamespaceId,
    },
    ParameterRead {
        parameter_ordinal: u32,
        projection: Box<[String]>,
    },
    LexicalRead {
        parts: Box<[String]>,
    },
    CompiledCall {
        target: OwnerResidualCompiledCallTarget,
        actuals: Box<[OwnerResidualActual]>,
        context: OwnerResidualCallContext,
    },
    AbiCall {
        canonical_name: String,
        contract: OwnerResidualAbiContract,
        actuals: Box<[OwnerResidualActual]>,
    },
    Infix {
        operation: String,
        left: OwnerResidualOpId,
        right: OwnerResidualOpId,
    },
    Record {
        tag: Option<String>,
        fields: Box<[OwnerResidualRecordField]>,
    },
    When {
        selector: OwnerResidualOpId,
        arms: Box<[OwnerResidualWhenArm]>,
    },
    Forward {
        output: Option<OwnerResidualOpId>,
    },
    Block {
        bindings: Box<[OwnerResidualBlockBinding]>,
        result: Option<OwnerResidualOpId>,
    },
    Collection {
        collection: OwnerCollectionKind,
        items: Box<[OwnerResidualOpId]>,
    },
    Latest {
        branches: Box<[OwnerResidualOpId]>,
    },
    Draining {
        input: Option<OwnerResidualOpId>,
    },
    Hold {
        initial: Option<OwnerResidualOpId>,
        updates: Box<[OwnerResidualOpId]>,
    },
    Then {
        value: Option<OwnerResidualOpId>,
    },
    MapEntry {
        key: Option<OwnerResidualOpId>,
        value: Option<OwnerResidualOpId>,
    },
    Source,
    Skip,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct OwnerResidualOp {
    frame: OwnerResidualFrameId,
    fallback: FlowType,
    static_number: Option<ExactNumber>,
    kind: OwnerResidualOpKind,
}

impl OwnerResidualOpKind {
    fn edge_count(&self) -> u64 {
        match self {
            Self::Fallback
            | Self::Surface { .. }
            | Self::ParameterRead { .. }
            | Self::LexicalRead { .. }
            | Self::Source
            | Self::Skip => 0,
            Self::CompiledCall {
                actuals, context, ..
            } => 1_u64
                .saturating_add(actuals.len() as u64)
                .saturating_add(u64::from(matches!(
                    context,
                    OwnerResidualCallContext::Explicit { .. }
                ))),
            Self::AbiCall { actuals, .. } => actuals.len() as u64,
            Self::Infix { .. } | Self::MapEntry { .. } => 2,
            Self::Record { fields, .. } => fields.len() as u64,
            Self::When { arms, .. } => 1_u64.saturating_add(arms.len() as u64),
            Self::Forward { output }
            | Self::Draining { input: output }
            | Self::Then { value: output } => u64::from(output.is_some()),
            Self::Block { bindings, result } => {
                (bindings.len() as u64).saturating_add(u64::from(result.is_some()))
            }
            Self::Collection { items, .. } => items.len() as u64,
            Self::Latest { branches } => branches.len() as u64,
            Self::Hold { initial, updates } => {
                u64::from(initial.is_some()).saturating_add(updates.len() as u64)
            }
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct OwnerResidualOwnerProgram {
    namespaces: Box<[u32]>,
    frames: Box<[OwnerResidualFrame]>,
    ops: Box<[OwnerResidualOp]>,
    root: OwnerResidualRoot,
    invocation_invariant: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct OwnerResidualProgram {
    owners: Box<[OwnerResidualOwnerProgram]>,
    op_count: u64,
    edge_count: u64,
    fingerprint_v1: [u8; 32],
}

/// Allocation-free work counters for one or more compiled residual evaluations.
///
/// These counters are evaluator telemetry only. They do not participate in any
/// semantic fingerprint or currentness receipt.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct OwnerResidualEvaluationWork {
    pub occurrences: u64,
    pub owner_dispatches: u64,
    pub compiled_call_dispatches: u64,
    pub op_visits: u64,
    pub maximum_owner_depth: u64,
}

impl OwnerResidualEvaluationWork {
    pub(crate) fn merge(&mut self, other: Self) {
        self.occurrences = self.occurrences.saturating_add(other.occurrences);
        self.owner_dispatches = self.owner_dispatches.saturating_add(other.owner_dispatches);
        self.compiled_call_dispatches = self
            .compiled_call_dispatches
            .saturating_add(other.compiled_call_dispatches);
        self.op_visits = self.op_visits.saturating_add(other.op_visits);
        self.maximum_owner_depth = self.maximum_owner_depth.max(other.maximum_owner_depth);
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum OwnerInterfaceTransferRoute {
    Own,
    Dependency(u32),
}

/// Shared result-specialization module for one interface SCC.
///
/// Modules retain only direct dependency modules. The dependency graph is a
/// DAG, so bodies can specialize nested user-call results without copying the
/// complete transitive interface closure into every owner evaluation.
#[derive(Clone)]
pub struct OwnerInterfaceTransferModule {
    key: OwnerInterfaceSccKey,
    result: Arc<OwnerInterfaceSccResult>,
    dependencies: Box<[Arc<OwnerInterfaceTransferModule>]>,
    program: Arc<OwnerResidualProgram>,
    constant_results: Box<[Option<EvaluatedResultValue>]>,
    fingerprint_v1: [u8; 32],
}

impl fmt::Debug for OwnerInterfaceTransferModule {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("OwnerInterfaceTransferModule")
            .field("key", &self.key)
            .field(
                "dependencies",
                &self
                    .dependencies
                    .iter()
                    .map(|dependency| &dependency.key)
                    .collect::<Vec<_>>(),
            )
            .field("fingerprint_v1", &self.fingerprint_v1)
            .finish()
    }
}

impl PartialEq for OwnerInterfaceTransferModule {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key && self.fingerprint_v1 == other.fingerprint_v1
    }
}

impl Eq for OwnerInterfaceTransferModule {}

impl std::ops::Deref for OwnerInterfaceTransferModule {
    type Target = OwnerInterfaceSccResult;

    fn deref(&self) -> &Self::Target {
        &self.result
    }
}

impl OwnerInterfaceTransferModule {
    pub fn key(&self) -> &OwnerInterfaceSccKey {
        &self.key
    }

    pub const fn fingerprint_v1(&self) -> [u8; 32] {
        self.fingerprint_v1
    }

    pub fn direct_dependency_keys(&self) -> impl Iterator<Item = &OwnerInterfaceSccKey> {
        self.dependencies.iter().map(|dependency| &dependency.key)
    }

    pub(crate) fn owns_owner(&self, owner: &StableCheckOwnerKey) -> bool {
        self.key.members.binary_search(owner).is_ok()
    }

    fn result(&self) -> &OwnerInterfaceSccResult {
        &self.result
    }

    fn constant_result_at(&self, owner: usize) -> Option<&EvaluatedResultValue> {
        self.constant_results.get(owner)?.as_ref()
    }

    fn residual_work_for_owner(&self, owner: &StableCheckOwnerKey) -> Option<(u64, u64)> {
        let owner = self.result.key.members.binary_search(owner).ok()?;
        let program = self.program.owners.get(owner)?;
        Some((
            program.ops.len() as u64,
            program.ops.iter().map(|op| op.kind.edge_count()).sum(),
        ))
    }

    #[cfg(test)]
    fn constant_result(&self, owner: &StableCheckOwnerKey) -> Option<&EvaluatedResultValue> {
        let owner = self.result.key.members.binary_search(owner).ok()?;
        self.constant_result_at(owner)
    }
}

fn owner_result_transfer_is_invocation_invariant(
    interface: &OwnerPublicInterface,
    draft: &OwnerResidualDraft,
) -> bool {
    let OwnerResidualDraft::Expression { nodes, .. } = draft else {
        return false;
    };
    if nodes.iter().any(|node| node.parameter_read.is_some()) {
        return false;
    }
    let mut input_variables = BTreeSet::new();
    for parameter in &interface.parameters {
        crate::collect_type_vars(&parameter.flow_type.ty, &mut input_variables);
    }
    if let Some(context) = &interface.context {
        crate::collect_type_vars(&context.flow_type.ty, &mut input_variables);
    }
    let mut transfer_variables = BTreeSet::new();
    crate::collect_type_vars(&interface.result.ty, &mut transfer_variables);
    if let Some(flush_type) = &interface.result_flush_type {
        crate::collect_type_vars(flush_type, &mut transfer_variables);
    }
    for node in nodes {
        crate::collect_type_vars(&node.flow_type.ty, &mut transfer_variables);
    }
    input_variables.is_disjoint(&transfer_variables)
}

fn owner_result_transfer_type_variables_are_declared(ty: &Type, declared: &[TypeVar]) -> bool {
    match ty {
        Type::Var(variable) => declared.binary_search(variable).is_ok(),
        Type::Object(shape) => shape
            .fields
            .values()
            .all(|ty| owner_result_transfer_type_variables_are_declared(ty, declared)),
        Type::List(item) | Type::Set(item) => {
            owner_result_transfer_type_variables_are_declared(item, declared)
        }
        Type::Map { key, value } => {
            owner_result_transfer_type_variables_are_declared(key, declared)
                && owner_result_transfer_type_variables_are_declared(value, declared)
        }
        Type::Function { args, result } => {
            args.iter()
                .all(|ty| owner_result_transfer_type_variables_are_declared(ty, declared))
                && owner_result_transfer_type_variables_are_declared(&result.ty, declared)
        }
        Type::VariantSet(variants) => variants.iter().all(|variant| match variant {
            Variant::Tag(_) => true,
            Variant::Tagged { fields, .. } => fields
                .fields
                .values()
                .all(|ty| owner_result_transfer_type_variables_are_declared(ty, declared)),
        }),
        Type::Union(members) => members
            .iter()
            .all(|ty| owner_result_transfer_type_variables_are_declared(ty, declared)),
        Type::Text
        | Type::Number
        | Type::Bytes(_)
        | Type::Bits { .. }
        | Type::Absent
        | Type::RenderContract
        | Type::UnresolvedShape { .. }
        | Type::Unknown => true,
    }
}

fn owner_result_transfer_interface_variables_are_complete(
    interface: &OwnerPublicInterface,
    draft: &OwnerResidualDraft,
    residual_type_variable_count: u32,
) -> bool {
    let declared = (0..residual_type_variable_count)
        .map(TypeVar)
        .collect::<Vec<_>>();
    interface
        .type_variables
        .windows(2)
        .all(|pair| pair[0] < pair[1])
        && interface.parameters.iter().all(|parameter| {
            owner_result_transfer_type_variables_are_declared(&parameter.flow_type.ty, &declared)
        })
        && interface.context.as_ref().is_none_or(|context| {
            owner_result_transfer_type_variables_are_declared(&context.flow_type.ty, &declared)
        })
        && owner_result_transfer_type_variables_are_declared(&interface.result.ty, &declared)
        && interface
            .result_flush_type
            .as_ref()
            .is_none_or(|ty| owner_result_transfer_type_variables_are_declared(ty, &declared))
        && interface.captures.iter().all(|capture| {
            owner_result_transfer_type_variables_are_declared(&capture.flow_type.ty, &declared)
                && capture.flush_type.as_ref().is_none_or(|ty| {
                    owner_result_transfer_type_variables_are_declared(ty, &declared)
                })
        })
        && interface.lexical_captures.iter().all(|capture| {
            owner_result_transfer_type_variables_are_declared(&capture.flow_type.ty, &declared)
        })
        && match draft {
            OwnerResidualDraft::Principal | OwnerResidualDraft::Parameter { .. } => true,
            OwnerResidualDraft::Expression { nodes, .. } => nodes.iter().all(|node| {
                owner_result_transfer_type_variables_are_declared(&node.flow_type.ty, &declared)
            }),
        }
}

struct OwnerResidualProgramBuilder<'a> {
    result: &'a OwnerInterfaceSccResult,
    drafts: &'a [OwnerResidualDraft],
    residual_type_variable_count: u32,
    dependencies: &'a [Arc<OwnerInterfaceTransferModule>],
    routes: &'a BTreeMap<StableCheckOwnerKey, OwnerInterfaceTransferRoute>,
    namespaces: Vec<u32>,
    frames: Vec<OwnerResidualFrame>,
    ops: Vec<Option<OwnerResidualOp>>,
    surfaces: BTreeMap<(OwnerResidualFrameId, StableCheckOwnerKey), OwnerResidualOpId>,
    dependency_surface_namespaces: BTreeMap<(OwnerResidualFrameId, u32), OwnerResidualNamespaceId>,
}

impl<'a> OwnerResidualProgramBuilder<'a> {
    fn new(
        result: &'a OwnerInterfaceSccResult,
        drafts: &'a [OwnerResidualDraft],
        residual_type_variable_count: u32,
        dependencies: &'a [Arc<OwnerInterfaceTransferModule>],
        routes: &'a BTreeMap<StableCheckOwnerKey, OwnerInterfaceTransferRoute>,
    ) -> Self {
        Self {
            result,
            drafts,
            residual_type_variable_count,
            dependencies,
            routes,
            namespaces: Vec::new(),
            frames: Vec::new(),
            ops: Vec::new(),
            surfaces: BTreeMap::new(),
            dependency_surface_namespaces: BTreeMap::new(),
        }
    }

    fn push_namespace(
        &mut self,
        count: u32,
    ) -> Result<OwnerResidualNamespaceId, OwnerBodyInferenceError> {
        let id = checked_u32(self.namespaces.len(), "owner residual namespace count")?;
        self.namespaces.push(count);
        Ok(id)
    }

    fn push_frame(
        &mut self,
        interface: &OwnerPublicInterface,
        namespace: OwnerResidualNamespaceId,
    ) -> Result<OwnerResidualFrameId, OwnerBodyInferenceError> {
        let frame = checked_u32(self.frames.len(), "owner residual frame count")?;
        let parameters = interface
            .parameters
            .iter()
            .map(|parameter| OwnerResidualFrameParameter {
                ordinal: parameter.ordinal,
                flow_type: parameter.flow_type.clone(),
            })
            .collect::<Vec<_>>()
            .into_boxed_slice();
        let context = interface
            .context
            .as_ref()
            .map(|context| OwnerResidualFrameContext {
                flow_type: context.flow_type.clone(),
            });
        self.frames.push(OwnerResidualFrame {
            namespace,
            parameters,
            context,
            result: interface.result.clone(),
            result_flush_type: interface.result_flush_type.clone(),
            type_variables: interface.type_variables.clone(),
        });
        Ok(frame)
    }

    fn push_op(
        &mut self,
        op: OwnerResidualOp,
    ) -> Result<OwnerResidualOpId, OwnerBodyInferenceError> {
        let id = checked_u32(self.ops.len(), "owner residual op count")?;
        self.ops.push(Some(op));
        Ok(id)
    }

    fn reserve_op(&mut self) -> Result<OwnerResidualOpId, OwnerBodyInferenceError> {
        let id = checked_u32(self.ops.len(), "owner residual op count")?;
        self.ops.push(None);
        Ok(id)
    }

    fn dependency_owner(
        &self,
        dependency: u32,
        owner: &StableCheckOwnerKey,
    ) -> Result<(&OwnerInterfaceTransferModule, usize), OwnerBodyInferenceError> {
        let module = self.dependencies.get(dependency as usize).ok_or_else(|| {
            OwnerBodyInferenceError::new("owner residual references a missing dependency module")
        })?;
        let owner_index = module
            .result
            .key
            .members
            .binary_search(owner)
            .map_err(|_| {
                OwnerBodyInferenceError::new(format!(
                    "owner residual dependency {:?} does not own {owner:?}",
                    module.key
                ))
            })?;
        Ok((module, owner_index))
    }

    fn compile_reference(
        &mut self,
        frame: OwnerResidualFrameId,
        local_ops: &BTreeMap<StableExpressionKey, OwnerResidualOpId>,
        reference: &OwnerResidualExpressionRef,
    ) -> Result<OwnerResidualOpId, OwnerBodyInferenceError> {
        match reference {
            OwnerResidualExpressionRef::Local { expression } => {
                local_ops.get(expression).copied().ok_or_else(|| {
                    OwnerBodyInferenceError::new(format!(
                        "owner residual references missing local expression {expression:?}"
                    ))
                })
            }
            OwnerResidualExpressionRef::Child { owner, .. } => {
                if let Some(op) = self.surfaces.get(&(frame, owner.clone())).copied() {
                    return Ok(op);
                }
                let (namespace, flow_type) = match self.routes.get(owner).copied() {
                    Some(OwnerInterfaceTransferRoute::Own) => {
                        let interface = self.result.owner(owner).ok_or_else(|| {
                            OwnerBodyInferenceError::new(format!(
                                "owner residual lost same-component child {owner:?}"
                            ))
                        })?;
                        let namespace = self
                            .frames
                            .get(frame as usize)
                            .ok_or_else(|| {
                                OwnerBodyInferenceError::new("owner residual child has no frame")
                            })?
                            .namespace;
                        (namespace, interface.result.clone())
                    }
                    Some(OwnerInterfaceTransferRoute::Dependency(dependency)) => {
                        let (module, owner_index) = self.dependency_owner(dependency, owner)?;
                        let flow_type = module.result.owners[owner_index].result.clone();
                        let key = (frame, dependency);
                        let namespace = if let Some(namespace) =
                            self.dependency_surface_namespaces.get(&key).copied()
                        {
                            namespace
                        } else {
                            let namespace =
                                self.push_namespace(module.result.type_variable_count)?;
                            self.dependency_surface_namespaces.insert(key, namespace);
                            namespace
                        };
                        (namespace, flow_type)
                    }
                    None => {
                        return Err(OwnerBodyInferenceError::new(format!(
                            "owner residual references undeclared child owner {owner:?}"
                        )));
                    }
                };
                let op = self.push_op(OwnerResidualOp {
                    frame,
                    fallback: flow_type,
                    static_number: None,
                    kind: OwnerResidualOpKind::Surface { namespace },
                })?;
                self.surfaces.insert((frame, owner.clone()), op);
                Ok(op)
            }
        }
    }

    fn compile_call(
        &mut self,
        frame: OwnerResidualFrameId,
        node: &OwnerResidualNode,
        local_ops: &BTreeMap<StableExpressionKey, OwnerResidualOpId>,
    ) -> Result<OwnerResidualOpKind, OwnerBodyInferenceError> {
        let mut actuals = node
            .inputs
            .iter()
            .filter_map(|input| {
                input
                    .formal_ordinal
                    .map(|ordinal| (ordinal, &input.expression))
            })
            .map(|(ordinal, expression)| {
                Ok((
                    ordinal,
                    self.compile_reference(frame, local_ops, expression)?,
                ))
            })
            .collect::<Result<BTreeMap<_, _>, OwnerBodyInferenceError>>()?;
        let explicit_context = node
            .inputs
            .iter()
            .find(|input| input.explicit_pass)
            .map(|input| self.compile_reference(frame, local_ops, &input.expression))
            .transpose()?;
        match node.call_target.as_ref() {
            Some(OwnerResidualCallTarget::Owner { owner }) => {
                let context = explicit_context
                    .map_or(OwnerResidualCallContext::Inherited, |value| {
                        OwnerResidualCallContext::Explicit { value }
                    });
                let target = match self.routes.get(owner).copied() {
                    Some(OwnerInterfaceTransferRoute::Own) => {
                        let owner_index =
                            self.result.key.members.binary_search(owner).map_err(|_| {
                                OwnerBodyInferenceError::new(format!(
                                    "owner residual call lost same-component target {owner:?}"
                                ))
                            })?;
                        OwnerResidualCompiledCallTarget::Own {
                            owner: checked_u32(
                                owner_index,
                                "owner residual same-component owner count",
                            )?,
                        }
                    }
                    Some(OwnerInterfaceTransferRoute::Dependency(dependency)) => {
                        let (_, owner_index) = self.dependency_owner(dependency, owner)?;
                        OwnerResidualCompiledCallTarget::Dependency {
                            dependency,
                            owner: checked_u32(
                                owner_index,
                                "owner residual dependency owner count",
                            )?,
                        }
                    }
                    None => {
                        return Err(OwnerBodyInferenceError::new(format!(
                            "owner residual call references undeclared owner {owner:?}"
                        )));
                    }
                };
                Ok(OwnerResidualOpKind::CompiledCall {
                    target,
                    actuals: std::mem::take(&mut actuals)
                        .into_iter()
                        .map(|(formal_ordinal, value)| OwnerResidualActual {
                            formal_ordinal,
                            value,
                        })
                        .collect::<Vec<_>>()
                        .into_boxed_slice(),
                    context,
                })
            }
            Some(OwnerResidualCallTarget::Abi {
                canonical_name,
                contract,
            }) => Ok(OwnerResidualOpKind::AbiCall {
                canonical_name: canonical_name.clone(),
                contract: contract.clone(),
                actuals: std::mem::take(&mut actuals)
                    .into_iter()
                    .map(|(formal_ordinal, value)| OwnerResidualActual {
                        formal_ordinal,
                        value,
                    })
                    .collect::<Vec<_>>()
                    .into_boxed_slice(),
            }),
            Some(
                OwnerResidualCallTarget::Unresolved | OwnerResidualCallTarget::Ambiguous { .. },
            )
            | None => Ok(OwnerResidualOpKind::Fallback),
        }
    }

    fn compile_node(
        &mut self,
        frame: OwnerResidualFrameId,
        node: &OwnerResidualNode,
        source_nodes: &BTreeMap<StableExpressionKey, &OwnerResidualNode>,
        local_ops: &BTreeMap<StableExpressionKey, OwnerResidualOpId>,
    ) -> Result<OwnerResidualOp, OwnerBodyInferenceError> {
        let reference = |builder: &mut Self, input: &OwnerResidualInput| {
            builder.compile_reference(frame, local_ops, &input.expression)
        };
        let find = |builder: &mut Self,
                    role: fn(&OwnerConstraintEdgeRole) -> bool|
         -> Result<Option<OwnerResidualOpId>, OwnerBodyInferenceError> {
            node.inputs
                .iter()
                .find(|input| role(&input.role))
                .map(|input| reference(builder, input))
                .transpose()
        };
        let kind = if let Some(read) = &node.parameter_read {
            OwnerResidualOpKind::ParameterRead {
                parameter_ordinal: read.parameter_ordinal,
                projection: read.projection.clone(),
            }
        } else if let OwnerConstraintNodeKind::Reference { parts }
        | OwnerConstraintNodeKind::Drain { parts } = &node.kind
        {
            OwnerResidualOpKind::LexicalRead {
                parts: parts.clone(),
            }
        } else {
            match &node.kind {
                OwnerConstraintNodeKind::Call { .. } | OwnerConstraintNodeKind::Pipe { .. } => {
                    self.compile_call(frame, node, local_ops)?
                }
                OwnerConstraintNodeKind::Infix { operation } => {
                    let Some(left) = find(self, |role| {
                        matches!(role, OwnerConstraintEdgeRole::InfixLeft)
                    })?
                    else {
                        return Ok(OwnerResidualOp {
                            frame,
                            fallback: node.flow_type.clone(),
                            static_number: None,
                            kind: OwnerResidualOpKind::Fallback,
                        });
                    };
                    let Some(right) = find(self, |role| {
                        matches!(role, OwnerConstraintEdgeRole::InfixRight)
                    })?
                    else {
                        return Ok(OwnerResidualOp {
                            frame,
                            fallback: node.flow_type.clone(),
                            static_number: None,
                            kind: OwnerResidualOpKind::Fallback,
                        });
                    };
                    OwnerResidualOpKind::Infix {
                        operation: operation.clone(),
                        left,
                        right,
                    }
                }
                OwnerConstraintNodeKind::Record { tag } => OwnerResidualOpKind::Record {
                    tag: tag.clone(),
                    fields: node
                        .inputs
                        .iter()
                        .filter_map(|input| match &input.role {
                            OwnerConstraintEdgeRole::RecordField {
                                name,
                                spread: false,
                            } => Some((name.clone(), &input.expression)),
                            _ => None,
                        })
                        .map(|(name, expression)| {
                            Ok(OwnerResidualRecordField {
                                name,
                                value: self.compile_reference(frame, local_ops, expression)?,
                            })
                        })
                        .collect::<Result<Vec<_>, OwnerBodyInferenceError>>()?
                        .into_boxed_slice(),
                },
                OwnerConstraintNodeKind::When => {
                    let Some(selector) = find(self, |role| {
                        matches!(role, OwnerConstraintEdgeRole::WhenInput)
                    })?
                    else {
                        return Ok(OwnerResidualOp {
                            frame,
                            fallback: node.flow_type.clone(),
                            static_number: None,
                            kind: OwnerResidualOpKind::Fallback,
                        });
                    };
                    let mut arms = Vec::new();
                    for input in node
                        .inputs
                        .iter()
                        .filter(|input| matches!(input.role, OwnerConstraintEdgeRole::WhenArm))
                    {
                        let OwnerResidualExpressionRef::Local { expression } = &input.expression
                        else {
                            continue;
                        };
                        let Some(arm) = source_nodes.get(expression).copied() else {
                            continue;
                        };
                        let pattern = match &arm.kind {
                            OwnerConstraintNodeKind::MatchArm { pattern }
                            | OwnerConstraintNodeKind::Arrow { pattern } => pattern.clone(),
                            _ => continue,
                        };
                        let Some(output) = arm.inputs.iter().find(|input| {
                            matches!(
                                input.role,
                                OwnerConstraintEdgeRole::MatchOutput
                                    | OwnerConstraintEdgeRole::ArrowOutput
                            )
                        }) else {
                            continue;
                        };
                        arms.push(OwnerResidualWhenArm {
                            pattern,
                            output: self.compile_reference(frame, local_ops, &output.expression)?,
                        });
                    }
                    OwnerResidualOpKind::When {
                        selector,
                        arms: arms.into_boxed_slice(),
                    }
                }
                OwnerConstraintNodeKind::MatchArm { .. }
                | OwnerConstraintNodeKind::Arrow { .. } => OwnerResidualOpKind::Forward {
                    output: node
                        .inputs
                        .iter()
                        .find(|input| {
                            matches!(
                                input.role,
                                OwnerConstraintEdgeRole::MatchOutput
                                    | OwnerConstraintEdgeRole::ArrowOutput
                            )
                        })
                        .map(|input| reference(self, input))
                        .transpose()?,
                },
                OwnerConstraintNodeKind::Block => OwnerResidualOpKind::Block {
                    bindings: node
                        .inputs
                        .iter()
                        .filter_map(|input| match &input.role {
                            OwnerConstraintEdgeRole::BlockBinding { name } => {
                                Some((name.clone(), input))
                            }
                            _ => None,
                        })
                        .map(|(name, input)| {
                            Ok(OwnerResidualBlockBinding {
                                name,
                                value: reference(self, input)?,
                            })
                        })
                        .collect::<Result<Vec<_>, OwnerBodyInferenceError>>()?
                        .into_boxed_slice(),
                    result: find(self, |role| {
                        matches!(role, OwnerConstraintEdgeRole::BlockResult)
                    })?,
                },
                OwnerConstraintNodeKind::Collection { collection, .. } => {
                    OwnerResidualOpKind::Collection {
                        collection: *collection,
                        items: node
                            .inputs
                            .iter()
                            .filter(|input| {
                                matches!(
                                    input.role,
                                    OwnerConstraintEdgeRole::CollectionItem
                                        | OwnerConstraintEdgeRole::MapEntry
                                )
                            })
                            .map(|input| reference(self, input))
                            .collect::<Result<Vec<_>, _>>()?
                            .into_boxed_slice(),
                    }
                }
                OwnerConstraintNodeKind::Latest => OwnerResidualOpKind::Latest {
                    branches: node
                        .inputs
                        .iter()
                        .filter(|input| matches!(input.role, OwnerConstraintEdgeRole::LatestBranch))
                        .map(|input| reference(self, input))
                        .collect::<Result<Vec<_>, _>>()?
                        .into_boxed_slice(),
                },
                OwnerConstraintNodeKind::Draining => OwnerResidualOpKind::Draining {
                    input: node
                        .inputs
                        .first()
                        .map(|input| reference(self, input))
                        .transpose()?,
                },
                OwnerConstraintNodeKind::Hold { .. } => OwnerResidualOpKind::Hold {
                    initial: find(self, |role| {
                        matches!(role, OwnerConstraintEdgeRole::HoldInitial)
                    })?,
                    updates: node
                        .inputs
                        .iter()
                        .filter(|input| matches!(input.role, OwnerConstraintEdgeRole::HoldUpdate))
                        .map(|input| reference(self, input))
                        .collect::<Result<Vec<_>, _>>()?
                        .into_boxed_slice(),
                },
                OwnerConstraintNodeKind::Then => OwnerResidualOpKind::Then {
                    value: find(self, |role| {
                        matches!(role, OwnerConstraintEdgeRole::ThenOutput)
                    })?
                    .or(find(self, |role| {
                        matches!(role, OwnerConstraintEdgeRole::ThenInput)
                    })?),
                },
                OwnerConstraintNodeKind::MapEntry => OwnerResidualOpKind::MapEntry {
                    key: find(self, |role| matches!(role, OwnerConstraintEdgeRole::MapKey))?,
                    value: find(self, |role| {
                        matches!(role, OwnerConstraintEdgeRole::MapValue)
                    })?,
                },
                OwnerConstraintNodeKind::Source => OwnerResidualOpKind::Source,
                OwnerConstraintNodeKind::Tag { name } if name == "SKIP" => {
                    OwnerResidualOpKind::Skip
                }
                _ => OwnerResidualOpKind::Fallback,
            }
        };
        let static_number = node
            .static_number
            .as_deref()
            .map(|literal| {
                ExactNumber::parse_strict(literal, None).map_err(|error| {
                    OwnerBodyInferenceError::new(format!(
                        "owner residual has invalid static number {literal:?}: {error}"
                    ))
                })
            })
            .transpose()?;
        Ok(OwnerResidualOp {
            frame,
            fallback: node.flow_type.clone(),
            static_number,
            kind,
        })
    }

    fn compile_owner_root(
        &mut self,
        owner: usize,
    ) -> Result<OwnerResidualRoot, OwnerBodyInferenceError> {
        let interface = self.result.owners.get(owner).ok_or_else(|| {
            OwnerBodyInferenceError::new("owner residual program lost its public interface")
        })?;
        let draft = self.drafts.get(owner).ok_or_else(|| {
            OwnerBodyInferenceError::new("owner residual program lost its unsealed draft")
        })?;
        let namespace = self.push_namespace(self.residual_type_variable_count)?;
        let frame = self.push_frame(interface, namespace)?;
        Ok(match draft {
            OwnerResidualDraft::Principal => OwnerResidualRoot::Principal { frame },
            OwnerResidualDraft::Parameter { read } => OwnerResidualRoot::Parameter {
                frame,
                read: read.clone(),
            },
            OwnerResidualDraft::Expression { root, nodes } => {
                let mut local_ops = BTreeMap::new();
                for node in nodes {
                    let op = self.reserve_op()?;
                    if local_ops.insert(node.expression.clone(), op).is_some() {
                        return Err(OwnerBodyInferenceError::new(format!(
                            "owner residual for {:?} repeats a local expression",
                            interface.owner
                        )));
                    }
                }
                let source_nodes = nodes
                    .iter()
                    .map(|node| (node.expression.clone(), node))
                    .collect::<BTreeMap<_, _>>();
                for node in nodes {
                    let op = local_ops[&node.expression] as usize;
                    let compiled = self.compile_node(frame, node, &source_nodes, &local_ops)?;
                    if self.ops[op].replace(compiled).is_some() {
                        return Err(OwnerBodyInferenceError::new(
                            "owner residual compiler filled one op twice",
                        ));
                    }
                }
                OwnerResidualRoot::Value {
                    frame,
                    op: self.compile_reference(frame, &local_ops, root)?,
                }
            }
        })
    }

    fn compile_owner(
        mut self,
        owner: usize,
    ) -> Result<OwnerResidualOwnerProgram, OwnerBodyInferenceError> {
        let interface = self.result.owners.get(owner).ok_or_else(|| {
            OwnerBodyInferenceError::new("owner residual program lost its public interface")
        })?;
        let draft = self.drafts.get(owner).ok_or_else(|| {
            OwnerBodyInferenceError::new("owner residual program lost its unsealed draft")
        })?;
        let invocation_invariant = owner_result_transfer_is_invocation_invariant(interface, draft);
        let root = self.compile_owner_root(owner)?;
        let ops = self
            .ops
            .into_iter()
            .enumerate()
            .map(|(index, op)| {
                op.ok_or_else(|| {
                    OwnerBodyInferenceError::new(format!(
                        "owner residual compiler left op {index} unsealed"
                    ))
                })
            })
            .collect::<Result<Vec<_>, _>>()?
            .into_boxed_slice();
        Ok(OwnerResidualOwnerProgram {
            namespaces: self.namespaces.into_boxed_slice(),
            frames: self.frames.into_boxed_slice(),
            ops,
            root,
            invocation_invariant,
        })
    }
}

fn compile_owner_residual_program(
    result: &OwnerInterfaceSccResult,
    drafts: &[OwnerResidualDraft],
    residual_type_variable_count: u32,
    dependencies: &[Arc<OwnerInterfaceTransferModule>],
    routes: &BTreeMap<StableCheckOwnerKey, OwnerInterfaceTransferRoute>,
) -> Result<OwnerResidualProgram, OwnerBodyInferenceError> {
    let owners = (0..result.owners.len())
        .map(|owner| {
            OwnerResidualProgramBuilder::new(
                result,
                drafts,
                residual_type_variable_count,
                dependencies,
                routes,
            )
            .compile_owner(owner)
        })
        .collect::<Result<Vec<_>, _>>()?
        .into_boxed_slice();
    let op_count = owners.iter().map(|owner| owner.ops.len() as u64).sum();
    let edge_count = owners
        .iter()
        .flat_map(|owner| owner.ops.iter())
        .map(|op| op.kind.edge_count())
        .sum();
    let fingerprint_v1 = fingerprint(OWNER_RESIDUAL_PROGRAM_DOMAIN_V3, &owners)?;
    Ok(OwnerResidualProgram {
        owners,
        op_count,
        edge_count,
        fingerprint_v1,
    })
}

fn precompute_owner_interface_transfer_constants(module: &mut OwnerInterfaceTransferModule) {
    let candidates = module
        .program
        .owners
        .iter()
        .enumerate()
        .filter(|(_, program)| program.invocation_invariant)
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    let mut constants = vec![None; module.result.owners.len()];
    for owner in candidates {
        let mut unifier = TypeUnifier::default();
        let providers = BTreeMap::new();
        let mut evaluator = CompiledOwnerResidualEvaluator::new(&providers, &mut unifier);
        let owner_key = module
            .result
            .key
            .members
            .get(owner)
            .expect("candidate owner exists");
        let Some(result) = evaluator.evaluate_owner_in_module(
            module,
            owner_key,
            &OwnerResidualDraftArguments::default(),
            None,
        ) else {
            continue;
        };
        let mut variables = BTreeSet::new();
        crate::collect_type_vars(&result.value.flow_type.ty, &mut variables);
        if variables.is_empty() {
            constants[owner] = Some(result.value);
        }
    }
    module.constant_results = constants.into_boxed_slice();
}

fn seal_owner_interface_transfer_module(
    result: Arc<OwnerInterfaceSccResult>,
    drafts: Box<[OwnerResidualDraft]>,
    residual_type_variable_count: u32,
    expected_dependencies: &[OwnerInterfaceSccKey],
    dependencies: impl IntoIterator<Item = Arc<OwnerInterfaceTransferModule>>,
) -> Result<OwnerInterfaceTransferModule, OwnerBodyInferenceError> {
    let trace =
        std::env::var_os("BOON_OWNER_REQUEST_TRACE").is_some() && result.key.members.len() >= 100;
    let total_started = Instant::now();
    if drafts.len() != result.owners.len() {
        return Err(OwnerBodyInferenceError::new(format!(
            "interface transfer module {:?} has {} residual drafts for {} owners",
            result.key,
            drafts.len(),
            result.owners.len()
        )));
    }
    if result
        .owners
        .iter()
        .map(|interface| &interface.owner)
        .ne(result.key.members.iter())
    {
        return Err(OwnerBodyInferenceError::new(format!(
            "interface transfer module {:?} has a non-canonical member table",
            result.key
        )));
    }
    let mut dependencies = dependencies
        .into_iter()
        .map(|dependency| (dependency.key.clone(), dependency))
        .collect::<BTreeMap<_, _>>();
    if dependencies.len() != expected_dependencies.len()
        || dependencies.keys().ne(expected_dependencies.iter())
    {
        return Err(OwnerBodyInferenceError::new(format!(
            "interface transfer module {:?} received the wrong direct dependencies",
            result.key
        )));
    }
    let dependencies = expected_dependencies
        .iter()
        .map(|key| {
            dependencies.remove(key).ok_or_else(|| {
                OwnerBodyInferenceError::new(format!(
                    "interface transfer module {:?} lost dependency {key:?}",
                    result.key
                ))
            })
        })
        .collect::<Result<Vec<_>, _>>()?
        .into_boxed_slice();
    let mut routes = BTreeMap::new();
    for owner in &result.key.members {
        if routes
            .insert(owner.clone(), OwnerInterfaceTransferRoute::Own)
            .is_some()
        {
            return Err(OwnerBodyInferenceError::new(
                "interface transfer module has duplicate own member",
            ));
        }
    }
    for (index, dependency) in dependencies.iter().enumerate() {
        let index = u32::try_from(index).map_err(|_| {
            OwnerBodyInferenceError::new(
                "interface transfer module dependency count exceeds the u32 bound",
            )
        })?;
        for owner in &dependency.key.members {
            if routes
                .insert(
                    owner.clone(),
                    OwnerInterfaceTransferRoute::Dependency(index),
                )
                .is_some()
            {
                return Err(OwnerBodyInferenceError::new(format!(
                    "interface transfer module {:?} has overlapping dependency member {owner:?}",
                    result.key
                )));
            }
        }
    }
    let mut used_dependencies = BTreeSet::new();
    for (interface, draft) in result.owners.iter().zip(drafts.iter()) {
        if !owner_result_transfer_interface_variables_are_complete(
            interface,
            draft,
            residual_type_variable_count,
        ) {
            return Err(OwnerBodyInferenceError::new(format!(
                "interface transfer for {:?} has an incomplete alpha-variable namespace",
                interface.owner
            )));
        }
        for dependency in owner_result_transfer_dependencies(draft) {
            match routes.get(&dependency) {
                Some(OwnerInterfaceTransferRoute::Own) => {}
                Some(OwnerInterfaceTransferRoute::Dependency(index)) => {
                    used_dependencies.insert(*index);
                }
                None => {
                    return Err(OwnerBodyInferenceError::new(format!(
                        "interface transfer for {:?} references non-direct dependency {dependency:?}",
                        interface.owner
                    )));
                }
            }
        }
    }
    if used_dependencies.len() != dependencies.len() {
        return Err(OwnerBodyInferenceError::new(format!(
            "interface transfer module {:?} received an unused dependency",
            result.key
        )));
    }
    let dependency_fingerprints = dependencies
        .iter()
        .map(|dependency| (&dependency.key, dependency.fingerprint_v1))
        .collect::<Vec<_>>();
    let program_started = Instant::now();
    let program = Arc::new(compile_owner_residual_program(
        &result,
        &drafts,
        residual_type_variable_count,
        &dependencies,
        &routes,
    )?);
    let program_ms = program_started.elapsed().as_secs_f64() * 1_000.0;
    let fingerprint_started = Instant::now();
    let fingerprint_v1 = fingerprint(
        OWNER_INTERFACE_TRANSFER_MODULE_DOMAIN_V5,
        &(
            &result.key,
            result.fingerprint_v1(),
            program.fingerprint_v1,
            dependency_fingerprints,
        ),
    )?;
    let fingerprint_ms = fingerprint_started.elapsed().as_secs_f64() * 1_000.0;
    let mut module = OwnerInterfaceTransferModule {
        key: result.key.clone(),
        result,
        dependencies,
        program,
        constant_results: Box::new([]),
        fingerprint_v1,
    };
    let constants_started = Instant::now();
    precompute_owner_interface_transfer_constants(&mut module);
    let constants_ms = constants_started.elapsed().as_secs_f64() * 1_000.0;
    if trace {
        eprintln!(
            "boon owner transfer module members={} program_ms={program_ms:.3} fingerprint_ms={fingerprint_ms:.3} constants_ms={constants_ms:.3} total_ms={:.3}",
            module.key.members.len(),
            total_started.elapsed().as_secs_f64() * 1_000.0,
        );
    }
    Ok(module)
}

pub(crate) fn project_owner_interface_transfer_module(
    scc: &OwnerInterfaceScc,
    projection: OwnerInterfaceSccProjection,
    dependencies: impl IntoIterator<Item = Arc<OwnerInterfaceTransferModule>>,
) -> Result<OwnerInterfaceTransferModule, OwnerBodyInferenceError> {
    let OwnerInterfaceSccProjection {
        result,
        residuals,
        residual_type_variable_count,
    } = projection;
    if result.key != scc.key {
        return Err(OwnerBodyInferenceError::new(format!(
            "interface transfer module expected {:?}, got {:?}",
            scc.key, result.key
        )));
    }
    let topology_dependencies = scc.dependencies.iter().collect::<BTreeSet<_>>();
    let mut dependencies_by_key = BTreeMap::new();
    for dependency in dependencies {
        match dependencies_by_key.entry(dependency.key.clone()) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(dependency);
            }
            std::collections::btree_map::Entry::Occupied(entry)
                if entry.get().fingerprint_v1() == dependency.fingerprint_v1() => {}
            std::collections::btree_map::Entry::Occupied(_) => {
                return Err(OwnerBodyInferenceError::new(format!(
                    "interface transfer module {:?} received conflicting versions of one dependency",
                    scc.key
                )));
            }
        }
    }
    if dependencies_by_key
        .keys()
        .any(|dependency| !topology_dependencies.contains(dependency))
    {
        return Err(OwnerBodyInferenceError::new(format!(
            "interface transfer module {:?} received a dependency outside its interface topology",
            scc.key
        )));
    }
    let dependency_keys = dependencies_by_key.keys().cloned().collect::<Vec<_>>();
    let dependencies = dependencies_by_key.into_values().collect::<Vec<_>>();
    seal_owner_interface_transfer_module(
        result,
        residuals,
        residual_type_variable_count,
        &dependency_keys,
        dependencies,
    )
}

pub(crate) fn owner_interface_transfer_dependency_owners(
    projection: &OwnerInterfaceSccProjection,
) -> Box<[StableCheckOwnerKey]> {
    projection
        .residuals
        .iter()
        .flat_map(owner_result_transfer_dependencies)
        .filter(|owner| projection.result.key.members.binary_search(owner).is_err())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>()
        .into_boxed_slice()
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OwnerBodyInterfaceSccPlan {
    #[serde(skip)]
    key: OwnerInterfaceSccKey,
    #[serde(skip)]
    module: Arc<OwnerInterfaceTransferModule>,
    key_fingerprint_v1: [u8; 32],
    module_fingerprint_v1: [u8; 32],
    /// Sorted exact member indices in `key.members` used by this body.
    /// Stable owner keys remain owned once by the SCC key instead of being
    /// copied into every importing body plan.
    referenced_members: Box<[u32]>,
}

impl OwnerBodyInterfaceSccPlan {
    pub fn key(&self) -> &OwnerInterfaceSccKey {
        &self.key
    }

    pub fn referenced_owners(&self) -> impl Iterator<Item = &StableCheckOwnerKey> {
        self.referenced_members
            .iter()
            .map(|index| &self.key.members[*index as usize])
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct OwnerBodyInterfacePlanWork {
    pub direct_owners: u64,
    pub required_owners: u64,
    pub provider_sccs: u64,
    pub result_transfers: u64,
    pub result_transfer_nodes: u64,
    pub result_transfer_edges: u64,
}

/// Exact immutable public-interface demand for one owner body.
///
/// The plan is discovered once from direct syntax imports. Each provider is a
/// shared transfer module whose dependency DAG owns transitive result
/// specialization, so individual bodies never flatten that closure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OwnerBodyInterfacePlan {
    owner: StableCheckOwnerKey,
    own_scc: OwnerBodyInterfaceSccPlan,
    imports: Box<[OwnerBodyInterfaceSccPlan]>,
    work: OwnerBodyInterfacePlanWork,
    fingerprint_v1: [u8; 32],
}

impl OwnerBodyInterfacePlan {
    pub fn owner(&self) -> &StableCheckOwnerKey {
        &self.owner
    }

    pub fn own_scc(&self) -> &OwnerBodyInterfaceSccPlan {
        &self.own_scc
    }

    pub fn imports(&self) -> &[OwnerBodyInterfaceSccPlan] {
        &self.imports
    }

    pub fn sccs(&self) -> impl Iterator<Item = &OwnerBodyInterfaceSccPlan> {
        std::iter::once(&self.own_scc).chain(self.imports.iter())
    }

    pub fn required_owner_count(&self) -> usize {
        self.sccs().map(|scc| scc.referenced_members.len()).sum()
    }

    pub const fn work(&self) -> OwnerBodyInterfacePlanWork {
        self.work
    }

    pub const fn fingerprint_v1(&self) -> [u8; 32] {
        self.fingerprint_v1
    }
}

/// Stateful demand walker used by typed request evaluators.
///
/// Callers ask for [`next_required_owner`](Self::next_required_owner), require
/// that owner's current provider SCC through their request graph, and feed the
/// result to [`provide_interface_module`](Self::provide_interface_module).
/// This keeps direct dependency discovery in the typechecker while allowing
/// the evaluator to record exact dynamic request edges.
pub struct OwnerBodyInterfacePlanner {
    owner: StableCheckOwnerKey,
    required: BTreeSet<StableCheckOwnerKey>,
    pending: VecDeque<StableCheckOwnerKey>,
    provider_sccs: Vec<Arc<OwnerInterfaceTransferModule>>,
    providers: BTreeMap<StableCheckOwnerKey, usize>,
    work: OwnerBodyInterfacePlanWork,
}

impl OwnerBodyInterfacePlanner {
    pub fn new(
        seed: &OwnerConstraintSeed,
        summary: &OwnerConstraintSummary,
    ) -> Result<Self, OwnerBodyInferenceError> {
        if summary.owner != seed.owner || summary.seed_fingerprint_v1 != seed.fingerprint_v1() {
            return Err(OwnerBodyInferenceError::new(
                "owner body interface planning has mismatched seed and summary",
            ));
        }
        let required = directly_required_interface_owners(seed, summary);
        let pending = required.iter().cloned().collect::<VecDeque<_>>();
        let direct_owners = required.len() as u64;
        Ok(Self {
            owner: seed.owner.clone(),
            required,
            pending,
            provider_sccs: Vec::new(),
            providers: BTreeMap::new(),
            work: OwnerBodyInterfacePlanWork {
                direct_owners,
                ..OwnerBodyInterfacePlanWork::default()
            },
        })
    }

    pub fn next_required_owner(&self) -> Option<&StableCheckOwnerKey> {
        self.pending.front()
    }

    pub fn provide_interface_module(
        &mut self,
        module: Arc<OwnerInterfaceTransferModule>,
    ) -> Result<(), OwnerBodyInferenceError> {
        let owner = self.pending.pop_front().ok_or_else(|| {
            OwnerBodyInferenceError::new("owner body interface planner received an extra module")
        })?;
        let interface = module.result().owner(&owner).ok_or_else(|| {
            OwnerBodyInferenceError::new(format!(
                "owner body interface planner expected provider for {owner:?}, got {:?}",
                module.key()
            ))
        })?;
        let provider = self
            .provider_sccs
            .iter()
            .position(|candidate| candidate.key() == module.key())
            .unwrap_or_else(|| {
                self.provider_sccs.push(Arc::clone(&module));
                self.provider_sccs.len() - 1
            });
        if self.providers.insert(owner.clone(), provider).is_some() {
            return Err(OwnerBodyInferenceError::new(format!(
                "owner body interface planner received {owner:?} twice"
            )));
        }
        self.work.result_transfers = self.work.result_transfers.saturating_add(1);
        let (nodes, edges) = module
            .residual_work_for_owner(&interface.owner)
            .ok_or_else(|| {
                OwnerBodyInferenceError::new(format!(
                    "owner body interface module {:?} has no residual program for {:?}",
                    module.key(),
                    interface.owner
                ))
            })?;
        self.work.result_transfer_nodes = self.work.result_transfer_nodes.saturating_add(nodes);
        self.work.result_transfer_edges = self.work.result_transfer_edges.saturating_add(edges);
        Ok(())
    }

    pub fn finish(mut self) -> Result<OwnerBodyInterfacePlan, OwnerBodyInferenceError> {
        if !self.pending.is_empty() || self.providers.len() != self.required.len() {
            return Err(OwnerBodyInferenceError::new(format!(
                "owner body interface plan for {:?} is incomplete",
                self.owner
            )));
        }
        let own_provider = self.providers.get(&self.owner).copied().ok_or_else(|| {
            OwnerBodyInferenceError::new(format!(
                "owner body interface plan for {:?} has no own provider",
                self.owner
            ))
        })?;
        let mut owners_by_scc = BTreeMap::<usize, Vec<StableCheckOwnerKey>>::new();
        for (owner, provider) in self.providers {
            owners_by_scc.entry(provider).or_default().push(owner);
        }
        let provider_sccs = self.provider_sccs;
        let own_referenced_owners = owners_by_scc.remove(&own_provider).ok_or_else(|| {
            OwnerBodyInferenceError::new("owner body interface plan lost its own SCC")
        })?;
        let seal_scc = |provider: usize,
                        referenced_owners: Vec<StableCheckOwnerKey>|
         -> Result<OwnerBodyInterfaceSccPlan, OwnerBodyInferenceError> {
            let module = provider_sccs.get(provider).cloned().ok_or_else(|| {
                OwnerBodyInferenceError::new("owner body interface plan lost a provider SCC")
            })?;
            let key = module.key().clone();
            let key_fingerprint_v1 = module.result().key_fingerprint_v1();
            let module_fingerprint_v1 = module.fingerprint_v1();
            let referenced_members = referenced_owners
                .iter()
                .map(|owner| {
                    key.members
                        .binary_search(owner)
                        .map_err(|_| {
                            OwnerBodyInferenceError::new(format!(
                                "owner body interface provider {:?} does not contain {owner:?}",
                                key
                            ))
                        })
                        .and_then(|index| {
                            u32::try_from(index).map_err(|_| {
                                OwnerBodyInferenceError::new(
                                    "owner body interface SCC exceeds the u32 member bound",
                                )
                            })
                        })
                })
                .collect::<Result<Vec<_>, _>>()?;
            Ok(OwnerBodyInterfaceSccPlan {
                key,
                module,
                key_fingerprint_v1,
                module_fingerprint_v1,
                referenced_members: referenced_members.into_boxed_slice(),
            })
        };
        let own_scc = seal_scc(own_provider, own_referenced_owners)?;
        let mut imports = owners_by_scc
            .into_iter()
            .map(|(provider, referenced_owners)| seal_scc(provider, referenced_owners))
            .collect::<Result<Vec<_>, _>>()?;
        imports.sort_by(|left, right| left.key.cmp(&right.key));
        let imports = imports.into_boxed_slice();
        self.work.required_owners = self.required.len() as u64;
        self.work.provider_sccs = imports.len() as u64 + 1;
        let fingerprint_v1 = fingerprint(
            OWNER_BODY_INTERFACE_PLAN_DOMAIN_V5,
            &(&self.owner, &own_scc, &imports),
        )?;
        Ok(OwnerBodyInterfacePlan {
            owner: self.owner,
            own_scc,
            imports,
            work: self.work,
            fingerprint_v1,
        })
    }
}

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
    /// Index into the currentness basis' canonical SCC sequence: own SCC
    /// first, followed by sorted imports. Provider identity and its complete
    /// currentness seal are stored once in that basis instead of being copied
    /// into every imported owner row.
    pub provider_scc: u32,
}

/// Frozen identity of one interface SCC consumed by owner-local inference.
///
/// `referenced_members` is the exact subset used by this owner, indexed into
/// the runtime key. The key itself is retained for request routing but omitted
/// from the compact currentness encoding; its content digest is sealed once.
/// The full SCC result fingerprint and its alpha namespace remain attached so
/// a cache hit cannot accidentally combine same-numbered `TypeVar`s from
/// another SCC.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct FrozenOwnerInterfaceSccRef {
    #[serde(skip)]
    pub key: OwnerInterfaceSccKey,
    pub key_fingerprint_v1: [u8; 32],
    pub result_fingerprint_v1: [u8; 32],
    pub transfer_module_fingerprint_v1: [u8; 32],
    pub type_variable_count: u32,
    pub referenced_members: Box<[u32]>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OwnerBodyInferenceBasis {
    pub owner: StableCheckOwnerKey,
    pub syntax_fingerprint_v1: [u8; 32],
    pub lexical_plan_fingerprint_v1: [u8; 32],
    pub signature_lexical_plan_fingerprint_v1: [u8; 32],
    pub seed_fingerprint_v1: [u8; 32],
    pub summary_fingerprint_v1: [u8; 32],
    pub own_scc: FrozenOwnerInterfaceSccRef,
    pub imports: Box<[FrozenOwnerInterfaceSccRef]>,
    /// Compact exact seal of the provider modules and referenced members from
    /// which `own_scc`, `imports`, and the per-interface import table were
    /// frozen. Rich rows remain available for direct validation.
    pub interface_plan_fingerprint_v1: [u8; 32],
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
    /// Exact source type captured before this consumer's contract can widen
    /// shared inference roots. Diagnostics and semantic projections use this
    /// fact instead of attempting to reconstruct a pre-call type from the
    /// finalized expression row.
    pub actual_type: Type,
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
    pub interface_plan_direct_owners: u64,
    pub interface_plan_required_owners: u64,
    pub interface_plan_provider_sccs: u64,
    pub interface_plan_result_transfers: u64,
    pub interface_plan_transfer_nodes: u64,
    pub interface_plan_transfer_edges: u64,
    pub calls: u64,
    pub unification_steps: u64,
}

impl OwnerBodyInferenceWork {
    fn accumulate(&mut self, other: Self) {
        self.statements = self.statements.saturating_add(other.statements);
        self.expressions = self.expressions.saturating_add(other.expressions);
        self.local_constraints = self
            .local_constraints
            .saturating_add(other.local_constraints);
        self.interface_imports = self
            .interface_imports
            .saturating_add(other.interface_imports);
        self.interface_plan_direct_owners = self
            .interface_plan_direct_owners
            .saturating_add(other.interface_plan_direct_owners);
        self.interface_plan_required_owners = self
            .interface_plan_required_owners
            .saturating_add(other.interface_plan_required_owners);
        self.interface_plan_provider_sccs = self
            .interface_plan_provider_sccs
            .saturating_add(other.interface_plan_provider_sccs);
        self.interface_plan_result_transfers = self
            .interface_plan_result_transfers
            .saturating_add(other.interface_plan_result_transfers);
        self.interface_plan_transfer_nodes = self
            .interface_plan_transfer_nodes
            .saturating_add(other.interface_plan_transfer_nodes);
        self.interface_plan_transfer_edges = self
            .interface_plan_transfer_edges
            .saturating_add(other.interface_plan_transfer_edges);
        self.calls = self.calls.saturating_add(other.calls);
        self.unification_steps = self
            .unification_steps
            .saturating_add(other.unification_steps);
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OwnerBodyInferenceReceipt {
    pub statement_rows: u32,
    pub child_rows: u32,
    pub expression_rows: u32,
    pub call_rows: u32,
    pub relocation_rows: u32,
    pub diagnostic_rows: u32,
    pub diagnostic_facts_fingerprint_v1: [u8; 32],
    pub signature_lexical_plan_fingerprint_v1: [u8; 32],
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
    /// Span-free diagnostic/global-reducer contribution emitted atomically by
    /// owner evaluation. Downstream requests project this authenticated fact;
    /// they do not rescan the owner body to reconstruct it.
    pub diagnostic_facts: crate::OwnerDiagnosticContribution,
    pub signature_lexical_plan: OwnerSignatureLexicalPlan,
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
        // Keep the rich basis and per-interface table available to direct
        // consumers, but seal their already-canonical interface-plan identity.
        // The plan commits every provider module, exact referenced member, and
        // result/type-variable surface without serializing that DAG again.
        let compact_currentness = (
            basis.syntax_fingerprint_v1,
            basis.lexical_plan_fingerprint_v1,
            basis.signature_lexical_plan_fingerprint_v1,
            basis.seed_fingerprint_v1,
            basis.summary_fingerprint_v1,
            basis.inference_abi_fingerprint_v1,
            basis.interface_plan_fingerprint_v1,
            result_fingerprint_v1,
        );
        let fingerprint_v1 = fingerprint(
            OWNER_BODY_INFERENCE_CURRENTNESS_DOMAIN_V13,
            &compact_currentness,
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
    Ok(interface.fingerprint_v1())
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
                source_map.owner(),
                diagnostic.code
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
                .statements()
                .get(*statement as usize)
                .filter(|source| source.statement == *statement)
                .ok_or_else(|| {
                    OwnerBodyInferenceError::new(format!(
                        "owner {:?} diagnostic {} references missing statement {}",
                        source_map.owner(),
                        diagnostic.code,
                        statement
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
                .expressions()
                .iter()
                .find(|source| &source.expression == expression)
                .ok_or_else(|| {
                    OwnerBodyInferenceError::new(format!(
                        "owner {:?} diagnostic {} references missing expression",
                        source_map.owner(),
                        diagnostic.code
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
    if shard.owner() != source_map.owner() {
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

type SourceUnitOwnerDiagnosticBasis = (StableCheckOwnerKey, [u8; 32], [u8; 32]);

/// Unit-local presentation of span-free owner diagnostic templates.
///
/// Rows keep physical lines and byte offsets local to one source unit. The
/// project receipt is solely responsible for applying project layout offsets,
/// so an edit in an earlier source unit cannot force unaffected units to
/// rematerialize their owner diagnostics.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceUnitOwnerDiagnostics {
    source_unit_id: SourceUnitId,
    basis: Box<[SourceUnitOwnerDiagnosticBasis]>,
    owner_count: u32,
    expression_count: u32,
    call_count: u32,
    work: OwnerBodyInferenceWork,
    diagnostics: Box<[TypeDiagnostic]>,
    fingerprint_v1: [u8; 32],
}

impl SourceUnitOwnerDiagnostics {
    pub const fn source_unit_id(&self) -> &SourceUnitId {
        &self.source_unit_id
    }

    pub const fn owner_count(&self) -> u32 {
        self.owner_count
    }

    pub const fn expression_count(&self) -> u32 {
        self.expression_count
    }

    pub const fn call_count(&self) -> u32 {
        self.call_count
    }

    pub const fn work(&self) -> OwnerBodyInferenceWork {
        self.work
    }

    pub fn diagnostics(&self) -> &[TypeDiagnostic] {
        &self.diagnostics
    }

    pub const fn fingerprint_v1(&self) -> [u8; 32] {
        self.fingerprint_v1
    }
}

pub fn project_source_unit_owner_diagnostics<'a>(
    source_unit_id: &SourceUnitId,
    expected_owners: impl IntoIterator<Item = &'a StableCheckOwnerKey>,
    bodies: impl IntoIterator<Item = &'a OwnerBodyInferenceShard>,
    source_maps: impl IntoIterator<Item = &'a OwnerSourceMap>,
) -> Result<SourceUnitOwnerDiagnostics, OwnerBodyInferenceError> {
    let expected_owners = expected_owners
        .into_iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    if expected_owners
        .iter()
        .any(|owner| owner.source_unit_id() != source_unit_id)
    {
        return Err(OwnerBodyInferenceError::new(
            "source-unit owner diagnostics received an owner from another source unit",
        ));
    }
    let mut bodies_by_owner = BTreeMap::new();
    for body in bodies {
        if bodies_by_owner.insert(body.owner().clone(), body).is_some() {
            return Err(OwnerBodyInferenceError::new(format!(
                "source-unit owner diagnostics received duplicate body {:?}",
                body.owner()
            )));
        }
    }
    let mut source_maps_by_owner = BTreeMap::new();
    for source_map in source_maps {
        if source_maps_by_owner
            .insert(source_map.owner().clone(), source_map)
            .is_some()
        {
            return Err(OwnerBodyInferenceError::new(format!(
                "source-unit owner diagnostics received duplicate source map {:?}",
                source_map.owner()
            )));
        }
    }
    if bodies_by_owner.keys().ne(expected_owners.iter()) {
        return Err(OwnerBodyInferenceError::new(
            "source-unit owner diagnostics body coverage differs from the expected owner set",
        ));
    }
    if source_maps_by_owner.keys().ne(expected_owners.iter()) {
        return Err(OwnerBodyInferenceError::new(
            "source-unit owner diagnostics source-map coverage differs from the expected owner set",
        ));
    }

    let mut diagnostics = Vec::new();
    let mut expression_count = 0usize;
    let mut call_count = 0usize;
    let mut work = OwnerBodyInferenceWork::default();
    let mut basis = Vec::with_capacity(expected_owners.len());
    for owner in &expected_owners {
        let body = bodies_by_owner[owner];
        let source_map = source_maps_by_owner[owner];
        diagnostics.extend(materialize_owner_diagnostics(body, source_map)?);
        expression_count = expression_count
            .checked_add(body.expressions.len())
            .ok_or_else(|| {
                OwnerBodyInferenceError::new(
                    "source-unit owner diagnostics expression count overflow",
                )
            })?;
        call_count = call_count.checked_add(body.calls.len()).ok_or_else(|| {
            OwnerBodyInferenceError::new("source-unit owner diagnostics call count overflow")
        })?;
        work.accumulate(body.work);
        basis.push((
            owner.clone(),
            body.fingerprint_v1(),
            source_map.fingerprint_v2(),
        ));
    }
    canonicalize_diagnostics(&mut diagnostics);
    let owner_count = checked_u32(
        expected_owners.len(),
        "source-unit owner diagnostics owner count",
    )?;
    let expression_count = checked_u32(
        expression_count,
        "source-unit owner diagnostics expression count",
    )?;
    let call_count = checked_u32(call_count, "source-unit owner diagnostics call count")?;
    let fingerprint_v1 = fingerprint(
        SOURCE_UNIT_OWNER_DIAGNOSTICS_DOMAIN_V1,
        &(
            source_unit_id,
            &basis,
            owner_count,
            expression_count,
            call_count,
            work,
            &diagnostics,
        ),
    )?;
    Ok(SourceUnitOwnerDiagnostics {
        source_unit_id: source_unit_id.clone(),
        basis: basis.into_boxed_slice(),
        owner_count,
        expression_count,
        call_count,
        work,
        diagnostics: diagnostics.into_boxed_slice(),
        fingerprint_v1,
    })
}

/// Partial source-bound owner diagnostics projected directly from immutable
/// owner inference results.
///
/// This is deliberately smaller than a checked-owner shard: it owns no dense
/// checked rows, construction ABI, compatibility DTO, or executable product.
/// The exact owner/body/source-map basis is sealed so a project-root request
/// can backdate an unchanged diagnostic result without reconstructing later
/// checked artifacts.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OwnerDiagnosticsAggregate {
    source_bundle_digest_v1: SourceBundleDigestV1,
    project_facts_fingerprint_v1: [u8; 32],
    owner_count: u32,
    expression_count: u32,
    call_count: u32,
    work: OwnerBodyInferenceWork,
    diagnostics: Box<[TypeDiagnostic]>,
    fingerprint_v1: [u8; 32],
}

impl OwnerDiagnosticsAggregate {
    pub const fn source_bundle_digest_v1(&self) -> SourceBundleDigestV1 {
        self.source_bundle_digest_v1
    }

    pub const fn project_facts_fingerprint_v1(&self) -> [u8; 32] {
        self.project_facts_fingerprint_v1
    }

    pub const fn owner_count(&self) -> u32 {
        self.owner_count
    }

    pub const fn expression_count(&self) -> u32 {
        self.expression_count
    }

    pub const fn call_count(&self) -> u32 {
        self.call_count
    }

    pub const fn work(&self) -> OwnerBodyInferenceWork {
        self.work
    }

    pub fn diagnostics(&self) -> &[TypeDiagnostic] {
        &self.diagnostics
    }

    pub const fn fingerprint_v1(&self) -> [u8; 32] {
        self.fingerprint_v1
    }
}

pub(crate) fn canonicalize_diagnostics(diagnostics: &mut Vec<TypeDiagnostic>) {
    diagnostics.sort_by(|left, right| {
        let severity = |severity| match severity {
            DiagnosticSeverity::Error => 0u8,
            DiagnosticSeverity::Warning => 1u8,
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
}

pub fn aggregate_owner_diagnostics<'a>(
    project: &ProjectSyntaxSnapshot,
    project_facts: &crate::ProjectDiagnosticFacts,
    expected_owners: impl IntoIterator<Item = &'a StableCheckOwnerKey>,
    bodies: impl IntoIterator<Item = &'a OwnerBodyInferenceShard>,
    source_maps: impl IntoIterator<Item = &'a OwnerSourceMap>,
) -> Result<OwnerDiagnosticsAggregate, OwnerBodyInferenceError> {
    let expected_owners = expected_owners
        .into_iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    let mut bodies_by_owner = BTreeMap::new();
    for body in bodies {
        if bodies_by_owner.insert(body.owner().clone(), body).is_some() {
            return Err(OwnerBodyInferenceError::new(format!(
                "owner diagnostics aggregate received duplicate body {:?}",
                body.owner()
            )));
        }
    }
    let mut source_maps_by_owner = BTreeMap::new();
    for source_map in source_maps {
        if source_maps_by_owner
            .insert(source_map.owner().clone(), source_map)
            .is_some()
        {
            return Err(OwnerBodyInferenceError::new(format!(
                "owner diagnostics aggregate received duplicate source map {:?}",
                source_map.owner()
            )));
        }
    }
    let body_owners = bodies_by_owner.keys().cloned().collect::<BTreeSet<_>>();
    let source_map_owners = source_maps_by_owner
        .keys()
        .cloned()
        .collect::<BTreeSet<_>>();
    if body_owners != expected_owners {
        return Err(OwnerBodyInferenceError::new(
            "owner diagnostics aggregate body coverage differs from the project owner set",
        ));
    }
    if source_map_owners != expected_owners {
        return Err(OwnerBodyInferenceError::new(
            "owner diagnostics aggregate source-map coverage differs from the project owner set",
        ));
    }
    let mut owners_by_unit = BTreeMap::<SourceUnitId, Vec<StableCheckOwnerKey>>::new();
    for owner in expected_owners {
        owners_by_unit
            .entry(owner.source_unit_id().clone())
            .or_default()
            .push(owner);
    }
    let projections = owners_by_unit
        .iter()
        .map(|(source_unit_id, owners)| {
            project_source_unit_owner_diagnostics(
                source_unit_id,
                owners,
                owners.iter().map(|owner| bodies_by_owner[owner]),
                owners.iter().map(|owner| source_maps_by_owner[owner]),
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    aggregate_source_unit_owner_diagnostics(project, project_facts, projections.iter())
}

pub fn aggregate_source_unit_owner_diagnostics<'a>(
    project: &ProjectSyntaxSnapshot,
    project_facts: &crate::ProjectDiagnosticFacts,
    projections: impl IntoIterator<Item = &'a SourceUnitOwnerDiagnostics>,
) -> Result<OwnerDiagnosticsAggregate, OwnerBodyInferenceError> {
    let project_evaluations = project
        .source_layouts()
        .iter()
        .map(|layout| {
            crate::evaluate_source_unit_project_diagnostics(
                project,
                &layout.source_unit_id,
                project_facts,
            )
            .map_err(|error| OwnerBodyInferenceError::new(error.to_string()))
        })
        .collect::<Result<Vec<_>, _>>()?;
    aggregate_source_unit_diagnostics(
        project,
        project_facts,
        projections,
        project_evaluations.iter(),
        project_evaluations
            .iter()
            .map(|evaluation| evaluation.result.as_ref()),
    )
}

pub fn aggregate_source_unit_diagnostics<'a, 'b, 'c>(
    project: &ProjectSyntaxSnapshot,
    project_facts: &crate::ProjectDiagnosticFacts,
    owner_projections: impl IntoIterator<Item = &'a SourceUnitOwnerDiagnostics>,
    project_evaluations: impl IntoIterator<Item = &'b crate::SourceUnitProjectDiagnosticsEvaluation>,
    project_projections: impl IntoIterator<Item = &'c crate::SourceUnitProjectDiagnostics>,
) -> Result<OwnerDiagnosticsAggregate, OwnerBodyInferenceError> {
    let source_bundle_digest_v1 = project.source_bundle_digest_v1();
    if project_facts.source_bundle_digest_v1() != source_bundle_digest_v1 {
        return Err(OwnerBodyInferenceError::new(
            "owner diagnostics aggregate project facts have a different source bundle",
        ));
    }
    let mut owner_projections_by_unit = BTreeMap::new();
    for projection in owner_projections {
        if owner_projections_by_unit
            .insert(projection.source_unit_id().clone(), projection)
            .is_some()
        {
            return Err(OwnerBodyInferenceError::new(format!(
                "owner diagnostics aggregate received duplicate unit {:?}",
                projection.source_unit_id()
            )));
        }
    }
    let mut project_evaluations_by_unit = BTreeMap::new();
    for evaluation in project_evaluations {
        let source_unit_id = evaluation.result.source_unit_id().clone();
        if project_evaluations_by_unit
            .insert(source_unit_id.clone(), evaluation)
            .is_some()
        {
            return Err(OwnerBodyInferenceError::new(format!(
                "owner diagnostics aggregate received duplicate project evaluation {source_unit_id:?}"
            )));
        }
    }
    let mut project_projections_by_unit = BTreeMap::new();
    for projection in project_projections {
        if project_projections_by_unit
            .insert(projection.source_unit_id().clone(), projection)
            .is_some()
        {
            return Err(OwnerBodyInferenceError::new(format!(
                "owner diagnostics aggregate received duplicate project projection {:?}",
                projection.source_unit_id()
            )));
        }
    }
    let expected_units = project
        .source_layouts()
        .iter()
        .map(|layout| layout.source_unit_id.clone())
        .collect::<BTreeSet<_>>();
    if owner_projections_by_unit.keys().ne(expected_units.iter()) {
        return Err(OwnerBodyInferenceError::new(
            "owner diagnostics aggregate owner-unit coverage differs from the project source layout",
        ));
    }
    if project_evaluations_by_unit.keys().ne(expected_units.iter()) {
        return Err(OwnerBodyInferenceError::new(
            "owner diagnostics aggregate project-evaluation coverage differs from the project source layout",
        ));
    }
    if project_projections_by_unit.keys().ne(expected_units.iter()) {
        return Err(OwnerBodyInferenceError::new(
            "owner diagnostics aggregate project-unit coverage differs from the project source layout",
        ));
    }

    let mut diagnostics = Vec::new();
    let mut basis = Vec::new();
    let mut reported_owner_count = 0u32;
    let mut expression_count = 0u32;
    let mut call_count = 0u32;
    let mut work = OwnerBodyInferenceWork::default();
    let mut project_diagnostic_count = 0usize;
    for layout in project.source_layouts() {
        let owner_projection = owner_projections_by_unit[&layout.source_unit_id];
        if owner_projection
            .basis
            .iter()
            .any(|(owner, _, _)| owner.source_unit_id() != &layout.source_unit_id)
        {
            return Err(OwnerBodyInferenceError::new(
                "owner diagnostics aggregate unit projection contains a foreign owner",
            ));
        }
        for diagnostic in owner_projection.diagnostics() {
            let mut diagnostic = diagnostic.clone();
            diagnostic.line = layout
                .start_line
                .checked_add(diagnostic.line.saturating_sub(1))
                .ok_or_else(|| {
                    OwnerBodyInferenceError::new("owner diagnostic global line overflow")
                })?;
            diagnostic.start =
                layout
                    .start_byte
                    .checked_add(diagnostic.start)
                    .ok_or_else(|| {
                        OwnerBodyInferenceError::new("owner diagnostic global start overflow")
                    })?;
            diagnostic.end = layout
                .start_byte
                .checked_add(diagnostic.end)
                .ok_or_else(|| {
                    OwnerBodyInferenceError::new("owner diagnostic global end overflow")
                })?;
            diagnostics.push(diagnostic);
        }
        let project_evaluation = project_evaluations_by_unit[&layout.source_unit_id];
        let project_projection = project_projections_by_unit[&layout.source_unit_id];
        if !project_evaluation.matches_inputs(project, project_facts)
            || project_evaluation.currentness.result_fingerprint_v1()
                != project_projection.fingerprint_v1()
        {
            return Err(OwnerBodyInferenceError::new(
                "owner diagnostics aggregate received stale source-unit project diagnostics",
            ));
        }
        project_diagnostic_count = project_diagnostic_count
            .checked_add(project_projection.rows().len())
            .ok_or_else(|| OwnerBodyInferenceError::new("project diagnostic row count overflow"))?;
        for row in project_projection.rows() {
            let mut diagnostic = row.diagnostic().clone();
            diagnostic.line = layout
                .start_line
                .checked_add(diagnostic.line.saturating_sub(1))
                .ok_or_else(|| {
                    OwnerBodyInferenceError::new("project diagnostic global line overflow")
                })?;
            if row.relocate_bytes() {
                diagnostic.start =
                    layout
                        .start_byte
                        .checked_add(diagnostic.start)
                        .ok_or_else(|| {
                            OwnerBodyInferenceError::new("project diagnostic global start overflow")
                        })?;
                diagnostic.end =
                    layout
                        .start_byte
                        .checked_add(diagnostic.end)
                        .ok_or_else(|| {
                            OwnerBodyInferenceError::new("project diagnostic global end overflow")
                        })?;
            }
            diagnostics.push(diagnostic);
        }
        basis.extend(owner_projection.basis.iter().cloned());
        reported_owner_count = reported_owner_count
            .checked_add(owner_projection.owner_count())
            .ok_or_else(|| {
                OwnerBodyInferenceError::new("owner diagnostics owner count overflow")
            })?;
        expression_count = expression_count
            .checked_add(owner_projection.expression_count())
            .ok_or_else(|| {
                OwnerBodyInferenceError::new("owner diagnostics expression count overflow")
            })?;
        call_count = call_count
            .checked_add(owner_projection.call_count())
            .ok_or_else(|| OwnerBodyInferenceError::new("owner diagnostics call count overflow"))?;
        work.accumulate(owner_projection.work());
    }
    basis.sort_by(|left, right| left.0.cmp(&right.0));
    if basis.windows(2).any(|pair| pair[0].0 == pair[1].0) {
        return Err(OwnerBodyInferenceError::new(
            "owner diagnostics aggregate received duplicate owner basis rows",
        ));
    }
    let expected_owners = project.stable_check_owner_keys().collect::<BTreeSet<_>>();
    if basis
        .iter()
        .map(|(owner, _, _)| owner)
        .ne(expected_owners.iter())
    {
        return Err(OwnerBodyInferenceError::new(
            "owner diagnostics aggregate owner coverage differs from the project owner set",
        ));
    }
    if project_diagnostic_count != project_facts.diagnostics().len() {
        return Err(OwnerBodyInferenceError::new(
            "owner diagnostics aggregate project rows do not cover every project diagnostic",
        ));
    }
    canonicalize_diagnostics(&mut diagnostics);
    let owner_count = checked_u32(basis.len(), "owner diagnostics owner count")?;
    if reported_owner_count != owner_count {
        return Err(OwnerBodyInferenceError::new(
            "owner diagnostics aggregate unit owner counts differ from their basis rows",
        ));
    }
    let project_facts_fingerprint_v1 = project_facts.fingerprint_v1();
    let fingerprint_v1 = fingerprint(
        OWNER_DIAGNOSTICS_AGGREGATE_DOMAIN_V8,
        &(
            source_bundle_digest_v1,
            &basis,
            project_facts_fingerprint_v1,
            owner_count,
            expression_count,
            call_count,
            work,
            &diagnostics,
        ),
    )?;
    Ok(OwnerDiagnosticsAggregate {
        source_bundle_digest_v1,
        project_facts_fingerprint_v1,
        owner_count,
        expression_count,
        call_count,
        work,
        diagnostics: diagnostics.into_boxed_slice(),
        fingerprint_v1,
    })
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

#[derive(Clone, Copy)]
enum PlannedLexicalRead {
    Unplanned,
    Bound(TypeVar),
    Imported { root: TypeVar, mode: FlowMode },
    Dynamic,
    Reserved,
}

fn planned_lexical_read_variables(
    syntax: &OwnerSyntaxInput,
    lexical_plan: &OwnerLexicalPlan,
    signature_lexical_plan: &OwnerSignatureLexicalPlan,
    expressions: &[TypeVar],
    external_expressions: &[TypeVar],
    expression_flushes: &[TypeVar],
    external_expression_flushes: &[TypeVar],
    parameter_variables: &BTreeMap<u32, TypeVar>,
    signature_declaration_variables: &BTreeMap<OwnerSignatureDeclarationTarget, TypeVar>,
    lexical_capture_variables: &BTreeMap<OwnerLexicalTargetRef, (TypeVar, FlowMode)>,
    context: Option<TypeVar>,
    unifier: &mut TypeUnifier,
) -> Result<Vec<PlannedLexicalRead>, OwnerBodyInferenceError> {
    if lexical_plan.reads().len() != syntax.expressions.len()
        || signature_lexical_plan.reads().len() != syntax.expressions.len()
        || !signature_lexical_plan.matches_base(lexical_plan)
    {
        return Err(OwnerBodyInferenceError::new(
            "owner body signature lexical plan does not cover its current base expression table",
        ));
    }

    let mut statement_variables = BTreeMap::new();
    for (statement, expression) in lexical_plan.statement_values() {
        if let Some(variable) = body_expression_boundary_variable(
            expressions,
            external_expressions,
            expression_flushes,
            external_expression_flushes,
            *expression,
            unifier,
        ) {
            statement_variables.insert(*statement, variable);
        }
    }

    let mut record_field_variables = BTreeMap::new();
    for field in lexical_plan.record_fields() {
        let expression = syntax
            .expressions
            .get(field.object as usize)
            .ok_or_else(|| {
                OwnerBodyInferenceError::new(
                    "owner body lexical record field references a missing expression",
                )
            })?;
        let fields = match &expression.kind {
            AstExprKind::Object(fields) | AstExprKind::TaggedObject { fields, .. } => fields,
            _ => {
                return Err(OwnerBodyInferenceError::new(
                    "owner body lexical record field belongs to a non-record expression",
                ));
            }
        };
        let value = fields.get(field.ordinal as usize).ok_or_else(|| {
            OwnerBodyInferenceError::new("owner body lexical record field ordinal is missing")
        })?;
        if value.spread || value.name != field.name {
            return Err(OwnerBodyInferenceError::new(
                "owner body lexical record field does not match its syntax field",
            ));
        }
        let value = checked_u32(value.value, "owner body lexical record field value")?;
        let variable = body_expression_boundary_variable(
            expressions,
            external_expressions,
            expression_flushes,
            external_expression_flushes,
            value,
            unifier,
        )
        .ok_or_else(|| {
            OwnerBodyInferenceError::new(
                "owner body lexical record field value is outside its expression namespace",
            )
        })?;
        record_field_variables.insert((field.object, field.ordinal), variable);
    }

    let mut reads = Vec::with_capacity(signature_lexical_plan.reads().len());
    for read in signature_lexical_plan.reads() {
        let Some(read) = read else {
            reads.push(PlannedLexicalRead::Unplanned);
            continue;
        };
        let (root, imported_mode) = match &read.target {
            OwnerEffectiveLexicalTarget::Static {
                target: OwnerLexicalDeclarationTarget::Parameter { ordinal },
            } => (Some(
                parameter_variables.get(ordinal).copied().ok_or_else(|| {
                    OwnerBodyInferenceError::new(format!(
                        "owner body lexical plan references missing parameter {ordinal}"
                    ))
                })?,
            ), None),
            OwnerEffectiveLexicalTarget::Static {
                target: OwnerLexicalDeclarationTarget::Statement { statement },
            } => {
                let statement_row = syntax.statements.get(*statement as usize).ok_or_else(|| {
                    OwnerBodyInferenceError::new(format!(
                        "owner body lexical plan references missing statement {statement}"
                    ))
                })?;
                let root = if matches!(statement_row.kind, AstStatementKind::Function { .. }) {
                    None
                } else {
                    Some(*statement_variables
                        .entry(*statement)
                        .or_insert_with(|| unifier.fresh()))
                };
                (root, None)
            }
            OwnerEffectiveLexicalTarget::Static {
                target:
                    OwnerLexicalDeclarationTarget::RecordField {
                        object, ordinal, ..
                    },
            } => (Some(
                record_field_variables
                    .get(&(*object, *ordinal))
                    .copied()
                    .ok_or_else(|| {
                    OwnerBodyInferenceError::new(format!(
                        "owner body lexical plan references missing record field {object}:{ordinal}"
                    ))
                    })?,
            ), None),
            OwnerEffectiveLexicalTarget::Static {
                target: OwnerLexicalDeclarationTarget::Passed,
            } => (syntax
                .statements
                .iter()
                .any(|statement| matches!(statement.kind, AstStatementKind::Function { .. }))
                .then_some(context)
                .flatten(), None),
            OwnerEffectiveLexicalTarget::FreshOut {
                call,
                formal_ordinal,
            } => (signature_declaration_variables
                .get(&OwnerSignatureDeclarationTarget::FreshOut {
                    call: call.clone(),
                    formal_ordinal: *formal_ordinal,
                })
                .copied(), None),
            OwnerEffectiveLexicalTarget::CallContext {
                call,
                context_ordinal,
            } => (signature_declaration_variables
                .get(&OwnerSignatureDeclarationTarget::CallContext {
                    call: call.clone(),
                    context_ordinal: *context_ordinal,
                })
                .copied(), None),
            OwnerEffectiveLexicalTarget::Imported { target } => lexical_capture_variables
                .get(target)
                .map_or((None, None), |(variable, mode)| {
                    (Some(*variable), Some(*mode))
                }),
            OwnerEffectiveLexicalTarget::Static {
                target:
                    OwnerLexicalDeclarationTarget::PatternBinding { .. }
                    | OwnerLexicalDeclarationTarget::Imported { .. }
                    | OwnerLexicalDeclarationTarget::Ambiguous { .. },
            }
            | OwnerEffectiveLexicalTarget::InvalidBareBinding { .. }
            | OwnerEffectiveLexicalTarget::Ambiguous { .. } => (None, None),
        };
        // Defer projection binding until the ordinary lexical-read branch.
        // Branch-local selector narrowing owns its projection independently
        // and must not close the root's public/body type in advance.
        let dynamic = matches!(
            &read.target,
            OwnerEffectiveLexicalTarget::FreshOut { .. }
                | OwnerEffectiveLexicalTarget::CallContext { .. }
        );
        reads.push(root.map_or(PlannedLexicalRead::Reserved, |root| {
            if dynamic {
                let _ = root;
                PlannedLexicalRead::Dynamic
            } else if let Some(mode) = imported_mode {
                PlannedLexicalRead::Imported { root, mode }
            } else {
                PlannedLexicalRead::Bound(root)
            }
        }));
    }
    Ok(reads)
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

fn push_lexical_read_diagnostics(
    syntax: &OwnerSyntaxInput,
    seed: &OwnerConstraintSeed,
    signature_lexical_plan: &OwnerSignatureLexicalPlan,
    diagnostics: &mut Vec<OwnerDiagnosticTemplate>,
) {
    let mut duplicate_record_names = BTreeSet::new();
    for expression in &syntax.expressions {
        let fields = match &expression.kind {
            AstExprKind::Object(fields) | AstExprKind::TaggedObject { fields, .. } => fields,
            _ => continue,
        };
        let mut names = BTreeSet::new();
        for (ordinal, field) in fields.iter().enumerate().filter(|(_, field)| !field.spread) {
            if names.insert(field.name.clone()) {
                continue;
            }
            duplicate_record_names.insert(field.name.clone());
            let Ok(ordinal) = u32::try_from(ordinal) else {
                continue;
            };
            diagnostics.push(OwnerDiagnosticTemplate {
                severity: DiagnosticSeverity::Error,
                code: "duplicate_record_field".to_owned(),
                message: format!("duplicate explicit record field `{}`", field.name),
                site: OwnerSourceAnchorSite::Expression {
                    expression: expression.stable_key.clone(),
                },
                role: Some(OwnerSourceAnchorRole::RecordField { ordinal }),
            });
        }
    }
    let functions = seed
        .declarations
        .iter()
        .filter(|declaration| declaration.kind == crate::OwnerDeclarationKind::Function)
        .filter_map(|declaration| {
            declaration
                .names
                .first()
                .map(|name| (declaration.statement, name))
        })
        .collect::<BTreeMap<_, _>>();
    for (index, read) in signature_lexical_plan.reads().iter().enumerate() {
        let Some(read) = read else { continue };
        let Some(expression) = seed.expressions.get(index) else {
            continue;
        };
        if !matches!(
            expression.kind,
            OwnerConstraintNodeKind::Reference { .. } | OwnerConstraintNodeKind::Drain { .. }
        ) {
            continue;
        }
        let diagnostic = match &read.target {
            OwnerEffectiveLexicalTarget::Static {
                target: OwnerLexicalDeclarationTarget::Statement { statement },
            } => functions.get(statement).map(|function| {
                (
                    "function_must_be_called",
                    format!(
                        "function `{function}` must be called with parentheses: `{function}()`"
                    ),
                )
            }),
            OwnerEffectiveLexicalTarget::Imported {
                target:
                    OwnerLexicalTargetRef::Declaration {
                        capability: boon_checked::OwnerLexicalDeclarationCapability::CallableOnly,
                        ..
                    },
            } => {
                let function = match &expression.kind {
                    OwnerConstraintNodeKind::Reference { parts }
                    | OwnerConstraintNodeKind::Drain { parts } => parts.join("/"),
                    _ => String::new(),
                };
                Some((
                    "function_must_be_called",
                    format!(
                        "function `{function}` must be called with parentheses: `{function}()`"
                    ),
                ))
            }
            OwnerEffectiveLexicalTarget::Static {
                target: OwnerLexicalDeclarationTarget::Ambiguous { name },
            }
            | OwnerEffectiveLexicalTarget::Ambiguous { name }
                if !duplicate_record_names.contains(name) =>
            {
                Some((
                    "ambiguous_lexical_read",
                    format!("ambiguous lexical reference `{name}` matches multiple declarations"),
                ))
            }
            OwnerEffectiveLexicalTarget::Static {
                target: OwnerLexicalDeclarationTarget::Passed,
            } if !seed.declarations.iter().any(|declaration| {
                declaration.public && declaration.kind == crate::OwnerDeclarationKind::Function
            }) =>
            {
                Some((
                    "unbound_passed_context",
                    "`PASSED` has no enclosing callable context".to_owned(),
                ))
            }
            _ => None,
        };
        let Some((code, message)) = diagnostic else {
            continue;
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
    signature_lexical_plan: &OwnerSignatureLexicalPlan,
    abi: &OwnerInferenceAbiEnvironment,
    diagnostics: &mut Vec<OwnerDiagnosticTemplate>,
) {
    for resolution in &summary.symbol_resolutions {
        let reference = resolution.reference();
        if reference.kind != OwnerReferenceKind::Value
            || !signature_lexical_plan.is_external_candidate(reference)
        {
            continue;
        }
        let direct = match resolution {
            OwnerSymbolResolution::CallableAsValue { .. } => {
                let function = reference.parts.join("/");
                Some((
                    "function_must_be_called",
                    format!(
                        "function `{function}` must be called with parentheses: `{function}()`"
                    ),
                ))
            }
            OwnerSymbolResolution::Unresolved { .. } => Some((
                "unknown_identifier",
                format!("unknown identifier `{}`", reference.parts.join(".")),
            )),
            OwnerSymbolResolution::Ambiguous { candidates, .. } => Some((
                "ambiguous_value",
                format!(
                    "ambiguous value `{}` has {} equally ranked project targets",
                    reference.parts.join("."),
                    candidates.len()
                ),
            )),
            OwnerSymbolResolution::Resolved { .. }
            | OwnerSymbolResolution::Authoritative { .. } => None,
        };
        if let Some((code, message)) = direct {
            diagnostics.push(OwnerDiagnosticTemplate {
                severity: DiagnosticSeverity::Error,
                code: code.to_owned(),
                message,
                site: OwnerSourceAnchorSite::Expression {
                    expression: reference.expression.clone(),
                },
                role: None,
            });
            continue;
        }
        let OwnerSymbolResolution::Authoritative { .. } = resolution else {
            continue;
        };
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
    signature_lexical_plan: &OwnerSignatureLexicalPlan,
) -> Box<[OwnerBodyRelocation]> {
    let mut relocations = BTreeSet::new();
    for resolved in &summary.resolved_references {
        if resolved.reference.kind == OwnerReferenceKind::Value
            && !signature_lexical_plan.is_external_candidate(&resolved.reference)
        {
            continue;
        }
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
    reference: &OwnerResidualExpressionRef,
    dependencies: &mut BTreeSet<StableCheckOwnerKey>,
) {
    if let OwnerResidualExpressionRef::Child { owner, .. } = reference {
        dependencies.insert(owner.clone());
    }
}

fn owner_result_transfer_dependencies(
    transfer: &OwnerResidualDraft,
) -> BTreeSet<StableCheckOwnerKey> {
    let OwnerResidualDraft::Expression { root, nodes } = transfer else {
        return BTreeSet::new();
    };
    let mut dependencies = BTreeSet::new();
    collect_result_expression_ref_owner(root, &mut dependencies);
    for node in nodes {
        if let Some(OwnerResidualCallTarget::Owner { owner }) = &node.call_target {
            dependencies.insert(owner.clone());
        }
        for input in &node.inputs {
            collect_result_expression_ref_owner(&input.expression, &mut dependencies);
        }
    }
    dependencies
}

/// Build an exact body-interface plan from already sealed SCC modules.
///
/// Persistent evaluators should normally drive [`OwnerBodyInterfacePlanner`]
/// directly so each provider lookup becomes an exact request dependency. This
/// convenience boundary is useful for direct typechecker callers and tests.
pub fn plan_owner_body_interfaces<'a>(
    seed: &OwnerConstraintSeed,
    summary: &OwnerConstraintSummary,
    available_modules: impl IntoIterator<Item = &'a OwnerInterfaceTransferModule>,
) -> Result<OwnerBodyInterfacePlan, OwnerBodyInferenceError> {
    let mut provider_by_owner = BTreeMap::<StableCheckOwnerKey, OwnerInterfaceSccKey>::new();
    let mut modules = BTreeMap::<OwnerInterfaceSccKey, Arc<OwnerInterfaceTransferModule>>::new();
    for module in available_modules {
        if let Some(previous) = modules.insert(module.key.clone(), Arc::new(module.clone()))
            && previous.fingerprint_v1() != module.fingerprint_v1()
        {
            return Err(OwnerBodyInferenceError::new(format!(
                "owner body interface planning received conflicting SCC modules {:?}",
                module.key
            )));
        }
        for interface in &module.result.owners {
            if let Some(previous) =
                provider_by_owner.insert(interface.owner.clone(), module.key.clone())
                && previous != module.key
            {
                return Err(OwnerBodyInferenceError::new(format!(
                    "owner body interface planning received multiple providers for {:?}",
                    interface.owner
                )));
            }
        }
    }
    let mut planner = OwnerBodyInterfacePlanner::new(seed, summary)?;
    while let Some(owner) = planner.next_required_owner().cloned() {
        let provider = provider_by_owner.get(&owner).ok_or_else(|| {
            OwnerBodyInferenceError::new(format!(
                "owner body interface planning {:?} is missing required interface {owner:?}",
                seed.owner
            ))
        })?;
        planner.provide_interface_module(Arc::clone(&modules[provider]))?;
    }
    planner.finish()
}

fn frozen_scc_ref(
    plan: &OwnerBodyInterfaceSccPlan,
) -> Result<FrozenOwnerInterfaceSccRef, OwnerBodyInferenceError> {
    let module = plan.module.as_ref();
    let result = module.result();
    if result.key != plan.key
        || result.key_fingerprint_v1() != plan.key_fingerprint_v1
        || module.fingerprint_v1() != plan.module_fingerprint_v1
    {
        return Err(OwnerBodyInferenceError::new(format!(
            "owner body interface plan expected SCC {:?}, got {:?}",
            plan.key, result.key
        )));
    }
    if plan.referenced_members.is_empty() {
        return Err(OwnerBodyInferenceError::new(format!(
            "owner body inference received unused interface SCC {:?}",
            result.key
        )));
    }
    if plan
        .referenced_members
        .windows(2)
        .any(|pair| pair[0] >= pair[1])
        || plan
            .referenced_members
            .iter()
            .any(|index| *index as usize >= result.key.members.len())
    {
        return Err(OwnerBodyInferenceError::new(format!(
            "owner body interface plan has invalid referenced members for SCC {:?}",
            result.key
        )));
    }
    Ok(FrozenOwnerInterfaceSccRef {
        key: result.key.clone(),
        key_fingerprint_v1: result.key_fingerprint_v1(),
        result_fingerprint_v1: result.fingerprint_v1(),
        transfer_module_fingerprint_v1: module.fingerprint_v1(),
        type_variable_count: result.type_variable_count,
        referenced_members: plan.referenced_members.clone(),
    })
}

fn signature_read_preserved_projection(
    seed: &OwnerConstraintSeed,
    signature_lexical_plan: &OwnerSignatureLexicalPlan,
    expression: u32,
) -> Option<Box<[String]>> {
    let Some(base) = seed
        .lexical_reads()
        .get(expression as usize)
        .and_then(Option::as_ref)
    else {
        return None;
    };
    let read = signature_lexical_plan
        .reads()
        .get(expression as usize)
        .and_then(Option::as_ref)?;
    (matches!(
        &read.target,
        OwnerEffectiveLexicalTarget::Static { target } if target == &base.target
    ) && read.projection == base.projection)
        .then(|| read.projection.clone())
}

fn exact_pattern_local_expressions(
    seed: &OwnerConstraintSeed,
    signature_lexical_plan: &OwnerSignatureLexicalPlan,
) -> BTreeSet<u32> {
    let mut expressions = BTreeSet::new();
    for arm in &seed.expressions {
        let selector = arm.inputs.iter().find_map(|input| {
            matches!(input.role, OwnerConstraintEdgeRole::MatchSelector).then_some(input.expression)
        });
        for input in &arm.inputs {
            match &input.role {
                OwnerConstraintEdgeRole::MatchBinding { .. }
                    if signature_read_preserved_projection(
                        seed,
                        signature_lexical_plan,
                        input.expression,
                    )
                    .is_some() =>
                {
                    expressions.insert(input.expression);
                }
                OwnerConstraintEdgeRole::MatchNarrowedSelector { projection }
                    if selector.is_some_and(|selector| {
                        signature_narrowed_selector_read_matches(
                            seed,
                            signature_lexical_plan,
                            selector,
                            projection,
                            input.expression,
                        )
                    }) =>
                {
                    expressions.insert(input.expression);
                }
                _ => {}
            }
        }
    }
    expressions
}

fn signature_narrowed_selector_read_matches(
    seed: &OwnerConstraintSeed,
    signature_lexical_plan: &OwnerSignatureLexicalPlan,
    selector: u32,
    projection: &[String],
    candidate: u32,
) -> bool {
    effective_narrowed_selector_read_matches(
        seed,
        signature_lexical_plan,
        selector,
        projection,
        candidate,
    )
}

fn bind_local_constraints(
    seed: &OwnerConstraintSeed,
    summary: &OwnerConstraintSummary,
    signature_lexical_plan: &OwnerSignatureLexicalPlan,
    abi: &OwnerInferenceAbiEnvironment,
    unifier: &mut TypeUnifier,
    expressions: &[TypeVar],
    external_expressions: &[TypeVar],
    planned_lexical_reads: &[PlannedLexicalRead],
    pattern_local_expressions: &BTreeSet<u32>,
    modes: &mut [Option<FlowMode>],
    direct_effects: &mut [CheckedEffectSummary],
    calls: &mut Vec<BodyCallPlan>,
    pattern_narrowings: &mut Vec<OwnerPatternNarrowing>,
    flow_constraints: &mut Vec<OwnerFlowConstraint>,
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
                if pattern_local_expressions.contains(&(index as u32)) {
                    // The owning match arm binds this occurrence against an
                    // arm-local pattern value below.
                } else if let PlannedLexicalRead::Bound(root) = planned_lexical_reads[index] {
                    // The shared lexical plan is authoritative over project
                    // symbol resolution. This is what makes whole-scope and
                    // record-field shadowing stable during inference.
                    let read = signature_lexical_plan.reads()[index]
                        .as_ref()
                        .expect("bound lexical root must have a read plan");
                    let local = bind_projection(unifier, root, &read.projection);
                    unifier.unify(Type::Var(variable), Type::Var(local));
                } else if let PlannedLexicalRead::Imported {
                    root,
                    mode: imported_mode,
                } = planned_lexical_reads[index]
                {
                    let read = signature_lexical_plan.reads()[index]
                        .as_ref()
                        .expect("imported lexical root must have a read plan");
                    let local = bind_projection(unifier, root, &read.projection);
                    unifier.unify(Type::Var(variable), Type::Var(local));
                    mode = Some(imported_mode);
                } else if matches!(planned_lexical_reads[index], PlannedLexicalRead::Reserved) {
                    // Ambiguous/PASSED-without-context reads are still planned
                    // locals and must not fall through to project symbols.
                } else if matches!(planned_lexical_reads[index], PlannedLexicalRead::Dynamic) {
                    // Dynamic projections bind after their call signature has
                    // instantiated the FreshOut/context root. Binding an open
                    // projection before that point would widen closed records.
                } else if resolved.contains_key(&expression.expression) {
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
                    Some(OwnerSymbolResolution::CallableAsValue { .. }) => {
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
                    bind_and_record_flow_variables(unifier, flow_constraints, variable, [input]);
                }
                mode = None;
            }
            OwnerConstraintNodeKind::Hold { .. } => {
                direct_effects[index].reads_state = true;
                direct_effects[index].writes_state = true;
            }
            OwnerConstraintNodeKind::Latest => {
                let inputs = expression
                    .inputs
                    .iter()
                    .filter_map(|input| {
                        expression_variable(expressions, external_expressions, input.expression)
                    })
                    .collect::<Vec<_>>();
                bind_and_record_flow_variables(unifier, flow_constraints, variable, inputs);
                mode = None;
            }
            OwnerConstraintNodeKind::When => {
                let inputs = expression
                    .inputs
                    .iter()
                    .filter(|input| matches!(input.role, OwnerConstraintEdgeRole::WhenArm))
                    .filter_map(|input| {
                        expression_variable(expressions, external_expressions, input.expression)
                    })
                    .collect::<Vec<_>>();
                bind_and_record_flow_variables(unifier, flow_constraints, variable, inputs);
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
                    bind_and_record_flow_variables(unifier, flow_constraints, variable, [input]);
                }
                mode = Some(FlowMode::PresentOrAbsent);
            }
            OwnerConstraintNodeKind::Infix { operation } => {
                if infix_requires_number_operands(operation) {
                    for input in &expression.inputs {
                        if let Some(input) =
                            expression_variable(expressions, external_expressions, input.expression)
                        {
                            unifier.bind_var(input, Type::Number);
                        }
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
                        bind_and_record_flow_variables(
                            unifier,
                            flow_constraints,
                            variable,
                            [output],
                        );
                    }
                    mode = None;
                } else {
                    unifier.bind_var(variable, Type::Absent);
                    mode = Some(FlowMode::Absent);
                }
                let pattern_ty = pattern_type(pattern, unifier);
                let local_bindings = expression
                    .inputs
                    .iter()
                    .filter_map(|input| {
                        let OwnerConstraintEdgeRole::MatchBinding { name } = &input.role else {
                            return None;
                        };
                        let projection = signature_read_preserved_projection(
                            seed,
                            signature_lexical_plan,
                            input.expression,
                        )?;
                        expression_variable(expressions, external_expressions, input.expression)
                            .map(|read| (name.clone(), projection, read))
                    })
                    .collect::<Vec<_>>();
                let mut bindings = Vec::new();
                for (name, projection, read) in &local_bindings {
                    if let Some(binding_ty) =
                        pattern_binding_type_from_pattern(pattern, &pattern_ty, name)
                    {
                        let root = unifier.fresh();
                        unifier.bind_var(root, binding_ty);
                        let projected = bind_projection(unifier, root, projection);
                        unifier.unify(Type::Var(*read), Type::Var(projected));
                        bindings.push((name.clone(), root));
                    }
                }
                let narrowed_payload = unifier.fresh();
                if let (crate::OwnerPatternConstraint::Tag { name, .. }, Type::VariantSet(variants)) =
                    (pattern, &pattern_ty)
                    && let Some(fields) = variants.iter().find_map(|variant| match variant {
                        Variant::Tagged { tag, fields } if tag == name => Some(fields.clone()),
                        Variant::Tag(_) | Variant::Tagged { .. } => None,
                    })
                {
                    unifier.bind_var(narrowed_payload, Type::Object(fields));
                }
                let selector_reads = expression
                    .inputs
                    .iter()
                    .filter_map(|input| {
                        let OwnerConstraintEdgeRole::MatchNarrowedSelector { projection } =
                            &input.role
                        else {
                            return None;
                        };
                        let selector = expression.inputs.iter().find_map(|input| {
                            matches!(input.role, OwnerConstraintEdgeRole::MatchSelector)
                                .then_some(input.expression)
                        })?;
                        if !signature_narrowed_selector_read_matches(
                            seed,
                            signature_lexical_plan,
                            selector,
                            projection,
                            input.expression,
                        ) {
                            return None;
                        }
                        expression_variable(expressions, external_expressions, input.expression)
                            .map(|read| (projection.clone(), read))
                    })
                    .collect::<Vec<_>>();
                for (projection, read) in &selector_reads {
                    if projection.is_empty() {
                        unifier.bind_flow_result(*read, pattern_ty.clone());
                    } else {
                        let projected = bind_projection(unifier, narrowed_payload, projection);
                        unifier.unify(Type::Var(*read), Type::Var(projected));
                    }
                }
                if let Some(selector) = expression
                    .inputs
                    .iter()
                    .find(|input| matches!(input.role, OwnerConstraintEdgeRole::MatchSelector))
                    .and_then(|input| {
                        expression_variable(expressions, external_expressions, input.expression)
                    })
                {
                    pattern_narrowings.push(OwnerPatternNarrowing {
                        selector,
                        pattern: pattern.clone(),
                        bindings: bindings.into_boxed_slice(),
                        binding_reads: local_bindings.into_boxed_slice(),
                        selector_reads: selector_reads.into_boxed_slice(),
                    });
                }
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
                        bind_and_record_flow_variables(
                            unifier,
                            flow_constraints,
                            variable,
                            [result],
                        );
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
                    let item = if expression.inputs.is_empty() {
                        unifier.fresh_contextual_hole()
                    } else {
                        unifier.fresh()
                    };
                    let inputs = expression
                        .inputs
                        .iter()
                        .filter_map(|input| {
                            expression_variable(expressions, external_expressions, input.expression)
                        })
                        .collect::<Vec<_>>();
                    if !inputs.is_empty() {
                        bind_and_record_structural_flow_variables(
                            unifier,
                            flow_constraints,
                            item,
                            inputs,
                        );
                    }
                    unifier.replace_derived_provider(
                        variable,
                        Type::List(Type::shared(Type::Var(item))),
                    );
                }
                OwnerCollectionKind::Set => {
                    let item = if expression.inputs.is_empty() {
                        unifier.fresh_contextual_hole()
                    } else {
                        unifier.fresh()
                    };
                    let inputs = expression
                        .inputs
                        .iter()
                        .filter_map(|input| {
                            expression_variable(expressions, external_expressions, input.expression)
                        })
                        .collect::<Vec<_>>();
                    if !inputs.is_empty() {
                        bind_and_record_structural_flow_variables(
                            unifier,
                            flow_constraints,
                            item,
                            inputs,
                        );
                    }
                    unifier.replace_derived_provider(
                        variable,
                        Type::Set(Type::shared(Type::Var(item))),
                    );
                }
                OwnerCollectionKind::Bytes => {
                    let size = fixed_size_or_capacity
                        .map(|size| BytesType::Fixed(size as usize))
                        .unwrap_or(BytesType::Dynamic);
                    unifier.replace_derived_provider(variable, Type::Bytes(size));
                }
                OwnerCollectionKind::Map => {
                    let empty = expression.inputs.is_empty();
                    let key = if empty {
                        unifier.fresh_contextual_hole()
                    } else {
                        unifier.fresh()
                    };
                    let value = if empty {
                        unifier.fresh_contextual_hole()
                    } else {
                        unifier.fresh()
                    };
                    let entries = expression
                        .inputs
                        .iter()
                        .filter_map(|input| {
                            expression_variable(expressions, external_expressions, input.expression)
                        })
                        .collect::<Vec<_>>();
                    let keys = entries
                        .iter()
                        .map(|entry| bind_projection(unifier, *entry, &["key".to_owned()]))
                        .collect::<Vec<_>>();
                    let values = entries
                        .iter()
                        .map(|entry| bind_projection(unifier, *entry, &["value".to_owned()]))
                        .collect::<Vec<_>>();
                    if !keys.is_empty() {
                        bind_and_record_structural_flow_variables(
                            unifier,
                            flow_constraints,
                            key,
                            keys,
                        );
                        bind_and_record_structural_flow_variables(
                            unifier,
                            flow_constraints,
                            value,
                            values,
                        );
                    }
                    unifier.replace_derived_provider(
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
                    bind_and_record_flow_variables(unifier, flow_constraints, variable, [output]);
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
    ordinal: u32,
    flow_type: FlowType,
}

#[derive(Clone)]
struct InstantiatedCallContext {
    ordinal: u32,
    name: String,
    provider_parameter_ordinal: u32,
    flow_type: FlowType,
}

#[derive(Clone)]
struct InstantiatedCallSignature {
    parameters: Vec<InstantiatedCallParameter>,
    contexts: Vec<InstantiatedCallContext>,
    result: FlowType,
    result_specialization: crate::OwnerAbiResultSpecialization,
    result_flush_type: Option<Type>,
    context: Option<Type>,
    effect: CheckedEffectSummary,
    target: InferredOwnerCallableTarget,
}

#[derive(Clone)]
struct InferredCallDraft {
    plan: BodyCallPlan,
    matched_inputs: Box<[crate::OwnerSignatureMatchedInputPlan]>,
    explicit_pass: Option<crate::OwnerSignaturePassPlan>,
    dynamic_inputs: Box<[(u32, Type)]>,
    dynamic_pass: Option<(u32, Type)>,
    target: InferredOwnerCallableTarget,
    effect: CheckedEffectSummary,
    actual_inputs: BTreeMap<u32, Type>,
    first_transfer: Option<OwnerCallTransferReplay>,
    resolved_result: Option<FlowType>,
    type_substitutions: Vec<(TypeVar, Type)>,
    contextual_type_variables: Vec<TypeVar>,
    syntax_discriminated_result: bool,
    valid: bool,
}

#[derive(Clone, Eq, PartialEq)]
pub(crate) struct EvaluatedResultValue {
    pub(crate) flow_type: FlowType,
    pub(crate) parameter_derived: bool,
    pub(crate) syntax_selected: bool,
    pub(crate) static_number: Option<ExactNumber>,
}

/// Formal-ordinal-indexed values stay tiny in result-transfer evaluation.
/// Keep them inline while retaining the deterministic replace-and-sort
/// semantics of the former `BTreeMap` representation.
#[derive(Clone, Default, Eq, PartialEq)]
pub(crate) struct OwnerResidualDraftArguments {
    entries: SmallVec<[(u32, EvaluatedResultValue); 4]>,
}

impl OwnerResidualDraftArguments {
    pub(crate) fn insert(&mut self, ordinal: u32, value: EvaluatedResultValue) {
        match self
            .entries
            .binary_search_by_key(&ordinal, |(candidate, _)| *candidate)
        {
            Ok(index) => self.entries[index].1 = value,
            Err(index) => self.entries.insert(index, (ordinal, value)),
        }
    }

    fn get(&self, ordinal: &u32) -> Option<&EvaluatedResultValue> {
        self.entries
            .binary_search_by_key(ordinal, |(candidate, _)| *candidate)
            .ok()
            .and_then(|index| self.entries.get(index))
            .map(|(_, value)| value)
    }

    fn values(&self) -> impl Iterator<Item = &EvaluatedResultValue> {
        self.entries.iter().map(|(_, value)| value)
    }
}

impl<const N: usize> From<[(u32, EvaluatedResultValue); N]> for OwnerResidualDraftArguments {
    fn from(entries: [(u32, EvaluatedResultValue); N]) -> Self {
        let mut result = Self::default();
        for (ordinal, value) in entries {
            result.insert(ordinal, value);
        }
        result
    }
}

#[derive(Clone, Eq, PartialEq)]
struct OwnerCallTransferInvocation {
    arguments: OwnerResidualDraftArguments,
    context: Option<EvaluatedResultValue>,
}

#[derive(Clone)]
struct OwnerCallTransferReplay {
    invocation: OwnerCallTransferInvocation,
    evaluation: EvaluatedOwnerResult,
}

#[derive(Clone)]
struct EvaluatedOwnerResult {
    value: EvaluatedResultValue,
    type_substitutions: Vec<(TypeVar, Type)>,
    contextual_type_variables: Vec<TypeVar>,
    owned_variables: Vec<TypeVar>,
}

struct OwnerResidualDraftVariables {
    entries: SmallVec<[(TypeVar, TypeVar); 8]>,
}

impl OwnerResidualDraftVariables {
    fn new() -> Self {
        Self {
            entries: SmallVec::new(),
        }
    }

    fn replacement_or_insert(
        &mut self,
        variable: TypeVar,
        unifier: &mut TypeUnifier,
        owned_variables: &mut Vec<TypeVar>,
    ) -> TypeVar {
        match self
            .entries
            .binary_search_by_key(&variable, |(source, _)| *source)
        {
            Ok(index) => self.entries[index].1,
            Err(index) => {
                let instantiated = unifier.fresh();
                owned_variables.push(instantiated);
                self.entries.insert(index, (variable, instantiated));
                instantiated
            }
        }
    }

    fn replacement(&self, variable: TypeVar) -> Option<TypeVar> {
        self.entries
            .binary_search_by_key(&variable, |(source, _)| *source)
            .ok()
            .and_then(|index| self.entries.get(index))
            .map(|(_, replacement)| *replacement)
    }

    fn instantiate(
        &mut self,
        ty: &Type,
        unifier: &mut TypeUnifier,
        owned_variables: &mut Vec<TypeVar>,
    ) -> Type {
        if !owner_result_transfer_type_has_variable(ty) {
            return ty.clone();
        }
        match ty {
            Type::Var(variable) => {
                Type::Var(self.replacement_or_insert(*variable, unifier, owned_variables))
            }
            Type::Object(shape) => Type::object(ObjectShape {
                fields: shape
                    .fields
                    .iter()
                    .map(|(name, ty)| {
                        (name.clone(), self.instantiate(ty, unifier, owned_variables))
                    })
                    .collect(),
                field_order: shape.field_order.clone(),
                open: shape.open,
            }),
            Type::List(item) => Type::List(Type::shared(self.instantiate(
                item,
                unifier,
                owned_variables,
            ))),
            Type::Set(item) => Type::Set(Type::shared(self.instantiate(
                item,
                unifier,
                owned_variables,
            ))),
            Type::Map { key, value } => Type::Map {
                key: Box::new(self.instantiate(key, unifier, owned_variables)),
                value: Box::new(self.instantiate(value, unifier, owned_variables)),
            },
            Type::Function { args, result } => Type::Function {
                args: args
                    .iter()
                    .map(|argument| self.instantiate(argument, unifier, owned_variables))
                    .collect(),
                result: Box::new(FlowType {
                    mode: result.mode,
                    ty: self.instantiate(&result.ty, unifier, owned_variables),
                }),
            },
            Type::VariantSet(variants) => Type::VariantSet(
                variants
                    .iter()
                    .map(|variant| match variant {
                        Variant::Tag(tag) => Variant::Tag(tag.clone()),
                        Variant::Tagged { tag, fields } => Variant::Tagged {
                            tag: tag.clone(),
                            fields: ObjectShape {
                                fields: fields
                                    .fields
                                    .iter()
                                    .map(|(name, ty)| {
                                        (
                                            name.clone(),
                                            self.instantiate(ty, unifier, owned_variables),
                                        )
                                    })
                                    .collect(),
                                field_order: fields.field_order.clone(),
                                open: fields.open,
                            }
                            .into(),
                        },
                    })
                    .collect(),
            ),
            Type::Union(members) => Type::Union(
                members
                    .iter()
                    .map(|member| self.instantiate(member, unifier, owned_variables))
                    .collect(),
            ),
            Type::Text
            | Type::Number
            | Type::Bytes(_)
            | Type::Bits { .. }
            | Type::Absent
            | Type::RenderContract
            | Type::UnresolvedShape { .. }
            | Type::Unknown => ty.clone(),
        }
    }
}

fn owner_result_transfer_type_has_variable(ty: &Type) -> bool {
    match ty {
        Type::Var(_) => true,
        Type::Object(shape) => shape
            .fields
            .values()
            .any(owner_result_transfer_type_has_variable),
        Type::List(item) | Type::Set(item) => owner_result_transfer_type_has_variable(item),
        Type::Map { key, value } => {
            owner_result_transfer_type_has_variable(key)
                || owner_result_transfer_type_has_variable(value)
        }
        Type::Function { args, result } => {
            args.iter().any(owner_result_transfer_type_has_variable)
                || owner_result_transfer_type_has_variable(&result.ty)
        }
        Type::VariantSet(variants) => variants.iter().any(|variant| match variant {
            Variant::Tag(_) => false,
            Variant::Tagged { fields, .. } => fields
                .fields
                .values()
                .any(owner_result_transfer_type_has_variable),
        }),
        Type::Union(members) => members.iter().any(owner_result_transfer_type_has_variable),
        Type::Text
        | Type::Number
        | Type::Bytes(_)
        | Type::Bits { .. }
        | Type::Absent
        | Type::RenderContract
        | Type::UnresolvedShape { .. }
        | Type::Unknown => false,
    }
}

type OwnerResidualDraftActiveNodes = SmallVec<[usize; 16]>;

fn reinstantiate_owner_call_transfer(
    cached: &EvaluatedOwnerResult,
    unifier: &mut TypeUnifier,
) -> EvaluatedOwnerResult {
    let mut replacements = BTreeMap::new();
    let mut owned_variables = Vec::with_capacity(cached.owned_variables.len());
    for variable in &cached.owned_variables {
        let replacement = unifier.fresh();
        replacements.insert(*variable, Type::Var(replacement));
        owned_variables.push(replacement);
    }
    let mut value = cached.value.clone();
    value.flow_type.ty = apply_checked_type_substitution_lookup(&value.flow_type.ty, &replacements);
    EvaluatedOwnerResult {
        value,
        type_substitutions: cached
            .type_substitutions
            .iter()
            .map(|(variable, value)| {
                (
                    *variable,
                    apply_checked_type_substitution_lookup(value, &replacements),
                )
            })
            .collect(),
        contextual_type_variables: cached.contextual_type_variables.clone(),
        owned_variables,
    }
}

#[derive(Clone)]
struct OwnerResidualFrameRuntime {
    arguments: OwnerResidualDraftArguments,
    context: Option<EvaluatedResultValue>,
    substitutions: crate::InlineTypeSubstitutions,
    principal: FlowType,
    result_flush_type: Option<Type>,
    type_substitutions: Vec<(TypeVar, Type)>,
    contextual_type_variables: Vec<TypeVar>,
}

struct CompiledOwnerResidualProgramEvaluator<'program, 'unifier> {
    module: Option<&'program OwnerInterfaceTransferModule>,
    program: &'program OwnerResidualOwnerProgram,
    external_arguments: &'program OwnerResidualDraftArguments,
    external_context: Option<&'program EvaluatedResultValue>,
    unifier: &'unifier mut TypeUnifier,
    active_owners: &'unifier mut SmallVec<[StableCheckOwnerKey; 16]>,
    work: &'unifier mut OwnerResidualEvaluationWork,
    namespaces: Vec<OwnerResidualDraftVariables>,
    frames: Vec<Option<OwnerResidualFrameRuntime>>,
    initializing_frames: BTreeSet<OwnerResidualFrameId>,
    owned_variables: Vec<TypeVar>,
}

impl<'program, 'unifier> CompiledOwnerResidualProgramEvaluator<'program, 'unifier> {
    fn new(
        module: Option<&'program OwnerInterfaceTransferModule>,
        program: &'program OwnerResidualOwnerProgram,
        arguments: &'program OwnerResidualDraftArguments,
        context: Option<&'program EvaluatedResultValue>,
        unifier: &'unifier mut TypeUnifier,
        active_owners: &'unifier mut SmallVec<[StableCheckOwnerKey; 16]>,
        work: &'unifier mut OwnerResidualEvaluationWork,
    ) -> Self {
        Self {
            module,
            program,
            external_arguments: arguments,
            external_context: context,
            unifier,
            active_owners,
            work,
            namespaces: program
                .namespaces
                .iter()
                .map(|_| OwnerResidualDraftVariables::new())
                .collect(),
            frames: vec![None; program.frames.len()],
            initializing_frames: BTreeSet::new(),
            owned_variables: Vec::new(),
        }
    }

    fn initialize_frame(&mut self, frame: OwnerResidualFrameId) -> Option<()> {
        if self.frames.get(frame as usize)?.is_some() {
            return Some(());
        }
        if !self.initializing_frames.insert(frame) {
            return None;
        }
        let descriptor = self.program.frames.get(frame as usize)?.clone();
        let runtime = (|| {
            let mut arguments = OwnerResidualDraftArguments::default();
            for parameter in &descriptor.parameters {
                if let Some(value) = self.external_arguments.get(&parameter.ordinal).cloned() {
                    arguments.insert(parameter.ordinal, value);
                }
            }
            let context = descriptor
                .context
                .as_ref()
                .and(self.external_context.cloned());
            let namespace = descriptor.namespace as usize;
            let variables = self.namespaces.get_mut(namespace)?;
            let mut substitutions = crate::InlineTypeSubstitutions::default();
            for parameter in &descriptor.parameters {
                if let Some(actual) = arguments.get(&parameter.ordinal) {
                    let formal = variables.instantiate(
                        &parameter.flow_type.ty,
                        self.unifier,
                        &mut self.owned_variables,
                    );
                    let actual = self.unifier.resolve(&actual.flow_type.ty);
                    if !self.unifier.bind_call_pattern_input(&actual, &formal) {
                        crate::unify_checked_type_pattern(&formal, &actual, &mut substitutions);
                    }
                }
            }
            if let (Some(formal), Some(actual)) = (&descriptor.context, &context) {
                let formal = variables.instantiate(
                    &formal.flow_type.ty,
                    self.unifier,
                    &mut self.owned_variables,
                );
                let actual = self.unifier.resolve(&actual.flow_type.ty);
                if !self.unifier.bind_call_pattern_input(&actual, &formal) {
                    crate::unify_checked_type_pattern(&formal, &actual, &mut substitutions);
                }
            }
            let instantiated_result = variables.instantiate(
                &descriptor.result.ty,
                self.unifier,
                &mut self.owned_variables,
            );
            for variable in &descriptor.type_variables {
                let Some(instantiated) = variables.replacement(*variable) else {
                    continue;
                };
                if substitutions.replacement(instantiated).is_some() {
                    continue;
                }
                let resolved = self.unifier.resolve(&Type::Var(instantiated));
                if resolved != Type::Var(instantiated) {
                    substitutions.insert(instantiated, resolved);
                }
            }
            let principal = FlowType {
                mode: descriptor.result.mode,
                ty: apply_checked_type_substitution_lookup(&instantiated_result, &substitutions),
            };
            let result_flush_type = descriptor.result_flush_type.as_ref().map(|flush_type| {
                let flush_type =
                    variables.instantiate(flush_type, self.unifier, &mut self.owned_variables);
                apply_checked_type_substitution_lookup(&flush_type, &substitutions)
            });
            let mut contextual_type_variables = BTreeSet::new();
            if let Some(context) = &descriptor.context {
                crate::collect_type_vars(&context.flow_type.ty, &mut contextual_type_variables);
            }
            let type_substitutions = descriptor
                .type_variables
                .iter()
                .filter_map(|variable| {
                    let instantiated = variables.replacement(*variable)?;
                    substitutions.replacement(instantiated).map(|value| {
                        (
                            *variable,
                            apply_checked_type_substitution_lookup(value, &substitutions),
                        )
                    })
                })
                .collect();
            Some(OwnerResidualFrameRuntime {
                arguments,
                context,
                substitutions,
                principal,
                result_flush_type,
                type_substitutions,
                contextual_type_variables: contextual_type_variables.into_iter().collect(),
            })
        })();
        self.initializing_frames.remove(&frame);
        let runtime = runtime?;
        *self.frames.get_mut(frame as usize)? = Some(runtime);
        Some(())
    }

    fn parameter_value(
        &self,
        frame: OwnerResidualFrameId,
        parameter_ordinal: u32,
        projection: &[String],
    ) -> Option<EvaluatedResultValue> {
        let actual = self
            .frames
            .get(frame as usize)?
            .as_ref()?
            .arguments
            .get(&parameter_ordinal)?;
        let ty = if projection.is_empty() {
            Some(actual.flow_type.ty.clone())
        } else {
            crate::type_for_nested_path(&actual.flow_type.ty, projection)
        }?;
        Some(EvaluatedResultValue {
            flow_type: FlowType {
                mode: actual.flow_type.mode,
                ty,
            },
            parameter_derived: true,
            syntax_selected: actual.syntax_selected,
            static_number: projection
                .is_empty()
                .then(|| actual.static_number.clone())
                .flatten(),
        })
    }

    fn resolve_type(
        &mut self,
        namespace: OwnerResidualNamespaceId,
        ty: &Type,
        substitutions: Option<&crate::InlineTypeSubstitutions>,
    ) -> Option<Type> {
        let variables = self.namespaces.get_mut(namespace as usize)?;
        let ty = variables.instantiate(ty, self.unifier, &mut self.owned_variables);
        Some(substitutions.map_or(ty.clone(), |substitutions| {
            apply_checked_type_substitution_lookup(&ty, substitutions)
        }))
    }

    fn fallback(&mut self, op: &OwnerResidualOp) -> Option<EvaluatedResultValue> {
        let frame = self.program.frames.get(op.frame as usize)?;
        let substitutions = self
            .frames
            .get(op.frame as usize)?
            .as_ref()?
            .substitutions
            .clone();
        let ty = self.resolve_type(frame.namespace, &op.fallback.ty, Some(&substitutions))?;
        Some(EvaluatedResultValue {
            flow_type: FlowType {
                mode: op.fallback.mode,
                ty,
            },
            parameter_derived: false,
            syntax_selected: false,
            static_number: op.static_number.clone(),
        })
    }

    fn surface(
        &mut self,
        op: &OwnerResidualOp,
        namespace: OwnerResidualNamespaceId,
    ) -> Option<EvaluatedResultValue> {
        let ty = self.resolve_type(namespace, &op.fallback.ty, None)?;
        Some(EvaluatedResultValue {
            flow_type: FlowType {
                mode: op.fallback.mode,
                ty: self.unifier.resolve(&ty),
            },
            parameter_derived: false,
            syntax_selected: false,
            static_number: None,
        })
    }

    fn evaluate_root(
        &mut self,
        root: &OwnerResidualRoot,
        active: &mut OwnerResidualDraftActiveNodes,
        constant: Option<&EvaluatedResultValue>,
    ) -> Option<EvaluatedOwnerResult> {
        let frame = root.frame();
        self.initialize_frame(frame)?;
        let evaluated = if let Some(constant) = constant {
            Some(constant.clone())
        } else {
            match root {
                OwnerResidualRoot::Principal { .. } => None,
                OwnerResidualRoot::Parameter { read, .. } => {
                    self.parameter_value(frame, read.parameter_ordinal, &read.projection)
                }
                OwnerResidualRoot::Value { op, .. } => {
                    self.evaluate_op(*op, &BTreeMap::new(), active)
                }
            }
        };
        let runtime = self.frames.get(frame as usize)?.as_ref()?.clone();
        let mut value = if let Some(mut evaluated) = evaluated {
            let selected = evaluated.syntax_selected
                && crate::type_has_concrete_outer_shape(&evaluated.flow_type.ty);
            let evaluated_is_closed =
                boon_checked::type_is_recursively_closed(&evaluated.flow_type.ty);
            evaluated.flow_type.ty = if selected || evaluated_is_closed {
                evaluated.flow_type.ty
            } else {
                specialize_checked_call_result(&runtime.principal.ty, &evaluated.flow_type.ty)
            };
            evaluated.syntax_selected = selected;
            evaluated
        } else {
            EvaluatedResultValue {
                flow_type: runtime.principal,
                parameter_derived: runtime
                    .arguments
                    .values()
                    .any(|value| value.parameter_derived),
                syntax_selected: false,
                static_number: None,
            }
        };
        if let Some(flush_type) = runtime.result_flush_type {
            value.flow_type.ty =
                boon_checked::canonical_union_type(vec![value.flow_type.ty, flush_type]);
            if value.flow_type.mode == FlowMode::Absent {
                value.flow_type.mode = FlowMode::Continuous;
            }
        }
        Some(EvaluatedOwnerResult {
            value,
            type_substitutions: runtime.type_substitutions,
            contextual_type_variables: runtime.contextual_type_variables,
            owned_variables: Vec::new(),
        })
    }

    fn evaluate_op(
        &mut self,
        op: OwnerResidualOpId,
        lexical: &BTreeMap<String, EvaluatedResultValue>,
        active: &mut OwnerResidualDraftActiveNodes,
    ) -> Option<EvaluatedResultValue> {
        self.work.op_visits = self.work.op_visits.saturating_add(1);
        let index = op as usize;
        let op = self.program.ops.get(index)?.clone();
        if active.contains(&index) {
            return self.fallback(&op);
        }
        active.push(index);
        let value = self.evaluate_op_inner(&op, lexical, active);
        debug_assert_eq!(active.pop(), Some(index));
        value
    }

    fn evaluate_op_inner(
        &mut self,
        op: &OwnerResidualOp,
        lexical: &BTreeMap<String, EvaluatedResultValue>,
        active: &mut OwnerResidualDraftActiveNodes,
    ) -> Option<EvaluatedResultValue> {
        match &op.kind {
            OwnerResidualOpKind::Fallback => self.fallback(op),
            OwnerResidualOpKind::Surface { namespace } => self.surface(op, *namespace),
            OwnerResidualOpKind::ParameterRead {
                parameter_ordinal,
                projection,
            } => self
                .parameter_value(op.frame, *parameter_ordinal, projection)
                .or_else(|| self.fallback(op)),
            OwnerResidualOpKind::LexicalRead { parts } => {
                let value = parts.split_first().and_then(|(name, projection)| {
                    lexical.get(name).and_then(|value| {
                        let ty = if projection.is_empty() {
                            Some(value.flow_type.ty.clone())
                        } else {
                            crate::type_for_nested_path(&value.flow_type.ty, projection)
                        }?;
                        Some(EvaluatedResultValue {
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
                        })
                    })
                });
                value.or_else(|| self.fallback(op))
            }
            OwnerResidualOpKind::CompiledCall {
                target,
                actuals,
                context,
            } => {
                self.work.compiled_call_dispatches =
                    self.work.compiled_call_dispatches.saturating_add(1);
                let mut arguments = OwnerResidualDraftArguments::default();
                for actual in actuals {
                    arguments.insert(
                        actual.formal_ordinal,
                        self.evaluate_op(actual.value, lexical, active)?,
                    );
                }
                let context = match context {
                    OwnerResidualCallContext::Inherited => self
                        .frames
                        .get(op.frame as usize)
                        .and_then(Option::as_ref)
                        .and_then(|runtime| runtime.context.clone()),
                    OwnerResidualCallContext::Explicit { value } => {
                        Some(self.evaluate_op(*value, lexical, active)?)
                    }
                };
                let current_module = self.module?;
                let (module, owner) = match target {
                    OwnerResidualCompiledCallTarget::Own { owner } => {
                        (current_module, *owner as usize)
                    }
                    OwnerResidualCompiledCallTarget::Dependency { dependency, owner } => (
                        current_module
                            .dependencies
                            .get(*dependency as usize)?
                            .as_ref(),
                        *owner as usize,
                    ),
                };
                let result = evaluate_compiled_owner_in_module(
                    module,
                    owner,
                    &arguments,
                    context.as_ref(),
                    self.unifier,
                    self.active_owners,
                    self.work,
                )?;
                self.owned_variables.extend(result.owned_variables);
                Some(result.value)
            }
            OwnerResidualOpKind::AbiCall {
                canonical_name,
                contract,
                actuals,
            } => self
                .evaluate_abi_call(canonical_name, contract, actuals, lexical, active)
                .or_else(|| self.fallback(op)),
            OwnerResidualOpKind::Infix {
                operation,
                left,
                right,
            } => {
                let left = self.evaluate_op(*left, lexical, active)?;
                let right = self.evaluate_op(*right, lexical, active)?;
                let static_number = left
                    .static_number
                    .as_ref()
                    .zip(right.static_number.as_ref())
                    .and_then(|(left, right)| static_number_infix(left, operation, right));
                let fallback = self.fallback(op)?;
                Some(EvaluatedResultValue {
                    static_number,
                    parameter_derived: left.parameter_derived || right.parameter_derived,
                    syntax_selected: left.syntax_selected || right.syntax_selected,
                    ..fallback
                })
            }
            OwnerResidualOpKind::Record { tag, fields } => {
                let mut resolved = Vec::with_capacity(fields.len());
                let mut parameter_derived = false;
                let mut syntax_selected = false;
                for field in fields {
                    let value = self.evaluate_op(field.value, lexical, active)?;
                    parameter_derived |= value.parameter_derived;
                    syntax_selected |= value.syntax_selected;
                    resolved.push((field.name.clone(), value.flow_type.ty));
                }
                let shape: ObjectShape = ObjectShape::from_ordered_fields(resolved, false);
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
            OwnerResidualOpKind::When { selector, arms } => {
                let selector = self.evaluate_op(*selector, lexical, active)?;
                let selector_is_concrete =
                    crate::type_is_singleton_syntax_discriminant(&selector.flow_type.ty);
                let mut outputs = Vec::new();
                for arm in arms {
                    if selector_is_concrete
                        && !owner_pattern_accepts(&selector.flow_type.ty, &arm.pattern)
                    {
                        continue;
                    }
                    let mut arm_lexical = lexical.clone();
                    extend_owner_pattern_bindings(&mut arm_lexical, &selector, &arm.pattern);
                    if let Some(output) = self.evaluate_op(arm.output, &arm_lexical, active)
                        && !matches!(output.flow_type.ty, Type::Absent)
                    {
                        outputs.push(output);
                        if selector_is_concrete {
                            break;
                        }
                    }
                }
                let mut outputs = outputs.into_iter();
                let first = outputs.next()?;
                let result = outputs.fold(first, |mut result, next| {
                    result.flow_type.ty =
                        widen_structural_type(&result.flow_type.ty, &next.flow_type.ty);
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
            OwnerResidualOpKind::Forward { output } => output
                .and_then(|output| self.evaluate_op(output, lexical, active))
                .or_else(|| self.fallback(op)),
            OwnerResidualOpKind::Block { bindings, result } => {
                let mut lexical = lexical.clone();
                let mut parameter_derived = false;
                let mut syntax_selected = false;
                for binding in bindings {
                    let value = self.evaluate_op(binding.value, &lexical, active)?;
                    parameter_derived |= value.parameter_derived;
                    syntax_selected |= value.syntax_selected;
                    lexical.insert(binding.name.clone(), value);
                }
                if let Some(result) = result {
                    let mut value = self.evaluate_op(*result, &lexical, active)?;
                    value.parameter_derived |= parameter_derived;
                    value.syntax_selected |= syntax_selected;
                    Some(value)
                } else {
                    self.fallback(op)
                }
            }
            OwnerResidualOpKind::Collection { collection, items } => {
                let values = items
                    .iter()
                    .map(|item| self.evaluate_op(*item, lexical, active))
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
                    OwnerCollectionKind::Bytes | OwnerCollectionKind::Map => {
                        self.fallback(op)?.flow_type.ty
                    }
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
            OwnerResidualOpKind::Latest { branches } => {
                let values = branches
                    .iter()
                    .map(|branch| self.evaluate_op(*branch, lexical, active))
                    .collect::<Option<Vec<_>>>()?;
                let ty = values
                    .iter()
                    .filter(|value| !matches!(value.flow_type.ty, Type::Absent))
                    .map(|value| value.flow_type.ty.clone())
                    .reduce(|left, right| widen_structural_type(&left, &right))
                    .unwrap_or(self.fallback(op)?.flow_type.ty);
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
            OwnerResidualOpKind::Draining { input } => input
                .and_then(|input| self.evaluate_op(input, lexical, active))
                .or_else(|| self.fallback(op)),
            OwnerResidualOpKind::Hold { initial, updates } => {
                let Some(mut value) =
                    initial.and_then(|initial| self.evaluate_op(initial, lexical, active))
                else {
                    return self.fallback(op);
                };
                for update in updates {
                    let update = self.evaluate_op(*update, lexical, active)?;
                    if !matches!(update.flow_type.ty, Type::Absent) {
                        value.flow_type.ty = crate::widen_checked_hold_type(
                            &value.flow_type.ty,
                            &update.flow_type.ty,
                        );
                    }
                    value.parameter_derived |= update.parameter_derived;
                    value.syntax_selected |= update.syntax_selected;
                }
                value.flow_type.mode = FlowMode::Continuous;
                value.static_number = None;
                Some(value)
            }
            OwnerResidualOpKind::Then { value } => value
                .and_then(|value| self.evaluate_op(value, lexical, active))
                .map(|mut value| {
                    value.flow_type.mode = FlowMode::PresentOrAbsent;
                    value
                }),
            OwnerResidualOpKind::MapEntry { key, value } => {
                let key = self.evaluate_op((*key)?, lexical, active)?;
                let value = self.evaluate_op((*value)?, lexical, active)?;
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
            OwnerResidualOpKind::Source => {
                let fallback = self.fallback(op)?;
                Some(EvaluatedResultValue {
                    flow_type: FlowType {
                        mode: FlowMode::PresentOrAbsent,
                        ty: fallback.flow_type.ty,
                    },
                    ..fallback
                })
            }
            OwnerResidualOpKind::Skip => Some(EvaluatedResultValue {
                flow_type: FlowType {
                    mode: FlowMode::Absent,
                    ty: Type::Absent,
                },
                parameter_derived: false,
                syntax_selected: false,
                static_number: None,
            }),
        }
    }

    fn evaluate_abi_call(
        &mut self,
        function: &str,
        contract: &OwnerResidualAbiContract,
        inputs: &[OwnerResidualActual],
        lexical: &BTreeMap<String, EvaluatedResultValue>,
        active: &mut OwnerResidualDraftActiveNodes,
    ) -> Option<EvaluatedResultValue> {
        let mut actuals = OwnerResidualDraftArguments::default();
        let mut instantiation = crate::InlineTypeSubstitutions::default();
        for input in inputs {
            let parameter = contract
                .parameters
                .binary_search_by_key(&input.formal_ordinal, |parameter| parameter.ordinal)
                .ok()
                .and_then(|index| contract.parameters.get(index))?;
            let actual = self.evaluate_op(input.value, lexical, active)?;
            crate::unify_checked_type_pattern(
                &parameter.flow_type.ty,
                &actual.flow_type.ty,
                &mut instantiation,
            );
            actuals.insert(parameter.ordinal, actual);
        }
        let mut ty = apply_checked_type_substitution_lookup(&contract.result.ty, &instantiation);
        ty = crate::specialize_owner_abi_result_type(
            &ty,
            contract.result_specialization,
            contract.parameters.iter().filter_map(|parameter| {
                actuals
                    .get(&parameter.ordinal)
                    .map(|actual| (parameter.name.clone(), actual.flow_type.ty.clone()))
            }),
        );
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
        } else if contract.kind == CheckedCallableKind::External {
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

    fn evaluate(
        mut self,
        constant: Option<&EvaluatedResultValue>,
        principal_only: bool,
    ) -> Option<EvaluatedOwnerResult> {
        let root = if principal_only {
            OwnerResidualRoot::Principal {
                frame: self.program.root.frame(),
            }
        } else {
            self.program.root.clone()
        };
        let mut result = self.evaluate_root(
            &root,
            &mut OwnerResidualDraftActiveNodes::new(),
            (!principal_only).then_some(constant).flatten(),
        )?;
        result.owned_variables = self.owned_variables;
        Some(result)
    }
}

fn evaluate_compiled_owner_in_module(
    module: &OwnerInterfaceTransferModule,
    owner: usize,
    arguments: &OwnerResidualDraftArguments,
    context: Option<&EvaluatedResultValue>,
    unifier: &mut TypeUnifier,
    active_owners: &mut SmallVec<[StableCheckOwnerKey; 16]>,
    work: &mut OwnerResidualEvaluationWork,
) -> Option<EvaluatedOwnerResult> {
    let owner_key = module.result.key.members.get(owner)?;
    let program = module.program.owners.get(owner)?;
    let principal_only = active_owners.iter().any(|active| active == owner_key);
    if !principal_only {
        active_owners.push(owner_key.clone());
    }
    work.owner_dispatches = work.owner_dispatches.saturating_add(1);
    work.maximum_owner_depth = work.maximum_owner_depth.max(active_owners.len() as u64);
    let result = {
        CompiledOwnerResidualProgramEvaluator::new(
            Some(module),
            program,
            arguments,
            context,
            unifier,
            active_owners,
            work,
        )
        .evaluate(module.constant_result_at(owner), principal_only)
    };
    if !principal_only {
        debug_assert_eq!(active_owners.pop().as_ref(), Some(owner_key));
    }
    result
}

struct CompiledOwnerResidualEvaluator<'a, 'unifier> {
    providers: &'a BTreeMap<StableCheckOwnerKey, &'a OwnerInterfaceTransferModule>,
    unifier: &'unifier mut TypeUnifier,
    active_owners: SmallVec<[StableCheckOwnerKey; 16]>,
    work: OwnerResidualEvaluationWork,
}

impl<'a, 'unifier> CompiledOwnerResidualEvaluator<'a, 'unifier> {
    fn new(
        providers: &'a BTreeMap<StableCheckOwnerKey, &'a OwnerInterfaceTransferModule>,
        unifier: &'unifier mut TypeUnifier,
    ) -> Self {
        Self {
            providers,
            unifier,
            active_owners: SmallVec::new(),
            work: OwnerResidualEvaluationWork::default(),
        }
    }

    fn evaluate_owner(
        &mut self,
        owner: &StableCheckOwnerKey,
        arguments: &OwnerResidualDraftArguments,
        context: Option<&EvaluatedResultValue>,
    ) -> Option<EvaluatedOwnerResult> {
        let module = *self.providers.get(owner)?;
        self.evaluate_owner_in_module(module, owner, arguments, context)
    }

    fn evaluate_owner_in_module(
        &mut self,
        module: &OwnerInterfaceTransferModule,
        owner: &StableCheckOwnerKey,
        arguments: &OwnerResidualDraftArguments,
        context: Option<&EvaluatedResultValue>,
    ) -> Option<EvaluatedOwnerResult> {
        let owner = module.result.key.members.binary_search(owner).ok()?;
        evaluate_compiled_owner_in_module(
            module,
            owner,
            arguments,
            context,
            self.unifier,
            &mut self.active_owners,
            &mut self.work,
        )
    }
}

pub(crate) struct OwnerResidualOccurrenceEvaluation {
    pub(crate) value: EvaluatedResultValue,
    pub(crate) work: OwnerResidualEvaluationWork,
}

/// Evaluate one call occurrence against an SCC-sealed result-transfer module.
///
/// Interface inference and body inference share this exact specialization
/// authority. Every invocation receives a fresh alpha frame in `unifier`, and
/// only the occurrence result is returned; the callable's generic public
/// interface is never constrained by the caller.
pub(crate) fn evaluate_owner_result_transfer_occurrence(
    module: &OwnerInterfaceTransferModule,
    owner: &StableCheckOwnerKey,
    arguments: &OwnerResidualDraftArguments,
    context: Option<&EvaluatedResultValue>,
    unifier: &mut TypeUnifier,
) -> Option<OwnerResidualOccurrenceEvaluation> {
    let providers = BTreeMap::new();
    let mut evaluator = CompiledOwnerResidualEvaluator::new(&providers, unifier);
    let value = evaluator
        .evaluate_owner_in_module(module, owner, arguments, context)
        .map(|result| result.value)?;
    evaluator.work.occurrences = 1;
    Some(OwnerResidualOccurrenceEvaluation {
        value,
        work: evaluator.work,
    })
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

fn abi_actual_by_name<'a>(
    contract: &OwnerResidualAbiContract,
    actuals: &'a OwnerResidualDraftArguments,
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
            parameters: interface
                .parameters
                .iter()
                .map(|parameter| InstantiatedCallParameter {
                    name: parameter.name.clone(),
                    ordinal: parameter.ordinal,
                    flow_type: FlowType {
                        mode: parameter.flow_type.mode,
                        ty: instantiate_type(&parameter.flow_type.ty, unifier, &mut variables),
                    },
                })
                .collect(),
            contexts: Vec::new(),
            result: FlowType {
                mode: interface.result.mode,
                ty: instantiate_type(&interface.result.ty, unifier, &mut variables),
            },
            result_specialization: crate::OwnerAbiResultSpecialization::Fixed,
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
    abi.callable(&call.function).and_then(|signature| {
        let parameters = signature
            .parameters
            .iter()
            .map(|parameter| InstantiatedCallParameter {
                name: parameter.name.clone(),
                ordinal: parameter.ordinal,
                flow_type: FlowType {
                    mode: parameter.flow_type.mode,
                    ty: instantiate_type(&parameter.flow_type.ty, unifier, &mut variables),
                },
            })
            .collect();
        let contexts = signature
            .contexts
            .iter()
            .enumerate()
            .map(|(ordinal, context)| {
                Some(InstantiatedCallContext {
                    ordinal: u32::try_from(ordinal).ok()?,
                    name: context.name.clone(),
                    provider_parameter_ordinal: context.provider_parameter_ordinal,
                    flow_type: FlowType {
                        mode: context.flow_type.mode,
                        ty: instantiate_type(&context.flow_type.ty, unifier, &mut variables),
                    },
                })
            })
            .collect::<Option<Vec<_>>>()?;
        Some(InstantiatedCallSignature {
            parameters,
            contexts,
            result: FlowType {
                mode: signature.result.mode,
                ty: instantiate_type(&signature.result.ty, unifier, &mut variables),
            },
            result_specialization: signature.result_specialization,
            result_flush_type: None,
            context: None,
            effect: signature.effect,
            target: InferredOwnerCallableTarget::Authoritative,
        })
    })
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

#[allow(clippy::too_many_arguments)]
fn bind_signature_declaration_reads(
    target: &OwnerSignatureDeclarationTarget,
    root: TypeVar,
    mode: FlowMode,
    signature_lexical_plan: &OwnerSignatureLexicalPlan,
    signature_read_expressions: &BTreeMap<OwnerSignatureDeclarationTarget, Vec<usize>>,
    unifier: &mut TypeUnifier,
    expressions: &[TypeVar],
    modes: &mut [Option<FlowMode>],
) {
    bind_signature_declaration_read_types(
        target,
        root,
        signature_lexical_plan,
        signature_read_expressions,
        unifier,
        expressions,
    );
    for expression in signature_read_expressions.get(target).into_iter().flatten() {
        modes[*expression] = flow_mode_join(modes[*expression], Some(mode));
    }
}

fn bind_signature_declaration_read_types(
    target: &OwnerSignatureDeclarationTarget,
    root: TypeVar,
    signature_lexical_plan: &OwnerSignatureLexicalPlan,
    signature_read_expressions: &BTreeMap<OwnerSignatureDeclarationTarget, Vec<usize>>,
    unifier: &mut TypeUnifier,
    expressions: &[TypeVar],
) {
    for expression in signature_read_expressions.get(target).into_iter().flatten() {
        let Some(read) = signature_lexical_plan.reads()[*expression].as_ref() else {
            continue;
        };
        bind_signature_declaration_read_type(
            unifier,
            root,
            expressions[*expression],
            &read.projection,
        );
    }
}

fn bind_signature_declaration_read_type(
    unifier: &mut TypeUnifier,
    root: TypeVar,
    occurrence: TypeVar,
    projection: &[String],
) {
    let projected = bind_projection(unifier, root, projection);
    let provider = unifier.resolve(&Type::Var(projected));
    if boon_checked::type_is_recursively_closed(&provider) {
        // A contextual provider can close after its first projected read
        // created a sparse consumer scaffold. The closed FreshOut/context
        // surface owns this occurrence; replacing the detached scaffold keeps
        // later consumers from retaining a stale private alpha.
        unifier.publish_authoritative_provider(occurrence, provider);
    } else {
        // An open provider still carries live inference holes. Preserve the
        // ordinary equation so downstream requirements can reach it.
        unifier.unify(Type::Var(occurrence), Type::Var(projected));
    }
}

fn replay_signature_declaration_read_types(
    signature_declaration_variables: &BTreeMap<OwnerSignatureDeclarationTarget, TypeVar>,
    signature_lexical_plan: &OwnerSignatureLexicalPlan,
    signature_read_expressions: &BTreeMap<OwnerSignatureDeclarationTarget, Vec<usize>>,
    unifier: &mut TypeUnifier,
    expressions: &[TypeVar],
) {
    for (target, root) in signature_declaration_variables {
        bind_signature_declaration_read_types(
            target,
            *root,
            signature_lexical_plan,
            signature_read_expressions,
            unifier,
            expressions,
        );
    }
}

fn signature_input_anchor_role(
    source: OwnerSignatureMatchedInputSource,
) -> Option<OwnerSourceAnchorRole> {
    match source {
        OwnerSignatureMatchedInputSource::PipeInput => None,
        OwnerSignatureMatchedInputSource::CallArgument { ordinal } => {
            Some(OwnerSourceAnchorRole::CallArgument { ordinal })
        }
        OwnerSignatureMatchedInputSource::PipeArgument { ordinal } => {
            Some(OwnerSourceAnchorRole::PipeArgument { ordinal })
        }
    }
}

fn signature_pass_anchor_role(source: OwnerSignaturePassSource) -> OwnerSourceAnchorRole {
    match source {
        OwnerSignaturePassSource::Call => OwnerSourceAnchorRole::CallPass,
        OwnerSignaturePassSource::Pipe => OwnerSourceAnchorRole::PipePass,
    }
}

fn push_signature_call_lexical_diagnostics(
    call: &OwnerSignatureCallPlan,
    diagnostics: &mut Vec<OwnerDiagnosticTemplate>,
) {
    for error in &call.lexical_errors {
        let (code, message, role) = match error {
            OwnerSignatureCallLexicalError::PipeWithoutValueInput => (
                "pipe_without_value_input",
                format!("`{}` has no ordinary input for the pipe", call.function),
                None,
            ),
            OwnerSignatureCallLexicalError::UnexpectedCallEntry { name, source } => (
                "unexpected_call_entry",
                format!(
                    "`{}` has an unexpected extra call entry `{name}`",
                    call.function
                ),
                signature_input_anchor_role(*source),
            ),
            OwnerSignatureCallLexicalError::MisorderedCallEntry {
                position,
                expected_name,
                actual_name,
                source,
            } => (
                "misordered_call_entry",
                format!(
                    "`{}` call entry {position} must be `{expected_name}`, found `{actual_name}`; arguments keep declaration names and order",
                    call.function
                ),
                signature_input_anchor_role(*source),
            ),
            OwnerSignatureCallLexicalError::MissingCallEntry { name } => (
                "missing_call_entry",
                format!("`{}` is missing call entry `{name}`", call.function),
                None,
            ),
            OwnerSignatureCallLexicalError::BareOrdinaryInput { name, source } => (
                "bare_ordinary_input",
                format!(
                    "bare `{name}` cannot fill ordinary input `{name}`; write `{name}: expression`"
                ),
                signature_input_anchor_role(*source),
            ),
            OwnerSignatureCallLexicalError::PassOnAuthoritativeCallable {
                source,
                callable_kind,
            } => (
                "pass_on_authoritative_callable",
                format!(
                    "`PASS:` is only valid on user callable calls; `{}` is {}",
                    call.function,
                    match callable_kind {
                        CheckedCallableKind::Builtin => "a built-in callable",
                        CheckedCallableKind::External => "an external callable",
                        CheckedCallableKind::User => "authoritative",
                    }
                ),
                Some(signature_pass_anchor_role(*source)),
            ),
            OwnerSignatureCallLexicalError::InvalidForwardOutTarget {
                formal_ordinal,
                formal_name,
                expression,
            } => (
                "invalid_forward_out_target",
                format!(
                    "output parameter `{formal_name}` must be bare for a fresh output or name one existing OUT"
                ),
                call.matched_inputs
                    .iter()
                    .find(|input| {
                        input.formal_ordinal == *formal_ordinal && input.expression == *expression
                    })
                    .and_then(|input| signature_input_anchor_role(input.source)),
            ),
            OwnerSignatureCallLexicalError::MissingEnclosingOut {
                formal_ordinal,
                formal_name,
                expression,
                target_name,
            } => (
                "missing_enclosing_out",
                format!(
                    "no enclosing OUT named `{target_name}` exists for output parameter `{formal_name}`"
                ),
                call.matched_inputs
                    .iter()
                    .find(|input| {
                        input.formal_ordinal == *formal_ordinal && input.expression == *expression
                    })
                    .and_then(|input| signature_input_anchor_role(input.source)),
            ),
            OwnerSignatureCallLexicalError::DuplicateCallContext { name } => (
                "duplicate_call_context",
                format!(
                    "callable `{}` declares call context `{name}` more than once",
                    call.function
                ),
                None,
            ),
        };
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
}

fn bind_calls(
    calls: Vec<BodyCallPlan>,
    seed: &OwnerConstraintSeed,
    signature_lexical_plan: &OwnerSignatureLexicalPlan,
    signature_dynamic_expressions: &[bool],
    signature_declaration_variables: &BTreeMap<OwnerSignatureDeclarationTarget, TypeVar>,
    signature_read_expressions: &BTreeMap<OwnerSignatureDeclarationTarget, Vec<usize>>,
    interfaces: &BTreeMap<StableCheckOwnerKey, &OwnerPublicInterface>,
    unifier: &mut TypeUnifier,
    expressions: &[TypeVar],
    external_expressions: &[TypeVar],
    call_flushes: &[TypeVar],
    modes: &mut [Option<FlowMode>],
    direct_effects: &mut [CheckedEffectSummary],
    abi: &OwnerInferenceAbiEnvironment,
    caller_has_context: bool,
    caller_is_callable: bool,
    diagnostics: &mut Vec<OwnerDiagnosticTemplate>,
    work: &mut OwnerBodyInferenceWork,
) -> Vec<InferredCallDraft> {
    let mut calls = calls;
    calls.sort_by_key(|call| {
        signature_lexical_plan
            .call(call.expression)
            .map_or(u32::MAX, |call| call.structural_ordinal)
    });
    calls
        .into_iter()
        .map(|call| {
            work.calls = work.calls.saturating_add(1);
            let signature = instantiate_call_signature(&call, interfaces, unifier, abi);
            let call_variable = expressions[call.expression];
            let (
                parameters,
                contexts,
                result,
                result_specialization,
                result_flush_type,
                context,
                effect,
                target,
                mut valid,
            ) = match signature {
                Some(signature) => (
                    signature.parameters,
                    signature.contexts,
                    signature.result,
                    signature.result_specialization,
                    signature.result_flush_type,
                    signature.context,
                    signature.effect,
                    signature.target,
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
                        Vec::new(),
                        FlowType {
                            mode: FlowMode::Continuous,
                            ty: Type::Unknown,
                        },
                        crate::OwnerAbiResultSpecialization::Fixed,
                        None,
                        None,
                        CheckedEffectSummary::default(),
                        target,
                        false,
                    )
                }
            };
            let signature_call = signature_lexical_plan.call(call.expression);
            if let Some(signature_call) = signature_call {
                push_signature_call_lexical_diagnostics(signature_call, diagnostics);
            }
            valid &= signature_call.is_some_and(|call| call.valid);
            if valid
                && context.is_some()
                && signature_call.is_some_and(|call| call.explicit_pass.is_none())
                && !caller_has_context
                && matches!(target, InferredOwnerCallableTarget::Owner { .. })
            {
                push_owner_call_diagnostic(
                    diagnostics,
                    &call,
                    "missing_pass_context",
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
                    },
                    None,
                );
                valid = false;
            }
            let result = FlowType {
                mode: result.mode,
                ty: crate::specialize_owner_abi_result_type(
                    &result.ty,
                    result_specialization,
                    signature_call
                        .into_iter()
                        .flat_map(|call| &call.matched_inputs)
                        .filter_map(|planned| {
                            let parameter = parameters
                                .binary_search_by_key(&planned.formal_ordinal, |parameter| {
                                    parameter.ordinal
                                })
                                .ok()
                                .and_then(|index| parameters.get(index))?;
                            let input = expression_variable(
                                expressions,
                                external_expressions,
                                planned.expression,
                            )?;
                            Some((parameter.name.clone(), Type::Var(input)))
                        }),
                ),
            };
            if valid && let Some(field) = call.function.strip_prefix("Field/") {
                if let Some(input) = signature_call.and_then(|call| {
                    call.matched_inputs
                        .iter()
                        .find(|input| input.source == OwnerSignatureMatchedInputSource::PipeInput)
                        .map(|input| input.expression)
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

            if valid && let Some(signature_call) = signature_call {
                // Provider/static inputs must settle shared signature
                // variables before projected FreshOut/CallContext reads can
                // open a structural shape. This preserves an exact closed
                // provider value while still staging every dynamic consumer.
                for planned in signature_call.matched_inputs.iter().filter(|planned| {
                    !signature_dynamic_expressions
                        .get(planned.expression as usize)
                        .copied()
                        .unwrap_or(false)
                }) {
                    let Some(input) =
                        expression_variable(expressions, external_expressions, planned.expression)
                    else {
                        continue;
                    };
                    if let Some(expected) = parameters
                        .binary_search_by_key(&planned.formal_ordinal, |parameter| {
                            parameter.ordinal
                        })
                        .ok()
                        .and_then(|index| parameters.get(index))
                    {
                        unifier.bind_call_input(input, expected.flow_type.ty.clone());
                    }
                }
                if let (Some(pass), Some(context)) = (&signature_call.explicit_pass, &context)
                    && !signature_dynamic_expressions
                        .get(pass.expression as usize)
                        .copied()
                        .unwrap_or(false)
                    && let Some(input) =
                        expression_variable(expressions, external_expressions, pass.expression)
                {
                    unifier.bind_call_input(input, context.clone());
                }
            }
            if valid
                && let Some(signature_call) = signature_call
                && signature_call.valid
            {
                for output in &signature_call.outputs {
                    let crate::OwnerSignatureOutputBindingPlan::Fresh { target, .. } = output
                    else {
                        continue;
                    };
                    let Some(variable) = signature_declaration_variables.get(target).copied()
                    else {
                        continue;
                    };
                    let Some(parameter) = parameters
                        .iter()
                        .find(|parameter| parameter.ordinal == output.formal_ordinal())
                    else {
                        continue;
                    };
                    unifier.unify(Type::Var(variable), parameter.flow_type.ty.clone());
                    bind_signature_declaration_reads(
                        target,
                        variable,
                        parameter.flow_type.mode,
                        signature_lexical_plan,
                        signature_read_expressions,
                        unifier,
                        expressions,
                        modes,
                    );
                }
                for planned in &signature_call.contexts {
                    let Some(variable) = signature_declaration_variables
                        .get(&planned.target)
                        .copied()
                    else {
                        continue;
                    };
                    let Some(context) = contexts.iter().find(|context| {
                        context.ordinal == planned.context_ordinal
                            && context.name == planned.name
                            && context.provider_parameter_ordinal
                                == planned.provider_parameter_ordinal
                    }) else {
                        continue;
                    };
                    unifier.unify(Type::Var(variable), context.flow_type.ty.clone());
                    bind_signature_declaration_reads(
                        &planned.target,
                        variable,
                        context.flow_type.mode,
                        signature_lexical_plan,
                        signature_read_expressions,
                        unifier,
                        expressions,
                        modes,
                    );
                }
            }
            let dynamic_inputs = if valid {
                signature_call
                    .into_iter()
                    .flat_map(|call| &call.matched_inputs)
                    .filter(|planned| {
                        signature_dynamic_expressions
                            .get(planned.expression as usize)
                            .copied()
                            .unwrap_or(false)
                    })
                    .filter_map(|planned| {
                        parameters
                            .binary_search_by_key(&planned.formal_ordinal, |parameter| {
                                parameter.ordinal
                            })
                            .ok()
                            .and_then(|index| parameters.get(index))
                            .map(|expected| (planned.expression, expected.flow_type.ty.clone()))
                    })
                    .collect::<Vec<_>>()
                    .into_boxed_slice()
            } else {
                Box::new([])
            };
            let dynamic_pass = if valid {
                signature_call
                    .and_then(|call| call.explicit_pass)
                    .filter(|pass| {
                        signature_dynamic_expressions
                            .get(pass.expression as usize)
                            .copied()
                            .unwrap_or(false)
                    })
                    .zip(context.clone())
                    .map(|(pass, context)| (pass.expression, context))
            } else {
                None
            };
            let result_mode = if valid {
                let input_mode = |name: &str| {
                    let formal = parameters.iter().find(|parameter| parameter.name == name)?;
                    let expression = signature_call?
                        .matched_inputs
                        .iter()
                        .find(|input| input.formal_ordinal == formal.ordinal)?
                        .expression;
                    let index = expression as usize;
                    if index < expressions.len() {
                        modes.get(index).copied().flatten()
                    } else {
                        seed.external_expressions
                            .get(index.checked_sub(expressions.len())?)
                            .and_then(|external| interfaces.get(&external.owner))
                            .map(|interface| interface.result.mode)
                    }
                };
                if call.function == "List/map" {
                    input_mode("new").unwrap_or(result.mode)
                } else if call.function == "List/latest" {
                    input_mode("list").unwrap_or(result.mode)
                } else if matches!(&target, InferredOwnerCallableTarget::Authoritative)
                    && abi
                        .callable(&call.function)
                        .is_some_and(|contract| contract.kind == CheckedCallableKind::External)
                {
                    signature_call
                        .into_iter()
                        .flat_map(|call| &call.matched_inputs)
                        .filter_map(|input| modes.get(input.expression as usize).copied().flatten())
                        .fold(result.mode, crate::merge_flow_modes)
                } else {
                    result.mode
                }
            } else {
                result.mode
            };
            modes[call.expression] = flow_mode_join(modes[call.expression], Some(result_mode));
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
                matched_inputs: signature_call
                    .map(|call| call.matched_inputs.clone())
                    .unwrap_or_default(),
                explicit_pass: signature_call.and_then(|call| call.explicit_pass),
                dynamic_inputs,
                dynamic_pass,
                plan: call,
                target,
                effect: if valid {
                    effect
                } else {
                    CheckedEffectSummary::default()
                },
                actual_inputs: BTreeMap::new(),
                first_transfer: None,
                resolved_result: None,
                type_substitutions: Vec::new(),
                contextual_type_variables: Vec::new(),
                syntax_discriminated_result: false,
                valid,
            }
        })
        .collect()
}

fn expression_depends_on_pending_call(
    seed: &OwnerConstraintSeed,
    expression: u32,
    pending_calls: &BTreeSet<usize>,
    active: &mut BTreeSet<usize>,
) -> bool {
    let expression = expression as usize;
    if pending_calls.contains(&expression) {
        return true;
    }
    let Some(node) = seed.expressions.get(expression) else {
        return false;
    };
    if !active.insert(expression) {
        return false;
    }
    let depends = node.inputs.iter().any(|input| {
        expression_depends_on_pending_call(seed, input.expression, pending_calls, active)
    });
    active.remove(&expression);
    depends
}

fn bind_ready_dynamic_call_input_layer(
    drafts: &mut [InferredCallDraft],
    seed: &OwnerConstraintSeed,
    unifier: &mut TypeUnifier,
    expressions: &[TypeVar],
    external_expressions: &[TypeVar],
) -> Result<bool, OwnerBodyInferenceError> {
    let pending_calls = drafts
        .iter()
        .filter(|draft| {
            draft.valid && (!draft.dynamic_inputs.is_empty() || draft.dynamic_pass.is_some())
        })
        .map(|draft| draft.plan.expression)
        .collect::<BTreeSet<_>>();
    if pending_calls.is_empty() {
        return Ok(false);
    }
    let ready = drafts
        .iter()
        .enumerate()
        .filter(|(_, draft)| {
            draft.valid
                && (!draft.dynamic_inputs.is_empty() || draft.dynamic_pass.is_some())
                && draft
                    .dynamic_inputs
                    .iter()
                    .map(|(expression, _)| *expression)
                    .chain(draft.dynamic_pass.iter().map(|(expression, _)| *expression))
                    .all(|expression| {
                        !expression_depends_on_pending_call(
                            seed,
                            expression,
                            &pending_calls,
                            &mut BTreeSet::new(),
                        )
                    })
        })
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    if ready.is_empty() {
        return Err(OwnerBodyInferenceError::new(
            "dynamic call-input dependency graph contains no ready producer layer",
        ));
    }

    // Freeze every sibling consumer in this ready layer before mutating any
    // shared producer. This preserves one exact provider epoch for repeated
    // uses while still letting child layers close results for their parents.
    for index in &ready {
        let draft = &mut drafts[*index];
        for (expression, _) in &draft.dynamic_inputs {
            let Some(input) = expression_variable(expressions, external_expressions, *expression)
            else {
                continue;
            };
            draft
                .actual_inputs
                .insert(*expression, unifier.resolve(&Type::Var(input)));
        }
        if let Some((expression, _)) = &draft.dynamic_pass
            && let Some(input) = expression_variable(expressions, external_expressions, *expression)
        {
            draft
                .actual_inputs
                .insert(*expression, unifier.resolve(&Type::Var(input)));
        }
    }
    for index in ready {
        let draft = &mut drafts[index];
        for (expression, expected) in std::mem::take(&mut draft.dynamic_inputs).into_vec() {
            let Some(input) = expression_variable(expressions, external_expressions, expression)
            else {
                continue;
            };
            unifier.bind_call_input(input, expected);
        }
        if let Some((expression, expected)) = draft.dynamic_pass.take()
            && let Some(input) = expression_variable(expressions, external_expressions, expression)
        {
            unifier.bind_call_input(input, expected);
        }
    }
    Ok(true)
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
    providers: &BTreeMap<StableCheckOwnerKey, &OwnerInterfaceTransferModule>,
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
    let matched_inputs = drafts[call_index].matched_inputs.clone();
    let explicit_pass = drafts[call_index].explicit_pass;
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
                providers,
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
    if !interfaces.contains_key(target_owner) {
        states[call_index] = 2;
        return;
    }
    let mut arguments = OwnerResidualDraftArguments::default();
    for input in &matched_inputs {
        let reference = input.expression;
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
            arguments.insert(input.formal_ordinal, actual);
        }
    }
    let explicit_context = explicit_pass.and_then(|pass| {
        body_expression_result_value(
            pass.expression,
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
    let invocation = OwnerCallTransferInvocation {
        arguments,
        context: explicit_context.or(inherited_context),
    };
    // Compare the first pass exactly as it was observed. Re-resolving its
    // variables here would let a value that became concrete between passes
    // masquerade as the same transfer invocation even though it can select a
    // different result branch.
    let replayed = drafts[call_index]
        .first_transfer
        .as_ref()
        .filter(|cached| cached.invocation == invocation)
        .map(|cached| reinstantiate_owner_call_transfer(&cached.evaluation, unifier));
    let evaluated = if let Some(replayed) = replayed {
        Some(replayed)
    } else {
        let evaluated = {
            let mut evaluator = CompiledOwnerResidualEvaluator::new(providers, unifier);
            evaluator.evaluate_owner(
                target_owner,
                &invocation.arguments,
                invocation.context.as_ref(),
            )
        };
        if drafts[call_index].first_transfer.is_none()
            && let Some(evaluation) = &evaluated
        {
            drafts[call_index].first_transfer = Some(OwnerCallTransferReplay {
                invocation,
                evaluation: evaluation.clone(),
            });
        }
        evaluated
    };
    if let Some(evaluated) = evaluated {
        let result = expressions[plan.expression];
        unifier.mark_authoritative_provider(result);
        unifier.replace_derived_provider(result, evaluated.value.flow_type.ty.clone());
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
    providers: &BTreeMap<StableCheckOwnerKey, &OwnerInterfaceTransferModule>,
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
            providers,
            unifier,
            expressions,
            external_expressions,
            caller_context,
            modes,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn settle_owner_call_transfers_and_holds(
    drafts: &mut [InferredCallDraft],
    syntax: &OwnerSyntaxInput,
    seed: &OwnerConstraintSeed,
    interfaces: &BTreeMap<StableCheckOwnerKey, &OwnerPublicInterface>,
    providers: &BTreeMap<StableCheckOwnerKey, &OwnerInterfaceTransferModule>,
    unifier: &mut TypeUnifier,
    hold_authorities: &mut BTreeMap<TypeVar, Type>,
    expressions: &[TypeVar],
    external_expressions: &[TypeVar],
    caller_context: Option<TypeVar>,
    modes: &mut [Option<FlowMode>],
    pattern_narrowings: &[OwnerPatternNarrowing],
    inherited_pattern_narrowings: &[OwnerInheritedPatternNarrowing],
    flow_constraints: &[OwnerFlowConstraint],
) -> Result<(), OwnerBodyInferenceError> {
    fn semantic_surface_snapshot(
        unifier: &mut TypeUnifier,
        expressions: &[TypeVar],
        external_expressions: &[TypeVar],
        modes: &[Option<FlowMode>],
    ) -> (Vec<Type>, Vec<Option<FlowMode>>) {
        let mut variables = BTreeMap::new();
        let mut next = 0;
        let types = expressions
            .iter()
            .chain(external_expressions)
            .map(|variable| {
                let resolved = unifier.resolve(&Type::Var(*variable));
                alpha_normalize_type(&resolved, &mut variables, &mut next)
            })
            .collect();
        (types, modes.to_vec())
    }

    let hold_count = seed
        .expressions
        .iter()
        .filter(|expression| {
            matches!(expression.kind, OwnerConstraintNodeKind::Hold { .. })
                && expression
                    .inputs
                    .iter()
                    .any(|input| matches!(input.role, OwnerConstraintEdgeRole::HoldUpdate))
        })
        .count();
    if hold_count == 0 {
        refine_owner_call_transfers(
            drafts,
            syntax,
            seed,
            interfaces,
            providers,
            unifier,
            expressions,
            external_expressions,
            caller_context,
            modes,
        );
        refine_owner_pattern_narrowings(unifier, pattern_narrowings);
        refine_owner_inherited_pattern_narrowings(unifier, inherited_pattern_narrowings);
        if !replay_flow_constraints(unifier, flow_constraints) {
            return Err(OwnerBodyInferenceError::new(format!(
                "owner body {:?} has a non-convergent local value-flow graph",
                seed.owner
            )));
        }
        unifier.refine_contextual_flow_holes();
        return Ok(());
    }
    let maximum_rounds = drafts.len().saturating_add(hold_count).saturating_add(3);
    let mut previous_surface =
        semantic_surface_snapshot(unifier, expressions, external_expressions, modes);
    for _ in 0..maximum_rounds {
        refine_owner_call_transfers(
            drafts,
            syntax,
            seed,
            interfaces,
            providers,
            unifier,
            expressions,
            external_expressions,
            caller_context,
            modes,
        );
        refine_owner_pattern_narrowings(unifier, pattern_narrowings);
        refine_owner_inherited_pattern_narrowings(unifier, inherited_pattern_narrowings);
        replay_owner_hold_constraints(
            unifier,
            hold_authorities,
            seed,
            expressions,
            external_expressions,
        );
        if !replay_flow_constraints(unifier, flow_constraints) {
            return Err(OwnerBodyInferenceError::new(format!(
                "owner body {:?} has a non-convergent local value-flow graph",
                seed.owner
            )));
        }
        unifier.refine_contextual_flow_holes();
        let current_surface =
            semantic_surface_snapshot(unifier, expressions, external_expressions, modes);
        if current_surface == previous_surface {
            return Ok(());
        }
        previous_surface = current_surface;
    }
    Err(OwnerBodyInferenceError::new(format!(
        "owner body {:?} did not settle calls and HOLD updates in {maximum_rounds} rounds",
        seed.owner
    )))
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
    role: Option<OwnerSourceAnchorRole>,
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
    push_owner_call_diagnostic(diagnostics, call, "user_call_argument_type", message, role);
}

fn push_pass_type_diagnostic(
    diagnostics: &mut Vec<OwnerDiagnosticTemplate>,
    call: &BodyCallPlan,
    actual: &Type,
    expected: &Type,
    role: Option<OwnerSourceAnchorRole>,
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
    role: Option<OwnerSourceAnchorRole>,
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
        role,
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
    for draft in drafts.iter_mut().filter(|draft| draft.valid) {
        let call = &draft.plan;
        let matched_inputs = draft
            .matched_inputs
            .iter()
            .map(|input| {
                (
                    input.formal_ordinal,
                    (input.expression, signature_input_anchor_role(input.source)),
                )
            })
            .collect::<BTreeMap<_, _>>();
        let explicit_context = draft.explicit_pass.map(|pass| pass.expression);
        let explicit_context_role = draft
            .explicit_pass
            .map(|pass| signature_pass_anchor_role(pass.source));
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
                for (formal_ordinal, (reference, _)) in &matched_inputs {
                    let Some(actual) = body_expression_type(
                        *reference,
                        unifier,
                        expressions,
                        external_expressions,
                    ) else {
                        continue;
                    };
                    actuals.insert(*formal_ordinal, actual);
                    if let Some(exact) = draft.actual_inputs.get(reference).cloned() {
                        exact_actuals.insert(*formal_ordinal, unifier.resolve(&exact));
                    }
                }
                let exact_context_actual = explicit_context
                    .and_then(|reference| draft.actual_inputs.get(&reference).cloned())
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
                        matched_inputs
                            .get(&parameter.ordinal)
                            .and_then(|(_, role)| *role),
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
                            matched_inputs
                                .get(&parameter.ordinal)
                                .and_then(|(_, role)| *role),
                        );
                    }
                }
                if explicit_context.is_some()
                    && let (Some(context), Some(actual)) =
                        (&interface.context, exact_context_actual)
                {
                    let expected =
                        crate::substitute_checked_type(&context.flow_type.ty, &substitutions);
                    push_pass_type_diagnostic(
                        diagnostics,
                        call,
                        &actual,
                        &expected,
                        explicit_context_role,
                    );
                }
            }
            InferredOwnerCallableTarget::Authoritative => {
                let Some(contract) = abi.callable(&call.function) else {
                    continue;
                };
                let mut substitutions = BTreeMap::new();
                let mut actuals = BTreeMap::new();
                for (formal_ordinal, (reference, _)) in &matched_inputs {
                    let Some(parameter) = contract
                        .parameters
                        .binary_search_by_key(formal_ordinal, |parameter| parameter.ordinal)
                        .ok()
                        .and_then(|index| contract.parameters.get(index))
                    else {
                        continue;
                    };
                    let Some(actual) = body_expression_type(
                        *reference,
                        unifier,
                        expressions,
                        external_expressions,
                    ) else {
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
                        matched_inputs
                            .get(&parameter.ordinal)
                            .and_then(|(_, role)| *role),
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
    lexical_plan: &OwnerLexicalPlan,
    seed: &OwnerConstraintSeed,
    summary: &OwnerConstraintSummary,
    own_scc: &OwnerInterfaceSccResult,
) -> Result<(), OwnerBodyInferenceError> {
    if !lexical_plan.matches_input(syntax)
        || syntax.owner != seed.owner
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
    if seed.lexical_reads_fingerprint_v1() != lexical_plan.reads_fingerprint_v1() {
        return Err(OwnerBodyInferenceError::new(
            "owner body inference has mismatched seed and lexical plan",
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

fn owner_body_alpha_namespace(
    unifier: &mut TypeUnifier,
    own_variables: &BTreeMap<TypeVar, TypeVar>,
    own_scc_type_variable_count: u32,
) -> (BTreeMap<TypeVar, TypeVar>, u32) {
    let mut alpha_variables = BTreeMap::new();
    for (stable, instantiated) in own_variables {
        debug_assert!(stable.0 < own_scc_type_variable_count);
        if let Type::Var(root) = unifier.resolve(&Type::Var(*instantiated)) {
            alpha_variables.entry(root).or_insert(*stable);
        }
    }
    (alpha_variables, own_scc_type_variable_count)
}

/// Infer one immutable owner body against exact frozen public interfaces.
///
/// This function never descends another owner body and never allocates a
/// project-global checked identity. The caller must provide exactly the own,
/// child-value, value-read, and callable interfaces named by the resolved
/// owner inputs. Shared transfer modules retain the exact transitive result
/// dependencies without flattening them into every body evaluation.
pub fn evaluate_owner_body(
    syntax: &OwnerSyntaxInput,
    lexical_plan: &OwnerLexicalPlan,
    seed: &OwnerConstraintSeed,
    summary: &OwnerConstraintSummary,
    abi: &OwnerInferenceAbiEnvironment,
    interface_plan: &OwnerBodyInterfacePlan,
) -> Result<OwnerBodyInferenceEvaluation, OwnerBodyInferenceError> {
    evaluate_owner_body_impl(
        syntax,
        lexical_plan,
        seed,
        summary,
        abi,
        interface_plan,
        None,
    )
}

pub fn evaluate_owner_body_with_signature_plan(
    syntax: &OwnerSyntaxInput,
    lexical_plan: &OwnerLexicalPlan,
    seed: &OwnerConstraintSeed,
    summary: &OwnerConstraintSummary,
    abi: &OwnerInferenceAbiEnvironment,
    interface_plan: &OwnerBodyInterfacePlan,
    signature_lexical_plan: &OwnerSignatureLexicalPlan,
) -> Result<OwnerBodyInferenceEvaluation, OwnerBodyInferenceError> {
    evaluate_owner_body_impl(
        syntax,
        lexical_plan,
        seed,
        summary,
        abi,
        interface_plan,
        Some(signature_lexical_plan),
    )
}

fn evaluate_owner_body_impl(
    syntax: &OwnerSyntaxInput,
    lexical_plan: &OwnerLexicalPlan,
    seed: &OwnerConstraintSeed,
    summary: &OwnerConstraintSummary,
    abi: &OwnerInferenceAbiEnvironment,
    interface_plan: &OwnerBodyInterfacePlan,
    supplied_signature_lexical_plan: Option<&OwnerSignatureLexicalPlan>,
) -> Result<OwnerBodyInferenceEvaluation, OwnerBodyInferenceError> {
    let own_scc = interface_plan.own_scc().module.result();
    validate_inputs(syntax, lexical_plan, seed, summary, own_scc)?;
    if interface_plan.owner() != &seed.owner {
        return Err(OwnerBodyInferenceError::new(
            "owner body inference received an interface plan for another owner",
        ));
    }
    if interface_plan.own_scc().key != own_scc.key {
        return Err(OwnerBodyInferenceError::new(format!(
            "owner body inference {:?} received the wrong own interface SCC",
            seed.owner
        )));
    }
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
    let mut interfaces = BTreeMap::new();
    let mut providers = BTreeMap::new();
    let mut frozen_results = Vec::new();
    for planned_scc in interface_plan.sccs() {
        let module = planned_scc.module.as_ref();
        let result = module.result();
        let frozen = frozen_scc_ref(planned_scc)?;
        for member in &frozen.referenced_members {
            let owner = frozen.key.members.get(*member as usize).ok_or_else(|| {
                OwnerBodyInferenceError::new(format!(
                    "interface SCC {:?} has no planned member {member}",
                    result.key
                ))
            })?;
            let interface = result.owner(owner).ok_or_else(|| {
                OwnerBodyInferenceError::new(format!(
                    "interface SCC {:?} does not publish its member {owner:?}",
                    result.key
                ))
            })?;
            insert_interface(&mut interfaces, interface)?;
            if providers.insert(owner.clone(), module).is_some() {
                return Err(OwnerBodyInferenceError::new(format!(
                    "owner body inference received multiple provider SCCs for {owner:?}"
                )));
            }
        }
        frozen_results.push(frozen);
    }
    if interfaces.len() != interface_plan.required_owner_count() {
        return Err(OwnerBodyInferenceError::new(format!(
            "owner body inference {:?} did not receive its exact interface import set",
            seed.owner
        )));
    }
    let own_interface = interfaces.get(&seed.owner).copied().ok_or_else(|| {
        OwnerBodyInferenceError::new(format!(
            "owner body inference {:?} has no planned own interface",
            seed.owner
        ))
    })?;
    let own_scc_index = frozen_results
        .iter()
        .position(|frozen| frozen.key == own_scc.key)
        .expect("validated own SCC is present exactly once");
    let own_scc_ref = frozen_results.remove(own_scc_index);
    frozen_results.sort_by(|left, right| left.key.cmp(&right.key));
    let signature_lexical_plan = if let Some(plan) = supplied_signature_lexical_plan {
        let signature_inputs_match = plan
            .matches_signature_inputs(seed, summary, abi, |owner| {
                interfaces
                    .get(owner)
                    .map(|interface| OwnerCallableLexicalSignature::from_interface(interface))
            })
            .map_err(|error| {
                OwnerBodyInferenceError::new(format!(
                    "cannot validate owner body signature lexical inputs: {error}"
                ))
            })?;
        if !plan.matches_base(lexical_plan) || !plan.matches_seed(seed) || !signature_inputs_match {
            return Err(OwnerBodyInferenceError::new(format!(
                "owner body inference {:?} received a stale signature lexical plan",
                seed.owner
            )));
        }
        plan.clone()
    } else {
        project_owner_signature_lexical_plan(
            seed,
            lexical_plan,
            summary,
            abi,
            interfaces.values().copied(),
        )
        .map_err(|error| {
            OwnerBodyInferenceError::new(format!(
                "cannot project signature lexical plan for {:?}: {error}",
                seed.owner
            ))
        })?
    };
    if supplied_signature_lexical_plan.is_some()
        && (!summary.matches_signature_plan(&signature_lexical_plan)
            || !summary.matches_effective_references(signature_lexical_plan.external_candidates()))
    {
        return Err(OwnerBodyInferenceError::new(format!(
            "owner body inference {:?} received a summary from another effective lexical plan",
            seed.owner
        )));
    }
    let inference_abi_fingerprint_v1 = abi.fingerprint_v1();
    let mut interface_imports = interfaces
        .values()
        .map(|interface| {
            let provider = providers[&interface.owner];
            let provider_scc = if provider.key == own_scc_ref.key {
                0
            } else {
                let index = frozen_results
                    .binary_search_by(|frozen| frozen.key.cmp(&provider.key))
                    .map_err(|_| {
                        OwnerBodyInferenceError::new(format!(
                            "owner body inference {:?} lost provider SCC {:?}",
                            seed.owner, provider.key
                        ))
                    })?;
                u32::try_from(index + 1).map_err(|_| {
                    OwnerBodyInferenceError::new(
                        "owner body interface provider index exceeds the u32 bound",
                    )
                })?
            };
            Ok(OwnerBodyInterfaceImport {
                owner: interface.owner.clone(),
                interface_fingerprint_v1: owner_body_interface_fingerprint_v1(interface)?,
                provider_scc,
            })
        })
        .collect::<Result<Vec<_>, OwnerBodyInferenceError>>()?;
    interface_imports.sort_by(|left, right| left.owner.cmp(&right.owner));
    let basis = OwnerBodyInferenceBasis {
        owner: seed.owner.clone(),
        syntax_fingerprint_v1: syntax.fingerprint_v1(),
        lexical_plan_fingerprint_v1: lexical_plan.fingerprint_v1(),
        signature_lexical_plan_fingerprint_v1: signature_lexical_plan.fingerprint_v1(),
        seed_fingerprint_v1: seed.fingerprint_v1(),
        summary_fingerprint_v1: summary.fingerprint_v1(),
        own_scc: own_scc_ref,
        imports: frozen_results.into_boxed_slice(),
        interface_plan_fingerprint_v1: interface_plan.fingerprint_v1(),
        inference_abi_fingerprint_v1,
    };
    let mut work = OwnerBodyInferenceWork {
        statements: syntax.statements.len() as u64,
        expressions: syntax.expressions.len() as u64,
        interface_plan_direct_owners: interface_plan.work.direct_owners,
        interface_plan_required_owners: interface_plan.work.required_owners,
        interface_plan_provider_sccs: interface_plan.work.provider_sccs,
        interface_plan_result_transfers: interface_plan.work.result_transfers,
        interface_plan_transfer_nodes: interface_plan.work.result_transfer_nodes,
        interface_plan_transfer_edges: interface_plan.work.result_transfer_edges,
        ..OwnerBodyInferenceWork::default()
    };
    let mut unifier = TypeUnifier::default();
    let mut hold_authorities = BTreeMap::new();
    let expressions = (0..seed.expressions.len())
        .map(|_| unifier.fresh())
        .collect::<Vec<_>>();
    mark_owner_derived_providers(&mut unifier, seed, &expressions);
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
    let mut own_variables = BTreeMap::new();

    for ((external, variable), flush_variable) in seed
        .external_expressions
        .iter()
        .zip(&external_expressions)
        .zip(&external_expression_flushes)
    {
        let (ty, flush_type) = if external.is_exact_enclosing_capture_for(&seed.owner) {
            let capture = own_interface
                .captures
                .iter()
                .find(|capture| {
                    capture.owner == external.owner && capture.expression == external.expression
                })
                .ok_or_else(|| {
                    OwnerBodyInferenceError::new(format!(
                        "owner body {:?} is missing enclosing capture {:?}",
                        seed.owner, external.expression
                    ))
                })?;
            (
                instantiate_type(&capture.flow_type.ty, &mut unifier, &mut own_variables),
                capture.flush_type.as_ref().map_or(Type::Absent, |ty| {
                    instantiate_type(ty, &mut unifier, &mut own_variables)
                }),
            )
        } else {
            let interface = interfaces[&external.owner];
            let mut variables = BTreeMap::new();
            (
                instantiate_type(&interface.result.ty, &mut unifier, &mut variables),
                interface
                    .result_flush_type
                    .as_ref()
                    .map_or(Type::Absent, |ty| {
                        instantiate_type(ty, &mut unifier, &mut variables)
                    }),
            )
        };
        unifier.bind_var(*variable, ty);
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
    let mut own_parameter_variables = Vec::with_capacity(own_interface.parameters.len());
    let mut own_parameter_variables_by_ordinal = BTreeMap::new();
    for parameter in &own_interface.parameters {
        let variable = unifier.fresh();
        let ty = instantiate_type(&parameter.flow_type.ty, &mut unifier, &mut own_variables);
        unifier.bind_var(variable, ty);
        if own_parameter_variables_by_ordinal
            .insert(parameter.ordinal, variable)
            .is_some()
        {
            return Err(OwnerBodyInferenceError::new(format!(
                "owner body interface repeats parameter ordinal {}",
                parameter.ordinal
            )));
        }
        own_parameter_variables.push(variable);
    }
    let context = own_interface.context.as_ref().map(|context| {
        let variable = unifier.fresh();
        let ty = instantiate_type(&context.flow_type.ty, &mut unifier, &mut own_variables);
        unifier.bind_var(variable, ty);
        variable
    });
    let expected_lexical_captures = signature_lexical_plan
        .imported_capture_sites()
        .iter()
        .map(|capture| (capture.target.clone(), capture.demand_paths.clone()))
        .collect::<Vec<_>>();
    let actual_lexical_captures = own_interface
        .lexical_captures
        .iter()
        .map(|capture| (capture.target.clone(), capture.demand_paths.clone()))
        .collect::<Vec<_>>();
    if actual_lexical_captures != expected_lexical_captures {
        return Err(OwnerBodyInferenceError::new(format!(
            "owner body inference {:?} received stale or duplicate lexical captures",
            seed.owner
        )));
    }
    let mut lexical_capture_variables = BTreeMap::new();
    for capture in &own_interface.lexical_captures {
        let variable = unifier.fresh();
        let ty = instantiate_type(&capture.flow_type.ty, &mut unifier, &mut own_variables);
        unifier.bind_var(variable, ty);
        lexical_capture_variables
            .insert(capture.target.clone(), (variable, capture.flow_type.mode));
    }
    let imported_context = lexical_capture_variables
        .iter()
        .filter_map(|(target, (variable, _))| {
            matches!(target, OwnerLexicalTargetRef::ContextFormal { .. }).then_some(*variable)
        })
        .collect::<Vec<_>>();
    if imported_context.len() > 1 || (context.is_some() && !imported_context.is_empty()) {
        return Err(OwnerBodyInferenceError::new(
            "owner body has conflicting local and imported PASSED context formals",
        ));
    }
    let effective_context = context.or(imported_context.first().copied());
    let own_result = instantiate_type(&own_interface.result.ty, &mut unifier, &mut own_variables);
    let mut signature_declaration_variables = BTreeMap::new();
    for declaration in signature_lexical_plan.declarations() {
        if signature_declaration_variables
            .insert(declaration.target.clone(), unifier.fresh())
            .is_some()
        {
            return Err(OwnerBodyInferenceError::new(
                "owner signature lexical plan repeats a dynamic declaration",
            ));
        }
    }

    let planned_lexical_reads = planned_lexical_read_variables(
        syntax,
        lexical_plan,
        &signature_lexical_plan,
        &expressions,
        &external_expressions,
        &expression_flushes,
        &external_expression_flushes,
        &own_parameter_variables_by_ordinal,
        &signature_declaration_variables,
        &lexical_capture_variables,
        effective_context,
        &mut unifier,
    )?;

    let mut modes = vec![None; expressions.len()];
    let mut direct_effects = vec![CheckedEffectSummary::default(); expressions.len()];
    let mut calls = Vec::new();
    let mut pattern_narrowings = Vec::new();
    let mut flow_constraints = Vec::new();
    let inherited_pattern_plans = inherited_pattern_read_plans(&signature_lexical_plan);
    let mut pattern_local_expressions =
        exact_pattern_local_expressions(seed, &signature_lexical_plan);
    pattern_local_expressions.extend(
        inherited_pattern_plans
            .iter()
            .flat_map(|plan| plan.reads.iter().map(|read| read.expression)),
    );
    let inherited_pattern_narrowings = instantiate_owner_inherited_pattern_narrowings(
        &inherited_pattern_plans,
        |target| {
            lexical_capture_variables
                .get(target)
                .map(|(variable, _)| *variable)
        },
        &expressions,
        &mut unifier,
    )
    .map_err(OwnerBodyInferenceError::new)?;
    bind_local_constraints(
        seed,
        summary,
        &signature_lexical_plan,
        abi,
        &mut unifier,
        &expressions,
        &external_expressions,
        &planned_lexical_reads,
        &pattern_local_expressions,
        &mut modes,
        &mut direct_effects,
        &mut calls,
        &mut pattern_narrowings,
        &mut flow_constraints,
        &mut work,
    );
    for plan in &inherited_pattern_plans {
        let mode = lexical_capture_variables
            .get(&plan.target)
            .map(|(_, mode)| *mode)
            .ok_or_else(|| {
                OwnerBodyInferenceError::new(format!(
                    "inherited pattern target {:?} has no capture mode",
                    plan.target
                ))
            })?;
        for read in &plan.reads {
            modes[read.expression as usize] = Some(mode);
        }
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
        if signature_lexical_plan.reads()[index].is_some() {
            // The exhaustive signature plan is authoritative over an earlier
            // base external candidate. Dynamic OUT/context reads must not also
            // unify with a same-named project value.
            continue;
        }
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
    // Selectors that are already concrete from local/project authority must
    // narrow projected bindings before call inputs are frozen. The later pass
    // remains necessary for selectors whose concrete result is produced by a
    // user call during transfer evaluation.
    refine_owner_pattern_narrowings(&mut unifier, &pattern_narrowings);
    refine_owner_inherited_pattern_narrowings(&mut unifier, &inherited_pattern_narrowings);
    if !replay_flow_constraints(&mut unifier, &flow_constraints) {
        return Err(OwnerBodyInferenceError::new(format!(
            "owner body {:?} has a non-convergent local value-flow graph",
            seed.owner
        )));
    }
    initialize_owner_hold_constraints(
        &mut unifier,
        &mut hold_authorities,
        seed,
        &expressions,
        &external_expressions,
    );
    unifier.refine_contextual_flow_holes();
    let mut pre_call_actual_types = expressions
        .iter()
        .chain(&external_expressions)
        .map(|variable| unifier.resolve(&Type::Var(*variable)))
        .collect::<Vec<_>>();

    let mut diagnostics = Vec::new();
    push_invalid_syntax_diagnostics(seed, &mut diagnostics);
    push_lexical_read_diagnostics(syntax, seed, &signature_lexical_plan, &mut diagnostics);
    push_external_value_diagnostics(summary, &signature_lexical_plan, abi, &mut diagnostics);
    let mut signature_read_expressions =
        BTreeMap::<OwnerSignatureDeclarationTarget, Vec<usize>>::new();
    for (index, read) in signature_lexical_plan.reads().iter().enumerate() {
        let target = match read.as_ref().map(|read| &read.target) {
            Some(OwnerEffectiveLexicalTarget::FreshOut {
                call,
                formal_ordinal,
            }) => Some(OwnerSignatureDeclarationTarget::FreshOut {
                call: call.clone(),
                formal_ordinal: *formal_ordinal,
            }),
            Some(OwnerEffectiveLexicalTarget::CallContext {
                call,
                context_ordinal,
            }) => Some(OwnerSignatureDeclarationTarget::CallContext {
                call: call.clone(),
                context_ordinal: *context_ordinal,
            }),
            _ => None,
        };
        if let Some(target) = target
            && !pattern_local_expressions.contains(&(index as u32))
        {
            signature_read_expressions
                .entry(target)
                .or_default()
                .push(index);
        }
    }
    let signature_dynamic_expressions =
        signature_dynamic_expression_index(seed, &signature_lexical_plan);
    let caller_is_callable =
        own_interface.declaration_kind == Some(crate::OwnerDeclarationKind::Function);
    let mut call_drafts = bind_calls(
        calls,
        seed,
        &signature_lexical_plan,
        &signature_dynamic_expressions,
        &signature_declaration_variables,
        &signature_read_expressions,
        &interfaces,
        &mut unifier,
        &expressions,
        &external_expressions,
        &call_flushes,
        &mut modes,
        &mut direct_effects,
        abi,
        effective_context.is_some(),
        caller_is_callable,
        &mut diagnostics,
        &mut work,
    );
    // Calls are initialized outer-first so their consumer requirements cannot
    // pre-shape a nested contextual provider. A later call in that same pass
    // can therefore close a FreshOut/context root after its projected reads
    // were first linked. Replay the now-current provider epoch before any
    // actual-input snapshot or occurrence transfer consumes those reads.
    replay_signature_declaration_read_types(
        &signature_declaration_variables,
        &signature_lexical_plan,
        &signature_read_expressions,
        &mut unifier,
        &expressions,
    );
    refine_owner_pattern_narrowings(&mut unifier, &pattern_narrowings);
    refine_owner_inherited_pattern_narrowings(&mut unifier, &inherited_pattern_narrowings);
    if !replay_flow_constraints(&mut unifier, &flow_constraints) {
        return Err(OwnerBodyInferenceError::new(format!(
            "owner body {:?} has a non-convergent local value-flow graph",
            seed.owner
        )));
    }
    // The call pass publishes principal results for update expressions. Replay
    // HOLD before freezing the all-provider actual-input epoch.
    replay_owner_hold_constraints(
        &mut unifier,
        &mut hold_authorities,
        seed,
        &expressions,
        &external_expressions,
    );
    if !replay_flow_constraints(&mut unifier, &flow_constraints) {
        return Err(OwnerBodyInferenceError::new(format!(
            "owner body {:?} has a non-convergent local value-flow graph",
            seed.owner
        )));
    }
    // Freeze one all-providers-first epoch before result-transfer consumers
    // run. `resolve` retains still-live TypeVar roots, so a child user-call
    // result can close later without replacing this snapshot. Re-snapshotting
    // after the first transfer pass would instead record requirements imposed
    // by sibling/outer consumers (for example Number in place of a FreshOut
    // Text item) and suppress the diagnostic that this exact snapshot owns.
    for (slot, variable) in pre_call_actual_types
        .iter_mut()
        .zip(expressions.iter().chain(&external_expressions))
    {
        *slot = unifier.resolve(&Type::Var(*variable));
    }
    for draft in call_drafts.iter_mut().filter(|draft| draft.valid) {
        draft.actual_inputs = draft
            .plan
            .inputs
            .iter()
            .filter_map(|(_, reference)| {
                pre_call_actual_types
                    .get(*reference as usize)
                    .cloned()
                    .map(|actual| (*reference, actual))
            })
            .collect();
    }
    settle_owner_call_transfers_and_holds(
        &mut call_drafts,
        syntax,
        seed,
        &interfaces,
        &providers,
        &mut unifier,
        &mut hold_authorities,
        &expressions,
        &external_expressions,
        effective_context,
        &mut modes,
        &pattern_narrowings,
        &inherited_pattern_narrowings,
        &flow_constraints,
    )?;
    replay_signature_declaration_read_types(
        &signature_declaration_variables,
        &signature_lexical_plan,
        &signature_read_expressions,
        &mut unifier,
        &expressions,
    );
    refine_owner_pattern_narrowings(&mut unifier, &pattern_narrowings);
    refine_owner_inherited_pattern_narrowings(&mut unifier, &inherited_pattern_narrowings);
    if !replay_flow_constraints(&mut unifier, &flow_constraints) {
        return Err(OwnerBodyInferenceError::new(format!(
            "owner body {:?} has a non-convergent local value-flow graph",
            seed.owner
        )));
    }
    while bind_ready_dynamic_call_input_layer(
        &mut call_drafts,
        seed,
        &mut unifier,
        &expressions,
        &external_expressions,
    )? {
        // Binding one ready dynamic layer can close a contextual declaration
        // root. Refresh its projected occurrences before transfer evaluation
        // observes this provider epoch.
        replay_signature_declaration_read_types(
            &signature_declaration_variables,
            &signature_lexical_plan,
            &signature_read_expressions,
            &mut unifier,
            &expressions,
        );
        settle_owner_call_transfers_and_holds(
            &mut call_drafts,
            syntax,
            seed,
            &interfaces,
            &providers,
            &mut unifier,
            &mut hold_authorities,
            &expressions,
            &external_expressions,
            effective_context,
            &mut modes,
            &pattern_narrowings,
            &inherited_pattern_narrowings,
            &flow_constraints,
        )?;
        replay_signature_declaration_read_types(
            &signature_declaration_variables,
            &signature_lexical_plan,
            &signature_read_expressions,
            &mut unifier,
            &expressions,
        );
        refine_owner_pattern_narrowings(&mut unifier, &pattern_narrowings);
        refine_owner_inherited_pattern_narrowings(&mut unifier, &inherited_pattern_narrowings);
        if !replay_flow_constraints(&mut unifier, &flow_constraints) {
            return Err(OwnerBodyInferenceError::new(format!(
                "owner body {:?} has a non-convergent local value-flow graph",
                seed.owner
            )));
        }
    }
    validate_owner_call_types(
        &mut call_drafts,
        &interfaces,
        abi,
        &mut unifier,
        &expressions,
        &external_expressions,
        effective_context,
        &mut diagnostics,
    );
    // Own-interface alphas already live in the SCC's stable namespace. Keep
    // that identity when projecting the body instead of renumbering the same
    // live roots from zero. Checked compatibility assembly combines the
    // interface scheme with these occurrence types, so independently
    // renumbering an interface-backed root would sever correlations such as a
    // callable parameter alpha reused by a nested HOLD/read expression.
    //
    // Body-only roots start after the complete SCC namespace. They remain
    // private implementation variables and therefore cannot collide with a
    // stable interface alpha from this SCC.
    let (mut alpha_variables, mut next_alpha) =
        owner_body_alpha_namespace(&mut unifier, &own_variables, own_scc.type_variable_count);
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
    if let Some(context) = effective_context {
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
            let actual_inputs = &draft.actual_inputs;
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
                            actual_type: alpha_normalize_type(
                                &unifier.resolve(
                                    actual_inputs.get(&expression).unwrap_or(&Type::Unknown),
                                ),
                                &mut alpha_variables,
                                &mut next_alpha,
                            ),
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
    let relocations = collect_relocations(seed, summary, &signature_lexical_plan);

    let diagnostic_facts =
        crate::owner_diagnostics::project_owner_diagnostic_contribution_from_rows(
            syntax,
            lexical_plan,
            &inferred_statements,
            &inferred_expressions,
            &inferred_calls,
            &signature_lexical_plan,
            abi,
        )
        .map_err(|error| {
            OwnerBodyInferenceError::new(format!(
                "cannot publish owner diagnostic facts for {:?}: {error}",
                seed.owner
            ))
        })?;
    let local_content_digest_v1 = fingerprint(
        OWNER_BODY_INFERENCE_CONTENT_DOMAIN_V8,
        &(
            &inferred_statements,
            &inferred_children,
            &inferred_expressions,
            &inferred_calls,
            &relocations,
            &diagnostics,
            diagnostic_facts.fingerprint_v1(),
            signature_lexical_plan.fingerprint_v1(),
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
        diagnostic_facts_fingerprint_v1: diagnostic_facts.fingerprint_v1(),
        signature_lexical_plan_fingerprint_v1: signature_lexical_plan.fingerprint_v1(),
        local_content_digest_v1,
    };
    // The construction receipt already commits every semantic row, diagnostic,
    // effect, and row count above. Bind the stable owner to that compact seal
    // instead of serializing the same rich body a second time.
    let fingerprint_v1 = fingerprint(OWNER_BODY_INFERENCE_DOMAIN_V10, &(&seed.owner, &receipt))?;
    work.unification_steps = unifier.steps();
    let result = Arc::new(OwnerBodyInferenceShard {
        owner: seed.owner.clone(),
        statements: inferred_statements.into_boxed_slice(),
        children: inferred_children.into_boxed_slice(),
        expressions: inferred_expressions.into_boxed_slice(),
        calls: inferred_calls.into_boxed_slice(),
        relocations,
        diagnostics: diagnostics.into_boxed_slice(),
        diagnostic_facts,
        signature_lexical_plan,
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
    lexical_plan: &OwnerLexicalPlan,
    seed: &OwnerConstraintSeed,
    summary: &OwnerConstraintSummary,
    abi: &OwnerInferenceAbiEnvironment,
    own_module: &'a OwnerInterfaceTransferModule,
    imported_modules: impl IntoIterator<Item = &'a OwnerInterfaceTransferModule>,
) -> Result<OwnerBodyInferenceShard, OwnerBodyInferenceError> {
    let imported_modules = imported_modules.into_iter().collect::<Vec<_>>();
    let interface_plan = plan_owner_body_interfaces(
        seed,
        summary,
        std::iter::once(own_module).chain(imported_modules.iter().copied()),
    )?;
    evaluate_owner_body(syntax, lexical_plan, seed, summary, abi, &interface_plan)
        .map(|evaluation| Arc::unwrap_or_clone(evaluation.result))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ResolvedOwnerSymbolReference, build_owner_interface_topology,
        evaluate_owner_interface_scc_component_for_tests, project_owner_constraint_seed,
        project_owner_lexical_plan, project_owner_source_map, project_owner_syntax_input,
        resolve_owner_constraint_seed,
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
    ) -> Vec<OwnerInterfaceTransferModule> {
        let abi_provider = test_abi();
        solve_with_abi(seeds, summaries, &abi_provider)
    }

    fn solve_with_abi(
        seeds: &[OwnerConstraintSeed],
        summaries: &[OwnerConstraintSummary],
        abi_provider: &crate::OwnerAbiEnvironment,
    ) -> Vec<OwnerInterfaceTransferModule> {
        let topology = build_owner_interface_topology(summaries.iter()).unwrap();
        let seeds = seeds
            .iter()
            .map(|seed| (seed.owner.clone(), seed))
            .collect::<BTreeMap<_, _>>();
        let summaries = summaries
            .iter()
            .map(|summary| (summary.owner.clone(), summary))
            .collect::<BTreeMap<_, _>>();
        let mut modules =
            BTreeMap::<OwnerInterfaceSccKey, Arc<OwnerInterfaceTransferModule>>::new();
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
                .map(|dependency| modules.get(dependency).unwrap())
                .collect::<Vec<_>>();
            let component = evaluate_owner_interface_scc_component_for_tests(
                scc,
                &abi,
                scc.key.members.iter().map(|owner| seeds[owner]),
                scc.key.members.iter().map(|owner| summaries[owner]),
                dependencies.iter().map(|module| module.result()),
                dependencies.iter().map(|module| Arc::clone(module)),
            )
            .unwrap();
            modules.insert(scc.key.clone(), component.module);
        }
        topology
            .sccs
            .iter()
            .map(|scc| Arc::unwrap_or_clone(modules.remove(&scc.key).unwrap()))
            .collect()
    }

    fn infer(
        syntax: &OwnerSyntaxInput,
        seed: &OwnerConstraintSeed,
        summary: &OwnerConstraintSummary,
        results: &[OwnerInterfaceTransferModule],
    ) -> OwnerBodyInferenceShard {
        let abi_provider = test_abi();
        infer_with_abi(syntax, seed, summary, results, &abi_provider)
    }

    fn infer_with_abi(
        syntax: &OwnerSyntaxInput,
        seed: &OwnerConstraintSeed,
        summary: &OwnerConstraintSummary,
        results: &[OwnerInterfaceTransferModule],
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
        let lexical_plan = project_owner_lexical_plan(syntax).unwrap();
        infer_owner_body(
            syntax,
            &lexical_plan,
            seed,
            summary,
            &abi,
            own_scc,
            results.iter().filter(|result| result.key != own_scc.key),
        )
        .unwrap()
    }

    #[test]
    fn body_normalization_reuses_the_own_interface_alpha_namespace() {
        let mut unifier = TypeUnifier::default();
        let earlier = unifier.fresh();
        let selected_once = unifier.fresh();
        let body_only = unifier.fresh();
        let own_variables = BTreeMap::from([(TypeVar(36), earlier), (TypeVar(46), selected_once)]);
        let (mut variables, mut next) =
            owner_body_alpha_namespace(&mut unifier, &own_variables, 47);

        assert_eq!(
            alpha_normalize_type(
                &Type::Union(vec![
                    Type::Var(selected_once),
                    Type::VariantSet(vec![Variant::Tag("False".to_owned())].into()),
                ]),
                &mut variables,
                &mut next,
            ),
            Type::Union(vec![
                Type::Var(TypeVar(46)),
                Type::VariantSet(vec![Variant::Tag("False".to_owned())].into()),
            ]),
        );
        assert_eq!(
            alpha_normalize_type(&Type::Var(body_only), &mut variables, &mut next),
            Type::Var(TypeVar(47)),
        );
        assert_eq!(next, 48);
    }

    #[test]
    fn call_transfer_replay_freshens_only_invocation_owned_variables() {
        let mut unifier = TypeUnifier::default();
        let caller_variable = unifier.fresh();
        let first_owned = unifier.fresh();
        let second_owned = unifier.fresh();
        let stable_substitution_variable = TypeVar(91);
        let stable_contextual_variable = TypeVar(92);
        let cached = EvaluatedOwnerResult {
            value: EvaluatedResultValue {
                flow_type: FlowType {
                    mode: FlowMode::PresentOrAbsent,
                    ty: Type::object(ObjectShape::from_ordered_fields(
                        [
                            ("caller".to_owned(), Type::Var(caller_variable)),
                            (
                                "owned".to_owned(),
                                Type::List(Type::shared(Type::Var(first_owned))),
                            ),
                            (
                                "nested".to_owned(),
                                Type::Map {
                                    key: Box::new(Type::Var(second_owned)),
                                    value: Box::new(Type::Var(caller_variable)),
                                },
                            ),
                        ],
                        false,
                    )),
                },
                parameter_derived: true,
                syntax_selected: true,
                static_number: Some(ExactNumber::from_i64(7)),
            },
            type_substitutions: vec![(
                stable_substitution_variable,
                Type::object(ObjectShape::from_ordered_fields(
                    [
                        ("owned".to_owned(), Type::Var(second_owned)),
                        ("caller".to_owned(), Type::Var(caller_variable)),
                    ],
                    false,
                )),
            )],
            contextual_type_variables: vec![stable_contextual_variable],
            owned_variables: vec![first_owned, second_owned],
        };

        // Bind the first invocation's variables before replay. A replay must
        // neither inherit these bindings nor replace caller-owned variables.
        unifier.bind_var(first_owned, Type::Number);
        unifier.bind_var(second_owned, Type::Text);
        let replayed = reinstantiate_owner_call_transfer(&cached, &mut unifier);
        let replay_first = replayed.owned_variables[0];
        let replay_second = replayed.owned_variables[1];

        assert_ne!(replay_first, first_owned);
        assert_ne!(replay_second, second_owned);
        assert_eq!(
            unifier.resolve(&Type::Var(replay_first)),
            Type::Var(replay_first)
        );
        assert_eq!(
            unifier.resolve(&Type::Var(replay_second)),
            Type::Var(replay_second)
        );
        assert_eq!(
            replayed.value.flow_type,
            FlowType {
                mode: FlowMode::PresentOrAbsent,
                ty: Type::object(ObjectShape::from_ordered_fields(
                    [
                        ("caller".to_owned(), Type::Var(caller_variable)),
                        (
                            "owned".to_owned(),
                            Type::List(Type::shared(Type::Var(replay_first))),
                        ),
                        (
                            "nested".to_owned(),
                            Type::Map {
                                key: Box::new(Type::Var(replay_second)),
                                value: Box::new(Type::Var(caller_variable)),
                            },
                        ),
                    ],
                    false,
                )),
            }
        );
        assert_eq!(
            replayed.type_substitutions,
            vec![(
                stable_substitution_variable,
                Type::object(ObjectShape::from_ordered_fields(
                    [
                        ("owned".to_owned(), Type::Var(replay_second)),
                        ("caller".to_owned(), Type::Var(caller_variable)),
                    ],
                    false,
                )),
            )]
        );
        assert_eq!(
            replayed.contextual_type_variables,
            vec![stable_contextual_variable]
        );
        assert!(replayed.value.parameter_derived);
        assert!(replayed.value.syntax_selected);
        assert_eq!(replayed.value.static_number, Some(ExactNumber::from_i64(7)));
    }

    #[test]
    fn transfer_modules_precompute_only_occurrence_invariant_results() {
        let unit = link(concat!(
            "FUNCTION label(ignored) {\n",
            "    TEXT { fixed }\n",
            "}\n",
            "FUNCTION identity(input) {\n",
            "    input\n",
            "}\n",
            "value: [label: label(ignored: 1), identity: identity(input: 1)]\n",
        ));
        let label_owner = owner_named(&unit, "label");
        let identity_owner = owner_named(&unit, "identity");
        let value_owner = owner_named(&unit, "value");
        let (_, _, label_seed) = inputs(&unit, &label_owner);
        let (_, _, identity_seed) = inputs(&unit, &identity_owner);
        let (_, _, value_seed) = inputs(&unit, &value_owner);
        let label_summary = resolve_owner_constraint_seed(&label_seed, []).unwrap();
        let identity_summary = resolve_owner_constraint_seed(&identity_seed, []).unwrap();
        let value_summary = resolve_owner_constraint_seed(&value_seed, []).unwrap();
        let results = solve(
            &[label_seed.clone(), identity_seed.clone(), value_seed],
            &[
                label_summary.clone(),
                identity_summary.clone(),
                value_summary,
            ],
        );
        let label_plan = plan_owner_body_interfaces(&label_seed, &label_summary, &results).unwrap();
        let label = label_plan
            .own_scc
            .module
            .constant_result(&label_owner)
            .expect("constant Text transfer must be compiled once by its module");
        assert_eq!(label.flow_type.ty, Type::Text);
        assert_eq!(label.static_number, None);
        let providers = BTreeMap::from([(label_owner.clone(), label_plan.own_scc.module.as_ref())]);
        let arguments = OwnerResidualDraftArguments::from([(
            0,
            EvaluatedResultValue {
                flow_type: FlowType {
                    mode: FlowMode::Continuous,
                    ty: Type::Number,
                },
                parameter_derived: false,
                syntax_selected: false,
                static_number: Some(ExactNumber::from_i64(1)),
            },
        )]);
        let mut unifier = TypeUnifier::default();
        let mut evaluator = CompiledOwnerResidualEvaluator::new(&providers, &mut unifier);
        let evaluated = evaluator
            .evaluate_owner(&label_owner, &arguments, None)
            .expect("constant result must retain occurrence metadata");
        assert_eq!(evaluated.value.flow_type.ty, Type::Text);
        assert!(
            evaluated
                .type_substitutions
                .iter()
                .any(|(_, value)| value == &Type::Number)
        );
        let identity_plan =
            plan_owner_body_interfaces(&identity_seed, &identity_summary, &results).unwrap();
        assert!(
            identity_plan
                .own_scc
                .module
                .constant_result(&identity_owner)
                .is_none(),
            "parameter-dependent transfers must remain occurrence-local"
        );
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
    fn hold_call_transfer_replays_initial_and_update_values() {
        let source = concat!(
            "FUNCTION machine(trigger) {\n",
            "    NotStarted |> HOLD state {\n",
            "        trigger |> THEN { WaveformOpened[timescale_unit: TEXT { ns }] }\n",
            "    }\n",
            "}\n",
            "value: machine(trigger: True)\n",
        );
        let unit = link(source);
        let machine = owner_named(&unit, "machine");
        let value = owner_named(&unit, "value");
        let owners = unit.stable_check_owner_keys().collect::<Vec<_>>();
        let inputs = owners
            .iter()
            .map(|owner| (owner.clone(), self::inputs(&unit, owner)))
            .collect::<BTreeMap<_, _>>();
        let machine_parameters = inputs[&machine]
            .2
            .declarations
            .iter()
            .find(|declaration| declaration.public)
            .expect("machine declaration")
            .parameters
            .clone();
        let summaries = owners
            .iter()
            .map(|owner| {
                let seed = &inputs[owner].2;
                if owner == &value {
                    let reference = seed
                        .references
                        .iter()
                        .find(|reference| {
                            reference.kind == OwnerReferenceKind::Callable
                                && reference.parts.as_ref() == ["machine"]
                        })
                        .cloned()
                        .expect("value machine reference");
                    resolve_owner_constraint_seed(
                        seed,
                        [ResolvedOwnerSymbolReference {
                            reference,
                            owner: machine.clone(),
                            projection: Box::new([]),
                            parameters: machine_parameters.clone(),
                        }],
                    )
                    .unwrap()
                } else {
                    resolve_owner_constraint_seed(seed, []).unwrap()
                }
            })
            .collect::<Vec<_>>();
        let seeds = owners
            .iter()
            .map(|owner| inputs[owner].2.clone())
            .collect::<Vec<_>>();
        let interfaces = solve(&seeds, &summaries);
        let value_index = owners
            .iter()
            .position(|owner| owner == &value)
            .expect("value owner index");
        let body = infer(
            &inputs[&value].0,
            &inputs[&value].2,
            &summaries[value_index],
            &interfaces,
        );
        let call = body
            .calls
            .iter()
            .find(|call| call.function == "machine")
            .expect("machine occurrence");
        let Type::VariantSet(variants) = &call.result.ty else {
            panic!("machine call must retain HOLD state domain: {call:#?}");
        };
        assert!(
            variants
                .iter()
                .any(|variant| { matches!(variant, Variant::Tag(tag) if tag == "NotStarted") })
        );
        assert!(variants.iter().any(|variant| {
            matches!(
                variant,
                Variant::Tagged { tag, fields }
                    if tag == "WaveformOpened"
                        && fields.fields.get("timescale_unit") == Some(&Type::Text)
            )
        }));
        assert!(boon_checked::type_is_recursively_closed(&call.result.ty));
    }

    #[test]
    fn wrapper_call_closes_a_nested_producer_record_with_a_hold() {
        let source = concat!(
            "FUNCTION stateful(row) {\n",
            "    [\n",
            "        formatter: row.formatter |> HOLD formatter {}\n",
            "        segments: row.segments\n",
            "    ]\n",
            "}\n",
            "FUNCTION wrapper(row) {\n",
            "    row.item_kind |> WHEN {\n",
            "        VariableRow => stateful(row: row)\n",
            "        __ => row\n",
            "    }\n",
            "}\n",
            "FUNCTION producer(raw) {\n",
            "    [\n",
            "        item_kind: VariableRow\n",
            "        formatter: Hexadecimal\n",
            "        segments: raw.segments\n",
            "    ]\n",
            "}\n",
            "value: wrapper(row: producer(raw: [segments: TEXT { closed }]))\n",
        );
        let unit = link(source);
        let stateful = owner_named(&unit, "stateful");
        let wrapper = owner_named(&unit, "wrapper");
        let producer = owner_named(&unit, "producer");
        let value = owner_named(&unit, "value");
        let owners = unit.stable_check_owner_keys().collect::<Vec<_>>();
        let inputs = owners
            .iter()
            .map(|owner| (owner.clone(), self::inputs(&unit, owner)))
            .collect::<BTreeMap<_, _>>();
        let parameters = |owner: &StableCheckOwnerKey| {
            inputs[owner]
                .2
                .declarations
                .iter()
                .find(|declaration| declaration.public)
                .expect("callable declaration")
                .parameters
                .clone()
        };
        let summaries = owners
            .iter()
            .map(|owner| {
                let seed = &inputs[owner].2;
                let resolutions = seed.references.iter().filter_map(|reference| {
                    if reference.kind != OwnerReferenceKind::Callable {
                        return None;
                    }
                    let target = if reference.parts.as_ref() == ["stateful"] {
                        &stateful
                    } else if reference.parts.as_ref() == ["wrapper"] {
                        &wrapper
                    } else if reference.parts.as_ref() == ["producer"] {
                        &producer
                    } else {
                        return None;
                    };
                    Some(ResolvedOwnerSymbolReference {
                        reference: reference.clone(),
                        owner: target.clone(),
                        projection: Box::new([]),
                        parameters: parameters(target),
                    })
                });
                resolve_owner_constraint_seed(seed, resolutions).unwrap()
            })
            .collect::<Vec<_>>();
        let seeds = owners
            .iter()
            .map(|owner| inputs[owner].2.clone())
            .collect::<Vec<_>>();
        let modules = solve(&seeds, &summaries);
        let interface = modules
            .iter()
            .find_map(|module| module.owner(&value))
            .expect("value public interface");

        assert!(
            boon_checked::type_is_recursively_closed(&interface.result.ty),
            "wrapper occurrence must close every generic field: {interface:#?}",
        );
        let Type::Object(value) = &interface.result.ty else {
            panic!("wrapper occurrence must publish a record: {interface:#?}");
        };
        assert_eq!(
            value.fields.get("formatter"),
            Some(&Type::VariantSet(
                vec![Variant::Tag("Hexadecimal".to_owned())].into()
            )),
        );
        assert_eq!(value.fields.get("segments"), Some(&Type::Text),);
    }

    #[test]
    fn late_closed_contextual_provider_replaces_a_projected_occurrence() {
        let mut unifier = TypeUnifier::default();
        let root = unifier.fresh();
        let occurrence = unifier.fresh();
        let projection = ["file".to_owned()];

        // The first owner-body pass sees only the contextual declaration root
        // and creates an open consumer projection.
        bind_signature_declaration_read_type(&mut unifier, root, occurrence, &projection);
        assert!(!boon_checked::type_is_recursively_closed(
            &unifier.resolve(&Type::Var(occurrence))
        ));

        // A later enclosing List/filter or List/map input closes the FreshOut
        // item. Replaying the read must replace the detached occurrence root,
        // which is the exact default_file_tree_selected_file failure shape.
        unifier.publish_authoritative_provider(
            root,
            Type::object(ObjectShape::from_ordered_fields(
                [("file".to_owned(), Type::Text)],
                false,
            )),
        );
        bind_signature_declaration_read_type(&mut unifier, root, occurrence, &projection);
        assert_eq!(unifier.resolve(&Type::Var(occurrence)), Type::Text);
    }

    #[test]
    fn closed_call_input_refreshes_an_existing_fresh_out_projection() {
        let mut unifier = TypeUnifier::default();
        let actual = unifier.fresh();
        let fresh_out = unifier.fresh();
        let occurrence = unifier.fresh();
        let projection = ["file".to_owned()];

        // The output-scoped mapper body projects its FreshOut before the
        // parent list actual has specialized the formal item alpha.
        bind_signature_declaration_read_type(&mut unifier, fresh_out, occurrence, &projection);
        assert!(!boon_checked::type_is_recursively_closed(
            &unifier.resolve(&Type::Var(occurrence))
        ));

        unifier.bind_var(
            actual,
            Type::List(Type::shared(Type::object(
                ObjectShape::from_ordered_fields([("file".to_owned(), Type::Text)], false),
            ))),
        );
        unifier.bind_call_input(actual, Type::List(Type::shared(Type::Var(fresh_out))));
        bind_signature_declaration_read_type(&mut unifier, fresh_out, occurrence, &projection);

        assert_eq!(unifier.resolve(&Type::Var(occurrence)), Type::Text);
    }

    #[test]
    fn filter_map_then_projection_latest_closes_the_owner_body_result() {
        let source = concat!(
            "rows: LIST {\n",
            "    [kind: FileTreeFile, file: TEXT { simple.vcd }, file_row_elements: [select_file: Duration[milliseconds: 1] |> Timer/interval()]]\n",
            "}\n",
            "selected_file:\n",
            "    rows\n",
            "    |> List/filter(item, if: item.kind == FileTreeFile)\n",
            "    |> List/map(item, new:\n",
            "        item.file_row_elements.select_file |> THEN { item.file }\n",
            "    )\n",
            "    |> List/latest()\n",
        );
        let unit = link(source);
        let rows = owner_named(&unit, "rows");
        let selected_file = owner_named(&unit, "selected_file");
        let owners = unit.stable_check_owner_keys().collect::<Vec<_>>();
        let inputs = owners
            .iter()
            .map(|owner| (owner.clone(), self::inputs(&unit, owner)))
            .collect::<BTreeMap<_, _>>();
        let summaries = owners
            .iter()
            .map(|owner| {
                let seed = &inputs[owner].2;
                if owner == &selected_file {
                    let reference = seed
                        .references
                        .iter()
                        .find(|reference| {
                            reference.kind == OwnerReferenceKind::Value
                                && reference.parts.as_ref() == ["rows"]
                        })
                        .cloned()
                        .expect("selected_file rows reference");
                    resolve_owner_constraint_seed(
                        seed,
                        [ResolvedOwnerSymbolReference {
                            reference,
                            owner: rows.clone(),
                            projection: Box::new([]),
                            parameters: Box::new([]),
                        }],
                    )
                    .unwrap()
                } else {
                    resolve_owner_constraint_seed(seed, []).unwrap()
                }
            })
            .collect::<Vec<_>>();
        let seeds = owners
            .iter()
            .map(|owner| inputs[owner].2.clone())
            .collect::<Vec<_>>();
        let project =
            ProjectSyntaxSnapshot::from_unit_snapshots("app/RUN.bn", vec![Arc::new(unit.clone())])
                .unwrap();
        let abi_provider = crate::project_owner_abi_environment(
            &project,
            &boon_checked::ExternalTypeEnvironment::empty(boon_checked::ProgramRole::Client),
        )
        .unwrap();
        let interfaces = solve_with_abi(&seeds, &summaries, &abi_provider);
        let selected_index = owners
            .iter()
            .position(|owner| owner == &selected_file)
            .expect("selected_file owner index");
        let body = infer_with_abi(
            &inputs[&selected_file].0,
            &inputs[&selected_file].2,
            &summaries[selected_index],
            &interfaces,
            &abi_provider,
        );

        assert_eq!(
            body.calls
                .iter()
                .map(|call| call.function.as_str())
                .collect::<Vec<_>>(),
            ["List/latest", "List/map", "List/filter"]
        );
        let filter = body
            .calls
            .iter()
            .find(|call| call.function == "List/filter")
            .expect("List/filter call");
        let map = body
            .calls
            .iter()
            .find(|call| call.function == "List/map")
            .expect("List/map call");
        let latest = body
            .calls
            .iter()
            .find(|call| call.function == "List/latest")
            .expect("List/latest call");
        assert!(matches!(filter.result.ty, Type::List(_)));
        assert_eq!(
            map.result,
            FlowType {
                ty: Type::List(Type::shared(Type::Text)),
                mode: FlowMode::PresentOrAbsent,
            }
        );
        assert_eq!(latest.result.ty, Type::Text);
        assert_eq!(
            body.expression(&latest.expression)
                .expect("List/latest expression")
                .flow_type
                .ty,
            Type::Text
        );
        assert_eq!(
            interfaces
                .iter()
                .find_map(|result| result.owner(&selected_file))
                .expect("selected_file interface")
                .result
                .ty,
            Type::Text
        );
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
    fn sealed_residual_program_keeps_private_alphas_out_of_the_public_interface() {
        let unit = link(concat!(
            "FUNCTION choose(kind) {\n",
            "    kind |> WHEN {\n",
            "        Record => [value: 1]\n",
            "        __ => LIST { 1 }\n",
            "    }\n",
            "}\n",
        ));
        let choose = owner_named(&unit, "choose");
        let (_, _, seed) = inputs(&unit, &choose);
        let summary = resolve_owner_constraint_seed(&seed, []).unwrap();
        let modules = solve(&[seed.clone()], &[summary.clone()]);
        let module = modules
            .iter()
            .find(|module| module.owns_owner(&choose))
            .expect("choose residual module");
        let owner = module.key.members.binary_search(&choose).unwrap();
        let program = &module.program.owners[owner];

        assert!(!program.ops.is_empty());
        assert_eq!(program.frames[program.root.frame() as usize].namespace, 0);
        assert!(program.namespaces[0] >= module.type_variable_count);
        assert_eq!(module.program.op_count, program.ops.len() as u64);
        assert_eq!(
            module.program.edge_count,
            program
                .ops
                .iter()
                .map(|op| op.kind.edge_count())
                .sum::<u64>()
        );

        let mut forged = module.result.owners[owner].clone();
        forged.lexical_captures = vec![crate::OwnerInterfaceLexicalCapture {
            target: OwnerLexicalTargetRef::Declaration {
                owner: choose,
                declaration: boon_checked::OwnerDeclarationStableKey::Public,
                capability: boon_checked::OwnerLexicalDeclarationCapability::Value,
            },
            demand_paths: vec![Box::<[String]>::default()].into_boxed_slice(),
            flow_type: FlowType {
                mode: FlowMode::Continuous,
                ty: Type::Var(TypeVar(u32::MAX)),
            },
        }]
        .into_boxed_slice();
        assert!(!owner_result_transfer_interface_variables_are_complete(
            &forged,
            &OwnerResidualDraft::Principal,
            program.namespaces[0],
        ));
    }

    #[test]
    fn residual_linker_is_exact_and_composes_acyclic_calls() {
        let unit = link(concat!(
            "FUNCTION leaf(input) {\n",
            "    input\n",
            "}\n",
            "FUNCTION wrapper(input) {\n",
            "    leaf(input: input)\n",
            "}\n",
            "spare: 2\n",
        ));
        let leaf = owner_named(&unit, "leaf");
        let wrapper = owner_named(&unit, "wrapper");
        let spare = owner_named(&unit, "spare");
        let (_, _, leaf_seed) = inputs(&unit, &leaf);
        let (_, _, wrapper_seed) = inputs(&unit, &wrapper);
        let (_, _, spare_seed) = inputs(&unit, &spare);
        let leaf_summary = resolve_owner_constraint_seed(&leaf_seed, []).unwrap();
        let callable = wrapper_seed
            .references
            .iter()
            .find(|reference| reference.kind == OwnerReferenceKind::Callable)
            .cloned()
            .expect("wrapper callable reference");
        let parameters = leaf_seed
            .declarations
            .iter()
            .find(|declaration| declaration.public)
            .expect("leaf public declaration")
            .parameters
            .clone();
        let wrapper_summary = resolve_owner_constraint_seed(
            &wrapper_seed,
            [ResolvedOwnerSymbolReference {
                reference: callable,
                owner: leaf.clone(),
                projection: Box::new([]),
                parameters,
            }],
        )
        .unwrap();
        let spare_summary = resolve_owner_constraint_seed(&spare_seed, []).unwrap();
        let modules = solve(
            &[leaf_seed.clone(), wrapper_seed, spare_seed],
            &[leaf_summary, wrapper_summary, spare_summary],
        );
        let leaf_module = modules
            .iter()
            .find(|module| module.owns_owner(&leaf))
            .expect("leaf residual module");
        let wrapper_module = modules
            .iter()
            .find(|module| module.owns_owner(&wrapper))
            .expect("wrapper residual module");
        let spare_module = modules
            .iter()
            .find(|module| module.owns_owner(&spare))
            .expect("spare residual module");
        let wrapper_program = &wrapper_module.program.owners[0];

        assert!(wrapper_program.ops.iter().any(|op| matches!(
            op.kind,
            OwnerResidualOpKind::CompiledCall {
                target: OwnerResidualCompiledCallTarget::Dependency { .. },
                ..
            }
        )));

        let child_draft = OwnerResidualDraft::Expression {
            root: OwnerResidualExpressionRef::Child {
                owner: leaf.clone(),
                expression: leaf_seed.expressions[0].expression.clone(),
            },
            nodes: Box::new([]),
        };
        let residual_type_variable_count = wrapper_program.namespaces[0];
        let error = seal_owner_interface_transfer_module(
            Arc::clone(&wrapper_module.result),
            vec![child_draft.clone()].into_boxed_slice(),
            residual_type_variable_count,
            &[],
            [],
        )
        .unwrap_err();
        assert!(error.to_string().contains("non-direct dependency"));

        let leaf_key = leaf_module.key.clone();
        let error = seal_owner_interface_transfer_module(
            Arc::clone(&wrapper_module.result),
            vec![child_draft.clone()].into_boxed_slice(),
            residual_type_variable_count,
            std::slice::from_ref(&leaf_key),
            [Arc::new(spare_module.clone())],
        )
        .unwrap_err();
        assert!(error.to_string().contains("wrong direct dependencies"));

        let error = seal_owner_interface_transfer_module(
            Arc::clone(&wrapper_module.result),
            vec![OwnerResidualDraft::Principal].into_boxed_slice(),
            residual_type_variable_count,
            std::slice::from_ref(&leaf_key),
            [Arc::new(leaf_module.clone())],
        )
        .unwrap_err();
        assert!(error.to_string().contains("unused dependency"));

        let exact = seal_owner_interface_transfer_module(
            Arc::clone(&wrapper_module.result),
            vec![child_draft].into_boxed_slice(),
            residual_type_variable_count,
            std::slice::from_ref(&leaf_key),
            [Arc::new(leaf_module.clone())],
        )
        .expect("exact direct dependency seals");
        assert_eq!(
            exact.direct_dependency_keys().collect::<Vec<_>>(),
            [&leaf_key]
        );
    }

    #[test]
    fn residual_linker_shares_same_scc_definitions_and_falls_back_only_on_the_actual_backedge() {
        let unit = link(concat!(
            "FUNCTION first(value) {\n",
            "    second(value: value)\n",
            "}\n",
            "FUNCTION second(value) {\n",
            "    Stop |> WHEN {\n",
            "        Stop => value\n",
            "        __ => first(value: value)\n",
            "    }\n",
            "}\n",
            "value: first(value: Ready)\n",
        ));
        let first = owner_named(&unit, "first");
        let second = owner_named(&unit, "second");
        let owners = unit.stable_check_owner_keys().collect::<Vec<_>>();
        let inputs = owners
            .iter()
            .map(|owner| (owner.clone(), self::inputs(&unit, owner)))
            .collect::<BTreeMap<_, _>>();
        let parameters = |owner: &StableCheckOwnerKey| {
            inputs[owner]
                .2
                .declarations
                .iter()
                .find(|declaration| declaration.public)
                .expect("callable declaration")
                .parameters
                .clone()
        };
        let summaries = owners
            .iter()
            .map(|owner| {
                let seed = &inputs[owner].2;
                let resolutions = seed.references.iter().filter_map(|reference| {
                    if reference.kind != OwnerReferenceKind::Callable {
                        return None;
                    }
                    let target = if reference.parts.as_ref() == ["first"] {
                        &first
                    } else if reference.parts.as_ref() == ["second"] {
                        &second
                    } else {
                        return None;
                    };
                    Some(ResolvedOwnerSymbolReference {
                        reference: reference.clone(),
                        owner: target.clone(),
                        projection: Box::new([]),
                        parameters: parameters(target),
                    })
                });
                resolve_owner_constraint_seed(seed, resolutions).unwrap()
            })
            .collect::<Vec<_>>();
        let seeds = owners
            .iter()
            .map(|owner| inputs[owner].2.clone())
            .collect::<Vec<_>>();
        let modules = solve(&seeds, &summaries);
        let recursive_module = modules
            .iter()
            .find(|module| module.owns_owner(&first))
            .expect("recursive component module");
        assert!(recursive_module.owns_owner(&second));
        let first_index = recursive_module.key.members.binary_search(&first).unwrap();
        let second_index = recursive_module.key.members.binary_search(&second).unwrap();
        let first_program = &recursive_module.program.owners[first_index];
        let second_program = &recursive_module.program.owners[second_index];
        assert!(first_program.ops.iter().any(|op| matches!(
            op.kind,
            OwnerResidualOpKind::CompiledCall {
                target: OwnerResidualCompiledCallTarget::Own { owner },
                ..
            } if owner as usize == second_index
        )));
        assert!(second_program.ops.iter().any(|op| matches!(
            op.kind,
            OwnerResidualOpKind::CompiledCall {
                target: OwnerResidualCompiledCallTarget::Own { owner },
                ..
            } if owner as usize == first_index
        )));
        assert_eq!(
            recursive_module.program.op_count,
            first_program.ops.len() as u64 + second_program.ops.len() as u64,
            "same-component callees must be stored once, not physically copied at call sites",
        );
    }

    #[test]
    fn residual_linker_rejects_an_undeclared_alpha() {
        let unit = link("value: 1\n");
        let value = owner_named(&unit, "value");
        let (_, _, seed) = inputs(&unit, &value);
        let summary = resolve_owner_constraint_seed(&seed, []).unwrap();
        let modules = solve(&[seed], &[summary]);
        let module = modules
            .iter()
            .find(|module| module.owns_owner(&value))
            .expect("value residual module");
        let residual_type_variable_count = module.program.owners[0].namespaces[0];
        let mut forged = (*module.result).clone();
        forged.owners[0].result.ty = Type::Var(TypeVar(residual_type_variable_count));

        let error = seal_owner_interface_transfer_module(
            Arc::new(forged),
            vec![OwnerResidualDraft::Principal].into_boxed_slice(),
            residual_type_variable_count,
            &[],
            [],
        )
        .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("incomplete alpha-variable namespace")
        );
    }

    #[test]
    fn residual_surface_namespaces_share_within_one_dependency_and_isolate_peers() {
        let fallback = FlowType {
            mode: FlowMode::PresentOrAbsent,
            ty: Type::Var(TypeVar(0)),
        };
        let surface = |namespace| OwnerResidualOp {
            frame: 0,
            fallback: fallback.clone(),
            static_number: None,
            kind: OwnerResidualOpKind::Surface { namespace },
        };
        let program = OwnerResidualOwnerProgram {
            namespaces: vec![1, 1, 1].into_boxed_slice(),
            frames: Box::new([]),
            ops: vec![surface(0), surface(1), surface(1), surface(2)].into_boxed_slice(),
            root: OwnerResidualRoot::Principal { frame: 0 },
            invocation_invariant: false,
        };
        let arguments = OwnerResidualDraftArguments::default();
        let mut unifier = TypeUnifier::default();
        let mut active_owners = SmallVec::new();
        let mut work = OwnerResidualEvaluationWork::default();
        let mut evaluator = CompiledOwnerResidualProgramEvaluator::new(
            None,
            &program,
            &arguments,
            None,
            &mut unifier,
            &mut active_owners,
            &mut work,
        );
        let Type::Var(own) = evaluator
            .resolve_type(0, &Type::Var(TypeVar(0)), None)
            .unwrap()
        else {
            unreachable!();
        };
        evaluator.unifier.bind_var(own, Type::Text);
        let mut active = OwnerResidualDraftActiveNodes::new();
        let lexical = BTreeMap::new();
        let own = evaluator.evaluate_op(0, &lexical, &mut active).unwrap();
        let dependency = evaluator.evaluate_op(1, &lexical, &mut active).unwrap();
        let repeated = evaluator.evaluate_op(2, &lexical, &mut active).unwrap();
        let peer = evaluator.evaluate_op(3, &lexical, &mut active).unwrap();

        assert_eq!(own.flow_type.ty, Type::Text);
        assert!(dependency == repeated);
        assert_ne!(dependency.flow_type.ty, Type::Text);
        assert_ne!(dependency.flow_type.ty, peer.flow_type.ty);
    }

    #[test]
    fn closed_all_arm_transfer_replaces_an_open_principal_without_syntax_selection() {
        let unit = link(concat!(
            "FUNCTION label(value) {\n",
            "    value |> WHEN {\n",
            "        BinaryValue => value.bits\n",
            "        StringValue => value.text\n",
            "        __ => TEXT { ? }\n",
            "    }\n",
            "}\n",
        ));
        let label = owner_named(&unit, "label");
        let (_, _, seed) = inputs(&unit, &label);
        let summary = resolve_owner_constraint_seed(&seed, []).unwrap();
        let interfaces = solve(&[seed.clone()], &[summary.clone()]);
        let interface = interfaces
            .iter()
            .find_map(|result| result.owner(&label))
            .expect("label interface");
        assert!(
            !boon_checked::type_is_recursively_closed(&interface.result.ty),
            "fixture requires the broad definition-site principal: {interface:#?}",
        );

        let plan = plan_owner_body_interfaces(&seed, &summary, &interfaces).unwrap();
        let providers = BTreeMap::from([(label.clone(), plan.own_scc.module.as_ref())]);
        let actual = Type::VariantSet(
            vec![
                Variant::Tagged {
                    tag: "BinaryValue".to_owned(),
                    fields: ObjectShape::from_ordered_fields::<boon_checked::SharedObjectShape>(
                        [("bits".to_owned(), Type::Text)],
                        false,
                    ),
                },
                Variant::Tagged {
                    tag: "StringValue".to_owned(),
                    fields: ObjectShape::from_ordered_fields::<boon_checked::SharedObjectShape>(
                        [("text".to_owned(), Type::Text)],
                        false,
                    ),
                },
            ]
            .into(),
        );
        let arguments = OwnerResidualDraftArguments::from([(
            0,
            EvaluatedResultValue {
                flow_type: FlowType {
                    mode: FlowMode::Continuous,
                    ty: actual,
                },
                parameter_derived: true,
                syntax_selected: false,
                static_number: None,
            },
        )]);
        let mut unifier = TypeUnifier::default();
        let mut evaluator = CompiledOwnerResidualEvaluator::new(&providers, &mut unifier);
        let evaluated = evaluator
            .evaluate_owner(&label, &arguments, None)
            .expect("label transfer result");
        assert_eq!(evaluated.value.flow_type.ty, Type::Text);
        assert!(evaluated.value.parameter_derived);
        assert!(
            !evaluated.value.syntax_selected,
            "a multi-variant selector evaluates every arm",
        );
    }

    #[test]
    fn syntax_selected_call_transfer_matches_the_whole_checker_oracle() {
        let source = "FUNCTION choose(kind) {\n    kind |> WHEN {\n        Record => [\n            value: 1\n        ]\n        __ => LIST { 1 }\n    }\n}\nFUNCTION probe() {\n    selected: choose(kind: Record)\n    checked: selected.value + 1\n    selected\n}\n";
        let unit = link(source);
        let value = owner_named(&unit, "probe");
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
            "choose residual: {:#?}",
            interfaces
                .iter()
                .find(|module| module.owns_owner(&choose))
                .unwrap()
                .program
        );
        assert_eq!(
            call.syntax_discriminated_result,
            oracle.syntax_discriminated_result
        );
        assert!(call.syntax_discriminated_result);
        let Type::Object(result) = &call.result.ty else {
            panic!("selected choose occurrence must be one record: {call:#?}");
        };
        assert!(
            !result.open,
            "a downstream field requirement must not reopen the selected result",
        );
        assert_eq!(result.fields.get("value"), Some(&Type::Number));
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

        assert!(
            body.diagnostics.iter().any(|diagnostic| {
                diagnostic.code == "hold_initializer_flush"
                    && diagnostic.message
                        == "a `HOLD` initializer must produce a valid storable value and cannot `FLUSH`"
            }),
            "interfaces: {interfaces:#?}\nbody: {body:#?}"
        );
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
    fn compact_body_seal_changes_with_semantic_content() {
        let original = link("value: 1\n");
        let changed = link("value: 2\n");
        let owner = owner_named(&original, "value");
        let changed_owner = owner_named(&changed, "value");
        let (syntax, _, seed) = inputs(&original, &owner);
        let (changed_syntax, _, changed_seed) = inputs(&changed, &changed_owner);
        let summary = resolve_owner_constraint_seed(&seed, []).unwrap();
        let changed_summary = resolve_owner_constraint_seed(&changed_seed, []).unwrap();
        let interfaces = solve(&[seed.clone()], &[summary.clone()]);
        let changed_interfaces = solve(&[changed_seed.clone()], &[changed_summary.clone()]);
        let body = infer(&syntax, &seed, &summary, &interfaces);
        let changed_body = infer(
            &changed_syntax,
            &changed_seed,
            &changed_summary,
            &changed_interfaces,
        );

        assert_ne!(body.fingerprint_v1(), changed_body.fingerprint_v1());
        assert_ne!(
            body.receipt.local_content_digest_v1,
            changed_body.receipt.local_content_digest_v1
        );
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
    fn missing_authoritative_signature_remains_a_body_diagnostic_not_a_plan_error() {
        let unit = link("value: Number/to_text(value: 1)\n");
        let owner = owner_named(&unit, "value");
        let (syntax, _, seed) = inputs(&unit, &owner);
        let lexical_plan = project_owner_lexical_plan(&syntax).unwrap();
        let summary = resolve_owner_constraint_seed(&seed, []).unwrap();
        let abi = crate::OwnerInferenceAbiEnvironment::from_lookups(
            [owner.clone()],
            [crate::OwnerCallableAbiLookup::missing("Number/to_text").unwrap()],
        )
        .unwrap();
        let topology = build_owner_interface_topology([&summary]).unwrap();
        let component = evaluate_owner_interface_scc_component_for_tests(
            topology.sccs.first().unwrap(),
            &abi,
            [&seed],
            [&summary],
            [],
            [],
        )
        .unwrap();
        let body = infer_owner_body(
            &syntax,
            &lexical_plan,
            &seed,
            &summary,
            &abi,
            &component.module,
            [],
        )
        .unwrap();

        assert_eq!(body.calls.len(), 1);
        assert!(!body.calls[0].valid);
        assert!(body.signature_lexical_plan.calls().iter().any(|call| {
            call.function == "Number/to_text"
                && matches!(call.target, crate::OwnerSignatureCallTarget::Authoritative)
                && !call.valid
        }));
        assert!(
            body.diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "missing_authoritative_callable")
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
