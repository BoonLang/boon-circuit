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
use boon_typecheck::{
    CheckedCallContextKind, CheckedCallId, CheckedCallableKind, CheckedContextBinding,
    CheckedContextTypeSubstitution, CheckedContextualOperation, CheckedEffectSummary,
    CheckedEvaluationScope, CheckedExprId, CheckedExternalDeclarationIdentityV1,
    CheckedMatchPattern, CheckedParameterKind, CheckedParameterRequirement, CheckedProgram,
    CheckedResourceBinding, CheckedScopeKind, CheckedSourceId, CheckedSpan, CheckedStateId,
    CheckedStatementId, CheckedStatementKind, CheckedTypeSubstitution, ContextFormalId, DeclId,
    FlowType, LexicalScopeId, ProgramRole, Type,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
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
    pub result: FlowType,
    pub role: ProgramRole,
    pub effect: CheckedEffectSummary,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<CheckedStatementId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_expression: Option<CheckedExprId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contextual_operation: Option<CheckedContextualOperation>,
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
    Number(String),
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
        role: ProgramRole,
        effect: CheckedEffectSummary,
        result: FlowType,
        instance: OutCallInstanceId,
        arguments: Vec<SemanticCallArgument>,
        parameter_bindings: Vec<SemanticCallParameterBinding>,
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

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticMaterializationResultKind {
    RuntimeValue,
    RenderSlot,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticExecutionGraphV1 {
    pub expressions: Vec<SemanticExpression>,
    pub statements: Vec<SemanticStatement>,
    pub scopes: Vec<SemanticScope>,
    pub callables: Vec<SemanticCallable>,
    pub calls: Vec<SemanticCall>,
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
    checked: &CheckedProgram,
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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_row_predecessors: Vec<SemanticContextualRowPredecessor>,
    pub body: SemanticExprId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub direction: Option<SemanticExprId>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub inherited_order: Vec<SemanticContextualOrderKey>,
    pub result_kind: SemanticMaterializationResultKind,
    pub row_local: SemanticMaterializationLocalId,
    pub owner: StaticOwnerId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_list_id: Option<SemanticListId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_scope_id: Option<SemanticRowScopeId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_list_id: Option<SemanticListId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_scope_id: Option<SemanticRowScopeId>,
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

impl SemanticExecutionGraphV1 {
    pub fn validate(&self, out_net: &ResolvedOutGraph) -> Result<(), String> {
        self.validate_static_owners(out_net)?;
        self.validate_dense_ids()?;
        self.validate_callable_and_call_tables()?;
        self.validate_checked_expression_origins(out_net)?;

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
        for state in &self.states {
            self.require_expression(state.expression, format!("state {} expression", state.id))?;
            self.require_expression(state.initial, format!("state {} initial value", state.id))?;
            self.validate_owner(state.owner, format!("state {}", state.id))?;
            self.require_statement(state.statement, format!("state {} statement", state.id))?;
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

    pub(crate) fn validate_checked_roots(&self, checked: &CheckedProgram) -> Result<(), String> {
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
                    default: boon_typecheck::CheckedParameterDefault::CallableProfile { profile },
                } = &parameter.requirement
                    && profile.is_empty()
                {
                    return Err(format!(
                        "semantic callable {} parameter {} has an empty default profile",
                        callable.id, parameter.name
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
                role,
                effect,
                result,
                instance,
                arguments,
                parameter_bindings,
                contexts,
            } => {
                validate_call_instance(out_net, *instance, &context("call instance"))?;
                let call_definition = self.require_call(*call, context("call definition"))?;
                let callable_definition =
                    self.require_callable(*callable, context("callable definition"))?;
                let expected_kind = match callable_definition.kind {
                    CheckedCallableKind::Builtin => SemanticCallableKind::Builtin,
                    CheckedCallableKind::External => SemanticCallableKind::External,
                    CheckedCallableKind::User => {
                        return Err(format!(
                            "expression {expression} retains an unexpanded user call"
                        ));
                    }
                };
                let resolved_instance = &out_net.call_instances[instance.as_usize()];
                let expected_flow_type = FlowType {
                    mode: resolved_instance.result.mode,
                    ty: crate::contextual_expansion::erase_runtime_type_vars(
                        &resolved_instance.result.ty,
                    ),
                };
                let expression_definition =
                    self.require_expression(expression, context("call expression"))?;
                if call_definition.callable != *callable
                    || *callable_kind != expected_kind
                    || name != &callable_definition.name
                    || function != &call_definition.function
                    || *role != call_definition.role
                    || *effect != callable_definition.effect
                    || result != &call_definition.result
                    || expression_definition.checked_expr_id != call_definition.checked_expression
                    || expression_definition.effect != *effect
                    || expression_definition.flow_type != expected_flow_type
                {
                    return Err(format!(
                        "expression {expression} call contract differs from semantic call {call}"
                    ));
                }
                if resolved_instance.provenance.call_id != Some(call_definition.checked_call)
                    || resolved_instance.provenance.callable != callable_definition.checked_callable
                {
                    return Err(format!(
                        "expression {expression} call instance {instance} has stale checked provenance"
                    ));
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
                        if context.call_instance != *instance {
                            return Err(format!(
                                "expression {expression} context uses call instance {} instead of {instance}",
                                context.call_instance
                            ));
                        }
                        Ok(context.ordinal)
                    })
                    .collect::<Result<Vec<_>, String>>()?;
                if actual_contexts != expected_contexts {
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
                let expected = callable.parameters.get(parameter.ordinal).ok_or_else(|| {
                    format!(
                        "expression {expression} references missing parameter {}:{}",
                        parameter.callable, parameter.ordinal
                    )
                })?;
                if expected.id != *parameter {
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
            for predecessor in &materialization.source_row_predecessors {
                let referenced = match predecessor {
                    SemanticContextualRowPredecessor::Materialized { materialization }
                    | SemanticContextualRowPredecessor::Provenance { materialization } => {
                        Some(*materialization)
                    }
                    SemanticContextualRowPredecessor::Value
                    | SemanticContextualRowPredecessor::Stored { .. } => None,
                };
                if let Some(referenced) = referenced {
                    self.require_materialization(
                        referenced,
                        format!("materialization {} predecessor", materialization.id),
                    )?;
                }
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

    fn one_expression_graph() -> SemanticExecutionGraphV1 {
        SemanticExecutionGraphV1 {
            expressions: vec![SemanticExpression {
                id: SemanticExprId(0),
                value_id: SemanticValueId(0),
                checked_expr_id: CheckedExprId(0),
                flow_type: FlowType {
                    mode: boon_typecheck::FlowMode::Continuous,
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
            ..SemanticExecutionGraphV1::default()
        }
    }

    #[test]
    fn valid_empty_graph_is_accepted() {
        let semantic = empty_semantic_program();
        SemanticExecutionGraphV1::default()
            .validate(semantic.resolved_out_graph())
            .unwrap();
    }

    #[test]
    fn non_dense_expression_id_is_rejected() {
        let semantic = empty_semantic_program();
        let mut graph = one_expression_graph();
        graph.expressions[0].id = SemanticExprId(1);
        let error = graph.validate(semantic.resolved_out_graph()).unwrap_err();
        assert!(error.contains("non-dense ID 1"), "{error}");
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
