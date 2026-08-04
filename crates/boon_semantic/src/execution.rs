//! Normalized semantic execution graph.
//!
//! This module owns the post-elaboration graph that contract verification
//! inspects.  Its IDs are deliberately distinct from executable/backend IDs:
//! lowering may remap them, but it must not reinterpret checked coordinates as
//! executable identity.

use crate::{
    OutCallInstanceId, ProducerFunctionId, ProducerResultStatementId, ResolvedOutGraph,
    StaticOwnerDef, StaticOwnerId,
};
use boon_checked::{
    CheckedCallContextKind, CheckedCallId, CheckedCallableKind, CheckedContextBinding,
    CheckedContextTypeSubstitution, CheckedContextualOperation, CheckedEffectSummary,
    CheckedEvaluationScope, CheckedExprId, CheckedExternalDeclarationIdentityV1,
    CheckedIntrinsicV1, CheckedMatchPattern, CheckedParameterKind, CheckedParameterRequirement,
    CheckedProgramFields, CheckedResourceBinding, CheckedScopeKind, CheckedSourceId, CheckedSpan,
    CheckedStateId, CheckedStatementId, CheckedStatementKind, CheckedTypeSubstitution,
    ContextFormalId, DeclId, FlowType, LexicalScopeId, ProgramRole, Type,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt;

macro_rules! typed_semantic_id {
    ($($name:ident),+ $(,)?) => {
        $(
            #[derive(
                Clone,
                Copy,
                Debug,
                Eq,
                Hash,
                Ord,
                PartialEq,
                PartialOrd,
                Serialize,
                Deserialize,
            )]
            #[serde(transparent)]
            pub struct $name(pub usize);

            impl $name {
                pub const fn as_usize(self) -> usize {
                    self.0
                }
            }

            impl fmt::Display for $name {
                fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                    self.0.fmt(formatter)
                }
            }
        )+
    };
}

typed_semantic_id!(
    SemanticExprId,
    SemanticValueId,
    SemanticLocalBindingId,
    SemanticStatementId,
    SemanticSourceId,
    SemanticStateId,
    SemanticCallableId,
    SemanticListId,
    SemanticMigrationId,
    SemanticMaterializationId,
    SemanticMaterializationLocalId,
    SemanticRowScopeId,
    SemanticFieldId,
    SemanticBindingId,
    SemanticReadId,
    SemanticCaptureId,
    SemanticDerivedValueId,
    SemanticTriggerArmId,
    SemanticStateUpdateArmId,
    SemanticListMutationId,
    SemanticExternalDependencyId,
    SemanticDependencyUseId,
    SemanticHostEffectScheduleId,
    SemanticActivationId,
    SemanticPulseBatchId,
    SemanticRowValueId,
    SemanticValueListAuthorityId,
    SemanticScopeId,
    SemanticCallId,
);

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct SemanticParameterId {
    pub callable: SemanticCallableId,
    pub ordinal: usize,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct SemanticCallContextId {
    pub call_instance: OutCallInstanceId,
    pub ordinal: usize,
}

/// One compact concrete invocation in the OUT call tree.
///
/// Shared callable definitions refer to checked call sites. An occurrence
/// overlay resolves such a site relative to its parent invocation and owns the
/// concrete call-local context ordinals without cloning the callable body.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticCallOccurrence {
    pub id: OutCallInstanceId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<OutCallInstanceId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub call: Option<SemanticCallId>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub context_ordinals: Vec<usize>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticExpression {
    pub id: SemanticExprId,
    /// The normalized value produced by this expression. In schema V1 there is
    /// exactly one dense value identity per expression identity.
    pub value_id: SemanticValueId,
    pub checked_expr_id: CheckedExprId,
    pub flow_type: FlowType,
    pub effect: CheckedEffectSummary,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<StaticOwnerId>,
    #[serde(
        default,
        skip_serializing_if = "SemanticValueProvenance::is_runtime_only"
    )]
    pub provenance: SemanticValueProvenance,
    /// Exact semantic resource path after contextual call expansion.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource_binding_path: Option<String>,
    pub kind: SemanticExpressionKind,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticExpressionOrigin {
    pub expression: SemanticExprId,
    pub checked_expression: CheckedExprId,
    pub checked_scope: LexicalScopeId,
    pub checked_span: CheckedSpan,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owning_statement: Option<SemanticStatementId>,
    /// The concrete call frame in which the checked expression was expanded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub call_instance: Option<OutCallInstanceId>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticValueProvenance {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub members: Vec<SemanticValueMember>,
}

impl Default for SemanticValueProvenance {
    fn default() -> Self {
        Self {
            members: vec![SemanticValueMember {
                path: Vec::new(),
                origin: SemanticValueOrigin::Runtime,
            }],
        }
    }
}

impl SemanticValueProvenance {
    fn is_runtime_only(&self) -> bool {
        self.members.as_slice()
            == [SemanticValueMember {
                path: Vec::new(),
                origin: SemanticValueOrigin::Runtime,
            }]
    }

    pub(crate) fn direct_resource_origin(&self) -> Option<&SemanticValueOrigin> {
        let [member] = self.members.as_slice() else {
            return None;
        };
        if !member.path.is_empty()
            || !matches!(
                &member.origin,
                SemanticValueOrigin::Source { .. } | SemanticValueOrigin::ProducerSource { .. }
            )
        {
            return None;
        }
        Some(&member.origin)
    }

