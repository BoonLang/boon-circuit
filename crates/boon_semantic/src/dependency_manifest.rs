//! Exhaustive callable dependency closure for verification and lowering.
//!
//! The manifest is derived only after every semantic graph is final. Checked
//! IDs are retained as source provenance; semantic IDs and explicit child
//! ordinals carry executable authority. Every checked and semantic record
//! instance receives exactly one primary callable/root owner and exactly one
//! coverage disposition.

use crate::*;
use boon_contract::SourceBundleDigestV1;
use boon_typecheck::{
    CheckedCallEntry, CheckedDeclarationKind, CheckedEvaluationScope, CheckedExpressionKind,
    CheckedParameterKind, CheckedProgram, CheckedStatementKind, DeclId, FlowType, LexicalScopeId,
    ProgramRole,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

pub const CALLABLE_DEPENDENCY_MANIFEST_SCHEMA_V1: &str = "boon.callable-dependency-manifest.v1";
const CHECKED_PROGRAM_DIGEST_DOMAIN: &[u8] = b"boon.checked-program.v1\0";
const DEPENDENCY_COMPONENT_DIGEST_DOMAIN: &[u8] = b"boon.callable-dependency-components.v1\0";
const DEPENDENCY_RECORD_PAYLOAD_DOMAIN: &[u8] = b"boon.callable-dependency-record-payload.v1\0";
const DEPENDENCY_PUBLIC_SHAPE_DOMAIN: &[u8] = b"boon.callable-dependency-public-shape.v1\0";
const DEPENDENCY_IMPLEMENTATION_DOMAIN: &[u8] = b"boon.callable-dependency-implementation.v1\0";
const DEPENDENCY_MANIFEST_DIGEST_DOMAIN: &[u8] = b"boon.callable-dependency-manifest.v1\0";

macro_rules! dependency_id {
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

dependency_id!(SemanticDependencyRecordId, SemanticDependencyCoverageId);

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SemanticDependencyOwnerV1 {
    ProgramRoot,
    Callable { callable: SemanticCallableId },
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticDependencyChannelV1 {
    ParentValueFormal,
    PassedContextFormal,
    OutFormal,
    OutputEvaluatedFormal,
    CompilerContext,
    LexicalCapture,
    LocalFact,
    ResourceRead,
    ResourceWrite,
    ResourceBehavior,
    CalledCallable,
    NormalizedDefault,
    ExternalValueOrCall,
    RuntimeIntrinsicOrHostEffect,
    MigrationPredecessor,
    PersistenceActivation,
    TypeAndFlowInstance,
    StructuralRepresentation,
    SemanticProfile,
    AssuranceInput,
    CoverageRouting,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticDependencyRoleV1 {
    FormulaBinder,
    FixedDefinition,
    ResourceOrProviderBehavior,
    CoverageOrRouting,
    AssuranceOrActivation,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticDependencyVisibilityV1 {
    Public,
    Private,
    Diagnostic,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticDependencyMultiplicityV1 {
    Once,
    PerTick,
    PerEvent,
    PerRowMaterialization,
    PerTransition,
    PerActivation,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticDependencyLifetimeV1 {
    Definition,
    Call,
    Snapshot,
    Event,
    Row,
    Activation,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticDependencyPhaseV1 {
    Definition,
    CurrentValue,
    PreviousCommittedValue,
    CandidateWrite,
    Commit,
    EventPayload,
    EffectCompletion,
    PersistenceActivation,
}

/// Stable domain tag for one checked or semantic identity.
///
/// `Indexed` preserves the exact typed ID value under an explicit domain.
/// Composite records use a parent identity plus an exact child ordinal.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticDependencyEntityDomainV1 {
    CheckedScope,
    CheckedDeclaration,
    CheckedStatement,
    CheckedExpression,
    CheckedCallable,
    CheckedContextFormal,
    CheckedCall,
    CheckedCallResultPath,
    CheckedOrderChain,
    CheckedPatternBinding,
    CheckedResourceProjection,
    CheckedSource,
    CheckedState,
    CheckedList,
    CheckedOccurrence,
    ProducerMaterialization,
    OutCallInstance,
    OutPort,
    OutNet,
    StaticOwner,
    SemanticScope,
    SemanticExpression,
    SemanticValue,
    SemanticStatement,
    SemanticCallable,
    SemanticCall,
    SemanticSource,
    SemanticState,
    SemanticFunction,
    SemanticMaterialization,
    SemanticRowScope,
    SemanticList,
    SemanticValueListAuthority,
    SemanticResourceAlias,
    SemanticMaterializationBinding,
    SemanticListProjection,
    SemanticProducerResource,
    SemanticProducerInstance,
    SemanticField,
    SemanticBinding,
    SemanticRead,
    SemanticDependencyUse,
    SemanticTriggerArm,
    SemanticStateUpdateArm,
    SemanticExternalDependency,
    SemanticListMutation,
    SemanticDerivedValue,
    SemanticHostEffect,
    SemanticCapture,
    SemanticMigrationInput,
    SemanticViewRoot,
    SemanticViewNode,
    SemanticViewArgument,
    SemanticViewBinding,
    SemanticStorageLocal,
    SemanticStorageField,
    SemanticStorageCapture,
    SemanticStorageRowValue,
    SemanticStorageRowSourceProjection,
    SemanticStorageExternalReference,
    SemanticProducerResult,
    SemanticNamedValue,
    SemanticMemory,
    SemanticMigrationEdge,
    SourceUnit,
    SourceExpression,
    RenderSlot,
    SourcePayloadShape,
    OutputContract,
    HostPort,
    Diagnostic,
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SemanticDependencyEntityV1 {
    Program,
    Indexed {
        domain: SemanticDependencyEntityDomainV1,
        index: u64,
    },
    Composite {
        domain: SemanticDependencyEntityDomainV1,
        parent_index: u64,
        child_ordinal: usize,
    },
    Digest {
        domain: SemanticDependencyEntityDomainV1,
        digest: [u8; 32],
    },
}

impl SemanticDependencyEntityV1 {
    fn indexed(domain: SemanticDependencyEntityDomainV1, index: usize) -> Self {
        Self::Indexed {
            domain,
            index: index as u64,
        }
    }

    fn checked(domain: SemanticDependencyEntityDomainV1, index: u32) -> Self {
        Self::Indexed {
            domain,
            index: u64::from(index),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticDependencySubjectKindV1 {
    CheckedProgram,
    ProducerMaterializationTable,
    ResolvedOutGraph,
    ExecutionGraph,
    ResourceGraph,
    ReactiveGraph,
    LoweringContract,
    ViewBindingGraph,
    StorageGraph,
    MemoryGraph,
    CheckedScope,
    CheckedDeclaration,
    CheckedStatement,
    CheckedExpression,
    CheckedCallable,
    CheckedCallableParameter,
    CheckedCallableContext,
    CheckedContextFormal,
    CheckedCall,
    CheckedCallEntry,
    CheckedCallContext,
    CheckedContextSubstitution,
    CheckedTypeSubstitution,
    CheckedCallResultPath,
    CheckedOrderChain,
    CheckedOrderKey,
    CheckedPatternBinding,
    CheckedResourceProjection,
    CheckedSource,
    CheckedState,
    CheckedList,
    CheckedOccurrence,
    CheckedLoweringMetadata,
    CheckedLoweringSideTableEntry,
    CheckedNamedValueStatementSite,
    ProducerMaterializationRequest,
    OutCallInstance,
    OutInputBinding,
    OutPassedBinding,
    OutPort,
    OutNet,
    OutStructuralProducer,
    OutStaticOwner,
    ExecutionScope,
    ExecutionExpression,
    ExecutionExpressionOrigin,
    ExecutionStatement,
    ExecutionCallable,
    ExecutionCallableParameter,
    ExecutionCallableContext,
    ExecutionCall,
    ExecutionCallEntry,
    ExecutionCallContext,
    ExecutionSource,
    ExecutionState,
    ExecutionRoot,
    ExecutionFunction,
    ExecutionFunctionParameter,
    ExecutionMaterialization,
    ExecutionStaticOwner,
    ResourceRowScope,
    ResourceList,
    ResourceValueListAuthority,
    ResourceSource,
    ResourceState,
    ResourceAlias,
    ResourceMaterializationBinding,
    ResourceListProjection,
    ResourceProducer,
    ReactiveProducerInstance,
    ReactiveProducerParameter,
    ReactiveField,
    ReactiveBinding,
    ReactiveRead,
    ReactiveDependencyUse,
    ReactiveCallSchedule,
    ReactiveDerivedValue,
    ReactiveTriggerArm,
    ReactiveStateUpdateArm,
    ReactiveListMutation,
    ReactiveDependencyEdge,
    ReactivePossibleCauses,
    ReactiveHostEffect,
    ReactiveOutput,
    ReactiveViewCapture,
    ReactiveMigrationInput,
    LoweringMetadata,
    LoweringSourceUnit,
    LoweringExpressionType,
    LoweringExpressionOccurrence,
    LoweringFunctionType,
    LoweringFunctionParameter,
    LoweringNamedValue,
    LoweringRenderSlot,
    LoweringSourcePayload,
    LoweringSourcePayloadField,
    LoweringDiagnostic,
    LoweringOutputContract,
    LoweringHostPort,
    ViewRoot,
    ViewNode,
    ViewArgument,
    ViewBinding,
    ViewBindingTarget,
    StorageOwner,
    StorageLocal,
    StorageLocalMember,
    StorageCapture,
    StorageField,
    StorageBinding,
    StorageSource,
    StorageRowValue,
    StorageRowSourceProjection,
    StorageExternalReference,
    StorageProducerResult,
    StorageNamedValue,
    Memory,
    MemoryLeaf,
    MigrationEdge,
    MigrationInput,
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct SemanticDependencySubjectV1 {
    pub kind: SemanticDependencySubjectKindV1,
    pub identity: SemanticDependencyEntityV1,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<SemanticDependencyEntityV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub child_ordinal: Option<usize>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticDependencySemanticsV1 {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub projection: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flow_type: Option<FlowType>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evaluation_scope: Option<CheckedEvaluationScope>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub program_role: Option<ProgramRole>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub static_owner: Option<StaticOwnerId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub call_instance: Option<OutCallInstanceId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub row: Option<SemanticRowBinding>,
    pub multiplicity: SemanticDependencyMultiplicityV1,
    pub lifetime: SemanticDependencyLifetimeV1,
    pub phase: SemanticDependencyPhaseV1,
    pub visibility: SemanticDependencyVisibilityV1,
}

impl Default for SemanticDependencySemanticsV1 {
    fn default() -> Self {
        Self {
            projection: Vec::new(),
            flow_type: None,
            evaluation_scope: None,
            program_role: None,
            static_owner: None,
            call_instance: None,
            row: None,
            multiplicity: SemanticDependencyMultiplicityV1::Once,
            lifetime: SemanticDependencyLifetimeV1::Definition,
            phase: SemanticDependencyPhaseV1::Definition,
            visibility: SemanticDependencyVisibilityV1::Private,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticDependencyRecordV1 {
    pub id: SemanticDependencyRecordId,
    pub owner: SemanticDependencyOwnerV1,
    pub channel: SemanticDependencyChannelV1,
    pub roles: Vec<SemanticDependencyRoleV1>,
    pub subject: SemanticDependencySubjectV1,
    pub semantics: SemanticDependencySemanticsV1,
    pub payload_digest: [u8; 32],
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub referenced_dependencies: Vec<SemanticDependencyRecordId>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub referenced_owners: Vec<SemanticDependencyOwnerV1>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SemanticDependencyCoverageDispositionV1 {
    Dependency {
        dependency: SemanticDependencyRecordId,
    },
    Structural {
        payload_digest: [u8; 32],
    },
    Diagnostic {
        payload_digest: [u8; 32],
    },
    IntentionallyNonsemantic {
        payload_digest: [u8; 32],
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticDependencyCoverageV1 {
    pub id: SemanticDependencyCoverageId,
    pub subject: SemanticDependencySubjectV1,
    pub primary_owner: SemanticDependencyOwnerV1,
    pub disposition: SemanticDependencyCoverageDispositionV1,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProgramRootDependencyEntryV1 {
    pub direct_dependency_ids: Vec<SemanticDependencyRecordId>,
    pub closure_dependency_ids: Vec<SemanticDependencyRecordId>,
    pub public_shape_digest: [u8; 32],
    pub implementation_dependency_digest: [u8; 32],
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CallableDependencyEntryV1 {
    pub callable: SemanticCallableId,
    pub checked_callable: DeclId,
    pub direct_dependency_ids: Vec<SemanticDependencyRecordId>,
    pub closure_dependency_ids: Vec<SemanticDependencyRecordId>,
    pub public_shape_digest: [u8; 32],
    pub implementation_dependency_digest: [u8; 32],
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CallableDependencyComponentDigestsV1 {
    pub producer_materializations: [u8; 32],
    pub resolved_out_graph: [u8; 32],
    pub execution_graph: [u8; 32],
    pub resource_graph: [u8; 32],
    pub reactive_graph: [u8; 32],
    pub lowering_contract: [u8; 32],
    pub view_binding_graph: [u8; 32],
    pub scope_storage_graph: [u8; 32],
    pub memory_graph: [u8; 32],
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CallableDependencyManifestV1 {
    pub schema: String,
    pub source_bundle_digest_v1: SourceBundleDigestV1,
    pub checked_program_digest: CheckedProgramDigestV1,
    pub dependency_classifier_schema_digest: DependencyClassifierSchemaDigestV1,
    pub component_digests: CallableDependencyComponentDigestsV1,
    pub program_root: ProgramRootDependencyEntryV1,
    pub callable_entries: Vec<CallableDependencyEntryV1>,
    pub dependencies: Vec<SemanticDependencyRecordV1>,
    pub coverage: Vec<SemanticDependencyCoverageV1>,
    pub manifest_digest: CallableDependencyManifestDigestV1,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CallableDependencyManifestError {
    message: String,
}

impl CallableDependencyManifestError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for CallableDependencyManifestError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for CallableDependencyManifestError {}

#[derive(Clone, Debug)]
struct DependencyOwnerIndex {
    callable_by_checked: BTreeMap<DeclId, SemanticCallableId>,
    checked_scope_owner: BTreeMap<LexicalScopeId, SemanticDependencyOwnerV1>,
    checked_statement_owner:
        BTreeMap<boon_typecheck::CheckedStatementId, SemanticDependencyOwnerV1>,
    expression_owner: Vec<SemanticDependencyOwnerV1>,
    statement_owner: Vec<SemanticDependencyOwnerV1>,
    call_owner: Vec<SemanticDependencyOwnerV1>,
    out_call_owner: Vec<SemanticDependencyOwnerV1>,
    static_owner: BTreeMap<StaticOwnerId, SemanticDependencyOwnerV1>,
    source_owner: Vec<SemanticDependencyOwnerV1>,
    state_owner: Vec<SemanticDependencyOwnerV1>,
    list_owner: Vec<SemanticDependencyOwnerV1>,
    value_list_owner: Vec<SemanticDependencyOwnerV1>,
    binding_owner: Vec<SemanticDependencyOwnerV1>,
    storage_field_owner: Vec<SemanticDependencyOwnerV1>,
    memory_owner: Vec<SemanticDependencyOwnerV1>,
}

impl DependencyOwnerIndex {
    fn derive(
        checked: &CheckedProgram,
        out: &ResolvedOutGraph,
        execution: &SemanticExecutionGraphV1,
        resources: &SemanticResourceGraphV1,
        reactive: &SemanticReactiveGraphV1,
        storage: &SemanticScopeStorageGraphV1,
        memory: &SemanticMemoryGraphV1,
    ) -> Result<Self, CallableDependencyManifestError> {
        let mut callable_by_checked = BTreeMap::new();
        for (index, callable) in execution.callables.iter().enumerate() {
            if callable.id != SemanticCallableId(index) {
                return Err(CallableDependencyManifestError::new(format!(
                    "semantic callable {} is not dense index {index}",
                    callable.id
                )));
            }
            if let Some(previous) =
                callable_by_checked.insert(callable.checked_callable, callable.id)
            {
                return Err(CallableDependencyManifestError::new(format!(
                    "checked callable {} maps to semantic callables {previous} and {}",
                    callable.checked_callable.0, callable.id
                )));
            }
        }
        let checked_callables = checked
            .callables
            .iter()
            .map(|callable| callable.decl_id)
            .collect::<BTreeSet<_>>();
        if callable_by_checked.keys().copied().collect::<BTreeSet<_>>() != checked_callables {
            return Err(CallableDependencyManifestError::new(
                "semantic callable ownership is not a bijection over checked callables",
            ));
        }

        let mut checked_scope_owner = BTreeMap::new();
        for scope in &checked.scopes {
            let mut current = Some(scope.id);
            let mut visited = BTreeSet::new();
            let owner = loop {
                let Some(scope_id) = current else {
                    break SemanticDependencyOwnerV1::ProgramRoot;
                };
                if !visited.insert(scope_id) {
                    return Err(CallableDependencyManifestError::new(format!(
                        "checked scope ownership cycles at scope {}",
                        scope_id.0
                    )));
                }
                let candidate = checked
                    .scopes
                    .iter()
                    .find(|candidate| candidate.id == scope_id)
                    .ok_or_else(|| {
                        CallableDependencyManifestError::new(format!(
                            "checked scope ownership references missing scope {}",
                            scope_id.0
                        ))
                    })?;
                if let Some(callable) = candidate
                    .owner
                    .and_then(|declaration| callable_by_checked.get(&declaration).copied())
                {
                    break SemanticDependencyOwnerV1::Callable { callable };
                }
                current = candidate.parent;
            };
            if checked_scope_owner.insert(scope.id, owner).is_some() {
                return Err(CallableDependencyManifestError::new(format!(
                    "checked scope {} appears more than once",
                    scope.id.0
                )));
            }
        }

        let scope_owner = |scope: LexicalScopeId| {
            checked_scope_owner.get(&scope).copied().ok_or_else(|| {
                CallableDependencyManifestError::new(format!(
                    "dependency ownership references missing checked scope {}",
                    scope.0
                ))
            })
        };

        if execution.checked_expression_origins.len() != execution.expressions.len() {
            return Err(CallableDependencyManifestError::new(
                "semantic expression-origin table is not total",
            ));
        }
        let mut expression_owner = Vec::with_capacity(execution.expressions.len());
        for (index, expression) in execution.expressions.iter().enumerate() {
            if expression.id != SemanticExprId(index) {
                return Err(CallableDependencyManifestError::new(format!(
                    "semantic expression {} is not dense index {index}",
                    expression.id
                )));
            }
            let origin = execution
                .checked_expression_origins
                .get(index)
                .filter(|origin| origin.expression == expression.id)
                .ok_or_else(|| {
                    CallableDependencyManifestError::new(format!(
                        "semantic expression {} has no exact checked origin",
                        expression.id
                    ))
                })?;
            expression_owner.push(scope_owner(origin.checked_scope)?);
        }

        let mut statement_owner = Vec::with_capacity(execution.statements.len());
        for (index, statement) in execution.statements.iter().enumerate() {
            if statement.id != SemanticStatementId(index) {
                return Err(CallableDependencyManifestError::new(format!(
                    "semantic statement {} is not dense index {index}",
                    statement.id
                )));
            }
            let scope = execution
                .scopes
                .get(statement.scope.as_usize())
                .filter(|scope| scope.id == statement.scope)
                .ok_or_else(|| {
                    CallableDependencyManifestError::new(format!(
                        "semantic statement {} references missing scope {}",
                        statement.id, statement.scope
                    ))
                })?;
            statement_owner.push(scope_owner(scope.checked_scope)?);
        }

        let semantic_owner_for_checked = |owner: Option<DeclId>, context: &str| {
            owner.map_or(Ok(SemanticDependencyOwnerV1::ProgramRoot), |owner| {
                callable_by_checked
                    .get(&owner)
                    .copied()
                    .map(|callable| SemanticDependencyOwnerV1::Callable { callable })
                    .ok_or_else(|| {
                        CallableDependencyManifestError::new(format!(
                            "{context} references checked owner {} with no semantic callable identity",
                            owner.0
                        ))
                    })
            })
        };

        let mut checked_statement_owner = BTreeMap::new();
        for statement in &checked.statements {
            let owner = scope_owner(statement.scope_id)?;
            if checked_statement_owner
                .insert(statement.id, owner)
                .is_some()
            {
                return Err(CallableDependencyManifestError::new(format!(
                    "checked statement {} appears more than once",
                    statement.id.0
                )));
            }
        }

        let mut call_owner = Vec::with_capacity(execution.calls.len());
        for (index, call) in execution.calls.iter().enumerate() {
            if call.id != SemanticCallId(index) {
                return Err(CallableDependencyManifestError::new(format!(
                    "semantic call {} is not dense index {index}",
                    call.id
                )));
            }
            let owner = call
                .owner_callable
                .map_or(SemanticDependencyOwnerV1::ProgramRoot, |callable| {
                    SemanticDependencyOwnerV1::Callable { callable }
                });
            call_owner.push(owner);
        }

        let mut producer_root_owner = BTreeMap::new();
        for producer in out.producer_roots() {
            let owner = semantic_owner_for_checked(
                Some(producer.spec.callable),
                &format!("producer root {:?}", producer.spec.identity),
            )?;
            if producer_root_owner.insert(producer.call, owner).is_some() {
                return Err(CallableDependencyManifestError::new(format!(
                    "producer OUT call {} is rooted more than once",
                    producer.call
                )));
            }
        }

        let mut out_call_owner = Vec::with_capacity(out.call_instances.len());
        for (index, call) in out.call_instances.iter().enumerate() {
            if call.id != OutCallInstanceId(index) {
                return Err(CallableDependencyManifestError::new(format!(
                    "OUT call instance {} is not dense index {index}",
                    call.id
                )));
            }
            let mut root = call.id;
            let mut visited = BTreeSet::new();
            loop {
                if !visited.insert(root) {
                    return Err(CallableDependencyManifestError::new(format!(
                        "OUT call instance {} has cyclic concrete ancestry at {root}",
                        call.id
                    )));
                }
                let concrete = out
                    .call_instances
                    .get(root.as_usize())
                    .filter(|candidate| candidate.id == root)
                    .ok_or_else(|| {
                        CallableDependencyManifestError::new(format!(
                            "OUT call instance {} ancestry references missing call {root}",
                            call.id
                        ))
                    })?;
                let Some(parent) = concrete.parent else {
                    break;
                };
                root = parent;
            }
            out_call_owner.push(
                producer_root_owner
                    .get(&root)
                    .copied()
                    .unwrap_or(SemanticDependencyOwnerV1::ProgramRoot),
            );
        }

        let mut static_owner = BTreeMap::new();
        let mut attach_static_owner = |id: StaticOwnerId,
                                       owner: SemanticDependencyOwnerV1,
                                       context: &str|
         -> Result<(), CallableDependencyManifestError> {
            if let Some(previous) = static_owner.insert(id, owner)
                && previous != owner
            {
                return Err(CallableDependencyManifestError::new(format!(
                    "static owner {id} has conflicting primary owners {previous:?} and {owner:?} at {context}"
                )));
            }
            Ok(())
        };
        for call in &out.call_instances {
            if let Some(owner) = call.owner {
                attach_static_owner(
                    owner,
                    out_call_owner[call.id.as_usize()],
                    &format!("OUT call {}", call.id),
                )?;
            }
        }
        for net in &out.nets {
            if let Some(owner) = net.owner {
                let anchor = net.owner_anchor.ok_or_else(|| {
                    CallableDependencyManifestError::new(format!(
                        "OUT net {} has static owner {owner} without an exact port anchor",
                        net.id
                    ))
                })?;
                let port = out
                    .ports
                    .get(anchor.as_usize())
                    .filter(|port| port.id == anchor)
                    .ok_or_else(|| {
                        CallableDependencyManifestError::new(format!(
                            "OUT net {} owner anchor references missing port {anchor}",
                            net.id
                        ))
                    })?;
                attach_static_owner(
                    owner,
                    out_call_owner[port.call.as_usize()],
                    &format!("OUT net {}", net.id),
                )?;
            }
        }
        for owner in &out.static_owners {
            if !static_owner.contains_key(&owner.id) {
                return Err(CallableDependencyManifestError::new(format!(
                    "static owner {} has no exact call/net anchor; dependency ownership requires an engine identity",
                    owner.id
                )));
            }
        }

        // A semantic expression is an executable occurrence, not merely its
        // checked lexical definition. Its concrete call-root owns the
        // occurrence: the program root for ordinary expansion, or the producer
        // callable for a synthetic producer root. Static and frame coordinates
        // must agree whenever both are present. The checked expression remains
        // a referenced definition dependency under its lexical callable.
        for expression in &execution.expressions {
            let origin = execution
                .checked_expression_origins
                .get(expression.id.as_usize())
                .filter(|origin| origin.expression == expression.id)
                .ok_or_else(|| {
                    CallableDependencyManifestError::new(format!(
                        "semantic expression {} has no exact checked origin",
                        expression.id
                    ))
                })?;
            let static_primary = expression
                .owner
                .map(|owner| {
                    static_owner.get(&owner).copied().ok_or_else(|| {
                        CallableDependencyManifestError::new(format!(
                            "semantic expression {} references unanchored static owner {owner}",
                            expression.id
                        ))
                    })
                })
                .transpose()?;
            let frame_primary = origin
                .call_instance
                .map(|frame| {
                    out_call_owner
                        .get(frame.as_usize())
                        .copied()
                        .ok_or_else(|| {
                            CallableDependencyManifestError::new(format!(
                                "semantic expression {} references missing call frame {frame}",
                                expression.id
                            ))
                        })
                })
                .transpose()?;
            let concrete_primary = match (static_primary, frame_primary) {
                (Some(static_owner), Some(frame_owner)) if static_owner != frame_owner => {
                    return Err(CallableDependencyManifestError::new(format!(
                        "semantic expression {} has conflicting static {static_owner:?} and call-frame {frame_owner:?} occurrence owners",
                        expression.id
                    )));
                }
                (Some(owner), _) | (None, Some(owner)) => Some(owner),
                (None, None) => None,
            };
            if let Some(owner) = concrete_primary {
                expression_owner[expression.id.as_usize()] = owner;
            }
        }

        let expression_owner_at = |expression: SemanticExprId, context: &str| {
            expression_owner
                .get(expression.as_usize())
                .copied()
                .ok_or_else(|| {
                    CallableDependencyManifestError::new(format!(
                        "{context} references missing semantic expression {expression}"
                    ))
                })
        };
        let mut source_owner = Vec::with_capacity(resources.sources.len());
        for source in &resources.sources {
            source_owner.push(expression_owner_at(
                source.expression,
                &format!("semantic source {}", source.id),
            )?);
        }
        let mut state_owner = Vec::with_capacity(resources.states.len());
        for state in &resources.states {
            state_owner.push(expression_owner_at(
                state.expression,
                &format!("semantic state {}", state.id),
            )?);
        }
        let mut list_owner = Vec::with_capacity(resources.lists.len());
        for list in &resources.lists {
            list_owner.push(expression_owner_at(
                list.producer,
                &format!("semantic list {}", list.id),
            )?);
        }
        let mut value_list_owner = Vec::with_capacity(resources.value_list_authorities.len());
        for list in &resources.value_list_authorities {
            value_list_owner.push(expression_owner_at(
                list.producer,
                &format!("semantic value-list authority {}", list.id),
            )?);
        }

        let mut binding_owner = Vec::with_capacity(reactive.bindings.len());
        for binding in &reactive.bindings {
            binding_owner.push(expression_owner_at(
                binding.producer,
                &format!("reactive binding {}", binding.id),
            )?);
        }

        let owner_for_storage_origin = |field: &SemanticStorageFieldV1| {
            let mut candidates = Vec::new();
            if let Some(expression) = field.producer {
                candidates.push(expression_owner_at(
                    expression,
                    &format!("storage field {}", field.id),
                )?);
            }
            let explicit_owner = field
                .owner
                .map(|owner| {
                    static_owner.get(&owner).copied().ok_or_else(|| {
                        CallableDependencyManifestError::new(format!(
                            "storage field {} references unanchored static owner {owner}",
                            field.id
                        ))
                    })
                })
                .transpose()?;
            match &field.origin {
                SemanticStorageFieldOriginV1::Reactive {
                    field: reactive_field,
                } => {
                    let reactive_field = reactive
                        .fields
                        .get(reactive_field.as_usize())
                        .filter(|candidate| candidate.id == *reactive_field)
                        .ok_or_else(|| {
                            CallableDependencyManifestError::new(format!(
                                "storage field {} references missing reactive field {reactive_field}",
                                field.id
                            ))
                        })?;
                    candidates.push(expression_owner_at(
                        reactive_field.producer,
                        &format!("storage field {}", field.id),
                    )?);
                }
                SemanticStorageFieldOriginV1::StateAuthority { state } => {
                    candidates.push(*state_owner.get(state.as_usize()).ok_or_else(|| {
                        CallableDependencyManifestError::new(format!(
                            "storage field {} references missing state {state}",
                            field.id
                        ))
                    })?);
                }
                SemanticStorageFieldOriginV1::ListAuthority { list, .. } => {
                    candidates.push(*list_owner.get(list.as_usize()).ok_or_else(|| {
                        CallableDependencyManifestError::new(format!(
                            "storage field {} references missing list {list}",
                            field.id
                        ))
                    })?);
                }
                SemanticStorageFieldOriginV1::ValueListAuthority { authority, .. } => {
                    candidates
                        .push(*value_list_owner.get(authority.as_usize()).ok_or_else(|| {
                        CallableDependencyManifestError::new(format!(
                            "storage field {} references missing value-list authority {authority}",
                            field.id
                        ))
                    })?);
                }
                SemanticStorageFieldOriginV1::RecordProjection { expression, .. } => {
                    candidates.push(expression_owner_at(
                        *expression,
                        &format!("storage field {}", field.id),
                    )?);
                }
                SemanticStorageFieldOriginV1::DetachedCapture { target_owner, .. } => {
                    candidates.push(*static_owner.get(target_owner).ok_or_else(|| {
                        CallableDependencyManifestError::new(format!(
                            "storage field {} detached capture references unanchored owner {target_owner}",
                            field.id
                        ))
                    })?);
                }
            }
            explicit_owner.map_or_else(
                || exact_owner(candidates, &format!("storage field {}", field.id)),
                Ok,
            )
        };

        let mut storage_field_owner = Vec::with_capacity(storage.fields.len());
        for (index, field) in storage.fields.iter().enumerate() {
            if field.id != SemanticStorageFieldId(index) {
                return Err(CallableDependencyManifestError::new(format!(
                    "semantic storage field {} is not dense index {index}",
                    field.id
                )));
            }
            storage_field_owner.push(owner_for_storage_origin(field)?);
        }

        let mut memory_owner = Vec::with_capacity(memory.memories.len());
        for memory in &memory.memories {
            let binding = memory.backing.binding();
            memory_owner.push(*binding_owner.get(binding.as_usize()).ok_or_else(|| {
                CallableDependencyManifestError::new(format!(
                    "semantic memory {} backing references missing binding {binding}",
                    memory.id
                ))
            })?);
        }

        Ok(Self {
            callable_by_checked,
            checked_scope_owner,
            checked_statement_owner,
            expression_owner,
            statement_owner,
            call_owner,
            out_call_owner,
            static_owner,
            source_owner,
            state_owner,
            list_owner,
            value_list_owner,
            binding_owner,
            storage_field_owner,
            memory_owner,
        })
    }

    fn checked_scope(
        &self,
        scope: LexicalScopeId,
    ) -> Result<SemanticDependencyOwnerV1, CallableDependencyManifestError> {
        self.checked_scope_owner
            .get(&scope)
            .copied()
            .ok_or_else(|| {
                CallableDependencyManifestError::new(format!(
                    "dependency ownership references missing checked scope {}",
                    scope.0
                ))
            })
    }

    fn checked_statement(
        &self,
        statement: boon_typecheck::CheckedStatementId,
    ) -> Result<SemanticDependencyOwnerV1, CallableDependencyManifestError> {
        self.checked_statement_owner
            .get(&statement)
            .copied()
            .ok_or_else(|| {
                CallableDependencyManifestError::new(format!(
                    "dependency ownership references missing checked statement {}",
                    statement.0
                ))
            })
    }

    fn expression(
        &self,
        expression: SemanticExprId,
    ) -> Result<SemanticDependencyOwnerV1, CallableDependencyManifestError> {
        self.expression_owner
            .get(expression.as_usize())
            .copied()
            .ok_or_else(|| {
                CallableDependencyManifestError::new(format!(
                    "dependency ownership references missing semantic expression {expression}"
                ))
            })
    }

    fn statement(
        &self,
        statement: SemanticStatementId,
    ) -> Result<SemanticDependencyOwnerV1, CallableDependencyManifestError> {
        self.statement_owner
            .get(statement.as_usize())
            .copied()
            .ok_or_else(|| {
                CallableDependencyManifestError::new(format!(
                    "dependency ownership references missing semantic statement {statement}"
                ))
            })
    }

    fn call(
        &self,
        call: SemanticCallId,
    ) -> Result<SemanticDependencyOwnerV1, CallableDependencyManifestError> {
        self.call_owner
            .get(call.as_usize())
            .copied()
            .ok_or_else(|| {
                CallableDependencyManifestError::new(format!(
                    "dependency ownership references missing semantic call {call}"
                ))
            })
    }

    fn out_call(
        &self,
        call: OutCallInstanceId,
    ) -> Result<SemanticDependencyOwnerV1, CallableDependencyManifestError> {
        self.out_call_owner
            .get(call.as_usize())
            .copied()
            .ok_or_else(|| {
                CallableDependencyManifestError::new(format!(
                    "dependency ownership references missing OUT call {call}"
                ))
            })
    }

    fn static_owner(
        &self,
        owner: StaticOwnerId,
    ) -> Result<SemanticDependencyOwnerV1, CallableDependencyManifestError> {
        self.static_owner.get(&owner).copied().ok_or_else(|| {
            CallableDependencyManifestError::new(format!(
                "dependency ownership references unanchored static owner {owner}"
            ))
        })
    }

    fn source(
        &self,
        source: SemanticSourceId,
    ) -> Result<SemanticDependencyOwnerV1, CallableDependencyManifestError> {
        indexed_owner(
            &self.source_owner,
            source.as_usize(),
            "semantic source",
            source,
        )
    }

    fn state(
        &self,
        state: SemanticStateId,
    ) -> Result<SemanticDependencyOwnerV1, CallableDependencyManifestError> {
        indexed_owner(&self.state_owner, state.as_usize(), "semantic state", state)
    }

    fn list(
        &self,
        list: SemanticListId,
    ) -> Result<SemanticDependencyOwnerV1, CallableDependencyManifestError> {
        indexed_owner(&self.list_owner, list.as_usize(), "semantic list", list)
    }

    fn value_list(
        &self,
        list: SemanticValueListAuthorityId,
    ) -> Result<SemanticDependencyOwnerV1, CallableDependencyManifestError> {
        indexed_owner(
            &self.value_list_owner,
            list.as_usize(),
            "semantic value-list authority",
            list,
        )
    }

    fn binding(
        &self,
        binding: SemanticBindingId,
    ) -> Result<SemanticDependencyOwnerV1, CallableDependencyManifestError> {
        indexed_owner(
            &self.binding_owner,
            binding.as_usize(),
            "semantic binding",
            binding,
        )
    }

    fn storage_field(
        &self,
        field: SemanticStorageFieldId,
    ) -> Result<SemanticDependencyOwnerV1, CallableDependencyManifestError> {
        indexed_owner(
            &self.storage_field_owner,
            field.as_usize(),
            "semantic storage field",
            field,
        )
    }

    fn memory(
        &self,
        memory: SemanticMemoryId,
    ) -> Result<SemanticDependencyOwnerV1, CallableDependencyManifestError> {
        indexed_owner(
            &self.memory_owner,
            memory.as_usize(),
            "semantic memory",
            memory,
        )
    }
}

fn indexed_owner(
    owners: &[SemanticDependencyOwnerV1],
    index: usize,
    kind: &str,
    id: impl fmt::Display,
) -> Result<SemanticDependencyOwnerV1, CallableDependencyManifestError> {
    owners.get(index).copied().ok_or_else(|| {
        CallableDependencyManifestError::new(format!(
            "dependency ownership references missing {kind} {id}"
        ))
    })
}

fn exact_owner(
    candidates: Vec<SemanticDependencyOwnerV1>,
    context: &str,
) -> Result<SemanticDependencyOwnerV1, CallableDependencyManifestError> {
    let candidates = candidates.into_iter().collect::<BTreeSet<_>>();
    let candidates = candidates.iter().copied().collect::<Vec<_>>();
    let [owner] = candidates.as_slice() else {
        return Err(CallableDependencyManifestError::new(format!(
            "{context} resolves to {} primary callable/root owners {candidates:?}; an explicit engine identity is required",
            candidates.len(),
        )));
    };
    Ok(*owner)
}

#[derive(Clone, Debug)]
enum PendingDependencyReference {
    Entity(SemanticDependencyEntityV1),
    Owner(SemanticDependencyOwnerV1),
}

#[derive(Clone, Debug)]
struct PendingDependencyRecord {
    id: SemanticDependencyRecordId,
    owner: SemanticDependencyOwnerV1,
    channel: SemanticDependencyChannelV1,
    roles: Vec<SemanticDependencyRoleV1>,
    subject: SemanticDependencySubjectV1,
    semantics: SemanticDependencySemanticsV1,
    payload_digest: [u8; 32],
    references: Vec<PendingDependencyReference>,
}

struct DependencyRecordInput<'a, P> {
    owner: SemanticDependencyOwnerV1,
    channel: SemanticDependencyChannelV1,
    roles: Vec<SemanticDependencyRoleV1>,
    subject: SemanticDependencySubjectV1,
    semantics: SemanticDependencySemanticsV1,
    payload: &'a P,
    references: Vec<PendingDependencyReference>,
}

macro_rules! collect_dependency {
    (
        $collector:expr,
        $owner:expr,
        $channel:expr,
        $roles:expr,
        $subject:expr,
        $semantics:expr,
        $payload:expr,
        $references:expr $(,)?
    ) => {
        $collector.dependency(DependencyRecordInput {
            owner: $owner,
            channel: $channel,
            roles: $roles,
            subject: $subject,
            semantics: $semantics,
            payload: $payload,
            references: $references,
        })
    };
}

type FinishedDependencyCollection = (
    Vec<SemanticDependencyRecordV1>,
    Vec<SemanticDependencyCoverageV1>,
    BTreeMap<SemanticDependencyOwnerV1, Vec<SemanticDependencyRecordId>>,
    BTreeMap<SemanticDependencyOwnerV1, Vec<SemanticDependencyRecordId>>,
);

type ExpressionDependency = (
    SemanticDependencyChannelV1,
    Vec<SemanticDependencyRoleV1>,
    Vec<String>,
    Vec<PendingDependencyReference>,
);

#[derive(Default)]
struct DependencyCollector {
    pending: Vec<PendingDependencyRecord>,
    coverage: Vec<SemanticDependencyCoverageV1>,
    subjects: BTreeSet<SemanticDependencySubjectV1>,
    dependencies_by_entity: BTreeMap<SemanticDependencyEntityV1, Vec<SemanticDependencyRecordId>>,
}

impl DependencyCollector {
    fn dependency<P: Serialize>(
        &mut self,
        input: DependencyRecordInput<'_, P>,
    ) -> Result<SemanticDependencyRecordId, CallableDependencyManifestError> {
        let DependencyRecordInput {
            owner,
            channel,
            mut roles,
            subject,
            semantics,
            payload,
            references,
        } = input;
        self.claim_subject(&subject)?;
        roles.sort();
        roles.dedup();
        if roles.is_empty() {
            return Err(CallableDependencyManifestError::new(format!(
                "dependency subject {subject:?} has no explicit semantic role"
            )));
        }
        let payload_digest = dependency_payload_digest(payload)?;
        let id = SemanticDependencyRecordId(self.pending.len());
        self.pending.push(PendingDependencyRecord {
            id,
            owner,
            channel,
            roles,
            subject: subject.clone(),
            semantics,
            payload_digest,
            references,
        });
        self.dependencies_by_entity
            .entry(subject.identity.clone())
            .or_default()
            .push(id);
        self.coverage.push(SemanticDependencyCoverageV1 {
            id: SemanticDependencyCoverageId(self.coverage.len()),
            subject,
            primary_owner: owner,
            disposition: SemanticDependencyCoverageDispositionV1::Dependency { dependency: id },
        });
        Ok(id)
    }

    fn structural(
        &mut self,
        owner: SemanticDependencyOwnerV1,
        subject: SemanticDependencySubjectV1,
        payload: &impl Serialize,
    ) -> Result<(), CallableDependencyManifestError> {
        self.claim_subject(&subject)?;
        self.coverage.push(SemanticDependencyCoverageV1 {
            id: SemanticDependencyCoverageId(self.coverage.len()),
            subject,
            primary_owner: owner,
            disposition: SemanticDependencyCoverageDispositionV1::Structural {
                payload_digest: dependency_payload_digest(payload)?,
            },
        });
        Ok(())
    }

    fn diagnostic(
        &mut self,
        owner: SemanticDependencyOwnerV1,
        subject: SemanticDependencySubjectV1,
        payload: &impl Serialize,
    ) -> Result<(), CallableDependencyManifestError> {
        self.claim_subject(&subject)?;
        self.coverage.push(SemanticDependencyCoverageV1 {
            id: SemanticDependencyCoverageId(self.coverage.len()),
            subject,
            primary_owner: owner,
            disposition: SemanticDependencyCoverageDispositionV1::Diagnostic {
                payload_digest: dependency_payload_digest(payload)?,
            },
        });
        Ok(())
    }

    fn claim_subject(
        &mut self,
        subject: &SemanticDependencySubjectV1,
    ) -> Result<(), CallableDependencyManifestError> {
        if !self.subjects.insert(subject.clone()) {
            return Err(CallableDependencyManifestError::new(format!(
                "dependency coverage subject {subject:?} is classified more than once"
            )));
        }
        Ok(())
    }

    fn finish(
        mut self,
        owners: &BTreeSet<SemanticDependencyOwnerV1>,
    ) -> Result<FinishedDependencyCollection, CallableDependencyManifestError> {
        for ids in self.dependencies_by_entity.values_mut() {
            ids.sort();
            ids.dedup();
        }

        let mut records = Vec::with_capacity(self.pending.len());
        for pending in self.pending {
            let mut referenced_dependencies = Vec::new();
            let mut referenced_owners = Vec::new();
            for reference in pending.references {
                match reference {
                    PendingDependencyReference::Entity(entity) => {
                        let targets =
                            self.dependencies_by_entity.get(&entity).ok_or_else(|| {
                                CallableDependencyManifestError::new(format!(
                                    "dependency {} references entity {entity:?} with no dependency classification",
                                    pending.id
                                ))
                            })?;
                        referenced_dependencies.extend(targets.iter().copied());
                    }
                    PendingDependencyReference::Owner(owner) => {
                        if !owners.contains(&owner) {
                            return Err(CallableDependencyManifestError::new(format!(
                                "dependency {} references missing owner {owner:?}",
                                pending.id
                            )));
                        }
                        referenced_owners.push(owner);
                    }
                }
            }
            referenced_dependencies.sort();
            referenced_dependencies.dedup();
            referenced_dependencies.retain(|dependency| *dependency != pending.id);
            referenced_owners.sort();
            referenced_owners.dedup();
            referenced_owners.retain(|owner| *owner != pending.owner);
            records.push(SemanticDependencyRecordV1 {
                id: pending.id,
                owner: pending.owner,
                channel: pending.channel,
                roles: pending.roles,
                subject: pending.subject,
                semantics: pending.semantics,
                payload_digest: pending.payload_digest,
                referenced_dependencies,
                referenced_owners,
            });
        }

        let mut direct = owners
            .iter()
            .copied()
            .map(|owner| (owner, Vec::new()))
            .collect::<BTreeMap<_, _>>();
        for record in &records {
            direct
                .get_mut(&record.owner)
                .ok_or_else(|| {
                    CallableDependencyManifestError::new(format!(
                        "dependency {} has unregistered owner {:?}",
                        record.id, record.owner
                    ))
                })?
                .push(record.id);
        }
        for ids in direct.values_mut() {
            ids.sort();
            ids.dedup();
        }

        let mut closure = BTreeMap::new();
        for owner in owners {
            let mut pending = direct.get(owner).cloned().unwrap_or_default();
            let mut complete = BTreeSet::new();
            while let Some(dependency) = pending.pop() {
                if !complete.insert(dependency) {
                    continue;
                }
                let record = records
                    .get(dependency.as_usize())
                    .filter(|record| record.id == dependency)
                    .ok_or_else(|| {
                        CallableDependencyManifestError::new(format!(
                            "dependency closure references missing record {dependency}"
                        ))
                    })?;
                pending.extend(record.referenced_dependencies.iter().copied());
                for referenced_owner in &record.referenced_owners {
                    pending.extend(
                        direct
                            .get(referenced_owner)
                            .ok_or_else(|| {
                                CallableDependencyManifestError::new(format!(
                                    "dependency closure references missing owner {referenced_owner:?}"
                                ))
                            })?
                            .iter()
                            .copied(),
                    );
                }
            }
            closure.insert(*owner, complete.into_iter().collect());
        }

        for (index, coverage) in self.coverage.iter().enumerate() {
            if coverage.id != SemanticDependencyCoverageId(index) {
                return Err(CallableDependencyManifestError::new(
                    "dependency coverage IDs are not dense",
                ));
            }
        }
        Ok((records, self.coverage, direct, closure))
    }
}

fn top_subject(
    kind: SemanticDependencySubjectKindV1,
    identity: SemanticDependencyEntityV1,
) -> SemanticDependencySubjectV1 {
    SemanticDependencySubjectV1 {
        kind,
        identity,
        parent: None,
        child_ordinal: None,
    }
}

fn child_subject(
    kind: SemanticDependencySubjectKindV1,
    parent: SemanticDependencyEntityV1,
    ordinal: usize,
) -> SemanticDependencySubjectV1 {
    SemanticDependencySubjectV1 {
        kind,
        identity: parent.clone(),
        parent: Some(parent),
        child_ordinal: Some(ordinal),
    }
}

fn indexed_entity(
    domain: SemanticDependencyEntityDomainV1,
    index: usize,
) -> SemanticDependencyEntityV1 {
    SemanticDependencyEntityV1::indexed(domain, index)
}

fn expression_entity(expression: SemanticExprId) -> SemanticDependencyEntityV1 {
    indexed_entity(
        SemanticDependencyEntityDomainV1::SemanticExpression,
        expression.as_usize(),
    )
}

fn statement_entity(statement: SemanticStatementId) -> SemanticDependencyEntityV1 {
    indexed_entity(
        SemanticDependencyEntityDomainV1::SemanticStatement,
        statement.as_usize(),
    )
}

fn callable_entity(callable: SemanticCallableId) -> SemanticDependencyEntityV1 {
    indexed_entity(
        SemanticDependencyEntityDomainV1::SemanticCallable,
        callable.as_usize(),
    )
}

fn call_entity(call: SemanticCallId) -> SemanticDependencyEntityV1 {
    indexed_entity(
        SemanticDependencyEntityDomainV1::SemanticCall,
        call.as_usize(),
    )
}

fn out_call_entity(call: OutCallInstanceId) -> SemanticDependencyEntityV1 {
    indexed_entity(
        SemanticDependencyEntityDomainV1::OutCallInstance,
        call.as_usize(),
    )
}

fn source_entity(source: SemanticSourceId) -> SemanticDependencyEntityV1 {
    indexed_entity(
        SemanticDependencyEntityDomainV1::SemanticSource,
        source.as_usize(),
    )
}

fn state_entity(state: SemanticStateId) -> SemanticDependencyEntityV1 {
    indexed_entity(
        SemanticDependencyEntityDomainV1::SemanticState,
        state.as_usize(),
    )
}

fn list_entity(list: SemanticListId) -> SemanticDependencyEntityV1 {
    indexed_entity(
        SemanticDependencyEntityDomainV1::SemanticList,
        list.as_usize(),
    )
}

fn value_list_entity(list: SemanticValueListAuthorityId) -> SemanticDependencyEntityV1 {
    indexed_entity(
        SemanticDependencyEntityDomainV1::SemanticValueListAuthority,
        list.as_usize(),
    )
}

fn binding_entity(binding: SemanticBindingId) -> SemanticDependencyEntityV1 {
    indexed_entity(
        SemanticDependencyEntityDomainV1::SemanticBinding,
        binding.as_usize(),
    )
}

fn read_entity(read: SemanticReadId) -> SemanticDependencyEntityV1 {
    indexed_entity(
        SemanticDependencyEntityDomainV1::SemanticRead,
        read.as_usize(),
    )
}

fn field_entity(field: SemanticFieldId) -> SemanticDependencyEntityV1 {
    indexed_entity(
        SemanticDependencyEntityDomainV1::SemanticField,
        field.as_usize(),
    )
}

fn static_owner_entity(owner: StaticOwnerId) -> SemanticDependencyEntityV1 {
    indexed_entity(
        SemanticDependencyEntityDomainV1::StaticOwner,
        owner.as_usize(),
    )
}

fn storage_local_entity(
    owner: StaticOwnerId,
    local: SemanticMaterializationLocalId,
) -> SemanticDependencyEntityV1 {
    SemanticDependencyEntityV1::Composite {
        domain: SemanticDependencyEntityDomainV1::SemanticStorageLocal,
        parent_index: owner.as_usize() as u64,
        child_ordinal: local.as_usize(),
    }
}

fn view_root_entity(root: SemanticViewRootId) -> SemanticDependencyEntityV1 {
    indexed_entity(
        SemanticDependencyEntityDomainV1::SemanticViewRoot,
        root.as_usize(),
    )
}

fn view_node_entity(node: SemanticViewNodeId) -> SemanticDependencyEntityV1 {
    indexed_entity(
        SemanticDependencyEntityDomainV1::SemanticViewNode,
        node.as_usize(),
    )
}

fn view_argument_entity(argument: SemanticViewArgumentId) -> SemanticDependencyEntityV1 {
    indexed_entity(
        SemanticDependencyEntityDomainV1::SemanticViewArgument,
        argument.as_usize(),
    )
}

fn view_binding_entity(binding: SemanticViewBindingId) -> SemanticDependencyEntityV1 {
    indexed_entity(
        SemanticDependencyEntityDomainV1::SemanticViewBinding,
        binding.as_usize(),
    )
}

fn dependency_entity(entity: SemanticDependencyEntityV1) -> PendingDependencyReference {
    PendingDependencyReference::Entity(entity)
}

fn dependency_owner(owner: SemanticDependencyOwnerV1) -> PendingDependencyReference {
    PendingDependencyReference::Owner(owner)
}

fn dependency_payload_digest(
    payload: &impl Serialize,
) -> Result<[u8; 32], CallableDependencyManifestError> {
    boon_contract::canonical_serde_hash_v1(DEPENDENCY_RECORD_PAYLOAD_DOMAIN, payload).map_err(
        |error| {
            CallableDependencyManifestError::new(format!(
                "failed to hash dependency record payload: {error}"
            ))
        },
    )
}

fn canonical_dependency_hash(
    domain: &[u8],
    payload: &impl Serialize,
) -> Result<[u8; 32], CallableDependencyManifestError> {
    boon_contract::canonical_serde_hash_v1(domain, payload).map_err(|error| {
        CallableDependencyManifestError::new(format!(
            "failed to hash callable dependency payload: {error}"
        ))
    })
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn build_callable_dependency_manifest(
    dependency_classifier_schema_digest: [u8; 32],
    checked: &CheckedProgram,
    producer_materializations: &[ProducerMaterializationRequest],
    out: &ResolvedOutGraph,
    execution: &SemanticExecutionGraphV1,
    resources: &SemanticResourceGraphV1,
    reactive: &SemanticReactiveGraphV1,
    lowering: &SemanticLoweringContractV1,
    view: &SemanticViewBindingGraphV1,
    storage: &SemanticScopeStorageGraphV1,
    memory: &SemanticMemoryGraphV1,
) -> Result<CallableDependencyManifestV1, CallableDependencyManifestError> {
    if checked.source_bundle_digest_v1 != lowering.metadata.source_bundle_digest_v1
        || checked.source_bundle_digest_v1 != view.source_bundle_digest_v1
        || checked.source_bundle_digest_v1 != storage.source_bundle_digest_v1
        || checked.source_bundle_digest_v1 != memory.source_bundle_digest_v1
    {
        return Err(CallableDependencyManifestError::new(
            "dependency manifest inputs disagree on source-bundle identity",
        ));
    }

    let owner_index = DependencyOwnerIndex::derive(
        checked, out, execution, resources, reactive, storage, memory,
    )?;
    let mut owners = BTreeSet::from([SemanticDependencyOwnerV1::ProgramRoot]);
    owners.extend(
        execution
            .callables
            .iter()
            .map(|callable| SemanticDependencyOwnerV1::Callable {
                callable: callable.id,
            }),
    );

    let mut collector = DependencyCollector::default();
    inventory_checked(checked, execution, &owner_index, &mut collector)?;
    inventory_producer_requests(producer_materializations, &owner_index, &mut collector)?;
    inventory_out(out, execution, &owner_index, &mut collector)?;
    inventory_execution(execution, &owner_index, &mut collector)?;
    inventory_resources(resources, execution, &owner_index, &mut collector)?;
    inventory_reactive(reactive, execution, &owner_index, &mut collector)?;
    inventory_lowering(lowering, execution, resources, &owner_index, &mut collector)?;
    inventory_view(view, execution, reactive, &owner_index, &mut collector)?;
    inventory_storage(
        storage,
        execution,
        resources,
        reactive,
        &owner_index,
        &mut collector,
    )?;
    inventory_memory(memory, execution, &owner_index, &mut collector)?;

    let (dependencies, coverage, direct, closure) = collector.finish(&owners)?;
    let component_digests = CallableDependencyComponentDigestsV1 {
        producer_materializations: canonical_dependency_hash(
            DEPENDENCY_COMPONENT_DIGEST_DOMAIN,
            &producer_materializations,
        )?,
        resolved_out_graph: canonical_dependency_hash(DEPENDENCY_COMPONENT_DIGEST_DOMAIN, out)?,
        execution_graph: canonical_dependency_hash(DEPENDENCY_COMPONENT_DIGEST_DOMAIN, execution)?,
        resource_graph: canonical_dependency_hash(DEPENDENCY_COMPONENT_DIGEST_DOMAIN, resources)?,
        reactive_graph: canonical_dependency_hash(DEPENDENCY_COMPONENT_DIGEST_DOMAIN, reactive)?,
        lowering_contract: canonical_dependency_hash(DEPENDENCY_COMPONENT_DIGEST_DOMAIN, lowering)?,
        view_binding_graph: canonical_dependency_hash(DEPENDENCY_COMPONENT_DIGEST_DOMAIN, view)?,
        scope_storage_graph: canonical_dependency_hash(
            DEPENDENCY_COMPONENT_DIGEST_DOMAIN,
            storage,
        )?,
        memory_graph: canonical_dependency_hash(DEPENDENCY_COMPONENT_DIGEST_DOMAIN, memory)?,
    };

    let root_owner = SemanticDependencyOwnerV1::ProgramRoot;
    let root_direct = direct.get(&root_owner).cloned().unwrap_or_default();
    let root_closure = closure.get(&root_owner).cloned().unwrap_or_default();
    let program_root = ProgramRootDependencyEntryV1 {
        public_shape_digest: canonical_dependency_hash(
            DEPENDENCY_PUBLIC_SHAPE_DOMAIN,
            &(checked.role, checked.source_bundle_digest_v1),
        )?,
        implementation_dependency_digest: implementation_dependency_digest(
            root_owner,
            &root_closure,
            &dependencies,
        )?,
        direct_dependency_ids: root_direct,
        closure_dependency_ids: root_closure,
    };

    let mut callable_entries = Vec::with_capacity(execution.callables.len());
    for callable in &execution.callables {
        let owner = SemanticDependencyOwnerV1::Callable {
            callable: callable.id,
        };
        let direct_dependency_ids = direct.get(&owner).cloned().ok_or_else(|| {
            CallableDependencyManifestError::new(format!(
                "callable {} has no initialized dependency owner entry",
                callable.id
            ))
        })?;
        let closure_dependency_ids = closure.get(&owner).cloned().ok_or_else(|| {
            CallableDependencyManifestError::new(format!(
                "callable {} has no initialized dependency closure",
                callable.id
            ))
        })?;
        callable_entries.push(CallableDependencyEntryV1 {
            callable: callable.id,
            checked_callable: callable.checked_callable,
            public_shape_digest: callable_public_shape_digest(callable)?,
            implementation_dependency_digest: implementation_dependency_digest(
                owner,
                &closure_dependency_ids,
                &dependencies,
            )?,
            direct_dependency_ids,
            closure_dependency_ids,
        });
    }

    let checked_program_digest = CheckedProgramDigestV1(canonical_dependency_hash(
        CHECKED_PROGRAM_DIGEST_DOMAIN,
        checked,
    )?);
    let mut manifest = CallableDependencyManifestV1 {
        schema: CALLABLE_DEPENDENCY_MANIFEST_SCHEMA_V1.to_owned(),
        source_bundle_digest_v1: checked.source_bundle_digest_v1,
        checked_program_digest,
        dependency_classifier_schema_digest: DependencyClassifierSchemaDigestV1(
            dependency_classifier_schema_digest,
        ),
        component_digests,
        program_root,
        callable_entries,
        dependencies,
        coverage,
        manifest_digest: CallableDependencyManifestDigestV1([0; 32]),
    };
    validate_manifest_shape(&manifest, &owners)?;
    manifest.manifest_digest = callable_dependency_manifest_digest(&manifest)?;
    Ok(manifest)
}

impl CallableDependencyManifestV1 {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn validate_against(
        &self,
        dependency_classifier_schema_digest: [u8; 32],
        checked: &CheckedProgram,
        producer_materializations: &[ProducerMaterializationRequest],
        out: &ResolvedOutGraph,
        execution: &SemanticExecutionGraphV1,
        resources: &SemanticResourceGraphV1,
        reactive: &SemanticReactiveGraphV1,
        lowering: &SemanticLoweringContractV1,
        view: &SemanticViewBindingGraphV1,
        storage: &SemanticScopeStorageGraphV1,
        memory: &SemanticMemoryGraphV1,
    ) -> Result<(), CallableDependencyManifestError> {
        let expected = build_callable_dependency_manifest(
            dependency_classifier_schema_digest,
            checked,
            producer_materializations,
            out,
            execution,
            resources,
            reactive,
            lowering,
            view,
            storage,
            memory,
        )?;
        if self != &expected {
            return Err(CallableDependencyManifestError::new(
                "callable dependency manifest differs from its deterministic checked+semantic rederivation",
            ));
        }
        Ok(())
    }
}

fn callable_public_shape_digest(
    callable: &SemanticCallable,
) -> Result<[u8; 32], CallableDependencyManifestError> {
    #[derive(Serialize)]
    struct PublicParameter<'a> {
        ordinal: usize,
        name: &'a str,
        kind: boon_typecheck::CheckedParameterKind,
        flow_type: &'a FlowType,
        requirement: &'a boon_typecheck::CheckedParameterRequirement,
        evaluation_scope: boon_typecheck::CheckedEvaluationScope,
    }
    #[derive(Serialize)]
    struct PublicCallable<'a> {
        kind: boon_typecheck::CheckedCallableKind,
        name: &'a str,
        external_identity: &'a Option<boon_typecheck::CheckedExternalDeclarationIdentityV1>,
        parameters: Vec<PublicParameter<'a>>,
        contexts: &'a [SemanticCallableContext],
        context_scheme: Option<&'a boon_typecheck::CheckedContextScheme>,
        result: &'a FlowType,
        role: ProgramRole,
        effect: boon_typecheck::CheckedEffectSummary,
        contextual_operation: &'a Option<boon_typecheck::CheckedContextualOperation>,
    }

    let parameters = callable
        .parameters
        .iter()
        .map(|parameter| PublicParameter {
            ordinal: parameter.ordinal,
            name: &parameter.name,
            kind: parameter.kind,
            flow_type: &parameter.flow_type,
            requirement: &parameter.requirement,
            evaluation_scope: parameter.evaluation_scope,
        })
        .collect();
    // The semantic callable retains the stable formal identity. The complete
    // principal scheme lives in the checked formal table and is included in
    // the checked-program digest; wiring supplies it to future theorem DTOs.
    canonical_dependency_hash(
        DEPENDENCY_PUBLIC_SHAPE_DOMAIN,
        &PublicCallable {
            kind: callable.kind,
            name: &callable.name,
            external_identity: &callable.external_identity,
            parameters,
            contexts: &callable.contexts,
            context_scheme: None,
            result: &callable.result,
            role: callable.role,
            effect: callable.effect,
            contextual_operation: &callable.contextual_operation,
        },
    )
}

fn implementation_dependency_digest(
    owner: SemanticDependencyOwnerV1,
    closure: &[SemanticDependencyRecordId],
    dependencies: &[SemanticDependencyRecordV1],
) -> Result<[u8; 32], CallableDependencyManifestError> {
    let records = closure
        .iter()
        .map(|id| {
            dependencies
                .get(id.as_usize())
                .filter(|record| record.id == *id)
                .ok_or_else(|| {
                    CallableDependencyManifestError::new(format!(
                        "implementation dependency closure references missing record {id}"
                    ))
                })
        })
        .collect::<Result<Vec<_>, _>>()?;
    canonical_dependency_hash(DEPENDENCY_IMPLEMENTATION_DOMAIN, &(owner, records))
}

fn validate_manifest_shape(
    manifest: &CallableDependencyManifestV1,
    owners: &BTreeSet<SemanticDependencyOwnerV1>,
) -> Result<(), CallableDependencyManifestError> {
    if manifest.schema != CALLABLE_DEPENDENCY_MANIFEST_SCHEMA_V1 {
        return Err(CallableDependencyManifestError::new(format!(
            "unsupported callable dependency manifest schema `{}`",
            manifest.schema
        )));
    }
    for (index, dependency) in manifest.dependencies.iter().enumerate() {
        if dependency.id != SemanticDependencyRecordId(index) {
            return Err(CallableDependencyManifestError::new(format!(
                "dependency record {} is not dense index {index}",
                dependency.id
            )));
        }
        if !owners.contains(&dependency.owner) {
            return Err(CallableDependencyManifestError::new(format!(
                "dependency {} references missing owner {:?}",
                dependency.id, dependency.owner
            )));
        }
        if dependency.roles.is_empty() || !dependency.roles.windows(2).all(|pair| pair[0] < pair[1])
        {
            return Err(CallableDependencyManifestError::new(format!(
                "dependency {} roles are empty, duplicated, or non-canonical",
                dependency.id
            )));
        }
        for referenced in &dependency.referenced_dependencies {
            if manifest
                .dependencies
                .get(referenced.as_usize())
                .is_none_or(|candidate| candidate.id != *referenced)
            {
                return Err(CallableDependencyManifestError::new(format!(
                    "dependency {} references missing dependency {referenced}",
                    dependency.id
                )));
            }
        }
    }
    let mut subjects = BTreeSet::new();
    for (index, coverage) in manifest.coverage.iter().enumerate() {
        if coverage.id != SemanticDependencyCoverageId(index) {
            return Err(CallableDependencyManifestError::new(format!(
                "dependency coverage {} is not dense index {index}",
                coverage.id
            )));
        }
        if !subjects.insert(&coverage.subject) {
            return Err(CallableDependencyManifestError::new(format!(
                "dependency subject {:?} has duplicate coverage",
                coverage.subject
            )));
        }
        if !owners.contains(&coverage.primary_owner) {
            return Err(CallableDependencyManifestError::new(format!(
                "coverage {} references missing owner {:?}",
                coverage.id, coverage.primary_owner
            )));
        }
        if let SemanticDependencyCoverageDispositionV1::Dependency { dependency } =
            coverage.disposition
        {
            let record = manifest
                .dependencies
                .get(dependency.as_usize())
                .filter(|record| record.id == dependency)
                .ok_or_else(|| {
                    CallableDependencyManifestError::new(format!(
                        "coverage {} references missing dependency {dependency}",
                        coverage.id
                    ))
                })?;
            if record.subject != coverage.subject || record.owner != coverage.primary_owner {
                return Err(CallableDependencyManifestError::new(format!(
                    "coverage {} disagrees with dependency {dependency}",
                    coverage.id
                )));
            }
        }
    }
    Ok(())
}

#[derive(Serialize)]
struct CallableDependencyManifestDigestPayload<'a> {
    schema: &'a str,
    source_bundle_digest_v1: SourceBundleDigestV1,
    checked_program_digest: CheckedProgramDigestV1,
    dependency_classifier_schema_digest: DependencyClassifierSchemaDigestV1,
    component_digests: &'a CallableDependencyComponentDigestsV1,
    program_root: &'a ProgramRootDependencyEntryV1,
    callable_entries: &'a [CallableDependencyEntryV1],
    dependencies: &'a [SemanticDependencyRecordV1],
    coverage: &'a [SemanticDependencyCoverageV1],
}

fn callable_dependency_manifest_digest(
    manifest: &CallableDependencyManifestV1,
) -> Result<CallableDependencyManifestDigestV1, CallableDependencyManifestError> {
    canonical_dependency_hash(
        DEPENDENCY_MANIFEST_DIGEST_DOMAIN,
        &CallableDependencyManifestDigestPayload {
            schema: &manifest.schema,
            source_bundle_digest_v1: manifest.source_bundle_digest_v1,
            checked_program_digest: manifest.checked_program_digest,
            dependency_classifier_schema_digest: manifest.dependency_classifier_schema_digest,
            component_digests: &manifest.component_digests,
            program_root: &manifest.program_root,
            callable_entries: &manifest.callable_entries,
            dependencies: &manifest.dependencies,
            coverage: &manifest.coverage,
        },
    )
    .map(CallableDependencyManifestDigestV1)
}

fn inventory_checked(
    checked: &CheckedProgram,
    execution: &SemanticExecutionGraphV1,
    owners: &DependencyOwnerIndex,
    collector: &mut DependencyCollector,
) -> Result<(), CallableDependencyManifestError> {
    collector.structural(
        SemanticDependencyOwnerV1::ProgramRoot,
        top_subject(
            SemanticDependencySubjectKindV1::CheckedProgram,
            SemanticDependencyEntityV1::Program,
        ),
        checked,
    )?;

    for scope in &checked.scopes {
        let owner = owners.checked_scope(scope.id)?;
        collect_dependency!(
            collector,
            owner,
            SemanticDependencyChannelV1::CoverageRouting,
            vec![SemanticDependencyRoleV1::CoverageOrRouting],
            top_subject(
                SemanticDependencySubjectKindV1::CheckedScope,
                SemanticDependencyEntityV1::checked(
                    SemanticDependencyEntityDomainV1::CheckedScope,
                    scope.id.0,
                ),
            ),
            SemanticDependencySemanticsV1::default(),
            scope,
            Vec::new(),
        )?;
    }

    for declaration in &checked.declarations {
        let owner = if declaration.kind == CheckedDeclarationKind::Function {
            let callable = owners
                .callable_by_checked
                .get(&declaration.id)
                .copied()
                .ok_or_else(|| {
                    CallableDependencyManifestError::new(format!(
                        "function declaration {} has no semantic callable identity",
                        declaration.id.0
                    ))
                })?;
            SemanticDependencyOwnerV1::Callable { callable }
        } else {
            owners.checked_scope(declaration.scope_id)?
        };
        collect_dependency!(
            collector,
            owner,
            declaration_channel(declaration.kind),
            declaration_roles(declaration.kind),
            top_subject(
                SemanticDependencySubjectKindV1::CheckedDeclaration,
                SemanticDependencyEntityV1::checked(
                    SemanticDependencyEntityDomainV1::CheckedDeclaration,
                    declaration.id.0,
                ),
            ),
            SemanticDependencySemanticsV1 {
                flow_type: Some(declaration.flow_type.clone()),
                ..SemanticDependencySemanticsV1::default()
            },
            declaration,
            declaration
                .value
                .map(|expression| {
                    vec![PendingDependencyReference::Entity(
                        SemanticDependencyEntityV1::checked(
                            SemanticDependencyEntityDomainV1::CheckedExpression,
                            expression.0,
                        ),
                    )]
                })
                .unwrap_or_default(),
        )?;
    }

    for statement in &checked.statements {
        let owner = owners.checked_scope(statement.scope_id)?;
        let entity = SemanticDependencyEntityV1::checked(
            SemanticDependencyEntityDomainV1::CheckedStatement,
            statement.id.0,
        );
        collect_dependency!(
            collector,
            owner,
            checked_statement_channel(&statement.kind),
            vec![
                SemanticDependencyRoleV1::FixedDefinition,
                SemanticDependencyRoleV1::CoverageOrRouting,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::CheckedStatement,
                entity.clone(),
            ),
            SemanticDependencySemanticsV1::default(),
            statement,
            statement
                .value
                .map(|expression| {
                    vec![PendingDependencyReference::Entity(
                        SemanticDependencyEntityV1::checked(
                            SemanticDependencyEntityDomainV1::CheckedExpression,
                            expression.0,
                        ),
                    )]
                })
                .unwrap_or_default(),
        )?;
        for (ordinal, resource) in statement.resources.iter().enumerate() {
            collector.structural(
                owner,
                child_subject(
                    SemanticDependencySubjectKindV1::CheckedResourceProjection,
                    entity.clone(),
                    ordinal,
                ),
                resource,
            )?;
        }
    }

    for expression in &checked.expressions {
        let owner = owners.checked_scope(expression.scope_id)?;
        let entity = SemanticDependencyEntityV1::checked(
            SemanticDependencyEntityDomainV1::CheckedExpression,
            expression.id.0,
        );
        let (channel, roles, projection, references) =
            checked_expression_dependency(&expression.kind)?;
        collect_dependency!(
            collector,
            owner,
            channel,
            roles,
            top_subject(
                SemanticDependencySubjectKindV1::CheckedExpression,
                entity.clone(),
            ),
            SemanticDependencySemanticsV1 {
                projection,
                flow_type: Some(expression.flow_type.clone()),
                ..SemanticDependencySemanticsV1::default()
            },
            expression,
            references,
        )?;
        match &expression.kind {
            CheckedExpressionKind::TextTemplate { segments } => {
                for (ordinal, segment) in segments.iter().enumerate() {
                    collector.structural(
                        owner,
                        child_subject(
                            SemanticDependencySubjectKindV1::CheckedLoweringSideTableEntry,
                            entity.clone(),
                            ordinal,
                        ),
                        segment,
                    )?;
                }
            }
            CheckedExpressionKind::TaggedObject { fields, .. }
            | CheckedExpressionKind::Object { fields } => {
                for (ordinal, field) in fields.iter().enumerate() {
                    collector.structural(
                        owner,
                        child_subject(
                            SemanticDependencySubjectKindV1::CheckedLoweringSideTableEntry,
                            entity.clone(),
                            ordinal,
                        ),
                        field,
                    )?;
                }
            }
            CheckedExpressionKind::MatchArm { pattern, .. } => {
                collector.structural(
                    owner,
                    child_subject(
                        SemanticDependencySubjectKindV1::CheckedPatternBinding,
                        entity,
                        0,
                    ),
                    pattern,
                )?;
            }
            _ => {}
        }
    }

    let semantic_callable_by_checked = execution
        .callables
        .iter()
        .map(|callable| (callable.checked_callable, callable))
        .collect::<BTreeMap<_, _>>();
    for callable in &checked.callables {
        let semantic = semantic_callable_by_checked
            .get(&callable.decl_id)
            .ok_or_else(|| {
                CallableDependencyManifestError::new(format!(
                    "checked callable {} has no exact semantic callable",
                    callable.decl_id.0
                ))
            })?;
        let owner = SemanticDependencyOwnerV1::Callable {
            callable: semantic.id,
        };
        let entity = SemanticDependencyEntityV1::indexed(
            SemanticDependencyEntityDomainV1::SemanticCallable,
            semantic.id.as_usize(),
        );
        collect_dependency!(
            collector,
            owner,
            SemanticDependencyChannelV1::CalledCallable,
            vec![
                SemanticDependencyRoleV1::FixedDefinition,
                SemanticDependencyRoleV1::CoverageOrRouting,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::CheckedCallable,
                SemanticDependencyEntityV1::checked(
                    SemanticDependencyEntityDomainV1::CheckedCallable,
                    callable.decl_id.0,
                ),
            ),
            SemanticDependencySemanticsV1 {
                flow_type: Some(callable.result.clone()),
                program_role: Some(callable.role),
                visibility: SemanticDependencyVisibilityV1::Public,
                ..SemanticDependencySemanticsV1::default()
            },
            callable,
            Vec::new(),
        )?;
        for (ordinal, parameter) in callable.parameters.iter().enumerate() {
            let (channel, roles) = parameter_dependency(parameter);
            collect_dependency!(
                collector,
                owner,
                channel,
                roles,
                child_subject(
                    SemanticDependencySubjectKindV1::CheckedCallableParameter,
                    entity.clone(),
                    ordinal,
                ),
                SemanticDependencySemanticsV1 {
                    flow_type: Some(parameter.flow_type.clone()),
                    evaluation_scope: Some(parameter.evaluation_scope),
                    visibility: SemanticDependencyVisibilityV1::Public,
                    multiplicity: match parameter.evaluation_scope {
                        CheckedEvaluationScope::Parent => SemanticDependencyMultiplicityV1::Once,
                        CheckedEvaluationScope::Output { .. } => {
                            SemanticDependencyMultiplicityV1::PerRowMaterialization
                        }
                    },
                    ..SemanticDependencySemanticsV1::default()
                },
                parameter,
                Vec::new(),
            )?;
        }
        for (ordinal, context) in callable.contexts.iter().enumerate() {
            collect_dependency!(
                collector,
                owner,
                SemanticDependencyChannelV1::CompilerContext,
                vec![
                    SemanticDependencyRoleV1::ResourceOrProviderBehavior,
                    SemanticDependencyRoleV1::CoverageOrRouting,
                ],
                child_subject(
                    SemanticDependencySubjectKindV1::CheckedCallableContext,
                    entity.clone(),
                    ordinal,
                ),
                SemanticDependencySemanticsV1 {
                    flow_type: Some(context.flow_type.clone()),
                    multiplicity: SemanticDependencyMultiplicityV1::PerRowMaterialization,
                    lifetime: SemanticDependencyLifetimeV1::Call,
                    ..SemanticDependencySemanticsV1::default()
                },
                context,
                Vec::new(),
            )?;
        }
    }

    for formal in &checked.context_formals {
        let callable = owners
            .callable_by_checked
            .get(&formal.callable)
            .copied()
            .ok_or_else(|| {
                CallableDependencyManifestError::new(format!(
                    "context formal {} references missing semantic callable for declaration {}",
                    formal.id.0, formal.callable.0
                ))
            })?;
        collect_dependency!(
            collector,
            SemanticDependencyOwnerV1::Callable { callable },
            SemanticDependencyChannelV1::PassedContextFormal,
            vec![
                SemanticDependencyRoleV1::FormulaBinder,
                SemanticDependencyRoleV1::CoverageOrRouting,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::CheckedContextFormal,
                SemanticDependencyEntityV1::checked(
                    SemanticDependencyEntityDomainV1::CheckedContextFormal,
                    formal.id.0,
                ),
            ),
            SemanticDependencySemanticsV1 {
                flow_type: Some(formal.scheme.flow_type.clone()),
                visibility: SemanticDependencyVisibilityV1::Public,
                ..SemanticDependencySemanticsV1::default()
            },
            formal,
            Vec::new(),
        )?;
    }

    let semantic_call_by_checked = execution
        .calls
        .iter()
        .map(|call| (call.checked_call, call))
        .collect::<BTreeMap<_, _>>();
    for call in &checked.calls {
        let semantic = semantic_call_by_checked.get(&call.id).ok_or_else(|| {
            CallableDependencyManifestError::new(format!(
                "checked call {} has no exact semantic call",
                call.id.0
            ))
        })?;
        let owner = owners.call(semantic.id)?;
        let call_entity = SemanticDependencyEntityV1::indexed(
            SemanticDependencyEntityDomainV1::SemanticCall,
            semantic.id.as_usize(),
        );
        let callee = execution
            .callables
            .get(semantic.callable.as_usize())
            .filter(|candidate| candidate.id == semantic.callable)
            .ok_or_else(|| {
                CallableDependencyManifestError::new(format!(
                    "semantic call {} references missing callable {}",
                    semantic.id, semantic.callable
                ))
            })?;
        collect_dependency!(
            collector,
            owner,
            SemanticDependencyChannelV1::CalledCallable,
            vec![
                SemanticDependencyRoleV1::ResourceOrProviderBehavior,
                SemanticDependencyRoleV1::CoverageOrRouting,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::CheckedCall,
                SemanticDependencyEntityV1::checked(
                    SemanticDependencyEntityDomainV1::CheckedCall,
                    call.id.0,
                ),
            ),
            SemanticDependencySemanticsV1 {
                flow_type: Some(call.result.clone()),
                program_role: Some(call.role),
                lifetime: SemanticDependencyLifetimeV1::Call,
                ..SemanticDependencySemanticsV1::default()
            },
            call,
            vec![
                PendingDependencyReference::Owner(SemanticDependencyOwnerV1::Callable {
                    callable: callee.id,
                }),
                PendingDependencyReference::Entity(SemanticDependencyEntityV1::checked(
                    SemanticDependencyEntityDomainV1::CheckedExpression,
                    call.expression.0,
                )),
            ],
        )?;
        for (ordinal, entry) in call.entries.iter().enumerate() {
            let (channel, flow, references) = checked_call_entry_dependency(entry);
            collect_dependency!(
                collector,
                owner,
                channel,
                vec![
                    SemanticDependencyRoleV1::FixedDefinition,
                    SemanticDependencyRoleV1::CoverageOrRouting,
                ],
                child_subject(
                    SemanticDependencySubjectKindV1::CheckedCallEntry,
                    call_entity.clone(),
                    ordinal,
                ),
                SemanticDependencySemanticsV1 {
                    flow_type: flow,
                    lifetime: SemanticDependencyLifetimeV1::Call,
                    ..SemanticDependencySemanticsV1::default()
                },
                entry,
                references,
            )?;
        }
        for (ordinal, context) in call.contexts.iter().enumerate() {
            collect_dependency!(
                collector,
                owner,
                SemanticDependencyChannelV1::CompilerContext,
                vec![
                    SemanticDependencyRoleV1::ResourceOrProviderBehavior,
                    SemanticDependencyRoleV1::CoverageOrRouting,
                ],
                child_subject(
                    SemanticDependencySubjectKindV1::CheckedCallContext,
                    call_entity.clone(),
                    ordinal,
                ),
                SemanticDependencySemanticsV1 {
                    lifetime: SemanticDependencyLifetimeV1::Call,
                    ..SemanticDependencySemanticsV1::default()
                },
                context,
                Vec::new(),
            )?;
        }
        for (ordinal, substitution) in call.contextual_substitutions.iter().enumerate() {
            collect_dependency!(
                collector,
                owner,
                SemanticDependencyChannelV1::TypeAndFlowInstance,
                vec![SemanticDependencyRoleV1::CoverageOrRouting],
                child_subject(
                    SemanticDependencySubjectKindV1::CheckedContextSubstitution,
                    call_entity.clone(),
                    ordinal,
                ),
                SemanticDependencySemanticsV1 {
                    lifetime: SemanticDependencyLifetimeV1::Call,
                    ..SemanticDependencySemanticsV1::default()
                },
                substitution,
                Vec::new(),
            )?;
        }
        for (ordinal, substitution) in call.type_substitutions.iter().enumerate() {
            collect_dependency!(
                collector,
                owner,
                SemanticDependencyChannelV1::TypeAndFlowInstance,
                vec![SemanticDependencyRoleV1::CoverageOrRouting],
                child_subject(
                    SemanticDependencySubjectKindV1::CheckedTypeSubstitution,
                    call_entity.clone(),
                    ordinal,
                ),
                SemanticDependencySemanticsV1 {
                    lifetime: SemanticDependencyLifetimeV1::Call,
                    ..SemanticDependencySemanticsV1::default()
                },
                substitution,
                Vec::new(),
            )?;
        }
    }

    for (ordinal, path) in checked.call_result_paths.iter().enumerate() {
        let call = semantic_call_by_checked.get(&path.call).ok_or_else(|| {
            CallableDependencyManifestError::new(format!(
                "checked result path references call {} without semantic identity",
                path.call.0
            ))
        })?;
        collect_dependency!(
            collector,
            owners.call(call.id)?,
            SemanticDependencyChannelV1::CoverageRouting,
            vec![SemanticDependencyRoleV1::CoverageOrRouting],
            top_subject(
                SemanticDependencySubjectKindV1::CheckedCallResultPath,
                SemanticDependencyEntityV1::indexed(
                    SemanticDependencyEntityDomainV1::CheckedCallResultPath,
                    ordinal,
                ),
            ),
            SemanticDependencySemanticsV1::default(),
            path,
            Vec::new(),
        )?;
    }

    for (ordinal, chain) in checked.order_chains.iter().enumerate() {
        let call = semantic_call_by_checked.get(&chain.call).ok_or_else(|| {
            CallableDependencyManifestError::new(format!(
                "checked order chain references call {} without semantic identity",
                chain.call.0
            ))
        })?;
        let owner = owners.call(call.id)?;
        let entity = SemanticDependencyEntityV1::indexed(
            SemanticDependencyEntityDomainV1::CheckedOrderChain,
            ordinal,
        );
        collect_dependency!(
            collector,
            owner,
            SemanticDependencyChannelV1::CoverageRouting,
            vec![SemanticDependencyRoleV1::CoverageOrRouting],
            top_subject(
                SemanticDependencySubjectKindV1::CheckedOrderChain,
                entity.clone(),
            ),
            SemanticDependencySemanticsV1::default(),
            chain,
            Vec::new(),
        )?;
        for (key_ordinal, key) in chain.chain.keys.iter().enumerate() {
            collect_dependency!(
                collector,
                owner,
                SemanticDependencyChannelV1::CoverageRouting,
                vec![SemanticDependencyRoleV1::CoverageOrRouting],
                child_subject(
                    SemanticDependencySubjectKindV1::CheckedOrderKey,
                    entity.clone(),
                    key_ordinal,
                ),
                SemanticDependencySemanticsV1::default(),
                key,
                vec![PendingDependencyReference::Entity(
                    SemanticDependencyEntityV1::checked(
                        SemanticDependencyEntityDomainV1::CheckedExpression,
                        key.key.0,
                    ),
                )],
            )?;
        }
    }

    for (ordinal, binding) in checked.pattern_bindings.iter().enumerate() {
        collect_dependency!(
            collector,
            owners.checked_scope(
                checked
                    .expressions
                    .get(binding.selector.0 as usize)
                    .ok_or_else(|| {
                        CallableDependencyManifestError::new(format!(
                            "pattern binding {} references missing selector {}",
                            binding.declaration.0, binding.selector.0
                        ))
                    })?
                    .scope_id,
            )?,
            SemanticDependencyChannelV1::LocalFact,
            vec![
                SemanticDependencyRoleV1::FixedDefinition,
                SemanticDependencyRoleV1::CoverageOrRouting,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::CheckedPatternBinding,
                SemanticDependencyEntityV1::indexed(
                    SemanticDependencyEntityDomainV1::CheckedPatternBinding,
                    ordinal,
                ),
            ),
            SemanticDependencySemanticsV1 {
                projection: binding.projection.clone(),
                ..SemanticDependencySemanticsV1::default()
            },
            binding,
            vec![PendingDependencyReference::Entity(
                SemanticDependencyEntityV1::checked(
                    SemanticDependencyEntityDomainV1::CheckedExpression,
                    binding.selector.0,
                ),
            )],
        )?;
    }

    for (ordinal, requirement) in checked.resource_projection_requirements.iter().enumerate() {
        let expression = checked
            .expressions
            .get(requirement.expression.0 as usize)
            .ok_or_else(|| {
                CallableDependencyManifestError::new(format!(
                    "resource projection requirement references missing expression {}",
                    requirement.expression.0
                ))
            })?;
        collect_dependency!(
            collector,
            owners.checked_scope(expression.scope_id)?,
            SemanticDependencyChannelV1::ResourceRead,
            vec![
                SemanticDependencyRoleV1::ResourceOrProviderBehavior,
                SemanticDependencyRoleV1::CoverageOrRouting,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::CheckedResourceProjection,
                SemanticDependencyEntityV1::indexed(
                    SemanticDependencyEntityDomainV1::CheckedResourceProjection,
                    ordinal,
                ),
            ),
            SemanticDependencySemanticsV1 {
                projection: requirement.projection.clone(),
                ..SemanticDependencySemanticsV1::default()
            },
            requirement,
            vec![
                PendingDependencyReference::Entity(SemanticDependencyEntityV1::checked(
                    SemanticDependencyEntityDomainV1::CheckedExpression,
                    requirement.expression.0,
                )),
                PendingDependencyReference::Entity(SemanticDependencyEntityV1::checked(
                    SemanticDependencyEntityDomainV1::CheckedDeclaration,
                    requirement.target.0,
                )),
            ],
        )?;
    }

    for source in &checked.sources {
        collect_dependency!(
            collector,
            owners.checked_scope(source.owner_scope)?,
            SemanticDependencyChannelV1::ResourceBehavior,
            vec![
                SemanticDependencyRoleV1::ResourceOrProviderBehavior,
                SemanticDependencyRoleV1::CoverageOrRouting,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::CheckedSource,
                SemanticDependencyEntityV1::checked(
                    SemanticDependencyEntityDomainV1::CheckedSource,
                    source.id.0,
                ),
            ),
            SemanticDependencySemanticsV1 {
                multiplicity: SemanticDependencyMultiplicityV1::PerEvent,
                lifetime: SemanticDependencyLifetimeV1::Event,
                phase: SemanticDependencyPhaseV1::EventPayload,
                ..SemanticDependencySemanticsV1::default()
            },
            source,
            Vec::new(),
        )?;
    }
    for state in &checked.states {
        collect_dependency!(
            collector,
            owners.checked_scope(state.owner_scope)?,
            SemanticDependencyChannelV1::ResourceBehavior,
            vec![
                SemanticDependencyRoleV1::ResourceOrProviderBehavior,
                SemanticDependencyRoleV1::CoverageOrRouting,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::CheckedState,
                SemanticDependencyEntityV1::checked(
                    SemanticDependencyEntityDomainV1::CheckedState,
                    state.id.0,
                ),
            ),
            SemanticDependencySemanticsV1 {
                flow_type: Some(state.flow_type.clone()),
                multiplicity: SemanticDependencyMultiplicityV1::PerTransition,
                lifetime: SemanticDependencyLifetimeV1::Snapshot,
                phase: SemanticDependencyPhaseV1::CurrentValue,
                ..SemanticDependencySemanticsV1::default()
            },
            state,
            Vec::new(),
        )?;
    }
    for list in &checked.lists {
        collect_dependency!(
            collector,
            owners.checked_scope(list.owner_scope)?,
            SemanticDependencyChannelV1::ResourceBehavior,
            vec![
                SemanticDependencyRoleV1::ResourceOrProviderBehavior,
                SemanticDependencyRoleV1::CoverageOrRouting,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::CheckedList,
                SemanticDependencyEntityV1::checked(
                    SemanticDependencyEntityDomainV1::CheckedList,
                    list.id.0,
                ),
            ),
            SemanticDependencySemanticsV1 {
                multiplicity: SemanticDependencyMultiplicityV1::PerRowMaterialization,
                lifetime: SemanticDependencyLifetimeV1::Row,
                ..SemanticDependencySemanticsV1::default()
            },
            list,
            Vec::new(),
        )?;
    }
    for (ordinal, occurrence) in checked.occurrences.iter().enumerate() {
        let declaration = checked
            .declarations
            .iter()
            .find(|declaration| declaration.id == occurrence.target)
            .ok_or_else(|| {
                CallableDependencyManifestError::new(format!(
                    "checked occurrence references missing declaration {}",
                    occurrence.target.0
                ))
            })?;
        collector.diagnostic(
            if declaration.kind == CheckedDeclarationKind::Function {
                SemanticDependencyOwnerV1::Callable {
                    callable: owners.callable_by_checked[&declaration.id],
                }
            } else {
                owners.checked_scope(declaration.scope_id)?
            },
            top_subject(
                SemanticDependencySubjectKindV1::CheckedOccurrence,
                SemanticDependencyEntityV1::indexed(
                    SemanticDependencyEntityDomainV1::CheckedOccurrence,
                    ordinal,
                ),
            ),
            occurrence,
        )?;
    }

    inventory_checked_lowering_metadata(checked, owners, collector)
}

fn declaration_channel(kind: CheckedDeclarationKind) -> SemanticDependencyChannelV1 {
    match kind {
        CheckedDeclarationKind::ValueParameter => SemanticDependencyChannelV1::ParentValueFormal,
        CheckedDeclarationKind::OutParameter | CheckedDeclarationKind::FreshOut => {
            SemanticDependencyChannelV1::OutFormal
        }
        CheckedDeclarationKind::PatternBinding | CheckedDeclarationKind::Field => {
            SemanticDependencyChannelV1::LocalFact
        }
        CheckedDeclarationKind::Source
        | CheckedDeclarationKind::Hold
        | CheckedDeclarationKind::List => SemanticDependencyChannelV1::ResourceBehavior,
        CheckedDeclarationKind::ElementState => SemanticDependencyChannelV1::CompilerContext,
        CheckedDeclarationKind::Function | CheckedDeclarationKind::Builtin => {
            SemanticDependencyChannelV1::CalledCallable
        }
        CheckedDeclarationKind::External => SemanticDependencyChannelV1::ExternalValueOrCall,
    }
}

fn declaration_roles(kind: CheckedDeclarationKind) -> Vec<SemanticDependencyRoleV1> {
    match kind {
        CheckedDeclarationKind::ValueParameter | CheckedDeclarationKind::OutParameter => vec![
            SemanticDependencyRoleV1::FormulaBinder,
            SemanticDependencyRoleV1::CoverageOrRouting,
        ],
        CheckedDeclarationKind::Source
        | CheckedDeclarationKind::Hold
        | CheckedDeclarationKind::List
        | CheckedDeclarationKind::ElementState
        | CheckedDeclarationKind::External => vec![
            SemanticDependencyRoleV1::ResourceOrProviderBehavior,
            SemanticDependencyRoleV1::CoverageOrRouting,
        ],
        CheckedDeclarationKind::Function
        | CheckedDeclarationKind::Builtin
        | CheckedDeclarationKind::FreshOut
        | CheckedDeclarationKind::PatternBinding
        | CheckedDeclarationKind::Field => vec![
            SemanticDependencyRoleV1::FixedDefinition,
            SemanticDependencyRoleV1::CoverageOrRouting,
        ],
    }
}

fn checked_statement_channel(kind: &CheckedStatementKind) -> SemanticDependencyChannelV1 {
    match kind {
        CheckedStatementKind::Source { .. }
        | CheckedStatementKind::Hold { .. }
        | CheckedStatementKind::List { .. } => SemanticDependencyChannelV1::ResourceBehavior,
        CheckedStatementKind::Function { .. }
        | CheckedStatementKind::Field { .. }
        | CheckedStatementKind::Block
        | CheckedStatementKind::Spread
        | CheckedStatementKind::Expression => SemanticDependencyChannelV1::LocalFact,
    }
}

fn checked_expression_dependency(
    kind: &CheckedExpressionKind,
) -> Result<ExpressionDependency, CallableDependencyManifestError> {
    let fixed = vec![SemanticDependencyRoleV1::FixedDefinition];
    Ok(match kind {
        CheckedExpressionKind::Read {
            target, projection, ..
        } => (
            SemanticDependencyChannelV1::LexicalCapture,
            vec![
                SemanticDependencyRoleV1::FixedDefinition,
                SemanticDependencyRoleV1::CoverageOrRouting,
            ],
            projection.clone(),
            vec![PendingDependencyReference::Entity(
                SemanticDependencyEntityV1::checked(
                    SemanticDependencyEntityDomainV1::CheckedDeclaration,
                    target.0,
                ),
            )],
        ),
        CheckedExpressionKind::Passed {
            formal, projection, ..
        } => (
            SemanticDependencyChannelV1::PassedContextFormal,
            vec![
                SemanticDependencyRoleV1::FormulaBinder,
                SemanticDependencyRoleV1::CoverageOrRouting,
            ],
            projection.clone(),
            vec![PendingDependencyReference::Entity(
                SemanticDependencyEntityV1::checked(
                    SemanticDependencyEntityDomainV1::CheckedContextFormal,
                    formal.0,
                ),
            )],
        ),
        CheckedExpressionKind::ExternalRead { .. } => (
            SemanticDependencyChannelV1::ExternalValueOrCall,
            vec![
                SemanticDependencyRoleV1::ResourceOrProviderBehavior,
                SemanticDependencyRoleV1::AssuranceOrActivation,
            ],
            Vec::new(),
            Vec::new(),
        ),
        CheckedExpressionKind::Drain { target, projection } => (
            SemanticDependencyChannelV1::MigrationPredecessor,
            vec![
                SemanticDependencyRoleV1::ResourceOrProviderBehavior,
                SemanticDependencyRoleV1::CoverageOrRouting,
            ],
            projection.clone(),
            vec![PendingDependencyReference::Entity(
                SemanticDependencyEntityV1::checked(
                    SemanticDependencyEntityDomainV1::CheckedDeclaration,
                    target.0,
                ),
            )],
        ),
        CheckedExpressionKind::Source
        | CheckedExpressionKind::Hold { .. }
        | CheckedExpressionKind::Latest { .. } => (
            SemanticDependencyChannelV1::ResourceBehavior,
            vec![
                SemanticDependencyRoleV1::ResourceOrProviderBehavior,
                SemanticDependencyRoleV1::CoverageOrRouting,
            ],
            Vec::new(),
            Vec::new(),
        ),
        CheckedExpressionKind::Draining { .. } => (
            SemanticDependencyChannelV1::MigrationPredecessor,
            vec![
                SemanticDependencyRoleV1::ResourceOrProviderBehavior,
                SemanticDependencyRoleV1::CoverageOrRouting,
            ],
            Vec::new(),
            Vec::new(),
        ),
        CheckedExpressionKind::Call { call } => (
            SemanticDependencyChannelV1::CalledCallable,
            vec![
                SemanticDependencyRoleV1::ResourceOrProviderBehavior,
                SemanticDependencyRoleV1::CoverageOrRouting,
            ],
            Vec::new(),
            vec![PendingDependencyReference::Entity(
                SemanticDependencyEntityV1::checked(
                    SemanticDependencyEntityDomainV1::CheckedCall,
                    call.0,
                ),
            )],
        ),
        CheckedExpressionKind::Invalid { .. } => {
            return Err(CallableDependencyManifestError::new(
                "invalid checked expression cannot enter a verified dependency manifest",
            ));
        }
        CheckedExpressionKind::Text { .. }
        | CheckedExpressionKind::TextTemplate { .. }
        | CheckedExpressionKind::Number { .. }
        | CheckedExpressionKind::BytesByte { .. }
        | CheckedExpressionKind::Absent
        | CheckedExpressionKind::Flush { .. }
        | CheckedExpressionKind::Tag { .. }
        | CheckedExpressionKind::TaggedObject { .. }
        | CheckedExpressionKind::When { .. }
        | CheckedExpressionKind::While { .. }
        | CheckedExpressionKind::Then { .. }
        | CheckedExpressionKind::Infix { .. }
        | CheckedExpressionKind::MatchArm { .. }
        | CheckedExpressionKind::Block { .. }
        | CheckedExpressionKind::Object { .. }
        | CheckedExpressionKind::List { .. }
        | CheckedExpressionKind::Bytes { .. }
        | CheckedExpressionKind::Delimiter => (
            SemanticDependencyChannelV1::StructuralRepresentation,
            fixed,
            Vec::new(),
            Vec::new(),
        ),
    })
}

fn parameter_dependency(
    parameter: &boon_typecheck::CheckedParameter,
) -> (SemanticDependencyChannelV1, Vec<SemanticDependencyRoleV1>) {
    let channel = match (parameter.kind, parameter.evaluation_scope) {
        (CheckedParameterKind::Out, _) => SemanticDependencyChannelV1::OutFormal,
        (CheckedParameterKind::Value, CheckedEvaluationScope::Output { .. }) => {
            SemanticDependencyChannelV1::OutputEvaluatedFormal
        }
        (CheckedParameterKind::Value, CheckedEvaluationScope::Parent) => {
            if parameter.requirement.is_optional() {
                SemanticDependencyChannelV1::NormalizedDefault
            } else {
                SemanticDependencyChannelV1::ParentValueFormal
            }
        }
    };
    (
        channel,
        vec![
            SemanticDependencyRoleV1::FormulaBinder,
            SemanticDependencyRoleV1::CoverageOrRouting,
        ],
    )
}

fn checked_call_entry_dependency(
    entry: &CheckedCallEntry,
) -> (
    SemanticDependencyChannelV1,
    Option<FlowType>,
    Vec<PendingDependencyReference>,
) {
    match entry {
        CheckedCallEntry::Input {
            value,
            evaluation_scope,
            ..
        } => (
            match evaluation_scope {
                CheckedEvaluationScope::Parent => SemanticDependencyChannelV1::ParentValueFormal,
                CheckedEvaluationScope::Output { .. } => {
                    SemanticDependencyChannelV1::OutputEvaluatedFormal
                }
            },
            None,
            vec![PendingDependencyReference::Entity(
                SemanticDependencyEntityV1::checked(
                    SemanticDependencyEntityDomainV1::CheckedExpression,
                    value.0,
                ),
            )],
        ),
        CheckedCallEntry::FreshOut { .. } | CheckedCallEntry::ForwardOut { .. } => {
            (SemanticDependencyChannelV1::OutFormal, None, Vec::new())
        }
    }
}

fn inventory_checked_lowering_metadata(
    checked: &CheckedProgram,
    owners: &DependencyOwnerIndex,
    collector: &mut DependencyCollector,
) -> Result<(), CallableDependencyManifestError> {
    let metadata_entity =
        SemanticDependencyEntityV1::indexed(SemanticDependencyEntityDomainV1::Diagnostic, 0);
    collector.structural(
        SemanticDependencyOwnerV1::ProgramRoot,
        top_subject(
            SemanticDependencySubjectKindV1::CheckedLoweringMetadata,
            metadata_entity.clone(),
        ),
        &checked.lowering_metadata,
    )?;
    for (ordinal, statement) in checked
        .lowering_metadata
        .named_value_type_table
        .checked_statement_sites
        .iter()
        .copied()
        .enumerate()
    {
        collect_dependency!(
            collector,
            owners.checked_statement(statement)?,
            SemanticDependencyChannelV1::CoverageRouting,
            vec![
                SemanticDependencyRoleV1::CoverageOrRouting,
                SemanticDependencyRoleV1::AssuranceOrActivation,
            ],
            child_subject(
                SemanticDependencySubjectKindV1::CheckedNamedValueStatementSite,
                metadata_entity.clone(),
                ordinal,
            ),
            SemanticDependencySemanticsV1::default(),
            &statement,
            vec![dependency_entity(SemanticDependencyEntityV1::checked(
                SemanticDependencyEntityDomainV1::CheckedStatement,
                statement.0,
            ))],
        )?;
    }
    let mut ordinal = 0usize;
    macro_rules! cover_entries {
        ($entries:expr) => {
            for entry in $entries {
                collector.structural(
                    SemanticDependencyOwnerV1::ProgramRoot,
                    child_subject(
                        SemanticDependencySubjectKindV1::CheckedLoweringSideTableEntry,
                        metadata_entity.clone(),
                        ordinal,
                    ),
                    entry,
                )?;
                ordinal += 1;
            }
        };
    }
    cover_entries!(&checked.lowering_metadata.source_units);
    cover_entries!(&checked.lowering_metadata.source_payload_shape_table);
    cover_entries!(&checked.lowering_metadata.output_root_types);
    cover_entries!(&checked.lowering_metadata.expr_type_table.entries);
    cover_entries!(&checked.lowering_metadata.function_type_table.entries);
    cover_entries!(&checked.lowering_metadata.named_value_type_table.entries);
    cover_entries!(&checked.lowering_metadata.render_slot_table.slots);
    cover_entries!(&checked.lowering_metadata.diagnostics);
    if let Some(http) = &checked.lowering_metadata.host_port_table.http {
        cover_entries!(std::slice::from_ref(http));
    }
    if let Some(websocket) = &checked.lowering_metadata.host_port_table.websocket {
        cover_entries!(std::slice::from_ref(websocket));
    }
    let _ = owners;
    Ok(())
}

fn inventory_producer_requests(
    requests: &[ProducerMaterializationRequest],
    owners: &DependencyOwnerIndex,
    collector: &mut DependencyCollector,
) -> Result<(), CallableDependencyManifestError> {
    collector.structural(
        SemanticDependencyOwnerV1::ProgramRoot,
        top_subject(
            SemanticDependencySubjectKindV1::ProducerMaterializationTable,
            SemanticDependencyEntityV1::Program,
        ),
        &requests,
    )?;
    for request in requests {
        let owner = SemanticDependencyOwnerV1::Callable {
            callable: request.callable,
        };
        if !owners
            .callable_by_checked
            .values()
            .any(|callable| *callable == request.callable)
        {
            return Err(CallableDependencyManifestError::new(format!(
                "producer materialization {:?} references missing callable {}",
                request.identity, request.callable
            )));
        }
        collect_dependency!(
            collector,
            owner,
            SemanticDependencyChannelV1::CoverageRouting,
            vec![
                SemanticDependencyRoleV1::ResourceOrProviderBehavior,
                SemanticDependencyRoleV1::CoverageOrRouting,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::ProducerMaterializationRequest,
                SemanticDependencyEntityV1::Digest {
                    domain: SemanticDependencyEntityDomainV1::ProducerMaterialization,
                    digest: request.identity,
                },
            ),
            SemanticDependencySemanticsV1 {
                multiplicity: match request.mode {
                    ProducerMaterializationMode::Current => {
                        SemanticDependencyMultiplicityV1::PerTick
                    }
                    ProducerMaterializationMode::Invocation => {
                        SemanticDependencyMultiplicityV1::PerEvent
                    }
                },
                ..SemanticDependencySemanticsV1::default()
            },
            request,
            Vec::new(),
        )?;
    }
    Ok(())
}

fn inventory_out(
    out: &ResolvedOutGraph,
    execution: &SemanticExecutionGraphV1,
    owners: &DependencyOwnerIndex,
    collector: &mut DependencyCollector,
) -> Result<(), CallableDependencyManifestError> {
    collector.structural(
        SemanticDependencyOwnerV1::ProgramRoot,
        top_subject(
            SemanticDependencySubjectKindV1::ResolvedOutGraph,
            SemanticDependencyEntityV1::Program,
        ),
        out,
    )?;
    for call in &out.call_instances {
        let owner = owners.out_call(call.id)?;
        let entity = SemanticDependencyEntityV1::indexed(
            SemanticDependencyEntityDomainV1::OutCallInstance,
            call.id.as_usize(),
        );
        let callee = owners
            .callable_by_checked
            .get(&call.provenance.callable)
            .copied()
            .ok_or_else(|| {
                CallableDependencyManifestError::new(format!(
                    "OUT call {} references callable declaration {} without semantic identity",
                    call.id, call.provenance.callable.0
                ))
            })?;
        collect_dependency!(
            collector,
            owner,
            SemanticDependencyChannelV1::CalledCallable,
            vec![
                SemanticDependencyRoleV1::ResourceOrProviderBehavior,
                SemanticDependencyRoleV1::CoverageOrRouting,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::OutCallInstance,
                entity.clone(),
            ),
            SemanticDependencySemanticsV1 {
                call_instance: Some(call.id),
                static_owner: call.owner,
                flow_type: Some(call.result.clone()),
                lifetime: SemanticDependencyLifetimeV1::Call,
                ..SemanticDependencySemanticsV1::default()
            },
            call,
            vec![PendingDependencyReference::Owner(
                SemanticDependencyOwnerV1::Callable { callable: callee },
            )],
        )?;
        for (ordinal, input) in call.inputs.iter().enumerate() {
            collect_dependency!(
                collector,
                owner,
                SemanticDependencyChannelV1::ParentValueFormal,
                vec![
                    SemanticDependencyRoleV1::FixedDefinition,
                    SemanticDependencyRoleV1::CoverageOrRouting,
                ],
                child_subject(
                    SemanticDependencySubjectKindV1::OutInputBinding,
                    entity.clone(),
                    ordinal,
                ),
                SemanticDependencySemanticsV1 {
                    call_instance: Some(call.id),
                    lifetime: SemanticDependencyLifetimeV1::Call,
                    ..SemanticDependencySemanticsV1::default()
                },
                input,
                Vec::new(),
            )?;
        }
        if let Some(passed) = &call.passed {
            collect_dependency!(
                collector,
                owner,
                SemanticDependencyChannelV1::PassedContextFormal,
                vec![
                    SemanticDependencyRoleV1::FormulaBinder,
                    SemanticDependencyRoleV1::CoverageOrRouting,
                ],
                child_subject(SemanticDependencySubjectKindV1::OutPassedBinding, entity, 0),
                SemanticDependencySemanticsV1 {
                    call_instance: Some(call.id),
                    lifetime: SemanticDependencyLifetimeV1::Call,
                    ..SemanticDependencySemanticsV1::default()
                },
                passed,
                Vec::new(),
            )?;
        }
    }
    for port in &out.ports {
        let owner = owners.out_call(port.call)?;
        collect_dependency!(
            collector,
            owner,
            SemanticDependencyChannelV1::OutFormal,
            vec![
                SemanticDependencyRoleV1::FormulaBinder,
                SemanticDependencyRoleV1::CoverageOrRouting,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::OutPort,
                SemanticDependencyEntityV1::indexed(
                    SemanticDependencyEntityDomainV1::OutPort,
                    port.id.as_usize(),
                ),
            ),
            SemanticDependencySemanticsV1 {
                call_instance: Some(port.call),
                multiplicity: SemanticDependencyMultiplicityV1::PerRowMaterialization,
                lifetime: SemanticDependencyLifetimeV1::Row,
                ..SemanticDependencySemanticsV1::default()
            },
            port,
            Vec::new(),
        )?;
    }
    for net in &out.nets {
        let static_owner = net.owner.ok_or_else(|| {
            CallableDependencyManifestError::new(format!(
                "OUT net {} has no explicit static owner identity",
                net.id
            ))
        })?;
        let owner = owners.static_owner(static_owner)?;
        let entity = SemanticDependencyEntityV1::indexed(
            SemanticDependencyEntityDomainV1::OutNet,
            net.id.as_usize(),
        );
        collect_dependency!(
            collector,
            owner,
            SemanticDependencyChannelV1::OutFormal,
            vec![
                SemanticDependencyRoleV1::ResourceOrProviderBehavior,
                SemanticDependencyRoleV1::CoverageOrRouting,
            ],
            top_subject(SemanticDependencySubjectKindV1::OutNet, entity.clone()),
            SemanticDependencySemanticsV1 {
                static_owner: net.owner,
                multiplicity: SemanticDependencyMultiplicityV1::PerRowMaterialization,
                lifetime: SemanticDependencyLifetimeV1::Row,
                ..SemanticDependencySemanticsV1::default()
            },
            net,
            Vec::new(),
        )?;
        for (ordinal, producer) in net.producers.iter().enumerate() {
            collector.structural(
                owner,
                child_subject(
                    SemanticDependencySubjectKindV1::OutStructuralProducer,
                    entity.clone(),
                    ordinal,
                ),
                producer,
            )?;
        }
    }
    for owner in &out.static_owners {
        collect_dependency!(
            collector,
            owners.static_owner(owner.id)?,
            SemanticDependencyChannelV1::CoverageRouting,
            vec![SemanticDependencyRoleV1::CoverageOrRouting],
            top_subject(
                SemanticDependencySubjectKindV1::OutStaticOwner,
                SemanticDependencyEntityV1::indexed(
                    SemanticDependencyEntityDomainV1::StaticOwner,
                    owner.id.as_usize(),
                ),
            ),
            SemanticDependencySemanticsV1 {
                static_owner: Some(owner.id),
                ..SemanticDependencySemanticsV1::default()
            },
            owner,
            Vec::new(),
        )?;
    }
    let _ = execution;
    Ok(())
}

fn inventory_execution(
    execution: &SemanticExecutionGraphV1,
    owners: &DependencyOwnerIndex,
    collector: &mut DependencyCollector,
) -> Result<(), CallableDependencyManifestError> {
    collector.structural(
        SemanticDependencyOwnerV1::ProgramRoot,
        top_subject(
            SemanticDependencySubjectKindV1::ExecutionGraph,
            SemanticDependencyEntityV1::Program,
        ),
        execution,
    )?;

    for scope in &execution.scopes {
        let owner = owners.checked_scope(scope.checked_scope)?;
        let entity = indexed_entity(
            SemanticDependencyEntityDomainV1::SemanticScope,
            scope.id.as_usize(),
        );
        let references = scope
            .parent
            .map(|parent| {
                vec![dependency_entity(indexed_entity(
                    SemanticDependencyEntityDomainV1::SemanticScope,
                    parent.as_usize(),
                ))]
            })
            .unwrap_or_default();
        collect_dependency!(
            collector,
            owner,
            SemanticDependencyChannelV1::CoverageRouting,
            vec![SemanticDependencyRoleV1::CoverageOrRouting],
            top_subject(SemanticDependencySubjectKindV1::ExecutionScope, entity),
            SemanticDependencySemanticsV1::default(),
            scope,
            references,
        )?;
    }

    for expression in &execution.expressions {
        let origin = execution
            .checked_expression_origins
            .get(expression.id.as_usize())
            .filter(|origin| origin.expression == expression.id)
            .ok_or_else(|| {
                CallableDependencyManifestError::new(format!(
                    "semantic expression {} has no exact checked origin",
                    expression.id
                ))
            })?;
        let expression_owner = owners.expression(expression.id)?;
        let owner = if let Some(static_owner) = expression.owner {
            exact_owner(
                vec![expression_owner, owners.static_owner(static_owner)?],
                &format!("semantic expression {}", expression.id),
            )?
        } else {
            expression_owner
        };
        let (channel, roles, projection, mut references) =
            semantic_expression_dependency(expression, owners)?;
        references.push(dependency_entity(SemanticDependencyEntityV1::checked(
            SemanticDependencyEntityDomainV1::CheckedExpression,
            expression.checked_expr_id.0,
        )));
        references.extend(
            origin
                .owning_statement
                .map(statement_entity)
                .map(dependency_entity),
        );
        references.extend(
            origin
                .call_instance
                .map(out_call_entity)
                .map(dependency_entity),
        );
        references.extend(
            expression
                .owner
                .map(static_owner_entity)
                .map(dependency_entity),
        );
        collect_dependency!(
            collector,
            owner,
            channel,
            roles,
            top_subject(
                SemanticDependencySubjectKindV1::ExecutionExpression,
                expression_entity(expression.id),
            ),
            SemanticDependencySemanticsV1 {
                projection,
                flow_type: Some(expression.flow_type.clone()),
                call_instance: origin.call_instance,
                static_owner: expression.owner,
                phase: semantic_expression_phase(&expression.kind),
                multiplicity: semantic_expression_multiplicity(&expression.kind),
                lifetime: semantic_expression_lifetime(&expression.kind),
                ..SemanticDependencySemanticsV1::default()
            },
            expression,
            references,
        )?;
    }

    for origin in &execution.checked_expression_origins {
        collector.diagnostic(
            owners.expression(origin.expression)?,
            top_subject(
                SemanticDependencySubjectKindV1::ExecutionExpressionOrigin,
                expression_entity(origin.expression),
            ),
            origin,
        )?;
    }

    for statement in &execution.statements {
        let owner = owners.statement(statement.id)?;
        let mut references = Vec::new();
        references.extend(
            statement
                .parent
                .map(|parent| dependency_entity(statement_entity(parent))),
        );
        references.extend(
            statement
                .value
                .map(|value| dependency_entity(expression_entity(value))),
        );
        references.extend(
            statement
                .children
                .iter()
                .copied()
                .map(statement_entity)
                .map(dependency_entity),
        );
        match &statement.origin {
            SemanticStatementOrigin::Checked {
                statement: checked_statement,
            } => {
                references.push(dependency_entity(SemanticDependencyEntityV1::checked(
                    SemanticDependencyEntityDomainV1::CheckedStatement,
                    checked_statement.0,
                )));
            }
            SemanticStatementOrigin::ProducerResult { callable, .. } => {
                let callable = owners
                    .callable_by_checked
                    .get(callable)
                    .copied()
                    .ok_or_else(|| {
                        CallableDependencyManifestError::new(format!(
                            "semantic producer-result statement {} references checked callable {} without semantic identity",
                            statement.id, callable.0
                        ))
                    })?;
                references.push(dependency_owner(SemanticDependencyOwnerV1::Callable {
                    callable,
                }));
            }
        }
        collect_dependency!(
            collector,
            owner,
            match &statement.kind {
                SemanticStatementKind::Source { .. }
                | SemanticStatementKind::Hold { .. }
                | SemanticStatementKind::List { .. } => {
                    SemanticDependencyChannelV1::ResourceBehavior
                }
                SemanticStatementKind::Field { .. }
                | SemanticStatementKind::Block
                | SemanticStatementKind::Spread
                | SemanticStatementKind::Expression => {
                    SemanticDependencyChannelV1::StructuralRepresentation
                }
            },
            vec![
                SemanticDependencyRoleV1::FixedDefinition,
                SemanticDependencyRoleV1::CoverageOrRouting,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::ExecutionStatement,
                statement_entity(statement.id),
            ),
            SemanticDependencySemanticsV1 {
                flow_type: statement.flow_type.clone(),
                call_instance: statement.call_instance,
                ..SemanticDependencySemanticsV1::default()
            },
            statement,
            references,
        )?;
    }

    for callable in &execution.callables {
        let owner = SemanticDependencyOwnerV1::Callable {
            callable: callable.id,
        };
        let entity = callable_entity(callable.id);
        let mut references = vec![dependency_entity(indexed_entity(
            SemanticDependencyEntityDomainV1::SemanticScope,
            callable.scope.as_usize(),
        ))];
        references.extend(callable.body.map(|statement| {
            dependency_entity(SemanticDependencyEntityV1::checked(
                SemanticDependencyEntityDomainV1::CheckedStatement,
                statement.0,
            ))
        }));
        references.extend(callable.result_expression.map(|expression| {
            dependency_entity(SemanticDependencyEntityV1::checked(
                SemanticDependencyEntityDomainV1::CheckedExpression,
                expression.0,
            ))
        }));
        collect_dependency!(
            collector,
            owner,
            SemanticDependencyChannelV1::SemanticProfile,
            vec![
                SemanticDependencyRoleV1::FixedDefinition,
                SemanticDependencyRoleV1::CoverageOrRouting,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::ExecutionCallable,
                entity.clone(),
            ),
            SemanticDependencySemanticsV1 {
                flow_type: Some(callable.result.clone()),
                program_role: Some(callable.role),
                visibility: SemanticDependencyVisibilityV1::Public,
                ..SemanticDependencySemanticsV1::default()
            },
            callable,
            references,
        )?;
        for (ordinal, parameter) in callable.parameters.iter().enumerate() {
            collect_dependency!(
                collector,
                owner,
                match parameter.evaluation_scope {
                    CheckedEvaluationScope::Parent => {
                        SemanticDependencyChannelV1::ParentValueFormal
                    }
                    CheckedEvaluationScope::Output { .. } => {
                        SemanticDependencyChannelV1::OutputEvaluatedFormal
                    }
                },
                vec![SemanticDependencyRoleV1::FormulaBinder],
                child_subject(
                    SemanticDependencySubjectKindV1::ExecutionCallableParameter,
                    entity.clone(),
                    ordinal,
                ),
                SemanticDependencySemanticsV1 {
                    flow_type: Some(parameter.flow_type.clone()),
                    evaluation_scope: Some(parameter.evaluation_scope),
                    visibility: SemanticDependencyVisibilityV1::Public,
                    ..SemanticDependencySemanticsV1::default()
                },
                parameter,
                vec![dependency_entity(SemanticDependencyEntityV1::checked(
                    SemanticDependencyEntityDomainV1::CheckedDeclaration,
                    parameter.formal.0,
                ))],
            )?;
        }
        for (ordinal, context) in callable.contexts.iter().enumerate() {
            collect_dependency!(
                collector,
                owner,
                SemanticDependencyChannelV1::CompilerContext,
                vec![
                    SemanticDependencyRoleV1::FormulaBinder,
                    SemanticDependencyRoleV1::ResourceOrProviderBehavior,
                ],
                child_subject(
                    SemanticDependencySubjectKindV1::ExecutionCallableContext,
                    entity.clone(),
                    ordinal,
                ),
                SemanticDependencySemanticsV1 {
                    flow_type: Some(context.flow_type.clone()),
                    visibility: SemanticDependencyVisibilityV1::Public,
                    ..SemanticDependencySemanticsV1::default()
                },
                context,
                vec![dependency_entity(SemanticDependencyEntityV1::checked(
                    SemanticDependencyEntityDomainV1::CheckedDeclaration,
                    context.provider.0,
                ))],
            )?;
        }
    }

    for call in &execution.calls {
        let owner = owners.call(call.id)?;
        let entity = call_entity(call.id);
        collect_dependency!(
            collector,
            owner,
            SemanticDependencyChannelV1::CalledCallable,
            vec![
                SemanticDependencyRoleV1::ResourceOrProviderBehavior,
                SemanticDependencyRoleV1::CoverageOrRouting,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::ExecutionCall,
                entity.clone(),
            ),
            SemanticDependencySemanticsV1 {
                flow_type: Some(call.result.clone()),
                program_role: Some(call.role),
                lifetime: SemanticDependencyLifetimeV1::Call,
                ..SemanticDependencySemanticsV1::default()
            },
            call,
            vec![
                dependency_owner(SemanticDependencyOwnerV1::Callable {
                    callable: call.callable,
                }),
                dependency_entity(callable_entity(call.callable)),
            ],
        )?;
        for (ordinal, entry) in call.entries.iter().enumerate() {
            let (channel, flow_type, reference) = match entry {
                SemanticCallEntry::Input {
                    checked_value,
                    value_flow_type,
                    evaluation_scope,
                    ..
                } => (
                    match evaluation_scope {
                        CheckedEvaluationScope::Parent => {
                            SemanticDependencyChannelV1::ParentValueFormal
                        }
                        CheckedEvaluationScope::Output { .. } => {
                            SemanticDependencyChannelV1::OutputEvaluatedFormal
                        }
                    },
                    Some(value_flow_type.clone()),
                    Some(dependency_entity(SemanticDependencyEntityV1::checked(
                        SemanticDependencyEntityDomainV1::CheckedExpression,
                        checked_value.0,
                    ))),
                ),
                SemanticCallEntry::FreshOut { .. } | SemanticCallEntry::ForwardOut { .. } => {
                    (SemanticDependencyChannelV1::OutFormal, None, None)
                }
            };
            collect_dependency!(
                collector,
                owner,
                channel,
                vec![
                    SemanticDependencyRoleV1::FormulaBinder,
                    SemanticDependencyRoleV1::CoverageOrRouting,
                ],
                child_subject(
                    SemanticDependencySubjectKindV1::ExecutionCallEntry,
                    entity.clone(),
                    ordinal,
                ),
                SemanticDependencySemanticsV1 {
                    flow_type,
                    lifetime: SemanticDependencyLifetimeV1::Call,
                    ..SemanticDependencySemanticsV1::default()
                },
                entry,
                reference.into_iter().collect(),
            )?;
        }
        for (ordinal, context) in call.contexts.iter().enumerate() {
            collect_dependency!(
                collector,
                owner,
                SemanticDependencyChannelV1::CompilerContext,
                vec![
                    SemanticDependencyRoleV1::FormulaBinder,
                    SemanticDependencyRoleV1::CoverageOrRouting,
                ],
                child_subject(
                    SemanticDependencySubjectKindV1::ExecutionCallContext,
                    entity.clone(),
                    ordinal,
                ),
                SemanticDependencySemanticsV1 {
                    lifetime: SemanticDependencyLifetimeV1::Call,
                    ..SemanticDependencySemanticsV1::default()
                },
                context,
                Vec::new(),
            )?;
        }
    }

    for source in &execution.sources {
        let owner = owners.source(source.id)?;
        let mut references = vec![
            dependency_entity(statement_entity(source.statement)),
            dependency_entity(expression_entity(source.expression)),
        ];
        match &source.origin {
            SemanticSourceOrigin::Checked {
                source: checked_source,
            } => {
                references.push(dependency_entity(SemanticDependencyEntityV1::checked(
                    SemanticDependencyEntityDomainV1::CheckedSource,
                    checked_source.0,
                )));
            }
            SemanticSourceOrigin::ProducerInvocation { function, .. } => {
                references.push(dependency_owner(SemanticDependencyOwnerV1::Callable {
                    callable: *function,
                }));
            }
        }
        collect_dependency!(
            collector,
            owner,
            SemanticDependencyChannelV1::ResourceBehavior,
            vec![
                SemanticDependencyRoleV1::ResourceOrProviderBehavior,
                SemanticDependencyRoleV1::CoverageOrRouting,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::ExecutionSource,
                source_entity(source.id),
            ),
            SemanticDependencySemanticsV1 {
                static_owner: source.owner,
                call_instance: source.call_instance,
                multiplicity: SemanticDependencyMultiplicityV1::PerEvent,
                lifetime: SemanticDependencyLifetimeV1::Event,
                phase: SemanticDependencyPhaseV1::EventPayload,
                ..SemanticDependencySemanticsV1::default()
            },
            source,
            references,
        )?;
    }

    for state in &execution.states {
        let owner = owners.state(state.id)?;
        collect_dependency!(
            collector,
            owner,
            SemanticDependencyChannelV1::ResourceBehavior,
            vec![
                SemanticDependencyRoleV1::FixedDefinition,
                SemanticDependencyRoleV1::ResourceOrProviderBehavior,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::ExecutionState,
                state_entity(state.id),
            ),
            SemanticDependencySemanticsV1 {
                static_owner: state.owner,
                call_instance: state.call_instance,
                multiplicity: SemanticDependencyMultiplicityV1::PerTransition,
                lifetime: SemanticDependencyLifetimeV1::Snapshot,
                phase: SemanticDependencyPhaseV1::Commit,
                ..SemanticDependencySemanticsV1::default()
            },
            state,
            vec![
                dependency_entity(statement_entity(state.statement)),
                dependency_entity(expression_entity(state.expression)),
                dependency_entity(expression_entity(state.initial)),
                dependency_entity(SemanticDependencyEntityV1::checked(
                    SemanticDependencyEntityDomainV1::CheckedState,
                    state.checked_state.0,
                )),
            ],
        )?;
    }

    for (ordinal, root) in execution.roots.iter().enumerate() {
        let owner = owners.expression(root.expression)?;
        collect_dependency!(
            collector,
            owner,
            SemanticDependencyChannelV1::CoverageRouting,
            vec![SemanticDependencyRoleV1::CoverageOrRouting],
            child_subject(
                SemanticDependencySubjectKindV1::ExecutionRoot,
                SemanticDependencyEntityV1::Program,
                ordinal,
            ),
            SemanticDependencySemanticsV1::default(),
            root,
            vec![dependency_entity(expression_entity(root.expression))],
        )?;
    }

    for function in &execution.functions {
        let owner = SemanticDependencyOwnerV1::Callable {
            callable: function.callable,
        };
        let entity = SemanticDependencyEntityV1::Digest {
            domain: SemanticDependencyEntityDomainV1::SemanticFunction,
            digest: function.identity,
        };
        let mut references = vec![
            dependency_owner(owner),
            dependency_entity(expression_entity(function.root)),
        ];
        references.extend(
            function
                .invocation_source
                .map(expression_entity)
                .map(dependency_entity),
        );
        collect_dependency!(
            collector,
            owner,
            SemanticDependencyChannelV1::ResourceBehavior,
            vec![
                SemanticDependencyRoleV1::ResourceOrProviderBehavior,
                SemanticDependencyRoleV1::CoverageOrRouting,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::ExecutionFunction,
                entity.clone(),
            ),
            SemanticDependencySemanticsV1 {
                flow_type: Some(function.result_type.clone()),
                ..SemanticDependencySemanticsV1::default()
            },
            function,
            references,
        )?;
        for (ordinal, parameter) in function.parameters.iter().enumerate() {
            collect_dependency!(
                collector,
                owner,
                SemanticDependencyChannelV1::ParentValueFormal,
                vec![SemanticDependencyRoleV1::FormulaBinder],
                child_subject(
                    SemanticDependencySubjectKindV1::ExecutionFunctionParameter,
                    entity.clone(),
                    ordinal,
                ),
                SemanticDependencySemanticsV1 {
                    flow_type: Some(parameter.flow_type.clone()),
                    ..SemanticDependencySemanticsV1::default()
                },
                parameter,
                parameter
                    .input_expressions
                    .iter()
                    .copied()
                    .map(expression_entity)
                    .map(dependency_entity)
                    .collect(),
            )?;
        }
    }

    for materialization in &execution.materializations {
        let owner = owners.static_owner(materialization.owner)?;
        let entity = indexed_entity(
            SemanticDependencyEntityDomainV1::SemanticMaterialization,
            materialization.id.as_usize(),
        );
        collect_dependency!(
            collector,
            owner,
            SemanticDependencyChannelV1::ResourceBehavior,
            vec![
                SemanticDependencyRoleV1::ResourceOrProviderBehavior,
                SemanticDependencyRoleV1::CoverageOrRouting,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::ExecutionMaterialization,
                entity,
            ),
            SemanticDependencySemanticsV1 {
                static_owner: Some(materialization.owner),
                multiplicity: SemanticDependencyMultiplicityV1::PerRowMaterialization,
                lifetime: SemanticDependencyLifetimeV1::Row,
                ..SemanticDependencySemanticsV1::default()
            },
            materialization,
            materialization
                .expression_roots()
                .into_iter()
                .map(expression_entity)
                .map(dependency_entity)
                .collect(),
        )?;
    }

    for static_owner in &execution.static_owners {
        collect_dependency!(
            collector,
            owners.static_owner(static_owner.id)?,
            SemanticDependencyChannelV1::CoverageRouting,
            vec![SemanticDependencyRoleV1::CoverageOrRouting],
            top_subject(
                SemanticDependencySubjectKindV1::ExecutionStaticOwner,
                static_owner_entity(static_owner.id),
            ),
            SemanticDependencySemanticsV1 {
                static_owner: Some(static_owner.id),
                ..SemanticDependencySemanticsV1::default()
            },
            static_owner,
            static_owner
                .parent
                .map(static_owner_entity)
                .map(dependency_entity)
                .into_iter()
                .collect(),
        )?;
    }
    Ok(())
}

fn semantic_expression_dependency(
    expression: &SemanticExpression,
    owners: &DependencyOwnerIndex,
) -> Result<ExpressionDependency, CallableDependencyManifestError> {
    let mut references = Vec::new();
    let (channel, roles, projection) = match &expression.kind {
        SemanticExpressionKind::CanonicalRead {
            target,
            projection,
            source,
            ..
        } => {
            references.push(dependency_entity(SemanticDependencyEntityV1::checked(
                SemanticDependencyEntityDomainV1::CheckedDeclaration,
                target.0,
            )));
            references.extend(
                source
                    .as_ref()
                    .map(|source| source_entity(source.source))
                    .map(dependency_entity),
            );
            (
                SemanticDependencyChannelV1::LexicalCapture,
                vec![SemanticDependencyRoleV1::FormulaBinder],
                projection.clone(),
            )
        }
        SemanticExpressionKind::LocalRead {
            declaration,
            projection,
            ..
        } => {
            references.push(dependency_entity(SemanticDependencyEntityV1::checked(
                SemanticDependencyEntityDomainV1::CheckedDeclaration,
                declaration.0,
            )));
            (
                SemanticDependencyChannelV1::LocalFact,
                vec![SemanticDependencyRoleV1::FormulaBinder],
                projection.clone(),
            )
        }
        SemanticExpressionKind::ExternalRead { .. } => (
            SemanticDependencyChannelV1::ExternalValueOrCall,
            vec![SemanticDependencyRoleV1::ResourceOrProviderBehavior],
            Vec::new(),
        ),
        SemanticExpressionKind::ElementState { projection, .. } => (
            SemanticDependencyChannelV1::CompilerContext,
            vec![
                SemanticDependencyRoleV1::FormulaBinder,
                SemanticDependencyRoleV1::ResourceOrProviderBehavior,
            ],
            projection.clone(),
        ),
        SemanticExpressionKind::Drain {
            target, projection, ..
        } => {
            references.push(dependency_entity(SemanticDependencyEntityV1::checked(
                SemanticDependencyEntityDomainV1::CheckedDeclaration,
                target.0,
            )));
            (
                SemanticDependencyChannelV1::MigrationPredecessor,
                vec![
                    SemanticDependencyRoleV1::ResourceOrProviderBehavior,
                    SemanticDependencyRoleV1::AssuranceOrActivation,
                ],
                projection.clone(),
            )
        }
        SemanticExpressionKind::Call {
            call,
            callable,
            instance,
            arguments,
            parameter_bindings,
            ..
        } => {
            references.push(dependency_entity(call_entity(*call)));
            references.push(dependency_entity(out_call_entity(*instance)));
            references.push(dependency_owner(SemanticDependencyOwnerV1::Callable {
                callable: *callable,
            }));
            references.extend(
                arguments
                    .iter()
                    .map(|argument| argument.value)
                    .map(expression_entity)
                    .map(dependency_entity),
            );
            references.extend(parameter_bindings.iter().filter_map(
                |binding| match &binding.kind {
                    SemanticCallParameterBindingKind::Explicit { value, .. } => {
                        Some(dependency_entity(expression_entity(*value)))
                    }
                    SemanticCallParameterBindingKind::Omitted => None,
                },
            ));
            (
                SemanticDependencyChannelV1::CalledCallable,
                vec![
                    SemanticDependencyRoleV1::ResourceOrProviderBehavior,
                    SemanticDependencyRoleV1::CoverageOrRouting,
                ],
                Vec::new(),
            )
        }
        SemanticExpressionKind::Materialize { materialization } => {
            references.push(dependency_entity(indexed_entity(
                SemanticDependencyEntityDomainV1::SemanticMaterialization,
                materialization.as_usize(),
            )));
            (
                SemanticDependencyChannelV1::ResourceBehavior,
                vec![SemanticDependencyRoleV1::ResourceOrProviderBehavior],
                Vec::new(),
            )
        }
        SemanticExpressionKind::Draining { input } => {
            references.push(dependency_entity(expression_entity(*input)));
            (
                SemanticDependencyChannelV1::MigrationPredecessor,
                vec![
                    SemanticDependencyRoleV1::ResourceOrProviderBehavior,
                    SemanticDependencyRoleV1::AssuranceOrActivation,
                ],
                Vec::new(),
            )
        }
        SemanticExpressionKind::Source { .. } => (
            SemanticDependencyChannelV1::ResourceBehavior,
            vec![SemanticDependencyRoleV1::ResourceOrProviderBehavior],
            Vec::new(),
        ),
        SemanticExpressionKind::Hold {
            initial, updates, ..
        } => {
            references.push(dependency_entity(expression_entity(*initial)));
            references.extend(
                updates
                    .iter()
                    .copied()
                    .map(expression_entity)
                    .map(dependency_entity),
            );
            (
                SemanticDependencyChannelV1::ResourceBehavior,
                vec![
                    SemanticDependencyRoleV1::FixedDefinition,
                    SemanticDependencyRoleV1::ResourceOrProviderBehavior,
                ],
                Vec::new(),
            )
        }
        SemanticExpressionKind::Latest { branches } => {
            references.extend(
                branches
                    .iter()
                    .copied()
                    .map(expression_entity)
                    .map(dependency_entity),
            );
            (
                SemanticDependencyChannelV1::ResourceBehavior,
                vec![SemanticDependencyRoleV1::ResourceOrProviderBehavior],
                Vec::new(),
            )
        }
        SemanticExpressionKind::When { input, arms, .. } => {
            references.push(dependency_entity(expression_entity(*input)));
            references.extend(
                arms.iter()
                    .map(|arm| arm.output)
                    .map(expression_entity)
                    .map(dependency_entity),
            );
            (
                SemanticDependencyChannelV1::ResourceBehavior,
                vec![
                    SemanticDependencyRoleV1::ResourceOrProviderBehavior,
                    SemanticDependencyRoleV1::CoverageOrRouting,
                ],
                Vec::new(),
            )
        }
        SemanticExpressionKind::Then { input, output } => {
            references.push(dependency_entity(expression_entity(*input)));
            references.extend(output.map(expression_entity).map(dependency_entity));
            (
                SemanticDependencyChannelV1::ResourceBehavior,
                vec![SemanticDependencyRoleV1::ResourceOrProviderBehavior],
                Vec::new(),
            )
        }
        SemanticExpressionKind::Infix { left, right, .. } => {
            references.push(dependency_entity(expression_entity(*left)));
            references.push(dependency_entity(expression_entity(*right)));
            (
                SemanticDependencyChannelV1::StructuralRepresentation,
                vec![SemanticDependencyRoleV1::FixedDefinition],
                Vec::new(),
            )
        }
        SemanticExpressionKind::MatchArm { output, .. } => {
            references.extend(output.map(expression_entity).map(dependency_entity));
            (
                SemanticDependencyChannelV1::CoverageRouting,
                vec![SemanticDependencyRoleV1::CoverageOrRouting],
                Vec::new(),
            )
        }
        SemanticExpressionKind::TaggedObject { fields, .. }
        | SemanticExpressionKind::Object(fields) => {
            references.extend(
                fields
                    .iter()
                    .map(|field| field.value)
                    .map(expression_entity)
                    .map(dependency_entity),
            );
            (
                SemanticDependencyChannelV1::StructuralRepresentation,
                vec![SemanticDependencyRoleV1::FixedDefinition],
                Vec::new(),
            )
        }
        SemanticExpressionKind::TextTemplate { segments } => {
            references.extend(segments.iter().filter_map(|segment| match segment {
                SemanticTextSegment::Static { .. } => None,
                SemanticTextSegment::Dynamic { value } => {
                    Some(dependency_entity(expression_entity(*value)))
                }
            }));
            (
                SemanticDependencyChannelV1::StructuralRepresentation,
                vec![SemanticDependencyRoleV1::FixedDefinition],
                Vec::new(),
            )
        }
        SemanticExpressionKind::Block { bindings, result } => {
            references.extend(
                bindings
                    .iter()
                    .map(|binding| binding.value)
                    .chain(std::iter::once(*result))
                    .map(expression_entity)
                    .map(dependency_entity),
            );
            (
                SemanticDependencyChannelV1::LocalFact,
                vec![SemanticDependencyRoleV1::FixedDefinition],
                Vec::new(),
            )
        }
        SemanticExpressionKind::List { items, .. }
        | SemanticExpressionKind::Bytes { items, .. } => {
            references.extend(
                items
                    .iter()
                    .copied()
                    .map(expression_entity)
                    .map(dependency_entity),
            );
            (
                SemanticDependencyChannelV1::StructuralRepresentation,
                vec![SemanticDependencyRoleV1::FixedDefinition],
                Vec::new(),
            )
        }
        SemanticExpressionKind::Project { input, fields } => {
            references.push(dependency_entity(expression_entity(*input)));
            (
                SemanticDependencyChannelV1::StructuralRepresentation,
                vec![SemanticDependencyRoleV1::FixedDefinition],
                fields.clone(),
            )
        }
        SemanticExpressionKind::MaterializationLocal {
            owner, projection, ..
        } => {
            references.push(dependency_entity(static_owner_entity(*owner)));
            (
                SemanticDependencyChannelV1::LocalFact,
                vec![SemanticDependencyRoleV1::FormulaBinder],
                projection.clone(),
            )
        }
        SemanticExpressionKind::FunctionParameter {
            parameter,
            projection,
        } => {
            let owner = SemanticDependencyOwnerV1::Callable {
                callable: parameter.callable,
            };
            if !owners.callable_by_checked.values().any(|callable| {
                SemanticDependencyOwnerV1::Callable {
                    callable: *callable,
                } == owner
            }) {
                return Err(CallableDependencyManifestError::new(format!(
                    "semantic expression {} references missing function parameter owner {}",
                    expression.id, parameter.callable
                )));
            }
            references.push(dependency_owner(owner));
            (
                SemanticDependencyChannelV1::ParentValueFormal,
                vec![SemanticDependencyRoleV1::FormulaBinder],
                projection.clone(),
            )
        }
        SemanticExpressionKind::Text(_)
        | SemanticExpressionKind::Number(_)
        | SemanticExpressionKind::BytesByte(_)
        | SemanticExpressionKind::Absent
        | SemanticExpressionKind::Flush { .. }
        | SemanticExpressionKind::FlushBoundary { .. }
        | SemanticExpressionKind::Tag(_)
        | SemanticExpressionKind::Delimiter => (
            SemanticDependencyChannelV1::StructuralRepresentation,
            vec![SemanticDependencyRoleV1::FixedDefinition],
            Vec::new(),
        ),
    };
    Ok((channel, roles, projection, references))
}

fn semantic_expression_phase(kind: &SemanticExpressionKind) -> SemanticDependencyPhaseV1 {
    match kind {
        SemanticExpressionKind::Drain { .. } | SemanticExpressionKind::Draining { .. } => {
            SemanticDependencyPhaseV1::PreviousCommittedValue
        }
        SemanticExpressionKind::Source { .. } => SemanticDependencyPhaseV1::EventPayload,
        SemanticExpressionKind::Hold { .. } => SemanticDependencyPhaseV1::CandidateWrite,
        _ => SemanticDependencyPhaseV1::CurrentValue,
    }
}

fn semantic_expression_multiplicity(
    kind: &SemanticExpressionKind,
) -> SemanticDependencyMultiplicityV1 {
    match kind {
        SemanticExpressionKind::Source { .. } => SemanticDependencyMultiplicityV1::PerEvent,
        SemanticExpressionKind::Materialize { .. }
        | SemanticExpressionKind::MaterializationLocal { .. } => {
            SemanticDependencyMultiplicityV1::PerRowMaterialization
        }
        SemanticExpressionKind::Hold { .. } => SemanticDependencyMultiplicityV1::PerTransition,
        _ => SemanticDependencyMultiplicityV1::PerTick,
    }
}

fn semantic_expression_lifetime(kind: &SemanticExpressionKind) -> SemanticDependencyLifetimeV1 {
    match kind {
        SemanticExpressionKind::Source { .. } => SemanticDependencyLifetimeV1::Event,
        SemanticExpressionKind::Materialize { .. }
        | SemanticExpressionKind::MaterializationLocal { .. } => SemanticDependencyLifetimeV1::Row,
        SemanticExpressionKind::Hold { .. }
        | SemanticExpressionKind::Drain { .. }
        | SemanticExpressionKind::Draining { .. } => SemanticDependencyLifetimeV1::Snapshot,
        SemanticExpressionKind::Call { .. } | SemanticExpressionKind::FunctionParameter { .. } => {
            SemanticDependencyLifetimeV1::Call
        }
        _ => SemanticDependencyLifetimeV1::Definition,
    }
}

fn inventory_resources(
    resources: &SemanticResourceGraphV1,
    execution: &SemanticExecutionGraphV1,
    owners: &DependencyOwnerIndex,
    collector: &mut DependencyCollector,
) -> Result<(), CallableDependencyManifestError> {
    collector.structural(
        SemanticDependencyOwnerV1::ProgramRoot,
        top_subject(
            SemanticDependencySubjectKindV1::ResourceGraph,
            SemanticDependencyEntityV1::Program,
        ),
        resources,
    )?;

    for row_scope in &resources.row_scopes {
        let owner = owners.list(row_scope.list)?;
        collect_dependency!(
            collector,
            owner,
            SemanticDependencyChannelV1::CoverageRouting,
            vec![SemanticDependencyRoleV1::CoverageOrRouting],
            top_subject(
                SemanticDependencySubjectKindV1::ResourceRowScope,
                indexed_entity(
                    SemanticDependencyEntityDomainV1::SemanticRowScope,
                    row_scope.id.as_usize(),
                ),
            ),
            SemanticDependencySemanticsV1 {
                row: Some(SemanticRowBinding {
                    list: row_scope.list,
                    scope: row_scope.id,
                }),
                lifetime: SemanticDependencyLifetimeV1::Row,
                ..SemanticDependencySemanticsV1::default()
            },
            row_scope,
            vec![dependency_entity(list_entity(row_scope.list))],
        )?;
    }

    for list in &resources.lists {
        let owner = owners.list(list.id)?;
        let mut references = vec![
            dependency_entity(statement_entity(list.statement)),
            dependency_entity(expression_entity(list.producer)),
        ];
        if let SemanticListResourceOriginV1::CheckedLiteral { checked_list } = &list.origin {
            references.push(dependency_entity(SemanticDependencyEntityV1::checked(
                SemanticDependencyEntityDomainV1::CheckedList,
                checked_list.0,
            )));
        }
        references.extend(resource_initializer_expressions(&list.initializer));
        collect_dependency!(
            collector,
            owner,
            SemanticDependencyChannelV1::ResourceBehavior,
            vec![
                SemanticDependencyRoleV1::FixedDefinition,
                SemanticDependencyRoleV1::ResourceOrProviderBehavior,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::ResourceList,
                list_entity(list.id),
            ),
            SemanticDependencySemanticsV1 {
                row: Some(SemanticRowBinding {
                    list: list.id,
                    scope: list.row_scope,
                }),
                multiplicity: SemanticDependencyMultiplicityV1::PerRowMaterialization,
                lifetime: SemanticDependencyLifetimeV1::Row,
                ..SemanticDependencySemanticsV1::default()
            },
            list,
            references,
        )?;
    }

    for authority in &resources.value_list_authorities {
        let owner = owners.value_list(authority.id)?;
        let mut references = vec![
            dependency_entity(statement_entity(authority.statement)),
            dependency_entity(expression_entity(authority.producer)),
        ];
        if let SemanticListResourceOriginV1::CheckedLiteral { checked_list } = &authority.origin {
            references.push(dependency_entity(SemanticDependencyEntityV1::checked(
                SemanticDependencyEntityDomainV1::CheckedList,
                checked_list.0,
            )));
        }
        references.extend(resource_initializer_expressions(&authority.initializer));
        collect_dependency!(
            collector,
            owner,
            SemanticDependencyChannelV1::ResourceBehavior,
            vec![
                SemanticDependencyRoleV1::FixedDefinition,
                SemanticDependencyRoleV1::ResourceOrProviderBehavior,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::ResourceValueListAuthority,
                value_list_entity(authority.id),
            ),
            SemanticDependencySemanticsV1 {
                multiplicity: SemanticDependencyMultiplicityV1::PerRowMaterialization,
                lifetime: SemanticDependencyLifetimeV1::Row,
                ..SemanticDependencySemanticsV1::default()
            },
            authority,
            references,
        )?;
    }

    for source in &resources.sources {
        let primary = owners.source(source.id)?;
        let owner = if let Some(static_owner) = source.owner {
            exact_owner(
                vec![primary, owners.static_owner(static_owner)?],
                &format!("resource source {}", source.id),
            )?
        } else {
            primary
        };
        let mut references = vec![
            dependency_entity(statement_entity(source.statement)),
            dependency_entity(expression_entity(source.expression)),
        ];
        references.extend(source.target_list.map(list_entity).map(dependency_entity));
        collect_dependency!(
            collector,
            owner,
            SemanticDependencyChannelV1::ResourceBehavior,
            vec![
                SemanticDependencyRoleV1::ResourceOrProviderBehavior,
                SemanticDependencyRoleV1::CoverageOrRouting,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::ResourceSource,
                source_entity(source.id),
            ),
            SemanticDependencySemanticsV1 {
                static_owner: source.owner,
                row: source
                    .target_list
                    .zip(source.row_scope)
                    .map(|(list, scope)| SemanticRowBinding { list, scope }),
                multiplicity: SemanticDependencyMultiplicityV1::PerEvent,
                lifetime: SemanticDependencyLifetimeV1::Event,
                phase: SemanticDependencyPhaseV1::EventPayload,
                ..SemanticDependencySemanticsV1::default()
            },
            source,
            references,
        )?;
    }

    for state in &resources.states {
        let primary = owners.state(state.id)?;
        let owner = if let Some(static_owner) = state.owner {
            exact_owner(
                vec![primary, owners.static_owner(static_owner)?],
                &format!("resource state {}", state.id),
            )?
        } else {
            primary
        };
        let mut references = vec![
            dependency_entity(statement_entity(state.statement)),
            dependency_entity(expression_entity(state.expression)),
            dependency_entity(expression_entity(state.initial)),
        ];
        references.extend(
            state
                .expression_members
                .iter()
                .copied()
                .map(expression_entity)
                .map(dependency_entity),
        );
        references.extend(state.target_list.map(list_entity).map(dependency_entity));
        collect_dependency!(
            collector,
            owner,
            SemanticDependencyChannelV1::ResourceBehavior,
            vec![
                SemanticDependencyRoleV1::FixedDefinition,
                SemanticDependencyRoleV1::ResourceOrProviderBehavior,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::ResourceState,
                state_entity(state.id),
            ),
            SemanticDependencySemanticsV1 {
                flow_type: Some(state.flow_type.clone()),
                static_owner: state.owner,
                row: state
                    .target_list
                    .zip(state.row_scope)
                    .map(|(list, scope)| SemanticRowBinding { list, scope }),
                multiplicity: SemanticDependencyMultiplicityV1::PerTransition,
                lifetime: SemanticDependencyLifetimeV1::Snapshot,
                phase: SemanticDependencyPhaseV1::Commit,
                ..SemanticDependencySemanticsV1::default()
            },
            state,
            references,
        )?;
    }

    for (index, alias) in resources.aliases.iter().enumerate() {
        let target_owner = match alias.target {
            SemanticResourceAliasTargetV1::Source(source) => owners.source(source)?,
            SemanticResourceAliasTargetV1::State(state) => owners.state(state)?,
        };
        let owner = if let Some(static_owner) = alias.owner {
            exact_owner(
                vec![target_owner, owners.static_owner(static_owner)?],
                &format!("resource alias {index}"),
            )?
        } else {
            target_owner
        };
        let target = match alias.target {
            SemanticResourceAliasTargetV1::Source(source) => source_entity(source),
            SemanticResourceAliasTargetV1::State(state) => state_entity(state),
        };
        collect_dependency!(
            collector,
            owner,
            SemanticDependencyChannelV1::LexicalCapture,
            vec![
                SemanticDependencyRoleV1::FormulaBinder,
                SemanticDependencyRoleV1::CoverageOrRouting,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::ResourceAlias,
                indexed_entity(
                    SemanticDependencyEntityDomainV1::SemanticResourceAlias,
                    index,
                ),
            ),
            SemanticDependencySemanticsV1 {
                static_owner: alias.owner,
                ..SemanticDependencySemanticsV1::default()
            },
            alias,
            vec![dependency_entity(target)],
        )?;
    }

    for binding in &resources.materialization_bindings {
        let owner = owners.static_owner(binding.owner)?;
        let mut references = vec![dependency_entity(indexed_entity(
            SemanticDependencyEntityDomainV1::SemanticMaterialization,
            binding.materialization.as_usize(),
        ))];
        references.extend(
            binding
                .source
                .into_iter()
                .chain(binding.target)
                .map(|row| list_entity(row.list))
                .map(dependency_entity),
        );
        collect_dependency!(
            collector,
            owner,
            SemanticDependencyChannelV1::ResourceBehavior,
            vec![
                SemanticDependencyRoleV1::ResourceOrProviderBehavior,
                SemanticDependencyRoleV1::CoverageOrRouting,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::ResourceMaterializationBinding,
                indexed_entity(
                    SemanticDependencyEntityDomainV1::SemanticMaterializationBinding,
                    binding.materialization.as_usize(),
                ),
            ),
            SemanticDependencySemanticsV1 {
                static_owner: Some(binding.owner),
                row: binding.target.or(binding.source),
                multiplicity: SemanticDependencyMultiplicityV1::PerRowMaterialization,
                lifetime: SemanticDependencyLifetimeV1::Row,
                ..SemanticDependencySemanticsV1::default()
            },
            binding,
            references,
        )?;
    }

    for (index, projection) in resources.list_projections.iter().enumerate() {
        let owner = owners.list(projection.target)?;
        collect_dependency!(
            collector,
            owner,
            SemanticDependencyChannelV1::ResourceRead,
            vec![
                SemanticDependencyRoleV1::FormulaBinder,
                SemanticDependencyRoleV1::ResourceOrProviderBehavior,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::ResourceListProjection,
                indexed_entity(
                    SemanticDependencyEntityDomainV1::SemanticListProjection,
                    index,
                ),
            ),
            SemanticDependencySemanticsV1 {
                multiplicity: SemanticDependencyMultiplicityV1::PerRowMaterialization,
                lifetime: SemanticDependencyLifetimeV1::Row,
                ..SemanticDependencySemanticsV1::default()
            },
            projection,
            vec![
                dependency_entity(list_entity(projection.target)),
                dependency_entity(list_entity(projection.source)),
            ],
        )?;
    }

    for producer in &resources.producer_resources {
        let owner = owners.static_owner(producer.owner)?;
        collect_dependency!(
            collector,
            owner,
            SemanticDependencyChannelV1::ResourceBehavior,
            vec![
                SemanticDependencyRoleV1::ResourceOrProviderBehavior,
                SemanticDependencyRoleV1::CoverageOrRouting,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::ResourceProducer,
                SemanticDependencyEntityV1::Digest {
                    domain: SemanticDependencyEntityDomainV1::SemanticProducerResource,
                    digest: producer.identity,
                },
            ),
            SemanticDependencySemanticsV1 {
                static_owner: Some(producer.owner),
                call_instance: Some(producer.root_call),
                ..SemanticDependencySemanticsV1::default()
            },
            producer,
            vec![
                dependency_owner(SemanticDependencyOwnerV1::Callable {
                    callable: producer.callable,
                }),
                dependency_entity(statement_entity(producer.result_statement)),
            ],
        )?;
    }
    let _ = execution;
    Ok(())
}

fn resource_initializer_expressions(
    initializer: &SemanticListInitializerV1,
) -> Vec<PendingDependencyReference> {
    let mut expressions = Vec::new();
    match initializer {
        SemanticListInitializerV1::Empty => {}
        SemanticListInitializerV1::RecordLiteral {
            authority_root,
            rows,
        } => {
            expressions.push(*authority_root);
            for row in rows {
                expressions.push(row.expression);
                for field in &row.fields {
                    expressions.extend(field.expression);
                    expressions.extend(field.spread_origin);
                }
            }
        }
        SemanticListInitializerV1::ValueLiteral {
            authority_root,
            values,
        } => {
            expressions.push(*authority_root);
            expressions.extend(values.iter().map(|value| value.expression));
        }
        SemanticListInitializerV1::Range {
            authority_root,
            from_expression,
            to_expression,
            ..
        } => {
            expressions.push(*authority_root);
            expressions.push(*from_expression);
            expressions.push(*to_expression);
        }
    }
    expressions.sort();
    expressions.dedup();
    expressions
        .into_iter()
        .map(expression_entity)
        .map(dependency_entity)
        .collect()
}

fn inventory_reactive(
    reactive: &SemanticReactiveGraphV1,
    execution: &SemanticExecutionGraphV1,
    owners: &DependencyOwnerIndex,
    collector: &mut DependencyCollector,
) -> Result<(), CallableDependencyManifestError> {
    collector.structural(
        SemanticDependencyOwnerV1::ProgramRoot,
        top_subject(
            SemanticDependencySubjectKindV1::ReactiveGraph,
            SemanticDependencyEntityV1::Program,
        ),
        reactive,
    )?;

    for producer in &reactive.producer_instances {
        let owner = owners.static_owner(producer.owner)?;
        let entity = SemanticDependencyEntityV1::Digest {
            domain: SemanticDependencyEntityDomainV1::SemanticProducerInstance,
            digest: producer.identity,
        };
        collect_dependency!(
            collector,
            owner,
            SemanticDependencyChannelV1::ResourceBehavior,
            vec![
                SemanticDependencyRoleV1::ResourceOrProviderBehavior,
                SemanticDependencyRoleV1::CoverageOrRouting,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::ReactiveProducerInstance,
                entity.clone(),
            ),
            SemanticDependencySemanticsV1 {
                static_owner: Some(producer.owner),
                call_instance: Some(producer.root_call),
                multiplicity: match producer.mode {
                    ProducerMaterializationMode::Current => {
                        SemanticDependencyMultiplicityV1::PerTick
                    }
                    ProducerMaterializationMode::Invocation => {
                        SemanticDependencyMultiplicityV1::PerEvent
                    }
                },
                ..SemanticDependencySemanticsV1::default()
            },
            producer,
            vec![
                dependency_owner(SemanticDependencyOwnerV1::Callable {
                    callable: producer.callable,
                }),
                dependency_entity(expression_entity(producer.root_expression)),
                dependency_entity(statement_entity(producer.result_statement)),
            ],
        )?;
        for (ordinal, parameter) in producer.parameters.iter().enumerate() {
            collect_dependency!(
                collector,
                owner,
                SemanticDependencyChannelV1::ParentValueFormal,
                vec![SemanticDependencyRoleV1::FormulaBinder],
                child_subject(
                    SemanticDependencySubjectKindV1::ReactiveProducerParameter,
                    entity.clone(),
                    ordinal,
                ),
                SemanticDependencySemanticsV1 {
                    flow_type: Some(parameter.flow_type.clone()),
                    lifetime: SemanticDependencyLifetimeV1::Call,
                    ..SemanticDependencySemanticsV1::default()
                },
                parameter,
                parameter
                    .input_expressions
                    .iter()
                    .copied()
                    .map(expression_entity)
                    .map(dependency_entity)
                    .collect(),
            )?;
        }
    }

    for field in &reactive.fields {
        let owner = owner_for_expression_and_static(
            owners,
            field.producer,
            field.owner,
            &format!("reactive field {}", field.id),
        )?;
        collect_dependency!(
            collector,
            owner,
            SemanticDependencyChannelV1::ResourceWrite,
            vec![
                SemanticDependencyRoleV1::FixedDefinition,
                SemanticDependencyRoleV1::ResourceOrProviderBehavior,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::ReactiveField,
                field_entity(field.id),
            ),
            SemanticDependencySemanticsV1 {
                flow_type: Some(field.flow_type.clone()),
                static_owner: field.owner,
                row: field.row,
                phase: SemanticDependencyPhaseV1::CandidateWrite,
                ..SemanticDependencySemanticsV1::default()
            },
            field,
            vec![
                dependency_entity(statement_entity(field.statement)),
                dependency_entity(expression_entity(field.producer)),
            ],
        )?;
    }

    for binding in &reactive.bindings {
        let owner = owner_for_expression_and_static(
            owners,
            binding.producer,
            binding.owner,
            &format!("reactive binding {}", binding.id),
        )?;
        collect_dependency!(
            collector,
            owner,
            SemanticDependencyChannelV1::ResourceWrite,
            vec![
                SemanticDependencyRoleV1::FormulaBinder,
                SemanticDependencyRoleV1::ResourceOrProviderBehavior,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::ReactiveBinding,
                binding_entity(binding.id),
            ),
            SemanticDependencySemanticsV1 {
                flow_type: Some(binding.flow_type.clone()),
                static_owner: binding.owner,
                call_instance: binding.call_instance,
                phase: SemanticDependencyPhaseV1::CandidateWrite,
                ..SemanticDependencySemanticsV1::default()
            },
            binding,
            vec![
                dependency_entity(statement_entity(binding.statement)),
                dependency_entity(expression_entity(binding.producer)),
                dependency_entity(binding_target_entity(binding.target)),
            ],
        )?;
    }

    for read in &reactive.reads {
        let owner = owners.expression(read.expression)?;
        let (channel, projection, references) = reactive_read_dependency(&read.target);
        collect_dependency!(
            collector,
            owner,
            channel,
            vec![
                SemanticDependencyRoleV1::FormulaBinder,
                SemanticDependencyRoleV1::ResourceOrProviderBehavior,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::ReactiveRead,
                read_entity(read.id),
            ),
            SemanticDependencySemanticsV1 {
                projection,
                phase: SemanticDependencyPhaseV1::CurrentValue,
                ..SemanticDependencySemanticsV1::default()
            },
            read,
            references,
        )?;
    }

    for dependency in &reactive.dependency_uses {
        let owner = exact_owner(
            vec![
                owners.binding(dependency.dependent)?,
                owners.expression(dependency.expression)?,
            ],
            &format!("reactive dependency use {}", dependency.id),
        )?;
        let mut references = vec![dependency_entity(binding_entity(dependency.dependent))];
        match &dependency.target {
            SemanticDependencyTargetV1::ExternalRead { read } => {
                references.push(dependency_entity(read_entity(*read)));
            }
            SemanticDependencyTargetV1::ExternalCall { call, expression } => {
                references.push(dependency_entity(call_entity(*call)));
                references.push(dependency_entity(expression_entity(*expression)));
            }
        }
        if let SemanticDependencyTimingV1::After { boundaries } = &dependency.timing {
            references.extend(
                boundaries
                    .iter()
                    .copied()
                    .map(event_cause_entity)
                    .map(dependency_entity),
            );
        }
        collect_dependency!(
            collector,
            owner,
            SemanticDependencyChannelV1::ExternalValueOrCall,
            vec![
                SemanticDependencyRoleV1::ResourceOrProviderBehavior,
                SemanticDependencyRoleV1::CoverageOrRouting,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::ReactiveDependencyUse,
                indexed_entity(
                    SemanticDependencyEntityDomainV1::SemanticDependencyUse,
                    dependency.id.as_usize(),
                ),
            ),
            SemanticDependencySemanticsV1 {
                multiplicity: SemanticDependencyMultiplicityV1::PerEvent,
                lifetime: SemanticDependencyLifetimeV1::Activation,
                ..SemanticDependencySemanticsV1::default()
            },
            dependency,
            references,
        )?;
    }

    for schedule in &reactive.call_invocations {
        let owner = owners.expression(schedule.expression)?;
        let mut references = vec![
            dependency_entity(expression_entity(schedule.expression)),
            dependency_entity(call_entity(schedule.call)),
        ];
        references.extend(
            schedule
                .dependent_bindings
                .iter()
                .copied()
                .map(binding_entity)
                .map(dependency_entity),
        );
        references.extend(schedule.invocation_arms.iter().copied().map(|arm| {
            dependency_entity(indexed_entity(
                SemanticDependencyEntityDomainV1::SemanticTriggerArm,
                arm.as_usize(),
            ))
        }));
        collect_dependency!(
            collector,
            owner,
            SemanticDependencyChannelV1::ExternalValueOrCall,
            vec![
                SemanticDependencyRoleV1::ResourceOrProviderBehavior,
                SemanticDependencyRoleV1::CoverageOrRouting,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::ReactiveCallSchedule,
                expression_entity(schedule.expression),
            ),
            SemanticDependencySemanticsV1 {
                multiplicity: if schedule.current_capable {
                    SemanticDependencyMultiplicityV1::PerTick
                } else {
                    SemanticDependencyMultiplicityV1::PerEvent
                },
                lifetime: SemanticDependencyLifetimeV1::Activation,
                ..SemanticDependencySemanticsV1::default()
            },
            schedule,
            references,
        )?;
    }

    for derived in &reactive.derived_values {
        let owner = exact_owner(
            vec![
                owners.binding(derived.binding)?,
                owners.expression(derived.producer)?,
            ],
            &format!("reactive derived value {}", derived.id),
        )?;
        let mut references = vec![
            dependency_entity(binding_entity(derived.binding)),
            dependency_entity(field_entity(derived.field)),
            dependency_entity(statement_entity(derived.statement)),
            dependency_entity(expression_entity(derived.producer)),
        ];
        references.extend(
            derived
                .materialized_list
                .map(list_entity)
                .map(dependency_entity),
        );
        references.extend(
            derived
                .causes
                .iter()
                .copied()
                .map(event_cause_entity)
                .map(dependency_entity),
        );
        collect_dependency!(
            collector,
            owner,
            SemanticDependencyChannelV1::ResourceBehavior,
            vec![
                SemanticDependencyRoleV1::FixedDefinition,
                SemanticDependencyRoleV1::ResourceOrProviderBehavior,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::ReactiveDerivedValue,
                indexed_entity(
                    SemanticDependencyEntityDomainV1::SemanticDerivedValue,
                    derived.id.as_usize(),
                ),
            ),
            SemanticDependencySemanticsV1 {
                row: derived
                    .materialized_list
                    .zip(derived.materialized_row_scope)
                    .map(|(list, scope)| SemanticRowBinding { list, scope }),
                multiplicity: SemanticDependencyMultiplicityV1::PerTransition,
                lifetime: SemanticDependencyLifetimeV1::Snapshot,
                ..SemanticDependencySemanticsV1::default()
            },
            derived,
            references,
        )?;
    }

    for arm in &reactive.trigger_arms {
        let owner = owner_for_expression_and_static(
            owners,
            arm.gate_expression,
            arm.owner,
            &format!("reactive trigger arm {}", arm.id),
        )?;
        let owner = exact_owner(
            vec![owner, owners.expression(arm.output_expression)?],
            &format!("reactive trigger arm {}", arm.id),
        )?;
        collect_dependency!(
            collector,
            owner,
            SemanticDependencyChannelV1::CoverageRouting,
            vec![
                SemanticDependencyRoleV1::ResourceOrProviderBehavior,
                SemanticDependencyRoleV1::CoverageOrRouting,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::ReactiveTriggerArm,
                indexed_entity(
                    SemanticDependencyEntityDomainV1::SemanticTriggerArm,
                    arm.id.as_usize(),
                ),
            ),
            SemanticDependencySemanticsV1 {
                static_owner: arm.owner,
                row: None,
                multiplicity: SemanticDependencyMultiplicityV1::PerEvent,
                lifetime: SemanticDependencyLifetimeV1::Event,
                phase: SemanticDependencyPhaseV1::EventPayload,
                ..SemanticDependencySemanticsV1::default()
            },
            arm,
            vec![
                dependency_entity(event_cause_entity(arm.cause)),
                dependency_entity(expression_entity(arm.gate_expression)),
                dependency_entity(expression_entity(arm.output_expression)),
            ],
        )?;
    }

    for arm in &reactive.state_update_arms {
        let owner = owners.state(arm.state)?;
        collect_dependency!(
            collector,
            owner,
            SemanticDependencyChannelV1::ResourceWrite,
            vec![
                SemanticDependencyRoleV1::ResourceOrProviderBehavior,
                SemanticDependencyRoleV1::CoverageOrRouting,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::ReactiveStateUpdateArm,
                indexed_entity(
                    SemanticDependencyEntityDomainV1::SemanticStateUpdateArm,
                    arm.id.as_usize(),
                ),
            ),
            SemanticDependencySemanticsV1 {
                multiplicity: SemanticDependencyMultiplicityV1::PerTransition,
                lifetime: SemanticDependencyLifetimeV1::Snapshot,
                phase: SemanticDependencyPhaseV1::CandidateWrite,
                ..SemanticDependencySemanticsV1::default()
            },
            arm,
            vec![
                dependency_entity(state_entity(arm.state)),
                dependency_entity(indexed_entity(
                    SemanticDependencyEntityDomainV1::SemanticTriggerArm,
                    arm.trigger.as_usize(),
                )),
            ],
        )?;
    }

    for mutation in &reactive.list_mutations {
        let owner = owner_for_expression_and_static(
            owners,
            mutation.site,
            mutation.owner,
            &format!("reactive list mutation {}", mutation.id),
        )?;
        let mut references = vec![
            dependency_entity(list_entity(mutation.list)),
            dependency_entity(expression_entity(mutation.site)),
            dependency_entity(event_cause_entity(mutation.cause)),
        ];
        match &mutation.kind {
            SemanticListMutationKindV1::Append { gate, item, .. } => {
                references.push(dependency_entity(expression_entity(*gate)));
                references.push(dependency_entity(expression_entity(*item)));
            }
            SemanticListMutationKindV1::Remove {
                materialization,
                gate,
                predicate,
                owner,
                ..
            } => {
                references.push(dependency_entity(indexed_entity(
                    SemanticDependencyEntityDomainV1::SemanticMaterialization,
                    materialization.as_usize(),
                )));
                references.push(dependency_entity(expression_entity(*gate)));
                references.push(dependency_entity(expression_entity(*predicate)));
                references.push(dependency_entity(static_owner_entity(*owner)));
            }
        }
        collect_dependency!(
            collector,
            owner,
            SemanticDependencyChannelV1::ResourceWrite,
            vec![
                SemanticDependencyRoleV1::ResourceOrProviderBehavior,
                SemanticDependencyRoleV1::CoverageOrRouting,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::ReactiveListMutation,
                indexed_entity(
                    SemanticDependencyEntityDomainV1::SemanticListMutation,
                    mutation.id.as_usize(),
                ),
            ),
            SemanticDependencySemanticsV1 {
                static_owner: mutation.owner,
                row: mutation.row_scope.map(|scope| SemanticRowBinding {
                    list: mutation.list,
                    scope,
                }),
                multiplicity: SemanticDependencyMultiplicityV1::PerTransition,
                lifetime: SemanticDependencyLifetimeV1::Row,
                phase: SemanticDependencyPhaseV1::CandidateWrite,
                ..SemanticDependencySemanticsV1::default()
            },
            mutation,
            references,
        )?;
    }

    for edge in &reactive.dependencies {
        let owner = owners.state(edge.to)?;
        collect_dependency!(
            collector,
            owner,
            SemanticDependencyChannelV1::ResourceRead,
            vec![
                SemanticDependencyRoleV1::ResourceOrProviderBehavior,
                SemanticDependencyRoleV1::CoverageOrRouting,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::ReactiveDependencyEdge,
                indexed_entity(
                    SemanticDependencyEntityDomainV1::SemanticExternalDependency,
                    edge.id.as_usize(),
                ),
            ),
            SemanticDependencySemanticsV1 {
                multiplicity: SemanticDependencyMultiplicityV1::PerTransition,
                lifetime: SemanticDependencyLifetimeV1::Snapshot,
                ..SemanticDependencySemanticsV1::default()
            },
            edge,
            vec![
                dependency_entity(event_cause_entity(edge.from)),
                dependency_entity(state_entity(edge.to)),
            ],
        )?;
    }

    for causes in &reactive.possible_causes {
        let owner = owners.state(causes.state)?;
        collect_dependency!(
            collector,
            owner,
            SemanticDependencyChannelV1::AssuranceInput,
            vec![
                SemanticDependencyRoleV1::CoverageOrRouting,
                SemanticDependencyRoleV1::AssuranceOrActivation,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::ReactivePossibleCauses,
                state_entity(causes.state),
            ),
            SemanticDependencySemanticsV1::default(),
            causes,
            causes
                .causes
                .iter()
                .copied()
                .map(event_cause_entity)
                .map(dependency_entity)
                .collect(),
        )?;
    }

    for effect in &reactive.host_effect_schedules {
        let owner = owner_for_expression_and_static(
            owners,
            effect.expression,
            effect.owner,
            &format!("reactive host effect {}", effect.id),
        )?;
        let mut references = vec![
            dependency_entity(expression_entity(effect.expression)),
            dependency_entity(call_entity(effect.call)),
        ];
        references.extend(effect.state_update_arms.iter().copied().map(|arm| {
            dependency_entity(indexed_entity(
                SemanticDependencyEntityDomainV1::SemanticStateUpdateArm,
                arm.as_usize(),
            ))
        }));
        collect_dependency!(
            collector,
            owner,
            SemanticDependencyChannelV1::RuntimeIntrinsicOrHostEffect,
            vec![
                SemanticDependencyRoleV1::ResourceOrProviderBehavior,
                SemanticDependencyRoleV1::AssuranceOrActivation,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::ReactiveHostEffect,
                indexed_entity(
                    SemanticDependencyEntityDomainV1::SemanticHostEffect,
                    effect.id.as_usize(),
                ),
            ),
            SemanticDependencySemanticsV1 {
                static_owner: effect.owner,
                multiplicity: SemanticDependencyMultiplicityV1::PerActivation,
                lifetime: SemanticDependencyLifetimeV1::Activation,
                phase: SemanticDependencyPhaseV1::EffectCompletion,
                ..SemanticDependencySemanticsV1::default()
            },
            effect,
            references,
        )?;
    }

    for output in &reactive.output_values {
        let owner = exact_owner(
            vec![
                owners.expression(output.expression)?,
                owners.statement(output.statement)?,
            ],
            &format!("reactive output {}", output.ordinal),
        )?;
        let mut references = vec![
            dependency_entity(expression_entity(output.expression)),
            dependency_entity(statement_entity(output.statement)),
        ];
        references.extend(output.field.map(field_entity).map(dependency_entity));
        collect_dependency!(
            collector,
            owner,
            SemanticDependencyChannelV1::CoverageRouting,
            vec![
                SemanticDependencyRoleV1::CoverageOrRouting,
                SemanticDependencyRoleV1::AssuranceOrActivation,
            ],
            child_subject(
                SemanticDependencySubjectKindV1::ReactiveOutput,
                SemanticDependencyEntityV1::Program,
                output.ordinal,
            ),
            SemanticDependencySemanticsV1 {
                visibility: SemanticDependencyVisibilityV1::Public,
                ..SemanticDependencySemanticsV1::default()
            },
            output,
            references,
        )?;
    }

    for capture in &reactive.view_captures {
        let owner = owners.expression(capture.expression)?;
        collect_dependency!(
            collector,
            owner,
            SemanticDependencyChannelV1::LexicalCapture,
            vec![
                SemanticDependencyRoleV1::FormulaBinder,
                SemanticDependencyRoleV1::CoverageOrRouting,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::ReactiveViewCapture,
                indexed_entity(
                    SemanticDependencyEntityDomainV1::SemanticCapture,
                    capture.id.as_usize(),
                ),
            ),
            SemanticDependencySemanticsV1 {
                lifetime: SemanticDependencyLifetimeV1::Snapshot,
                ..SemanticDependencySemanticsV1::default()
            },
            capture,
            vec![
                dependency_entity(expression_entity(capture.expression)),
                dependency_entity(view_capture_target_entity(capture.target)),
            ],
        )?;
    }

    for migration in &reactive.migration_inputs {
        let owner = owner_for_expression_and_static(
            owners,
            migration.marker,
            migration.owner,
            &format!("reactive migration input {}", migration.id),
        )?;
        let owner = exact_owner(
            vec![owner, owners.expression(migration.input)?],
            &format!("reactive migration input {}", migration.id),
        )?;
        collect_dependency!(
            collector,
            owner,
            SemanticDependencyChannelV1::MigrationPredecessor,
            vec![
                SemanticDependencyRoleV1::ResourceOrProviderBehavior,
                SemanticDependencyRoleV1::AssuranceOrActivation,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::ReactiveMigrationInput,
                indexed_entity(
                    SemanticDependencyEntityDomainV1::SemanticMigrationInput,
                    migration.id.as_usize(),
                ),
            ),
            SemanticDependencySemanticsV1 {
                static_owner: migration.owner,
                multiplicity: SemanticDependencyMultiplicityV1::PerActivation,
                lifetime: SemanticDependencyLifetimeV1::Activation,
                phase: SemanticDependencyPhaseV1::PreviousCommittedValue,
                ..SemanticDependencySemanticsV1::default()
            },
            migration,
            vec![
                dependency_entity(expression_entity(migration.marker)),
                dependency_entity(expression_entity(migration.input)),
            ],
        )?;
    }
    let _ = execution;
    Ok(())
}

fn owner_for_expression_and_static(
    owners: &DependencyOwnerIndex,
    expression: SemanticExprId,
    static_owner: Option<StaticOwnerId>,
    context: &str,
) -> Result<SemanticDependencyOwnerV1, CallableDependencyManifestError> {
    let primary = owners.expression(expression)?;
    if let Some(static_owner) = static_owner {
        exact_owner(vec![primary, owners.static_owner(static_owner)?], context)
    } else {
        Ok(primary)
    }
}

fn binding_target_entity(target: SemanticBindingTargetV1) -> SemanticDependencyEntityV1 {
    match target {
        SemanticBindingTargetV1::Field { field } => field_entity(field),
        SemanticBindingTargetV1::Source { source } => source_entity(source),
        SemanticBindingTargetV1::State { state } => state_entity(state),
        SemanticBindingTargetV1::List { list } => list_entity(list),
    }
}

fn reactive_read_dependency(
    target: &SemanticReadTargetV1,
) -> (
    SemanticDependencyChannelV1,
    Vec<String>,
    Vec<PendingDependencyReference>,
) {
    match target {
        SemanticReadTargetV1::Binding {
            binding,
            projection,
        } => (
            SemanticDependencyChannelV1::ResourceRead,
            projection.clone(),
            vec![dependency_entity(binding_entity(*binding))],
        ),
        SemanticReadTargetV1::SourcePayload {
            binding,
            source,
            payload_projection,
            projection,
        } => {
            let mut combined = payload_projection.clone();
            combined.extend(projection.iter().cloned());
            (
                SemanticDependencyChannelV1::ResourceRead,
                combined,
                vec![
                    dependency_entity(binding_entity(*binding)),
                    dependency_entity(source_entity(*source)),
                ],
            )
        }
        SemanticReadTargetV1::StateProjection {
            binding,
            state,
            projection,
        } => (
            SemanticDependencyChannelV1::ResourceRead,
            projection.clone(),
            vec![
                dependency_entity(binding_entity(*binding)),
                dependency_entity(state_entity(*state)),
            ],
        ),
        SemanticReadTargetV1::Local {
            producer,
            projection,
            ..
        } => (
            SemanticDependencyChannelV1::LocalFact,
            projection.clone(),
            vec![dependency_entity(expression_entity(*producer))],
        ),
        SemanticReadTargetV1::External { .. } => (
            SemanticDependencyChannelV1::ExternalValueOrCall,
            Vec::new(),
            Vec::new(),
        ),
        SemanticReadTargetV1::ElementState { projection, .. } => (
            SemanticDependencyChannelV1::CompilerContext,
            projection.clone(),
            Vec::new(),
        ),
        SemanticReadTargetV1::MaterializationLocal {
            owner, projection, ..
        } => (
            SemanticDependencyChannelV1::LocalFact,
            projection.clone(),
            vec![dependency_entity(static_owner_entity(*owner))],
        ),
        SemanticReadTargetV1::FunctionParameter {
            parameter,
            projection,
        } => (
            SemanticDependencyChannelV1::ParentValueFormal,
            projection.clone(),
            vec![dependency_owner(SemanticDependencyOwnerV1::Callable {
                callable: parameter.callable,
            })],
        ),
    }
}

fn event_cause_entity(cause: SemanticEventCauseV1) -> SemanticDependencyEntityV1 {
    match cause {
        SemanticEventCauseV1::Source(source) => source_entity(source),
        SemanticEventCauseV1::State(state) => state_entity(state),
    }
}

fn view_capture_target_entity(target: SemanticViewCaptureTargetV1) -> SemanticDependencyEntityV1 {
    match target {
        SemanticViewCaptureTargetV1::Read { read } => read_entity(read),
        SemanticViewCaptureTargetV1::Source { source } => source_entity(source),
        SemanticViewCaptureTargetV1::Field { field } => field_entity(field),
    }
}

fn inventory_lowering(
    lowering: &SemanticLoweringContractV1,
    execution: &SemanticExecutionGraphV1,
    resources: &SemanticResourceGraphV1,
    owners: &DependencyOwnerIndex,
    collector: &mut DependencyCollector,
) -> Result<(), CallableDependencyManifestError> {
    collector.structural(
        SemanticDependencyOwnerV1::ProgramRoot,
        top_subject(
            SemanticDependencySubjectKindV1::LoweringContract,
            SemanticDependencyEntityV1::Program,
        ),
        lowering,
    )?;
    collect_dependency!(
        collector,
        SemanticDependencyOwnerV1::ProgramRoot,
        SemanticDependencyChannelV1::TypeAndFlowInstance,
        vec![
            SemanticDependencyRoleV1::FixedDefinition,
            SemanticDependencyRoleV1::AssuranceOrActivation,
        ],
        top_subject(
            SemanticDependencySubjectKindV1::LoweringMetadata,
            indexed_entity(SemanticDependencyEntityDomainV1::Diagnostic, 1),
        ),
        SemanticDependencySemanticsV1::default(),
        &lowering.metadata,
        Vec::new(),
    )?;

    for unit in &lowering.metadata.source_units {
        collector.structural(
            SemanticDependencyOwnerV1::ProgramRoot,
            top_subject(
                SemanticDependencySubjectKindV1::LoweringSourceUnit,
                indexed_entity(
                    SemanticDependencyEntityDomainV1::SourceUnit,
                    unit.id.as_usize(),
                ),
            ),
            unit,
        )?;
    }

    for source_expression in &lowering.metadata.expression_types {
        let entity = indexed_entity(
            SemanticDependencyEntityDomainV1::SourceExpression,
            source_expression.id.as_usize(),
        );
        collect_dependency!(
            collector,
            SemanticDependencyOwnerV1::ProgramRoot,
            SemanticDependencyChannelV1::TypeAndFlowInstance,
            vec![
                SemanticDependencyRoleV1::FixedDefinition,
                SemanticDependencyRoleV1::AssuranceOrActivation,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::LoweringExpressionType,
                entity.clone(),
            ),
            SemanticDependencySemanticsV1 {
                flow_type: Some(source_expression.flow_type.clone()),
                ..SemanticDependencySemanticsV1::default()
            },
            source_expression,
            Vec::new(),
        )?;
        for (ordinal, occurrence) in source_expression.occurrences.iter().enumerate() {
            collect_dependency!(
                collector,
                owners.expression(occurrence.expression)?,
                SemanticDependencyChannelV1::TypeAndFlowInstance,
                vec![
                    SemanticDependencyRoleV1::FixedDefinition,
                    SemanticDependencyRoleV1::AssuranceOrActivation,
                ],
                child_subject(
                    SemanticDependencySubjectKindV1::LoweringExpressionOccurrence,
                    entity.clone(),
                    ordinal,
                ),
                SemanticDependencySemanticsV1 {
                    flow_type: Some(occurrence.flow_type.clone()),
                    ..SemanticDependencySemanticsV1::default()
                },
                occurrence,
                vec![dependency_entity(expression_entity(occurrence.expression))],
            )?;
        }
    }

    for function in &lowering.metadata.function_types {
        let owner = SemanticDependencyOwnerV1::Callable {
            callable: function.callable,
        };
        let entity = callable_entity(function.callable);
        collect_dependency!(
            collector,
            owner,
            SemanticDependencyChannelV1::TypeAndFlowInstance,
            vec![
                SemanticDependencyRoleV1::FixedDefinition,
                SemanticDependencyRoleV1::AssuranceOrActivation,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::LoweringFunctionType,
                entity.clone(),
            ),
            SemanticDependencySemanticsV1 {
                flow_type: Some(function.result.clone()),
                visibility: SemanticDependencyVisibilityV1::Public,
                ..SemanticDependencySemanticsV1::default()
            },
            function,
            vec![dependency_entity(callable_entity(function.callable))],
        )?;
        for (ordinal, parameter) in function.parameters.iter().enumerate() {
            collect_dependency!(
                collector,
                owner,
                SemanticDependencyChannelV1::TypeAndFlowInstance,
                vec![
                    SemanticDependencyRoleV1::FormulaBinder,
                    SemanticDependencyRoleV1::AssuranceOrActivation,
                ],
                child_subject(
                    SemanticDependencySubjectKindV1::LoweringFunctionParameter,
                    entity.clone(),
                    ordinal,
                ),
                SemanticDependencySemanticsV1 {
                    flow_type: Some(parameter.flow_type.clone()),
                    visibility: SemanticDependencyVisibilityV1::Public,
                    ..SemanticDependencySemanticsV1::default()
                },
                parameter,
                Vec::new(),
            )?;
        }
    }

    for named in &lowering.metadata.named_value_types {
        let owner = owners.checked_statement(named.checked_statement)?;
        let entity = indexed_entity(
            SemanticDependencyEntityDomainV1::SemanticNamedValue,
            named.id.as_usize(),
        );
        collect_dependency!(
            collector,
            owner,
            SemanticDependencyChannelV1::TypeAndFlowInstance,
            vec![
                SemanticDependencyRoleV1::FixedDefinition,
                SemanticDependencyRoleV1::AssuranceOrActivation,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::LoweringNamedValue,
                entity.clone(),
            ),
            SemanticDependencySemanticsV1 {
                flow_type: Some(named.flow_type.clone()),
                ..SemanticDependencySemanticsV1::default()
            },
            named,
            vec![dependency_entity(SemanticDependencyEntityV1::checked(
                SemanticDependencyEntityDomainV1::CheckedStatement,
                named.checked_statement.0,
            ))],
        )?;
        for (ordinal, origin) in named.origins.iter().enumerate() {
            let mut references = Vec::new();
            references.extend(
                origin
                    .statements
                    .iter()
                    .copied()
                    .map(statement_entity)
                    .map(dependency_entity),
            );
            references.extend(
                origin
                    .expressions
                    .iter()
                    .copied()
                    .map(expression_entity)
                    .map(dependency_entity),
            );
            references.extend(
                origin
                    .bindings
                    .iter()
                    .copied()
                    .map(binding_entity)
                    .map(dependency_entity),
            );
            references.extend(
                origin
                    .sources
                    .iter()
                    .copied()
                    .map(source_entity)
                    .map(dependency_entity),
            );
            references.extend(
                origin
                    .states
                    .iter()
                    .copied()
                    .map(state_entity)
                    .map(dependency_entity),
            );
            references.extend(
                origin
                    .lists
                    .iter()
                    .copied()
                    .map(list_entity)
                    .map(dependency_entity),
            );
            collect_dependency!(
                collector,
                owner,
                SemanticDependencyChannelV1::TypeAndFlowInstance,
                vec![
                    SemanticDependencyRoleV1::FixedDefinition,
                    SemanticDependencyRoleV1::AssuranceOrActivation,
                ],
                child_subject(
                    SemanticDependencySubjectKindV1::LoweringNamedValue,
                    entity.clone(),
                    ordinal,
                ),
                SemanticDependencySemanticsV1 {
                    flow_type: Some(named.flow_type.clone()),
                    ..SemanticDependencySemanticsV1::default()
                },
                origin,
                references,
            )?;
        }
    }

    for slot in &lowering.metadata.render_slots {
        let mut candidates = vec![owners.statement(slot.statement)?];
        if let Some(expression) = slot.value {
            candidates.push(owners.expression(expression)?);
        }
        let owner = exact_owner(candidates, &format!("lowering render slot {}", slot.id))?;
        let mut references = vec![dependency_entity(statement_entity(slot.statement))];
        references.extend(slot.value.map(expression_entity).map(dependency_entity));
        collect_dependency!(
            collector,
            owner,
            SemanticDependencyChannelV1::StructuralRepresentation,
            vec![
                SemanticDependencyRoleV1::FixedDefinition,
                SemanticDependencyRoleV1::AssuranceOrActivation,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::LoweringRenderSlot,
                indexed_entity(
                    SemanticDependencyEntityDomainV1::RenderSlot,
                    slot.id.as_usize(),
                ),
            ),
            SemanticDependencySemanticsV1 {
                visibility: SemanticDependencyVisibilityV1::Public,
                ..SemanticDependencySemanticsV1::default()
            },
            slot,
            references,
        )?;
    }

    for payload in &lowering.metadata.source_payload_shapes {
        let entity = indexed_entity(
            SemanticDependencyEntityDomainV1::SourcePayloadShape,
            payload.id.as_usize(),
        );
        collect_dependency!(
            collector,
            SemanticDependencyOwnerV1::ProgramRoot,
            SemanticDependencyChannelV1::TypeAndFlowInstance,
            vec![
                SemanticDependencyRoleV1::ResourceOrProviderBehavior,
                SemanticDependencyRoleV1::AssuranceOrActivation,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::LoweringSourcePayload,
                entity.clone(),
            ),
            SemanticDependencySemanticsV1::default(),
            payload,
            payload
                .sources
                .iter()
                .copied()
                .map(source_entity)
                .map(dependency_entity)
                .collect(),
        )?;
        for (ordinal, field) in payload.fields.iter().enumerate() {
            collector.structural(
                SemanticDependencyOwnerV1::ProgramRoot,
                child_subject(
                    SemanticDependencySubjectKindV1::LoweringSourcePayloadField,
                    entity.clone(),
                    ordinal,
                ),
                field,
            )?;
        }
    }

    for diagnostic in &lowering.metadata.diagnostics {
        collector.diagnostic(
            SemanticDependencyOwnerV1::ProgramRoot,
            top_subject(
                SemanticDependencySubjectKindV1::LoweringDiagnostic,
                indexed_entity(
                    SemanticDependencyEntityDomainV1::Diagnostic,
                    diagnostic.id.as_usize(),
                ),
            ),
            diagnostic,
        )?;
    }

    for output in &lowering.output_contracts {
        let owner = exact_owner(
            vec![
                owners.statement(output.statement)?,
                owners.expression(output.expression)?,
                owners.binding(output.binding)?,
            ],
            &format!("lowering output contract {}", output.id),
        )?;
        let mut references = vec![
            dependency_entity(statement_entity(output.statement)),
            dependency_entity(expression_entity(output.expression)),
            dependency_entity(binding_entity(output.binding)),
        ];
        references.extend(output.field.map(field_entity).map(dependency_entity));
        collect_dependency!(
            collector,
            owner,
            SemanticDependencyChannelV1::AssuranceInput,
            vec![
                SemanticDependencyRoleV1::CoverageOrRouting,
                SemanticDependencyRoleV1::AssuranceOrActivation,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::LoweringOutputContract,
                indexed_entity(
                    SemanticDependencyEntityDomainV1::OutputContract,
                    output.id.as_usize(),
                ),
            ),
            SemanticDependencySemanticsV1 {
                visibility: SemanticDependencyVisibilityV1::Public,
                ..SemanticDependencySemanticsV1::default()
            },
            output,
            references,
        )?;
    }

    for port in &lowering.host_ports {
        let mut references = Vec::new();
        match &port.kind {
            SemanticHostPortKindV1::HttpServer {
                request,
                disconnect,
                response,
            } => {
                references.push(dependency_entity(source_entity(request.source)));
                references.extend(
                    disconnect
                        .as_ref()
                        .map(|binding| source_entity(binding.source))
                        .map(dependency_entity),
                );
                references.push(dependency_entity(indexed_entity(
                    SemanticDependencyEntityDomainV1::OutputContract,
                    response.output.as_usize(),
                )));
            }
            SemanticHostPortKindV1::WebSocketServer {
                open,
                message,
                close,
                error,
                actions,
            } => {
                references.extend(
                    [open.source, message.source, close.source, error.source]
                        .into_iter()
                        .map(source_entity)
                        .map(dependency_entity),
                );
                references.push(dependency_entity(indexed_entity(
                    SemanticDependencyEntityDomainV1::OutputContract,
                    actions.output.as_usize(),
                )));
            }
        }
        collect_dependency!(
            collector,
            SemanticDependencyOwnerV1::ProgramRoot,
            SemanticDependencyChannelV1::RuntimeIntrinsicOrHostEffect,
            vec![
                SemanticDependencyRoleV1::ResourceOrProviderBehavior,
                SemanticDependencyRoleV1::AssuranceOrActivation,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::LoweringHostPort,
                indexed_entity(
                    SemanticDependencyEntityDomainV1::HostPort,
                    port.id.as_usize(),
                ),
            ),
            SemanticDependencySemanticsV1 {
                visibility: SemanticDependencyVisibilityV1::Public,
                lifetime: SemanticDependencyLifetimeV1::Activation,
                ..SemanticDependencySemanticsV1::default()
            },
            port,
            references,
        )?;
    }
    let _ = (execution, resources);
    Ok(())
}

fn inventory_view(
    view: &SemanticViewBindingGraphV1,
    execution: &SemanticExecutionGraphV1,
    reactive: &SemanticReactiveGraphV1,
    owners: &DependencyOwnerIndex,
    collector: &mut DependencyCollector,
) -> Result<(), CallableDependencyManifestError> {
    collector.structural(
        SemanticDependencyOwnerV1::ProgramRoot,
        top_subject(
            SemanticDependencySubjectKindV1::ViewBindingGraph,
            SemanticDependencyEntityV1::Program,
        ),
        view,
    )?;

    for root in &view.roots {
        let owner = view_root_owner(view, root.id, execution, owners)?;
        collect_dependency!(
            collector,
            owner,
            SemanticDependencyChannelV1::AssuranceInput,
            vec![
                SemanticDependencyRoleV1::CoverageOrRouting,
                SemanticDependencyRoleV1::AssuranceOrActivation,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::ViewRoot,
                view_root_entity(root.id),
            ),
            SemanticDependencySemanticsV1 {
                visibility: SemanticDependencyVisibilityV1::Public,
                lifetime: SemanticDependencyLifetimeV1::Snapshot,
                ..SemanticDependencySemanticsV1::default()
            },
            root,
            vec![
                dependency_entity(indexed_entity(
                    SemanticDependencyEntityDomainV1::OutputContract,
                    root.output.as_usize(),
                )),
                dependency_entity(statement_entity(root.statement)),
                dependency_entity(expression_entity(root.expression)),
                dependency_entity(binding_entity(root.binding)),
                dependency_entity(indexed_entity(
                    SemanticDependencyEntityDomainV1::SemanticScope,
                    root.route_scope.as_usize(),
                )),
            ],
        )?;
    }

    for node in &view.nodes {
        let owner = exact_owner(
            vec![
                view_root_owner(view, node.root, execution, owners)?,
                owners.expression(node.expression)?,
            ],
            &format!("view node {}", node.id),
        )?;
        collect_dependency!(
            collector,
            owner,
            SemanticDependencyChannelV1::StructuralRepresentation,
            vec![
                SemanticDependencyRoleV1::FixedDefinition,
                SemanticDependencyRoleV1::AssuranceOrActivation,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::ViewNode,
                view_node_entity(node.id),
            ),
            SemanticDependencySemanticsV1 {
                visibility: SemanticDependencyVisibilityV1::Public,
                lifetime: SemanticDependencyLifetimeV1::Snapshot,
                ..SemanticDependencySemanticsV1::default()
            },
            &(
                node.id,
                node.root,
                node.expression,
                node.value,
                node.call,
                node.callable,
            ),
            vec![
                dependency_entity(view_root_entity(node.root)),
                dependency_entity(expression_entity(node.expression)),
                dependency_entity(call_entity(node.call)),
                dependency_owner(SemanticDependencyOwnerV1::Callable {
                    callable: node.callable,
                }),
            ],
        )?;
    }

    for argument in &view.arguments {
        let owner = exact_owner(
            vec![
                view_root_owner(view, argument.root, execution, owners)?,
                view_node_owner(view, argument.node, execution, owners)?,
                owners.expression(argument.expression)?,
            ],
            &format!("view argument {}", argument.id),
        )?;
        let channel = match argument.kind {
            SemanticViewArgumentKindV1::RenderTree => {
                SemanticDependencyChannelV1::StructuralRepresentation
            }
            SemanticViewArgumentKindV1::BindingInput => SemanticDependencyChannelV1::ResourceRead,
        };
        collect_dependency!(
            collector,
            owner,
            channel,
            vec![
                SemanticDependencyRoleV1::FormulaBinder,
                SemanticDependencyRoleV1::AssuranceOrActivation,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::ViewArgument,
                view_argument_entity(argument.id),
            ),
            SemanticDependencySemanticsV1 {
                visibility: SemanticDependencyVisibilityV1::Public,
                lifetime: SemanticDependencyLifetimeV1::Snapshot,
                ..SemanticDependencySemanticsV1::default()
            },
            &(
                argument.id,
                argument.root,
                argument.node,
                argument.call,
                argument.callable,
                argument.formal,
                argument.ordinal,
                argument.expression,
                argument.value,
                argument.kind,
            ),
            vec![
                dependency_entity(view_root_entity(argument.root)),
                dependency_entity(view_node_entity(argument.node)),
                dependency_entity(call_entity(argument.call)),
                dependency_owner(SemanticDependencyOwnerV1::Callable {
                    callable: argument.callable,
                }),
                dependency_entity(SemanticDependencyEntityV1::checked(
                    SemanticDependencyEntityDomainV1::CheckedDeclaration,
                    argument.formal.0,
                )),
                dependency_entity(expression_entity(argument.expression)),
            ],
        )?;
    }

    for binding in &view.bindings {
        let _capture = reactive
            .view_captures
            .get(binding.capture.as_usize())
            .filter(|capture| capture.id == binding.capture)
            .ok_or_else(|| {
                CallableDependencyManifestError::new(format!(
                    "view binding {} references missing reactive capture {}",
                    binding.id, binding.capture
                ))
            })?;
        let owner_candidates = vec![
            view_root_owner(view, binding.root, execution, owners)?,
            view_node_owner(view, binding.node, execution, owners)?,
            view_argument_owner(view, binding.argument, execution, owners)?,
        ];
        let target = match binding.target {
            SemanticViewBindingTargetV1::Data { read } => {
                let read = reactive
                    .reads
                    .get(read.as_usize())
                    .filter(|candidate| candidate.id == read)
                    .ok_or_else(|| {
                        CallableDependencyManifestError::new(format!(
                            "view binding {} references missing reactive read {read}",
                            binding.id
                        ))
                    })?;
                read_entity(read.id)
            }
            SemanticViewBindingTargetV1::Event { source } => source_entity(source),
        };
        let owner = exact_owner(owner_candidates, &format!("view binding {}", binding.id))?;
        let entity = view_binding_entity(binding.id);
        let mut references = vec![
            dependency_entity(view_root_entity(binding.root)),
            dependency_entity(view_node_entity(binding.node)),
            dependency_entity(view_argument_entity(binding.argument)),
            dependency_entity(indexed_entity(
                SemanticDependencyEntityDomainV1::SemanticCapture,
                binding.capture.as_usize(),
            )),
            dependency_entity(expression_entity(binding.expression)),
            dependency_entity(indexed_entity(
                SemanticDependencyEntityDomainV1::SemanticScope,
                binding.route_scope.as_usize(),
            )),
            dependency_entity(target.clone()),
        ];
        references.extend(
            binding
                .row
                .map(|row| list_entity(row.list))
                .map(dependency_entity),
        );
        collect_dependency!(
            collector,
            owner,
            match binding.target {
                SemanticViewBindingTargetV1::Data { .. } => {
                    SemanticDependencyChannelV1::ResourceRead
                }
                SemanticViewBindingTargetV1::Event { .. } => {
                    SemanticDependencyChannelV1::ResourceBehavior
                }
            },
            vec![
                SemanticDependencyRoleV1::FormulaBinder,
                SemanticDependencyRoleV1::AssuranceOrActivation,
            ],
            top_subject(SemanticDependencySubjectKindV1::ViewBinding, entity.clone()),
            SemanticDependencySemanticsV1 {
                row: binding.row,
                visibility: SemanticDependencyVisibilityV1::Public,
                multiplicity: if matches!(binding.target, SemanticViewBindingTargetV1::Event { .. })
                {
                    SemanticDependencyMultiplicityV1::PerEvent
                } else {
                    SemanticDependencyMultiplicityV1::PerTick
                },
                lifetime: SemanticDependencyLifetimeV1::Snapshot,
                ..SemanticDependencySemanticsV1::default()
            },
            &(
                binding.id,
                binding.root,
                binding.node,
                binding.argument,
                binding.capture,
                binding.expression,
                binding.value,
                binding.target,
                binding.route_scope,
                binding.row,
            ),
            references,
        )?;
        collect_dependency!(
            collector,
            owner,
            match binding.target {
                SemanticViewBindingTargetV1::Data { .. } => {
                    SemanticDependencyChannelV1::ResourceRead
                }
                SemanticViewBindingTargetV1::Event { .. } => {
                    SemanticDependencyChannelV1::ResourceBehavior
                }
            },
            vec![
                SemanticDependencyRoleV1::ResourceOrProviderBehavior,
                SemanticDependencyRoleV1::AssuranceOrActivation,
            ],
            child_subject(
                SemanticDependencySubjectKindV1::ViewBindingTarget,
                entity,
                0,
            ),
            SemanticDependencySemanticsV1 {
                row: binding.row,
                visibility: SemanticDependencyVisibilityV1::Public,
                ..SemanticDependencySemanticsV1::default()
            },
            &binding.target,
            vec![dependency_entity(target)],
        )?;
    }
    Ok(())
}

fn view_root_owner(
    view: &SemanticViewBindingGraphV1,
    root: SemanticViewRootId,
    _execution: &SemanticExecutionGraphV1,
    owners: &DependencyOwnerIndex,
) -> Result<SemanticDependencyOwnerV1, CallableDependencyManifestError> {
    let root = view
        .roots
        .get(root.as_usize())
        .filter(|candidate| candidate.id == root)
        .ok_or_else(|| {
            CallableDependencyManifestError::new(format!(
                "dependency ownership references missing view root {root}"
            ))
        })?;
    // `route_scope` is a lexical definition coordinate and remains a
    // dependency reference. The retained root itself is an executable
    // occurrence, so only its statement, expression, and binding determine
    // primary ownership.
    exact_owner(
        vec![
            owners.statement(root.statement)?,
            owners.expression(root.expression)?,
            owners.binding(root.binding)?,
        ],
        &format!("view root {}", root.id),
    )
}

fn view_node_owner(
    view: &SemanticViewBindingGraphV1,
    node: SemanticViewNodeId,
    execution: &SemanticExecutionGraphV1,
    owners: &DependencyOwnerIndex,
) -> Result<SemanticDependencyOwnerV1, CallableDependencyManifestError> {
    let node = view
        .nodes
        .get(node.as_usize())
        .filter(|candidate| candidate.id == node)
        .ok_or_else(|| {
            CallableDependencyManifestError::new(format!(
                "dependency ownership references missing view node {node}"
            ))
        })?;
    exact_owner(
        vec![
            view_root_owner(view, node.root, execution, owners)?,
            owners.expression(node.expression)?,
        ],
        &format!("view node {}", node.id),
    )
}

fn view_argument_owner(
    view: &SemanticViewBindingGraphV1,
    argument: SemanticViewArgumentId,
    execution: &SemanticExecutionGraphV1,
    owners: &DependencyOwnerIndex,
) -> Result<SemanticDependencyOwnerV1, CallableDependencyManifestError> {
    let argument = view
        .arguments
        .get(argument.as_usize())
        .filter(|candidate| candidate.id == argument)
        .ok_or_else(|| {
            CallableDependencyManifestError::new(format!(
                "dependency ownership references missing view argument {argument}"
            ))
        })?;
    exact_owner(
        vec![
            view_root_owner(view, argument.root, execution, owners)?,
            view_node_owner(view, argument.node, execution, owners)?,
            owners.expression(argument.expression)?,
        ],
        &format!("view argument {}", argument.id),
    )
}

fn inventory_storage(
    storage: &SemanticScopeStorageGraphV1,
    execution: &SemanticExecutionGraphV1,
    resources: &SemanticResourceGraphV1,
    reactive: &SemanticReactiveGraphV1,
    owners: &DependencyOwnerIndex,
    collector: &mut DependencyCollector,
) -> Result<(), CallableDependencyManifestError> {
    collector.structural(
        SemanticDependencyOwnerV1::ProgramRoot,
        top_subject(
            SemanticDependencySubjectKindV1::StorageGraph,
            SemanticDependencyEntityV1::Program,
        ),
        storage,
    )?;

    for storage_owner in &storage.owners {
        let owner = owners.static_owner(storage_owner.id)?;
        let mut references = Vec::new();
        references.extend(
            storage_owner
                .parent
                .map(static_owner_entity)
                .map(dependency_entity),
        );
        references.extend(
            [
                storage_owner.source_row,
                storage_owner.target_row,
                storage_owner.authority_row,
            ]
            .into_iter()
            .flatten()
            .map(|row| list_entity(row.list))
            .map(dependency_entity),
        );
        collect_dependency!(
            collector,
            owner,
            SemanticDependencyChannelV1::CoverageRouting,
            vec![
                SemanticDependencyRoleV1::CoverageOrRouting,
                SemanticDependencyRoleV1::AssuranceOrActivation,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::StorageOwner,
                static_owner_entity(storage_owner.id),
            ),
            SemanticDependencySemanticsV1 {
                static_owner: Some(storage_owner.id),
                row: storage_owner
                    .authority_row
                    .or(storage_owner.target_row)
                    .or(storage_owner.source_row),
                ..SemanticDependencySemanticsV1::default()
            },
            storage_owner,
            references,
        )?;
    }

    for local in &storage.locals {
        let owner = owners.static_owner(local.owner)?;
        let entity = storage_local_entity(local.owner, local.local);
        let mut references = vec![
            dependency_entity(static_owner_entity(local.owner)),
            dependency_entity(expression_entity(local.source)),
        ];
        references.extend(
            local
                .row
                .map(|row| list_entity(row.list))
                .map(dependency_entity),
        );
        collect_dependency!(
            collector,
            owner,
            SemanticDependencyChannelV1::LocalFact,
            vec![
                SemanticDependencyRoleV1::FormulaBinder,
                SemanticDependencyRoleV1::CoverageOrRouting,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::StorageLocal,
                entity.clone(),
            ),
            SemanticDependencySemanticsV1 {
                static_owner: Some(local.owner),
                row: local.row,
                multiplicity: SemanticDependencyMultiplicityV1::PerRowMaterialization,
                lifetime: SemanticDependencyLifetimeV1::Row,
                ..SemanticDependencySemanticsV1::default()
            },
            local,
            references,
        )?;
        for (ordinal, member) in local.members.iter().enumerate() {
            let mut references = vec![dependency_entity(storage_local_member_target_entity(
                member.target,
            ))];
            if let Some(forwarding) = &member.forwarded_from {
                match forwarding {
                    SemanticStorageLocalMemberForwardingV1::Local { owner, local, .. } => {
                        references.push(dependency_entity(storage_local_entity(*owner, *local)))
                    }
                    SemanticStorageLocalMemberForwardingV1::Row { row, .. } => {
                        references.push(dependency_entity(list_entity(row.list)));
                    }
                }
            }
            collect_dependency!(
                collector,
                owner,
                SemanticDependencyChannelV1::StructuralRepresentation,
                vec![
                    SemanticDependencyRoleV1::FormulaBinder,
                    SemanticDependencyRoleV1::FixedDefinition,
                ],
                child_subject(
                    SemanticDependencySubjectKindV1::StorageLocalMember,
                    entity.clone(),
                    ordinal,
                ),
                SemanticDependencySemanticsV1 {
                    projection: member.path.clone(),
                    static_owner: Some(local.owner),
                    row: local.row,
                    multiplicity: SemanticDependencyMultiplicityV1::PerRowMaterialization,
                    lifetime: SemanticDependencyLifetimeV1::Row,
                    ..SemanticDependencySemanticsV1::default()
                },
                member,
                references,
            )?;
        }
        for capture in &local.captures {
            collect_dependency!(
                collector,
                owner,
                SemanticDependencyChannelV1::LexicalCapture,
                vec![
                    SemanticDependencyRoleV1::FormulaBinder,
                    SemanticDependencyRoleV1::CoverageOrRouting,
                ],
                top_subject(
                    SemanticDependencySubjectKindV1::StorageCapture,
                    indexed_entity(
                        SemanticDependencyEntityDomainV1::SemanticStorageCapture,
                        capture.id.as_usize(),
                    ),
                ),
                SemanticDependencySemanticsV1 {
                    projection: capture.projection.clone(),
                    static_owner: Some(local.owner),
                    row: local.row,
                    multiplicity: SemanticDependencyMultiplicityV1::PerRowMaterialization,
                    lifetime: SemanticDependencyLifetimeV1::Row,
                    ..SemanticDependencySemanticsV1::default()
                },
                capture,
                vec![
                    dependency_entity(storage_local_entity(
                        capture.source_owner,
                        capture.source_local,
                    )),
                    dependency_entity(indexed_entity(
                        SemanticDependencyEntityDomainV1::SemanticStorageField,
                        capture.field.as_usize(),
                    )),
                ],
            )?;
        }
    }

    for field in &storage.fields {
        let owner = owners.storage_field(field.id)?;
        let mut references = Vec::new();
        references.extend(
            field
                .parent
                .map(|parent| {
                    indexed_entity(
                        SemanticDependencyEntityDomainV1::SemanticStorageField,
                        parent.as_usize(),
                    )
                })
                .map(dependency_entity),
        );
        references.extend(field.producer.map(expression_entity).map(dependency_entity));
        references.extend(
            field
                .reactive_field
                .map(field_entity)
                .map(dependency_entity),
        );
        match &field.origin {
            SemanticStorageFieldOriginV1::Reactive {
                field: reactive_field,
            } => references.push(dependency_entity(field_entity(*reactive_field))),
            SemanticStorageFieldOriginV1::StateAuthority { state } => {
                references.push(dependency_entity(state_entity(*state)));
            }
            SemanticStorageFieldOriginV1::ListAuthority { list, .. } => {
                references.push(dependency_entity(list_entity(*list)));
            }
            SemanticStorageFieldOriginV1::ValueListAuthority { authority, .. } => {
                references.push(dependency_entity(value_list_entity(*authority)));
            }
            SemanticStorageFieldOriginV1::RecordProjection {
                parent, expression, ..
            } => {
                references.push(dependency_entity(indexed_entity(
                    SemanticDependencyEntityDomainV1::SemanticStorageField,
                    parent.as_usize(),
                )));
                references.push(dependency_entity(expression_entity(*expression)));
            }
            SemanticStorageFieldOriginV1::DetachedCapture {
                capture,
                target_owner,
                target_local,
            } => {
                references.push(dependency_entity(indexed_entity(
                    SemanticDependencyEntityDomainV1::SemanticStorageCapture,
                    capture.as_usize(),
                )));
                references.push(dependency_entity(storage_local_entity(
                    *target_owner,
                    *target_local,
                )));
            }
        }
        collect_dependency!(
            collector,
            owner,
            SemanticDependencyChannelV1::StructuralRepresentation,
            vec![
                SemanticDependencyRoleV1::FixedDefinition,
                SemanticDependencyRoleV1::AssuranceOrActivation,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::StorageField,
                indexed_entity(
                    SemanticDependencyEntityDomainV1::SemanticStorageField,
                    field.id.as_usize(),
                ),
            ),
            SemanticDependencySemanticsV1 {
                flow_type: Some(field.flow_type.clone()),
                static_owner: field.owner,
                row: field.row,
                lifetime: if field.row.is_some() {
                    SemanticDependencyLifetimeV1::Row
                } else {
                    SemanticDependencyLifetimeV1::Snapshot
                },
                ..SemanticDependencySemanticsV1::default()
            },
            field,
            references,
        )?;
    }

    for binding in &storage.bindings {
        let owner = owners.binding(binding.binding)?;
        collect_dependency!(
            collector,
            owner,
            SemanticDependencyChannelV1::StructuralRepresentation,
            vec![
                SemanticDependencyRoleV1::FormulaBinder,
                SemanticDependencyRoleV1::AssuranceOrActivation,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::StorageBinding,
                binding_entity(binding.binding),
            ),
            SemanticDependencySemanticsV1 {
                row: storage_binding_row(&binding.target),
                ..SemanticDependencySemanticsV1::default()
            },
            binding,
            vec![
                dependency_entity(binding_entity(binding.binding)),
                dependency_entity(storage_binding_target_entity(&binding.target)),
            ],
        )?;
    }

    for source in &storage.sources {
        let owner = exact_owner(
            vec![
                owners.source(source.source)?,
                owners.binding(source.binding)?,
            ],
            &format!("storage source {}", source.source),
        )?;
        collect_dependency!(
            collector,
            owner,
            SemanticDependencyChannelV1::ResourceBehavior,
            vec![
                SemanticDependencyRoleV1::ResourceOrProviderBehavior,
                SemanticDependencyRoleV1::AssuranceOrActivation,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::StorageSource,
                source_entity(source.source),
            ),
            SemanticDependencySemanticsV1 {
                static_owner: source.owner,
                multiplicity: SemanticDependencyMultiplicityV1::PerEvent,
                lifetime: SemanticDependencyLifetimeV1::Event,
                ..SemanticDependencySemanticsV1::default()
            },
            source,
            vec![
                dependency_entity(source_entity(source.source)),
                dependency_entity(binding_entity(source.binding)),
            ],
        )?;
    }

    for (index, row_value) in storage.row_values.iter().enumerate() {
        let owner = owners.expression(row_value.expression)?;
        collect_dependency!(
            collector,
            owner,
            SemanticDependencyChannelV1::ResourceRead,
            vec![
                SemanticDependencyRoleV1::FormulaBinder,
                SemanticDependencyRoleV1::AssuranceOrActivation,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::StorageRowValue,
                indexed_entity(
                    SemanticDependencyEntityDomainV1::SemanticStorageRowValue,
                    index,
                ),
            ),
            SemanticDependencySemanticsV1 {
                projection: row_value.projection.clone(),
                row: Some(row_value.row),
                multiplicity: SemanticDependencyMultiplicityV1::PerRowMaterialization,
                lifetime: SemanticDependencyLifetimeV1::Row,
                ..SemanticDependencySemanticsV1::default()
            },
            row_value,
            vec![
                dependency_entity(expression_entity(row_value.expression)),
                dependency_entity(list_entity(row_value.row.list)),
            ],
        )?;
    }

    for (index, projection) in storage.row_source_projections.iter().enumerate() {
        let owner = owners.source(projection.source)?;
        collect_dependency!(
            collector,
            owner,
            SemanticDependencyChannelV1::ResourceRead,
            vec![
                SemanticDependencyRoleV1::FormulaBinder,
                SemanticDependencyRoleV1::AssuranceOrActivation,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::StorageRowSourceProjection,
                indexed_entity(
                    SemanticDependencyEntityDomainV1::SemanticStorageRowSourceProjection,
                    index,
                ),
            ),
            SemanticDependencySemanticsV1 {
                projection: projection.path.clone(),
                row: Some(projection.row),
                multiplicity: SemanticDependencyMultiplicityV1::PerRowMaterialization,
                lifetime: SemanticDependencyLifetimeV1::Row,
                ..SemanticDependencySemanticsV1::default()
            },
            projection,
            vec![
                dependency_entity(source_entity(projection.source)),
                dependency_entity(list_entity(projection.row.list)),
            ],
        )?;
    }

    for reference in &storage.external_references {
        let (expression, target) = match reference.kind {
            SemanticStorageExternalReferenceKindV1::Read { read, expression } => {
                (expression, read_entity(read))
            }
            SemanticStorageExternalReferenceKindV1::Call { call, expression } => {
                (expression, call_entity(call))
            }
        };
        let owner = owners.expression(expression)?;
        collect_dependency!(
            collector,
            owner,
            SemanticDependencyChannelV1::ExternalValueOrCall,
            vec![
                SemanticDependencyRoleV1::ResourceOrProviderBehavior,
                SemanticDependencyRoleV1::AssuranceOrActivation,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::StorageExternalReference,
                indexed_entity(
                    SemanticDependencyEntityDomainV1::SemanticStorageExternalReference,
                    reference.id.as_usize(),
                ),
            ),
            SemanticDependencySemanticsV1 {
                lifetime: SemanticDependencyLifetimeV1::Activation,
                ..SemanticDependencySemanticsV1::default()
            },
            reference,
            vec![
                dependency_entity(expression_entity(expression)),
                dependency_entity(target),
            ],
        )?;
    }

    for producer in &storage.producer_result_fields {
        let owner = exact_owner(
            vec![
                owners.binding(producer.binding)?,
                owners.storage_field(producer.storage_field)?,
            ],
            &format!("producer result storage {:?}", producer.identity),
        )?;
        collect_dependency!(
            collector,
            owner,
            SemanticDependencyChannelV1::StructuralRepresentation,
            vec![
                SemanticDependencyRoleV1::ResourceOrProviderBehavior,
                SemanticDependencyRoleV1::AssuranceOrActivation,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::StorageProducerResult,
                SemanticDependencyEntityV1::Digest {
                    domain: SemanticDependencyEntityDomainV1::SemanticProducerResult,
                    digest: producer.identity,
                },
            ),
            SemanticDependencySemanticsV1::default(),
            producer,
            vec![
                dependency_entity(binding_entity(producer.binding)),
                dependency_entity(field_entity(producer.reactive_field)),
                dependency_entity(indexed_entity(
                    SemanticDependencyEntityDomainV1::SemanticStorageField,
                    producer.storage_field.as_usize(),
                )),
            ],
        )?;
    }

    for (index, named) in storage.named_values.iter().enumerate() {
        let (owner, references) = storage_named_value_owner_and_references(named, owners)?;
        collect_dependency!(
            collector,
            owner,
            SemanticDependencyChannelV1::StructuralRepresentation,
            vec![
                SemanticDependencyRoleV1::FixedDefinition,
                SemanticDependencyRoleV1::AssuranceOrActivation,
            ],
            child_subject(
                SemanticDependencySubjectKindV1::StorageNamedValue,
                indexed_entity(
                    SemanticDependencyEntityDomainV1::SemanticNamedValue,
                    named.named_value.as_usize(),
                ),
                index,
            ),
            SemanticDependencySemanticsV1 {
                flow_type: Some(named.flow_type.clone()),
                projection: named
                    .projection
                    .iter()
                    .map(|step| step.selector.clone())
                    .collect(),
                ..SemanticDependencySemanticsV1::default()
            },
            named,
            references,
        )?;
    }
    let _ = (execution, resources, reactive);
    Ok(())
}

fn storage_local_member_target_entity(
    target: SemanticStorageLocalMemberTargetV1,
) -> SemanticDependencyEntityV1 {
    match target {
        SemanticStorageLocalMemberTargetV1::Field(field) => indexed_entity(
            SemanticDependencyEntityDomainV1::SemanticStorageField,
            field.as_usize(),
        ),
        SemanticStorageLocalMemberTargetV1::Source(source) => source_entity(source),
        SemanticStorageLocalMemberTargetV1::State(state) => state_entity(state),
    }
}

fn storage_binding_target_entity(
    target: &SemanticStorageBindingTargetV1,
) -> SemanticDependencyEntityV1 {
    match target {
        SemanticStorageBindingTargetV1::Value { field, .. }
        | SemanticStorageBindingTargetV1::List { field, .. } => indexed_entity(
            SemanticDependencyEntityDomainV1::SemanticStorageField,
            field.as_usize(),
        ),
        SemanticStorageBindingTargetV1::Source { source } => source_entity(*source),
        SemanticStorageBindingTargetV1::State { state, .. } => state_entity(*state),
    }
}

fn storage_binding_row(target: &SemanticStorageBindingTargetV1) -> Option<SemanticRowBinding> {
    match target {
        SemanticStorageBindingTargetV1::Value { row, .. }
        | SemanticStorageBindingTargetV1::State { row, .. } => *row,
        SemanticStorageBindingTargetV1::List { row, .. } => Some(*row),
        SemanticStorageBindingTargetV1::Source { .. } => None,
    }
}

fn storage_named_value_owner_and_references(
    named: &SemanticNamedValueStorageV1,
    owners: &DependencyOwnerIndex,
) -> Result<
    (SemanticDependencyOwnerV1, Vec<PendingDependencyReference>),
    CallableDependencyManifestError,
> {
    let mut candidates = Vec::new();
    let mut references = Vec::new();
    match &named.target {
        SemanticNamedValueStorageTargetV1::Field { binding, field } => {
            candidates.push(owners.storage_field(*field)?);
            references.push(dependency_entity(indexed_entity(
                SemanticDependencyEntityDomainV1::SemanticStorageField,
                field.as_usize(),
            )));
            if let Some(binding) = binding {
                candidates.push(owners.binding(*binding)?);
                references.push(dependency_entity(binding_entity(*binding)));
            }
        }
        SemanticNamedValueStorageTargetV1::Source { binding, source } => {
            candidates.push(owners.binding(*binding)?);
            candidates.push(owners.source(*source)?);
            references.push(dependency_entity(binding_entity(*binding)));
            references.push(dependency_entity(source_entity(*source)));
        }
        SemanticNamedValueStorageTargetV1::State {
            binding,
            state,
            field,
        } => {
            candidates.push(owners.binding(*binding)?);
            candidates.push(owners.state(*state)?);
            references.push(dependency_entity(binding_entity(*binding)));
            references.push(dependency_entity(state_entity(*state)));
            if let Some(field) = field {
                candidates.push(owners.storage_field(*field)?);
                references.push(dependency_entity(indexed_entity(
                    SemanticDependencyEntityDomainV1::SemanticStorageField,
                    field.as_usize(),
                )));
            }
        }
        SemanticNamedValueStorageTargetV1::List {
            binding,
            list,
            field,
            ..
        } => {
            candidates.push(owners.binding(*binding)?);
            candidates.push(owners.list(*list)?);
            candidates.push(owners.storage_field(*field)?);
            references.push(dependency_entity(binding_entity(*binding)));
            references.push(dependency_entity(list_entity(*list)));
            references.push(dependency_entity(indexed_entity(
                SemanticDependencyEntityDomainV1::SemanticStorageField,
                field.as_usize(),
            )));
        }
        SemanticNamedValueStorageTargetV1::Value {
            expression, field, ..
        } => {
            candidates.push(owners.expression(*expression)?);
            references.push(dependency_entity(expression_entity(*expression)));
            if let Some(field) = field {
                candidates.push(owners.storage_field(*field)?);
                references.push(dependency_entity(indexed_entity(
                    SemanticDependencyEntityDomainV1::SemanticStorageField,
                    field.as_usize(),
                )));
            }
        }
        SemanticNamedValueStorageTargetV1::DiagnosticOnly { .. } => {
            candidates.push(SemanticDependencyOwnerV1::ProgramRoot);
        }
    }
    for step in &named.projection {
        references.extend(
            step.expression
                .map(expression_entity)
                .map(dependency_entity),
        );
        references.extend(step.storage_field.map(|field| {
            dependency_entity(indexed_entity(
                SemanticDependencyEntityDomainV1::SemanticStorageField,
                field.as_usize(),
            ))
        }));
    }
    Ok((
        exact_owner(
            candidates,
            &format!(
                "storage named value {} origin {} target {}",
                named.named_value, named.origin_ordinal, named.target_ordinal
            ),
        )?,
        references,
    ))
}

fn inventory_memory(
    memory: &SemanticMemoryGraphV1,
    execution: &SemanticExecutionGraphV1,
    owners: &DependencyOwnerIndex,
    collector: &mut DependencyCollector,
) -> Result<(), CallableDependencyManifestError> {
    collector.structural(
        SemanticDependencyOwnerV1::ProgramRoot,
        top_subject(
            SemanticDependencySubjectKindV1::MemoryGraph,
            SemanticDependencyEntityV1::Program,
        ),
        memory,
    )?;

    for region in &memory.memories {
        let owner = owners.memory(region.id)?;
        let entity = indexed_entity(
            SemanticDependencyEntityDomainV1::SemanticMemory,
            region.id.as_usize(),
        );
        let mut references = vec![
            dependency_entity(binding_entity(region.backing.binding())),
            dependency_entity(indexed_entity(
                SemanticDependencyEntityDomainV1::SemanticStorageField,
                region.backing.storage_field().as_usize(),
            )),
        ];
        match region.backing {
            SemanticMemoryBackingV1::State { state, .. } => {
                references.push(dependency_entity(state_entity(state)));
            }
            SemanticMemoryBackingV1::List { list, .. } => {
                references.push(dependency_entity(list_entity(list)));
            }
        }
        if let SemanticMemoryStatusV1::Draining { marker } = region.status {
            references.push(dependency_entity(indexed_entity(
                SemanticDependencyEntityDomainV1::SemanticMigrationInput,
                marker.as_usize(),
            )));
        }
        collect_dependency!(
            collector,
            owner,
            SemanticDependencyChannelV1::PersistenceActivation,
            vec![
                SemanticDependencyRoleV1::ResourceOrProviderBehavior,
                SemanticDependencyRoleV1::AssuranceOrActivation,
            ],
            top_subject(SemanticDependencySubjectKindV1::Memory, entity.clone()),
            SemanticDependencySemanticsV1 {
                row: region.backing.row(),
                multiplicity: SemanticDependencyMultiplicityV1::PerActivation,
                lifetime: SemanticDependencyLifetimeV1::Activation,
                phase: SemanticDependencyPhaseV1::PersistenceActivation,
                ..SemanticDependencySemanticsV1::default()
            },
            region,
            references,
        )?;
        for (ordinal, leaf) in region.leaves.iter().enumerate() {
            collect_dependency!(
                collector,
                owner,
                SemanticDependencyChannelV1::TypeAndFlowInstance,
                vec![
                    SemanticDependencyRoleV1::FixedDefinition,
                    SemanticDependencyRoleV1::AssuranceOrActivation,
                ],
                child_subject(
                    SemanticDependencySubjectKindV1::MemoryLeaf,
                    entity.clone(),
                    ordinal,
                ),
                SemanticDependencySemanticsV1 {
                    projection: leaf.projection.clone(),
                    row: region.backing.row(),
                    lifetime: SemanticDependencyLifetimeV1::Activation,
                    phase: SemanticDependencyPhaseV1::PersistenceActivation,
                    ..SemanticDependencySemanticsV1::default()
                },
                leaf,
                Vec::new(),
            )?;
        }
    }

    for edge in &memory.migration_edges {
        let owner = owners.memory(edge.destination.memory)?;
        let entity = indexed_entity(
            SemanticDependencyEntityDomainV1::SemanticMigrationEdge,
            edge.id.as_usize(),
        );
        let mut references = vec![dependency_entity(indexed_entity(
            SemanticDependencyEntityDomainV1::SemanticMemory,
            edge.destination.memory.as_usize(),
        ))];
        match edge.initializer {
            SemanticMigrationInitializerV1::State { state, root } => {
                references.push(dependency_entity(state_entity(state)));
                references.push(dependency_entity(expression_entity(root)));
            }
            SemanticMigrationInitializerV1::ListMaterialization {
                list,
                materialization,
                source_root,
            } => {
                references.push(dependency_entity(list_entity(list)));
                references.push(dependency_entity(indexed_entity(
                    SemanticDependencyEntityDomainV1::SemanticMaterialization,
                    materialization.as_usize(),
                )));
                references.push(dependency_entity(expression_entity(source_root)));
            }
        }
        match edge.transform {
            SemanticMigrationTransformV1::Identity { input } => {
                references.push(dependency_entity(expression_entity(input)));
            }
            SemanticMigrationTransformV1::PureExpression { root } => {
                references.push(dependency_entity(expression_entity(root)));
            }
        }
        collect_dependency!(
            collector,
            owner,
            SemanticDependencyChannelV1::PersistenceActivation,
            vec![
                SemanticDependencyRoleV1::ResourceOrProviderBehavior,
                SemanticDependencyRoleV1::AssuranceOrActivation,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::MigrationEdge,
                entity.clone(),
            ),
            SemanticDependencySemanticsV1 {
                projection: edge.destination.projection.clone(),
                multiplicity: SemanticDependencyMultiplicityV1::PerActivation,
                lifetime: SemanticDependencyLifetimeV1::Activation,
                phase: SemanticDependencyPhaseV1::PersistenceActivation,
                ..SemanticDependencySemanticsV1::default()
            },
            edge,
            references,
        )?;
        for (ordinal, input) in edge.inputs.iter().enumerate() {
            collect_dependency!(
                collector,
                owner,
                SemanticDependencyChannelV1::MigrationPredecessor,
                vec![
                    SemanticDependencyRoleV1::ResourceOrProviderBehavior,
                    SemanticDependencyRoleV1::AssuranceOrActivation,
                ],
                child_subject(
                    SemanticDependencySubjectKindV1::MigrationInput,
                    entity.clone(),
                    ordinal,
                ),
                SemanticDependencySemanticsV1 {
                    projection: input.source.projection.clone(),
                    multiplicity: SemanticDependencyMultiplicityV1::PerActivation,
                    lifetime: SemanticDependencyLifetimeV1::Activation,
                    phase: SemanticDependencyPhaseV1::PreviousCommittedValue,
                    ..SemanticDependencySemanticsV1::default()
                },
                input,
                vec![
                    dependency_entity(expression_entity(input.expression)),
                    dependency_entity(indexed_entity(
                        SemanticDependencyEntityDomainV1::SemanticMemory,
                        input.source.memory.as_usize(),
                    )),
                ],
            )?;
        }
    }
    let _ = execution;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn checked_fixture(
        name: &str,
        source: &str,
        role: ProgramRole,
    ) -> boon_typecheck::CheckedProgram {
        let parsed = boon_parser::parse_source(name, source).expect("fixture parses");
        let (output, _) = boon_typecheck::check_program_profiled_with_external_types(
            &parsed,
            &boon_typecheck::ExternalTypeEnvironment::empty(role),
        );
        assert!(
            !output.report.has_errors(),
            "fixture diagnostics: {:#?}",
            output.report.diagnostics
        );
        output.program.expect("fixture checks")
    }

    fn semantic_program_fixture() -> SemanticProgram {
        let checked = checked_fixture(
            "dependency-manifest.bn",
            r#"
store: [
    count: 1
]

FUNCTION add(value) {
    value + 1
}
"#,
            ProgramRole::Session,
        );
        elaborate(checked, &[]).expect("dependency manifest fixture elaborates")
    }

    fn manifest_record(
        program: &SemanticProgram,
        kind: SemanticDependencySubjectKindV1,
        identity: SemanticDependencyEntityV1,
    ) -> &SemanticDependencyRecordV1 {
        let matches = program
            .dependency_manifest
            .dependencies
            .iter()
            .filter(|record| record.subject.kind == kind && record.subject.identity == identity)
            .collect::<Vec<_>>();
        let [record] = matches.as_slice() else {
            panic!(
                "manifest subject {kind:?}/{identity:?} resolves to {} records",
                matches.len()
            );
        };
        record
    }

    fn assert_manifest_mutation_rejected(
        program: &SemanticProgram,
        context: &str,
        mutate: impl FnOnce(&mut CallableDependencyManifestV1),
    ) {
        let mut manifest = program.dependency_manifest.clone();
        mutate(&mut manifest);
        let error = match manifest.validate_against(
            DEPENDENCY_CLASSIFIER_SCHEMA_DIGEST_V1,
            &program.checked_program,
            &program.producer_materializations,
            &program.resolved_out_graph,
            &program.execution_graph,
            &program.resource_graph,
            &program.reactive_graph,
            &program.lowering_contract,
            &program.view_binding_graph,
            &program.scope_storage_graph,
            &program.memory_graph,
        ) {
            Ok(()) => panic!("{context} mutation must be rejected"),
            Err(error) => error,
        };
        assert!(
            error
                .to_string()
                .contains("differs from its deterministic checked+semantic rederivation"),
            "{context}: {error}"
        );
    }

    fn owner(callable: usize) -> SemanticDependencyOwnerV1 {
        SemanticDependencyOwnerV1::Callable {
            callable: SemanticCallableId(callable),
        }
    }

    fn test_subject(index: usize) -> SemanticDependencySubjectV1 {
        top_subject(
            SemanticDependencySubjectKindV1::ExecutionExpression,
            indexed_entity(SemanticDependencyEntityDomainV1::SemanticExpression, index),
        )
    }

    #[test]
    fn closure_follows_exact_owner_and_entity_edges() {
        let root = SemanticDependencyOwnerV1::ProgramRoot;
        let caller = owner(0);
        let callee = owner(1);
        let owners = BTreeSet::from([root, caller, callee]);
        let callee_entity = indexed_entity(SemanticDependencyEntityDomainV1::SemanticExpression, 0);
        let mut collector = DependencyCollector::default();
        let callee_dependency = collect_dependency!(
            collector,
            callee,
            SemanticDependencyChannelV1::LocalFact,
            vec![SemanticDependencyRoleV1::FixedDefinition],
            test_subject(0),
            SemanticDependencySemanticsV1::default(),
            &"callee",
            Vec::new(),
        )
        .expect("callee dependency");
        let caller_dependency = collect_dependency!(
            collector,
            caller,
            SemanticDependencyChannelV1::CalledCallable,
            vec![SemanticDependencyRoleV1::ResourceOrProviderBehavior],
            test_subject(1),
            SemanticDependencySemanticsV1::default(),
            &"caller",
            vec![dependency_owner(callee), dependency_entity(callee_entity)],
        )
        .expect("caller dependency");
        let (_, coverage, direct, closure) = collector.finish(&owners).expect("collector finish");

        assert_eq!(coverage.len(), 2);
        assert_eq!(direct[&caller], vec![caller_dependency]);
        assert_eq!(direct[&callee], vec![callee_dependency]);
        assert_eq!(
            closure[&caller],
            vec![callee_dependency, caller_dependency],
            "caller closure must include both exact entity and callable-owner dependencies"
        );
    }

    #[test]
    fn duplicate_subject_classification_is_rejected() {
        let mut collector = DependencyCollector::default();
        let subject = test_subject(0);
        collector
            .structural(
                SemanticDependencyOwnerV1::ProgramRoot,
                subject.clone(),
                &"first",
            )
            .expect("first classification");
        let error = collector
            .diagnostic(SemanticDependencyOwnerV1::ProgramRoot, subject, &"second")
            .expect_err("duplicate subject must fail");
        assert!(error.to_string().contains("classified more than once"));
    }

    #[test]
    fn unresolved_entity_reference_is_rejected() {
        let root = SemanticDependencyOwnerV1::ProgramRoot;
        let owners = BTreeSet::from([root]);
        let mut collector = DependencyCollector::default();
        collect_dependency!(
            collector,
            root,
            SemanticDependencyChannelV1::AssuranceInput,
            vec![SemanticDependencyRoleV1::AssuranceOrActivation],
            test_subject(0),
            SemanticDependencySemanticsV1::default(),
            &"dependent",
            vec![dependency_entity(indexed_entity(
                SemanticDependencyEntityDomainV1::SemanticMemory,
                99,
            ))],
        )
        .expect("dependency is collected before references resolve");
        let error = collector
            .finish(&owners)
            .expect_err("unresolved exact entity must fail");
        assert!(error.to_string().contains("no dependency classification"));
    }

    #[test]
    fn mixed_primary_owners_require_an_engine_identity() {
        let error = exact_owner(
            vec![SemanticDependencyOwnerV1::ProgramRoot, owner(0)],
            "mixed fixture",
        )
        .expect_err("mixed ownership must fail");
        assert!(error.to_string().contains("explicit engine identity"));
    }

    #[test]
    fn implementation_digest_detects_dependency_payload_mutation() {
        let root = SemanticDependencyOwnerV1::ProgramRoot;
        let owners = BTreeSet::from([root]);
        let mut collector = DependencyCollector::default();
        collect_dependency!(
            collector,
            root,
            SemanticDependencyChannelV1::StructuralRepresentation,
            vec![SemanticDependencyRoleV1::FixedDefinition],
            test_subject(0),
            SemanticDependencySemanticsV1::default(),
            &"stable",
            Vec::new(),
        )
        .expect("dependency");
        let (records, _, _, closure) = collector.finish(&owners).expect("collector finish");
        let original =
            implementation_dependency_digest(root, &closure[&root], &records).expect("digest");
        let mut mutated = records;
        mutated[0].payload_digest[0] ^= 1;
        let changed =
            implementation_dependency_digest(root, &closure[&root], &mutated).expect("digest");
        assert_ne!(original, changed);
    }

    #[test]
    fn contextual_occurrence_is_owned_by_its_concrete_root_and_links_its_definition() {
        let checked = checked_fixture(
            "dependency-contextual-owner.bn",
            r#"
FUNCTION doubled(list, entry: OUT, new) {
    list
    |> List/map(
        item: entry
        new: new * 2
    )
}

rows: LIST { [value: 1] }
result:
    rows
    |> doubled(
        entry
        new: entry.value + 1
    )
"#,
            ProgramRole::Client,
        );
        let program = elaborate(checked, &[]).expect("contextual fixture elaborates");
        let materialization = program
            .execution_graph
            .materializations
            .first()
            .expect("fixture has one materialization");
        let expression = &program.execution_graph.expressions[materialization.body.as_usize()];
        let origin = &program.execution_graph.checked_expression_origins[expression.id.as_usize()];
        let occurrence = manifest_record(
            &program,
            SemanticDependencySubjectKindV1::ExecutionExpression,
            expression_entity(expression.id),
        );
        assert_eq!(
            occurrence.owner,
            SemanticDependencyOwnerV1::ProgramRoot,
            "expanded wrapper work belongs to its ordinary concrete call root"
        );
        let definition = manifest_record(
            &program,
            SemanticDependencySubjectKindV1::CheckedExpression,
            SemanticDependencyEntityV1::checked(
                SemanticDependencyEntityDomainV1::CheckedExpression,
                expression.checked_expr_id.0,
            ),
        );
        assert_eq!(definition.owner, owner(0));
        assert!(occurrence.referenced_dependencies.contains(&definition.id));

        let static_owner = expression.owner.expect("body has exact static owner");
        let static_record = manifest_record(
            &program,
            SemanticDependencySubjectKindV1::ExecutionStaticOwner,
            static_owner_entity(static_owner),
        );
        assert!(
            occurrence
                .referenced_dependencies
                .contains(&static_record.id)
        );
        let frame = origin.call_instance.expect("body has exact call frame");
        let frame_record = manifest_record(
            &program,
            SemanticDependencySubjectKindV1::OutCallInstance,
            out_call_entity(frame),
        );
        assert!(
            occurrence
                .referenced_dependencies
                .contains(&frame_record.id)
        );
    }

    #[test]
    fn producer_occurrence_owner_uses_the_synthetic_root_callable_and_fails_on_axis_drift() {
        let checked = checked_fixture(
            "dependency-producer-owner.bn",
            r#"
FUNCTION serve(value) {
    value + 0
}

ordinary: serve(value: 1)
"#,
            ProgramRole::Session,
        );
        let callable = SemanticCallableId(
            checked
                .callables
                .iter()
                .position(|callable| callable.name == "serve")
                .expect("serve callable"),
        );
        let program = elaborate(
            checked,
            &[ProducerMaterializationRequest {
                identity: [7; 32],
                callable,
                local_function: "serve".to_owned(),
                mode: ProducerMaterializationMode::Current,
            }],
        )
        .expect("producer fixture elaborates");
        let function = program
            .execution_graph
            .functions
            .first()
            .expect("fixture has one producer function");
        assert_eq!(function.callable, callable);
        let occurrence = manifest_record(
            &program,
            SemanticDependencySubjectKindV1::ExecutionExpression,
            expression_entity(function.root),
        );
        assert_eq!(occurrence.owner, owner(callable.as_usize()));
        let producer_root = program
            .resolved_out_graph
            .producer_roots()
            .first()
            .expect("fixture has one producer root");
        let root_record = manifest_record(
            &program,
            SemanticDependencySubjectKindV1::OutCallInstance,
            out_call_entity(producer_root.call),
        );
        assert_eq!(root_record.owner, owner(callable.as_usize()));

        let mut frame_only = program.execution_graph.clone();
        frame_only.expressions[function.root.as_usize()].owner = None;
        let frame_only_index = DependencyOwnerIndex::derive(
            &program.checked_program,
            &program.resolved_out_graph,
            &frame_only,
            &program.resource_graph,
            &program.reactive_graph,
            &program.scope_storage_graph,
            &program.memory_graph,
        )
        .expect("call frame alone retains the producer occurrence owner");
        assert_eq!(
            frame_only_index
                .expression(function.root)
                .expect("producer expression owner"),
            owner(callable.as_usize())
        );

        let ordinary_root = program
            .resolved_out_graph
            .call_instances
            .iter()
            .find(|call| call.parent.is_none() && call.id != producer_root.call)
            .expect("fixture has an ordinary program-root call")
            .id;
        let mut conflicting = program.execution_graph.clone();
        conflicting.checked_expression_origins[function.root.as_usize()].call_instance =
            Some(ordinary_root);
        let error = DependencyOwnerIndex::derive(
            &program.checked_program,
            &program.resolved_out_graph,
            &conflicting,
            &program.resource_graph,
            &program.reactive_graph,
            &program.scope_storage_graph,
            &program.memory_graph,
        )
        .expect_err("static and call-root occurrence axes must agree");
        assert!(error.to_string().contains("conflicting static"), "{error}");
    }

    #[test]
    fn validate_against_rejects_every_component_digest_mutation() {
        let program = semantic_program_fixture();
        macro_rules! reject_component {
            ($field:ident) => {
                assert_manifest_mutation_rejected(&program, stringify!($field), |manifest| {
                    manifest.component_digests.$field[0] ^= 1
                });
            };
        }
        reject_component!(producer_materializations);
        reject_component!(resolved_out_graph);
        reject_component!(execution_graph);
        reject_component!(resource_graph);
        reject_component!(reactive_graph);
        reject_component!(lowering_contract);
        reject_component!(view_binding_graph);
        reject_component!(scope_storage_graph);
        reject_component!(memory_graph);
    }

    #[test]
    fn validate_against_rejects_callable_closure_mutation() {
        let program = semantic_program_fixture();
        assert_manifest_mutation_rejected(&program, "callable closure", |manifest| {
            let callable = manifest
                .callable_entries
                .first_mut()
                .expect("fixture has a callable dependency entry");
            assert!(
                !callable.closure_dependency_ids.is_empty(),
                "fixture callable closure is nonempty"
            );
            callable.closure_dependency_ids.pop();
        });
    }
}