    pub(crate) fn projected(&self, projection: &[String]) -> Self {
        if projection.is_empty() {
            return self.clone();
        }
        let mut members = Vec::new();
        for member in &self.members {
            if let Some(suffix) = member.path.strip_prefix(projection) {
                let mut projected = member.clone();
                projected.path = suffix.to_vec();
                members.push(projected);
                continue;
            }
            let Some(suffix) = projection.strip_prefix(member.path.as_slice()) else {
                continue;
            };
            let mut projected = member.clone();
            projected.path.clear();
            match &mut projected.origin {
                SemanticValueOrigin::MaterializationLocal {
                    projection: local_projection,
                    ..
                } => local_projection.extend_from_slice(suffix),
                SemanticValueOrigin::Source { .. } | SemanticValueOrigin::ProducerSource { .. }
                    if !suffix.is_empty() =>
                {
                    projected.origin = SemanticValueOrigin::Runtime;
                }
                SemanticValueOrigin::Runtime
                | SemanticValueOrigin::Source { .. }
                | SemanticValueOrigin::ProducerSource { .. }
                | SemanticValueOrigin::State { .. } => {}
            }
            members.push(projected);
        }
        if members.is_empty() {
            Self::default()
        } else {
            members.sort();
            members.dedup();
            Self { members }
        }
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct SemanticValueMember {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub path: Vec<String>,
    pub origin: SemanticValueOrigin,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SemanticValueOrigin {
    Runtime,
    Source {
        source: SemanticSourceId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        owner: Option<StaticOwnerId>,
    },
    ProducerSource {
        function: SemanticCallableId,
        producer: ProducerFunctionId,
        identity: [u8; 32],
        owner: StaticOwnerId,
    },
    State {
        state: SemanticStateId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        owner: Option<StaticOwnerId>,
    },
    MaterializationLocal {
        owner: StaticOwnerId,
        local: SemanticMaterializationLocalId,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        projection: Vec<String>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticSourceRead {
    pub source: SemanticSourceId,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub payload_projection: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticRecordField {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub declaration: Option<DeclId>,
    pub name: String,
    pub value: SemanticExprId,
    pub spread: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticBlockBinding {
    pub id: SemanticLocalBindingId,
    pub declaration: DeclId,
    pub value: SemanticExprId,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SemanticTextSegment {
    Static { value: String },
    Dynamic { value: SemanticExprId },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticCallArgument {
    pub formal: DeclId,
    pub ordinal: usize,
    pub name: String,
    pub checked_value: CheckedExprId,
    pub value: SemanticExprId,
    pub from_pipe: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticCallableParameter {
    pub id: SemanticParameterId,
    pub formal: DeclId,
    pub ordinal: usize,
    pub name: String,
    pub kind: CheckedParameterKind,
    pub flow_type: FlowType,
    pub requirement: CheckedParameterRequirement,
    pub evaluation_scope: CheckedEvaluationScope,
    pub start: usize,
    pub end: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticCallableContext {
    pub name: String,
    pub kind: CheckedCallContextKind,
    pub provider: DeclId,
    pub flow_type: FlowType,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticCallableContextParameter {
    pub id: SemanticParameterId,
    pub formal: ContextFormalId,
    pub name: String,
    pub flow_type: FlowType,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticCallable {
    pub id: SemanticCallableId,
    pub checked_callable: DeclId,
    pub scope: SemanticScopeId,
    pub kind: CheckedCallableKind,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub external_identity: Option<CheckedExternalDeclarationIdentityV1>,
    pub parameters: Vec<SemanticCallableParameter>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub contexts: Vec<SemanticCallableContext>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_formal: Option<ContextFormalId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_parameter: Option<SemanticCallableContextParameter>,
    pub result: FlowType,
    pub role: ProgramRole,
    pub effect: CheckedEffectSummary,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<CheckedStatementId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_expression: Option<CheckedExprId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contextual_operation: Option<CheckedContextualOperation>,
    /// Canonical semantic body for an ordinary pure callable.
    ///
    /// A retained body is shared by every ordinary call occurrence and reads
    /// inputs through [`SemanticExpressionKind::FunctionParameter`]. Open
    /// boundary types and constructor-local render contexts are supplied by
    /// compact invocation overlays. Effectful, OUT-owning, external, and
    /// element-state-reading callables remain specialized.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub semantic_root: Option<SemanticExprId>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SemanticCallEntry {
    Input {
        formal: DeclId,
        ordinal: usize,
        name: String,
        checked_value: CheckedExprId,
        value_flow_type: FlowType,
        from_pipe: bool,
        evaluation_scope: CheckedEvaluationScope,
        requirement: CheckedParameterRequirement,
    },
    FreshOut {
        formal: DeclId,
        ordinal: usize,
        name: String,
        output: DeclId,
        scope: SemanticScopeId,
    },
    ForwardOut {
        formal: DeclId,
        ordinal: usize,
        name: String,
        target: DeclId,
        target_name: String,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticCallContextBinding {
    pub declaration: DeclId,
    pub signature: usize,
    pub scope: SemanticScopeId,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticCall {
    pub id: SemanticCallId,
    pub checked_call: CheckedCallId,
    pub checked_expression: CheckedExprId,
    pub callable: SemanticCallableId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_callable: Option<SemanticCallableId>,
    pub function: String,
    pub intrinsic: Option<CheckedIntrinsicV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub external_identity: Option<CheckedExternalDeclarationIdentityV1>,
    pub entries: Vec<SemanticCallEntry>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub contexts: Vec<SemanticCallContextBinding>,
    pub context_binding: CheckedContextBinding,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub contextual_substitutions: Vec<CheckedContextTypeSubstitution>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub type_substitutions: Vec<CheckedTypeSubstitution>,
    pub result: FlowType,
    pub role: ProgramRole,
    pub effect: CheckedEffectSummary,
    pub span: CheckedSpan,
    pub occurrence_segment: String,
    pub temporally_gated: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticCallParameterBinding {
    pub formal: DeclId,
    pub ordinal: usize,
    pub name: String,
    pub requirement: CheckedParameterRequirement,
    pub kind: SemanticCallParameterBindingKind,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticCallContextArgument {
    pub formal: ContextFormalId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checked_value: Option<CheckedExprId>,
    pub value: SemanticExprId,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SemanticCallParameterBindingKind {
    Explicit {
        checked_value: CheckedExprId,
        value: SemanticExprId,
        from_pipe: bool,
    },
    Omitted,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticSelectArm {
    pub pattern: CheckedMatchPattern,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bindings: Vec<SemanticPatternBinding>,
    pub output: SemanticExprId,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticPatternBinding {
    pub name: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub projection: Vec<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticCallableKind {
    User,
    Builtin,
    External,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticSelectKind {
    When,
    While,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticExpressionKind {
    CanonicalRead {
        target: DeclId,
        path: String,
        projection: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        source: Option<SemanticSourceRead>,
    },
    LocalRead {
        binding: SemanticLocalBindingId,
        declaration: DeclId,
        projection: Vec<String>,
    },
    ExternalRead {
        canonical_path: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        external_identity: Option<CheckedExternalDeclarationIdentityV1>,
    },
    ElementState {
        context: SemanticCallContextId,
        projection: Vec<String>,
    },
    Drain {
        target: DeclId,
        path: String,
        projection: Vec<String>,
    },
    Text(String),
    TextTemplate {
        segments: Vec<SemanticTextSegment>,
    },
    Number(boon_data::ExactNumber),
    BytesByte(u8),
    /// Private flow absence. It has no public data representation.
    Absent,
    /// Private fail-fast control. This node produces no ordinary value until a
    /// compiler-inserted lexical boundary consumes the carrier.
    Flush {
        payload: SemanticExprId,
    },
    /// Removes the private fail-fast carrier and exposes its payload as the
    /// boundary's ordinary closed Tag/tagged-object result.
    FlushBoundary {
        input: SemanticExprId,
    },
    Tag(String),
    TaggedObject {
        tag: String,
        fields: Vec<SemanticRecordField>,
    },
    Source {
        binding_path: String,
    },
    Call {
        call: SemanticCallId,
        callable: SemanticCallableId,
        callable_kind: SemanticCallableKind,
        name: String,
        function: String,
        intrinsic: Option<CheckedIntrinsicV1>,
        role: ProgramRole,
        effect: CheckedEffectSummary,
        result: FlowType,
        /// Concrete OUT-graph occurrence when this call participates in OUT,
        /// contextual, effect, or distributed topology. Pure calls retained
        /// inside a shared ordinary callable body have no OUT occurrence.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        instance: Option<OutCallInstanceId>,
        arguments: Vec<SemanticCallArgument>,
        parameter_bindings: Vec<SemanticCallParameterBinding>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        context_argument: Option<SemanticCallContextArgument>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        contexts: Vec<SemanticCallContextId>,
    },
    Materialize {
        materialization: SemanticMaterializationId,
    },
    Draining {
        input: SemanticExprId,
    },
    Hold {
        initial: SemanticExprId,
        name: String,
        binding_path: String,
        updates: Vec<SemanticExprId>,
    },
    Latest {
        branches: Vec<SemanticExprId>,
    },
    When {
        select_kind: SemanticSelectKind,
        input: SemanticExprId,
        arms: Vec<SemanticSelectArm>,
    },
    Then {
        input: SemanticExprId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        output: Option<SemanticExprId>,
    },
    Infix {
        left: SemanticExprId,
        op: String,
        right: SemanticExprId,
    },
    MatchArm {
        pattern: CheckedMatchPattern,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        output: Option<SemanticExprId>,
    },
    Object(Vec<SemanticRecordField>),
    Block {
        bindings: Vec<SemanticBlockBinding>,
        result: SemanticExprId,
    },
    List {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        capacity: Option<usize>,
        items: Vec<SemanticExprId>,
    },
    Bytes {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        fixed_size: Option<usize>,
        items: Vec<SemanticExprId>,
    },
    Delimiter,
    Project {
        input: SemanticExprId,
        fields: Vec<String>,
    },
    MaterializationLocal {
        owner: StaticOwnerId,
        local: SemanticMaterializationLocalId,
        projection: Vec<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        constructor_projection: Vec<String>,
    },
    FunctionParameter {
        parameter: SemanticParameterId,
        projection: Vec<String>,
    },
    MapEntry {
        key: SemanticExprId,
        value: SemanticExprId,
    },
    Map {
        entries: Vec<SemanticExprId>,
    },
    Set {
        items: Vec<SemanticExprId>,
    },
    Bits(boon_data::Bits),
}

impl SemanticExpressionKind {
    /// Canonical direct expression edges.
    ///
    /// `Materialize` is deliberately a leaf here because its expression roots
    /// belong to the referenced materialization rather than to the expression
    /// node itself. Passes that traverse through materializations use
    /// [`SemanticExecutionImageColumnsV1::expression_children`] instead.
    pub(crate) fn direct_children(&self) -> Vec<SemanticExprId> {
        match self {
            Self::CanonicalRead { .. }
            | Self::LocalRead { .. }
            | Self::ExternalRead { .. }
            | Self::ElementState { .. }
            | Self::Drain { .. }
            | Self::Text(_)
            | Self::Number(_)
            | Self::Bits(_)
            | Self::BytesByte(_)
            | Self::Absent
            | Self::Tag(_)
            | Self::Source { .. }
            | Self::Materialize { .. }
            | Self::Delimiter
            | Self::MaterializationLocal { .. }
            | Self::FunctionParameter { .. } => Vec::new(),
            Self::TextTemplate { segments } => segments
                .iter()
                .filter_map(|segment| match segment {
                    SemanticTextSegment::Static { .. } => None,
                    SemanticTextSegment::Dynamic { value } => Some(*value),
                })
                .collect(),
            Self::TaggedObject { fields, .. } | Self::Object(fields) => {
                fields.iter().map(|field| field.value).collect()
            }
            Self::Call {
                arguments,
                context_argument,
                ..
            } => arguments
                .iter()
                .map(|argument| argument.value)
                .chain(context_argument.iter().map(|argument| argument.value))
                .collect(),
            Self::Flush { payload: input }
            | Self::FlushBoundary { input }
            | Self::Draining { input }
            | Self::Project { input, .. } => vec![*input],
            Self::Hold {
                initial, updates, ..
            } => std::iter::once(*initial)
                .chain(updates.iter().copied())
                .collect(),
            Self::Latest { branches } => branches.clone(),
            Self::When { input, arms, .. } => std::iter::once(*input)
                .chain(arms.iter().map(|arm| arm.output))
                .collect(),
            Self::Then { input, output } => std::iter::once(*input)
                .chain(output.iter().copied())
                .collect(),
            Self::Infix { left, right, .. } => vec![*left, *right],
            Self::MapEntry { key, value } => vec![*key, *value],
            Self::MatchArm { output, .. } => output.iter().copied().collect(),
            Self::Block { bindings, result } => bindings
                .iter()
                .map(|binding| binding.value)
                .chain(std::iter::once(*result))
                .collect(),
            Self::List { items, .. }
            | Self::Bytes { items, .. }
            | Self::Map { entries: items }
            | Self::Set { items } => items.clone(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticMaterializationResultKind {
    RuntimeValue,
    RenderSlot,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticExecutionImageColumnsV1 {
    pub expressions: Vec<SemanticExpression>,
    pub statements: Vec<SemanticStatement>,
    pub scopes: Vec<SemanticScope>,
    pub callables: Vec<SemanticCallable>,
    pub calls: Vec<SemanticCall>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub call_occurrences: Vec<SemanticCallOccurrence>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<SemanticSourceDef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub states: Vec<SemanticStateDef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub roots: Vec<SemanticRoot>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub functions: Vec<SemanticFunction>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub materializations: Vec<SemanticContextualMaterialization>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub static_owners: Vec<SemanticStaticOwner>,
    /// Exact, dense checked-origin side table. This table is diagnostic and
    /// audit provenance; checked IDs never become semantic execution IDs.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub checked_expression_origins: Vec<SemanticExpressionOrigin>,
}

impl SemanticExecutionImageColumnsV1 {
    pub(crate) fn expression(&self, id: SemanticExprId) -> Result<&SemanticExpression, String> {
        self.expressions
            .get(id.as_usize())
            .filter(|expression| expression.id == id)
            .ok_or_else(|| format!("semantic execution graph references missing expression {id}"))
    }

    fn retained_definition_expressions(&self) -> Result<BTreeSet<SemanticExprId>, String> {
        let mut pending = self
            .callables
            .iter()
            .filter_map(|callable| callable.semantic_root)
            .collect::<Vec<_>>();
        let mut retained = BTreeSet::new();
        while let Some(id) = pending.pop() {
            if !retained.insert(id) {
                continue;
            }
            let expression = self.expression(id)?;
            pending.extend(expression.kind.direct_children());
        }
        Ok(retained)
    }

    fn validate_call_occurrences(&self, out_net: &ResolvedOutGraph) -> Result<(), String> {
        if self.call_occurrences.len() != out_net.call_instances.len() {
            return Err(format!(
                "semantic call occurrence count {} differs from OUT call count {}",
                self.call_occurrences.len(),
                out_net.call_instances.len()
            ));
        }
        for (index, occurrence) in self.call_occurrences.iter().enumerate() {
            let id = OutCallInstanceId(index);
            if occurrence.id != id {
                return Err(format!(
                    "semantic call occurrence {} is noncanonical at index {index}",
                    occurrence.id
                ));
            }
            let out = out_net
                .call_instances
                .get(index)
                .filter(|candidate| candidate.id == id)
                .ok_or_else(|| format!("OUT call occurrence {id} is missing or noncanonical"))?;
            if occurrence.parent != out.parent {
                return Err(format!(
                    "semantic call occurrence {id} parent {:?} differs from OUT parent {:?}",
                    occurrence.parent, out.parent
                ));
            }
            if let Some(parent) = occurrence.parent
                && parent.as_usize() >= index
            {
                return Err(format!(
                    "semantic call occurrence {id} has nonpreceding parent {parent}"
                ));
            }
            let expected_call = match out.provenance.call_id {
                Some(checked_call) => {
                    let mut matches = self
                        .calls
                        .iter()
                        .filter(|call| call.checked_call == checked_call);
                    let call = matches.next().ok_or_else(|| {
                        format!(
                            "semantic call occurrence {id} references absent checked call {}",
                            checked_call.0
                        )
                    })?;
                    if matches.next().is_some() {
                        return Err(format!(
                            "semantic call occurrence {id} checked call {} is ambiguous",
                            checked_call.0
                        ));
                    }
                    let callable = self.callable(call.callable)?;
                    if callable.checked_callable != out.provenance.callable {
                        return Err(format!(
                            "semantic call occurrence {id} callable {} differs from OUT callable {}",
                            callable.checked_callable.0, out.provenance.callable.0
                        ));
                    }
                    Some(call)
                }
                None => None,
            };
            if occurrence.call != expected_call.map(|call| call.id) {
                return Err(format!(
                    "semantic call occurrence {id} call {:?} differs from OUT provenance {:?}",
                    occurrence.call,
                    out.provenance.call_id.map(|call| call.0)
                ));
            }
            let expected_contexts = expected_call
                .into_iter()
                .flat_map(|call| call.contexts.iter().map(|context| context.signature))
                .collect::<Vec<_>>();
            if occurrence.context_ordinals != expected_contexts {
                return Err(format!(
                    "semantic call occurrence {id} contexts differ from its checked call"
                ));
            }
            if occurrence
                .context_ordinals
                .iter()
                .copied()
                .collect::<BTreeSet<_>>()
                .len()
                != occurrence.context_ordinals.len()
            {
                return Err(format!(
                    "semantic call occurrence {id} repeats a context ordinal"
                ));
            }
        }
        Ok(())
    }

    pub(crate) fn statement(&self, id: SemanticStatementId) -> Result<&SemanticStatement, String> {
        self.statements
            .get(id.as_usize())
            .filter(|statement| statement.id == id)
            .ok_or_else(|| format!("semantic execution graph references missing statement {id}"))
    }

    pub(crate) fn callable(&self, id: SemanticCallableId) -> Result<&SemanticCallable, String> {
        self.callables
            .get(id.as_usize())
            .filter(|callable| callable.id == id)
            .ok_or_else(|| format!("semantic execution graph references missing callable {id}"))
    }

    pub(crate) fn call(&self, id: SemanticCallId) -> Result<&SemanticCall, String> {
        self.calls
            .get(id.as_usize())
            .filter(|call| call.id == id)
            .ok_or_else(|| format!("semantic execution graph references missing call {id}"))
    }

    pub(crate) fn source(&self, id: SemanticSourceId) -> Result<&SemanticSourceDef, String> {
        self.sources
            .get(id.as_usize())
            .filter(|source| source.id == id)
            .ok_or_else(|| format!("semantic execution graph references missing source {id}"))
    }

    pub(crate) fn state(&self, id: SemanticStateId) -> Result<&SemanticStateDef, String> {
        self.states
            .get(id.as_usize())
            .filter(|state| state.id == id)
            .ok_or_else(|| format!("semantic execution graph references missing state {id}"))
    }

    pub(crate) fn value(&self, id: SemanticExprId) -> Result<SemanticValueId, String> {
        Ok(self.expression(id)?.value_id)
    }

    pub(crate) fn origin(&self, id: SemanticExprId) -> Result<&SemanticExpressionOrigin, String> {
        self.checked_expression_origins
            .get(id.as_usize())
            .filter(|origin| origin.expression == id)
            .ok_or_else(|| {
                format!("semantic expression {id} has no exact checked-expression origin")
            })
    }

    pub(crate) fn route_scope(&self, id: SemanticExprId) -> Result<SemanticScopeId, String> {
        let origin = self.origin(id)?;
        let matches = self
            .scopes
            .iter()
            .filter(|scope| scope.checked_scope == origin.checked_scope)
            .map(|scope| scope.id)
            .collect::<Vec<_>>();
        let [scope] = matches.as_slice() else {
            return Err(format!(
                "semantic expression {id} checked scope {} resolves to {} semantic scopes",
                origin.checked_scope.0,
                matches.len()
            ));
        };
        Ok(*scope)
    }

    /// Canonical expression edges with materialization roots expanded.
    ///
    /// `None` means a `Materialize` expression references a missing or
    /// non-canonical dense materialization ID. Callers retain ownership of
    /// their phase-specific diagnostic.
    pub(crate) fn expression_children(
        &self,
        kind: &SemanticExpressionKind,
    ) -> Option<Vec<SemanticExprId>> {
        match kind {
            SemanticExpressionKind::Materialize { materialization } => self
                .materializations
                .get(materialization.as_usize())
                .filter(|candidate| candidate.id == *materialization)
                .map(SemanticContextualMaterialization::expression_roots),
            _ => Some(kind.direct_children()),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticStaticOwner {
    pub id: StaticOwnerId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<StaticOwnerId>,
    pub child_ordinal: u32,
}

impl From<&StaticOwnerDef> for SemanticStaticOwner {
    fn from(owner: &StaticOwnerDef) -> Self {
        Self {
            id: owner.id,
            parent: owner.parent,
            child_ordinal: owner.child_ordinal,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticSourceDef {
    pub id: SemanticSourceId,
    pub origin: SemanticSourceOrigin,
    pub declaration: DeclId,
    pub statement: SemanticStatementId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checked_statement: Option<CheckedStatementId>,
    pub expression: SemanticExprId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub call_instance: Option<OutCallInstanceId>,
    pub binding_path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<StaticOwnerId>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SemanticSourceOrigin {
    Checked {
        source: CheckedSourceId,
    },
    ProducerInvocation {
        function: SemanticCallableId,
        producer: ProducerFunctionId,
        identity: [u8; 32],
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticStateDef {
    pub id: SemanticStateId,
    pub checked_state: CheckedStateId,
    pub declaration: DeclId,
    pub statement: SemanticStatementId,
    pub checked_statement: CheckedStatementId,
    pub expression: SemanticExprId,
    pub initial: SemanticExprId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub call_instance: Option<OutCallInstanceId>,
    pub binding_path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<StaticOwnerId>,
    pub lifetime: SemanticStateLifetimeV1,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SemanticStateLifetimeV1 {
    Persistent,
    ActivationLocal { then_expression: SemanticExprId },
}

pub(crate) struct SemanticStateLifetimeDeriverV1 {
    parents: Vec<Vec<(SemanticExprId, bool)>>,
    visit_generation: Vec<usize>,
    generation: usize,
}

impl SemanticStateLifetimeDeriverV1 {
    pub(crate) fn new(expressions: &[SemanticExpression]) -> Result<Self, String> {
        let mut parents = vec![Vec::new(); expressions.len()];
        for expression in expressions {
            for child in expression.kind.direct_children() {
                let Some(child_index) = expressions
                    .get(child.as_usize())
                    .filter(|candidate| candidate.id == child)
                    .map(|_| child.as_usize())
                else {
                    return Err(format!(
                        "semantic expression {} lifetime edge references missing child {child}",
                        expression.id
                    ));
                };
                let then_output = matches!(
                    &expression.kind,
                    SemanticExpressionKind::Then {
                        input,
                        output: Some(output),
                    } if *output == child
                        && !semantic_expression_is_producer_invocation_source(expressions, *input)
                );
                parents[child_index].push((expression.id, then_output));
            }
        }
        for entries in &mut parents {
            entries.sort();
            entries.dedup();
        }
        Ok(Self {
            visit_generation: vec![usize::MAX; expressions.len()],
            parents,
            generation: 0,
        })
    }

    pub(crate) fn derive(
        &mut self,
        state_expression: SemanticExprId,
    ) -> Result<SemanticStateLifetimeV1, String> {
        if self.parents.get(state_expression.as_usize()).is_none() {
            return Err(format!(
                "semantic state lifetime references missing expression {state_expression}"
            ));
        }
        if self.generation == usize::MAX {
            self.visit_generation.fill(usize::MAX);
            self.generation = 0;
        }
        let generation = self.generation;
        self.generation += 1;

        let mut pending = VecDeque::from([(state_expression, 0usize)]);
        let mut nearest_distance = None;
        let mut nearest = Vec::new();
        while let Some((expression, distance)) = pending.pop_front() {
            if nearest_distance.is_some_and(|nearest| distance >= nearest) {
                continue;
            }
            let expression_index = expression.as_usize();
            if self.visit_generation[expression_index] == generation {
                continue;
            }
            self.visit_generation[expression_index] = generation;
            for (parent, then_output) in &self.parents[expression_index] {
                let parent_distance = distance + 1;
                if *then_output {
                    match nearest_distance {
                        None => {
                            nearest_distance = Some(parent_distance);
                            nearest.push(*parent);
                        }
                        Some(nearest_distance) if parent_distance == nearest_distance => {
                            nearest.push(*parent);
                        }
                        Some(_) => {}
                    }
                } else {
                    pending.push_back((*parent, parent_distance));
                }
            }
        }
        nearest.sort();
        nearest.dedup();
        match nearest.as_slice() {
            [] => Ok(SemanticStateLifetimeV1::Persistent),
            [then_expression] => Ok(SemanticStateLifetimeV1::ActivationLocal {
                then_expression: *then_expression,
            }),
            candidates => Err(format!(
                "semantic state expression {state_expression} belongs to {} equally-near THEN activation sites: {candidates:?}",
                candidates.len()
            )),
        }
    }
}

fn semantic_expression_is_producer_invocation_source(
    expressions: &[SemanticExpression],
    expression: SemanticExprId,
) -> bool {
    // Invocation-mode producer expansion wraps the function body in a
    // synthetic THEN so the remote call has an exact private source route.
    // That wrapper is transport, not a source-language state-cell lifetime:
    // producer HOLD authority remains live for its process-local call-site
    // lease and is excluded from global persistence separately.
    expressions
        .get(expression.as_usize())
        .filter(|candidate| candidate.id == expression)
        .is_some_and(|expression| {
            matches!(expression.kind, SemanticExpressionKind::Source { .. })
                && !expression.provenance.members.is_empty()
                && expression.provenance.members.iter().all(|member| {
                    member.path.is_empty()
                        && matches!(member.origin, SemanticValueOrigin::ProducerSource { .. })
                })
        })
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticRoot {
    pub ordinal: usize,
    pub kind: SemanticRootKindV1,
    pub declaration: DeclId,
    pub checked_statement: CheckedStatementId,
    pub statement: SemanticStatementId,
    pub checked_expr_id: CheckedExprId,
    pub expression: SemanticExprId,
    pub value: SemanticValueId,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticRootKindV1 {
    RetainedVisualDocument,
    RetainedVisualScene,
    HostValue,
}

/// Shared checked authority for the exact execution/output-root boundary.
///
/// The diagnostic spelling and host type stay out of [`SemanticRoot`], whose
/// identity is structural. Lowering C consumes these presentation/type fields
/// only after revalidating the root's exact checked and semantic coordinates.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CheckedSemanticRootSpecV1 {
    pub kind: SemanticRootKindV1,
    pub root: String,
    pub value_path: String,
    pub data_type: Option<Type>,
    pub declaration: DeclId,
    pub checked_statement: CheckedStatementId,
    pub checked_expression: CheckedExprId,
    pub line: usize,
}

pub(crate) fn checked_semantic_root_specs_v1(
    checked: &CheckedProgramFields,
) -> Result<Vec<CheckedSemanticRootSpecV1>, String> {
    let require_statement = |id: CheckedStatementId| {
        let matches = checked
            .statements
            .iter()
            .filter(|statement| statement.id == id)
            .collect::<Vec<_>>();
        match matches.as_slice() {
            [statement] => Ok(*statement),
            _ => Err(format!(
                "checked output-root statement {} resolves to {} exact statements",
                id.0,
                matches.len()
            )),
        }
    };
    let require_declaration = |id: DeclId| {
        let matches = checked
            .declarations
            .iter()
            .filter(|declaration| declaration.id == id)
            .collect::<Vec<_>>();
        match matches.as_slice() {
            [declaration] => Ok(*declaration),
            _ => Err(format!(
                "checked output-root declaration {} resolves to {} exact declarations",
                id.0,
                matches.len()
            )),
        }
    };

    let mut consumed_output_types = BTreeSet::new();
    let mut roots = Vec::new();
    for statement in checked
        .statements
        .iter()
        .filter(|statement| statement.scope_id == checked.root_scope)
    {
        let declaration = match statement.kind {
            CheckedStatementKind::Field { declaration }
            | CheckedStatementKind::List {
                declaration: Some(declaration),
                ..
            } => declaration,
            CheckedStatementKind::Function { .. }
            | CheckedStatementKind::Source { .. }
            | CheckedStatementKind::Hold { .. }
            | CheckedStatementKind::List {
                declaration: None, ..
            }
            | CheckedStatementKind::Block
            | CheckedStatementKind::Spread
            | CheckedStatementKind::Expression => continue,
        };
        let root_declaration = require_declaration(declaration)?;
        let retained_kind = match root_declaration.name.as_str() {
            "document" => Some(SemanticRootKindV1::RetainedVisualDocument),
            "scene" => Some(SemanticRootKindV1::RetainedVisualScene),
            _ => None,
        };
        if let Some(kind) = retained_kind {
            let checked_expression = statement.value.ok_or_else(|| {
                format!(
                    "retained output `{}` statement {} has no checked value",
                    root_declaration.name, statement.id.0
                )
            })?;
            roots.push(CheckedSemanticRootSpecV1 {
                kind,
                root: root_declaration.name.clone(),
                value_path: root_declaration.name.clone(),
                data_type: None,
                declaration,
                checked_statement: statement.id,
                checked_expression,
                line: statement.span.line,
            });
            continue;
        }
        if root_declaration.name != "outputs" {
            continue;
        }
        for output_id in &statement.children {
            let output = require_statement(*output_id)?;
            let output_declaration = match output.kind {
                CheckedStatementKind::Field { declaration } => declaration,
                CheckedStatementKind::List {
                    declaration: Some(declaration),
                    ..
                } => declaration,
                CheckedStatementKind::List {
                    declaration: None, ..
                } => {
                    return Err(format!(
                        "output list statement {} has no checked declaration",
                        output.id.0
                    ));
                }
                CheckedStatementKind::Function { .. }
                | CheckedStatementKind::Source { .. }
                | CheckedStatementKind::Hold { .. }
                | CheckedStatementKind::Block
                | CheckedStatementKind::Spread
                | CheckedStatementKind::Expression => continue,
            };
            let declaration = require_declaration(output_declaration)?;
            let matches = checked
                .lowering_metadata
                .output_root_types
                .iter()
                .enumerate()
                .filter(|(_, entry)| {
                    entry.statement == output.id && entry.declaration == output_declaration
                })
                .collect::<Vec<_>>();
            let [(type_index, output_type)] = matches.as_slice() else {
                return Err(format!(
                    "output `{}` statement {} resolves to {} exact checked output types",
                    declaration.name,
                    output.id.0,
                    matches.len()
                ));
            };
            if !consumed_output_types.insert(*type_index) {
                return Err(format!(
                    "checked output type {type_index} is consumed more than once"
                ));
            }
            if output_type.name != declaration.name || output_type.value != output.value {
                return Err(format!(
                    "output `{}` type entry value {:?} differs from checked statement value {:?}",
                    declaration.name, output_type.value, output.value
                ));
            }
            let checked_expression = output.value.ok_or_else(|| {
                format!(
                    "host output `{}` statement {} has no checked value",
                    declaration.name, output.id.0
                )
            })?;
            roots.push(CheckedSemanticRootSpecV1 {
                kind: SemanticRootKindV1::HostValue,
                root: declaration.name.clone(),
                value_path: format!("outputs.{}", declaration.name),
                data_type: Some(output_type.ty.clone()),
                declaration: output_declaration,
                checked_statement: output.id,
                checked_expression,
                line: output.span.line,
            });
        }
    }
    if consumed_output_types.len() != checked.lowering_metadata.output_root_types.len() {
        return Err(format!(
            "{} checked output-root types were not consumed by exact output declarations",
            checked.lowering_metadata.output_root_types.len() - consumed_output_types.len()
        ));
    }
    roots.sort_by_key(|root| {
        (
            root.checked_statement,
            root.declaration,
            root.checked_expression,
            root.kind,
        )
    });
    let mut root_names = BTreeSet::new();
    for root in &roots {
        if !root_names.insert(root.root.as_str()) {
            return Err(format!(
                "output root `{}` is declared more than once",
                root.root
            ));
        }
    }
    Ok(roots)
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticFunctionParameter {
    pub id: SemanticParameterId,
    pub formal: DeclId,
    pub name: String,
    pub flow_type: FlowType,
    pub requirement: CheckedParameterRequirement,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub input_expressions: Vec<SemanticExprId>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticFunction {
    pub producer: ProducerFunctionId,
    pub callable: SemanticCallableId,
    pub identity: [u8; 32],
    pub name: String,
    pub parameters: Vec<SemanticFunctionParameter>,
    pub result_type: FlowType,
    pub root: SemanticExprId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invocation_source: Option<SemanticExprId>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticStatement {
    pub id: SemanticStatementId,
    pub origin: SemanticStatementOrigin,
    pub scope: SemanticScopeId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<SemanticStatementId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub call_instance: Option<OutCallInstanceId>,
    pub span: CheckedSpan,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub checked_resources: Vec<CheckedResourceBinding>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub declaration: Option<DeclId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flow_type: Option<FlowType>,
    pub kind: SemanticStatementKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<SemanticExprId>,
    pub value_use: SemanticMaterializationResultKind,
    pub children: Vec<SemanticStatementId>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SemanticStatementOrigin {
    Checked {
        statement: CheckedStatementId,
    },
    ProducerResult {
        identity: [u8; 32],
        function: ProducerFunctionId,
        callable: DeclId,
        root_call: OutCallInstanceId,
        result_statement: ProducerResultStatementId,
        checked_statement: CheckedStatementId,
        checked_result_expression: CheckedExprId,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticScope {
    pub id: SemanticScopeId,
    pub checked_scope: LexicalScopeId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<SemanticScopeId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<DeclId>,
    pub kind: CheckedScopeKind,
    pub span: CheckedSpan,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SemanticStatementKind {
    Field {
        name: String,
        path: String,
    },
    Source {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        path: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        event: Option<String>,
    },
    Hold {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        path: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        hold_name: Option<String>,
    },
    List {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        path: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        capacity: Option<usize>,
    },
    Block,
    Spread,
    Expression,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct SemanticRowBinding {
    pub list: SemanticListId,
    pub scope: SemanticRowScopeId,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SemanticContextualRowPredecessor {
    Value,
    Stored {
        row: SemanticRowBinding,
    },
    Materialized {
        materialization: SemanticMaterializationId,
    },
    Provenance {
        materialization: SemanticMaterializationId,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticContextualOperationKind {
    Map,
    Filter,
    Retain,
    Remove,
    Every,
    Any,
    Find,
    SortBy,
    ThenBy,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticContextualMaterialization {
    pub id: SemanticMaterializationId,
    pub operation: SemanticContextualOperationKind,
    pub source: SemanticExprId,
    pub body: SemanticExprId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub direction: Option<SemanticExprId>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub inherited_order: Vec<SemanticContextualOrderKey>,
    pub result_kind: SemanticMaterializationResultKind,
    pub row_local: SemanticMaterializationLocalId,
    pub owner: StaticOwnerId,
    pub item_type: Type,
    pub result_type: Type,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticContextualOrderKey {
    pub operation: SemanticContextualOperationKind,
    pub body: SemanticExprId,
    pub direction: SemanticExprId,
}

impl SemanticContextualMaterialization {
    pub fn expression_roots(&self) -> Vec<SemanticExprId> {
        let mut roots = Vec::with_capacity(3 + self.inherited_order.len() * 2);
        roots.push(self.source);
        roots.push(self.body);
        roots.extend(self.direction);
        for key in &self.inherited_order {
            roots.push(key.body);
            roots.push(key.direction);
        }
        roots
    }
}

impl SemanticExecutionImageColumnsV1 {
    pub fn validate(&self, out_net: &ResolvedOutGraph) -> Result<(), String> {
        self.validate_static_owners(out_net)?;
        self.validate_dense_ids()?;
        self.validate_callable_and_call_tables()?;
        self.validate_checked_expression_origins(out_net)?;
        self.validate_call_occurrences(out_net)?;
        let retained_definition_expressions = self.retained_definition_expressions()?;

        let materialization_locals = self
            .materializations
            .iter()
            .map(|materialization| (materialization.owner, materialization.row_local))
            .collect::<BTreeSet<_>>();
        if materialization_locals.len() != self.materializations.len() {
            return Err(
                "semantic contextual materializations define duplicate owner-local identities"
                    .to_owned(),
            );
        }
        self.validate_materialization_locals_are_dense(&materialization_locals)?;

        let mut local_definitions = BTreeMap::new();
        let mut local_references = BTreeSet::new();
        for expression in &self.expressions {
            self.validate_owner(expression.owner, format!("expression {}", expression.id))?;
            self.validate_provenance(&expression.provenance, &materialization_locals)?;
            self.validate_expression_kind(
                expression.id,
                &expression.kind,
                out_net,
                &materialization_locals,
                retained_definition_expressions.contains(&expression.id),
                &mut local_definitions,
                &mut local_references,
            )?;
        }
        validate_dense_local_bindings(&local_definitions)?;
        for binding in local_references {
            if !local_definitions.contains_key(&binding) {
                return Err(format!(
                    "semantic local read references missing binding {binding}"
                ));
            }
        }

        for statement in &self.statements {
            self.require_scope(statement.scope, format!("statement {}", statement.id))?;
            if let Some(parent) = statement.parent {
                self.require_statement(parent, format!("statement {} parent", statement.id))?;
            }
            if let Some(call) = statement.call_instance {
                validate_call_instance(
                    out_net,
                    call,
                    &format!("statement {} call instance", statement.id),
                )?;
            }
            if let Some(value) = statement.value {
                self.require_expression(value, format!("statement {} value", statement.id))?;
            }
            for child in &statement.children {
                self.require_statement(*child, format!("statement {} child", statement.id))?;
            }
        }
        for source in &self.sources {
            self.require_expression(
                source.expression,
                format!("source {} expression", source.id),
            )?;
            self.validate_owner(source.owner, format!("source {}", source.id))?;
            self.require_statement(source.statement, format!("source {} statement", source.id))?;
            if let Some(call) = source.call_instance {
                validate_call_instance(
                    out_net,
                    call,
                    &format!("source {} call instance", source.id),
                )?;
            }
            if let SemanticSourceOrigin::ProducerInvocation {
                function,
                producer,
                identity,
            } = source.origin
            {
                self.require_callable(function, format!("source {} callable", source.id))?;
                let semantic_function = self.require_producer_function(
                    producer,
                    format!("source {} producer function", source.id),
                )?;
                if semantic_function.callable != function {
                    return Err(format!(
                        "source {} producer {} does not match callable {}",
                        source.id, producer, function
                    ));
                }
                if semantic_function.identity != identity {
                    return Err(format!(
                        "source {} producer identity does not match function {}",
                        source.id, function
                    ));
                }
            }
        }
        let mut state_lifetime_deriver = SemanticStateLifetimeDeriverV1::new(&self.expressions)?;
        for state in &self.states {
            self.require_expression(state.expression, format!("state {} expression", state.id))?;
            self.require_expression(state.initial, format!("state {} initial value", state.id))?;
            self.validate_owner(state.owner, format!("state {}", state.id))?;
            self.require_statement(state.statement, format!("state {} statement", state.id))?;
            let expected_lifetime = state_lifetime_deriver.derive(state.expression)?;
            if state.lifetime != expected_lifetime {
                return Err(format!(
                    "semantic state {} lifetime {:?} differs from derived lifetime {:?}",
                    state.id, state.lifetime, expected_lifetime
                ));
            }
            if let Some(call) = state.call_instance {
                validate_call_instance(
                    out_net,
                    call,
                    &format!("state {} call instance", state.id),
                )?;
            }
        }
        let mut root_statements = BTreeSet::new();
        let mut root_values = BTreeSet::new();
        for (index, root) in self.roots.iter().enumerate() {
            if root.ordinal != index {
                return Err(format!(
                    "semantic execution root at index {index} has ordinal {}",
                    root.ordinal
                ));
            }
            let statement =
                self.require_statement(root.statement, format!("root {index} statement"))?;
            if statement.origin
                != (SemanticStatementOrigin::Checked {
                    statement: root.checked_statement,
                })
            {
                return Err(format!(
                    "semantic execution root {index} statement {} does not have exact checked origin {}",
                    root.statement, root.checked_statement.0
                ));
            }
            if statement.declaration != Some(root.declaration)
                || statement.call_instance.is_some()
                || statement.value != Some(root.expression)
            {
                return Err(format!(
                    "semantic execution root {index} does not match semantic statement {} declaration/value",
                    root.statement
                ));
            }
            let expression =
                self.require_expression(root.expression, format!("root {index} expression"))?;
            if expression.checked_expr_id != root.checked_expr_id
                || expression.value_id != root.value
            {
                return Err(format!(
                    "semantic execution root {index} does not match expression {} checked/value identity",
                    root.expression
                ));
            }
            let origin = self
                .checked_expression_origins
                .get(root.expression.as_usize())
                .filter(|origin| origin.expression == root.expression)
                .ok_or_else(|| {
                    format!(
                        "semantic execution root {index} expression {} has no exact checked origin",
                        root.expression
                    )
                })?;
            if origin.checked_expression != root.checked_expr_id
                || origin.owning_statement != Some(root.statement)
            {
                return Err(format!(
                    "semantic execution root {index} expression {} has mismatched checked/statement origin",
                    root.expression
                ));
            }
            if !root_statements.insert(root.statement) {
                return Err(format!(
                    "semantic execution statement {} is bound by more than one root",
                    root.statement
                ));
            }
            if !root_values.insert(root.value) {
                return Err(format!(
                    "semantic execution value {} is bound by more than one root",
                    root.value
                ));
            }
        }
        self.validate_functions(out_net)?;
        self.validate_materializations(&materialization_locals)?;
        Ok(())
    }

    pub(crate) fn validate_checked_roots(
        &self,
        checked: &CheckedProgramFields,
    ) -> Result<(), String> {
        let expected = checked_semantic_root_specs_v1(checked)?;
        if self.roots.len() != expected.len() {
            return Err(format!(
                "semantic execution roots contain {} entries but exact checked output-root inventory contains {}",
                self.roots.len(),
                expected.len()
            ));
        }
        for (index, (root, expected)) in self.roots.iter().zip(&expected).enumerate() {
            if root.ordinal != index
                || root.kind != expected.kind
                || root.declaration != expected.declaration
                || root.checked_statement != expected.checked_statement
            {
                return Err(format!(
                    "semantic execution root {index} differs from its exact checked output-root identity"
                ));
            }
            let statement =
                self.require_statement(root.statement, format!("root {index} statement"))?;
            if statement.origin
                != (SemanticStatementOrigin::Checked {
                    statement: expected.checked_statement,
                })
                || statement.declaration != Some(expected.declaration)
                || statement.call_instance.is_some()
                || statement.value != Some(root.expression)
            {
                return Err(format!(
                    "semantic execution root {index} is not owned by exact semantic output statement {}",
                    root.statement
                ));
            }
            let expression =
                self.require_expression(root.expression, format!("root {index} expression"))?;
            if expression.checked_expr_id != root.checked_expr_id
                || expression.value_id != root.value
            {
                return Err(format!(
                    "semantic execution root {index} checked/value identity differs from semantic expression {}",
                    root.expression
                ));
            }
            let origin = self
                .checked_expression_origins
                .get(root.expression.as_usize())
                .filter(|origin| origin.expression == root.expression)
                .ok_or_else(|| {
                    format!(
                        "semantic execution root {index} expression {} has no exact checked origin",
                        root.expression
                    )
                })?;
            if origin.checked_expression != root.checked_expr_id
                || origin.owning_statement != Some(root.statement)
            {
                return Err(format!(
                    "semantic execution root {index} expression {} has mismatched checked/statement origin",
                    root.expression
                ));
            }
        }
        Ok(())
    }

    fn validate_static_owners(&self, out_net: &ResolvedOutGraph) -> Result<(), String> {
        if self.static_owners.len() != out_net.static_owners.len() {
            return Err(format!(
                "semantic static owner count {} does not match resolved OUT owner count {}",
                self.static_owners.len(),
                out_net.static_owners.len()
            ));
        }
        for (index, owner) in self.static_owners.iter().enumerate() {
            let expected_id = StaticOwnerId(index);
            if owner.id != expected_id {
                return Err(format!(
                    "semantic static owner at index {index} has non-dense ID {}",
                    owner.id
                ));
            }
            if let Some(parent) = owner.parent
                && parent.as_usize() >= index
            {
                return Err(format!(
                    "semantic static owner {} has non-preceding parent {parent}",
                    owner.id
                ));
            }
            let resolved = &out_net.static_owners[index];
            if owner.id != resolved.id
                || owner.parent != resolved.parent
                || owner.child_ordinal != resolved.child_ordinal
            {
                return Err(format!(
                    "semantic static owner {} does not match its resolved OUT definition",
                    owner.id
                ));
            }
        }
        Ok(())
    }

    fn validate_dense_ids(&self) -> Result<(), String> {
        for (index, expression) in self.expressions.iter().enumerate() {
            if expression.id != SemanticExprId(index) {
                return Err(format!(
                    "semantic expression at index {index} has non-dense ID {}",
                    expression.id
                ));
            }
            if expression.value_id != SemanticValueId(index) {
                return Err(format!(
                    "semantic expression {} has non-dense value ID {}",
                    expression.id, expression.value_id
                ));
            }
        }
        for (index, statement) in self.statements.iter().enumerate() {
            if statement.id != SemanticStatementId(index) {
                return Err(format!(
                    "semantic statement at index {index} has non-dense ID {}",
                    statement.id
                ));
            }
        }
        for (index, scope) in self.scopes.iter().enumerate() {
            if scope.id != SemanticScopeId(index) {
                return Err(format!(
                    "semantic scope at index {index} has non-dense ID {}",
                    scope.id
                ));
            }
        }
        for (index, callable) in self.callables.iter().enumerate() {
            if callable.id != SemanticCallableId(index) {
                return Err(format!(
                    "semantic callable at index {index} has non-dense ID {}",
                    callable.id
                ));
            }
        }
        for (index, call) in self.calls.iter().enumerate() {
            if call.id != SemanticCallId(index) {
                return Err(format!(
                    "semantic call at index {index} has non-dense ID {}",
                    call.id
                ));
            }
        }
        for (index, source) in self.sources.iter().enumerate() {
            if source.id != SemanticSourceId(index) {
                return Err(format!(
                    "semantic source at index {index} has non-dense ID {}",
                    source.id
                ));
            }
        }
        for (index, state) in self.states.iter().enumerate() {
            if state.id != SemanticStateId(index) {
                return Err(format!(
                    "semantic state at index {index} has non-dense ID {}",
                    state.id
                ));
            }
        }
        for (index, function) in self.functions.iter().enumerate() {
            if function.producer != ProducerFunctionId(index) {
                return Err(format!(
                    "semantic producer function at index {index} has non-dense producer ID {}",
                    function.producer
                ));
            }
        }
        for (index, materialization) in self.materializations.iter().enumerate() {
            if materialization.id != SemanticMaterializationId(index) {
                return Err(format!(
                    "semantic materialization at index {index} has non-dense ID {}",
                    materialization.id
                ));
            }
        }
        Ok(())
    }

    fn validate_checked_expression_origins(
        &self,
        out_net: &ResolvedOutGraph,
    ) -> Result<(), String> {
        if self.checked_expression_origins.len() != self.expressions.len() {
            return Err(format!(
                "checked-expression origin count {} does not exactly cover {} semantic expressions",
                self.checked_expression_origins.len(),
                self.expressions.len()
            ));
        }
        for (index, origin) in self.checked_expression_origins.iter().enumerate() {
            let expression_id = SemanticExprId(index);
            if origin.expression != expression_id {
                return Err(format!(
                    "checked-expression origin at index {index} covers {}, expected {expression_id}",
                    origin.expression
                ));
            }
            let expression = &self.expressions[index];
            if origin.checked_expression != expression.checked_expr_id {
                return Err(format!(
                    "checked-expression origin for {} does not match expression checked ID",
                    origin.expression
                ));
            }
            if !self
                .scopes
                .iter()
                .any(|scope| scope.checked_scope == origin.checked_scope)
            {
                return Err(format!(
                    "checked-expression origin for {} references missing checked scope {}",
                    origin.expression, origin.checked_scope.0
                ));
            }
            if let Some(statement) = origin.owning_statement {
                self.require_statement(statement, "checked-expression origin")?;
            }
            if let Some(call) = origin.call_instance {
                validate_call_instance(out_net, call, "checked-expression origin")?;
            }
        }
        Ok(())
    }

    fn validate_callable_and_call_tables(&self) -> Result<(), String> {
        let mut checked_callables = BTreeSet::new();
        let mut semantic_roots = BTreeSet::new();
        for callable in &self.callables {
            if !checked_callables.insert(callable.checked_callable) {
                return Err(format!(
                    "semantic callable {} duplicates checked callable {}",
                    callable.id, callable.checked_callable.0
                ));
            }
            self.require_scope(callable.scope, format!("callable {}", callable.id))?;
            for (index, parameter) in callable.parameters.iter().enumerate() {
                let expected = SemanticParameterId {
                    callable: callable.id,
                    ordinal: parameter.ordinal,
                };
                if parameter.id != expected || parameter.ordinal != index {
                    return Err(format!(
                        "semantic callable {} parameter at index {index} has noncanonical identity {:?}",
                        callable.id, parameter.id
                    ));
                }
                if let CheckedParameterRequirement::Optional {
                    default: boon_checked::CheckedParameterDefault::CallableProfile { profile },
                } = &parameter.requirement
                    && profile.is_empty()
                {
                    return Err(format!(
                        "semantic callable {} parameter {} has an empty default profile",
                        callable.id, parameter.name
                    ));
                }
            }
            if let Some(root) = callable.semantic_root {
                if callable.kind != CheckedCallableKind::User
                    || callable.effect != CheckedEffectSummary::default()
                    || !callable.contexts.is_empty()
                    || callable.contextual_operation.is_some()
                    || callable.parameters.iter().any(|parameter| {
                        parameter.kind != CheckedParameterKind::Value
                            || !matches!(
                                parameter.requirement,
                                CheckedParameterRequirement::Required
                            )
                    })
                {
                    return Err(format!(
                        "semantic callable {} retains a body across a specialization-sensitive boundary",
                        callable.id
                    ));
                }
                match (&callable.context_formal, &callable.context_parameter) {
                    (None, None) => {}
                    (Some(formal), Some(parameter))
                        if parameter.formal == *formal
                            && parameter.id
                                == (SemanticParameterId {
                                    callable: callable.id,
                                    ordinal: callable.parameters.len(),
                                }) => {}
                    _ => {
                        return Err(format!(
                            "semantic callable {} has a stale retained PASSED parameter",
                            callable.id
                        ));
                    }
                }
                self.require_expression(root, format!("callable {} semantic root", callable.id))?;
                if !semantic_roots.insert(root) {
                    return Err(format!(
                        "semantic callable {} shares semantic root {root} with another callable",
                        callable.id
                    ));
                }
            }
        }

        let mut checked_calls = BTreeSet::new();
        for call in &self.calls {
            if !checked_calls.insert(call.checked_call) {
                return Err(format!(
                    "semantic call {} duplicates checked call {}",
                    call.id, call.checked_call.0
                ));
            }
            let callable = self.require_callable(call.callable, format!("call {}", call.id))?;
            if call.effect != callable.effect {
                return Err(format!(
                    "semantic call {} effect differs from callable {}",
                    call.id, callable.id
                ));
            }
            if let Some(owner) = call.owner_callable {
                self.require_callable(owner, format!("call {} owner", call.id))?;
            }
            if call.occurrence_segment.is_empty() {
                return Err(format!(
                    "semantic call {} has an empty occurrence segment",
                    call.id
                ));
            }
            let mut formals = BTreeSet::new();
            for entry in &call.entries {
                let (formal, ordinal, name) = match entry {
                    SemanticCallEntry::Input {
                        formal,
                        ordinal,
                        name,
                        requirement,
                        ..
                    } => {
                        let parameter = callable.parameters.get(*ordinal).ok_or_else(|| {
                            format!(
                                "semantic call {} references missing input ordinal {ordinal}",
                                call.id
                            )
                        })?;
                        if parameter.requirement != *requirement {
                            return Err(format!(
                                "semantic call {} input {formal:?} has stale requirement/default",
                                call.id
                            ));
                        }
                        (*formal, *ordinal, name)
                    }
                    SemanticCallEntry::FreshOut {
                        formal,
                        ordinal,
                        name,
                        scope,
                        ..
                    } => {
                        self.require_scope(*scope, format!("call {} OUT scope", call.id))?;
                        (*formal, *ordinal, name)
                    }
                    SemanticCallEntry::ForwardOut {
                        formal,
                        ordinal,
                        name,
                        ..
                    } => (*formal, *ordinal, name),
                };
                if !formals.insert(formal) {
                    return Err(format!(
                        "semantic call {} binds formal {} more than once",
                        call.id, formal.0
                    ));
                }
                let parameter = callable.parameters.get(ordinal).ok_or_else(|| {
                    format!(
                        "semantic call {} references missing parameter ordinal {ordinal}",
                        call.id
                    )
                })?;
                if parameter.formal != formal || parameter.name != *name {
                    return Err(format!(
                        "semantic call {} binding does not match callable {} parameter {ordinal}",
                        call.id, callable.id
                    ));
                }
            }
            for context in &call.contexts {
                self.require_scope(context.scope, format!("call {} context", call.id))?;
            }
        }
        Ok(())
    }

    fn validate_materialization_locals_are_dense(
        &self,
        locals: &BTreeSet<(StaticOwnerId, SemanticMaterializationLocalId)>,
    ) -> Result<(), String> {
        let mut by_owner = BTreeMap::<StaticOwnerId, Vec<SemanticMaterializationLocalId>>::new();
        for (owner, local) in locals {
            by_owner.entry(*owner).or_default().push(*local);
        }
        for (owner, owner_locals) in by_owner {
            for (index, local) in owner_locals.into_iter().enumerate() {
                if local != SemanticMaterializationLocalId(index) {
                    return Err(format!(
                        "semantic materialization owner {owner} has non-dense local ID {local}"
                    ));
                }
            }
        }
        Ok(())
    }

    fn validate_provenance(
        &self,
        provenance: &SemanticValueProvenance,
        materialization_locals: &BTreeSet<(StaticOwnerId, SemanticMaterializationLocalId)>,
    ) -> Result<(), String> {
        for member in &provenance.members {
            match &member.origin {
                SemanticValueOrigin::Runtime => {}
                SemanticValueOrigin::Source { source, owner } => {
                    self.require_source(*source, "value provenance source")?;
                    self.validate_owner(*owner, "value provenance source")?;
                }
                SemanticValueOrigin::ProducerSource {
                    function,
                    producer,
                    identity,
                    owner,
                } => {
                    self.require_callable(*function, "value provenance producer callable")?;
                    let semantic_function = self.require_producer_function(
                        *producer,
                        "value provenance producer function",
                    )?;
                    if semantic_function.callable != *function
                        || semantic_function.identity != *identity
                    {
                        return Err(format!(
                            "value provenance producer does not match function {function}"
                        ));
                    }
                    self.validate_owner(Some(*owner), "value provenance producer")?;
                }
                SemanticValueOrigin::State { state, owner } => {
                    self.require_state(*state, "value provenance state")?;
                    self.validate_owner(*owner, "value provenance state")?;
                }
                SemanticValueOrigin::MaterializationLocal { owner, local, .. } => {
                    self.validate_materialization_local(
                        *owner,
                        *local,
                        materialization_locals,
                        "value provenance",
                    )?;
                }
            }
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn validate_expression_kind(
        &self,
        expression: SemanticExprId,
        kind: &SemanticExpressionKind,
        out_net: &ResolvedOutGraph,
        materialization_locals: &BTreeSet<(StaticOwnerId, SemanticMaterializationLocalId)>,
        retained_definition_expression: bool,
        local_definitions: &mut BTreeMap<SemanticLocalBindingId, (DeclId, SemanticExprId)>,
        local_references: &mut BTreeSet<SemanticLocalBindingId>,
    ) -> Result<(), String> {
        let context = |field: &str| format!("expression {expression} {field}");
        match kind {
            SemanticExpressionKind::CanonicalRead { source, .. } => {
                if let Some(source) = source {
                    self.require_source(source.source, context("source read"))?;
                }
            }
            SemanticExpressionKind::LocalRead { binding, .. } => {
                local_references.insert(*binding);
            }
            SemanticExpressionKind::ElementState { context: call, .. } => {
                validate_call_context(out_net, *call, &context("element-state context"))?;
            }
            SemanticExpressionKind::TextTemplate { segments } => {
                for segment in segments {
                    if let SemanticTextSegment::Dynamic { value } = segment {
                        self.require_expression(*value, context("text segment"))?;
                    }
                }
            }
            SemanticExpressionKind::TaggedObject { fields, .. }
            | SemanticExpressionKind::Object(fields) => {
                for field in fields {
                    self.require_expression(field.value, context("record field"))?;
                }
            }
            SemanticExpressionKind::Call {
                call,
                callable,
                callable_kind,
                name,
                function,
                intrinsic,
                role,
                effect,
                result,
                instance,
                arguments,
                parameter_bindings,
                context_argument,
                contexts,
            } => {
                if let Some(instance) = instance {
                    validate_call_instance(out_net, *instance, &context("call instance"))?;
                }
                let call_definition = self.require_call(*call, context("call definition"))?;
                let callable_definition =
                    self.require_callable(*callable, context("callable definition"))?;
                let expected_kind = match callable_definition.kind {
                    CheckedCallableKind::User if callable_definition.semantic_root.is_some() => {
                        SemanticCallableKind::User
                    }
                    CheckedCallableKind::Builtin => SemanticCallableKind::Builtin,
                    CheckedCallableKind::External => SemanticCallableKind::External,
                    CheckedCallableKind::User => {
                        return Err(format!(
                            "expression {expression} retains a user call without a semantic callable body"
                        ));
                    }
                };
                let resolved_instance =
                    instance.map(|instance| &out_net.call_instances[instance.as_usize()]);
                let expected_result = resolved_instance
                    .map(|instance| &instance.result)
                    .unwrap_or(&call_definition.result);
                let expected_flow_type = FlowType {
                    mode: expected_result.mode,
                    ty: crate::contextual_expansion::erase_runtime_type_vars(&expected_result.ty),
                };
                let expression_definition =
                    self.require_expression(expression, context("call expression"))?;
                if call_definition.callable != *callable
                    || *callable_kind != expected_kind
                    || name != &callable_definition.name
                    || function != &call_definition.function
                    || *intrinsic != call_definition.intrinsic
                    || *role != call_definition.role
                    || *effect != callable_definition.effect
                    || result != &call_definition.result
                    || expression_definition.checked_expr_id != call_definition.checked_expression
                    || expression_definition.effect != *effect
                    || expression_definition.flow_type != expected_flow_type
                {
                    return Err(format!(
                        "expression {expression} call contract differs from semantic call {call}: \
                         callable={callable:?}/{:?}, kind={callable_kind:?}/{expected_kind:?}, \
                         name={name:?}/{:?}, function={function:?}/{:?}, \
                         intrinsic={intrinsic:?}/{:?}, role={role:?}/{:?}, \
                         callable effect={effect:?}/{:?}, result={result:?}/{:?}, \
                         checked={:?}/{:?}, expression effect={:?}, \
                         expression flow={:?}, resolved instance flow={expected_flow_type:?}",
                        call_definition.callable,
                        callable_definition.name,
                        call_definition.function,
                        call_definition.intrinsic,
                        call_definition.role,
                        callable_definition.effect,
                        call_definition.result,
                        expression_definition.checked_expr_id,
                        call_definition.checked_expression,
                        expression_definition.effect,
                        expression_definition.flow_type,
                    ));
                }
                if let Some(resolved_instance) = resolved_instance {
                    if resolved_instance.provenance.call_id != Some(call_definition.checked_call)
                        || resolved_instance.provenance.callable
                            != callable_definition.checked_callable
                    {
                        return Err(format!(
                            "expression {expression} call instance {instance:?} has stale checked provenance"
                        ));
                    }
                } else {
                    let retained_context_overlay = retained_definition_expression
                        && contexts.is_empty()
                        && !call_definition.contexts.is_empty()
                        && *effect == CheckedEffectSummary::default()
                        && *callable_kind == SemanticCallableKind::Builtin
                        && boon_checked::is_registered_render_constructor(
                            &call_definition.function,
                        );
                    if (!contexts.is_empty()
                        || !call_definition.contexts.is_empty()
                        || *effect != CheckedEffectSummary::default()
                        || *callable_kind == SemanticCallableKind::External)
                        && !retained_context_overlay
                    {
                        return Err(format!(
                            "expression {expression} omits its OUT instance for a contextual, effectful, or external call"
                        ));
                    }
                }
                let value_parameters = callable_definition
                    .parameters
                    .iter()
                    .filter(|parameter| parameter.kind == CheckedParameterKind::Value)
                    .collect::<Vec<_>>();
                if parameter_bindings.len() != value_parameters.len() {
                    return Err(format!(
                        "expression {expression} has {} parameter bindings for {} value parameters",
                        parameter_bindings.len(),
                        value_parameters.len()
                    ));
                }
                let mut explicit_formals = BTreeSet::new();
                for (binding, parameter) in parameter_bindings.iter().zip(value_parameters) {
                    if binding.formal != parameter.formal
                        || binding.ordinal != parameter.ordinal
                        || binding.name != parameter.name
                        || binding.requirement != parameter.requirement
                    {
                        return Err(format!(
                            "expression {expression} parameter binding {} differs from callable {}",
                            binding.ordinal, callable
                        ));
                    }
                    match &binding.kind {
                        SemanticCallParameterBindingKind::Explicit {
                            checked_value,
                            value,
                            from_pipe,
                        } => {
                            let matches = arguments
                                .iter()
                                .filter(|argument| {
                                    argument.formal == binding.formal
                                        && argument.ordinal == binding.ordinal
                                        && argument.name == binding.name
                                        && argument.checked_value == *checked_value
                                        && argument.value == *value
                                        && argument.from_pipe == *from_pipe
                                })
                                .count();
                            if matches != 1 || !explicit_formals.insert(binding.formal) {
                                return Err(format!(
                                    "expression {expression} explicit parameter {} has {matches} exact arguments",
                                    binding.ordinal
                                ));
                            }
                        }
                        SemanticCallParameterBindingKind::Omitted => {
                            if !binding.requirement.is_optional()
                                || arguments
                                    .iter()
                                    .any(|argument| argument.formal == binding.formal)
                            {
                                return Err(format!(
                                    "expression {expression} illegally omits parameter {}",
                                    binding.ordinal
                                ));
                            }
                        }
                    }
                }
                if arguments.len() != explicit_formals.len() {
                    return Err(format!(
                        "expression {expression} has an argument without an exact parameter binding"
                    ));
                }
                match (&callable_definition.context_parameter, context_argument) {
                    (None, None) => {}
                    (Some(parameter), Some(argument))
                        if *callable_kind == SemanticCallableKind::User
                            && parameter.formal == argument.formal =>
                    {
                        let argument_definition = self.require_expression(
                            argument.value,
                            context("PASSED context argument"),
                        )?;
                        if let Some(resolved_instance) = resolved_instance {
                            let passed = resolved_instance.passed.ok_or_else(|| {
                                format!(
                                    "expression {expression} retained callable {} without a resolved PASSED value",
                                    callable_definition.id
                                )
                            })?;
                            if passed.formal != argument.formal
                                || Some(passed.value.expression) != argument.checked_value
                            {
                                return Err(format!(
                                    "expression {expression} retained PASSED argument differs from call instance {instance:?}"
                                ));
                            }
                        } else {
                            match (call_definition.context_binding, argument.checked_value) {
                                (
                                    CheckedContextBinding::Explicit { value, .. },
                                    Some(checked_value),
                                ) if value == checked_value => {}
                                (CheckedContextBinding::Inherited { formal }, None) => {
                                    let owner = call_definition.owner_callable.ok_or_else(|| {
                                        format!(
                                            "expression {expression} inherits PASSED without an owner callable"
                                        )
                                    })?;
                                    let owner = self.require_callable(
                                        owner,
                                        context("PASSED owner callable"),
                                    )?;
                                    let owner_parameter = owner
                                        .context_parameter
                                        .as_ref()
                                        .filter(|parameter| parameter.formal == formal)
                                        .ok_or_else(|| {
                                            format!(
                                                "expression {expression} inherits PASSED formal {} from a mismatched owner",
                                                formal.0
                                            )
                                        })?;
                                    let mut pending = vec![argument_definition.id];
                                    let mut visited = BTreeSet::new();
                                    let mut captured_leaf = false;
                                    while let Some(value) = pending.pop() {
                                        if !visited.insert(value) {
                                            continue;
                                        }
                                        let value = self.require_expression(
                                            value,
                                            context("PASSED capture value"),
                                        )?;
                                        match &value.kind {
                                            SemanticExpressionKind::FunctionParameter {
                                                parameter,
                                                ..
                                            } if *parameter == owner_parameter.id => {
                                                captured_leaf = true;
                                            }
                                            SemanticExpressionKind::Project { input, .. } => {
                                                pending.push(*input);
                                            }
                                            SemanticExpressionKind::Object(fields)
                                                if !fields.is_empty()
                                                    && fields.iter().all(|field| !field.spread) =>
                                            {
                                                pending
                                                    .extend(fields.iter().map(|field| field.value));
                                            }
                                            _ => {
                                                captured_leaf = false;
                                                break;
                                            }
                                        }
                                    }
                                    if !captured_leaf {
                                        return Err(format!(
                                            "expression {expression} inherited PASSED argument is not an exact capture of the owner's hidden parameter: owner={} expected={:?} actual={:?}",
                                            owner.id, owner_parameter.id, argument_definition.kind,
                                        ));
                                    }
                                }
                                _ => {
                                    return Err(format!(
                                        "expression {expression} retained PASSED argument has no exact checked binding"
                                    ));
                                }
                            }
                        }
                    }
                    _ => {
                        return Err(format!(
                            "expression {expression} retained PASSED argument differs from callable {}",
                            callable_definition.id
                        ));
                    }
                }
                for argument in arguments {
                    self.require_expression(argument.value, context("call argument"))?;
                }
                let expected_contexts = call_definition
                    .contexts
                    .iter()
                    .map(|context| context.signature)
                    .collect::<Vec<_>>();
                let actual_contexts = contexts
                    .iter()
                    .map(|context| {
                        if Some(context.call_instance) != *instance {
                            return Err(format!(
                                "expression {expression} context uses call instance {} instead of {instance:?}",
                                context.call_instance
                            ));
                        }
                        Ok(context.ordinal)
                    })
                    .collect::<Result<Vec<_>, String>>()?;
                let retained_context_overlay = retained_definition_expression
                    && instance.is_none()
                    && contexts.is_empty()
                    && !expected_contexts.is_empty()
                    && *effect == CheckedEffectSummary::default()
                    && *callable_kind == SemanticCallableKind::Builtin
                    && boon_checked::is_registered_render_constructor(&call_definition.function);
                if actual_contexts != expected_contexts && !retained_context_overlay {
                    return Err(format!(
                        "expression {expression} contexts differ from semantic call {call}"
                    ));
                }
                for call in contexts {
                    validate_call_context(out_net, *call, &context("call context"))?;
                }
            }
            SemanticExpressionKind::Materialize { materialization } => {
                self.require_materialization(*materialization, context("materialization"))?;
            }
            SemanticExpressionKind::Flush { payload: input }
            | SemanticExpressionKind::FlushBoundary { input }
            | SemanticExpressionKind::Draining { input }
            | SemanticExpressionKind::Project { input, .. } => {
                self.require_expression(*input, context("input"))?;
            }
            SemanticExpressionKind::Hold {
                initial, updates, ..
            } => {
                self.require_expression(*initial, context("initial value"))?;
                for update in updates {
                    self.require_expression(*update, context("update"))?;
                }
            }
            SemanticExpressionKind::Latest { branches } => {
                for branch in branches {
                    self.require_expression(*branch, context("latest branch"))?;
                }
            }
            SemanticExpressionKind::When { input, arms, .. } => {
                self.require_expression(*input, context("when input"))?;
                for arm in arms {
                    self.require_expression(arm.output, context("when arm"))?;
                }
            }
            SemanticExpressionKind::Then { input, output } => {
                self.require_expression(*input, context("then input"))?;
                if let Some(output) = output {
                    self.require_expression(*output, context("then output"))?;
                }
            }
            SemanticExpressionKind::Infix { left, right, .. } => {
                self.require_expression(*left, context("left operand"))?;
                self.require_expression(*right, context("right operand"))?;
            }
            SemanticExpressionKind::MapEntry { key, value } => {
                self.require_expression(*key, context("map key"))?;
                self.require_expression(*value, context("map value"))?;
            }
            SemanticExpressionKind::MatchArm { output, .. } => {
                if let Some(output) = output {
                    self.require_expression(*output, context("match output"))?;
                }
            }
            SemanticExpressionKind::Block { bindings, result } => {
                for binding in bindings {
                    self.require_expression(binding.value, context("block binding"))?;
                    if local_definitions
                        .insert(binding.id, (binding.declaration, binding.value))
                        .is_some()
                    {
                        return Err(format!(
                            "semantic local binding {} is defined more than once",
                            binding.id
                        ));
                    }
                }
                self.require_expression(*result, context("block result"))?;
            }
            SemanticExpressionKind::List { items, .. }
            | SemanticExpressionKind::Bytes { items, .. }
            | SemanticExpressionKind::Map { entries: items }
            | SemanticExpressionKind::Set { items } => {
                for item in items {
                    self.require_expression(*item, context("item"))?;
                }
            }
            SemanticExpressionKind::MaterializationLocal { owner, local, .. } => {
                self.validate_materialization_local(
                    *owner,
                    *local,
                    materialization_locals,
                    context("materialization local"),
                )?;
            }
            SemanticExpressionKind::FunctionParameter { parameter, .. } => {
                let callable =
                    self.require_callable(parameter.callable, context("function parameter"))?;
                let expected = callable
                    .parameters
                    .get(parameter.ordinal)
                    .map(|parameter| parameter.id)
                    .or_else(|| {
                        callable
                            .context_parameter
                            .as_ref()
                            .filter(|context| context.id.ordinal == parameter.ordinal)
                            .map(|context| context.id)
                    })
                    .ok_or_else(|| {
                        format!(
                            "expression {expression} references missing parameter {}:{}",
                            parameter.callable, parameter.ordinal
                        )
                    })?;
                if expected != *parameter {
                    return Err(format!(
                        "expression {expression} parameter {}:{} does not match function table",
                        parameter.callable, parameter.ordinal
                    ));
                }
            }
            SemanticExpressionKind::ExternalRead { .. }
            | SemanticExpressionKind::Drain { .. }
            | SemanticExpressionKind::Text(_)
            | SemanticExpressionKind::Number(_)
            | SemanticExpressionKind::Bits(_)
            | SemanticExpressionKind::BytesByte(_)
            | SemanticExpressionKind::Absent
            | SemanticExpressionKind::Tag(_)
            | SemanticExpressionKind::Source { .. }
            | SemanticExpressionKind::Delimiter => {}
        }
        Ok(())
    }

    fn validate_functions(&self, out_net: &ResolvedOutGraph) -> Result<(), String> {
        let producer_roots = out_net
            .producer_roots()
            .iter()
            .map(|root| (root.spec.function, &root.spec))
            .collect::<BTreeMap<_, _>>();
        let function_producers = self
            .functions
            .iter()
            .map(|function| (function.producer, function.callable))
            .collect::<BTreeMap<_, _>>();
        if function_producers.len() != self.functions.len() {
            return Err("semantic functions contain duplicate producer IDs".to_owned());
        }
        if function_producers.keys().copied().collect::<BTreeSet<_>>()
            != producer_roots.keys().copied().collect::<BTreeSet<_>>()
        {
            return Err(
                "semantic function producer IDs do not exactly cover resolved producer roots"
                    .to_owned(),
            );
        }
        for function in &self.functions {
            let producer = producer_roots.get(&function.producer).ok_or_else(|| {
                format!(
                    "semantic callable {} references missing producer {}",
                    function.callable, function.producer
                )
            })?;
            let callable = self.require_callable(
                function.callable,
                format!("producer function {}", function.producer),
            )?;
            if callable.checked_callable != producer.callable {
                return Err(format!(
                    "semantic producer {} callable {} does not match checked callable {}",
                    function.producer, function.callable, producer.callable.0
                ));
            }
            if function.identity != producer.identity {
                return Err(format!(
                    "semantic callable {} identity does not match producer {}",
                    function.callable, function.producer
                ));
            }
            self.require_expression(
                function.root,
                format!("producer function {} root", function.producer),
            )?;
            if let Some(source) = function.invocation_source {
                self.require_expression(
                    source,
                    format!("producer function {} invocation source", function.producer),
                )?;
            }
            for (ordinal, parameter) in function.parameters.iter().enumerate() {
                let expected = SemanticParameterId {
                    callable: function.callable,
                    ordinal,
                };
                if parameter.id != expected {
                    return Err(format!(
                        "semantic producer {} parameter at ordinal {ordinal} has ID {:?}",
                        function.producer, parameter.id
                    ));
                }
            }
        }
        Ok(())
    }

    fn validate_materializations(
        &self,
        materialization_locals: &BTreeSet<(StaticOwnerId, SemanticMaterializationLocalId)>,
    ) -> Result<(), String> {
        for materialization in &self.materializations {
            self.validate_owner(
                Some(materialization.owner),
                format!("materialization {}", materialization.id),
            )?;
            self.validate_materialization_local(
                materialization.owner,
                materialization.row_local,
                materialization_locals,
                format!("materialization {}", materialization.id),
            )?;
            for expression in materialization.expression_roots() {
                self.require_expression(
                    expression,
                    format!("materialization {} expression", materialization.id),
                )?;
            }
        }
        Ok(())
    }

    fn validate_materialization_local(
        &self,
        owner: StaticOwnerId,
        local: SemanticMaterializationLocalId,
        locals: &BTreeSet<(StaticOwnerId, SemanticMaterializationLocalId)>,
        context: impl fmt::Display,
    ) -> Result<(), String> {
        self.validate_owner(Some(owner), context.to_string())?;
        if !locals.contains(&(owner, local)) {
            return Err(format!(
                "{context} references missing materialization local {owner}:{local}"
            ));
        }
        Ok(())
    }

    fn validate_owner(
        &self,
        owner: Option<StaticOwnerId>,
        context: impl fmt::Display,
    ) -> Result<(), String> {
        let Some(owner) = owner else {
            return Ok(());
        };
        if self
            .static_owners
            .get(owner.as_usize())
            .is_none_or(|definition| definition.id != owner)
        {
            return Err(format!("{context} references missing static owner {owner}"));
        }
        Ok(())
    }

    fn require_expression(
        &self,
        id: SemanticExprId,
        context: impl fmt::Display,
    ) -> Result<&SemanticExpression, String> {
        self.expressions
            .get(id.as_usize())
            .filter(|expression| expression.id == id)
            .ok_or_else(|| format!("{context} references missing semantic expression {id}"))
    }

    fn require_statement(
        &self,
        id: SemanticStatementId,
        context: impl fmt::Display,
    ) -> Result<&SemanticStatement, String> {
        self.statements
            .get(id.as_usize())
            .filter(|statement| statement.id == id)
            .ok_or_else(|| format!("{context} references missing semantic statement {id}"))
    }

    fn require_scope(
        &self,
        id: SemanticScopeId,
        context: impl fmt::Display,
    ) -> Result<&SemanticScope, String> {
        self.scopes
            .get(id.as_usize())
            .filter(|scope| scope.id == id)
            .ok_or_else(|| format!("{context} references missing semantic scope {id}"))
    }

    fn require_source(
        &self,
        id: SemanticSourceId,
        context: impl fmt::Display,
    ) -> Result<&SemanticSourceDef, String> {
        self.sources
            .get(id.as_usize())
            .filter(|source| source.id == id)
            .ok_or_else(|| format!("{context} references missing semantic source {id}"))
    }

    fn require_state(
        &self,
        id: SemanticStateId,
        context: impl fmt::Display,
    ) -> Result<&SemanticStateDef, String> {
        self.states
            .get(id.as_usize())
            .filter(|state| state.id == id)
            .ok_or_else(|| format!("{context} references missing semantic state {id}"))
    }

    fn require_callable(
        &self,
        id: SemanticCallableId,
        context: impl fmt::Display,
    ) -> Result<&SemanticCallable, String> {
        self.callables
            .get(id.as_usize())
            .filter(|callable| callable.id == id)
            .ok_or_else(|| format!("{context} references missing semantic callable {id}"))
    }

    fn require_call(
        &self,
        id: SemanticCallId,
        context: impl fmt::Display,
    ) -> Result<&SemanticCall, String> {
        self.calls
            .get(id.as_usize())
            .filter(|call| call.id == id)
            .ok_or_else(|| format!("{context} references missing semantic call {id}"))
    }

    fn require_producer_function(
        &self,
        id: ProducerFunctionId,
        context: impl fmt::Display,
    ) -> Result<&SemanticFunction, String> {
        self.functions
            .get(id.as_usize())
            .filter(|function| function.producer == id)
            .ok_or_else(|| format!("{context} references missing semantic producer {id}"))
    }

    fn require_materialization(
        &self,
        id: SemanticMaterializationId,
        context: impl fmt::Display,
    ) -> Result<&SemanticContextualMaterialization, String> {
        self.materializations
            .get(id.as_usize())
            .filter(|materialization| materialization.id == id)
            .ok_or_else(|| format!("{context} references missing semantic materialization {id}"))
    }
}

fn validate_call_context(
    out_net: &ResolvedOutGraph,
    context: SemanticCallContextId,
    label: &str,
) -> Result<(), String> {
    validate_call_instance(out_net, context.call_instance, label)
}

fn validate_call_instance(
    out_net: &ResolvedOutGraph,
    call: OutCallInstanceId,
    context: &str,
) -> Result<(), String> {
    if out_net
        .call_instances
        .get(call.as_usize())
        .is_none_or(|instance| instance.id != call)
    {
        return Err(format!(
            "{context} references missing resolved OUT call instance {call}"
        ));
    }
    Ok(())
}

fn validate_dense_local_bindings(
    definitions: &BTreeMap<SemanticLocalBindingId, (DeclId, SemanticExprId)>,
) -> Result<(), String> {
    for (index, binding) in definitions.keys().copied().enumerate() {
        if binding != SemanticLocalBindingId(index) {
            return Err(format!(
                "semantic local binding at index {index} has non-dense ID {binding}"
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_semantic_program() -> crate::SemanticProgram {
        let parsed = boon_parser::parse_source("semantic-execution-empty.bn", "").unwrap();
        let checked = boon_typecheck::check_program(&parsed)
            .program
            .expect("empty program typechecks");
        crate::elaborate(checked, &[]).expect("empty program elaborates")
    }

    fn one_expression_graph() -> SemanticExecutionImageColumnsV1 {
        SemanticExecutionImageColumnsV1 {
            expressions: vec![SemanticExpression {
                id: SemanticExprId(0),
                value_id: SemanticValueId(0),
                checked_expr_id: CheckedExprId(0),
                flow_type: FlowType {
                    mode: boon_checked::FlowMode::Continuous,
                    ty: Type::Text,
                },
                effect: CheckedEffectSummary::default(),
                owner: None,
                provenance: SemanticValueProvenance::default(),
                resource_binding_path: None,
                kind: SemanticExpressionKind::Text("value".to_owned()),
            }],
            checked_expression_origins: vec![SemanticExpressionOrigin {
                expression: SemanticExprId(0),
                checked_expression: CheckedExprId(0),
                checked_scope: LexicalScopeId(0),
                checked_span: CheckedSpan::default(),
                owning_statement: None,
                call_instance: None,
            }],
            scopes: vec![SemanticScope {
                id: SemanticScopeId(0),
                checked_scope: LexicalScopeId(0),
                parent: None,
                owner: None,
                kind: CheckedScopeKind::Root,
                span: CheckedSpan::default(),
            }],
            ..SemanticExecutionImageColumnsV1::default()
        }
    }

    fn lifetime_expression(id: usize, kind: SemanticExpressionKind) -> SemanticExpression {
        SemanticExpression {
            id: SemanticExprId(id),
            value_id: SemanticValueId(id),
            checked_expr_id: CheckedExprId(u32::try_from(id).unwrap()),
            flow_type: FlowType {
                mode: boon_checked::FlowMode::Continuous,
                ty: Type::Text,
            },
            effect: CheckedEffectSummary::default(),
            owner: None,
            provenance: SemanticValueProvenance::default(),
            resource_binding_path: None,
            kind,
        }
    }

    #[test]
    fn state_lifetime_deriver_reuses_one_parent_index() {
        let expressions = vec![
            lifetime_expression(0, SemanticExpressionKind::Text("state".to_owned())),
            lifetime_expression(
                1,
                SemanticExpressionKind::Project {
                    input: SemanticExprId(0),
                    fields: Vec::new(),
                },
            ),
            lifetime_expression(2, SemanticExpressionKind::Text("trigger".to_owned())),
            lifetime_expression(
                3,
                SemanticExpressionKind::Then {
                    input: SemanticExprId(2),
                    output: Some(SemanticExprId(1)),
                },
            ),
            lifetime_expression(4, SemanticExpressionKind::Text("persistent".to_owned())),
        ];
        let mut deriver = SemanticStateLifetimeDeriverV1::new(&expressions).unwrap();

        assert_eq!(
            deriver.derive(SemanticExprId(0)).unwrap(),
            SemanticStateLifetimeV1::ActivationLocal {
                then_expression: SemanticExprId(3),
            }
        );
        assert_eq!(
            deriver.derive(SemanticExprId(4)).unwrap(),
            SemanticStateLifetimeV1::Persistent
        );
    }

    #[test]
    fn state_lifetime_deriver_rejects_equal_nearest_then_sites() {
        let expressions = vec![
            lifetime_expression(0, SemanticExpressionKind::Text("state".to_owned())),
            lifetime_expression(1, SemanticExpressionKind::Text("left".to_owned())),
            lifetime_expression(2, SemanticExpressionKind::Text("right".to_owned())),
            lifetime_expression(
                3,
                SemanticExpressionKind::Then {
                    input: SemanticExprId(1),
                    output: Some(SemanticExprId(0)),
                },
            ),
            lifetime_expression(
                4,
                SemanticExpressionKind::Then {
                    input: SemanticExprId(2),
                    output: Some(SemanticExprId(0)),
                },
            ),
        ];
        let mut deriver = SemanticStateLifetimeDeriverV1::new(&expressions).unwrap();

        let error = deriver.derive(SemanticExprId(0)).unwrap_err();
        assert!(
            error.contains("2 equally-near THEN activation sites"),
            "{error}"
        );
        assert!(error.contains("SemanticExprId(3)"), "{error}");
        assert!(error.contains("SemanticExprId(4)"), "{error}");
    }

    #[test]
    fn valid_empty_graph_is_accepted() {
        let semantic = empty_semantic_program();
        SemanticExecutionImageColumnsV1::default()
            .validate(semantic.resolved_out_graph())
            .unwrap();
    }

    #[test]
    fn non_dense_expression_and_value_ids_are_rejected() {
        let semantic = empty_semantic_program();
        let mut graph = one_expression_graph();
        graph.expressions[0].id = SemanticExprId(1);
        let error = graph.validate(semantic.resolved_out_graph()).unwrap_err();
        assert!(error.contains("non-dense ID 1"), "{error}");

        let mut graph = one_expression_graph();
        graph.expressions[0].value_id = SemanticValueId(1);
        let error = graph.validate(semantic.resolved_out_graph()).unwrap_err();
        assert!(error.contains("non-dense value ID 1"), "{error}");
    }

    #[test]
    fn missing_child_expression_is_rejected() {
        let semantic = empty_semantic_program();
        let mut graph = one_expression_graph();
        graph.expressions[0].kind = SemanticExpressionKind::Draining {
            input: SemanticExprId(9),
        };
        let error = graph.validate(semantic.resolved_out_graph()).unwrap_err();
        assert!(error.contains("missing semantic expression 9"), "{error}");
    }

    #[test]
    fn non_dense_local_binding_is_rejected() {
        let semantic = empty_semantic_program();
        let mut graph = one_expression_graph();
        graph.expressions[0].kind = SemanticExpressionKind::Block {
            bindings: vec![SemanticBlockBinding {
                id: SemanticLocalBindingId(1),
                declaration: DeclId(0),
                value: SemanticExprId(0),
            }],
            result: SemanticExprId(0),
        };
        let error = graph.validate(semantic.resolved_out_graph()).unwrap_err();
        assert!(error.contains("non-dense ID 1"), "{error}");
    }

    #[test]
    fn incomplete_checked_origin_coverage_is_rejected() {
        let semantic = empty_semantic_program();
        let mut graph = one_expression_graph();
        graph.checked_expression_origins.clear();
        let error = graph.validate(semantic.resolved_out_graph()).unwrap_err();
        assert!(error.contains("does not exactly cover"), "{error}");
    }
}
