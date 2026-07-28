use crate::{
    ContextualMaterialization, ContextualOperationKind, ContextualOrderKey,
    ContextualRowPredecessor, DependencyEdge, DerivedValue, DerivedValueKind, ErasedBinding,
    ErasedBindingId, ErasedBindingTarget, ErasedDependencyTiming, ErasedFieldDef, ErasedFieldRole,
    ErasedLocalCapture, ErasedLocalDef, ErasedLocalMember, ErasedLocalMemberForwarding,
    ErasedLocalMemberTarget, ErasedOwnerDef, ErasedReadId, ErasedRowBinding,
    ErasedRowSourceProjection, ErasedRowValue, ErasedSourceDef, ErasedSourceOrigin,
    ErasedTemporalBoundary, EventCause, ExecutableBlockBinding, ExecutableCallArgument,
    ExecutableCallContextId, ExecutableCallableKind, ExecutableExprId, ExecutableExpression,
    ExecutableExpressionKind, ExecutableFunction, ExecutableFunctionParameter,
    ExecutableLocalBindingId, ExecutableParameterId, ExecutablePatternBinding, ExecutableProgram,
    ExecutableRecordField, ExecutableRoot, ExecutableSelectArm, ExecutableSourceDef,
    ExecutableSourceId, ExecutableSourceOrigin, ExecutableStateDef, ExecutableStateId,
    ExecutableStatement, ExecutableStatementId, ExecutableStatementKind, ExecutableTextSegment,
    ExecutableValueMember, ExecutableValueOrigin, ExecutableValueProvenance, ExprId, FieldId,
    FunctionId, InitialValue, ListId, ListInitialRecord, ListInitializer, ListMemory, ListMutation,
    ListMutationKind, ListProjection, ListProjectionKind, ListRowInitialField,
    MaterializationLocalId, MaterializationResultKind, PossibleCause, ProducerFunctionArgument,
    ProducerFunctionInstance, RowScope, ScopeId, SourceId, SourcePayloadDescriptor,
    SourcePayloadField, SourcePayloadSchema, SourcePort, StateCell, StateId, StateUpdateArm,
    TriggerOwnedArm, producer_identity_text,
};
use boon_semantic::{
    OutCallInstanceId, ProducerFunctionId, SemanticBindingId, SemanticBindingTargetV1,
    SemanticBlockBinding, SemanticCall, SemanticCallArgument, SemanticCallContextId,
    SemanticCallEntry, SemanticCallId, SemanticCallParameterBinding,
    SemanticCallParameterBindingKind, SemanticCallable, SemanticCallableId, SemanticCallableKind,
    SemanticContextualMaterialization, SemanticContextualOperationKind, SemanticContextualOrderKey,
    SemanticContextualRowPredecessor, SemanticDependencyTargetV1, SemanticDependencyTimingV1,
    SemanticDerivedValueKindV1, SemanticEventCauseV1, SemanticExecutionGraphV1, SemanticExprId,
    SemanticExpression, SemanticExpressionKind, SemanticFieldId, SemanticFunction,
    SemanticFunctionParameter, SemanticInitialValueV1, SemanticListId, SemanticListInitializerV1,
    SemanticListKeyPolicyV1, SemanticListMutationKindV1, SemanticListProjectionKindV1,
    SemanticLocalBindingId, SemanticLoweringContractV1, SemanticMaterializationId,
    SemanticMaterializationLocalId, SemanticMaterializationResultKind, SemanticNamedValueId,
    SemanticNamedValueStorageTargetV1, SemanticParameterId, SemanticPatternBinding,
    SemanticReactiveGraphV1, SemanticReadId, SemanticReadTargetV1, SemanticRecordField,
    SemanticResourceGraphV1, SemanticRoot, SemanticRowBinding, SemanticRowScopeId,
    SemanticScopeStorageGraphV1, SemanticSelectArm, SemanticSourceDef, SemanticSourceId,
    SemanticSourceOrigin, SemanticSourceRead, SemanticStateDef, SemanticStateId, SemanticStatement,
    SemanticStatementId, SemanticStatementKind, SemanticStorageBindingTargetV1,
    SemanticStorageExternalReferenceId, SemanticStorageExternalReferenceKindV1,
    SemanticStorageFieldId, SemanticStorageFieldOriginV1, SemanticStorageFieldRoleV1,
    SemanticStorageLocalMemberForwardingV1, SemanticStorageLocalMemberTargetV1,
    SemanticStorageProjectionId, SemanticTextSegment, SemanticTriggerArmId, SemanticValueId,
    SemanticValueListAuthorityId, SemanticValueMember, SemanticValueOrigin,
    SemanticValueProvenance, StaticOwnerDef, StaticOwnerId,
};
use boon_typecheck::{
    CheckedExternalDeclarationIdentityV1, CheckedExternalDeclarationKind, CheckedParameterKind,
    CheckedParameterRequirement, DeclId, FlowType,
};
use std::collections::{BTreeMap, BTreeSet};

type ExecutableCallInstanceMap = BTreeMap<OutCallInstanceId, usize>;
type ExecutableCallContextMap = BTreeMap<SemanticCallContextId, ExecutableCallContextId>;
type AllocatedCallIdentities = (ExecutableCallInstanceMap, ExecutableCallContextMap);
type RuntimeSourceMap = BTreeMap<SemanticSourceId, SourceId>;
type RuntimeStateMap = BTreeMap<SemanticStateId, StateId>;
type AllocatedRuntimeResourceIds = (RuntimeSourceMap, RuntimeStateMap);

fn semantic_data_type(value: &boon_typecheck::Type) -> crate::SemanticDataType {
    match value {
        boon_typecheck::Type::Text => crate::SemanticDataType::Text,
        boon_typecheck::Type::Number => crate::SemanticDataType::Number,
        boon_typecheck::Type::Bytes(boon_typecheck::BytesType::Dynamic) => {
            crate::SemanticDataType::Bytes { fixed_len: None }
        }
        boon_typecheck::Type::Bytes(boon_typecheck::BytesType::Fixed(fixed_len)) => {
            crate::SemanticDataType::Bytes {
                fixed_len: Some(*fixed_len),
            }
        }
        boon_typecheck::Type::Absent => crate::SemanticDataType::Unknown {
            reason: "private absence is not semantic data".to_owned(),
        },
        boon_typecheck::Type::VariantSet(variants) => {
            let mut variants = variants
                .iter()
                .map(|variant| match variant {
                    boon_typecheck::Variant::Tag(tag) => crate::SemanticVariantType {
                        tag: tag.clone(),
                        fields: Vec::new(),
                        open: false,
                    },
                    boon_typecheck::Variant::Tagged { tag, fields } => crate::SemanticVariantType {
                        tag: tag.clone(),
                        fields: semantic_type_fields(&fields.fields),
                        open: fields.open,
                    },
                })
                .collect::<Vec<_>>();
            variants.sort_by(|left, right| left.tag.cmp(&right.tag));
            crate::SemanticDataType::Variant { variants }
        }
        boon_typecheck::Type::Object(shape) => crate::SemanticDataType::Record {
            fields: semantic_type_fields(&shape.fields),
            open: shape.open,
        },
        boon_typecheck::Type::List(item) => crate::SemanticDataType::List {
            item: Box::new(semantic_data_type(item)),
        },
        boon_typecheck::Type::Union(members) => crate::SemanticDataType::Union {
            members: members.iter().map(semantic_data_type).collect(),
        },
        boon_typecheck::Type::Function { .. } => crate::SemanticDataType::Unknown {
            reason: "function values are not semantic memory data".to_owned(),
        },
        boon_typecheck::Type::RenderContract => crate::SemanticDataType::Unknown {
            reason: "render contracts are not semantic memory data".to_owned(),
        },
        boon_typecheck::Type::UnresolvedShape { reason } => crate::SemanticDataType::Unknown {
            reason: reason.clone(),
        },
        boon_typecheck::Type::Var(var) => crate::SemanticDataType::Unknown {
            reason: format!("unresolved type variable {}", var.0),
        },
        boon_typecheck::Type::Unknown => crate::SemanticDataType::Unknown {
            reason: "unknown type".to_owned(),
        },
    }
}

fn semantic_type_fields(
    fields: &BTreeMap<String, boon_typecheck::Type>,
) -> Vec<crate::SemanticTypeField> {
    fields
        .iter()
        .map(|(name, data_type)| crate::SemanticTypeField {
            name: name.clone(),
            data_type: semantic_data_type(data_type),
        })
        .collect()
}

/// Executable allocation for normalized lexical/route scope identity.
///
/// This is deliberately distinct from [`ScopeId`], which is the runtime row
/// scope domain. A semantic lexical scope must never be reinterpreted as a row
/// scope merely because both domains are currently dense.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(super) struct ExecutableLexicalScopeId(pub(super) usize);

/// Staged allocation for reactive field anchors. This is not a final
/// [`crate::FieldId`]: the lowering contract's complete storage-field domain
/// also owns list-authority, nested-record, and capture fields.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(super) struct MappedReactiveFieldId(pub(super) usize);

impl std::fmt::Display for MappedReactiveFieldId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(super) struct MappedReactiveBindingId(pub(super) usize);

impl std::fmt::Display for MappedReactiveBindingId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(super) struct MappedReactiveReadId(pub(super) usize);

impl std::fmt::Display for MappedReactiveReadId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(super) struct MappedReactiveTriggerId(pub(super) usize);

impl std::fmt::Display for MappedReactiveTriggerId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
    }
}

/// The only semantic-ID to executable-ID allocation table.
///
/// Dense semantic coordinates are not executable coordinates merely because
/// both currently use `usize`. Every conversion below must go through this
/// table so a later executable allocator can change without reopening semantic
/// discovery in `boon_ir`.
#[derive(Clone, Debug)]
pub(super) struct SemanticToExecutableMap {
    expressions: Vec<ExecutableExprId>,
    values: Vec<ExecutableExprId>,
    statements: Vec<ExecutableStatementId>,
    lexical_scopes: Vec<ExecutableLexicalScopeId>,
    sources: Vec<ExecutableSourceId>,
    states: Vec<ExecutableStateId>,
    callables: Vec<FunctionId>,
    call_expressions: Vec<Vec<ExecutableExprId>>,
    producer_functions: BTreeMap<ProducerFunctionId, FunctionId>,
    materializations: Vec<usize>,
    local_bindings: BTreeMap<SemanticLocalBindingId, ExecutableLocalBindingId>,
    call_instances: BTreeMap<OutCallInstanceId, usize>,
    call_contexts: BTreeMap<SemanticCallContextId, ExecutableCallContextId>,
    materialization_locals:
        BTreeMap<(StaticOwnerId, SemanticMaterializationLocalId), MaterializationLocalId>,
    lists: Vec<ListId>,
    row_scopes: Vec<ScopeId>,
    value_list_authorities: Vec<()>,
    runtime_sources: BTreeMap<SemanticSourceId, SourceId>,
    runtime_states: BTreeMap<SemanticStateId, StateId>,
}

#[derive(Clone, Debug)]
pub(super) struct MappedSemanticExecution {
    pub executable: ExecutableProgram,
    pub materializations: Vec<ContextualMaterialization>,
    pub static_owners: Vec<StaticOwnerDef>,
    pub id_map: SemanticToExecutableMap,
    semantic_callable_count: usize,
    semantic_call_count: usize,
    semantic_scope_count: usize,
    semantic_producer_functions: Vec<ProducerFunctionId>,
    semantic_list_count: usize,
    semantic_row_scope_count: usize,
    semantic_value_list_authority_count: usize,
}

#[derive(Clone, Debug)]
pub(super) struct MappedSemanticResources {
    pub row_scopes: Vec<RowScope>,
    pub lists: Vec<ListMemory>,
    pub sources: Vec<SourcePort>,
    pub state_cells: Vec<StateCell>,
    pub list_projections: Vec<ListProjection>,
    erased_value_list_authority_count: usize,
}

/// Exact mechanically mapped field identity. Parent/role topology remains a
/// lowering-contract responsibility, so this is intentionally not an
/// [`crate::ErasedFieldDef`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct MappedSemanticField {
    pub id: MappedReactiveFieldId,
    pub statement: ExecutableStatementId,
    pub declaration: DeclId,
    pub owner: Option<StaticOwnerId>,
    pub owner_ancestry: Vec<StaticOwnerId>,
    pub row: Option<ErasedRowBinding>,
    pub name: String,
    pub path: String,
    pub producer: ExecutableExprId,
    pub value: ExecutableExprId,
    pub flow_type: FlowType,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum MappedSemanticBindingTarget {
    Field {
        field: MappedReactiveFieldId,
    },
    Source {
        executable: ExecutableSourceId,
        runtime: SourceId,
    },
    State {
        executable: ExecutableStateId,
        runtime: StateId,
        published: bool,
        field: MappedReactiveFieldId,
        row: Option<ErasedRowBinding>,
    },
    List {
        list: ListId,
        field: MappedReactiveFieldId,
        row: ErasedRowBinding,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct MappedSemanticBinding {
    pub id: MappedReactiveBindingId,
    pub declaration: DeclId,
    pub statement: ExecutableStatementId,
    pub call_instance: Option<OutCallInstanceId>,
    pub owner: Option<StaticOwnerId>,
    pub owner_ancestry: Vec<StaticOwnerId>,
    pub producer: ExecutableExprId,
    pub value: ExecutableExprId,
    pub flow_type: FlowType,
    pub diagnostic_path: String,
    pub target: MappedSemanticBindingTarget,
}

/// A mapped read before bundle crossings and render topology assign the final
/// erased target variants.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum MappedSemanticReadTarget {
    Binding {
        binding: MappedReactiveBindingId,
        projection: Vec<String>,
    },
    SourcePayload {
        binding: MappedReactiveBindingId,
        source: SourceId,
        payload_projection: Vec<String>,
        projection: Vec<String>,
    },
    StateProjection {
        binding: MappedReactiveBindingId,
        state: StateId,
        projection: Vec<String>,
    },
    Local {
        binding: ExecutableLocalBindingId,
        declaration: DeclId,
        producer: ExecutableExprId,
        projection: Vec<String>,
    },
    External {
        canonical_path: String,
        external_identity: Option<CheckedExternalDeclarationIdentityV1>,
    },
    ElementState {
        context: ExecutableCallContextId,
        projection: Vec<String>,
    },
    MaterializationLocal {
        owner: StaticOwnerId,
        local: MaterializationLocalId,
        projection: Vec<String>,
    },
    FunctionParameter {
        parameter: ExecutableParameterId,
        projection: Vec<String>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct MappedSemanticRead {
    pub id: MappedReactiveReadId,
    pub expression: ExecutableExprId,
    pub value: ExecutableExprId,
    pub target: MappedSemanticReadTarget,
}

/// External-call references are resolved to bundle crossing ordinals only at
/// the atomic bundle boundary. Until then the concrete executable occurrence
/// and frozen external identity are the total, non-guessed target.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum MappedSemanticDependencyTarget {
    ExternalRead {
        read: MappedReactiveReadId,
    },
    ExternalCall {
        expression: ExecutableExprId,
        external_identity: CheckedExternalDeclarationIdentityV1,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct MappedSemanticDependencyUse {
    pub dependent: MappedReactiveBindingId,
    pub expression: ExecutableExprId,
    pub target: MappedSemanticDependencyTarget,
    pub timing: ErasedDependencyTiming,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct MappedSemanticSource {
    pub source: SourceId,
    pub owner: Option<StaticOwnerId>,
    pub owner_ancestry: Vec<StaticOwnerId>,
    pub executable: ExecutableSourceId,
    pub binding: MappedReactiveBindingId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct MappedSemanticProducerInstance {
    pub identity: [u8; 32],
    pub owner: StaticOwnerId,
    pub function: FunctionId,
    pub function_name: String,
    pub result_field: MappedReactiveFieldId,
    pub result_path: String,
    pub root: ExecutableExprId,
    pub mode: boon_semantic::ProducerMaterializationMode,
    pub invocation_source: Option<SourceId>,
    pub arguments: Vec<ProducerFunctionArgument>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct MappedSemanticTriggerArm {
    pub id: MappedReactiveTriggerId,
    pub cause: EventCause,
    pub gate_checked_expression: boon_typecheck::CheckedExprId,
    pub gate_expression: ExecutableExprId,
    pub owner: Option<StaticOwnerId>,
    pub route_scope: ExecutableLexicalScopeId,
    pub row_scope: Option<ScopeId>,
    pub output_expression: ExecutableExprId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct MappedSemanticStateUpdateArm {
    pub id: usize,
    pub state: StateId,
    pub trigger: MappedReactiveTriggerId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct MappedSemanticListMutation {
    pub id: usize,
    pub list_id: ListId,
    pub site: ExecutableExprId,
    pub cause: EventCause,
    pub owner: Option<StaticOwnerId>,
    pub route_scope: ExecutableLexicalScopeId,
    pub row_scope: Option<ScopeId>,
    pub trigger: MappedReactiveTriggerId,
    pub kind: ListMutationKind,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct MappedSemanticDerivedValue {
    pub field: MappedReactiveFieldId,
    pub executable_statement_id: ExecutableStatementId,
    pub path: String,
    pub kind: DerivedValueKind,
    pub materialized_list_id: Option<ListId>,
    pub materialized_row_scope_id: Option<ScopeId>,
    pub causes: Vec<EventCause>,
    pub trigger_arms: Vec<MappedReactiveTriggerId>,
    pub default_roots: Vec<ExecutableExprId>,
    pub sources: Vec<String>,
    pub indexed: bool,
    pub scope_id: Option<ScopeId>,
    pub startup_recompute: bool,
}

#[derive(Clone, Debug)]
struct SemanticReactiveToMappedMap {
    fields: Vec<MappedReactiveFieldId>,
    bindings: Vec<MappedReactiveBindingId>,
    reads: Vec<MappedReactiveReadId>,
    trigger_arms: Vec<MappedReactiveTriggerId>,
    state_update_arms: Vec<usize>,
    list_mutations: Vec<usize>,
    derived_values: Vec<usize>,
    dependency_uses: Vec<usize>,
    dependencies: Vec<usize>,
    host_effect_schedules: Vec<usize>,
}

#[derive(Clone, Debug)]
pub(super) struct MappedSemanticReactive {
    pub producer_function_instances: Vec<MappedSemanticProducerInstance>,
    pub fields: Vec<MappedSemanticField>,
    pub bindings: Vec<MappedSemanticBinding>,
    pub sources: Vec<MappedSemanticSource>,
    pub reads: Vec<MappedSemanticRead>,
    pub dependency_uses: Vec<MappedSemanticDependencyUse>,
    pub trigger_arms: Vec<MappedSemanticTriggerArm>,
    pub state_update_arms: Vec<MappedSemanticStateUpdateArm>,
    pub list_mutations: Vec<MappedSemanticListMutation>,
    pub derived_values: Vec<MappedSemanticDerivedValue>,
    pub dependencies: Vec<DependencyEdge>,
    pub possible_causes: Vec<PossibleCause>,
    id_map: SemanticReactiveToMappedMap,
    semantic_producer_instance_count: usize,
    referenced_trigger_ids: BTreeSet<MappedReactiveTriggerId>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum MappedSemanticStorageReadTarget {
    Binding {
        binding: ErasedBindingId,
        projection: Vec<String>,
    },
    SourcePayload {
        binding: ErasedBindingId,
        source: SourceId,
        payload_projection: Vec<String>,
        projection: Vec<String>,
    },
    StateProjection {
        binding: ErasedBindingId,
        state: StateId,
        projection: Vec<String>,
    },
    Local {
        binding: ExecutableLocalBindingId,
        declaration: DeclId,
        producer: ExecutableExprId,
        projection: Vec<String>,
    },
    BundleExternal {
        reference: SemanticStorageExternalReferenceId,
    },
    ElementState {
        context: ExecutableCallContextId,
        projection: Vec<String>,
    },
    MaterializationLocal {
        owner: StaticOwnerId,
        local: MaterializationLocalId,
        projection: Vec<String>,
    },
    FunctionParameter {
        parameter: ExecutableParameterId,
        projection: Vec<String>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct MappedSemanticStorageRead {
    pub id: ErasedReadId,
    pub expression: ExecutableExprId,
    pub target: MappedSemanticStorageReadTarget,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum MappedSemanticStorageDependencyTarget {
    BundleExternalRead {
        read: ErasedReadId,
        reference: SemanticStorageExternalReferenceId,
    },
    BundleExternalCall {
        reference: SemanticStorageExternalReferenceId,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct MappedSemanticStorageDependencyUse {
    pub dependent: ErasedBindingId,
    pub expression: ExecutableExprId,
    pub target: MappedSemanticStorageDependencyTarget,
    pub timing: ErasedDependencyTiming,
}

/// Final-ID join for one semantic call-invocation schedule.
///
/// C/E/F may consume this record when they bind distributed calls, but they
/// must not rediscover its bindings or trigger ownership from paths.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct MappedSemanticCallInvocationSchedule {
    pub expression: ExecutableExprId,
    pub value: ExecutableExprId,
    pub call: SemanticCallId,
    pub current_capable: bool,
    pub dependent_bindings: Vec<ErasedBindingId>,
    pub invocation_arms: Vec<TriggerOwnedArm>,
}

/// Final-ID join for one semantic host-effect schedule.
///
/// The semantic schedule ID remains its dense vector index. Referenced state
/// update arms are copied from the already-finalized arm domain, so later
/// assembly has no reason to walk executable expressions to infer scheduling.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct MappedSemanticHostEffectSchedule {
    pub id: usize,
    pub expression: ExecutableExprId,
    pub value: ExecutableExprId,
    pub call: SemanticCallId,
    pub checked_expression: boon_typecheck::CheckedExprId,
    pub owner: Option<StaticOwnerId>,
    pub operation: String,
    pub state_update_arms: Vec<StateUpdateArm>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum MappedSemanticExternalReferenceKind {
    Read {
        semantic_read: SemanticReadId,
        read: ErasedReadId,
        expression: ExecutableExprId,
    },
    Call {
        call: SemanticCallId,
        expression: ExecutableExprId,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct MappedSemanticExternalReference {
    pub id: SemanticStorageExternalReferenceId,
    pub kind: MappedSemanticExternalReferenceKind,
    pub canonical_path: String,
    pub external_identity: Option<CheckedExternalDeclarationIdentityV1>,
    pub bundle_ready: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum MappedSemanticNamedValueTarget {
    Field {
        binding: Option<ErasedBindingId>,
        field: FieldId,
    },
    Source {
        binding: ErasedBindingId,
        source: SourceId,
    },
    State {
        binding: ErasedBindingId,
        state: StateId,
        field: Option<FieldId>,
    },
    List {
        binding: ErasedBindingId,
        list: ListId,
        field: FieldId,
        row: ErasedRowBinding,
    },
    Value {
        expression: ExecutableExprId,
        value: ExecutableExprId,
        field: Option<FieldId>,
    },
    DiagnosticOnly {
        reason: boon_semantic::SemanticNamedValueDiagnosticOnlyReasonV1,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct MappedSemanticNamedValueProjection {
    pub id: SemanticStorageProjectionId,
    pub ordinal: usize,
    pub selector: String,
    pub field_ordinal: usize,
    pub input_type: boon_typecheck::Type,
    pub output_type: boon_typecheck::Type,
    pub storage_field: Option<FieldId>,
    pub expression: Option<ExecutableExprId>,
    pub value: Option<ExecutableExprId>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum MappedSemanticStorageTypePathSegment {
    ObjectField {
        selector: String,
        field_ordinal: usize,
    },
    ListItem,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct MappedSemanticStorageFixedBytesRefinement {
    pub path: Vec<MappedSemanticStorageTypePathSegment>,
    pub fixed_len: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum MappedSemanticStorageRepresentation {
    Exact,
    CheckedFixedBytes {
        refinements: Vec<MappedSemanticStorageFixedBytesRefinement>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct MappedSemanticNamedValue {
    pub named_value: SemanticNamedValueId,
    pub checked_statement: boon_typecheck::CheckedStatementId,
    pub diagnostic_path: String,
    pub origin_ordinal: usize,
    pub target_ordinal: usize,
    pub target: MappedSemanticNamedValueTarget,
    pub projection: Vec<MappedSemanticNamedValueProjection>,
    pub representation: MappedSemanticStorageRepresentation,
    pub flow_type: FlowType,
}

#[derive(Clone, Debug)]
struct SemanticStorageToErasedMap {
    storage_fields: Vec<FieldId>,
    reactive_fields: Vec<FieldId>,
    bindings: Vec<ErasedBindingId>,
    reads: Vec<ErasedReadId>,
    external_references: Vec<SemanticStorageExternalReferenceId>,
}

#[derive(Clone, Debug)]
pub(super) struct MappedSemanticStorage {
    pub owners: Vec<ErasedOwnerDef>,
    pub locals: Vec<ErasedLocalDef>,
    pub fields: Vec<ErasedFieldDef>,
    pub bindings: Vec<ErasedBinding>,
    pub sources: Vec<ErasedSourceDef>,
    pub reads: Vec<MappedSemanticStorageRead>,
    pub row_values: Vec<ErasedRowValue>,
    pub row_source_projections: Vec<ErasedRowSourceProjection>,
    pub dependency_uses: Vec<MappedSemanticStorageDependencyUse>,
    pub call_invocations: Vec<MappedSemanticCallInvocationSchedule>,
    pub host_effect_schedules: Vec<MappedSemanticHostEffectSchedule>,
    pub external_references: Vec<MappedSemanticExternalReference>,
    pub named_values: Vec<MappedSemanticNamedValue>,
    pub producer_function_instances: Vec<ProducerFunctionInstance>,
    pub derived_values: Vec<DerivedValue>,
    pub trigger_arms: Vec<TriggerOwnedArm>,
    pub state_update_arms: Vec<StateUpdateArm>,
    pub list_mutations: Vec<ListMutation>,
    pub dependencies: Vec<DependencyEdge>,
    pub possible_causes: Vec<PossibleCause>,
    named_value_checked_statements: Vec<boon_typecheck::CheckedStatementId>,
    id_map: SemanticStorageToErasedMap,
}

impl MappedSemanticExecution {
    pub fn validate_totality(&self) -> Result<(), String> {
        let emitted_local_bindings = self
            .executable
            .expressions
            .iter()
            .filter_map(|expression| match &expression.kind {
                ExecutableExpressionKind::Block { bindings, .. } => Some(bindings.as_slice()),
                _ => None,
            })
            .flatten()
            .map(|binding| binding.id)
            .collect::<Vec<_>>();
        let emitted_call_instances = self
            .executable
            .expressions
            .iter()
            .filter_map(|expression| match &expression.kind {
                ExecutableExpressionKind::Call { instance, .. } => Some(*instance),
                _ => None,
            })
            .collect::<Vec<_>>();
        let emitted_call_contexts = self
            .executable
            .expressions
            .iter()
            .filter_map(|expression| match &expression.kind {
                ExecutableExpressionKind::Call { contexts, .. } => Some(contexts.as_slice()),
                _ => None,
            })
            .flatten()
            .copied()
            .collect::<Vec<_>>();
        let emitted_call_expressions = self
            .executable
            .expressions
            .iter()
            .filter_map(|expression| {
                matches!(expression.kind, ExecutableExpressionKind::Call { .. })
                    .then_some(expression.id)
            })
            .collect::<Vec<_>>();
        let emitted_materialization_locals = self
            .materializations
            .iter()
            .map(|materialization| (materialization.owner, materialization.row_local))
            .collect::<Vec<_>>();
        let emitted_call_instance_count = emitted_call_instances
            .iter()
            .copied()
            .collect::<BTreeSet<_>>()
            .len();
        let emitted_call_context_count = emitted_call_contexts
            .iter()
            .copied()
            .collect::<BTreeSet<_>>()
            .len();
        let exact_lengths = [
            (
                "expression",
                self.id_map.expressions.len(),
                self.executable.expressions.len(),
            ),
            (
                "statement",
                self.id_map.statements.len(),
                self.executable.statements.len(),
            ),
            (
                "lexical scope",
                self.id_map.lexical_scopes.len(),
                self.semantic_scope_count,
            ),
            (
                "source",
                self.id_map.sources.len(),
                self.executable.sources.len(),
            ),
            (
                "state",
                self.id_map.states.len(),
                self.executable.states.len(),
            ),
            (
                "value",
                self.id_map.values.len(),
                self.executable.expressions.len(),
            ),
            (
                "callable identity",
                self.id_map.callables.len(),
                self.semantic_callable_count,
            ),
            (
                "call inventory",
                self.id_map.call_expressions.len(),
                self.semantic_call_count,
            ),
            (
                "call expression",
                self.id_map.call_expressions.iter().map(Vec::len).sum(),
                emitted_call_expressions.len(),
            ),
            (
                "producer function",
                self.id_map.producer_functions.len(),
                self.executable.functions.len(),
            ),
            (
                "producer function sequence",
                self.semantic_producer_functions.len(),
                self.executable.functions.len(),
            ),
            (
                "materialization",
                self.id_map.materializations.len(),
                self.materializations.len(),
            ),
            (
                "local binding",
                self.id_map.local_bindings.len(),
                emitted_local_bindings.len(),
            ),
            (
                "call instance",
                self.id_map.call_instances.len(),
                emitted_call_instance_count,
            ),
            (
                "call context",
                self.id_map.call_contexts.len(),
                emitted_call_context_count,
            ),
            (
                "materialization local",
                self.id_map.materialization_locals.len(),
                emitted_materialization_locals.len(),
            ),
            (
                "runtime source identity",
                self.id_map.runtime_sources.len(),
                self.executable.sources.len(),
            ),
            (
                "runtime state identity",
                self.id_map.runtime_states.len(),
                self.executable.states.len(),
            ),
            (
                "list identity",
                self.id_map.lists.len(),
                self.semantic_list_count,
            ),
            (
                "row scope identity",
                self.id_map.row_scopes.len(),
                self.semantic_row_scope_count,
            ),
            (
                "value-list authority erasure",
                self.id_map.value_list_authorities.len(),
                self.semantic_value_list_authority_count,
            ),
        ];
        for (label, mapped, emitted) in exact_lengths {
            if mapped != emitted {
                return Err(format!(
                    "semantic-to-executable {label} map covers {mapped} identities but emitted {emitted} records"
                ));
            }
        }
        for (index, expression) in self.executable.expressions.iter().enumerate() {
            let expected = self.id_map.expressions[index];
            if expression.id != expected {
                return Err(format!(
                    "mapped expression at index {index} emitted ID {}, expected {expected}",
                    expression.id
                ));
            }
            let expected_value = self.id_map.values[index];
            if expression.id != expected_value {
                return Err(format!(
                    "mapped value at index {index} emitted expression ID {}, expected {expected_value}",
                    expression.id
                ));
            }
        }
        for (index, statement) in self.executable.statements.iter().enumerate() {
            let expected = self.id_map.statements[index];
            if statement.id != expected {
                return Err(format!(
                    "mapped statement at index {index} emitted ID {}, expected {expected}",
                    statement.id
                ));
            }
        }
        for (index, source) in self.executable.sources.iter().enumerate() {
            let expected = self.id_map.sources[index];
            if source.id != expected {
                return Err(format!(
                    "mapped source at index {index} emitted ID {}, expected {expected}",
                    source.id
                ));
            }
        }
        for (index, state) in self.executable.states.iter().enumerate() {
            let expected = self.id_map.states[index];
            if state.id != expected {
                return Err(format!(
                    "mapped state at index {index} emitted ID {}, expected {expected}",
                    state.id
                ));
            }
        }
        for (index, callable) in self.id_map.callables.iter().copied().enumerate() {
            let expected = FunctionId(index);
            if callable != expected {
                return Err(format!(
                    "mapped callable at index {index} has ID {callable}, expected {expected}"
                ));
            }
        }
        for (index, function) in self.executable.functions.iter().enumerate() {
            let producer = self.semantic_producer_functions[index];
            let expected = self.id_map.producer_function(producer)?;
            if function.id != expected {
                return Err(format!(
                    "mapped producer function {producer} at index {index} emitted callable ID {}, expected {expected}",
                    function.id,
                ));
            }
        }
        for (index, materialization) in self.materializations.iter().enumerate() {
            let expected = self.id_map.materializations[index];
            if materialization.id != expected {
                return Err(format!(
                    "mapped materialization at index {index} emitted ID {}, expected {expected}",
                    materialization.id
                ));
            }
        }
        require_exact_identity_set(
            self.id_map.local_bindings.values().copied(),
            emitted_local_bindings,
            "local binding",
        )?;
        require_exact_identity_set(
            self.id_map.call_instances.values().copied(),
            emitted_call_instances,
            "call instance",
        )?;
        require_exact_identity_set(
            self.id_map.call_contexts.values().copied(),
            emitted_call_contexts,
            "call context",
        )?;
        require_exact_identity_set(
            self.id_map.call_expressions.iter().flatten().copied(),
            emitted_call_expressions,
            "call expression",
        )?;
        require_exact_identity_set(
            self.id_map
                .materialization_locals
                .iter()
                .map(|((owner, _), local)| (*owner, *local)),
            emitted_materialization_locals,
            "materialization local",
        )?;
        Ok(())
    }
}

impl SemanticToExecutableMap {
    fn allocate(
        graph: &SemanticExecutionGraphV1,
        resources: &SemanticResourceGraphV1,
    ) -> Result<Self, String> {
        require_dense(
            graph.expressions.iter().map(|expression| expression.id.0),
            "semantic expression",
        )?;
        require_dense(
            graph
                .expressions
                .iter()
                .map(|expression| expression.value_id.as_usize()),
            "semantic value",
        )?;
        require_dense(
            graph.statements.iter().map(|statement| statement.id.0),
            "semantic statement",
        )?;
        require_dense(
            graph.scopes.iter().map(|scope| scope.id.as_usize()),
            "semantic lexical scope",
        )?;
        require_dense(
            graph.sources.iter().map(|source| source.id.0),
            "semantic source",
        )?;
        require_dense(
            graph.states.iter().map(|state| state.id.0),
            "semantic state",
        )?;
        require_dense(
            graph
                .callables
                .iter()
                .map(|callable| callable.id.as_usize()),
            "semantic callable",
        )?;
        require_dense(
            graph.calls.iter().map(|call| call.id.as_usize()),
            "semantic call",
        )?;
        require_dense(
            graph
                .functions
                .iter()
                .map(|function| function.producer.as_usize()),
            "semantic producer function",
        )?;
        require_dense(
            graph
                .materializations
                .iter()
                .map(|materialization| materialization.id.0),
            "semantic materialization",
        )?;
        require_dense(
            resources.lists.iter().map(|list| list.id.as_usize()),
            "semantic list",
        )?;
        require_dense(
            resources.row_scopes.iter().map(|scope| scope.id.as_usize()),
            "semantic row scope",
        )?;
        require_dense(
            resources
                .value_list_authorities
                .iter()
                .map(|authority| authority.id.as_usize()),
            "semantic value-list authority",
        )?;

        validate_callable_and_call_inventory(graph)?;

        let expressions = (0..graph.expressions.len())
            .map(ExecutableExprId)
            .collect::<Vec<_>>();
        let unique_expressions = expressions.iter().copied().collect::<BTreeSet<_>>();
        if unique_expressions.len() != expressions.len() {
            return Err(
                "semantic expression allocation is not a one-to-one executable mapping".to_owned(),
            );
        }
        let values = allocate_values(graph, &expressions)?;
        let callables = (0..graph.callables.len())
            .map(FunctionId)
            .collect::<Vec<_>>();
        let call_expressions = allocate_call_expressions(graph, &expressions)?;
        let mut producer_functions = BTreeMap::new();
        for function in &graph.functions {
            let executable = exact_map(
                &callables,
                function.callable.as_usize(),
                "semantic producer callable",
                function.callable,
            )?;
            if producer_functions
                .insert(function.producer, executable)
                .is_some()
            {
                return Err(format!(
                    "semantic producer function {} is defined more than once",
                    function.producer
                ));
            }
        }
        let local_bindings = allocate_local_bindings(graph)?;
        let (call_instances, call_contexts) = allocate_call_identities(graph)?;
        let materialization_locals = allocate_materialization_locals(graph)?;
        let (runtime_sources, runtime_states) = allocate_runtime_resource_ids(graph, resources)?;

        let allocated = Self {
            expressions,
            values,
            statements: (0..graph.statements.len())
                .map(ExecutableStatementId)
                .collect(),
            lexical_scopes: (0..graph.scopes.len())
                .map(ExecutableLexicalScopeId)
                .collect(),
            sources: (0..graph.sources.len()).map(ExecutableSourceId).collect(),
            states: (0..graph.states.len()).map(ExecutableStateId).collect(),
            callables,
            call_expressions,
            producer_functions,
            materializations: (0..graph.materializations.len()).collect(),
            local_bindings,
            call_instances,
            call_contexts,
            materialization_locals,
            lists: (0..resources.lists.len()).map(ListId).collect(),
            row_scopes: (0..resources.row_scopes.len()).map(ScopeId).collect(),
            value_list_authorities: vec![(); resources.value_list_authorities.len()],
            runtime_sources,
            runtime_states,
        };
        allocated.validate_allocation_bijections()?;
        Ok(allocated)
    }

    fn validate_allocation_bijections(&self) -> Result<(), String> {
        require_unique_allocation(
            self.expressions.iter().copied(),
            self.expressions.len(),
            "expression",
        )?;
        require_unique_allocation(self.values.iter().copied(), self.values.len(), "value")?;
        require_unique_allocation(
            self.statements.iter().copied(),
            self.statements.len(),
            "statement",
        )?;
        require_unique_allocation(
            self.lexical_scopes.iter().copied(),
            self.lexical_scopes.len(),
            "lexical scope",
        )?;
        require_unique_allocation(self.sources.iter().copied(), self.sources.len(), "source")?;
        require_unique_allocation(self.states.iter().copied(), self.states.len(), "state")?;
        require_unique_allocation(
            self.callables.iter().copied(),
            self.callables.len(),
            "callable",
        )?;
        let call_expression_count = self.call_expressions.iter().map(Vec::len).sum();
        require_unique_allocation(
            self.call_expressions.iter().flatten().copied(),
            call_expression_count,
            "call expression",
        )?;
        require_unique_allocation(
            self.producer_functions.values().copied(),
            self.producer_functions.len(),
            "producer function",
        )?;
        require_unique_allocation(
            self.materializations.iter().copied(),
            self.materializations.len(),
            "materialization",
        )?;
        require_unique_allocation(
            self.local_bindings.values().copied(),
            self.local_bindings.len(),
            "local binding",
        )?;
        require_unique_allocation(
            self.call_instances.values().copied(),
            self.call_instances.len(),
            "call instance",
        )?;
        require_unique_allocation(
            self.call_contexts.values().copied(),
            self.call_contexts.len(),
            "call context",
        )?;
        require_unique_allocation(
            self.materialization_locals
                .iter()
                .map(|((owner, _), local)| (*owner, *local)),
            self.materialization_locals.len(),
            "materialization local",
        )?;
        require_unique_allocation(self.lists.iter().copied(), self.lists.len(), "list")?;
        require_unique_allocation(
            self.row_scopes.iter().copied(),
            self.row_scopes.len(),
            "row scope",
        )?;
        require_unique_allocation(
            self.runtime_sources.values().copied(),
            self.runtime_sources.len(),
            "runtime source",
        )?;
        require_unique_allocation(
            self.runtime_states.values().copied(),
            self.runtime_states.len(),
            "runtime state",
        )
    }

    pub(super) fn expression(&self, id: SemanticExprId) -> Result<ExecutableExprId, String> {
        exact_map(&self.expressions, id.as_usize(), "semantic expression", id)
    }

    /// V1 has exactly one value per semantic expression, so the executable
    /// value handle is the expression that produces it. This is an allocated
    /// lookup, not a numeric reinterpretation of `SemanticValueId`.
    pub(super) fn value(&self, id: SemanticValueId) -> Result<ExecutableExprId, String> {
        exact_map(&self.values, id.as_usize(), "semantic value", id)
    }

    fn statement(&self, id: SemanticStatementId) -> Result<ExecutableStatementId, String> {
        exact_map(&self.statements, id.as_usize(), "semantic statement", id)
    }

    pub(super) fn lexical_scope(
        &self,
        id: boon_semantic::SemanticScopeId,
    ) -> Result<ExecutableLexicalScopeId, String> {
        exact_map(
            &self.lexical_scopes,
            id.as_usize(),
            "semantic lexical scope",
            id,
        )
    }

    fn source(&self, id: SemanticSourceId) -> Result<ExecutableSourceId, String> {
        exact_map(&self.sources, id.as_usize(), "semantic source", id)
    }

    fn state(&self, id: SemanticStateId) -> Result<ExecutableStateId, String> {
        exact_map(&self.states, id.as_usize(), "semantic state", id)
    }

    pub(super) fn callable(&self, id: SemanticCallableId) -> Result<FunctionId, String> {
        exact_map(&self.callables, id.as_usize(), "semantic callable", id)
    }

    /// Resolves an exact semantic call occurrence. `SemanticCallId` alone is
    /// intentionally insufficient because one checked call can be expanded
    /// into multiple executable expressions.
    pub(super) fn call_expression(
        &self,
        id: SemanticCallId,
        occurrence: SemanticExprId,
    ) -> Result<ExecutableExprId, String> {
        let expressions = self
            .call_expressions
            .get(id.as_usize())
            .ok_or_else(|| format!("semantic call {id} has no executable mapping"))?;
        if expressions.is_empty() {
            return Err(format!(
                "semantic call {id} has no executable call-expression counterpart"
            ));
        }
        let occurrence = self.expression(occurrence)?;
        expressions.contains(&occurrence).then_some(occurrence).ok_or_else(|| {
            format!(
                "semantic call {id} does not own executable call-expression occurrence {occurrence}"
            )
        })
    }

    #[cfg(test)]
    fn unique_call_expression(&self, id: SemanticCallId) -> Result<ExecutableExprId, String> {
        let expressions = self
            .call_expressions
            .get(id.as_usize())
            .ok_or_else(|| format!("semantic call {id} has no executable mapping"))?;
        match expressions.as_slice() {
            [expression] => Ok(*expression),
            [] => Err(format!(
                "semantic call {id} has no executable call-expression counterpart"
            )),
            expressions => Err(format!(
                "semantic call {id} has {} executable call-expression counterparts and no exact singular mapping",
                expressions.len()
            )),
        }
    }

    pub(super) fn producer_function(&self, id: ProducerFunctionId) -> Result<FunctionId, String> {
        self.producer_functions
            .get(&id)
            .copied()
            .ok_or_else(|| format!("semantic producer function {id} has no executable mapping"))
    }

    fn materialization(&self, id: SemanticMaterializationId) -> Result<usize, String> {
        exact_map(
            &self.materializations,
            id.as_usize(),
            "semantic materialization",
            id,
        )
    }

    fn local(&self, id: SemanticLocalBindingId) -> Result<ExecutableLocalBindingId, String> {
        self.local_bindings
            .get(&id)
            .copied()
            .ok_or_else(|| format!("semantic local binding {id} has no executable mapping"))
    }

    fn list(&self, id: SemanticListId) -> Result<ListId, String> {
        exact_map(&self.lists, id.as_usize(), "semantic list", id)
    }

    fn row_scope(&self, id: SemanticRowScopeId) -> Result<ScopeId, String> {
        exact_map(&self.row_scopes, id.as_usize(), "semantic row scope", id)
    }

    fn value_list_authority(&self, id: SemanticValueListAuthorityId) -> Result<(), String> {
        exact_map(
            &self.value_list_authorities,
            id.as_usize(),
            "semantic value-list authority",
            id,
        )
    }

    pub(super) fn parameter(
        &self,
        id: SemanticParameterId,
    ) -> Result<ExecutableParameterId, String> {
        Ok(ExecutableParameterId {
            function: self.callable(id.callable)?,
            ordinal: id.ordinal,
        })
    }

    pub(super) fn call_instance(&self, id: OutCallInstanceId) -> Result<usize, String> {
        self.call_instances
            .get(&id)
            .copied()
            .ok_or_else(|| format!("semantic call instance {id} has no executable mapping"))
    }

    fn call_context(&self, id: SemanticCallContextId) -> Result<ExecutableCallContextId, String> {
        self.call_contexts.get(&id).copied().ok_or_else(|| {
            format!(
                "semantic call context {}:{} has no executable mapping",
                id.call_instance, id.ordinal
            )
        })
    }

    fn materialization_local(
        &self,
        owner: StaticOwnerId,
        id: SemanticMaterializationLocalId,
    ) -> Result<MaterializationLocalId, String> {
        self.materialization_locals
            .get(&(owner, id))
            .copied()
            .ok_or_else(|| {
                format!("semantic materialization local {owner}:{id} has no executable mapping")
            })
    }

    pub(super) fn runtime_source(&self, id: SemanticSourceId) -> Result<SourceId, String> {
        self.runtime_sources
            .get(&id)
            .copied()
            .ok_or_else(|| format!("semantic source {id} has no runtime mapping"))
    }

    fn runtime_state(&self, id: SemanticStateId) -> Result<StateId, String> {
        self.runtime_states
            .get(&id)
            .copied()
            .ok_or_else(|| format!("semantic state {id} has no runtime mapping"))
    }
}

fn require_unique_allocation<T: Ord>(
    values: impl IntoIterator<Item = T>,
    expected: usize,
    label: &str,
) -> Result<(), String> {
    let unique = values.into_iter().collect::<BTreeSet<_>>();
    if unique.len() == expected {
        Ok(())
    } else {
        Err(format!(
            "semantic-to-executable {label} allocation maps {expected} source identities to {} unique executable identities",
            unique.len()
        ))
    }
}

fn allocate_values(
    graph: &SemanticExecutionGraphV1,
    expressions: &[ExecutableExprId],
) -> Result<Vec<ExecutableExprId>, String> {
    let mut allocated = vec![None; graph.expressions.len()];
    for expression in &graph.expressions {
        let executable = exact_map(
            expressions,
            expression.id.as_usize(),
            "semantic value expression",
            expression.id,
        )?;
        let slot = allocated
            .get_mut(expression.value_id.as_usize())
            .ok_or_else(|| {
                format!(
                    "semantic value {} has no executable value slot",
                    expression.value_id
                )
            })?;
        if slot.replace(executable).is_some() {
            return Err(format!(
                "semantic value {} is produced by more than one expression",
                expression.value_id
            ));
        }
    }
    allocated
        .into_iter()
        .enumerate()
        .map(|(index, value)| {
            value.ok_or_else(|| {
                format!(
                    "semantic value {} has no executable expression counterpart",
                    SemanticValueId(index)
                )
            })
        })
        .collect()
}

fn allocate_call_expressions(
    graph: &SemanticExecutionGraphV1,
    expressions: &[ExecutableExprId],
) -> Result<Vec<Vec<ExecutableExprId>>, String> {
    let mut allocated = vec![Vec::new(); graph.calls.len()];
    for expression in &graph.expressions {
        let SemanticExpressionKind::Call { call, .. } = &expression.kind else {
            continue;
        };
        let executable = exact_map(
            expressions,
            expression.id.as_usize(),
            "semantic call expression",
            expression.id,
        )?;
        allocated
            .get_mut(call.as_usize())
            .ok_or_else(|| {
                format!(
                    "semantic expression {} references call {call} without an inventory record",
                    expression.id
                )
            })?
            .push(executable);
    }
    Ok(allocated)
}

fn validate_callable_and_call_inventory(graph: &SemanticExecutionGraphV1) -> Result<(), String> {
    let mut checked_callables = BTreeSet::new();
    for callable in &graph.callables {
        if !checked_callables.insert(callable.checked_callable) {
            return Err(format!(
                "semantic callable {} duplicates checked callable {}",
                callable.id, callable.checked_callable.0
            ));
        }
        require_semantic_scope(graph, callable.scope, "semantic callable")?;
        validate_external_callable_identity(callable)?;
        for (ordinal, parameter) in callable.parameters.iter().enumerate() {
            let expected = SemanticParameterId {
                callable: callable.id,
                ordinal,
            };
            if parameter.id != expected || parameter.ordinal != ordinal {
                return Err(format!(
                    "semantic callable {} parameter at index {ordinal} has noncanonical identity {:?}",
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
    for call in &graph.calls {
        if !checked_calls.insert(call.checked_call) {
            return Err(format!(
                "semantic call {} duplicates checked call {}",
                call.id, call.checked_call.0
            ));
        }
        let callable = semantic_callable(graph, call.callable)?;
        if call.external_identity != callable.external_identity {
            return Err(format!(
                "semantic call {} external identity differs from callable {}",
                call.id, callable.id
            ));
        }
        if call.function != callable.name
            || call.effect != callable.effect
            || call.role != callable.role
        {
            return Err(format!(
                "semantic call {} name/role/effect provenance differs from callable {}",
                call.id, callable.id
            ));
        }
        if let Some(owner) = call.owner_callable {
            semantic_callable(graph, owner).map_err(|error| {
                format!(
                    "semantic call {} has invalid owner callable: {error}",
                    call.id
                )
            })?;
        }
        if call.occurrence_segment.is_empty() {
            return Err(format!(
                "semantic call {} has an empty occurrence segment",
                call.id
            ));
        }
        let mut bound_formals = BTreeSet::new();
        for entry in &call.entries {
            let (formal, ordinal, name) = match entry {
                SemanticCallEntry::Input {
                    formal,
                    ordinal,
                    name,
                    evaluation_scope,
                    requirement,
                    ..
                } => {
                    let parameter = callable_parameter(callable, *ordinal, call.id, "input entry")?;
                    if parameter.kind != CheckedParameterKind::Value
                        || parameter.evaluation_scope != *evaluation_scope
                        || parameter.requirement != *requirement
                    {
                        return Err(format!(
                            "semantic call {} input ordinal {ordinal} has stale callable provenance",
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
                    let parameter =
                        callable_parameter(callable, *ordinal, call.id, "fresh OUT entry")?;
                    if parameter.kind != CheckedParameterKind::Out {
                        return Err(format!(
                            "semantic call {} fresh OUT ordinal {ordinal} is not an OUT parameter",
                            call.id
                        ));
                    }
                    require_semantic_scope(graph, *scope, "semantic call fresh OUT")?;
                    (*formal, *ordinal, name)
                }
                SemanticCallEntry::ForwardOut {
                    formal,
                    ordinal,
                    name,
                    ..
                } => {
                    let parameter =
                        callable_parameter(callable, *ordinal, call.id, "forward OUT entry")?;
                    if parameter.kind != CheckedParameterKind::Out {
                        return Err(format!(
                            "semantic call {} forwarded ordinal {ordinal} is not an OUT parameter",
                            call.id
                        ));
                    }
                    (*formal, *ordinal, name)
                }
            };
            let parameter = callable_parameter(callable, ordinal, call.id, "entry")?;
            if parameter.formal != formal || parameter.name != *name {
                return Err(format!(
                    "semantic call {} entry ordinal {ordinal} differs from callable {}",
                    call.id, callable.id
                ));
            }
            if !bound_formals.insert(formal) {
                return Err(format!(
                    "semantic call {} binds formal {} more than once",
                    call.id, formal.0
                ));
            }
        }
        for context in &call.contexts {
            require_semantic_scope(graph, context.scope, "semantic call context")?;
            if callable.contexts.get(context.signature).is_none() {
                return Err(format!(
                    "semantic call {} context signature {} is absent from callable {}",
                    call.id, context.signature, callable.id
                ));
            }
        }
    }
    Ok(())
}

fn validate_external_callable_identity(callable: &SemanticCallable) -> Result<(), String> {
    match (callable.kind, callable.external_identity) {
        (boon_typecheck::CheckedCallableKind::External, Some(identity))
            if identity.kind != CheckedExternalDeclarationKind::Callable =>
        {
            Err(format!(
                "semantic external callable {} carries a non-callable external identity",
                callable.id
            ))
        }
        (
            boon_typecheck::CheckedCallableKind::Builtin
            | boon_typecheck::CheckedCallableKind::User,
            Some(_),
        ) => Err(format!(
            "semantic non-external callable {} carries an external identity",
            callable.id
        )),
        _ => Ok(()),
    }
}

fn callable_parameter<'a>(
    callable: &'a SemanticCallable,
    ordinal: usize,
    call: SemanticCallId,
    context: &str,
) -> Result<&'a boon_semantic::SemanticCallableParameter, String> {
    callable.parameters.get(ordinal).ok_or_else(|| {
        format!(
            "semantic call {call} {context} references missing callable {} parameter ordinal {ordinal}",
            callable.id
        )
    })
}

fn semantic_callable(
    graph: &SemanticExecutionGraphV1,
    id: SemanticCallableId,
) -> Result<&SemanticCallable, String> {
    graph
        .callables
        .get(id.as_usize())
        .filter(|callable| callable.id == id)
        .ok_or_else(|| format!("missing semantic callable {id}"))
}

fn semantic_call(
    graph: &SemanticExecutionGraphV1,
    id: SemanticCallId,
) -> Result<&SemanticCall, String> {
    graph
        .calls
        .get(id.as_usize())
        .filter(|call| call.id == id)
        .ok_or_else(|| format!("missing semantic call {id}"))
}

fn require_semantic_scope(
    graph: &SemanticExecutionGraphV1,
    id: boon_semantic::SemanticScopeId,
    context: &str,
) -> Result<(), String> {
    if graph
        .scopes
        .get(id.as_usize())
        .is_none_or(|scope| scope.id != id)
    {
        return Err(format!("{context} references missing semantic scope {id}"));
    }
    Ok(())
}

fn allocate_local_bindings(
    graph: &SemanticExecutionGraphV1,
) -> Result<BTreeMap<SemanticLocalBindingId, ExecutableLocalBindingId>, String> {
    let mut definitions = BTreeMap::new();
    let mut references = BTreeSet::new();
    for expression in &graph.expressions {
        match &expression.kind {
            SemanticExpressionKind::Block { bindings, .. } => {
                for binding in bindings {
                    if let Some(previous) = definitions.insert(binding.id, expression.id) {
                        return Err(format!(
                            "semantic local binding {} is defined by both expressions {previous} and {}",
                            binding.id, expression.id
                        ));
                    }
                }
            }
            SemanticExpressionKind::LocalRead { binding, .. } => {
                references.insert(*binding);
            }
            _ => {}
        }
    }
    for reference in references {
        if !definitions.contains_key(&reference) {
            return Err(format!(
                "semantic local binding {reference} is referenced without a definition"
            ));
        }
    }
    let mut allocated = BTreeMap::new();
    for (index, id) in definitions.keys().copied().enumerate() {
        if id != SemanticLocalBindingId(index) {
            return Err(format!(
                "semantic local binding {id} is noncanonical at allocation index {index}"
            ));
        }
        allocated.insert(id, ExecutableLocalBindingId(index));
    }
    Ok(allocated)
}

fn allocate_call_identities(
    graph: &SemanticExecutionGraphV1,
) -> Result<AllocatedCallIdentities, String> {
    let mut instances = BTreeMap::<
        OutCallInstanceId,
        (
            SemanticCallId,
            SemanticCallableId,
            BTreeSet<SemanticCallContextId>,
        ),
    >::new();
    let mut context_definitions = BTreeSet::new();
    let mut context_references = BTreeSet::new();
    for expression in &graph.expressions {
        match &expression.kind {
            SemanticExpressionKind::Call {
                call,
                callable,
                instance,
                contexts,
                ..
            } => {
                let mut expression_contexts = BTreeSet::new();
                for context in contexts {
                    if context.call_instance != *instance {
                        return Err(format!(
                            "semantic call expression {} instance {instance} contains noncanonical context {}:{}",
                            expression.id, context.call_instance, context.ordinal
                        ));
                    }
                    if !expression_contexts.insert(*context) {
                        return Err(format!(
                            "semantic call expression {} contains duplicate context {instance}:{}",
                            expression.id, context.ordinal
                        ));
                    }
                    context_definitions.insert(*context);
                }
                let definition = (*call, *callable, expression_contexts);
                if let Some(previous) = instances.insert(*instance, definition.clone())
                    && previous != definition
                {
                    return Err(format!(
                        "semantic call instance {instance} has inconsistent call, callable, or context identity across expression occurrences"
                    ));
                }
            }
            SemanticExpressionKind::ElementState { context, .. } => {
                context_references.insert(*context);
            }
            _ => {}
        }
    }
    for reference in context_references {
        if !context_definitions.contains(&reference) {
            return Err(format!(
                "semantic call context {}:{} is referenced without a call definition",
                reference.call_instance, reference.ordinal
            ));
        }
    }
    let call_instances = instances
        .keys()
        .copied()
        .map(|instance| (instance, instance.as_usize()))
        .collect::<BTreeMap<_, _>>();
    let mut call_contexts = BTreeMap::new();
    for context in context_definitions {
        let call_instance = call_instances
            .get(&context.call_instance)
            .copied()
            .ok_or_else(|| {
                format!(
                    "semantic call context {}:{} has a dangling call instance",
                    context.call_instance, context.ordinal
                )
            })?;
        call_contexts.insert(
            context,
            ExecutableCallContextId {
                call_instance,
                ordinal: context.ordinal,
            },
        );
    }
    Ok((call_instances, call_contexts))
}

fn allocate_materialization_locals(
    graph: &SemanticExecutionGraphV1,
) -> Result<BTreeMap<(StaticOwnerId, SemanticMaterializationLocalId), MaterializationLocalId>, String>
{
    let mut definitions = BTreeMap::new();
    for materialization in &graph.materializations {
        require_semantic_owner(graph, materialization.owner, "semantic materialization")?;
        let key = (materialization.owner, materialization.row_local);
        if let Some(previous) = definitions.insert(key, materialization.id) {
            return Err(format!(
                "semantic materialization local {}:{} is defined by both materializations {previous} and {}",
                materialization.owner, materialization.row_local, materialization.id
            ));
        }
    }

    let mut references = BTreeSet::new();
    for expression in &graph.expressions {
        if let SemanticExpressionKind::MaterializationLocal { owner, local, .. } = &expression.kind
        {
            require_semantic_owner(graph, *owner, "semantic materialization-local expression")?;
            references.insert((*owner, *local));
        }
        for member in &expression.provenance.members {
            if let SemanticValueOrigin::MaterializationLocal { owner, local, .. } = &member.origin {
                require_semantic_owner(graph, *owner, "semantic materialization-local provenance")?;
                references.insert((*owner, *local));
            }
        }
    }
    for (owner, local) in references {
        if !definitions.contains_key(&(owner, local)) {
            return Err(format!(
                "semantic materialization local {owner}:{local} is referenced without a definition"
            ));
        }
    }

    let mut by_owner = BTreeMap::<StaticOwnerId, Vec<SemanticMaterializationLocalId>>::new();
    for (owner, local) in definitions.keys().copied() {
        u32::try_from(local.as_usize()).map_err(|_| {
            format!(
                "semantic materialization local {owner}:{local} exceeds executable u32 identity space"
            )
        })?;
        by_owner.entry(owner).or_default().push(local);
    }
    let mut allocated = BTreeMap::new();
    for (owner, locals) in by_owner {
        for (index, local) in locals.into_iter().enumerate() {
            if local != SemanticMaterializationLocalId(index) {
                return Err(format!(
                    "semantic materialization owner {owner} local {local} is noncanonical at allocation index {index}"
                ));
            }
            let executable = u32::try_from(index).map_err(|_| {
                format!(
                    "semantic materialization owner {owner} has more than {} executable locals",
                    u32::MAX
                )
            })?;
            allocated.insert((owner, local), MaterializationLocalId(executable));
        }
    }
    Ok(allocated)
}

fn require_semantic_owner(
    graph: &SemanticExecutionGraphV1,
    owner: StaticOwnerId,
    context: &str,
) -> Result<(), String> {
    if graph
        .static_owners
        .get(owner.as_usize())
        .is_none_or(|candidate| candidate.id != owner)
    {
        return Err(format!("{context} references missing static owner {owner}"));
    }
    Ok(())
}

fn allocate_runtime_resource_ids(
    graph: &SemanticExecutionGraphV1,
    resources: &SemanticResourceGraphV1,
) -> Result<AllocatedRuntimeResourceIds, String> {
    if resources.sources.len() != graph.sources.len() {
        return Err(format!(
            "semantic runtime source domain has {} resource records for {} execution definitions",
            resources.sources.len(),
            graph.sources.len()
        ));
    }
    let mut runtime_sources = BTreeMap::new();
    for (index, resource) in resources.sources.iter().enumerate() {
        let definition = graph.sources.get(index).ok_or_else(|| {
            format!(
                "semantic runtime source {} has no execution definition",
                resource.id
            )
        })?;
        if resource.id != definition.id || resource.id != SemanticSourceId(index) {
            return Err(format!(
                "semantic runtime source {} is noncanonical at allocation index {index}",
                resource.id
            ));
        }
        if runtime_sources
            .insert(resource.id, SourceId(index))
            .is_some()
        {
            return Err(format!(
                "semantic runtime source {} is defined more than once",
                resource.id
            ));
        }
    }

    if resources.states.len() != graph.states.len() {
        return Err(format!(
            "semantic runtime state domain has {} resource records for {} execution definitions",
            resources.states.len(),
            graph.states.len()
        ));
    }
    let mut runtime_states = BTreeMap::new();
    for (index, resource) in resources.states.iter().enumerate() {
        let definition = graph.states.get(index).ok_or_else(|| {
            format!(
                "semantic runtime state {} has no execution definition",
                resource.id
            )
        })?;
        if resource.id != definition.id || resource.id != SemanticStateId(index) {
            return Err(format!(
                "semantic runtime state {} is noncanonical at allocation index {index}",
                resource.id
            ));
        }
        if runtime_states.insert(resource.id, StateId(index)).is_some() {
            return Err(format!(
                "semantic runtime state {} is defined more than once",
                resource.id
            ));
        }
    }
    Ok((runtime_sources, runtime_states))
}

pub(super) fn map_semantic_execution(
    graph: &SemanticExecutionGraphV1,
    resources: &SemanticResourceGraphV1,
) -> Result<MappedSemanticExecution, String> {
    let id_map = SemanticToExecutableMap::allocate(graph, resources)?;
    let expressions = graph
        .expressions
        .iter()
        .map(|expression| map_expression(graph, &id_map, expression))
        .collect::<Result<Vec<_>, _>>()?;
    let statements = graph
        .statements
        .iter()
        .map(|statement| map_statement(&id_map, statement))
        .collect::<Result<Vec<_>, _>>()?;
    let sources = graph
        .sources
        .iter()
        .map(|source| map_source(&id_map, source))
        .collect::<Result<Vec<_>, _>>()?;
    let states = graph
        .states
        .iter()
        .map(|state| map_state(&id_map, state))
        .collect::<Result<Vec<_>, _>>()?;
    let roots = graph
        .roots
        .iter()
        .map(|root| map_root(graph, &id_map, root))
        .collect::<Result<Vec<_>, _>>()?;
    let functions = graph
        .functions
        .iter()
        .map(|function| map_function(graph, &id_map, function))
        .collect::<Result<Vec<_>, _>>()?;
    let materializations = graph
        .materializations
        .iter()
        .map(|materialization| map_materialization(&id_map, materialization))
        .collect::<Result<Vec<_>, _>>()?;
    let static_owners = graph
        .static_owners
        .iter()
        .map(|owner| StaticOwnerDef {
            id: owner.id,
            parent: owner.parent,
            child_ordinal: owner.child_ordinal,
        })
        .collect();

    Ok(MappedSemanticExecution {
        executable: ExecutableProgram {
            expressions,
            statements,
            sources,
            states,
            roots,
            functions,
        },
        materializations,
        static_owners,
        id_map,
        semantic_callable_count: graph.callables.len(),
        semantic_call_count: graph.calls.len(),
        semantic_scope_count: graph.scopes.len(),
        semantic_producer_functions: graph
            .functions
            .iter()
            .map(|function| function.producer)
            .collect(),
        semantic_list_count: resources.lists.len(),
        semantic_row_scope_count: resources.row_scopes.len(),
        semantic_value_list_authority_count: resources.value_list_authorities.len(),
    })
}

pub(super) fn map_semantic_resources(
    execution: &SemanticExecutionGraphV1,
    graph: &SemanticResourceGraphV1,
    ids: &SemanticToExecutableMap,
) -> Result<MappedSemanticResources, String> {
    validate_erased_resource_metadata(graph, ids)?;
    for authority in &graph.value_list_authorities {
        ids.value_list_authority(authority.id)?;
        ids.statement(authority.statement)?;
        ids.expression(authority.producer)?;
        match &authority.origin {
            boon_semantic::SemanticListResourceOriginV1::CheckedLiteral { .. } => {}
            boon_semantic::SemanticListResourceOriginV1::Derived {
                statement,
                producer,
            } => {
                ids.statement(*statement)?;
                ids.expression(*producer)?;
            }
        }
        validate_initializer_references(ids, &authority.initializer)?;
    }
    let row_scopes = graph
        .row_scopes
        .iter()
        .map(|scope| {
            let list = semantic_list_resource(graph, scope.list)?;
            if scope.semantic_path != list.semantic_path {
                return Err(format!(
                    "semantic row scope {} path `{}` differs from list {} path `{}`",
                    scope.id, scope.semantic_path, list.id, list.semantic_path
                ));
            }
            Ok(RowScope {
                id: ids.row_scope(scope.id)?,
                list: scope.semantic_path.clone(),
                function: "checked_list".to_owned(),
                row_scope: scope.stable_name.clone(),
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let lists = graph
        .lists
        .iter()
        .map(|list| {
            ids.statement(list.statement)?;
            ids.expression(list.producer)?;
            match &list.origin {
                boon_semantic::SemanticListResourceOriginV1::CheckedLiteral { .. } => {}
                boon_semantic::SemanticListResourceOriginV1::Derived {
                    statement,
                    producer,
                } => {
                    ids.statement(*statement)?;
                    ids.expression(*producer)?;
                }
            }
            validate_initializer_references(ids, &list.initializer)?;
            let (has_generation, hidden_key_type) = match list.key_policy {
                SemanticListKeyPolicyV1::GeneratedOccurrenceU64 { has_generation } => {
                    (has_generation, crate::hidden_key_type(&list.semantic_path))
                }
            };
            Ok(ListMemory {
                id: ids.list(list.id)?,
                name: list.semantic_path.clone(),
                source_line: list.span.line,
                row_scope_id: Some(ids.row_scope(list.row_scope)?),
                hidden_key_type,
                has_generation,
                graph_clones_per_item: 0,
                capacity: list.capacity,
                initializer: map_list_initializer(ids, &list.initializer)?,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let sources = graph
        .sources
        .iter()
        .map(|source| map_source_resource(execution, ids, source))
        .collect::<Result<Vec<_>, String>>()?;
    let state_cells = graph
        .states
        .iter()
        .map(|state| map_state_resource(execution, ids, state))
        .collect::<Result<Vec<_>, String>>()?;
    let list_projections = graph
        .list_projections
        .iter()
        .map(|projection| {
            let target = semantic_list_resource(graph, projection.target)?;
            let source = semantic_list_resource(graph, projection.source)?;
            Ok(ListProjection {
                target: target.semantic_path.clone(),
                list: source.semantic_path.clone(),
                kind: match &projection.kind {
                    SemanticListProjectionKindV1::Chunk { resolved_size, .. } => {
                        ListProjectionKind::Chunk {
                            size: *resolved_size,
                        }
                    }
                },
            })
        })
        .collect::<Result<Vec<_>, String>>()?;

    let mapped = MappedSemanticResources {
        row_scopes,
        lists,
        sources,
        state_cells,
        list_projections,
        erased_value_list_authority_count: graph.value_list_authorities.len(),
    };
    mapped.validate_totality(graph, ids)?;
    Ok(mapped)
}

fn validate_erased_resource_metadata(
    graph: &SemanticResourceGraphV1,
    ids: &SemanticToExecutableMap,
) -> Result<(), String> {
    for alias in &graph.aliases {
        match alias.target {
            boon_semantic::SemanticResourceAliasTargetV1::Source(source) => {
                ids.source(source)?;
            }
            boon_semantic::SemanticResourceAliasTargetV1::State(state) => {
                ids.state(state)?;
            }
        }
    }
    for binding in &graph.materialization_bindings {
        ids.materialization(binding.materialization)?;
        if let Some(source) = binding.source {
            map_row_binding(ids, source)?;
        }
        if let Some(target) = binding.target {
            map_row_binding(ids, target)?;
        }
        for predecessor in &binding.predecessors {
            map_row_predecessor(ids, predecessor)?;
        }
    }
    for producer in &graph.producer_resources {
        let callable = ids.callable(producer.callable)?;
        let expected = ids.producer_function(producer.function)?;
        if callable != expected {
            return Err(format!(
                "semantic producer resource callable {} maps to function {}, expected producer {}",
                producer.callable, callable, producer.function
            ));
        }
        ids.statement(producer.result_statement)?;
        if let Some(source) = producer.invocation_source {
            ids.source(source)?;
        }
    }
    Ok(())
}

impl MappedSemanticResources {
    fn validate_totality(
        &self,
        graph: &SemanticResourceGraphV1,
        ids: &SemanticToExecutableMap,
    ) -> Result<(), String> {
        let exact_lengths = [
            ("row scope", graph.row_scopes.len(), self.row_scopes.len()),
            ("list", graph.lists.len(), self.lists.len()),
            ("source resource", graph.sources.len(), self.sources.len()),
            ("state resource", graph.states.len(), self.state_cells.len()),
            (
                "runtime source identity",
                ids.runtime_sources.len(),
                self.sources.len(),
            ),
            (
                "runtime state identity",
                ids.runtime_states.len(),
                self.state_cells.len(),
            ),
            (
                "list projection",
                graph.list_projections.len(),
                self.list_projections.len(),
            ),
            (
                "erased value-list authority",
                graph.value_list_authorities.len(),
                self.erased_value_list_authority_count,
            ),
        ];
        for (label, semantic, executable) in exact_lengths {
            if semantic != executable {
                return Err(format!(
                    "semantic {label} graph has {semantic} records but executable mapping emitted {executable}"
                ));
            }
        }
        for (index, scope) in self.row_scopes.iter().enumerate() {
            let expected = ids.row_scope(SemanticRowScopeId(index))?;
            if scope.id != expected {
                return Err(format!(
                    "mapped row scope at index {index} emitted ID {}, expected {expected}",
                    scope.id
                ));
            }
        }
        for (index, list) in self.lists.iter().enumerate() {
            let expected = ids.list(SemanticListId(index))?;
            if list.id != expected {
                return Err(format!(
                    "mapped list at index {index} emitted ID {}, expected {expected}",
                    list.id
                ));
            }
        }
        for (index, source) in self.sources.iter().enumerate() {
            let semantic = graph.sources.get(index).ok_or_else(|| {
                format!("mapped source resource at index {index} has no semantic record")
            })?;
            let expected_runtime = ids.runtime_source(semantic.id)?;
            let expected_executable = ids.source(semantic.id)?;
            if source.id != expected_runtime
                || source.executable_source_id != Some(expected_executable)
            {
                return Err(format!(
                    "mapped source resource at index {index} emitted runtime ID {} and executable ID {:?}, expected {expected_runtime} and {expected_executable}",
                    source.id, source.executable_source_id
                ));
            }
        }
        for (index, state) in self.state_cells.iter().enumerate() {
            let semantic = graph.states.get(index).ok_or_else(|| {
                format!("mapped state resource at index {index} has no semantic record")
            })?;
            let expected_runtime = ids.runtime_state(semantic.id)?;
            let expected_executable = ids.state(semantic.id)?;
            if state.id != expected_runtime
                || state.executable_state_id != Some(expected_executable)
            {
                return Err(format!(
                    "mapped state resource at index {index} emitted runtime ID {} and executable ID {:?}, expected {expected_runtime} and {expected_executable}",
                    state.id, state.executable_state_id
                ));
            }
        }
        require_exact_identity_set(
            ids.runtime_sources.values().copied(),
            self.sources.iter().map(|source| source.id),
            "runtime source",
        )?;
        require_exact_identity_set(
            ids.runtime_states.values().copied(),
            self.state_cells.iter().map(|state| state.id),
            "runtime state",
        )?;
        let unique_projection_targets = self
            .list_projections
            .iter()
            .map(|projection| projection.target.as_str())
            .collect::<BTreeSet<_>>();
        if unique_projection_targets.len() != self.list_projections.len() {
            return Err(
                "semantic list projections do not map one-to-one to executable targets".to_owned(),
            );
        }
        Ok(())
    }
}

impl SemanticReactiveToMappedMap {
    fn allocate(graph: &SemanticReactiveGraphV1) -> Result<Self, String> {
        require_dense(
            graph.fields.iter().map(|field| field.id.as_usize()),
            "semantic reactive field",
        )?;
        require_dense(
            graph.bindings.iter().map(|binding| binding.id.as_usize()),
            "semantic reactive binding",
        )?;
        require_dense(
            graph.reads.iter().map(|read| read.id.as_usize()),
            "semantic reactive read",
        )?;
        require_dense(
            graph
                .dependency_uses
                .iter()
                .map(|dependency| dependency.id.as_usize()),
            "semantic reactive dependency use",
        )?;
        require_dense(
            graph
                .trigger_arms
                .iter()
                .map(|trigger| trigger.id.as_usize()),
            "semantic reactive trigger arm",
        )?;
        require_dense(
            graph.state_update_arms.iter().map(|arm| arm.id.as_usize()),
            "semantic reactive state update arm",
        )?;
        require_dense(
            graph
                .list_mutations
                .iter()
                .map(|mutation| mutation.id.as_usize()),
            "semantic reactive list mutation",
        )?;
        require_dense(
            graph
                .derived_values
                .iter()
                .map(|derived| derived.id.as_usize()),
            "semantic reactive derived value",
        )?;
        require_dense(
            graph
                .dependencies
                .iter()
                .map(|dependency| dependency.id.as_usize()),
            "semantic reactive dependency edge",
        )?;
        require_dense(
            graph
                .host_effect_schedules
                .iter()
                .map(|schedule| schedule.id.as_usize()),
            "semantic reactive host-effect schedule",
        )?;

        Ok(Self {
            fields: (0..graph.fields.len()).map(MappedReactiveFieldId).collect(),
            bindings: (0..graph.bindings.len())
                .map(MappedReactiveBindingId)
                .collect(),
            reads: (0..graph.reads.len()).map(MappedReactiveReadId).collect(),
            trigger_arms: (0..graph.trigger_arms.len())
                .map(MappedReactiveTriggerId)
                .collect(),
            state_update_arms: (0..graph.state_update_arms.len()).collect(),
            list_mutations: (0..graph.list_mutations.len()).collect(),
            derived_values: (0..graph.derived_values.len()).collect(),
            dependency_uses: (0..graph.dependency_uses.len()).collect(),
            dependencies: (0..graph.dependencies.len()).collect(),
            host_effect_schedules: (0..graph.host_effect_schedules.len()).collect(),
        })
    }

    fn field(&self, id: SemanticFieldId) -> Result<MappedReactiveFieldId, String> {
        exact_map(&self.fields, id.as_usize(), "semantic field", id)
    }

    fn binding(&self, id: SemanticBindingId) -> Result<MappedReactiveBindingId, String> {
        exact_map(&self.bindings, id.as_usize(), "semantic binding", id)
    }

    fn read(&self, id: SemanticReadId) -> Result<MappedReactiveReadId, String> {
        exact_map(&self.reads, id.as_usize(), "semantic read", id)
    }

    fn trigger(&self, id: SemanticTriggerArmId) -> Result<MappedReactiveTriggerId, String> {
        exact_map(
            &self.trigger_arms,
            id.as_usize(),
            "semantic trigger arm",
            id,
        )
    }

    fn state_update_arm(
        &self,
        id: boon_semantic::SemanticStateUpdateArmId,
    ) -> Result<usize, String> {
        exact_map(
            &self.state_update_arms,
            id.as_usize(),
            "semantic state update arm",
            id,
        )
    }

    fn list_mutation(&self, id: boon_semantic::SemanticListMutationId) -> Result<usize, String> {
        exact_map(
            &self.list_mutations,
            id.as_usize(),
            "semantic list mutation",
            id,
        )
    }
}

fn validate_reactive_dependency_closure(
    resource_graph: &SemanticResourceGraphV1,
    graph: &SemanticReactiveGraphV1,
) -> Result<(), String> {
    let mut expected_edges = BTreeSet::new();
    let mut expected_causes = resource_graph
        .states
        .iter()
        .map(|state| (state.id, BTreeSet::new()))
        .collect::<BTreeMap<_, _>>();

    for arm in &graph.state_update_arms {
        let trigger = graph
            .trigger_arms
            .get(arm.trigger.as_usize())
            .filter(|candidate| candidate.id == arm.trigger)
            .ok_or_else(|| {
                format!(
                    "semantic state update arm {} references missing trigger {}",
                    arm.id, arm.trigger
                )
            })?;
        let target = semantic_state_resource(resource_graph, arm.state)?;
        let cause_scoped = match trigger.cause {
            SemanticEventCauseV1::Source(source) => {
                semantic_source_resource(resource_graph, source)?.scoped
            }
            SemanticEventCauseV1::State(state) => {
                semantic_state_resource(resource_graph, state)?.scoped
            }
        };
        expected_edges.insert((trigger.cause, arm.state, target.scoped || cause_scoped));
        expected_causes
            .get_mut(&arm.state)
            .ok_or_else(|| {
                format!(
                    "semantic state update arm {} targets state {} outside the resource domain",
                    arm.id, arm.state
                )
            })?
            .insert(trigger.cause);
    }

    if graph.dependencies.len() != expected_edges.len() {
        return Err(format!(
            "semantic dependency closure contains {} edges, expected {} exact state-trigger edges",
            graph.dependencies.len(),
            expected_edges.len()
        ));
    }
    for (index, (dependency, expected)) in graph
        .dependencies
        .iter()
        .zip(expected_edges.iter().copied())
        .enumerate()
    {
        if dependency.id.as_usize() != index
            || (dependency.from, dependency.to, dependency.indexed) != expected
        {
            return Err(format!(
                "semantic dependency edge {} is not the canonical edge {:?} at index {index}",
                dependency.id, expected
            ));
        }
    }

    if graph.possible_causes.len() != expected_causes.len() {
        return Err(format!(
            "semantic possible-causes closure contains {} state entries, expected {}",
            graph.possible_causes.len(),
            expected_causes.len()
        ));
    }
    for ((state, causes), actual) in expected_causes.iter().zip(&graph.possible_causes) {
        let expected = causes.iter().copied().collect::<Vec<_>>();
        if actual.state != *state || actual.causes != expected {
            return Err(format!(
                "semantic possible-causes entry for state {} is not the exact state-trigger cause set",
                actual.state
            ));
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) fn map_semantic_reactive(
    execution: &SemanticExecutionGraphV1,
    resource_graph: &SemanticResourceGraphV1,
    graph: &SemanticReactiveGraphV1,
    ids: &SemanticToExecutableMap,
    resources: &MappedSemanticResources,
) -> Result<MappedSemanticReactive, String> {
    let reactive_ids = SemanticReactiveToMappedMap::allocate(graph)?;
    validate_reactive_dependency_closure(resource_graph, graph)?;
    let fields = graph
        .fields
        .iter()
        .map(|field| map_reactive_field(execution, ids, &reactive_ids, field))
        .collect::<Result<Vec<_>, _>>()?;
    let bindings = graph
        .bindings
        .iter()
        .map(|binding| {
            map_reactive_binding(
                execution,
                resource_graph,
                ids,
                &reactive_ids,
                &fields,
                binding,
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    let sources = map_reactive_sources(execution, resource_graph, ids, &bindings)?;
    let reads = graph
        .reads
        .iter()
        .map(|read| map_reactive_read(execution, resource_graph, ids, &reactive_ids, read))
        .collect::<Result<Vec<_>, _>>()?;
    let trigger_arms = graph
        .trigger_arms
        .iter()
        .map(|trigger| map_reactive_trigger(execution, ids, &reactive_ids, trigger))
        .collect::<Result<Vec<_>, _>>()?;

    let mut referenced_trigger_ids = BTreeSet::new();
    let mapped_state_transitions = graph
        .state_update_arms
        .iter()
        .map(|arm| {
            let trigger = reactive_ids.trigger(arm.trigger)?;
            referenced_trigger_ids.insert(trigger);
            trigger_arms.get(trigger.0).ok_or_else(|| {
                format!(
                    "semantic state update arm {} maps to missing trigger {}",
                    arm.id, arm.trigger
                )
            })?;
            let id = *reactive_ids
                .state_update_arms
                .get(arm.id.as_usize())
                .ok_or_else(|| {
                    format!("semantic state update arm {} has no staged mapping", arm.id)
                })?;
            Ok(MappedSemanticStateUpdateArm {
                id,
                state: ids.runtime_state(arm.state)?,
                trigger,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;

    let list_mutations = graph
        .list_mutations
        .iter()
        .map(|mutation| {
            map_reactive_list_mutation(
                execution,
                graph,
                ids,
                &reactive_ids,
                mutation,
                &mut referenced_trigger_ids,
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    let derived_mapping = ReactiveDerivedMappingContext {
        execution,
        resource_graph,
        graph,
        ids,
        reactive_ids: &reactive_ids,
        resources,
        fields: &fields,
        bindings: &bindings,
        trigger_arms: &trigger_arms,
    };
    let derived_values = graph
        .derived_values
        .iter()
        .map(|derived| {
            map_reactive_derived_value(&derived_mapping, derived, &mut referenced_trigger_ids)
        })
        .collect::<Result<Vec<_>, _>>()?;
    validate_reactive_call_and_host_schedules(
        execution,
        graph,
        ids,
        &reactive_ids,
        &mut referenced_trigger_ids,
    )?;
    let dependency_uses = graph
        .dependency_uses
        .iter()
        .map(|dependency| {
            map_reactive_dependency_use(execution, graph, ids, &reactive_ids, dependency)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let dependencies = graph
        .dependencies
        .iter()
        .map(|dependency| {
            let from = semantic_event_cause_path(dependency.from, ids, resources)?;
            let state = semantic_state_resource(resource_graph, dependency.to)?;
            Ok(DependencyEdge {
                from,
                to: state.path.clone(),
                indexed: dependency.indexed,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let possible_causes = graph
        .possible_causes
        .iter()
        .enumerate()
        .map(|(index, causes)| {
            let expected = SemanticStateId(index);
            if causes.state != expected {
                return Err(format!(
                    "semantic possible-causes entry at index {index} covers {}, expected {expected}",
                    causes.state
                ));
            }
            let state = semantic_state_resource(resource_graph, causes.state)?;
            let mut sources = causes
                .causes
                .iter()
                .copied()
                .map(|cause| semantic_event_cause_path(cause, ids, resources))
                .collect::<Result<Vec<_>, _>>()?;
            sources.sort();
            sources.dedup();
            Ok(PossibleCause {
                target: state.path.clone(),
                sources,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let producer_function_instances = graph
        .producer_instances
        .iter()
        .map(|instance| {
            map_reactive_producer_instance(execution, resource_graph, ids, &fields, instance)
        })
        .collect::<Result<Vec<_>, _>>()?;

    let mapped = MappedSemanticReactive {
        producer_function_instances,
        fields,
        bindings,
        sources,
        reads,
        dependency_uses,
        trigger_arms,
        state_update_arms: mapped_state_transitions,
        list_mutations,
        derived_values,
        dependencies,
        possible_causes,
        id_map: reactive_ids,
        semantic_producer_instance_count: graph.producer_instances.len(),
        referenced_trigger_ids,
    };
    mapped.validate_totality(graph, resource_graph, ids)?;
    Ok(mapped)
}

fn semantic_execution_statement(
    graph: &SemanticExecutionGraphV1,
    id: SemanticStatementId,
) -> Result<&SemanticStatement, String> {
    graph
        .statements
        .get(id.as_usize())
        .filter(|statement| statement.id == id)
        .ok_or_else(|| format!("missing semantic statement {id}"))
}

fn semantic_execution_expression(
    graph: &SemanticExecutionGraphV1,
    id: SemanticExprId,
) -> Result<&SemanticExpression, String> {
    graph
        .expressions
        .get(id.as_usize())
        .filter(|expression| expression.id == id)
        .ok_or_else(|| format!("missing semantic expression {id}"))
}

fn semantic_source_resource(
    graph: &SemanticResourceGraphV1,
    id: SemanticSourceId,
) -> Result<&boon_semantic::SemanticSourceResourceV1, String> {
    graph
        .sources
        .get(id.as_usize())
        .filter(|source| source.id == id)
        .ok_or_else(|| format!("missing semantic source resource {id}"))
}

fn semantic_state_resource(
    graph: &SemanticResourceGraphV1,
    id: SemanticStateId,
) -> Result<&boon_semantic::SemanticStateResourceV1, String> {
    graph
        .states
        .get(id.as_usize())
        .filter(|state| state.id == id)
        .ok_or_else(|| format!("missing semantic state resource {id}"))
}

fn mapped_owner_ancestry(
    graph: &SemanticExecutionGraphV1,
    owner: Option<StaticOwnerId>,
) -> Result<Vec<StaticOwnerId>, String> {
    // Executable storage uses canonical root-to-leaf ancestry, with the
    // concrete owner last. The semantic resource graph retains its historical
    // leaf-to-root audit order and is normalized only at that explicit join.
    let mut ancestry = Vec::new();
    let mut next = owner;
    let mut remaining = graph.static_owners.len().saturating_add(1);
    while let Some(id) = next {
        if remaining == 0 {
            return Err("semantic static-owner ancestry contains a cycle".to_owned());
        }
        remaining -= 1;
        let definition = graph
            .static_owners
            .get(id.as_usize())
            .filter(|definition| definition.id == id)
            .ok_or_else(|| format!("missing semantic static owner {id}"))?;
        ancestry.push(id);
        next = definition.parent;
    }
    ancestry.reverse();
    Ok(ancestry)
}

fn map_reactive_field(
    execution: &SemanticExecutionGraphV1,
    ids: &SemanticToExecutableMap,
    reactive_ids: &SemanticReactiveToMappedMap,
    field: &boon_semantic::SemanticFieldV1,
) -> Result<MappedSemanticField, String> {
    let statement = semantic_execution_statement(execution, field.statement)?;
    if statement.declaration != Some(field.declaration)
        || statement.value != Some(field.producer)
        || statement.flow_type.as_ref() != Some(&field.flow_type)
    {
        return Err(format!(
            "semantic field {} has stale statement/declaration/value/type provenance",
            field.id
        ));
    }
    let expression = semantic_execution_expression(execution, field.producer)?;
    if expression.value_id != field.value
        || expression.owner != field.owner
        || expression.flow_type != field.flow_type
    {
        return Err(format!(
            "semantic field {} has stale producer/value/owner/type provenance",
            field.id
        ));
    }
    let producer = ids.expression(field.producer)?;
    let value = ids.value(field.value)?;
    if producer != value {
        return Err(format!(
            "semantic field {} producer {} and value {} map to different executable values",
            field.id, field.producer, field.value
        ));
    }
    Ok(MappedSemanticField {
        id: reactive_ids.field(field.id)?,
        statement: ids.statement(field.statement)?,
        declaration: field.declaration,
        owner: field.owner,
        owner_ancestry: mapped_owner_ancestry(execution, field.owner)?,
        row: field.row.map(|row| map_row_binding(ids, row)).transpose()?,
        name: field.name.clone(),
        path: field.path.clone(),
        producer,
        value,
        flow_type: field.flow_type.clone(),
    })
}

fn mapped_field(
    fields: &[MappedSemanticField],
    id: MappedReactiveFieldId,
) -> Result<&MappedSemanticField, String> {
    fields
        .get(id.0)
        .filter(|field| field.id == id)
        .ok_or_else(|| format!("missing mapped semantic field {id}"))
}

fn unique_mapped_field_for_statement<'a>(
    fields: &'a [MappedSemanticField],
    statement: ExecutableStatementId,
    declaration: DeclId,
    context: &str,
) -> Result<&'a MappedSemanticField, String> {
    let candidates = fields
        .iter()
        .filter(|field| field.statement == statement && field.declaration == declaration)
        .collect::<Vec<_>>();
    let [field] = candidates.as_slice() else {
        return Err(format!(
            "{context} statement {statement} declaration {} resolves to {} mapped fields",
            declaration.0,
            candidates.len()
        ));
    };
    Ok(*field)
}

fn map_reactive_binding(
    execution: &SemanticExecutionGraphV1,
    resources: &SemanticResourceGraphV1,
    ids: &SemanticToExecutableMap,
    reactive_ids: &SemanticReactiveToMappedMap,
    fields: &[MappedSemanticField],
    binding: &boon_semantic::SemanticBindingV1,
) -> Result<MappedSemanticBinding, String> {
    let statement = semantic_execution_statement(execution, binding.statement)?;
    let expression = semantic_execution_expression(execution, binding.producer)?;
    let producer_matches_statement =
        matches!(binding.target, SemanticBindingTargetV1::Source { .. })
            || statement.value == Some(binding.producer);
    if statement.declaration != Some(binding.declaration)
        || !producer_matches_statement
        || expression.value_id != binding.value
        || expression.owner != binding.owner
        || expression.flow_type != binding.flow_type
    {
        return Err(format!(
            "semantic binding {} has stale statement/producer/value/owner/type provenance: statement={} declaration={:?}/{}, value={:?}/{}, expression_value={}/{}, owner={:?}/{:?}, flow={:?}/{:?}",
            binding.id,
            binding.statement,
            statement.declaration,
            binding.declaration.0,
            statement.value,
            binding.producer,
            expression.value_id,
            binding.value,
            expression.owner,
            binding.owner,
            expression.flow_type,
            binding.flow_type,
        ));
    }
    if statement.call_instance != binding.call_instance {
        return Err(format!(
            "semantic binding {} call-instance provenance differs from statement {}",
            binding.id, binding.statement
        ));
    }
    let executable_statement = ids.statement(binding.statement)?;
    let producer = ids.expression(binding.producer)?;
    let value = ids.value(binding.value)?;
    if producer != value {
        return Err(format!(
            "semantic binding {} producer and value map to different executable values",
            binding.id
        ));
    }
    let call_instance = binding.call_instance;
    let (diagnostic_path, target) = match binding.target {
        SemanticBindingTargetV1::Field { field } => {
            let field = mapped_field(fields, reactive_ids.field(field)?)?;
            if field.statement != executable_statement
                || field.declaration != binding.declaration
                || field.producer != producer
            {
                return Err(format!(
                    "semantic binding {} field target {} has inconsistent statement/declaration/producer identity",
                    binding.id, field.id
                ));
            }
            (
                field.path.clone(),
                MappedSemanticBindingTarget::Field { field: field.id },
            )
        }
        SemanticBindingTargetV1::Source { source } => {
            let source = semantic_source_resource(resources, source)?;
            if source.statement != binding.statement
                || source.declaration != binding.declaration
                || source.expression != binding.producer
            {
                return Err(format!(
                    "semantic binding {} source target {} has stale provenance",
                    binding.id, source.id
                ));
            }
            (
                source.semantic_path.clone(),
                MappedSemanticBindingTarget::Source {
                    executable: ids.source(source.id)?,
                    runtime: ids.runtime_source(source.id)?,
                },
            )
        }
        SemanticBindingTargetV1::State { state } => {
            let state = semantic_state_resource(resources, state)?;
            if state.statement != binding.statement
                || state.declaration != binding.declaration
                || state.expression != binding.producer
            {
                return Err(format!(
                    "semantic binding {} state target {} has stale provenance: statement={}/{}, declaration={:?}/{:?}, expression={}/{}",
                    binding.id,
                    state.id,
                    state.statement,
                    binding.statement,
                    state.declaration,
                    binding.declaration,
                    state.expression,
                    binding.producer,
                ));
            }
            let field = unique_mapped_field_for_statement(
                fields,
                executable_statement,
                binding.declaration,
                "semantic state binding",
            )?;
            (
                state.path.clone(),
                MappedSemanticBindingTarget::State {
                    executable: ids.state(state.id)?,
                    runtime: ids.runtime_state(state.id)?,
                    published: state.published,
                    field: field.id,
                    row: state
                        .target_list
                        .zip(state.row_scope)
                        .map(|(list, scope)| {
                            map_row_binding(ids, SemanticRowBinding { list, scope })
                        })
                        .transpose()?,
                },
            )
        }
        SemanticBindingTargetV1::List { list } => {
            let list = semantic_list_resource(resources, list)?;
            if list.statement != binding.statement
                || list.declaration != binding.declaration
                || list.producer != binding.producer
            {
                return Err(format!(
                    "semantic binding {} list target {} has stale provenance",
                    binding.id, list.id
                ));
            }
            let field = unique_mapped_field_for_statement(
                fields,
                executable_statement,
                binding.declaration,
                "semantic list binding",
            )?;
            (
                list.semantic_path.clone(),
                MappedSemanticBindingTarget::List {
                    list: ids.list(list.id)?,
                    field: field.id,
                    row: map_row_binding(
                        ids,
                        SemanticRowBinding {
                            list: list.id,
                            scope: list.row_scope,
                        },
                    )?,
                },
            )
        }
    };
    Ok(MappedSemanticBinding {
        id: reactive_ids.binding(binding.id)?,
        declaration: binding.declaration,
        statement: executable_statement,
        call_instance,
        owner: binding.owner,
        owner_ancestry: mapped_owner_ancestry(execution, binding.owner)?,
        producer,
        value,
        flow_type: binding.flow_type.clone(),
        diagnostic_path,
        target,
    })
}

fn map_reactive_sources(
    execution: &SemanticExecutionGraphV1,
    resources: &SemanticResourceGraphV1,
    ids: &SemanticToExecutableMap,
    bindings: &[MappedSemanticBinding],
) -> Result<Vec<MappedSemanticSource>, String> {
    resources
        .sources
        .iter()
        .map(|source| {
            let executable = ids.source(source.id)?;
            let runtime = ids.runtime_source(source.id)?;
            let candidates = bindings
                .iter()
                .filter(|binding| {
                    matches!(
                        binding.target,
                        MappedSemanticBindingTarget::Source {
                            executable: candidate_executable,
                            runtime: candidate_runtime,
                        } if candidate_executable == executable && candidate_runtime == runtime
                    )
                })
                .collect::<Vec<_>>();
            let [binding] = candidates.as_slice() else {
                return Err(format!(
                    "semantic source {} resolves to {} exact mapped source bindings",
                    source.id,
                    candidates.len()
                ));
            };
            let expected_ancestry = mapped_owner_ancestry(execution, source.owner)?;
            let mut semantic_resource_ancestry = expected_ancestry.clone();
            semantic_resource_ancestry.reverse();
            if source.owner_ancestry != semantic_resource_ancestry
                || binding.owner != source.owner
                || binding.owner_ancestry != expected_ancestry
            {
                return Err(format!(
                    "semantic source {} has inconsistent structural owner ancestry",
                    source.id
                ));
            }
            Ok(MappedSemanticSource {
                source: runtime,
                owner: source.owner,
                owner_ancestry: expected_ancestry,
                executable,
                binding: binding.id,
            })
        })
        .collect()
}

fn mapped_binding(
    bindings: &[MappedSemanticBinding],
    id: MappedReactiveBindingId,
) -> Result<&MappedSemanticBinding, String> {
    bindings
        .get(id.0)
        .filter(|binding| binding.id == id)
        .ok_or_else(|| format!("missing mapped semantic binding {id}"))
}

fn map_reactive_read(
    execution: &SemanticExecutionGraphV1,
    resources: &SemanticResourceGraphV1,
    ids: &SemanticToExecutableMap,
    reactive_ids: &SemanticReactiveToMappedMap,
    read: &boon_semantic::SemanticReadBindingV1,
) -> Result<MappedSemanticRead, String> {
    let expression = semantic_execution_expression(execution, read.expression)?;
    if expression.value_id != read.value {
        return Err(format!(
            "semantic read {} value {} differs from expression {} value {}",
            read.id, read.value, read.expression, expression.value_id
        ));
    }
    let executable_expression = ids.expression(read.expression)?;
    let executable_value = ids.value(read.value)?;
    if executable_expression != executable_value {
        return Err(format!(
            "semantic read {} expression and value map to different executable identities",
            read.id
        ));
    }
    let target = match &read.target {
        SemanticReadTargetV1::Binding {
            binding,
            projection,
        } => MappedSemanticReadTarget::Binding {
            binding: reactive_ids.binding(*binding)?,
            projection: projection.clone(),
        },
        SemanticReadTargetV1::SourcePayload {
            binding,
            source,
            payload_projection,
            projection,
        } => {
            let resource = semantic_source_resource(resources, *source)?;
            if let Some(field) = payload_projection.first()
                && !resource
                    .payload_fields
                    .iter()
                    .any(|candidate| candidate.name == *field)
            {
                return Err(format!(
                    "semantic read {} payload projection `{field}` is absent from source {}",
                    read.id, source
                ));
            }
            MappedSemanticReadTarget::SourcePayload {
                binding: reactive_ids.binding(*binding)?,
                source: ids.runtime_source(*source)?,
                payload_projection: payload_projection.clone(),
                projection: projection.clone(),
            }
        }
        SemanticReadTargetV1::StateProjection {
            binding,
            state,
            projection,
        } => {
            semantic_state_resource(resources, *state)?;
            MappedSemanticReadTarget::StateProjection {
                binding: reactive_ids.binding(*binding)?,
                state: ids.runtime_state(*state)?,
                projection: projection.clone(),
            }
        }
        SemanticReadTargetV1::Local {
            binding,
            declaration,
            producer,
            producer_value,
            projection,
        } => {
            let mapped_producer = ids.expression(*producer)?;
            if mapped_producer != ids.value(*producer_value)? {
                return Err(format!(
                    "semantic read {} local producer and value map differently",
                    read.id
                ));
            }
            MappedSemanticReadTarget::Local {
                binding: ids.local(*binding)?,
                declaration: *declaration,
                producer: mapped_producer,
                projection: projection.clone(),
            }
        }
        SemanticReadTargetV1::External {
            canonical_path,
            external_identity,
        } => {
            if let Some(identity) = external_identity
                && identity.kind != CheckedExternalDeclarationKind::Value
            {
                return Err(format!(
                    "semantic read {} carries non-value external identity",
                    read.id
                ));
            }
            MappedSemanticReadTarget::External {
                canonical_path: canonical_path.clone(),
                external_identity: *external_identity,
            }
        }
        SemanticReadTargetV1::ElementState {
            context,
            projection,
        } => MappedSemanticReadTarget::ElementState {
            context: ids.call_context(*context)?,
            projection: projection.clone(),
        },
        SemanticReadTargetV1::MaterializationLocal {
            owner,
            local,
            projection,
        } => MappedSemanticReadTarget::MaterializationLocal {
            owner: *owner,
            local: ids.materialization_local(*owner, *local)?,
            projection: projection.clone(),
        },
        SemanticReadTargetV1::FunctionParameter {
            parameter,
            projection,
        } => MappedSemanticReadTarget::FunctionParameter {
            parameter: ids.parameter(*parameter)?,
            projection: projection.clone(),
        },
    };
    Ok(MappedSemanticRead {
        id: reactive_ids.read(read.id)?,
        expression: executable_expression,
        value: executable_value,
        target,
    })
}

fn map_semantic_event_cause(
    cause: SemanticEventCauseV1,
    ids: &SemanticToExecutableMap,
) -> Result<EventCause, String> {
    match cause {
        SemanticEventCauseV1::Source(source) => Ok(EventCause::Source(ids.runtime_source(source)?)),
        SemanticEventCauseV1::State(state) => Ok(EventCause::State(ids.runtime_state(state)?)),
    }
}

fn semantic_event_cause_path(
    cause: SemanticEventCauseV1,
    ids: &SemanticToExecutableMap,
    resources: &MappedSemanticResources,
) -> Result<String, String> {
    match map_semantic_event_cause(cause, ids)? {
        EventCause::Source(source) => resources
            .sources
            .get(source.as_usize())
            .filter(|candidate| candidate.id == source)
            .map(|source| source.path.clone())
            .ok_or_else(|| format!("semantic cause maps to missing source {source}")),
        EventCause::State(state) => resources
            .state_cells
            .get(state.as_usize())
            .filter(|candidate| candidate.id == state)
            .map(|state| state.path.clone())
            .ok_or_else(|| format!("semantic cause maps to missing state {state}")),
    }
}

fn map_reactive_trigger(
    execution: &SemanticExecutionGraphV1,
    ids: &SemanticToExecutableMap,
    reactive_ids: &SemanticReactiveToMappedMap,
    trigger: &boon_semantic::SemanticTriggerOwnedArmV1,
) -> Result<MappedSemanticTriggerArm, String> {
    let gate = semantic_execution_expression(execution, trigger.gate_expression)?;
    let output = semantic_execution_expression(execution, trigger.output_expression)?;
    if gate.checked_expr_id != trigger.gate_checked_expression
        || gate.value_id != trigger.gate_value
        || gate.owner != trigger.owner
        || output.value_id != trigger.output_value
    {
        return Err(format!(
            "semantic trigger {} has stale gate/output/value/owner provenance",
            trigger.id
        ));
    }
    if ids.expression(trigger.gate_expression)? != ids.value(trigger.gate_value)?
        || ids.expression(trigger.output_expression)? != ids.value(trigger.output_value)?
    {
        return Err(format!(
            "semantic trigger {} value identities do not map to their producing expressions",
            trigger.id
        ));
    }
    let route_scope = ids.lexical_scope(trigger.route_scope)?;
    let row_scope = trigger
        .row_scope
        .map(|scope| ids.row_scope(scope))
        .transpose()?;
    mapped_owner_ancestry(execution, trigger.owner)?;
    Ok(MappedSemanticTriggerArm {
        id: reactive_ids.trigger(trigger.id)?,
        cause: map_semantic_event_cause(trigger.cause, ids)?,
        gate_checked_expression: trigger.gate_checked_expression,
        gate_expression: ids.expression(trigger.gate_expression)?,
        owner: trigger.owner,
        route_scope,
        row_scope,
        output_expression: ids.expression(trigger.output_expression)?,
    })
}

fn exact_mutation_trigger<'a>(
    graph: &'a SemanticReactiveGraphV1,
    mutation: &boon_semantic::SemanticListMutationV1,
) -> Result<&'a boon_semantic::SemanticTriggerOwnedArmV1, String> {
    let (gate, gate_value, output, output_value) = match mutation.kind {
        SemanticListMutationKindV1::Append {
            gate,
            gate_value,
            item,
            item_value,
        } => (gate, gate_value, item, item_value),
        SemanticListMutationKindV1::Remove {
            gate,
            gate_value,
            predicate,
            predicate_value,
            ..
        } => (gate, gate_value, predicate, predicate_value),
    };
    let candidates = graph
        .trigger_arms
        .iter()
        .filter(|trigger| {
            trigger.cause == mutation.cause
                && trigger.gate_expression == gate
                && trigger.gate_value == gate_value
                && trigger.owner == mutation.owner
                && trigger.route_scope == mutation.route_scope
                && trigger.row_scope == mutation.row_scope
                && trigger.output_expression == output
                && trigger.output_value == output_value
        })
        .collect::<Vec<_>>();
    let [trigger] = candidates.as_slice() else {
        return Err(format!(
            "semantic list mutation {} resolves to {} exact trigger arms",
            mutation.id,
            candidates.len()
        ));
    };
    Ok(*trigger)
}

fn validate_value_producer(
    ids: &SemanticToExecutableMap,
    expression: SemanticExprId,
    value: SemanticValueId,
    context: &str,
) -> Result<ExecutableExprId, String> {
    let expression = ids.expression(expression)?;
    let mapped_value = ids.value(value)?;
    if expression != mapped_value {
        return Err(format!(
            "{context} expression and value map to different executable identities"
        ));
    }
    Ok(expression)
}

fn map_reactive_list_mutation(
    execution: &SemanticExecutionGraphV1,
    graph: &SemanticReactiveGraphV1,
    ids: &SemanticToExecutableMap,
    reactive_ids: &SemanticReactiveToMappedMap,
    mutation: &boon_semantic::SemanticListMutationV1,
    referenced_trigger_ids: &mut BTreeSet<MappedReactiveTriggerId>,
) -> Result<MappedSemanticListMutation, String> {
    let site = semantic_execution_expression(execution, mutation.site)?;
    if site.value_id != mutation.site_value {
        return Err(format!(
            "semantic list mutation {} has stale site value provenance",
            mutation.id
        ));
    }
    let site = validate_value_producer(
        ids,
        mutation.site,
        mutation.site_value,
        "semantic list mutation site",
    )?;
    let route_scope = ids.lexical_scope(mutation.route_scope)?;
    let row_scope = mutation
        .row_scope
        .map(|scope| ids.row_scope(scope))
        .transpose()?;
    mapped_owner_ancestry(execution, mutation.owner)?;
    let trigger = exact_mutation_trigger(graph, mutation)?;
    let trigger_index = reactive_ids.trigger(trigger.id)?;
    if !referenced_trigger_ids.insert(trigger_index)
        && graph
            .trigger_arms
            .get(trigger_index.0)
            .is_none_or(|candidate| candidate.id != trigger.id)
    {
        return Err(format!(
            "semantic list mutation {} references an invalid trigger {}",
            mutation.id, trigger.id
        ));
    }
    let kind = match mutation.kind {
        SemanticListMutationKindV1::Append {
            gate,
            gate_value,
            item,
            item_value,
        } => ListMutationKind::Append {
            gate: validate_value_producer(ids, gate, gate_value, "semantic list append gate")?,
            item: validate_value_producer(ids, item, item_value, "semantic list append item")?,
        },
        SemanticListMutationKindV1::Remove {
            materialization,
            gate,
            gate_value,
            owner,
            row_local,
            predicate,
            predicate_value,
            remove_when,
        } => {
            ids.materialization(materialization)?;
            ListMutationKind::Remove {
                gate: validate_value_producer(ids, gate, gate_value, "semantic list remove gate")?,
                owner,
                row_local: ids.materialization_local(owner, row_local)?,
                predicate: validate_value_producer(
                    ids,
                    predicate,
                    predicate_value,
                    "semantic list remove predicate",
                )?,
                remove_when,
            }
        }
    };
    let id = reactive_ids.list_mutation(mutation.id)?;
    Ok(MappedSemanticListMutation {
        id,
        list_id: ids.list(mutation.list)?,
        site,
        cause: map_semantic_event_cause(mutation.cause, ids)?,
        owner: mutation.owner,
        route_scope,
        row_scope,
        trigger: trigger_index,
        kind,
    })
}

struct ReactiveDerivedMappingContext<'a> {
    execution: &'a SemanticExecutionGraphV1,
    resource_graph: &'a SemanticResourceGraphV1,
    graph: &'a SemanticReactiveGraphV1,
    ids: &'a SemanticToExecutableMap,
    reactive_ids: &'a SemanticReactiveToMappedMap,
    resources: &'a MappedSemanticResources,
    fields: &'a [MappedSemanticField],
    bindings: &'a [MappedSemanticBinding],
    trigger_arms: &'a [MappedSemanticTriggerArm],
}

fn map_reactive_derived_value(
    context: &ReactiveDerivedMappingContext<'_>,
    derived: &boon_semantic::SemanticDerivedValueV1,
    referenced_trigger_ids: &mut BTreeSet<MappedReactiveTriggerId>,
) -> Result<MappedSemanticDerivedValue, String> {
    let field = mapped_field(context.fields, context.reactive_ids.field(derived.field)?)?;
    let binding = mapped_binding(
        context.bindings,
        context.reactive_ids.binding(derived.binding)?,
    )?;
    let statement = context.ids.statement(derived.statement)?;
    let producer = validate_value_producer(
        context.ids,
        derived.producer,
        derived.value,
        "semantic derived value",
    )?;
    if field.statement != statement
        || field.producer != producer
        || binding.statement != statement
        || binding.producer != producer
        || binding.value != producer
        || !matches!(
            binding.target,
            MappedSemanticBindingTarget::Field {
                field: candidate
            }
                | MappedSemanticBindingTarget::List {
                    field: candidate, ..
                } if candidate == field.id
        )
    {
        return Err(format!(
            "semantic derived value {} has inconsistent field/binding/statement/producer identity",
            derived.id
        ));
    }
    let expression = semantic_execution_expression(context.execution, derived.producer)?;
    if expression.value_id != derived.value {
        return Err(format!(
            "semantic derived value {} has stale semantic value provenance",
            derived.id
        ));
    }
    let materialized_list_id = derived
        .materialized_list
        .map(|list| context.ids.list(list))
        .transpose()?;
    let materialized_row_scope_id = derived
        .materialized_row_scope
        .map(|scope| context.ids.row_scope(scope))
        .transpose()?;
    if materialized_list_id.is_some() != materialized_row_scope_id.is_some() {
        return Err(format!(
            "semantic derived value {} has an unpaired materialized list/row scope",
            derived.id
        ));
    }
    if let (Some(list), Some(scope)) = (derived.materialized_list, derived.materialized_row_scope) {
        let list = semantic_list_resource(context.resource_graph, list)?;
        if list.row_scope != scope {
            return Err(format!(
                "semantic derived value {} materialized row scope differs from list {}",
                derived.id, list.id
            ));
        }
        if !matches!(
            binding.target,
            MappedSemanticBindingTarget::List {
                list: candidate_list,
                row: ErasedRowBinding {
                    list: row_list,
                    scope: row_scope,
                },
                ..
            } if candidate_list == materialized_list_id.expect("mapped list is present")
                && row_list == candidate_list
                && row_scope == materialized_row_scope_id.expect("mapped row scope is present")
        ) {
            return Err(format!(
                "semantic derived value {} materialized storage differs from its list binding",
                derived.id
            ));
        }
    }
    let causes = derived
        .causes
        .iter()
        .copied()
        .map(|cause| map_semantic_event_cause(cause, context.ids))
        .collect::<Result<Vec<_>, _>>()?;
    let mut source_paths = derived
        .causes
        .iter()
        .copied()
        .map(|cause| semantic_event_cause_path(cause, context.ids, context.resources))
        .collect::<Result<Vec<_>, _>>()?;
    source_paths.sort();
    source_paths.dedup();
    let mapped_triggers = derived
        .trigger_arms
        .iter()
        .map(|trigger| {
            let index = context.reactive_ids.trigger(*trigger)?;
            referenced_trigger_ids.insert(index);
            let semantic = context
                .graph
                .trigger_arms
                .get(index.0)
                .filter(|candidate| candidate.id == *trigger)
                .ok_or_else(|| {
                    format!(
                        "semantic derived value {} references missing trigger {trigger}",
                        derived.id
                    )
                })?;
            let mapped = context.trigger_arms.get(index.0).ok_or_else(|| {
                format!(
                    "semantic derived value {} trigger {trigger} has no mapped arm",
                    derived.id
                )
            })?;
            if map_semantic_event_cause(semantic.cause, context.ids)? != mapped.cause {
                return Err(format!(
                    "semantic derived value {} trigger {trigger} has stale mapped cause",
                    derived.id
                ));
            }
            Ok(mapped.id)
        })
        .collect::<Result<Vec<_>, String>>()?;
    let default_roots = derived
        .default_values
        .iter()
        .copied()
        .map(|value| context.ids.value(value))
        .collect::<Result<Vec<_>, _>>()?;
    let kind = match derived.kind {
        SemanticDerivedValueKindV1::SourceEventTransform => DerivedValueKind::SourceEventTransform,
        SemanticDerivedValueKindV1::ListView => DerivedValueKind::ListView,
        SemanticDerivedValueKindV1::Aggregate => DerivedValueKind::Aggregate,
        SemanticDerivedValueKindV1::Pure => DerivedValueKind::Pure,
    };
    // A materialized list field carries its target row identity so storage can
    // own the list's row fields.  That does not make the list-producing
    // operation row-indexed: the operation computes the whole list and writes
    // it into the keyed materialization.  Only scalar fields owned by an
    // existing row execute once per row.
    let indexed = field.row.is_some() && materialized_list_id.is_none();
    let scope_id = indexed.then(|| field.row.expect("indexed field has a row").scope);
    let derived_index = context
        .reactive_ids
        .derived_values
        .get(derived.id.as_usize())
        .copied()
        .ok_or_else(|| {
            format!(
                "semantic derived value {} has no mapped identity",
                derived.id
            )
        })?;
    if derived_index != derived.id.as_usize() {
        return Err(format!(
            "semantic derived value {} maps to noncanonical index {derived_index}",
            derived.id
        ));
    }
    Ok(MappedSemanticDerivedValue {
        field: field.id,
        executable_statement_id: statement,
        path: field.path.clone(),
        kind,
        materialized_list_id,
        materialized_row_scope_id,
        causes,
        trigger_arms: mapped_triggers,
        default_roots,
        sources: source_paths,
        indexed,
        scope_id,
        startup_recompute: derived.startup_recompute,
    })
}

fn map_dependency_timing(
    timing: &SemanticDependencyTimingV1,
    ids: &SemanticToExecutableMap,
) -> Result<ErasedDependencyTiming, String> {
    Ok(match timing {
        SemanticDependencyTimingV1::Immediate => ErasedDependencyTiming::Immediate,
        SemanticDependencyTimingV1::After { boundaries } => ErasedDependencyTiming::After {
            boundaries: boundaries
                .iter()
                .copied()
                .map(|boundary| match map_semantic_event_cause(boundary, ids)? {
                    EventCause::Source(source) => Ok(ErasedTemporalBoundary::Source(source)),
                    EventCause::State(state) => Ok(ErasedTemporalBoundary::State(state)),
                })
                .collect::<Result<Vec<_>, String>>()?,
        },
    })
}

fn map_reactive_dependency_use(
    execution: &SemanticExecutionGraphV1,
    graph: &SemanticReactiveGraphV1,
    ids: &SemanticToExecutableMap,
    reactive_ids: &SemanticReactiveToMappedMap,
    dependency: &boon_semantic::SemanticDependencyUseV1,
) -> Result<MappedSemanticDependencyUse, String> {
    let dependency_index = reactive_ids
        .dependency_uses
        .get(dependency.id.as_usize())
        .copied()
        .ok_or_else(|| {
            format!(
                "semantic dependency use {} has no mapped identity",
                dependency.id
            )
        })?;
    if dependency_index != dependency.id.as_usize() {
        return Err(format!(
            "semantic dependency use {} maps to noncanonical index {dependency_index}",
            dependency.id
        ));
    }
    let expression = ids.expression(dependency.expression)?;
    let target = match dependency.target {
        SemanticDependencyTargetV1::ExternalRead { read } => {
            let read_definition = graph
                .reads
                .get(read.as_usize())
                .filter(|candidate| candidate.id == read)
                .ok_or_else(|| {
                    format!(
                        "semantic dependency use {} references missing external read {read}",
                        dependency.id
                    )
                })?;
            if read_definition.expression != dependency.expression
                || !matches!(
                    read_definition.target,
                    SemanticReadTargetV1::External { .. }
                )
            {
                return Err(format!(
                    "semantic dependency use {} external-read target {read} has stale expression/kind provenance",
                    dependency.id
                ));
            }
            MappedSemanticDependencyTarget::ExternalRead {
                read: reactive_ids.read(read)?,
            }
        }
        SemanticDependencyTargetV1::ExternalCall {
            call,
            expression: call_expression,
        } => {
            if call_expression != dependency.expression {
                return Err(format!(
                    "semantic dependency use {} call target expression {} differs from dependency expression {}",
                    dependency.id, call_expression, dependency.expression
                ));
            }
            let executable_call = ids.call_expression(call, call_expression)?;
            if executable_call != expression {
                return Err(format!(
                    "semantic dependency use {} call occurrence maps to a different executable expression",
                    dependency.id
                ));
            }
            let call_definition = semantic_call(execution, call)?;
            let external_identity = call_definition.external_identity.ok_or_else(|| {
                format!(
                    "semantic dependency use {} call {call} has no frozen external identity",
                    dependency.id
                )
            })?;
            if external_identity.kind != CheckedExternalDeclarationKind::Callable {
                return Err(format!(
                    "semantic dependency use {} call {call} carries non-callable external identity",
                    dependency.id
                ));
            }
            MappedSemanticDependencyTarget::ExternalCall {
                expression,
                external_identity,
            }
        }
    };
    Ok(MappedSemanticDependencyUse {
        dependent: reactive_ids.binding(dependency.dependent)?,
        expression,
        target,
        timing: map_dependency_timing(&dependency.timing, ids)?,
    })
}

fn validate_reactive_call_and_host_schedules(
    execution: &SemanticExecutionGraphV1,
    graph: &SemanticReactiveGraphV1,
    ids: &SemanticToExecutableMap,
    reactive_ids: &SemanticReactiveToMappedMap,
    referenced_trigger_ids: &mut BTreeSet<MappedReactiveTriggerId>,
) -> Result<(), String> {
    let mut previous_expression = None;
    for schedule in &graph.call_invocations {
        if previous_expression.is_some_and(|previous| previous >= schedule.expression) {
            return Err(
                "semantic call invocation schedules are not strictly expression-ordered".to_owned(),
            );
        }
        previous_expression = Some(schedule.expression);
        let expression = semantic_execution_expression(execution, schedule.expression)?;
        if expression.value_id != schedule.value
            || !matches!(
                expression.kind,
                SemanticExpressionKind::Call { call, .. } if call == schedule.call
            )
            || ids.call_expression(schedule.call, schedule.expression)?
                != validate_value_producer(
                    ids,
                    schedule.expression,
                    schedule.value,
                    "semantic call invocation schedule",
                )?
        {
            return Err(format!(
                "semantic call invocation schedule for expression {} has stale call/value provenance",
                schedule.expression
            ));
        }
        for binding in &schedule.dependent_bindings {
            reactive_ids.binding(*binding)?;
        }
        for trigger in &schedule.invocation_arms {
            referenced_trigger_ids.insert(reactive_ids.trigger(*trigger)?);
        }
    }

    for schedule in &graph.host_effect_schedules {
        let index = reactive_ids
            .host_effect_schedules
            .get(schedule.id.as_usize())
            .copied()
            .ok_or_else(|| {
                format!(
                    "semantic host-effect schedule {} has no mapped identity",
                    schedule.id
                )
            })?;
        if index != schedule.id.as_usize() {
            return Err(format!(
                "semantic host-effect schedule {} maps to noncanonical index {index}",
                schedule.id
            ));
        }
        let expression = semantic_execution_expression(execution, schedule.expression)?;
        let mapped = validate_value_producer(
            ids,
            schedule.expression,
            schedule.value,
            "semantic host-effect schedule",
        )?;
        if expression.checked_expr_id != schedule.checked_expression
            || expression.owner != schedule.owner
            || !matches!(
                &expression.kind,
                SemanticExpressionKind::Call {
                    call,
                    function,
                    ..
                } if *call == schedule.call && function == &schedule.operation
            )
            || ids.call_expression(schedule.call, schedule.expression)? != mapped
        {
            return Err(format!(
                "semantic host-effect schedule {} has stale call/operation/owner provenance",
                schedule.id
            ));
        }
        for arm in &schedule.state_update_arms {
            reactive_ids.state_update_arm(*arm)?;
        }
    }
    Ok(())
}

fn map_reactive_producer_instance(
    execution: &SemanticExecutionGraphV1,
    resources: &SemanticResourceGraphV1,
    ids: &SemanticToExecutableMap,
    fields: &[MappedSemanticField],
    instance: &boon_semantic::SemanticProducerInstanceV1,
) -> Result<MappedSemanticProducerInstance, String> {
    let function = execution
        .functions
        .iter()
        .find(|function| function.producer == instance.function)
        .ok_or_else(|| {
            format!(
                "semantic producer instance {} has no function {}",
                producer_identity_text(instance.identity),
                instance.function
            )
        })?;
    let callable = semantic_callable(execution, instance.callable)?;
    if function.callable != instance.callable
        || function.identity != instance.identity
        || function.root != instance.root_expression
        || function.name != callable.name
    {
        return Err(format!(
            "semantic producer instance {} has stale function/callable/root provenance",
            producer_identity_text(instance.identity)
        ));
    }
    let resource_candidates = resources
        .producer_resources
        .iter()
        .filter(|resource| resource.identity == instance.identity)
        .collect::<Vec<_>>();
    let [resource] = resource_candidates.as_slice() else {
        return Err(format!(
            "semantic producer instance {} resolves to {} producer resources",
            producer_identity_text(instance.identity),
            resource_candidates.len()
        ));
    };
    if resource.function != instance.function
        || resource.callable != instance.callable
        || resource.root_call != instance.root_call
        || resource.result_statement != instance.result_statement
        || resource.result_declaration != instance.result_declaration
        || resource.result_path != instance.result_path
        || resource.owner != instance.owner
        || resource.mode != instance.mode
        || resource.invocation_source != instance.invocation_source
    {
        return Err(format!(
            "semantic producer instance {} differs from its producer resource",
            producer_identity_text(instance.identity)
        ));
    }
    let result_statement = ids.statement(instance.result_statement)?;
    let result_field = unique_mapped_field_for_statement(
        fields,
        result_statement,
        instance.result_declaration,
        "semantic producer result",
    )?;
    if result_field.path != instance.result_path
        || result_field.owner != Some(instance.owner)
        || result_field.producer != ids.expression(instance.root_expression)?
        || result_field.value != ids.value(instance.root_value)?
    {
        return Err(format!(
            "semantic producer instance {} result field has stale path/owner/root provenance",
            producer_identity_text(instance.identity)
        ));
    }
    if semantic_execution_expression(execution, instance.root_expression)?.value_id
        != instance.root_value
    {
        return Err(format!(
            "semantic producer instance {} has stale root value provenance",
            producer_identity_text(instance.identity)
        ));
    }
    let function_id = ids.producer_function(instance.function)?;
    if function_id != ids.callable(instance.callable)? {
        return Err(format!(
            "semantic producer instance {} function and callable map differently",
            producer_identity_text(instance.identity)
        ));
    }
    let arguments = instance
        .parameters
        .iter()
        .map(|parameter| {
            let definition = function
                .parameters
                .iter()
                .find(|candidate| candidate.id == parameter.parameter)
                .ok_or_else(|| {
                    format!(
                        "semantic producer instance {} references missing parameter {:?}",
                        producer_identity_text(instance.identity),
                        parameter.parameter
                    )
                })?;
            if definition.formal != parameter.formal
                || definition.name != parameter.name
                || definition.flow_type != parameter.flow_type
                || definition.input_expressions != parameter.input_expressions
                || parameter.input_expressions.len() != parameter.input_values.len()
            {
                return Err(format!(
                    "semantic producer instance {} parameter {:?} has stale formal/name/type/input provenance",
                    producer_identity_text(instance.identity),
                    parameter.parameter
                ));
            }
            let input_expressions = parameter
                .input_expressions
                .iter()
                .copied()
                .zip(parameter.input_values.iter().copied())
                .map(|(expression, value)| {
                    validate_value_producer(
                        ids,
                        expression,
                        value,
                        "semantic producer parameter input",
                    )
                })
                .collect::<Result<Vec<_>, _>>()?;
            Ok(ProducerFunctionArgument {
                name: parameter.name.clone(),
                parameter: ids.parameter(parameter.parameter)?,
                flow_type: parameter.flow_type.clone(),
                input_expressions,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(MappedSemanticProducerInstance {
        identity: instance.identity,
        owner: instance.owner,
        function: function_id,
        function_name: function.name.clone(),
        result_field: result_field.id,
        result_path: instance.result_path.clone(),
        root: ids.expression(instance.root_expression)?,
        mode: instance.mode,
        invocation_source: instance
            .invocation_source
            .map(|source| ids.runtime_source(source))
            .transpose()?,
        arguments,
    })
}

impl MappedSemanticReactive {
    pub(super) fn validate_totality(
        &self,
        graph: &SemanticReactiveGraphV1,
        resources: &SemanticResourceGraphV1,
        ids: &SemanticToExecutableMap,
    ) -> Result<(), String> {
        let exact_lengths = [
            (
                "producer instance",
                graph.producer_instances.len(),
                self.producer_function_instances.len(),
            ),
            ("field", graph.fields.len(), self.fields.len()),
            ("binding", graph.bindings.len(), self.bindings.len()),
            ("source", resources.sources.len(), self.sources.len()),
            ("read", graph.reads.len(), self.reads.len()),
            (
                "dependency use",
                graph.dependency_uses.len(),
                self.dependency_uses.len(),
            ),
            (
                "trigger arm",
                graph.trigger_arms.len(),
                self.trigger_arms.len(),
            ),
            (
                "state update arm",
                graph.state_update_arms.len(),
                self.state_update_arms.len(),
            ),
            (
                "list mutation",
                graph.list_mutations.len(),
                self.list_mutations.len(),
            ),
            (
                "derived value",
                graph.derived_values.len(),
                self.derived_values.len(),
            ),
            (
                "dependency edge",
                graph.dependencies.len(),
                self.dependencies.len(),
            ),
            (
                "possible-causes state",
                resources.states.len(),
                self.possible_causes.len(),
            ),
            (
                "host-effect schedule identity",
                graph.host_effect_schedules.len(),
                self.id_map.host_effect_schedules.len(),
            ),
        ];
        for (label, semantic, mapped) in exact_lengths {
            if semantic != mapped {
                return Err(format!(
                    "semantic reactive {label} graph has {semantic} records but mapping emitted {mapped}"
                ));
            }
        }
        if self.semantic_producer_instance_count != graph.producer_instances.len()
            || graph.producer_instances.len() != resources.producer_resources.len()
        {
            return Err(format!(
                "semantic producer-instance mapping covers {} identities for {} reactive instances and {} producer resources",
                self.semantic_producer_instance_count,
                graph.producer_instances.len(),
                resources.producer_resources.len()
            ));
        }
        let identity_lengths = [
            ("field", self.id_map.fields.len(), self.fields.len()),
            ("binding", self.id_map.bindings.len(), self.bindings.len()),
            ("read", self.id_map.reads.len(), self.reads.len()),
            (
                "trigger arm",
                self.id_map.trigger_arms.len(),
                self.trigger_arms.len(),
            ),
            (
                "state update arm",
                self.id_map.state_update_arms.len(),
                self.state_update_arms.len(),
            ),
            (
                "list mutation",
                self.id_map.list_mutations.len(),
                self.list_mutations.len(),
            ),
            (
                "derived value",
                self.id_map.derived_values.len(),
                self.derived_values.len(),
            ),
            (
                "dependency use",
                self.id_map.dependency_uses.len(),
                self.dependency_uses.len(),
            ),
            (
                "dependency edge",
                self.id_map.dependencies.len(),
                self.dependencies.len(),
            ),
        ];
        for (label, identities, mapped) in identity_lengths {
            if identities != mapped {
                return Err(format!(
                    "semantic reactive {label} identity map covers {identities} IDs but emitted {mapped} records"
                ));
            }
        }
        for (index, field) in self.fields.iter().enumerate() {
            let expected = self.id_map.field(SemanticFieldId(index))?;
            if field.id != expected {
                return Err(format!(
                    "mapped semantic field at index {index} emitted {}, expected {expected}",
                    field.id
                ));
            }
        }
        for (index, binding) in self.bindings.iter().enumerate() {
            let expected = self.id_map.binding(SemanticBindingId(index))?;
            if binding.id != expected {
                return Err(format!(
                    "mapped semantic binding at index {index} emitted {}, expected {expected}",
                    binding.id
                ));
            }
        }
        for (index, source) in self.sources.iter().enumerate() {
            let semantic = resources.sources.get(index).ok_or_else(|| {
                format!("mapped source at index {index} has no semantic source resource")
            })?;
            let expected = ids.runtime_source(semantic.id)?;
            if source.source != expected {
                return Err(format!(
                    "mapped semantic source at index {index} emitted {}, expected {expected}",
                    source.source
                ));
            }
        }
        for (index, read) in self.reads.iter().enumerate() {
            let expected = self.id_map.read(SemanticReadId(index))?;
            if read.id != expected {
                return Err(format!(
                    "mapped semantic read at index {index} emitted {}, expected {expected}",
                    read.id
                ));
            }
            match &read.target {
                MappedSemanticReadTarget::Binding { binding, .. } => {
                    mapped_binding(&self.bindings, *binding)?;
                }
                MappedSemanticReadTarget::SourcePayload {
                    binding, source, ..
                } => {
                    let binding = mapped_binding(&self.bindings, *binding)?;
                    if !matches!(
                        binding.target,
                        MappedSemanticBindingTarget::Source { runtime, .. }
                            if runtime == *source
                    ) {
                        return Err(format!(
                            "mapped semantic read {} has mismatched source binding {}",
                            read.id, binding.id
                        ));
                    }
                }
                MappedSemanticReadTarget::StateProjection { binding, state, .. } => {
                    let binding = mapped_binding(&self.bindings, *binding)?;
                    if !matches!(
                        binding.target,
                        MappedSemanticBindingTarget::State { runtime, .. }
                            if runtime == *state
                    ) {
                        return Err(format!(
                            "mapped semantic read {} has mismatched state binding {}",
                            read.id, binding.id
                        ));
                    }
                }
                MappedSemanticReadTarget::Local { .. }
                | MappedSemanticReadTarget::External { .. }
                | MappedSemanticReadTarget::ElementState { .. }
                | MappedSemanticReadTarget::MaterializationLocal { .. }
                | MappedSemanticReadTarget::FunctionParameter { .. } => {}
            }
        }
        for dependency in &self.dependency_uses {
            mapped_binding(&self.bindings, dependency.dependent)?;
            match dependency.target {
                MappedSemanticDependencyTarget::ExternalRead { read } => {
                    let read = self
                        .reads
                        .get(read.0)
                        .filter(|candidate| candidate.id == read)
                        .ok_or_else(|| {
                            format!("mapped dependency use references missing external read {read}")
                        })?;
                    if !matches!(read.target, MappedSemanticReadTarget::External { .. }) {
                        return Err(format!(
                            "mapped dependency use read {} is not external",
                            read.id
                        ));
                    }
                }
                MappedSemanticDependencyTarget::ExternalCall { expression, .. } => {
                    let expression = ids
                        .expressions
                        .get(expression.as_usize())
                        .copied()
                        .ok_or_else(|| {
                            format!(
                                "mapped dependency use references missing call expression {expression}"
                            )
                        })?;
                    if expression != dependency.expression {
                        return Err(
                            "mapped external-call dependency expression identity differs"
                                .to_owned(),
                        );
                    }
                }
            }
        }
        for (index, trigger) in self.trigger_arms.iter().enumerate() {
            let expected = self.id_map.trigger(SemanticTriggerArmId(index))?;
            if trigger.id != expected {
                return Err(format!(
                    "mapped semantic trigger at index {index} emitted {}, expected {expected}",
                    trigger.id
                ));
            }
        }
        for (index, arm) in self.state_update_arms.iter().enumerate() {
            let expected = self
                .id_map
                .state_update_arm(boon_semantic::SemanticStateUpdateArmId(index))?;
            if arm.id != expected {
                return Err(format!(
                    "mapped semantic state update arm at index {index} emitted {}, expected {expected}",
                    arm.id
                ));
            }
            self.trigger_arms
                .get(arm.trigger.0)
                .filter(|candidate| candidate.id == arm.trigger)
                .ok_or_else(|| {
                    format!(
                        "mapped semantic state update arm at index {index} references missing trigger {}",
                        arm.trigger
                    )
                })?;
        }
        for (index, mutation) in self.list_mutations.iter().enumerate() {
            let expected = *self.id_map.list_mutations.get(index).ok_or_else(|| {
                format!("mapped list mutation at index {index} has no staged identity")
            })?;
            if mutation.id != expected {
                return Err(format!(
                    "mapped list mutation at index {index} emitted {}, expected {expected}",
                    mutation.id
                ));
            }
            self.trigger_arms
                .get(mutation.trigger.0)
                .filter(|candidate| candidate.id == mutation.trigger)
                .ok_or_else(|| {
                    format!(
                        "mapped semantic list mutation at index {index} references missing trigger {}",
                        mutation.trigger
                    )
                })?;
        }
        let expected_triggers = (0..graph.trigger_arms.len())
            .map(MappedReactiveTriggerId)
            .collect::<BTreeSet<_>>();
        if self.referenced_trigger_ids != expected_triggers {
            return Err(format!(
                "mapped reactive records reference trigger IDs {:?}, expected exact set {:?}",
                self.referenced_trigger_ids, expected_triggers
            ));
        }
        let producer_identities = self
            .producer_function_instances
            .iter()
            .map(|instance| instance.identity)
            .collect::<BTreeSet<_>>();
        if producer_identities.len() != self.producer_function_instances.len() {
            return Err("mapped producer instances contain duplicate identities".to_owned());
        }
        let derived_fields = self
            .derived_values
            .iter()
            .map(|derived| derived.field)
            .collect::<BTreeSet<_>>();
        if derived_fields.len() != self.derived_values.len() {
            return Err("mapped derived values contain duplicate result fields".to_owned());
        }
        Ok(())
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn map_semantic_storage_join(
    execution: &SemanticExecutionGraphV1,
    resource_graph: &SemanticResourceGraphV1,
    reactive_graph: &SemanticReactiveGraphV1,
    storage_graph: &SemanticScopeStorageGraphV1,
    lowering_contract: &SemanticLoweringContractV1,
    ids: &SemanticToExecutableMap,
    resources: &MappedSemanticResources,
    reactive: &MappedSemanticReactive,
) -> Result<MappedSemanticStorage, String> {
    reactive.validate_totality(reactive_graph, resource_graph, ids)?;
    let storage_ids = SemanticStorageToErasedMap::allocate(storage_graph, reactive_graph)?;
    let external_references = map_storage_external_references(
        execution,
        reactive_graph,
        storage_graph,
        ids,
        reactive,
        &storage_ids,
    )?;
    let owners = map_storage_owners(execution, storage_graph, ids)?;
    let fields = map_storage_fields(execution, storage_graph, ids, reactive, &storage_ids)?;
    let locals = map_storage_locals(execution, storage_graph, ids, &storage_ids, &fields)?;
    let bindings = map_storage_bindings(storage_graph, ids, reactive, &storage_ids, &fields)?;
    let sources = map_storage_sources(resource_graph, storage_graph, ids, reactive, &storage_ids)?;
    let reads = map_storage_reads(reactive_graph, reactive, &storage_ids, &external_references)?;
    let row_values = storage_graph
        .row_values
        .iter()
        .map(|value| {
            Ok(ErasedRowValue {
                expression: ids.expression(value.expression)?,
                projection: value.projection.clone(),
                row: map_row_binding(ids, value.row)?,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let row_source_projections = storage_graph
        .row_source_projections
        .iter()
        .map(|projection| {
            Ok(ErasedRowSourceProjection {
                row: map_row_binding(ids, projection.row)?,
                path: projection.path.clone(),
                source: ids.runtime_source(projection.source)?,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let dependency_uses =
        map_storage_dependency_uses(reactive_graph, reactive, &storage_ids, &external_references)?;
    let named_values = map_storage_named_values(
        execution,
        resource_graph,
        reactive_graph,
        storage_graph,
        lowering_contract,
        ids,
        &storage_ids,
    )?;
    let named_value_checked_statements = lowering_contract
        .metadata
        .named_value_types
        .iter()
        .map(|value| value.checked_statement)
        .collect::<Vec<_>>();
    if named_value_checked_statements
        .iter()
        .copied()
        .collect::<BTreeSet<_>>()
        .len()
        != named_value_checked_statements.len()
    {
        return Err(
            "semantic named-value table repeats an exact checked statement site".to_owned(),
        );
    }
    let trigger_arms = reactive
        .trigger_arms
        .iter()
        .map(finalize_trigger_arm)
        .collect::<Vec<_>>();
    let finalized_state_transitions = finalize_state_update_arms(reactive, &trigger_arms)?;
    let list_mutations = finalize_list_mutations(reactive)?;
    let call_invocations = finalize_call_invocation_schedules(
        reactive_graph,
        ids,
        reactive,
        &storage_ids,
        &trigger_arms,
    )?;
    let host_effect_schedules = finalize_host_effect_schedules(
        reactive_graph,
        ids,
        reactive,
        &finalized_state_transitions,
    )?;
    let producer_function_instances =
        finalize_producer_instances(reactive_graph, storage_graph, reactive, &storage_ids)?;
    let derived_values =
        finalize_derived_values(reactive_graph, reactive, &storage_ids, &trigger_arms)?;

    let mapped = MappedSemanticStorage {
        owners,
        locals,
        fields,
        bindings,
        sources,
        reads,
        row_values,
        row_source_projections,
        dependency_uses,
        call_invocations,
        host_effect_schedules,
        external_references,
        named_values,
        producer_function_instances,
        derived_values,
        trigger_arms,
        state_update_arms: finalized_state_transitions,
        list_mutations,
        dependencies: reactive.dependencies.clone(),
        possible_causes: reactive.possible_causes.clone(),
        named_value_checked_statements,
        id_map: storage_ids,
    };
    mapped.validate_totality(storage_graph, reactive_graph, resources, ids)?;
    Ok(mapped)
}

impl SemanticStorageToErasedMap {
    fn allocate(
        storage: &SemanticScopeStorageGraphV1,
        reactive: &SemanticReactiveGraphV1,
    ) -> Result<Self, String> {
        require_dense(
            storage.fields.iter().map(|field| field.id.as_usize()),
            "semantic storage field",
        )?;
        require_dense(
            storage
                .external_references
                .iter()
                .map(|reference| reference.id.as_usize()),
            "semantic storage external reference",
        )?;

        let storage_fields = (0..storage.fields.len()).map(FieldId).collect::<Vec<_>>();
        let mut reactive_fields = vec![None; reactive.fields.len()];
        for field in &storage.fields {
            match (&field.origin, field.reactive_field) {
                (
                    SemanticStorageFieldOriginV1::Reactive { field: origin },
                    Some(reactive_field),
                ) if *origin == reactive_field => {
                    let slot = reactive_fields
                        .get_mut(reactive_field.as_usize())
                        .ok_or_else(|| {
                            format!(
                                "semantic storage field {} references reactive field {} outside the staged domain",
                                field.id, reactive_field
                            )
                        })?;
                    if slot.replace(storage_fields[field.id.as_usize()]).is_some() {
                        return Err(format!(
                            "reactive field {reactive_field} maps to multiple semantic storage fields"
                        ));
                    }
                }
                (SemanticStorageFieldOriginV1::Reactive { field: origin }, actual) => {
                    return Err(format!(
                        "semantic storage field {} reactive origin {} differs from join {:?}",
                        field.id, origin, actual
                    ));
                }
                (_, Some(reactive_field)) => {
                    return Err(format!(
                        "non-reactive semantic storage field {} claims reactive field {reactive_field}",
                        field.id
                    ));
                }
                (_, None) => {}
            }
        }
        let reactive_fields = reactive_fields
            .into_iter()
            .enumerate()
            .map(|(index, field)| {
                field.ok_or_else(|| {
                    format!(
                        "reactive field {} has no exact semantic storage field",
                        SemanticFieldId(index)
                    )
                })
            })
            .collect::<Result<Vec<_>, _>>()?;

        let mut bindings = vec![None; reactive.bindings.len()];
        for storage_binding in &storage.bindings {
            let slot = bindings
                .get_mut(storage_binding.binding.as_usize())
                .ok_or_else(|| {
                    format!(
                        "semantic storage binding {} is outside the staged binding domain",
                        storage_binding.binding
                    )
                })?;
            let erased = ErasedBindingId(storage_binding.binding.as_usize());
            if slot.replace(erased).is_some() {
                return Err(format!(
                    "reactive binding {} maps to multiple semantic storage bindings",
                    storage_binding.binding
                ));
            }
        }
        let bindings = bindings
            .into_iter()
            .enumerate()
            .map(|(index, binding)| {
                binding.ok_or_else(|| {
                    format!(
                        "reactive binding {} has no exact semantic storage binding",
                        SemanticBindingId(index)
                    )
                })
            })
            .collect::<Result<Vec<_>, _>>()?;

        Ok(Self {
            storage_fields,
            reactive_fields,
            bindings,
            reads: (0..reactive.reads.len()).map(ErasedReadId).collect(),
            external_references: storage
                .external_references
                .iter()
                .map(|reference| reference.id)
                .collect(),
        })
    }

    fn storage_field(&self, id: SemanticStorageFieldId) -> Result<FieldId, String> {
        exact_map(
            &self.storage_fields,
            id.as_usize(),
            "semantic storage field",
            id,
        )
    }

    fn reactive_field(&self, id: MappedReactiveFieldId) -> Result<FieldId, String> {
        exact_map(&self.reactive_fields, id.0, "mapped reactive field", id)
    }

    fn binding(&self, id: MappedReactiveBindingId) -> Result<ErasedBindingId, String> {
        exact_map(&self.bindings, id.0, "mapped reactive binding", id)
    }

    fn read(&self, id: MappedReactiveReadId) -> Result<ErasedReadId, String> {
        exact_map(&self.reads, id.0, "mapped reactive read", id)
    }

    fn external_reference(
        &self,
        id: SemanticStorageExternalReferenceId,
    ) -> Result<SemanticStorageExternalReferenceId, String> {
        exact_map(
            &self.external_references,
            id.as_usize(),
            "semantic storage external reference",
            id,
        )
    }
}

fn map_storage_owners(
    execution: &SemanticExecutionGraphV1,
    storage: &SemanticScopeStorageGraphV1,
    ids: &SemanticToExecutableMap,
) -> Result<Vec<ErasedOwnerDef>, String> {
    if storage.owners.len() != execution.static_owners.len() {
        return Err(format!(
            "semantic storage has {} owners for {} execution owners",
            storage.owners.len(),
            execution.static_owners.len()
        ));
    }
    storage
        .owners
        .iter()
        .enumerate()
        .map(|(index, owner)| {
            let semantic = execution
                .static_owners
                .get(index)
                .filter(|candidate| candidate.id == owner.id)
                .ok_or_else(|| {
                    format!(
                        "semantic storage owner {} is not the execution owner at index {index}",
                        owner.id
                    )
                })?;
            if semantic.parent != owner.parent
                || semantic.child_ordinal != owner.child_ordinal
                || owner.authority_row != owner.target_row.or(owner.source_row)
            {
                return Err(format!(
                    "semantic storage owner {} differs from execution ownership",
                    owner.id
                ));
            }
            Ok(ErasedOwnerDef {
                id: owner.id,
                parent: owner.parent,
                child_ordinal: owner.child_ordinal,
                source_row: owner
                    .source_row
                    .map(|row| map_row_binding(ids, row))
                    .transpose()?,
                target_row: owner
                    .target_row
                    .map(|row| map_row_binding(ids, row))
                    .transpose()?,
                authority_row: owner
                    .authority_row
                    .map(|row| map_row_binding(ids, row))
                    .transpose()?,
            })
        })
        .collect()
}

const fn map_storage_field_role(role: SemanticStorageFieldRoleV1) -> ErasedFieldRole {
    match role {
        SemanticStorageFieldRoleV1::Value => ErasedFieldRole::Value,
        SemanticStorageFieldRoleV1::ListAuthority => ErasedFieldRole::ListAuthority,
        SemanticStorageFieldRoleV1::ValueAuthority => ErasedFieldRole::ValueAuthority,
        SemanticStorageFieldRoleV1::Capture => ErasedFieldRole::Capture,
    }
}

fn map_storage_fields(
    execution: &SemanticExecutionGraphV1,
    storage: &SemanticScopeStorageGraphV1,
    ids: &SemanticToExecutableMap,
    reactive: &MappedSemanticReactive,
    storage_ids: &SemanticStorageToErasedMap,
) -> Result<Vec<ErasedFieldDef>, String> {
    storage
        .fields
        .iter()
        .map(|field| {
            let id = storage_ids.storage_field(field.id)?;
            if let Some(owner) = field.owner {
                storage
                    .owners
                    .get(owner.as_usize())
                    .filter(|candidate| candidate.id == owner)
                    .ok_or_else(|| {
                        format!("semantic storage field {} references missing owner {owner}", field.id)
                    })?;
            }
            let parent = field
                .parent
                .map(|parent| storage_ids.storage_field(parent))
                .transpose()?;
            let row = field.row.map(|row| map_row_binding(ids, row)).transpose()?;
            let statement = field
                .statement
                .map(|statement| ids.statement(statement))
                .transpose()?;
            let producer = field
                .producer
                .map(|producer| ids.expression(producer))
                .transpose()?;

            match &field.origin {
                SemanticStorageFieldOriginV1::Reactive {
                    field: reactive_field,
                } => {
                    let mapped_id = MappedReactiveFieldId(reactive_field.as_usize());
                    let mapped = mapped_field(&reactive.fields, mapped_id)?;
                    if storage_ids.reactive_field(mapped.id)? != id
                        || field.reactive_field != Some(*reactive_field)
                        || field.declaration != Some(mapped.declaration)
                        || field.owner != mapped.owner
                        || row != mapped.row
                        || field.name != mapped.name
                        || field.diagnostic_path != mapped.path
                        || statement != Some(mapped.statement)
                        || producer != Some(mapped.producer)
                        || field.flow_type != mapped.flow_type
                    {
                        return Err(format!(
                            "semantic storage field {} differs from exact mapped reactive field {}",
                            field.id, reactive_field
                        ));
                    }
                }
                SemanticStorageFieldOriginV1::ListAuthority { list, .. } => {
                    ids.list(*list)?;
                    if field.reactive_field.is_some() {
                        return Err(format!(
                            "list-authority storage field {} claims reactive identity",
                            field.id
                        ));
                    }
                }
                SemanticStorageFieldOriginV1::ValueListAuthority { authority, .. } => {
                    ids.value_list_authority(*authority)?;
                    if field.reactive_field.is_some() {
                        return Err(format!(
                            "value-list-authority storage field {} claims reactive identity",
                            field.id
                        ));
                    }
                }
                SemanticStorageFieldOriginV1::RecordProjection {
                    parent: origin_parent,
                    expression,
                    ..
                } => {
                    if field.parent != Some(*origin_parent) {
                        return Err(format!(
                            "record-projection storage field {} has inconsistent parent",
                            field.id
                        ));
                    }
                    ids.expression(*expression)?;
                }
                SemanticStorageFieldOriginV1::DetachedCapture {
                    target_owner,
                    target_local,
                    ..
                } => {
                    ids.materialization_local(*target_owner, *target_local)?;
                    if field.role != SemanticStorageFieldRoleV1::Capture
                        || field.reactive_field.is_some()
                    {
                        return Err(format!(
                            "detached-capture storage field {} has inconsistent role or reactive identity",
                            field.id
                        ));
                    }
                }
            }
            if let Some(statement) = field.statement {
                semantic_execution_statement(execution, statement)?;
            }
            if let Some(producer) = field.producer {
                semantic_execution_expression(execution, producer)?;
            }
            Ok(ErasedFieldDef {
                id,
                role: map_storage_field_role(field.role),
                declaration: field.declaration,
                static_owner: field.owner,
                parent,
                row,
                name: field.name.clone(),
                diagnostic_path: field.diagnostic_path.clone(),
                statement,
                producer,
                resource_only: field.resource_only,
                flow_type: field.flow_type.clone(),
            })
        })
        .collect()
}

fn map_storage_local_member_target(
    target: SemanticStorageLocalMemberTargetV1,
    ids: &SemanticToExecutableMap,
    storage_ids: &SemanticStorageToErasedMap,
) -> Result<ErasedLocalMemberTarget, String> {
    Ok(match target {
        SemanticStorageLocalMemberTargetV1::Field(field) => {
            ErasedLocalMemberTarget::Field(storage_ids.storage_field(field)?)
        }
        SemanticStorageLocalMemberTargetV1::Source(source) => {
            ErasedLocalMemberTarget::Source(ids.runtime_source(source)?)
        }
        SemanticStorageLocalMemberTargetV1::State(state) => {
            ErasedLocalMemberTarget::State(ids.runtime_state(state)?)
        }
    })
}

fn map_storage_local_member_forwarding(
    forwarding: &SemanticStorageLocalMemberForwardingV1,
    ids: &SemanticToExecutableMap,
) -> Result<ErasedLocalMemberForwarding, String> {
    Ok(match forwarding {
        SemanticStorageLocalMemberForwardingV1::Local { owner, local, path } => {
            ErasedLocalMemberForwarding::Local {
                owner: *owner,
                local: ids.materialization_local(*owner, *local)?,
                path: path.clone(),
            }
        }
        SemanticStorageLocalMemberForwardingV1::Row { row, path } => {
            ErasedLocalMemberForwarding::Row {
                row: map_row_binding(ids, *row)?,
                path: path.clone(),
            }
        }
    })
}

fn map_storage_locals(
    execution: &SemanticExecutionGraphV1,
    storage: &SemanticScopeStorageGraphV1,
    ids: &SemanticToExecutableMap,
    storage_ids: &SemanticStorageToErasedMap,
    fields: &[ErasedFieldDef],
) -> Result<Vec<ErasedLocalDef>, String> {
    if storage.locals.len() != execution.materializations.len() {
        return Err(format!(
            "semantic storage has {} locals for {} materializations",
            storage.locals.len(),
            execution.materializations.len()
        ));
    }
    require_dense(
        storage
            .locals
            .iter()
            .flat_map(|local| &local.captures)
            .map(|capture| capture.id.as_usize()),
        "semantic storage capture",
    )?;

    let mut seen_locals = BTreeSet::new();
    let mapped = storage
        .locals
        .iter()
        .map(|local| {
            if !seen_locals.insert((local.owner, local.local)) {
                return Err(format!(
                    "semantic storage repeats local {}:{}",
                    local.owner, local.local
                ));
            }
            let materializations = execution
                .materializations
                .iter()
                .filter(|materialization| {
                    materialization.owner == local.owner && materialization.row_local == local.local
                })
                .collect::<Vec<_>>();
            let [materialization] = materializations.as_slice() else {
                return Err(format!(
                    "semantic storage local {}:{} resolves to {} exact materializations",
                    local.owner,
                    local.local,
                    materializations.len()
                ));
            };
            if materialization.source != local.source
                || materialization.item_type != local.item_type
            {
                return Err(format!(
                    "semantic storage local {}:{} differs from its materialization",
                    local.owner, local.local
                ));
            }
            let row = local.row.map(|row| map_row_binding(ids, row)).transpose()?;
            let target_row = match (
                materialization.target_list_id,
                materialization.target_scope_id,
            ) {
                (Some(list), Some(scope)) => {
                    Some(map_row_binding(ids, SemanticRowBinding { list, scope })?)
                }
                (None, None) => None,
                _ => {
                    return Err(format!(
                        "semantic materialization {} has a partial target-row identity",
                        materialization.id
                    ));
                }
            };
            let members = local
                .members
                .iter()
                .map(|member| {
                    Ok(ErasedLocalMember {
                        path: member.path.clone(),
                        target: map_storage_local_member_target(member.target, ids, storage_ids)?,
                        forwarded_from: member
                            .forwarded_from
                            .as_ref()
                            .map(|forwarding| map_storage_local_member_forwarding(forwarding, ids))
                            .transpose()?,
                    })
                })
                .collect::<Result<Vec<_>, String>>()?;
            let captures = local
                .captures
                .iter()
                .map(|capture| {
                    let semantic_field = storage
                        .fields
                        .get(capture.field.as_usize())
                        .filter(|candidate| candidate.id == capture.field)
                        .ok_or_else(|| {
                            format!(
                                "semantic storage capture {} references missing field {}",
                                capture.id, capture.field
                            )
                        })?;
                    if !matches!(
                        semantic_field.origin,
                        SemanticStorageFieldOriginV1::DetachedCapture {
                            capture: id,
                            target_owner,
                            target_local,
                        } if id == capture.id
                            && target_owner == local.owner
                            && target_local == local.local
                    ) {
                        return Err(format!(
                            "semantic storage capture {} and field {} do not form an exact join",
                            capture.id, capture.field
                        ));
                    }
                    let field = storage_ids.storage_field(capture.field)?;
                    fields
                        .get(field.as_usize())
                        .filter(|candidate| {
                            candidate.id == field
                                && candidate.role == ErasedFieldRole::Capture
                                && candidate.row == target_row
                        })
                        .ok_or_else(|| {
                            format!(
                                "semantic storage capture {} maps to inconsistent FieldId {field}",
                                capture.id
                            )
                        })?;
                    Ok(ErasedLocalCapture {
                        source_owner: capture.source_owner,
                        source_local: ids
                            .materialization_local(capture.source_owner, capture.source_local)?,
                        projection: capture.projection.clone(),
                        field,
                    })
                })
                .collect::<Result<Vec<_>, String>>()?;
            Ok(ErasedLocalDef {
                owner: local.owner,
                local: ids.materialization_local(local.owner, local.local)?,
                row,
                source: ids.expression(local.source)?,
                item_type: local.item_type.clone(),
                members,
                captures,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;

    for materialization in &execution.materializations {
        if !seen_locals.contains(&(materialization.owner, materialization.row_local)) {
            return Err(format!(
                "semantic materialization {} has no exact storage local {}:{}",
                materialization.id, materialization.owner, materialization.row_local
            ));
        }
    }
    Ok(mapped)
}

fn final_binding_field(
    semantic: SemanticStorageFieldId,
    storage_ids: &SemanticStorageToErasedMap,
    fields: &[ErasedFieldDef],
) -> Result<Option<FieldId>, String> {
    let field = storage_ids.storage_field(semantic)?;
    let definition = fields
        .get(field.as_usize())
        .filter(|candidate| candidate.id == field)
        .ok_or_else(|| {
            format!("semantic storage field {semantic} maps to missing FieldId {field}")
        })?;
    Ok((!definition.resource_only).then_some(field))
}

fn map_storage_binding_target(
    target: &SemanticStorageBindingTargetV1,
    mapped: &MappedSemanticBinding,
    ids: &SemanticToExecutableMap,
    storage_ids: &SemanticStorageToErasedMap,
    fields: &[ErasedFieldDef],
    reactive_fields: &[MappedSemanticField],
) -> Result<ErasedBindingTarget, String> {
    match target {
        SemanticStorageBindingTargetV1::Value { field, row } => {
            let MappedSemanticBindingTarget::Field {
                field: reactive_field,
            } = &mapped.target
            else {
                return Err(format!(
                    "storage binding {} is a value but staged target is {:?}",
                    mapped.id, mapped.target
                ));
            };
            let final_field = storage_ids.storage_field(*field)?;
            if storage_ids.reactive_field(*reactive_field)? != final_field {
                return Err(format!(
                    "storage binding {} field {} is not the exact join for reactive field {}",
                    mapped.id, field, reactive_field
                ));
            }
            let mapped_field = mapped_field(reactive_fields, *reactive_field)?;
            let row = row.map(|row| map_row_binding(ids, row)).transpose()?;
            if row != mapped_field.row {
                return Err(format!(
                    "storage binding {} value row differs from reactive field {}",
                    mapped.id, reactive_field
                ));
            }
            Ok(ErasedBindingTarget::Value {
                field: final_binding_field(*field, storage_ids, fields)?,
                row,
            })
        }
        SemanticStorageBindingTargetV1::Source { source } => {
            let MappedSemanticBindingTarget::Source {
                executable,
                runtime,
            } = &mapped.target
            else {
                return Err(format!(
                    "storage binding {} is a source but staged target is {:?}",
                    mapped.id, mapped.target
                ));
            };
            if *executable != ids.source(*source)? || *runtime != ids.runtime_source(*source)? {
                return Err(format!(
                    "storage binding {} source {} differs from its staged allocation",
                    mapped.id, source
                ));
            }
            Ok(ErasedBindingTarget::Source {
                executable: *executable,
                runtime: *runtime,
            })
        }
        SemanticStorageBindingTargetV1::State {
            state,
            published,
            field,
            row,
        } => {
            let MappedSemanticBindingTarget::State {
                executable,
                runtime,
                published: staged_published,
                field: reactive_field,
                row: staged_row,
            } = &mapped.target
            else {
                return Err(format!(
                    "storage binding {} is a state but staged target is {:?}",
                    mapped.id, mapped.target
                ));
            };
            if *executable != ids.state(*state)?
                || *runtime != ids.runtime_state(*state)?
                || *published != *staged_published
            {
                return Err(format!(
                    "storage binding {} state {} differs from its staged allocation",
                    mapped.id, state
                ));
            }
            let field = field
                .map(|field| {
                    let final_field = storage_ids.storage_field(field)?;
                    if storage_ids.reactive_field(*reactive_field)? != final_field {
                        return Err(format!(
                            "storage binding {} state field {} is not the exact join for reactive field {}",
                            mapped.id, field, reactive_field
                        ));
                    }
                    final_binding_field(field, storage_ids, fields)
                })
                .transpose()?
                .flatten();
            if field.is_none() {
                return Err(format!(
                    "storage binding {} state {} omits its staged reactive field {}",
                    mapped.id, state, reactive_field
                ));
            }
            let row = row.map(|row| map_row_binding(ids, row)).transpose()?;
            if row != *staged_row {
                return Err(format!(
                    "storage binding {} state {} row differs from staged topology",
                    mapped.id, state
                ));
            }
            Ok(ErasedBindingTarget::State {
                executable: *executable,
                runtime: *runtime,
                published: *published,
                field,
                row,
            })
        }
        SemanticStorageBindingTargetV1::List { list, field, row } => {
            let MappedSemanticBindingTarget::List {
                list: staged_list,
                field: reactive_field,
                row: staged_row,
            } = &mapped.target
            else {
                return Err(format!(
                    "storage binding {} is a list but staged target is {:?}",
                    mapped.id, mapped.target
                ));
            };
            let final_field = storage_ids.storage_field(*field)?;
            let row = map_row_binding(ids, *row)?;
            if *staged_list != ids.list(*list)?
                || storage_ids.reactive_field(*reactive_field)? != final_field
                || *staged_row != row
            {
                return Err(format!(
                    "storage binding {} list {} differs from its staged field/row allocation",
                    mapped.id, list
                ));
            }
            Ok(ErasedBindingTarget::Value {
                field: None,
                row: Some(row),
            })
        }
    }
}

fn map_storage_bindings(
    storage: &SemanticScopeStorageGraphV1,
    ids: &SemanticToExecutableMap,
    reactive: &MappedSemanticReactive,
    storage_ids: &SemanticStorageToErasedMap,
    fields: &[ErasedFieldDef],
) -> Result<Vec<ErasedBinding>, String> {
    let mut result = vec![None; reactive.bindings.len()];
    for storage_binding in &storage.bindings {
        let staged_id = MappedReactiveBindingId(storage_binding.binding.as_usize());
        let mapped = mapped_binding(&reactive.bindings, staged_id)?;
        let id = storage_ids.binding(mapped.id)?;
        if storage_binding.owner_ancestry != mapped.owner_ancestry
            || storage_binding.diagnostic_path != mapped.diagnostic_path
        {
            return Err(format!(
                "semantic storage binding {} differs from staged owner/path metadata",
                storage_binding.binding
            ));
        }
        let binding = ErasedBinding {
            id,
            declaration: mapped.declaration,
            static_owner: mapped.owner,
            owner_ancestry: storage_binding.owner_ancestry.clone(),
            flow_type: mapped.flow_type.clone(),
            producer: mapped.producer,
            diagnostic_path: storage_binding.diagnostic_path.clone(),
            target: map_storage_binding_target(
                &storage_binding.target,
                mapped,
                ids,
                storage_ids,
                fields,
                &reactive.fields,
            )?,
        };
        let slot = result.get_mut(id.as_usize()).ok_or_else(|| {
            format!(
                "semantic storage binding {} maps outside the erased binding domain",
                storage_binding.binding
            )
        })?;
        if slot.replace(binding).is_some() {
            return Err(format!(
                "semantic storage binding {} maps to duplicate ErasedBindingId {id}",
                storage_binding.binding
            ));
        }
    }
    result
        .into_iter()
        .enumerate()
        .map(|(index, binding)| {
            binding.ok_or_else(|| {
                format!(
                    "ErasedBindingId {} has no exact semantic storage binding",
                    ErasedBindingId(index)
                )
            })
        })
        .collect()
}

fn map_storage_sources(
    resources: &SemanticResourceGraphV1,
    storage: &SemanticScopeStorageGraphV1,
    ids: &SemanticToExecutableMap,
    reactive: &MappedSemanticReactive,
    storage_ids: &SemanticStorageToErasedMap,
) -> Result<Vec<ErasedSourceDef>, String> {
    if storage.sources.len() != resources.sources.len()
        || reactive.sources.len() != resources.sources.len()
    {
        return Err(format!(
            "semantic storage/source staging covers {}/{} sources for {} resources",
            storage.sources.len(),
            reactive.sources.len(),
            resources.sources.len()
        ));
    }
    let mut result = vec![None; resources.sources.len()];
    for source in &storage.sources {
        let semantic = semantic_source_resource(resources, source.source)?;
        let runtime = ids.runtime_source(source.source)?;
        let staged = reactive
            .sources
            .get(runtime.as_usize())
            .filter(|candidate| candidate.source == runtime)
            .ok_or_else(|| {
                format!(
                    "semantic storage source {} has no staged runtime source {runtime}",
                    source.source
                )
            })?;
        let binding = storage_ids.binding(staged.binding)?;
        if source.owner != semantic.owner
            || source.owner != staged.owner
            || source.owner_ancestry != staged.owner_ancestry
            || source.origin != semantic.origin
            || source.binding.as_usize() != staged.binding.0
            || staged.executable != ids.source(source.source)?
        {
            return Err(format!(
                "semantic storage source {} differs from exact resource/staged identity",
                source.source
            ));
        }
        let erased = ErasedSourceDef {
            source: runtime,
            static_owner: source.owner,
            owner_ancestry: source.owner_ancestry.clone(),
            origin: ErasedSourceOrigin::Executable {
                executable: staged.executable,
                binding,
            },
        };
        let slot = result.get_mut(runtime.as_usize()).ok_or_else(|| {
            format!(
                "semantic storage source {} maps outside runtime sources",
                source.source
            )
        })?;
        if slot.replace(erased).is_some() {
            return Err(format!("semantic storage repeats runtime source {runtime}"));
        }
    }
    result
        .into_iter()
        .enumerate()
        .map(|(index, source)| {
            source.ok_or_else(|| {
                format!(
                    "runtime source {} has no exact semantic storage source",
                    SourceId(index)
                )
            })
        })
        .collect()
}

fn map_storage_external_references(
    execution: &SemanticExecutionGraphV1,
    reactive_graph: &SemanticReactiveGraphV1,
    storage: &SemanticScopeStorageGraphV1,
    ids: &SemanticToExecutableMap,
    reactive: &MappedSemanticReactive,
    storage_ids: &SemanticStorageToErasedMap,
) -> Result<Vec<MappedSemanticExternalReference>, String> {
    let mut expected = BTreeSet::new();
    for read in &reactive_graph.reads {
        if matches!(read.target, SemanticReadTargetV1::External { .. }) {
            expected.insert((0_u8, read.id.as_usize(), read.expression.as_usize()));
        }
    }
    for schedule in &reactive_graph.call_invocations {
        expected.insert((
            1_u8,
            schedule.call.as_usize(),
            schedule.expression.as_usize(),
        ));
    }

    let mut seen = BTreeSet::new();
    let mut mapped = Vec::with_capacity(storage.external_references.len());
    for reference in &storage.external_references {
        storage_ids.external_reference(reference.id)?;
        if reference.bundle_ready != reference.external_identity.is_some() {
            return Err(format!(
                "semantic external reference {} has inconsistent bundle readiness",
                reference.id
            ));
        }
        let kind = match reference.kind {
            SemanticStorageExternalReferenceKindV1::Read { read, expression } => {
                let semantic = reactive_graph
                    .reads
                    .get(read.as_usize())
                    .filter(|candidate| candidate.id == read)
                    .ok_or_else(|| {
                        format!(
                            "semantic external reference {} targets missing read {read}",
                            reference.id
                        )
                    })?;
                if semantic.expression != expression {
                    return Err(format!(
                        "semantic external reference {} read {read} has stale expression identity",
                        reference.id
                    ));
                }
                let staged_id = MappedReactiveReadId(read.as_usize());
                let staged = reactive
                    .reads
                    .get(staged_id.0)
                    .filter(|candidate| candidate.id == staged_id)
                    .ok_or_else(|| {
                        format!(
                            "semantic external reference {} read {read} has no staged mapping",
                            reference.id
                        )
                    })?;
                let MappedSemanticReadTarget::External {
                    canonical_path,
                    external_identity,
                } = &staged.target
                else {
                    return Err(format!(
                        "semantic external reference {} read {read} is not staged as external",
                        reference.id
                    ));
                };
                if staged.expression != ids.expression(expression)?
                    || canonical_path != &reference.canonical_path
                    || external_identity != &reference.external_identity
                    || reference.external_identity.is_some_and(|identity| {
                        identity.kind != CheckedExternalDeclarationKind::Value
                    })
                {
                    return Err(format!(
                        "semantic external reference {} read {read} differs from exact staged identity",
                        reference.id
                    ));
                }
                if !seen.insert((0_u8, read.as_usize(), expression.as_usize())) {
                    return Err(format!(
                        "semantic external read {read} has multiple storage references"
                    ));
                }
                MappedSemanticExternalReferenceKind::Read {
                    semantic_read: read,
                    read: storage_ids.read(staged.id)?,
                    expression: staged.expression,
                }
            }
            SemanticStorageExternalReferenceKindV1::Call { call, expression } => {
                let semantic = semantic_call(execution, call)?;
                let executable = ids.call_expression(call, expression)?;
                if semantic.function != reference.canonical_path
                    || semantic.external_identity != reference.external_identity
                    || reference.external_identity.is_some_and(|identity| {
                        identity.kind != CheckedExternalDeclarationKind::Callable
                    })
                {
                    return Err(format!(
                        "semantic external reference {} call {call}/{} differs from exact semantic identity",
                        reference.id, expression
                    ));
                }
                if !reactive_graph
                    .call_invocations
                    .iter()
                    .any(|schedule| schedule.call == call && schedule.expression == expression)
                {
                    return Err(format!(
                        "semantic external reference {} call {call}/{} has no invocation schedule",
                        reference.id, expression
                    ));
                }
                if !seen.insert((1_u8, call.as_usize(), expression.as_usize())) {
                    return Err(format!(
                        "semantic external call {call}/{expression} has multiple storage references"
                    ));
                }
                MappedSemanticExternalReferenceKind::Call {
                    call,
                    expression: executable,
                }
            }
        };
        mapped.push(MappedSemanticExternalReference {
            id: reference.id,
            kind,
            canonical_path: reference.canonical_path.clone(),
            external_identity: reference.external_identity,
            bundle_ready: reference.bundle_ready,
        });
    }
    if seen != expected {
        return Err(format!(
            "semantic storage external-reference identities {seen:?} differ from exact staged set {expected:?}"
        ));
    }
    Ok(mapped)
}

fn external_reference_for_read(
    references: &[MappedSemanticExternalReference],
    read: SemanticReadId,
) -> Result<&MappedSemanticExternalReference, String> {
    let matches = references
        .iter()
        .filter(|reference| {
            matches!(
                reference.kind,
                MappedSemanticExternalReferenceKind::Read {
                    semantic_read,
                    ..
                } if semantic_read == read
            )
        })
        .collect::<Vec<_>>();
    let [reference] = matches.as_slice() else {
        return Err(format!(
            "semantic external read {read} resolves to {} exact storage references",
            matches.len()
        ));
    };
    Ok(*reference)
}

fn external_reference_for_call(
    references: &[MappedSemanticExternalReference],
    call: SemanticCallId,
    expression: ExecutableExprId,
) -> Result<&MappedSemanticExternalReference, String> {
    let matches = references
        .iter()
        .filter(|reference| {
            matches!(
                reference.kind,
                MappedSemanticExternalReferenceKind::Call {
                    call: candidate,
                    expression: candidate_expression,
                } if candidate == call && candidate_expression == expression
            )
        })
        .collect::<Vec<_>>();
    let [reference] = matches.as_slice() else {
        return Err(format!(
            "semantic external call {call}/{expression} resolves to {} exact storage references",
            matches.len()
        ));
    };
    Ok(*reference)
}

fn map_storage_reads(
    reactive_graph: &SemanticReactiveGraphV1,
    reactive: &MappedSemanticReactive,
    storage_ids: &SemanticStorageToErasedMap,
    external_references: &[MappedSemanticExternalReference],
) -> Result<Vec<MappedSemanticStorageRead>, String> {
    if reactive_graph.reads.len() != reactive.reads.len() {
        return Err(format!(
            "reactive read graph has {} records but staging has {}",
            reactive_graph.reads.len(),
            reactive.reads.len()
        ));
    }
    reactive_graph
        .reads
        .iter()
        .map(|semantic| {
            let staged_id = MappedReactiveReadId(semantic.id.as_usize());
            let staged = reactive
                .reads
                .get(staged_id.0)
                .filter(|candidate| candidate.id == staged_id)
                .ok_or_else(|| format!("semantic read {} has no exact staged read", semantic.id))?;
            let target = match &staged.target {
                MappedSemanticReadTarget::Binding {
                    binding,
                    projection,
                } => MappedSemanticStorageReadTarget::Binding {
                    binding: storage_ids.binding(*binding)?,
                    projection: projection.clone(),
                },
                MappedSemanticReadTarget::SourcePayload {
                    binding,
                    source,
                    payload_projection,
                    projection,
                } => MappedSemanticStorageReadTarget::SourcePayload {
                    binding: storage_ids.binding(*binding)?,
                    source: *source,
                    payload_projection: payload_projection.clone(),
                    projection: projection.clone(),
                },
                MappedSemanticReadTarget::StateProjection {
                    binding,
                    state,
                    projection,
                } => MappedSemanticStorageReadTarget::StateProjection {
                    binding: storage_ids.binding(*binding)?,
                    state: *state,
                    projection: projection.clone(),
                },
                MappedSemanticReadTarget::Local {
                    binding,
                    declaration,
                    producer,
                    projection,
                } => MappedSemanticStorageReadTarget::Local {
                    binding: *binding,
                    declaration: *declaration,
                    producer: *producer,
                    projection: projection.clone(),
                },
                MappedSemanticReadTarget::External { .. } => {
                    let reference = external_reference_for_read(external_references, semantic.id)?;
                    MappedSemanticStorageReadTarget::BundleExternal {
                        reference: reference.id,
                    }
                }
                MappedSemanticReadTarget::ElementState {
                    context,
                    projection,
                } => MappedSemanticStorageReadTarget::ElementState {
                    context: *context,
                    projection: projection.clone(),
                },
                MappedSemanticReadTarget::MaterializationLocal {
                    owner,
                    local,
                    projection,
                } => MappedSemanticStorageReadTarget::MaterializationLocal {
                    owner: *owner,
                    local: *local,
                    projection: projection.clone(),
                },
                MappedSemanticReadTarget::FunctionParameter {
                    parameter,
                    projection,
                } => MappedSemanticStorageReadTarget::FunctionParameter {
                    parameter: *parameter,
                    projection: projection.clone(),
                },
            };
            Ok(MappedSemanticStorageRead {
                id: storage_ids.read(staged.id)?,
                expression: staged.expression,
                target,
            })
        })
        .collect()
}

fn map_storage_dependency_uses(
    reactive_graph: &SemanticReactiveGraphV1,
    reactive: &MappedSemanticReactive,
    storage_ids: &SemanticStorageToErasedMap,
    external_references: &[MappedSemanticExternalReference],
) -> Result<Vec<MappedSemanticStorageDependencyUse>, String> {
    if reactive_graph.dependency_uses.len() != reactive.dependency_uses.len() {
        return Err(format!(
            "reactive dependency-use graph has {} records but staging has {}",
            reactive_graph.dependency_uses.len(),
            reactive.dependency_uses.len()
        ));
    }
    reactive_graph
        .dependency_uses
        .iter()
        .enumerate()
        .map(|(index, semantic)| {
            if semantic.id.as_usize() != index {
                return Err(format!(
                    "semantic dependency use {} is not canonical at index {index}",
                    semantic.id
                ));
            }
            let staged = &reactive.dependency_uses[index];
            let target = match (&semantic.target, &staged.target) {
                (
                    SemanticDependencyTargetV1::ExternalRead { read: semantic_read },
                    MappedSemanticDependencyTarget::ExternalRead { read: staged_read },
                ) => {
                    let expected_staged = MappedReactiveReadId(semantic_read.as_usize());
                    if *staged_read != expected_staged {
                        return Err(format!(
                            "semantic dependency use {} read {} differs from staged read {}",
                            semantic.id, semantic_read, staged_read
                        ));
                    }
                    let reference =
                        external_reference_for_read(external_references, *semantic_read)?;
                    MappedSemanticStorageDependencyTarget::BundleExternalRead {
                        read: storage_ids.read(*staged_read)?,
                        reference: reference.id,
                    }
                }
                (
                    SemanticDependencyTargetV1::ExternalCall { call, .. },
                    MappedSemanticDependencyTarget::ExternalCall {
                        expression,
                        external_identity,
                    },
                ) => {
                    let reference =
                        external_reference_for_call(external_references, *call, *expression)?;
                    if reference.external_identity != Some(*external_identity) {
                        return Err(format!(
                            "semantic dependency use {} call identity differs from storage reference {}",
                            semantic.id, reference.id
                        ));
                    }
                    MappedSemanticStorageDependencyTarget::BundleExternalCall {
                        reference: reference.id,
                    }
                }
                _ => {
                    return Err(format!(
                        "semantic dependency use {} target differs from its staged target",
                        semantic.id
                    ));
                }
            };
            Ok(MappedSemanticStorageDependencyUse {
                dependent: storage_ids.binding(staged.dependent)?,
                expression: staged.expression,
                target,
                timing: staged.timing.clone(),
            })
        })
        .collect()
}

fn semantic_storage_binding(
    storage: &SemanticScopeStorageGraphV1,
    id: SemanticBindingId,
) -> Result<&boon_semantic::SemanticStorageBindingV1, String> {
    let matches = storage
        .bindings
        .iter()
        .filter(|binding| binding.binding == id)
        .collect::<Vec<_>>();
    let [binding] = matches.as_slice() else {
        return Err(format!(
            "semantic binding {id} resolves to {} exact storage bindings",
            matches.len()
        ));
    };
    Ok(*binding)
}

fn map_storage_named_value_target(
    target: &SemanticNamedValueStorageTargetV1,
    origin: &boon_semantic::SemanticNamedValueTypeOriginV1,
    storage: &SemanticScopeStorageGraphV1,
    ids: &SemanticToExecutableMap,
    storage_ids: &SemanticStorageToErasedMap,
) -> Result<MappedSemanticNamedValueTarget, String> {
    Ok(match target {
        SemanticNamedValueStorageTargetV1::Field { binding, field } => {
            let storage_field = storage
                .fields
                .get(field.as_usize())
                .filter(|candidate| candidate.id == *field)
                .ok_or_else(|| format!("named value references missing storage field {field}"))?;
            let binding = binding
                .map(|binding| {
                    if !origin.bindings.contains(&binding)
                        || !matches!(
                            semantic_storage_binding(storage, binding)?.target,
                            SemanticStorageBindingTargetV1::Value {
                                field: candidate,
                                ..
                            } if candidate == *field
                        )
                    {
                        return Err(format!(
                            "named-value field {field} binding {binding} is not an exact origin/storage join"
                        ));
                    }
                    storage_ids.binding(MappedReactiveBindingId(binding.as_usize()))
                })
                .transpose()?;
            if binding.is_none()
                && !storage_field
                    .statement
                    .is_some_and(|statement| origin.statements.contains(&statement))
                && !storage_field
                    .producer
                    .is_some_and(|producer| origin.expressions.contains(&producer))
            {
                return Err(format!(
                    "unbound named-value field {field} is absent from the exact semantic origin"
                ));
            }
            MappedSemanticNamedValueTarget::Field {
                binding,
                field: storage_ids.storage_field(*field)?,
            }
        }
        SemanticNamedValueStorageTargetV1::Source { binding, source } => {
            if !origin.bindings.contains(binding) && !origin.sources.contains(source) {
                return Err(format!(
                    "named-value source {source} binding {binding} is absent from the exact semantic origin"
                ));
            }
            if !matches!(
                semantic_storage_binding(storage, *binding)?.target,
                SemanticStorageBindingTargetV1::Source {
                    source: candidate,
                } if candidate == *source
            ) {
                return Err(format!(
                    "named-value source {source} binding {binding} differs from storage topology"
                ));
            }
            MappedSemanticNamedValueTarget::Source {
                binding: storage_ids.binding(MappedReactiveBindingId(binding.as_usize()))?,
                source: ids.runtime_source(*source)?,
            }
        }
        SemanticNamedValueStorageTargetV1::State {
            binding,
            state,
            field,
        } => {
            if !origin.bindings.contains(binding) && !origin.states.contains(state) {
                return Err(format!(
                    "named-value state {state} binding {binding} is absent from the exact semantic origin"
                ));
            }
            if !matches!(
                semantic_storage_binding(storage, *binding)?.target,
                SemanticStorageBindingTargetV1::State {
                    state: candidate,
                    field: candidate_field,
                    ..
                } if candidate == *state && candidate_field == *field
            ) {
                return Err(format!(
                    "named-value state {state} binding {binding} differs from storage topology"
                ));
            }
            MappedSemanticNamedValueTarget::State {
                binding: storage_ids.binding(MappedReactiveBindingId(binding.as_usize()))?,
                state: ids.runtime_state(*state)?,
                field: field
                    .map(|field| storage_ids.storage_field(field))
                    .transpose()?,
            }
        }
        SemanticNamedValueStorageTargetV1::List {
            binding,
            list,
            field,
            row,
        } => {
            if !origin.bindings.contains(binding) && !origin.lists.contains(list) {
                return Err(format!(
                    "named-value list {list} binding {binding} is absent from the exact semantic origin"
                ));
            }
            if !matches!(
                semantic_storage_binding(storage, *binding)?.target,
                SemanticStorageBindingTargetV1::List {
                    list: candidate,
                    field: candidate_field,
                    row: candidate_row,
                } if candidate == *list
                    && candidate_field == *field
                    && candidate_row == *row
            ) {
                return Err(format!(
                    "named-value list {list} binding {binding} differs from storage topology"
                ));
            }
            MappedSemanticNamedValueTarget::List {
                binding: storage_ids.binding(MappedReactiveBindingId(binding.as_usize()))?,
                list: ids.list(*list)?,
                field: storage_ids.storage_field(*field)?,
                row: map_row_binding(ids, *row)?,
            }
        }
        SemanticNamedValueStorageTargetV1::Value {
            expression,
            value,
            field,
        } => {
            if !origin.expressions.contains(expression) || !origin.values.contains(value) {
                return Err(format!(
                    "named-value expression {expression}/value {value} is absent from the exact semantic origin"
                ));
            }
            let expression = ids.expression(*expression)?;
            let value = ids.value(*value)?;
            if expression != value {
                return Err(
                    "named-value expression and value map to different executable identities"
                        .to_owned(),
                );
            }
            MappedSemanticNamedValueTarget::Value {
                expression,
                value,
                field: field
                    .map(|field| storage_ids.storage_field(field))
                    .transpose()?,
            }
        }
        SemanticNamedValueStorageTargetV1::DiagnosticOnly { reason } => {
            if origin.statements.is_empty()
                || !origin.expressions.is_empty()
                || !origin.bindings.is_empty()
                || !origin.sources.is_empty()
                || !origin.states.is_empty()
                || !origin.lists.is_empty()
            {
                return Err(
                    "diagnostic-only named value has executable semantic origin identity"
                        .to_owned(),
                );
            }
            MappedSemanticNamedValueTarget::DiagnosticOnly { reason: *reason }
        }
    })
}

fn named_value_storage_flow_type(
    execution: &SemanticExecutionGraphV1,
    resources: &SemanticResourceGraphV1,
    reactive: &SemanticReactiveGraphV1,
    storage: &SemanticScopeStorageGraphV1,
    target: &SemanticNamedValueStorageTargetV1,
) -> Result<FlowType, String> {
    Ok(match target {
        SemanticNamedValueStorageTargetV1::Field { field, .. } => storage
            .fields
            .get(field.as_usize())
            .filter(|candidate| candidate.id == *field)
            .map(|field| field.flow_type.clone())
            .ok_or_else(|| format!("named value references missing storage field {field}"))?,
        SemanticNamedValueStorageTargetV1::Source { binding, source } => {
            reactive
                .bindings
                .get(binding.as_usize())
                .filter(|candidate| candidate.id == *binding)
                .ok_or_else(|| {
                    format!("named value source references missing binding {binding}")
                })?;
            FlowType {
                mode: boon_typecheck::FlowMode::TickPresent,
                ty: semantic_source_resource(resources, *source)?
                    .payload_type
                    .clone(),
            }
        }
        SemanticNamedValueStorageTargetV1::State { binding, state, .. } => {
            reactive
                .bindings
                .get(binding.as_usize())
                .filter(|candidate| candidate.id == *binding)
                .ok_or_else(|| format!("named value state references missing binding {binding}"))?;
            semantic_state_resource(resources, *state)?
                .flow_type
                .clone()
        }
        SemanticNamedValueStorageTargetV1::List { binding, .. } => reactive
            .bindings
            .get(binding.as_usize())
            .filter(|candidate| candidate.id == *binding)
            .map(|binding| binding.flow_type.clone())
            .ok_or_else(|| format!("named value list references missing binding {binding}"))?,
        SemanticNamedValueStorageTargetV1::Value {
            expression, value, ..
        } => {
            let expression = semantic_execution_expression(execution, *expression)?;
            if expression.value_id != *value {
                return Err(format!(
                    "named-value target expression {} does not own value {value}",
                    expression.id
                ));
            }
            expression.flow_type.clone()
        }
        SemanticNamedValueStorageTargetV1::DiagnosticOnly { .. } => FlowType {
            mode: boon_typecheck::FlowMode::Continuous,
            ty: boon_typecheck::Type::Unknown,
        },
    })
}

fn canonical_type_field_order(shape: &boon_typecheck::ObjectShape) -> Vec<String> {
    let mut order = Vec::new();
    let mut seen = BTreeSet::new();
    for field in shape.field_order.iter().chain(shape.fields.keys()) {
        if shape.fields.contains_key(field) && seen.insert(field.clone()) {
            order.push(field.clone());
        }
    }
    order
}

const fn named_value_target_storage_field(
    target: &SemanticNamedValueStorageTargetV1,
) -> Option<SemanticStorageFieldId> {
    match target {
        SemanticNamedValueStorageTargetV1::Field { field, .. }
        | SemanticNamedValueStorageTargetV1::List { field, .. } => Some(*field),
        SemanticNamedValueStorageTargetV1::State { field, .. }
        | SemanticNamedValueStorageTargetV1::Value { field, .. } => *field,
        SemanticNamedValueStorageTargetV1::Source { .. }
        | SemanticNamedValueStorageTargetV1::DiagnosticOnly { .. } => None,
    }
}

fn derive_mapped_storage_representation(
    storage: &boon_typecheck::Type,
    contract: &boon_typecheck::Type,
) -> Result<MappedSemanticStorageRepresentation, String> {
    fn visit(
        storage: &boon_typecheck::Type,
        contract: &boon_typecheck::Type,
        path: &mut Vec<MappedSemanticStorageTypePathSegment>,
        refinements: &mut Vec<MappedSemanticStorageFixedBytesRefinement>,
    ) -> Result<(), String> {
        if storage == contract {
            return Ok(());
        }
        match (storage, contract) {
            (
                boon_typecheck::Type::Bytes(boon_typecheck::BytesType::Dynamic),
                boon_typecheck::Type::Bytes(boon_typecheck::BytesType::Fixed(fixed_len)),
            ) => {
                refinements.push(MappedSemanticStorageFixedBytesRefinement {
                    path: path.clone(),
                    fixed_len: *fixed_len,
                });
                Ok(())
            }
            (boon_typecheck::Type::List(storage), boon_typecheck::Type::List(contract)) => {
                path.push(MappedSemanticStorageTypePathSegment::ListItem);
                let result = visit(storage, contract, path, refinements);
                path.pop();
                result
            }
            (boon_typecheck::Type::Object(storage), boon_typecheck::Type::Object(contract))
                if storage.open == contract.open
                    && storage.fields.len() == contract.fields.len()
                    && storage.fields.keys().eq(contract.fields.keys()) =>
            {
                for (field_ordinal, selector) in
                    canonical_type_field_order(storage).into_iter().enumerate()
                {
                    let storage_field = storage.fields.get(&selector).ok_or_else(|| {
                        format!("mapped storage representation lost object field `{selector}`")
                    })?;
                    let contract_field = contract.fields.get(&selector).ok_or_else(|| {
                        format!(
                            "mapped storage representation contract lost object field `{selector}`"
                        )
                    })?;
                    path.push(MappedSemanticStorageTypePathSegment::ObjectField {
                        selector,
                        field_ordinal,
                    });
                    visit(storage_field, contract_field, path, refinements)?;
                    path.pop();
                }
                Ok(())
            }
            _ => Err(format!(
                "named-value storage representation {storage:?} does not exactly preserve contract {contract:?}"
            )),
        }
    }

    let mut refinements = Vec::new();
    visit(storage, contract, &mut Vec::new(), &mut refinements)?;
    Ok(if refinements.is_empty() {
        MappedSemanticStorageRepresentation::Exact
    } else {
        MappedSemanticStorageRepresentation::CheckedFixedBytes { refinements }
    })
}

fn map_storage_representation_shape(
    semantic: &boon_semantic::SemanticStorageRepresentationV1,
) -> Result<MappedSemanticStorageRepresentation, String> {
    Ok(match semantic {
        boon_semantic::SemanticStorageRepresentationV1::Exact => {
            MappedSemanticStorageRepresentation::Exact
        }
        boon_semantic::SemanticStorageRepresentationV1::CheckedFixedBytes { refinements } => {
            if refinements.is_empty() {
                return Err(
                    "checked fixed-BYTES storage representation has no refinements".to_owned(),
                );
            }
            MappedSemanticStorageRepresentation::CheckedFixedBytes {
                refinements: refinements
                    .iter()
                    .map(|refinement| MappedSemanticStorageFixedBytesRefinement {
                        path: refinement
                            .path
                            .iter()
                            .map(|segment| match segment {
                                boon_semantic::SemanticStorageTypePathSegmentV1::ObjectField {
                                    selector,
                                    field_ordinal,
                                } => MappedSemanticStorageTypePathSegment::ObjectField {
                                    selector: selector.clone(),
                                    field_ordinal: *field_ordinal,
                                },
                                boon_semantic::SemanticStorageTypePathSegmentV1::ListItem => {
                                    MappedSemanticStorageTypePathSegment::ListItem
                                }
                            })
                            .collect(),
                        fixed_len: refinement.fixed_len,
                    })
                    .collect(),
            }
        }
    })
}

fn map_storage_representation(
    semantic: &boon_semantic::SemanticStorageRepresentationV1,
    storage_type: &boon_typecheck::Type,
    contract_type: &boon_typecheck::Type,
) -> Result<MappedSemanticStorageRepresentation, String> {
    let mapped = map_storage_representation_shape(semantic)?;
    let expected = derive_mapped_storage_representation(storage_type, contract_type)?;
    if mapped != expected {
        return Err(format!(
            "semantic named-value storage representation {mapped:?} differs from exact structural refinement {expected:?}"
        ));
    }
    Ok(mapped)
}

#[allow(clippy::too_many_arguments)]
fn map_storage_named_values(
    execution: &SemanticExecutionGraphV1,
    resources: &SemanticResourceGraphV1,
    reactive: &SemanticReactiveGraphV1,
    storage: &SemanticScopeStorageGraphV1,
    lowering: &SemanticLoweringContractV1,
    ids: &SemanticToExecutableMap,
    storage_ids: &SemanticStorageToErasedMap,
) -> Result<Vec<MappedSemanticNamedValue>, String> {
    require_dense(
        lowering
            .metadata
            .named_value_types
            .iter()
            .map(|value| value.id.as_usize()),
        "semantic named value",
    )?;
    require_dense(
        storage
            .named_values
            .iter()
            .flat_map(|value| &value.projection)
            .map(|projection| projection.id.as_usize()),
        "semantic named-value projection",
    )?;

    let mut seen_targets = BTreeSet::new();
    let mut mapped = Vec::with_capacity(storage.named_values.len());
    for value in &storage.named_values {
        let named = lowering
            .metadata
            .named_value_types
            .get(value.named_value.as_usize())
            .filter(|candidate| candidate.id == value.named_value)
            .ok_or_else(|| {
                format!(
                    "storage named value references missing type metadata {}",
                    value.named_value
                )
            })?;
        let origin = named.origins.get(value.origin_ordinal).ok_or_else(|| {
            format!(
                "storage named value {} references missing origin {}",
                value.named_value, value.origin_ordinal
            )
        })?;
        if !seen_targets.insert((
            value.named_value,
            value.origin_ordinal,
            value.target_ordinal,
        )) {
            return Err(format!(
                "storage named value {} origin {} target {} has duplicate identity",
                value.named_value, value.origin_ordinal, value.target_ordinal
            ));
        }
        let selectors = value
            .projection
            .iter()
            .map(|projection| projection.selector.as_str())
            .collect::<Vec<_>>();
        if selectors
            != origin
                .checked
                .projection
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>()
        {
            return Err(format!(
                "storage named value {} origin {} projection differs from structural checked selectors",
                value.named_value, value.origin_ordinal
            ));
        }
        let mut storage_type =
            named_value_storage_flow_type(execution, resources, reactive, storage, &value.target)?
                .ty;
        let mut parent_field = named_value_target_storage_field(&value.target);
        for (ordinal, step) in value.projection.iter().enumerate() {
            if step.ordinal != ordinal || step.input_type != storage_type {
                return Err(format!(
                    "named-value projection {} has stale ordinal/input type",
                    step.id
                ));
            }
            let boon_typecheck::Type::Object(shape) = &storage_type else {
                return Err(format!(
                    "named-value projection {} selector `{}` requires object input, got {:?}",
                    step.id, step.selector, storage_type
                ));
            };
            let field_order = canonical_type_field_order(shape);
            if field_order.get(step.field_ordinal) != Some(&step.selector) {
                return Err(format!(
                    "named-value projection {} selector `{}` differs from structural field ordinal {}",
                    step.id, step.selector, step.field_ordinal
                ));
            }
            let output = shape.fields.get(&step.selector).ok_or_else(|| {
                format!(
                    "named-value projection {} references missing structural selector `{}`",
                    step.id, step.selector
                )
            })?;
            if output != &step.output_type {
                return Err(format!(
                    "named-value projection {} output type differs from structural selector `{}`",
                    step.id, step.selector
                ));
            }
            let expected_storage_field = parent_field
                .map(|parent| {
                    let candidates = storage
                        .fields
                        .iter()
                        .filter(|field| {
                            field.parent == Some(parent) && field.name == step.selector
                        })
                        .map(|field| field.id)
                        .collect::<Vec<_>>();
                    match candidates.as_slice() {
                        [] => Ok(None),
                        [field] => Ok(Some(*field)),
                        _ => Err(format!(
                            "named-value projection {} selector `{}` resolves to {} storage children",
                            step.id,
                            step.selector,
                            candidates.len()
                        )),
                    }
                })
                .transpose()?
                .flatten();
            if step.storage_field != expected_storage_field {
                return Err(format!(
                    "named-value projection {} storage field differs from structural parent/selector",
                    step.id
                ));
            }
            let (expected_expression, expected_value) = expected_storage_field
                .map(|field| -> Result<_, String> {
                    let field = storage
                        .fields
                        .get(field.as_usize())
                        .filter(|candidate| candidate.id == field)
                        .ok_or_else(|| {
                            format!(
                                "named-value projection {} references missing storage field {field}",
                                step.id
                            )
                    })?;
                    let expression = field.producer;
                    let value = match expression {
                        Some(expression) => Some(
                            semantic_execution_expression(execution, expression)?.value_id,
                        ),
                        None => None,
                    };
                    Ok((expression, value))
                })
                .transpose()?
                .unwrap_or((None, None));
            if step.expression != expected_expression || step.value != expected_value {
                return Err(format!(
                    "named-value projection {} expression/value differs from its exact storage field",
                    step.id
                ));
            }
            storage_type = step.output_type.clone();
            parent_field = step.storage_field;
        }
        let representation =
            map_storage_representation(&value.representation, &storage_type, &value.flow_type.ty)?;
        let projection = value
            .projection
            .iter()
            .enumerate()
            .map(|(ordinal, projection)| {
                if projection.ordinal != ordinal {
                    return Err(format!(
                        "named-value projection {} has ordinal {}, expected {ordinal}",
                        projection.id, projection.ordinal
                    ));
                }
                let expression = projection
                    .expression
                    .map(|expression| ids.expression(expression))
                    .transpose()?;
                let mapped_value = projection.value.map(|value| ids.value(value)).transpose()?;
                if expression.is_some() && mapped_value.is_some() && expression != mapped_value {
                    return Err(format!(
                        "named-value projection {} expression/value identities differ",
                        projection.id
                    ));
                }
                Ok(MappedSemanticNamedValueProjection {
                    id: projection.id,
                    ordinal: projection.ordinal,
                    selector: projection.selector.clone(),
                    field_ordinal: projection.field_ordinal,
                    input_type: projection.input_type.clone(),
                    output_type: projection.output_type.clone(),
                    storage_field: projection
                        .storage_field
                        .map(|field| storage_ids.storage_field(field))
                        .transpose()?,
                    expression,
                    value: mapped_value,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        for source in &origin.sources {
            semantic_source_resource(resources, *source)?;
        }
        for state in &origin.states {
            semantic_state_resource(resources, *state)?;
        }
        for binding in &origin.bindings {
            reactive
                .bindings
                .get(binding.as_usize())
                .filter(|candidate| candidate.id == *binding)
                .ok_or_else(|| {
                    format!("named-value origin references missing reactive binding {binding}")
                })?;
        }
        mapped.push(MappedSemanticNamedValue {
            named_value: value.named_value,
            checked_statement: named.checked_statement,
            diagnostic_path: named.diagnostic_path.clone(),
            origin_ordinal: value.origin_ordinal,
            target_ordinal: value.target_ordinal,
            target: map_storage_named_value_target(
                &value.target,
                origin,
                storage,
                ids,
                storage_ids,
            )?,
            projection,
            representation,
            flow_type: value.flow_type.clone(),
        });
    }

    for named in &lowering.metadata.named_value_types {
        for (origin_ordinal, _) in named.origins.iter().enumerate() {
            let ordinals = storage
                .named_values
                .iter()
                .filter(|value| {
                    value.named_value == named.id && value.origin_ordinal == origin_ordinal
                })
                .map(|value| value.target_ordinal)
                .collect::<Vec<_>>();
            if ordinals.is_empty()
                || ordinals
                    .iter()
                    .copied()
                    .enumerate()
                    .any(|(expected, actual)| expected != actual)
            {
                return Err(format!(
                    "semantic named value {} origin {origin_ordinal} target ordinals are not total/dense: {ordinals:?}",
                    named.id
                ));
            }
        }
    }
    Ok(mapped)
}

fn finalize_trigger_arm(trigger: &MappedSemanticTriggerArm) -> TriggerOwnedArm {
    TriggerOwnedArm {
        cause: trigger.cause,
        gate_checked_expr_id: trigger.gate_checked_expression,
        gate_expression_id: trigger.gate_expression,
        owner: trigger.owner,
        output_expression_id: trigger.output_expression,
    }
}

fn finalize_state_update_arms(
    reactive: &MappedSemanticReactive,
    triggers: &[TriggerOwnedArm],
) -> Result<Vec<StateUpdateArm>, String> {
    reactive
        .state_update_arms
        .iter()
        .enumerate()
        .map(|(index, arm)| {
            if arm.id != index {
                return Err(format!(
                    "staged state update arm at index {index} has identity {}",
                    arm.id
                ));
            }
            let staged_trigger = reactive
                .trigger_arms
                .get(arm.trigger.0)
                .filter(|candidate| candidate.id == arm.trigger)
                .ok_or_else(|| {
                    format!(
                        "staged state update arm {} references missing trigger {}",
                        arm.id, arm.trigger
                    )
                })?;
            let trigger = triggers.get(arm.trigger.0).ok_or_else(|| {
                format!(
                    "staged state update arm {} trigger {} was not finalized",
                    arm.id, arm.trigger
                )
            })?;
            if trigger != &finalize_trigger_arm(staged_trigger) {
                return Err(format!(
                    "staged state update arm {} trigger {} finalized inconsistently",
                    arm.id, arm.trigger
                ));
            }
            Ok(StateUpdateArm {
                state: arm.state,
                cause: trigger.cause,
                gate_checked_expr_id: trigger.gate_checked_expr_id,
                gate_expression_id: trigger.gate_expression_id,
                owner: trigger.owner,
                output_expression_id: trigger.output_expression_id,
            })
        })
        .collect()
}

fn finalize_list_mutations(reactive: &MappedSemanticReactive) -> Result<Vec<ListMutation>, String> {
    reactive
        .list_mutations
        .iter()
        .enumerate()
        .map(|(index, mutation)| {
            if mutation.id != index {
                return Err(format!(
                    "staged list mutation at index {index} has identity {}",
                    mutation.id
                ));
            }
            let trigger = reactive
                .trigger_arms
                .get(mutation.trigger.0)
                .filter(|candidate| candidate.id == mutation.trigger)
                .ok_or_else(|| {
                    format!(
                        "staged list mutation {} references missing trigger {}",
                        mutation.id, mutation.trigger
                    )
                })?;
            let (gate, output) = match &mutation.kind {
                ListMutationKind::Append { gate, item } => (*gate, *item),
                ListMutationKind::Remove {
                    gate, predicate, ..
                } => (*gate, *predicate),
            };
            if trigger.cause != mutation.cause
                || trigger.owner != mutation.owner
                || trigger.gate_expression != gate
                || trigger.output_expression != output
            {
                return Err(format!(
                    "staged list mutation {} differs from exact trigger {}",
                    mutation.id, mutation.trigger
                ));
            }
            let ordinal = u32::try_from(mutation.id).map_err(|_| {
                format!(
                    "staged list mutation {} exceeds final schedule ordinal range",
                    mutation.id
                )
            })?;
            Ok(ListMutation {
                list_id: mutation.list_id,
                site: mutation.site,
                ordinal,
                cause: mutation.cause,
                owner: mutation.owner,
                kind: mutation.kind.clone(),
            })
        })
        .collect()
}

fn finalize_call_invocation_schedules(
    graph: &SemanticReactiveGraphV1,
    ids: &SemanticToExecutableMap,
    reactive: &MappedSemanticReactive,
    storage_ids: &SemanticStorageToErasedMap,
    triggers: &[TriggerOwnedArm],
) -> Result<Vec<MappedSemanticCallInvocationSchedule>, String> {
    let mut previous_expression = None;
    graph
        .call_invocations
        .iter()
        .map(|schedule| {
            if previous_expression.is_some_and(|previous| previous >= schedule.expression) {
                return Err(
                    "semantic call-invocation schedules are not strictly expression-ordered"
                        .to_owned(),
                );
            }
            previous_expression = Some(schedule.expression);
            let expression = ids.call_expression(schedule.call, schedule.expression)?;
            let value = ids.value(schedule.value)?;
            if expression != value {
                return Err(format!(
                    "semantic call-invocation schedule for expression {} has different executable expression/value identities",
                    schedule.expression
                ));
            }
            let dependent_bindings = schedule
                .dependent_bindings
                .iter()
                .map(|binding| {
                    let staged = reactive.id_map.binding(*binding)?;
                    storage_ids.binding(staged)
                })
                .collect::<Result<Vec<_>, String>>()?;
            let invocation_arms = schedule
                .invocation_arms
                .iter()
                .map(|trigger| {
                    let staged = reactive.id_map.trigger(*trigger)?;
                    triggers.get(staged.0).cloned().ok_or_else(|| {
                        format!(
                            "semantic call-invocation schedule expression {} references missing finalized trigger {}",
                            schedule.expression, trigger
                        )
                    })
                })
                .collect::<Result<Vec<_>, String>>()?;
            Ok(MappedSemanticCallInvocationSchedule {
                expression,
                value,
                call: schedule.call,
                current_capable: schedule.current_capable,
                dependent_bindings,
                invocation_arms,
            })
        })
        .collect()
}

fn finalize_host_effect_schedules(
    graph: &SemanticReactiveGraphV1,
    ids: &SemanticToExecutableMap,
    reactive: &MappedSemanticReactive,
    finalized_state_transitions: &[StateUpdateArm],
) -> Result<Vec<MappedSemanticHostEffectSchedule>, String> {
    graph
        .host_effect_schedules
        .iter()
        .enumerate()
        .map(|(index, schedule)| {
            let allocated = reactive
                .id_map
                .host_effect_schedules
                .get(schedule.id.as_usize())
                .copied()
                .ok_or_else(|| {
                    format!(
                        "semantic host-effect schedule {} has no staged identity",
                        schedule.id
                    )
                })?;
            if allocated != index || schedule.id.as_usize() != index {
                return Err(format!(
                    "semantic host-effect schedule {} is noncanonical at index {index}",
                    schedule.id
                ));
            }
            let expression = ids.call_expression(schedule.call, schedule.expression)?;
            let value = ids.value(schedule.value)?;
            if expression != value {
                return Err(format!(
                    "semantic host-effect schedule {} has different executable expression/value identities",
                    schedule.id
                ));
            }
            let mapped_arms = schedule
                .state_update_arms
                .iter()
                .map(|arm| {
                    let staged = reactive.id_map.state_update_arm(*arm)?;
                    finalized_state_transitions
                        .get(staged)
                        .cloned()
                        .ok_or_else(|| {
                        format!(
                            "semantic host-effect schedule {} references missing finalized state update arm {arm}",
                            schedule.id
                        )
                    })
                })
                .collect::<Result<Vec<_>, String>>()?;
            Ok(MappedSemanticHostEffectSchedule {
                id: allocated,
                expression,
                value,
                call: schedule.call,
                checked_expression: schedule.checked_expression,
                owner: schedule.owner,
                operation: schedule.operation.clone(),
                state_update_arms: mapped_arms,
            })
        })
        .collect()
}

fn finalize_producer_instances(
    reactive_graph: &SemanticReactiveGraphV1,
    storage: &SemanticScopeStorageGraphV1,
    reactive: &MappedSemanticReactive,
    storage_ids: &SemanticStorageToErasedMap,
) -> Result<Vec<ProducerFunctionInstance>, String> {
    if reactive_graph.producer_instances.len() != reactive.producer_function_instances.len()
        || storage.producer_result_fields.len() != reactive_graph.producer_instances.len()
    {
        return Err(format!(
            "producer-result storage covers {} records for {}/{} semantic/staged instances",
            storage.producer_result_fields.len(),
            reactive_graph.producer_instances.len(),
            reactive.producer_function_instances.len()
        ));
    }
    let mut result_storage = BTreeMap::new();
    for result in &storage.producer_result_fields {
        if result_storage.insert(result.identity, result).is_some() {
            return Err(format!(
                "producer identity {} has multiple storage-result joins",
                producer_identity_text(result.identity)
            ));
        }
    }
    let mut seen = BTreeSet::new();
    reactive_graph
        .producer_instances
        .iter()
        .zip(&reactive.producer_function_instances)
        .map(|(semantic, mapped)| {
            if semantic.identity != mapped.identity {
                return Err(
                    "semantic and staged producer instance order/identity differs".to_owned(),
                );
            }
            let result = result_storage.get(&mapped.identity).ok_or_else(|| {
                format!(
                    "producer identity {} has no exact storage-result join",
                    producer_identity_text(mapped.identity)
                )
            })?;
            if !seen.insert(mapped.identity) {
                return Err(format!(
                    "staged producer identity {} is duplicated",
                    producer_identity_text(mapped.identity)
                ));
            }
            let reactive_field = SemanticFieldId(mapped.result_field.0);
            let field = storage_ids.reactive_field(mapped.result_field)?;
            let storage_field = storage
                .fields
                .get(result.storage_field.as_usize())
                .filter(|candidate| candidate.id == result.storage_field)
                .ok_or_else(|| {
                    format!(
                        "producer identity {} references missing storage field {}",
                        producer_identity_text(mapped.identity),
                        result.storage_field
                    )
                })?;
            if result.reactive_field != reactive_field
                || storage_ids.storage_field(result.storage_field)? != field
                || storage_field.reactive_field != Some(reactive_field)
                || storage_field.producer_identity != Some(mapped.identity)
            {
                return Err(format!(
                    "producer identity {} field joins differ across reactive/storage domains",
                    producer_identity_text(mapped.identity)
                ));
            }
            let semantic_binding = reactive_graph
                .bindings
                .get(result.binding.as_usize())
                .filter(|candidate| candidate.id == result.binding)
                .ok_or_else(|| {
                    format!(
                        "producer identity {} references missing result binding {}",
                        producer_identity_text(mapped.identity),
                        result.binding
                    )
                })?;
            if semantic_binding.statement != semantic.result_statement
                || semantic_binding.producer != semantic.root_expression
                || semantic_binding.value != semantic.root_value
            {
                return Err(format!(
                    "producer identity {} result binding {} has stale producer identity",
                    producer_identity_text(mapped.identity),
                    result.binding
                ));
            }
            storage_ids.binding(MappedReactiveBindingId(result.binding.as_usize()))?;
            Ok(ProducerFunctionInstance {
                identity: mapped.identity,
                owner: mapped.owner,
                function: mapped.function,
                function_name: mapped.function_name.clone(),
                result_field: field,
                result_path: mapped.result_path.clone(),
                root: mapped.root,
                mode: mapped.mode,
                invocation_source: mapped.invocation_source,
                arguments: mapped.arguments.clone(),
            })
        })
        .collect()
}

fn finalize_derived_values(
    reactive_graph: &SemanticReactiveGraphV1,
    reactive: &MappedSemanticReactive,
    storage_ids: &SemanticStorageToErasedMap,
    triggers: &[TriggerOwnedArm],
) -> Result<Vec<DerivedValue>, String> {
    if reactive_graph.derived_values.len() != reactive.derived_values.len() {
        return Err(format!(
            "semantic/staged derived value counts differ: {}/{}",
            reactive_graph.derived_values.len(),
            reactive.derived_values.len()
        ));
    }
    reactive_graph
        .derived_values
        .iter()
        .zip(&reactive.derived_values)
        .enumerate()
        .map(|(index, (semantic, mapped))| {
            if semantic.id.as_usize() != index || semantic.field.as_usize() != mapped.field.0 {
                return Err(format!(
                    "semantic derived value {} differs from staged field {}",
                    semantic.id, mapped.field
                ));
            }
            let trigger_arms = mapped
                .trigger_arms
                .iter()
                .map(|trigger| {
                    triggers.get(trigger.0).cloned().ok_or_else(|| {
                        format!(
                            "staged derived value {} references missing finalized trigger {}",
                            semantic.id, trigger
                        )
                    })
                })
                .collect::<Result<Vec<_>, String>>()?;
            Ok(DerivedValue {
                id: storage_ids.reactive_field(mapped.field)?,
                executable_statement_id: mapped.executable_statement_id,
                path: mapped.path.clone(),
                kind: mapped.kind.clone(),
                materialized_list_id: mapped.materialized_list_id,
                materialized_row_scope_id: mapped.materialized_row_scope_id,
                causes: mapped.causes.clone(),
                trigger_arms,
                default_roots: mapped.default_roots.clone(),
                sources: mapped.sources.clone(),
                indexed: mapped.indexed,
                scope_id: mapped.scope_id,
                startup_recompute: mapped.startup_recompute,
            })
        })
        .collect()
}

impl MappedSemanticStorage {
    pub(super) fn validate_totality(
        &self,
        storage: &SemanticScopeStorageGraphV1,
        reactive: &SemanticReactiveGraphV1,
        resources: &MappedSemanticResources,
        ids: &SemanticToExecutableMap,
    ) -> Result<(), String> {
        let lengths = [
            ("owner", storage.owners.len(), self.owners.len()),
            ("local", storage.locals.len(), self.locals.len()),
            ("field", storage.fields.len(), self.fields.len()),
            ("binding", storage.bindings.len(), self.bindings.len()),
            ("source", storage.sources.len(), self.sources.len()),
            ("read", reactive.reads.len(), self.reads.len()),
            ("row value", storage.row_values.len(), self.row_values.len()),
            (
                "row source projection",
                storage.row_source_projections.len(),
                self.row_source_projections.len(),
            ),
            (
                "dependency use",
                reactive.dependency_uses.len(),
                self.dependency_uses.len(),
            ),
            (
                "call-invocation schedule",
                reactive.call_invocations.len(),
                self.call_invocations.len(),
            ),
            (
                "host-effect schedule",
                reactive.host_effect_schedules.len(),
                self.host_effect_schedules.len(),
            ),
            (
                "external reference",
                storage.external_references.len(),
                self.external_references.len(),
            ),
            (
                "named value target",
                storage.named_values.len(),
                self.named_values.len(),
            ),
            (
                "producer instance",
                reactive.producer_instances.len(),
                self.producer_function_instances.len(),
            ),
            (
                "derived value",
                reactive.derived_values.len(),
                self.derived_values.len(),
            ),
            (
                "trigger arm",
                reactive.trigger_arms.len(),
                self.trigger_arms.len(),
            ),
            (
                "state update arm",
                reactive.state_update_arms.len(),
                self.state_update_arms.len(),
            ),
            (
                "list mutation",
                reactive.list_mutations.len(),
                self.list_mutations.len(),
            ),
            (
                "dependency edge",
                reactive.dependencies.len(),
                self.dependencies.len(),
            ),
            (
                "possible cause",
                resources.state_cells.len(),
                self.possible_causes.len(),
            ),
        ];
        for (label, semantic, mapped) in lengths {
            if semantic != mapped {
                return Err(format!(
                    "semantic storage {label} domain has {semantic} records but mapping emitted {mapped}"
                ));
            }
        }
        let allocation_lengths = [
            (
                "storage field",
                storage.fields.len(),
                self.id_map.storage_fields.len(),
            ),
            (
                "reactive field",
                reactive.fields.len(),
                self.id_map.reactive_fields.len(),
            ),
            (
                "binding",
                reactive.bindings.len(),
                self.id_map.bindings.len(),
            ),
            ("read", reactive.reads.len(), self.id_map.reads.len()),
            (
                "external reference",
                storage.external_references.len(),
                self.id_map.external_references.len(),
            ),
        ];
        for (label, semantic, allocated) in allocation_lengths {
            if semantic != allocated {
                return Err(format!(
                    "semantic storage {label} domain has {semantic} records but allocation covers {allocated}"
                ));
            }
        }
        for (index, field) in self.fields.iter().enumerate() {
            let expected = self.id_map.storage_field(SemanticStorageFieldId(index))?;
            if field.id != expected {
                return Err(format!(
                    "mapped storage field at index {index} has {}, expected {expected}",
                    field.id
                ));
            }
            if let Some(parent) = field.parent {
                self.fields
                    .get(parent.as_usize())
                    .filter(|candidate| candidate.id == parent)
                    .ok_or_else(|| {
                        format!(
                            "mapped storage field {} has missing parent {parent}",
                            field.id
                        )
                    })?;
            }
        }
        let reactive_fields = self
            .id_map
            .reactive_fields
            .iter()
            .copied()
            .collect::<BTreeSet<_>>();
        if reactive_fields.len() != reactive.fields.len() {
            return Err(
                "mapped reactive fields do not join one-to-one to storage FieldIds".to_owned(),
            );
        }
        for (index, binding) in self.bindings.iter().enumerate() {
            let expected = self.id_map.binding(MappedReactiveBindingId(index))?;
            if binding.id != expected {
                return Err(format!(
                    "mapped storage binding at index {index} has {}, expected {expected}",
                    binding.id
                ));
            }
        }
        for (index, read) in self.reads.iter().enumerate() {
            let expected = self.id_map.read(MappedReactiveReadId(index))?;
            if read.id != expected {
                return Err(format!(
                    "mapped storage read at index {index} has {}, expected {expected}",
                    read.id
                ));
            }
            if let MappedSemanticStorageReadTarget::BundleExternal { reference } = read.target {
                self.external_references
                    .get(reference.as_usize())
                    .filter(|candidate| {
                        candidate.id == reference
                            && matches!(
                                candidate.kind,
                                MappedSemanticExternalReferenceKind::Read {
                                    read: candidate_read,
                                    ..
                                } if candidate_read == read.id
                            )
                    })
                    .ok_or_else(|| {
                        format!(
                            "mapped read {} references missing bundle-external identity {reference}",
                            read.id
                        )
                    })?;
            }
        }
        for (index, source) in self.sources.iter().enumerate() {
            if source.source != SourceId(index) {
                return Err(format!(
                    "mapped storage source at index {index} has runtime ID {}",
                    source.source
                ));
            }
        }
        for (index, (semantic, reference)) in storage
            .external_references
            .iter()
            .zip(&self.external_references)
            .enumerate()
        {
            let expected = self
                .id_map
                .external_reference(SemanticStorageExternalReferenceId(index))?;
            if reference.id != expected
                || reference.canonical_path != semantic.canonical_path
                || reference.external_identity != semantic.external_identity
                || reference.bundle_ready != reference.external_identity.is_some()
                || !matches!(
                    (semantic.kind, &reference.kind),
                    (
                        SemanticStorageExternalReferenceKindV1::Read { read, .. },
                        MappedSemanticExternalReferenceKind::Read {
                            semantic_read,
                            ..
                        },
                    ) if read == *semantic_read
                ) && !matches!(
                    (semantic.kind, &reference.kind),
                    (
                        SemanticStorageExternalReferenceKindV1::Call { call, .. },
                        MappedSemanticExternalReferenceKind::Call {
                            call: mapped_call,
                            ..
                        },
                    ) if call == *mapped_call
                )
            {
                return Err(format!(
                    "mapped external reference at index {index} has inconsistent identity/readiness"
                ));
            }
        }
        let mut expected_projection_id = 0;
        let covered_named_values = storage
            .named_values
            .iter()
            .map(|value| value.named_value)
            .collect::<BTreeSet<_>>();
        let expected_named_values = (0..self.named_value_checked_statements.len())
            .map(SemanticNamedValueId)
            .collect::<BTreeSet<_>>();
        if covered_named_values != expected_named_values {
            return Err(format!(
                "mapped named-value rows cover IDs {covered_named_values:?}, expected exact checked-site domain {expected_named_values:?}"
            ));
        }
        for (semantic, mapped) in storage.named_values.iter().zip(&self.named_values) {
            let expected_representation =
                map_storage_representation_shape(&semantic.representation)?;
            let expected_checked_statement = self
                .named_value_checked_statements
                .get(semantic.named_value.as_usize())
                .copied()
                .ok_or_else(|| {
                    format!(
                        "semantic named value {} has no exact checked statement site",
                        semantic.named_value
                    )
                })?;
            if mapped.named_value != semantic.named_value
                || mapped.checked_statement != expected_checked_statement
                || mapped.origin_ordinal != semantic.origin_ordinal
                || mapped.target_ordinal != semantic.target_ordinal
                || mapped.flow_type != semantic.flow_type
                || mapped.projection.len() != semantic.projection.len()
                || mapped.representation != expected_representation
            {
                return Err(format!(
                    "mapped named-value target {}:{}/{} differs from semantic storage identity",
                    semantic.named_value, semantic.origin_ordinal, semantic.target_ordinal
                ));
            }
            for (semantic_projection, projection) in
                semantic.projection.iter().zip(&mapped.projection)
            {
                if projection.id != SemanticStorageProjectionId(expected_projection_id) {
                    return Err(format!(
                        "mapped named-value projection {} is not dense at index {expected_projection_id}",
                        projection.id
                    ));
                }
                if projection.id != semantic_projection.id
                    || projection.ordinal != semantic_projection.ordinal
                    || projection.selector != semantic_projection.selector
                    || projection.field_ordinal != semantic_projection.field_ordinal
                    || projection.input_type != semantic_projection.input_type
                    || projection.output_type != semantic_projection.output_type
                {
                    return Err(format!(
                        "mapped named-value projection {} differs from semantic storage",
                        projection.id
                    ));
                }
                expected_projection_id += 1;
            }
        }
        for dependency in &self.dependency_uses {
            self.bindings
                .get(dependency.dependent.as_usize())
                .filter(|candidate| candidate.id == dependency.dependent)
                .ok_or_else(|| {
                    format!(
                        "mapped dependency use references missing binding {}",
                        dependency.dependent
                    )
                })?;
            let (reference, expected_read) = match dependency.target {
                MappedSemanticStorageDependencyTarget::BundleExternalRead { read, reference } => {
                    self.reads
                        .get(read.as_usize())
                        .filter(|candidate| candidate.id == read)
                        .ok_or_else(|| {
                            format!("mapped dependency use references missing read {read}")
                        })?;
                    (reference, Some(read))
                }
                MappedSemanticStorageDependencyTarget::BundleExternalCall { reference } => {
                    (reference, None)
                }
            };
            let external = self
                .external_references
                .get(reference.as_usize())
                .filter(|candidate| candidate.id == reference)
                .ok_or_else(|| {
                    format!(
                        "mapped dependency use references missing external identity {reference}"
                    )
                })?;
            match (expected_read, &external.kind) {
                (
                    Some(read),
                    MappedSemanticExternalReferenceKind::Read {
                        read: external_read,
                        ..
                    },
                ) if read == *external_read => {}
                (None, MappedSemanticExternalReferenceKind::Call { .. }) => {}
                _ => {
                    return Err(format!(
                        "mapped dependency use target kind differs from external reference {reference}"
                    ));
                }
            }
        }
        for (semantic, mapped) in reactive.call_invocations.iter().zip(&self.call_invocations) {
            let expected_expression = ids.call_expression(semantic.call, semantic.expression)?;
            let expected_value = ids.value(semantic.value)?;
            let expected_bindings = semantic
                .dependent_bindings
                .iter()
                .map(|binding| {
                    self.id_map
                        .binding(MappedReactiveBindingId(binding.as_usize()))
                })
                .collect::<Result<Vec<_>, String>>()?;
            let expected_arms = semantic
                .invocation_arms
                .iter()
                .map(|trigger| {
                    self.trigger_arms
                        .get(trigger.as_usize())
                        .cloned()
                        .ok_or_else(|| {
                            format!(
                                "semantic call-invocation schedule expression {} references missing trigger {trigger}",
                                semantic.expression
                            )
                        })
                })
                .collect::<Result<Vec<_>, String>>()?;
            if mapped.expression != expected_expression
                || mapped.value != expected_value
                || mapped.call != semantic.call
                || mapped.current_capable != semantic.current_capable
                || mapped.dependent_bindings != expected_bindings
                || mapped.invocation_arms != expected_arms
            {
                return Err(format!(
                    "mapped call-invocation schedule for semantic expression {} differs from its exact final-ID join",
                    semantic.expression
                ));
            }
        }
        for (index, (semantic, mapped)) in reactive
            .host_effect_schedules
            .iter()
            .zip(&self.host_effect_schedules)
            .enumerate()
        {
            let expected_expression = ids.call_expression(semantic.call, semantic.expression)?;
            let expected_value = ids.value(semantic.value)?;
            let expected_arms = semantic
                .state_update_arms
                .iter()
                .map(|arm| {
                    self.state_update_arms
                        .get(arm.as_usize())
                        .cloned()
                        .ok_or_else(|| {
                            format!(
                                "semantic host-effect schedule {} references missing state update arm {arm}",
                                semantic.id
                            )
                        })
                })
                .collect::<Result<Vec<_>, String>>()?;
            if semantic.id.as_usize() != index
                || mapped.id != index
                || mapped.expression != expected_expression
                || mapped.value != expected_value
                || mapped.call != semantic.call
                || mapped.checked_expression != semantic.checked_expression
                || mapped.owner != semantic.owner
                || mapped.operation != semantic.operation
                || mapped.state_update_arms != expected_arms
            {
                return Err(format!(
                    "mapped host-effect schedule {} differs from its exact final-ID join",
                    semantic.id
                ));
            }
        }
        for (index, (semantic, mapped)) in reactive
            .list_mutations
            .iter()
            .zip(&self.list_mutations)
            .enumerate()
        {
            let expected_ordinal = u32::try_from(index).map_err(|_| {
                format!("semantic list mutation {index} exceeds final schedule ordinal range")
            })?;
            if semantic.id.as_usize() != index
                || mapped.ordinal != expected_ordinal
                || mapped.list_id != ids.list(semantic.list)?
                || mapped.site != ids.expression(semantic.site)?
                || mapped.owner != semantic.owner
            {
                return Err(format!(
                    "mapped list mutation at index {index} differs from its exact semantic schedule"
                ));
            }
        }
        for (semantic, mapped) in reactive.derived_values.iter().zip(&self.derived_values) {
            let expected = self
                .id_map
                .reactive_field(MappedReactiveFieldId(semantic.field.as_usize()))?;
            if mapped.id != expected {
                return Err(format!(
                    "mapped derived value {} has result field {}, expected {expected}",
                    semantic.id, mapped.id
                ));
            }
        }
        for (semantic, mapped) in storage
            .producer_result_fields
            .iter()
            .zip(&self.producer_function_instances)
        {
            if mapped.identity != semantic.identity
                || mapped.result_field != self.id_map.storage_field(semantic.storage_field)?
            {
                return Err(format!(
                    "mapped producer identity {} differs from its exact storage-result join",
                    producer_identity_text(semantic.identity)
                ));
            }
        }
        let producer_fields = self
            .producer_function_instances
            .iter()
            .map(|producer| producer.result_field)
            .collect::<BTreeSet<_>>();
        if producer_fields.len() != self.producer_function_instances.len() {
            return Err("mapped producer instances reuse result FieldIds".to_owned());
        }
        let derived_fields = self
            .derived_values
            .iter()
            .map(|derived| derived.id)
            .collect::<BTreeSet<_>>();
        if derived_fields.len() != self.derived_values.len() {
            return Err("mapped derived values reuse result FieldIds".to_owned());
        }
        Ok(())
    }
}

fn semantic_list_resource(
    graph: &SemanticResourceGraphV1,
    id: SemanticListId,
) -> Result<&boon_semantic::SemanticListResourceV1, String> {
    graph
        .lists
        .get(id.as_usize())
        .filter(|list| list.id == id)
        .ok_or_else(|| format!("missing semantic list resource {id}"))
}

fn validate_initializer_references(
    ids: &SemanticToExecutableMap,
    initializer: &SemanticListInitializerV1,
) -> Result<(), String> {
    match initializer {
        SemanticListInitializerV1::Empty => {}
        SemanticListInitializerV1::RecordLiteral {
            authority_root,
            rows,
        } => {
            ids.expression(*authority_root)?;
            for row in rows {
                ids.expression(row.expression)?;
                for field in &row.fields {
                    if let Some(expression) = field.expression {
                        ids.expression(expression)?;
                    }
                    if let Some(origin) = field.spread_origin {
                        ids.expression(origin)?;
                    }
                }
            }
        }
        SemanticListInitializerV1::ValueLiteral {
            authority_root,
            values,
        } => {
            ids.expression(*authority_root)?;
            for value in values {
                ids.expression(value.expression)?;
            }
        }
        SemanticListInitializerV1::Range {
            authority_root,
            from_expression,
            to_expression,
            ..
        } => {
            ids.expression(*authority_root)?;
            ids.expression(*from_expression)?;
            ids.expression(*to_expression)?;
        }
    }
    Ok(())
}

fn map_list_initializer(
    ids: &SemanticToExecutableMap,
    initializer: &SemanticListInitializerV1,
) -> Result<ListInitializer, String> {
    Ok(match initializer {
        SemanticListInitializerV1::Empty => ListInitializer::Empty,
        SemanticListInitializerV1::RecordLiteral { rows, .. } => ListInitializer::RecordLiteral {
            rows: rows
                .iter()
                .map(|row| {
                    Ok(ListInitialRecord {
                        fields: row
                            .fields
                            .iter()
                            .map(|field| {
                                Ok(ListRowInitialField {
                                    name: field.name.clone(),
                                    value: map_initial_value(&field.value),
                                    expression: field
                                        .expression
                                        .map(|expression| ids.expression(expression))
                                        .transpose()?,
                                })
                            })
                            .collect::<Result<Vec<_>, String>>()?,
                    })
                })
                .collect::<Result<Vec<_>, String>>()?,
        },
        SemanticListInitializerV1::ValueLiteral { .. } => {
            return Err(
                "value-only list initializer reached runtime row-storage lowering".to_owned(),
            );
        }
        SemanticListInitializerV1::Range { from, to, .. } => ListInitializer::Range {
            from: *from,
            to: *to,
        },
    })
}

fn map_initial_value(value: &SemanticInitialValueV1) -> InitialValue {
    match value {
        SemanticInitialValueV1::Text { value } => InitialValue::Text {
            value: value.clone(),
        },
        SemanticInitialValueV1::Number { value } => InitialValue::Number {
            value: value.clone(),
        },
        SemanticInitialValueV1::Bytes { bytes, fixed_len } => InitialValue::Bytes {
            bytes: bytes.clone(),
            fixed_len: *fixed_len,
        },
        SemanticInitialValueV1::Tag { name } => InitialValue::Tag { name: name.clone() },
        SemanticInitialValueV1::Data { value } => InitialValue::Data {
            value: value.clone(),
        },
        SemanticInitialValueV1::RootInitialField { path } => {
            InitialValue::RootInitialField { path: path.clone() }
        }
        SemanticInitialValueV1::RowInitialField { path } => {
            InitialValue::RowInitialField { path: path.clone() }
        }
        SemanticInitialValueV1::Unknown { summary } => InitialValue::Unknown {
            summary: summary.clone(),
        },
    }
}

fn map_source_resource(
    execution: &SemanticExecutionGraphV1,
    ids: &SemanticToExecutableMap,
    source: &boon_semantic::SemanticSourceResourceV1,
) -> Result<SourcePort, String> {
    ids.statement(source.statement)?;
    if let Some(target) = source.target_list {
        ids.list(target)?;
    }
    let expression = semantic_expression(execution, source.expression)?;
    let source_expr_id = match source.origin {
        SemanticSourceOrigin::Checked { .. } => {
            Some(runtime_checked_expression_id(expression.checked_expr_id)?)
        }
        SemanticSourceOrigin::ProducerInvocation { .. } => None,
    };
    Ok(SourcePort {
        id: ids.runtime_source(source.id)?,
        path: source.semantic_path.clone(),
        binding_path: source.binding_path.clone(),
        executable_source_id: Some(ids.source(source.id)?),
        static_owner: source.owner,
        source_expr_id,
        source_line: source.span.line,
        scoped: source.scoped,
        scope_id: source
            .row_scope
            .map(|scope| ids.row_scope(scope))
            .transpose()?,
        interval_ms: source.interval_ms,
        payload_schema: map_source_payload_schema(source),
    })
}

fn map_source_payload_schema(
    source: &boon_semantic::SemanticSourceResourceV1,
) -> SourcePayloadSchema {
    let typed_fields = source
        .payload_fields
        .iter()
        .filter_map(|field| {
            let data_type = semantic_data_type(&field.data_type);
            (!matches!(data_type, crate::SemanticDataType::Unknown { .. })).then(|| {
                SourcePayloadDescriptor {
                    field: SourcePayloadField::from_name(&field.name),
                    data_type,
                }
            })
        })
        .collect::<Vec<_>>();
    SourcePayloadSchema {
        fields: typed_fields
            .iter()
            .map(|field| field.field.clone())
            .collect(),
        typed_fields,
    }
}

fn map_state_resource(
    execution: &SemanticExecutionGraphV1,
    ids: &SemanticToExecutableMap,
    state: &boon_semantic::SemanticStateResourceV1,
) -> Result<StateCell, String> {
    ids.expression(state.expression)?;
    ids.expression(state.initial)?;
    if let Some(target) = state.target_list {
        ids.list(target)?;
    }
    let mut expression_ids = BTreeSet::new();
    for expression in &state.expression_members {
        let expression = semantic_expression(execution, *expression)?;
        expression_ids.insert(runtime_checked_expression_id(expression.checked_expr_id)?);
    }
    Ok(StateCell {
        id: ids.runtime_state(state.id)?,
        path: state.path.clone(),
        published: state.published,
        semantic_path: state.semantic_path.clone(),
        executable_state_id: Some(ids.state(state.id)?),
        static_owner: state.owner,
        statement_id: ids.statement(state.statement)?.as_usize(),
        scope_id: state
            .row_scope
            .map(|scope| ids.row_scope(scope))
            .transpose()?,
        hold_name: state.hold_name.clone(),
        expression_ids: expression_ids.into_iter().collect(),
        indexed: state.scoped,
        source_line: state.span.line,
    })
}

fn semantic_expression(
    graph: &SemanticExecutionGraphV1,
    id: SemanticExprId,
) -> Result<&SemanticExpression, String> {
    graph
        .expressions
        .get(id.as_usize())
        .filter(|expression| expression.id == id)
        .ok_or_else(|| format!("missing semantic expression {id}"))
}

fn runtime_checked_expression_id(id: boon_typecheck::CheckedExprId) -> Result<ExprId, String> {
    let value = usize::try_from(id.0).map_err(|_| {
        format!(
            "checked expression {} exceeds executable usize identity space",
            id.0
        )
    })?;
    Ok(ExprId(value))
}

fn map_expression(
    graph: &SemanticExecutionGraphV1,
    ids: &SemanticToExecutableMap,
    expression: &SemanticExpression,
) -> Result<ExecutableExpression, String> {
    let id = ids.expression(expression.id)?;
    let value = ids.value(expression.value_id)?;
    if id != value {
        return Err(format!(
            "semantic expression {} and value {} map to different executable handles",
            expression.id, expression.value_id
        ));
    }
    Ok(ExecutableExpression {
        id,
        checked_expr_id: expression.checked_expr_id,
        flow_type: expression.flow_type.clone(),
        effect: expression.effect,
        owner: expression.owner,
        provenance: map_provenance(graph, ids, &expression.provenance)?,
        resource_binding_path: expression.resource_binding_path.clone(),
        kind: map_expression_kind(graph, ids, expression)?,
    })
}

fn map_expression_kind(
    graph: &SemanticExecutionGraphV1,
    ids: &SemanticToExecutableMap,
    expression: &SemanticExpression,
) -> Result<ExecutableExpressionKind, String> {
    let kind = &expression.kind;
    Ok(match kind {
        SemanticExpressionKind::CanonicalRead {
            target,
            path,
            projection,
            source,
        } => ExecutableExpressionKind::CanonicalRead {
            target: *target,
            path: path.clone(),
            projection: projection.clone(),
            source: source
                .as_ref()
                .map(|source| map_source_read(graph, source))
                .transpose()?,
        },
        SemanticExpressionKind::LocalRead {
            binding,
            declaration,
            projection,
        } => ExecutableExpressionKind::LocalRead {
            binding: ids.local(*binding)?,
            declaration: *declaration,
            projection: projection.clone(),
        },
        SemanticExpressionKind::ExternalRead {
            canonical_path,
            external_identity,
        } => {
            validate_external_value_identity(
                expression.id,
                canonical_path,
                external_identity.as_ref(),
            )?;
            ExecutableExpressionKind::ExternalRead {
                canonical_path: canonical_path.clone(),
            }
        }
        SemanticExpressionKind::ElementState {
            context,
            projection,
        } => ExecutableExpressionKind::ElementState {
            context: ids.call_context(*context)?,
            projection: projection.clone(),
        },
        SemanticExpressionKind::Drain {
            target,
            path,
            projection,
        } => ExecutableExpressionKind::Drain {
            target: *target,
            path: path.clone(),
            projection: projection.clone(),
        },
        SemanticExpressionKind::Text(value) => ExecutableExpressionKind::Text(value.clone()),
        SemanticExpressionKind::TextTemplate { segments } => {
            ExecutableExpressionKind::TextTemplate {
                segments: segments
                    .iter()
                    .map(|segment| map_text_segment(ids, segment))
                    .collect::<Result<Vec<_>, _>>()?,
            }
        }
        SemanticExpressionKind::Number(value) => ExecutableExpressionKind::Number(value.clone()),
        SemanticExpressionKind::BytesByte(value) => ExecutableExpressionKind::BytesByte(*value),
        SemanticExpressionKind::Absent => ExecutableExpressionKind::Absent,
        SemanticExpressionKind::Flush { payload } => ExecutableExpressionKind::Flush {
            payload: ids.expression(*payload)?,
        },
        SemanticExpressionKind::FlushBoundary { input } => {
            ExecutableExpressionKind::FlushBoundary {
                input: ids.expression(*input)?,
            }
        }
        SemanticExpressionKind::Tag(value) => ExecutableExpressionKind::Tag(value.clone()),
        SemanticExpressionKind::TaggedObject { tag, fields } => {
            ExecutableExpressionKind::TaggedObject {
                tag: tag.clone(),
                fields: fields
                    .iter()
                    .map(|field| map_record_field(ids, field))
                    .collect::<Result<Vec<_>, _>>()?,
            }
        }
        SemanticExpressionKind::Source { binding_path } => ExecutableExpressionKind::Source {
            binding_path: binding_path.clone(),
        },
        SemanticExpressionKind::Call {
            callable_kind,
            name,
            instance,
            arguments,
            contexts,
            ..
        } => {
            validate_call_expression(graph, ids, expression)?;
            ExecutableExpressionKind::Call {
                callable_kind: match callable_kind {
                    SemanticCallableKind::Builtin => ExecutableCallableKind::Builtin,
                    SemanticCallableKind::External => ExecutableCallableKind::External,
                },
                name: name.clone(),
                instance: ids.call_instance(*instance)?,
                arguments: arguments
                    .iter()
                    .map(|argument| map_call_argument(ids, argument))
                    .collect::<Result<Vec<_>, _>>()?,
                contexts: contexts
                    .iter()
                    .map(|context| ids.call_context(*context))
                    .collect::<Result<Vec<_>, _>>()?,
            }
        }
        SemanticExpressionKind::Materialize { materialization } => {
            ExecutableExpressionKind::Materialize {
                materialization: ids.materialization(*materialization)?,
            }
        }
        SemanticExpressionKind::Draining { input } => ExecutableExpressionKind::Draining {
            input: ids.expression(*input)?,
        },
        SemanticExpressionKind::Hold {
            initial,
            name,
            binding_path,
            updates,
        } => ExecutableExpressionKind::Hold {
            initial: ids.expression(*initial)?,
            name: name.clone(),
            binding_path: binding_path.clone(),
            updates: updates
                .iter()
                .map(|update| ids.expression(*update))
                .collect::<Result<Vec<_>, _>>()?,
        },
        SemanticExpressionKind::Latest { branches } => ExecutableExpressionKind::Latest {
            branches: branches
                .iter()
                .map(|branch| ids.expression(*branch))
                .collect::<Result<Vec<_>, _>>()?,
        },
        SemanticExpressionKind::When {
            select_kind: _,
            input,
            arms,
        } => ExecutableExpressionKind::When {
            input: ids.expression(*input)?,
            arms: arms
                .iter()
                .map(|arm| map_select_arm(ids, arm))
                .collect::<Result<Vec<_>, _>>()?,
        },
        SemanticExpressionKind::Then { input, output } => ExecutableExpressionKind::Then {
            input: ids.expression(*input)?,
            output: output.map(|output| ids.expression(output)).transpose()?,
        },
        SemanticExpressionKind::Infix { left, op, right } => ExecutableExpressionKind::Infix {
            left: ids.expression(*left)?,
            op: op.clone(),
            right: ids.expression(*right)?,
        },
        SemanticExpressionKind::MatchArm { pattern, output } => {
            ExecutableExpressionKind::MatchArm {
                pattern: pattern.clone(),
                output: output.map(|output| ids.expression(output)).transpose()?,
            }
        }
        SemanticExpressionKind::Object(fields) => ExecutableExpressionKind::Object(
            fields
                .iter()
                .map(|field| map_record_field(ids, field))
                .collect::<Result<Vec<_>, _>>()?,
        ),
        SemanticExpressionKind::Block { bindings, result } => ExecutableExpressionKind::Block {
            bindings: bindings
                .iter()
                .map(|binding| map_block_binding(ids, binding))
                .collect::<Result<Vec<_>, _>>()?,
            result: ids.expression(*result)?,
        },
        SemanticExpressionKind::List { capacity, items } => ExecutableExpressionKind::List {
            capacity: *capacity,
            items: items
                .iter()
                .map(|item| ids.expression(*item))
                .collect::<Result<Vec<_>, _>>()?,
        },
        SemanticExpressionKind::Bytes { fixed_size, items } => ExecutableExpressionKind::Bytes {
            fixed_size: *fixed_size,
            items: items
                .iter()
                .map(|item| ids.expression(*item))
                .collect::<Result<Vec<_>, _>>()?,
        },
        SemanticExpressionKind::Delimiter => ExecutableExpressionKind::Delimiter,
        SemanticExpressionKind::Project { input, fields } => ExecutableExpressionKind::Project {
            input: ids.expression(*input)?,
            fields: fields.clone(),
        },
        SemanticExpressionKind::MaterializationLocal {
            owner,
            local,
            projection,
        } => ExecutableExpressionKind::MaterializationLocal {
            owner: *owner,
            local: ids.materialization_local(*owner, *local)?,
            projection: projection.clone(),
        },
        SemanticExpressionKind::FunctionParameter {
            parameter,
            projection,
        } => ExecutableExpressionKind::FunctionParameter {
            parameter: ids.parameter(*parameter)?,
            projection: projection.clone(),
        },
    })
}

fn map_provenance(
    graph: &SemanticExecutionGraphV1,
    ids: &SemanticToExecutableMap,
    provenance: &SemanticValueProvenance,
) -> Result<ExecutableValueProvenance, String> {
    Ok(ExecutableValueProvenance {
        members: provenance
            .members
            .iter()
            .map(|member| map_value_member(graph, ids, member))
            .collect::<Result<Vec<_>, _>>()?,
    })
}

fn map_value_member(
    graph: &SemanticExecutionGraphV1,
    ids: &SemanticToExecutableMap,
    member: &SemanticValueMember,
) -> Result<ExecutableValueMember, String> {
    Ok(ExecutableValueMember {
        path: member.path.clone(),
        origin: match &member.origin {
            SemanticValueOrigin::Runtime => ExecutableValueOrigin::Runtime,
            SemanticValueOrigin::Source { source, owner } => ExecutableValueOrigin::Source {
                source: checked_source_id(graph, *source)?,
                owner: *owner,
            },
            SemanticValueOrigin::ProducerSource {
                function,
                producer,
                identity,
                owner,
            } => {
                let executable_function = ids.callable(*function)?;
                let expected = ids.producer_function(*producer)?;
                if executable_function != expected {
                    return Err(format!(
                        "semantic producer source function {} does not map to producer {}",
                        function, producer
                    ));
                }
                ExecutableValueOrigin::ProducerSource {
                    function: executable_function,
                    identity: *identity,
                    owner: *owner,
                }
            }
            SemanticValueOrigin::State { state, owner } => ExecutableValueOrigin::State {
                state: checked_state_id(graph, *state)?,
                owner: *owner,
            },
            SemanticValueOrigin::MaterializationLocal {
                owner,
                local,
                projection,
            } => ExecutableValueOrigin::MaterializationLocal {
                owner: *owner,
                local: ids.materialization_local(*owner, *local)?,
                projection: projection.clone(),
            },
        },
    })
}

fn map_source_read(
    graph: &SemanticExecutionGraphV1,
    source: &SemanticSourceRead,
) -> Result<boon_typecheck::CheckedSourceRead, String> {
    Ok(boon_typecheck::CheckedSourceRead {
        source: checked_source_id(graph, source.source)?,
        payload_projection: source.payload_projection.clone(),
    })
}

fn checked_source_id(
    graph: &SemanticExecutionGraphV1,
    source: SemanticSourceId,
) -> Result<boon_typecheck::CheckedSourceId, String> {
    let definition = graph
        .sources
        .get(source.as_usize())
        .filter(|definition| definition.id == source)
        .ok_or_else(|| format!("missing semantic source {}", source))?;
    match definition.origin {
        SemanticSourceOrigin::Checked { source } => Ok(source),
        SemanticSourceOrigin::ProducerInvocation { .. } => Err(format!(
            "producer invocation semantic source {} cannot be represented as a checked source read",
            source
        )),
    }
}

fn checked_state_id(
    graph: &SemanticExecutionGraphV1,
    state: SemanticStateId,
) -> Result<boon_typecheck::CheckedStateId, String> {
    graph
        .states
        .get(state.as_usize())
        .filter(|definition| definition.id == state)
        .map(|definition| definition.checked_state)
        .ok_or_else(|| format!("missing semantic state {}", state))
}

fn map_text_segment(
    ids: &SemanticToExecutableMap,
    segment: &SemanticTextSegment,
) -> Result<ExecutableTextSegment, String> {
    Ok(match segment {
        SemanticTextSegment::Static { value } => ExecutableTextSegment::Static {
            value: value.clone(),
        },
        SemanticTextSegment::Dynamic { value } => ExecutableTextSegment::Dynamic {
            value: ids.expression(*value)?,
        },
    })
}

fn map_record_field(
    ids: &SemanticToExecutableMap,
    field: &SemanticRecordField,
) -> Result<ExecutableRecordField, String> {
    Ok(ExecutableRecordField {
        declaration: field.declaration,
        name: field.name.clone(),
        value: ids.expression(field.value)?,
        spread: field.spread,
    })
}

fn map_block_binding(
    ids: &SemanticToExecutableMap,
    binding: &SemanticBlockBinding,
) -> Result<ExecutableBlockBinding, String> {
    Ok(ExecutableBlockBinding {
        id: ids.local(binding.id)?,
        declaration: binding.declaration,
        value: ids.expression(binding.value)?,
    })
}

fn validate_external_value_identity(
    expression: SemanticExprId,
    canonical_path: &str,
    identity: Option<&boon_typecheck::CheckedExternalDeclarationIdentityV1>,
) -> Result<(), String> {
    if canonical_path.is_empty() {
        return Err(format!(
            "semantic external-read expression {expression} has an empty canonical path"
        ));
    }
    if identity.is_some_and(|identity| identity.kind != CheckedExternalDeclarationKind::Value) {
        return Err(format!(
            "semantic external-read expression {expression} carries a non-value external identity"
        ));
    }
    Ok(())
}

fn validate_call_expression(
    graph: &SemanticExecutionGraphV1,
    ids: &SemanticToExecutableMap,
    expression: &SemanticExpression,
) -> Result<(), String> {
    let SemanticExpressionKind::Call {
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
    } = &expression.kind
    else {
        return Err(format!(
            "semantic expression {} is not a call expression",
            expression.id
        ));
    };
    let call_definition = semantic_call(graph, *call)?;
    let callable_definition = semantic_callable(graph, *callable)?;
    ids.call_expression(*call, expression.id)?;
    ids.callable(*callable)?;
    let expected_kind = match callable_definition.kind {
        boon_typecheck::CheckedCallableKind::Builtin => SemanticCallableKind::Builtin,
        boon_typecheck::CheckedCallableKind::External => SemanticCallableKind::External,
        boon_typecheck::CheckedCallableKind::User => {
            return Err(format!(
                "semantic expression {} retains expanded user call {call}",
                expression.id
            ));
        }
    };
    if call_definition.callable != *callable
        || *callable_kind != expected_kind
        || name != &callable_definition.name
        || function != &call_definition.function
        || *role != call_definition.role
        || *effect != call_definition.effect
        || result != &call_definition.result
        || expression.checked_expr_id != call_definition.checked_expression
        || expression.effect != *effect
        || expression.flow_type != *result
    {
        return Err(format!(
            "semantic expression {} call contract differs from semantic call {call}: callable={callable:?}/{:?}, kind={callable_kind:?}/{expected_kind:?}, name={name:?}/{:?}, function={function:?}/{:?}, role={role:?}/{:?}, effect={effect:?}/{:?}, result={result:?}/{:?}, checked={:?}/{:?}, expression_effect={:?}, expression_flow={:?}",
            expression.id,
            call_definition.callable,
            callable_definition.name,
            call_definition.function,
            call_definition.role,
            call_definition.effect,
            call_definition.result,
            expression.checked_expr_id,
            call_definition.checked_expression,
            expression.effect,
            expression.flow_type,
        ));
    }
    if call_definition.external_identity != callable_definition.external_identity {
        return Err(format!(
            "semantic expression {} call {call} has stale external identity provenance",
            expression.id
        ));
    }
    if !matches!(
        call_definition.context_binding,
        boon_typecheck::CheckedContextBinding::None
    ) {
        return Err(format!(
            "semantic expression {} retains non-erased contextual PASS binding for call {call}",
            expression.id
        ));
    }

    validate_call_parameter_bindings(
        *instance,
        callable_definition,
        arguments,
        parameter_bindings,
    )?;
    validate_call_input_provenance(call_definition, arguments)?;

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
                    "semantic call {call} context {} uses call instance {} instead of {instance}",
                    context.ordinal, context.call_instance
                ));
            }
            Ok(context.ordinal)
        })
        .collect::<Result<Vec<_>, String>>()?;
    if actual_contexts != expected_contexts {
        return Err(format!(
            "semantic expression {} contexts differ from semantic call {call}",
            expression.id
        ));
    }
    Ok(())
}

fn validate_call_input_provenance(
    call: &SemanticCall,
    arguments: &[SemanticCallArgument],
) -> Result<(), String> {
    let inputs = call
        .entries
        .iter()
        .filter_map(|entry| match entry {
            SemanticCallEntry::Input {
                formal,
                ordinal,
                name,
                checked_value,
                value_flow_type,
                from_pipe,
                ..
            } => Some((
                *formal,
                *ordinal,
                name,
                *checked_value,
                value_flow_type,
                *from_pipe,
            )),
            SemanticCallEntry::FreshOut { .. } | SemanticCallEntry::ForwardOut { .. } => None,
        })
        .collect::<Vec<_>>();
    if inputs.len() != arguments.len() {
        return Err(format!(
            "semantic call {} has {} input provenance entries for {} concrete arguments",
            call.id,
            inputs.len(),
            arguments.len()
        ));
    }
    for argument in arguments {
        let matches = inputs
            .iter()
            .filter(
                |(formal, ordinal, name, checked_value, _flow_type, from_pipe)| {
                    *formal == argument.formal
                        && *ordinal == argument.ordinal
                        && *name == &argument.name
                        && *checked_value == argument.checked_value
                        && *from_pipe == argument.from_pipe
                },
            )
            .count();
        if matches != 1 {
            return Err(format!(
                "semantic call {} argument ordinal {} has {matches} exact call-table provenance entries",
                call.id, argument.ordinal
            ));
        }
    }
    Ok(())
}

fn validate_call_parameter_bindings(
    instance: OutCallInstanceId,
    callable: &SemanticCallable,
    arguments: &[SemanticCallArgument],
    bindings: &[SemanticCallParameterBinding],
) -> Result<(), String> {
    let value_parameters = callable
        .parameters
        .iter()
        .filter(|parameter| parameter.kind == CheckedParameterKind::Value)
        .collect::<Vec<_>>();
    if bindings.len() != value_parameters.len() {
        return Err(format!(
            "semantic call {instance} has {} parameter bindings for {} value parameters",
            bindings.len(),
            value_parameters.len()
        ));
    }
    let mut arguments_by_formal = BTreeMap::new();
    let mut argument_ordinals = BTreeSet::new();
    let mut previous_ordinal = None;
    for argument in arguments {
        if previous_ordinal.is_some_and(|previous| previous >= argument.ordinal) {
            return Err(format!(
                "semantic call {instance} arguments are not strictly ordered by ordinal"
            ));
        }
        previous_ordinal = Some(argument.ordinal);
        if arguments_by_formal
            .insert(argument.formal, argument)
            .is_some()
        {
            return Err(format!(
                "semantic call {instance} has duplicate argument formal {}",
                argument.formal.0
            ));
        }
        if !argument_ordinals.insert(argument.ordinal) {
            return Err(format!(
                "semantic call {instance} has duplicate argument ordinal {}",
                argument.ordinal
            ));
        }
    }

    let mut bindings_by_formal = BTreeMap::new();
    let mut binding_ordinals = BTreeSet::new();
    previous_ordinal = None;
    for (binding, parameter) in bindings.iter().zip(value_parameters) {
        if previous_ordinal.is_some_and(|previous| previous >= binding.ordinal) {
            return Err(format!(
                "semantic call {instance} parameter bindings are not strictly ordered by ordinal"
            ));
        }
        previous_ordinal = Some(binding.ordinal);
        if bindings_by_formal.insert(binding.formal, binding).is_some() {
            return Err(format!(
                "semantic call {instance} has duplicate parameter formal {}",
                binding.formal.0
            ));
        }
        if !binding_ordinals.insert(binding.ordinal) {
            return Err(format!(
                "semantic call {instance} has duplicate parameter ordinal {}",
                binding.ordinal
            ));
        }
        if binding.formal != parameter.formal
            || binding.ordinal != parameter.ordinal
            || binding.name != parameter.name
            || binding.requirement != parameter.requirement
        {
            return Err(format!(
                "semantic call {instance} parameter binding {} differs from callable {}",
                binding.ordinal, callable.id
            ));
        }
        // `SemanticExpressionKind::Call` carries value-parameter bindings only.
        // Calls with OUT formals must already be represented by `Materialize`;
        // any future OUT binding variant is deliberately an exhaustive-match
        // failure here until its executable erasure is defined.
        match &binding.kind {
            SemanticCallParameterBindingKind::Explicit {
                checked_value,
                value,
                from_pipe,
            } => {
                let argument = arguments_by_formal.get(&binding.formal).ok_or_else(|| {
                    format!(
                        "semantic call {instance} explicit parameter {} has no argument",
                        binding.formal.0
                    )
                })?;
                if argument.ordinal != binding.ordinal
                    || argument.name != binding.name
                    || argument.checked_value != *checked_value
                    || argument.value != *value
                    || argument.from_pipe != *from_pipe
                {
                    return Err(format!(
                        "semantic call {instance} explicit parameter {} differs from its argument",
                        binding.formal.0
                    ));
                }
            }
            SemanticCallParameterBindingKind::Omitted => {
                if !matches!(
                    binding.requirement,
                    CheckedParameterRequirement::Optional { .. }
                ) {
                    return Err(format!(
                        "semantic call {instance} omits required parameter {}",
                        binding.formal.0
                    ));
                }
                if arguments_by_formal.contains_key(&binding.formal) {
                    return Err(format!(
                        "semantic call {instance} omitted parameter {} still has an argument",
                        binding.formal.0
                    ));
                }
            }
        }
    }
    for formal in arguments_by_formal.keys() {
        if !bindings_by_formal.contains_key(formal) {
            return Err(format!(
                "semantic call {instance} argument formal {} has no parameter binding",
                formal.0
            ));
        }
    }
    Ok(())
}

fn map_call_argument(
    ids: &SemanticToExecutableMap,
    argument: &SemanticCallArgument,
) -> Result<ExecutableCallArgument, String> {
    Ok(ExecutableCallArgument {
        ordinal: argument.ordinal,
        name: argument.name.clone(),
        value: ids.expression(argument.value)?,
        from_pipe: argument.from_pipe,
    })
}

fn map_select_arm(
    ids: &SemanticToExecutableMap,
    arm: &SemanticSelectArm,
) -> Result<ExecutableSelectArm, String> {
    Ok(ExecutableSelectArm {
        pattern: arm.pattern.clone(),
        bindings: arm.bindings.iter().map(map_pattern_binding).collect(),
        output: ids.expression(arm.output)?,
    })
}

fn map_pattern_binding(binding: &SemanticPatternBinding) -> ExecutablePatternBinding {
    ExecutablePatternBinding {
        name: binding.name.clone(),
        projection: binding.projection.clone(),
    }
}

fn map_statement(
    ids: &SemanticToExecutableMap,
    statement: &SemanticStatement,
) -> Result<ExecutableStatement, String> {
    Ok(ExecutableStatement {
        id: ids.statement(statement.id)?,
        declaration: statement.declaration,
        flow_type: statement.flow_type.clone(),
        kind: match &statement.kind {
            SemanticStatementKind::Field { name, path } => ExecutableStatementKind::Field {
                name: name.clone(),
                path: path.clone(),
            },
            SemanticStatementKind::Source { name, path, event } => {
                ExecutableStatementKind::Source {
                    name: name.clone(),
                    path: path.clone(),
                    event: event.clone(),
                }
            }
            SemanticStatementKind::Hold {
                name,
                path,
                hold_name,
            } => ExecutableStatementKind::Hold {
                name: name.clone(),
                path: path.clone(),
                hold_name: hold_name.clone(),
            },
            SemanticStatementKind::List {
                name,
                path,
                capacity,
            } => ExecutableStatementKind::List {
                name: name.clone(),
                path: path.clone(),
                capacity: *capacity,
            },
            SemanticStatementKind::Block => ExecutableStatementKind::Block,
            SemanticStatementKind::Spread => ExecutableStatementKind::Spread,
            SemanticStatementKind::Expression => ExecutableStatementKind::Expression,
        },
        value: statement
            .value
            .map(|value| ids.expression(value))
            .transpose()?,
        value_use: map_result_kind(statement.value_use),
        children: statement
            .children
            .iter()
            .map(|child| ids.statement(*child))
            .collect::<Result<Vec<_>, _>>()?,
    })
}

fn map_source(
    ids: &SemanticToExecutableMap,
    source: &SemanticSourceDef,
) -> Result<ExecutableSourceDef, String> {
    Ok(ExecutableSourceDef {
        id: ids.source(source.id)?,
        origin: match source.origin {
            SemanticSourceOrigin::Checked { source } => ExecutableSourceOrigin::Checked { source },
            SemanticSourceOrigin::ProducerInvocation {
                function,
                producer,
                identity,
            } => {
                let executable_function = ids.callable(function)?;
                let expected = ids.producer_function(producer)?;
                if executable_function != expected {
                    return Err(format!(
                        "semantic source {} function {} does not map to producer {}",
                        source.id, function, producer
                    ));
                }
                ExecutableSourceOrigin::ProducerInvocation {
                    function: executable_function,
                    identity,
                }
            }
        },
        declaration: source.declaration,
        expression: ids.expression(source.expression)?,
        binding_path: source.binding_path.clone(),
        owner: source.owner,
    })
}

fn map_state(
    ids: &SemanticToExecutableMap,
    state: &SemanticStateDef,
) -> Result<ExecutableStateDef, String> {
    Ok(ExecutableStateDef {
        id: ids.state(state.id)?,
        checked_state: state.checked_state,
        declaration: state.declaration,
        expression: ids.expression(state.expression)?,
        initial: ids.expression(state.initial)?,
        binding_path: state.binding_path.clone(),
        owner: state.owner,
    })
}

fn map_root(
    graph: &SemanticExecutionGraphV1,
    ids: &SemanticToExecutableMap,
    root: &SemanticRoot,
) -> Result<ExecutableRoot, String> {
    let expression = semantic_expression(graph, root.expression)?;
    let executable = ids.expression(root.expression)?;
    if ids.value(root.value)? != executable {
        return Err(format!(
            "semantic root {} expression/value identity differs",
            root.ordinal
        ));
    }
    Ok(ExecutableRoot {
        checked_expr_id: expression.checked_expr_id,
        expression: executable,
    })
}

fn map_function(
    graph: &SemanticExecutionGraphV1,
    ids: &SemanticToExecutableMap,
    function: &SemanticFunction,
) -> Result<ExecutableFunction, String> {
    let callable = semantic_callable(graph, function.callable)?;
    if callable.kind != boon_typecheck::CheckedCallableKind::User {
        return Err(format!(
            "semantic producer {} references non-user callable {}",
            function.producer, function.callable
        ));
    }
    if callable.name != function.name {
        return Err(format!(
            "semantic producer {} name `{}` differs from callable {} name `{}`",
            function.producer, function.name, callable.id, callable.name
        ));
    }
    let id = ids.callable(function.callable)?;
    let expected = ids.producer_function(function.producer)?;
    if id != expected {
        return Err(format!(
            "semantic callable {} does not map to producer {}",
            function.callable, function.producer
        ));
    }
    for parameter in &function.parameters {
        let callable_parameter = callable
            .parameters
            .get(parameter.id.ordinal)
            .filter(|candidate| candidate.id == parameter.id)
            .ok_or_else(|| {
                format!(
                    "semantic producer {} parameter {:?} has no exact callable definition",
                    function.producer, parameter.id
                )
            })?;
        if parameter.id.callable != function.callable
            || parameter.formal != callable_parameter.formal
            || parameter.name != callable_parameter.name
            || parameter.flow_type != callable_parameter.flow_type
            || parameter.requirement != callable_parameter.requirement
        {
            return Err(format!(
                "semantic producer {} parameter {:?} differs from callable {}",
                function.producer, parameter.id, callable.id
            ));
        }
        for input in &parameter.input_expressions {
            let input = semantic_expression(graph, *input)?;
            if !matches!(
                &input.kind,
                SemanticExpressionKind::FunctionParameter {
                    parameter: candidate,
                    ..
                } if *candidate == parameter.id
            ) {
                return Err(format!(
                    "semantic producer {} parameter {:?} input {} is not an exact function-parameter expression",
                    function.producer, parameter.id, input.id
                ));
            }
        }
    }
    Ok(ExecutableFunction {
        id,
        identity: function.identity,
        name: function.name.clone(),
        parameters: function
            .parameters
            .iter()
            .map(|parameter| map_function_parameter(ids, parameter))
            .collect::<Result<Vec<_>, _>>()?,
        result_type: function.result_type.clone(),
        root: ids.expression(function.root)?,
        invocation_source: function
            .invocation_source
            .map(|source| ids.expression(source))
            .transpose()?,
    })
}

fn map_function_parameter(
    ids: &SemanticToExecutableMap,
    parameter: &SemanticFunctionParameter,
) -> Result<ExecutableFunctionParameter, String> {
    Ok(ExecutableFunctionParameter {
        id: ids.parameter(parameter.id)?,
        name: parameter.name.clone(),
        flow_type: parameter.flow_type.clone(),
    })
}

fn map_materialization(
    ids: &SemanticToExecutableMap,
    materialization: &SemanticContextualMaterialization,
) -> Result<ContextualMaterialization, String> {
    Ok(ContextualMaterialization {
        id: ids.materialization(materialization.id)?,
        operation: map_operation(materialization.operation),
        source: ids.expression(materialization.source)?,
        source_row_predecessors: materialization
            .source_row_predecessors
            .iter()
            .map(|predecessor| map_row_predecessor(ids, predecessor))
            .collect::<Result<Vec<_>, _>>()?,
        body: ids.expression(materialization.body)?,
        direction: materialization
            .direction
            .map(|direction| ids.expression(direction))
            .transpose()?,
        inherited_order: materialization
            .inherited_order
            .iter()
            .map(|order| map_order(ids, order))
            .collect::<Result<Vec<_>, _>>()?,
        result_kind: map_result_kind(materialization.result_kind),
        row_local: ids.materialization_local(materialization.owner, materialization.row_local)?,
        owner: materialization.owner,
        source_list_id: materialization
            .source_list_id
            .map(|id| ids.list(id))
            .transpose()?,
        source_scope_id: materialization
            .source_scope_id
            .map(|id| ids.row_scope(id))
            .transpose()?,
        target_list_id: materialization
            .target_list_id
            .map(|id| ids.list(id))
            .transpose()?,
        target_scope_id: materialization
            .target_scope_id
            .map(|id| ids.row_scope(id))
            .transpose()?,
        item_type: materialization.item_type.clone(),
        result_type: materialization.result_type.clone(),
    })
}

fn map_row_predecessor(
    ids: &SemanticToExecutableMap,
    predecessor: &SemanticContextualRowPredecessor,
) -> Result<ContextualRowPredecessor, String> {
    Ok(match predecessor {
        SemanticContextualRowPredecessor::Value => ContextualRowPredecessor::Value,
        SemanticContextualRowPredecessor::Stored { row } => ContextualRowPredecessor::Stored {
            row: map_row_binding(ids, *row)?,
        },
        SemanticContextualRowPredecessor::Materialized { materialization } => {
            ContextualRowPredecessor::Materialized {
                materialization: ids.materialization(*materialization)?,
            }
        }
        SemanticContextualRowPredecessor::Provenance { materialization } => {
            ContextualRowPredecessor::Provenance {
                materialization: ids.materialization(*materialization)?,
            }
        }
    })
}

fn map_row_binding(
    ids: &SemanticToExecutableMap,
    row: SemanticRowBinding,
) -> Result<crate::ErasedRowBinding, String> {
    Ok(crate::ErasedRowBinding {
        list: ids.list(row.list)?,
        scope: ids.row_scope(row.scope)?,
    })
}

fn map_order(
    ids: &SemanticToExecutableMap,
    order: &SemanticContextualOrderKey,
) -> Result<ContextualOrderKey, String> {
    Ok(ContextualOrderKey {
        operation: map_operation(order.operation),
        body: ids.expression(order.body)?,
        direction: ids.expression(order.direction)?,
    })
}

const fn map_operation(operation: SemanticContextualOperationKind) -> ContextualOperationKind {
    match operation {
        SemanticContextualOperationKind::Map => ContextualOperationKind::Map,
        SemanticContextualOperationKind::Filter => ContextualOperationKind::Filter,
        SemanticContextualOperationKind::Retain => ContextualOperationKind::Retain,
        SemanticContextualOperationKind::Remove => ContextualOperationKind::Remove,
        SemanticContextualOperationKind::Every => ContextualOperationKind::Every,
        SemanticContextualOperationKind::Any => ContextualOperationKind::Any,
        SemanticContextualOperationKind::Find => ContextualOperationKind::Find,
        SemanticContextualOperationKind::SortBy => ContextualOperationKind::SortBy,
        SemanticContextualOperationKind::ThenBy => ContextualOperationKind::ThenBy,
    }
}

const fn map_result_kind(kind: SemanticMaterializationResultKind) -> MaterializationResultKind {
    match kind {
        SemanticMaterializationResultKind::RuntimeValue => MaterializationResultKind::RuntimeValue,
        SemanticMaterializationResultKind::RenderSlot => MaterializationResultKind::RenderSlot,
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn finish_verified_semantic_lowering(
    execution_graph: &SemanticExecutionGraphV1,
    resource_graph: &SemanticResourceGraphV1,
    reactive_graph: &SemanticReactiveGraphV1,
    lowering_contract: &SemanticLoweringContractV1,
    view_binding_graph: &boon_semantic::SemanticViewBindingGraphV1,
    scope_storage_graph: &SemanticScopeStorageGraphV1,
    memory_graph: &boon_semantic::SemanticMemoryGraphV1,
    mapped: MappedSemanticExecution,
    resources: MappedSemanticResources,
) -> Result<crate::ErasedProgramFields, String> {
    let reactive = map_semantic_reactive(
        execution_graph,
        resource_graph,
        reactive_graph,
        &mapped.id_map,
        &resources,
    )?;
    let storage = map_semantic_storage_join(
        execution_graph,
        resource_graph,
        reactive_graph,
        scope_storage_graph,
        lowering_contract,
        &mapped.id_map,
        &resources,
        &reactive,
    )?;
    if mapped.static_owners.len() != storage.owners.len()
        || mapped
            .static_owners
            .iter()
            .zip(&storage.owners)
            .any(|(semantic, erased)| {
                (semantic.id, semantic.parent, semantic.child_ordinal)
                    != (erased.id, erased.parent, erased.child_ordinal)
            })
    {
        return Err(
            "semantic execution and storage mappings disagree on the exact static-owner forest"
                .to_owned(),
        );
    }
    let (mapped_role_references, external_value_references, external_call_references) =
        map_distributed_references(execution_graph, &mapped, &storage)?;
    let reads = finalize_storage_reads(&storage, &resources, &external_value_references)?;
    let dependency_uses = finalize_storage_dependency_uses(&storage, &external_call_references)?;
    let output_values = map_output_values(lowering_contract, &mapped.id_map, &storage.id_map)?;
    let host_ports = map_host_ports(lowering_contract);
    let view_bindings = map_view_bindings(
        view_binding_graph,
        &mapped.id_map,
        &storage.id_map,
        &storage,
    )?;
    let (semantic_memory, migration_edges) = map_semantic_memory(
        execution_graph,
        reactive_graph,
        memory_graph,
        &mapped.id_map,
        &storage.id_map,
    )?;
    let expression_types = map_expression_types(&lowering_contract.metadata);
    let function_types = map_function_types(&lowering_contract.metadata);
    let named_value_types = map_named_value_types(&lowering_contract.metadata);
    let expression_coverage =
        map_expression_coverage(&lowering_contract.metadata, &mapped_role_references);
    let semantic_index = map_semantic_index(
        execution_graph,
        lowering_contract,
        &mapped.id_map,
        &resources,
        &storage,
        &output_values,
        &view_bindings,
    )?;

    let MappedSemanticExecution {
        executable,
        materializations,
        ..
    } = mapped;
    let MappedSemanticResources {
        row_scopes,
        lists,
        sources,
        state_cells,
        list_projections,
        ..
    } = resources;
    let MappedSemanticStorage {
        owners,
        locals,
        fields,
        bindings,
        sources: storage_sources,
        row_values,
        row_source_projections,
        producer_function_instances,
        derived_values,
        state_update_arms: finalized_state_transitions,
        list_mutations,
        dependencies,
        possible_causes,
        ..
    } = storage;
    let graph_node_count = executable.expressions.len();

    Ok(crate::ErasedProgramFields {
        executable,
        scope_index: crate::ErasedScopeIndex {
            owners,
            locals,
            fields,
            bindings,
            sources: storage_sources,
            reads,
            row_values,
            row_source_projections,
            dependencies: dependency_uses,
        },
        expression_count: lowering_contract.metadata.original_source_expression_count,
        expression_coverage,
        distributed_references: mapped_role_references,
        producer_function_instances,
        semantic_index,
        graph_node_count,
        row_scopes,
        sources,
        host_ports,
        state_cells,
        lists,
        semantic_memory,
        migration_edges,
        output_values,
        derived_values,
        dependencies,
        possible_causes,
        state_update_arms: finalized_state_transitions,
        list_mutations,
        list_projections,
        materializations,
        view_bindings,
        expression_types,
        function_types,
        named_value_types,
        hidden_identity_verified: true,
        static_schedule_verified: true,
    })
}

type ExternalReferenceMap = BTreeMap<SemanticStorageExternalReferenceId, usize>;

fn map_distributed_references(
    execution: &SemanticExecutionGraphV1,
    mapped: &MappedSemanticExecution,
    storage: &MappedSemanticStorage,
) -> Result<
    (
        crate::DistributedReferences,
        ExternalReferenceMap,
        ExternalReferenceMap,
    ),
    String,
> {
    let mut value_references = Vec::new();
    let mut calls = Vec::new();
    let mut value_ids = BTreeMap::new();
    let mut call_ids = BTreeMap::new();

    for reference in &storage.external_references {
        let identity_role = reference
            .external_identity
            .map(|identity| identity.producer_role);
        let producer_role = identity_role
            .or_else(|| distributed_role_from_path(&reference.canonical_path))
            .ok_or_else(|| {
                format!(
                    "semantic external reference {} (`{}`) has neither a sealed producer role nor a canonical role namespace",
                    reference.id, reference.canonical_path
                )
            })?;
        match reference.kind {
            MappedSemanticExternalReferenceKind::Read {
                expression,
                semantic_read: _,
                read: _,
            } => {
                if reference
                    .external_identity
                    .is_some_and(|identity| identity.kind != CheckedExternalDeclarationKind::Value)
                {
                    return Err(format!(
                        "semantic external value reference {} carries a callable identity",
                        reference.id
                    ));
                }
                let executable = mapped
                    .executable
                    .expressions
                    .get(expression.as_usize())
                    .filter(|candidate| candidate.id == expression)
                    .ok_or_else(|| {
                        format!(
                            "semantic external value reference {} maps to missing executable expression {expression}",
                            reference.id
                        )
                    })?;
                if !matches!(
                    executable.kind,
                    ExecutableExpressionKind::ExternalRead { .. }
                ) {
                    return Err(format!(
                        "semantic external value reference {} maps to non-read executable expression {expression}",
                        reference.id
                    ));
                }
                let index = value_references.len();
                if value_ids.insert(reference.id, index).is_some() {
                    return Err(format!(
                        "semantic external value reference {} is mapped more than once",
                        reference.id
                    ));
                }
                value_references.push(crate::DistributedValueReference {
                    expr_id: ExprId(executable.checked_expr_id.0 as usize),
                    canonical_path: reference.canonical_path.clone(),
                    local_alias_paths: Vec::new(),
                    producer_role,
                    flow_mode: executable.flow_type.mode,
                    value_type: executable.flow_type.ty.clone(),
                });
            }
            MappedSemanticExternalReferenceKind::Call { call, expression } => {
                if reference.external_identity.is_some_and(|identity| {
                    identity.kind != CheckedExternalDeclarationKind::Callable
                }) {
                    return Err(format!(
                        "semantic external call reference {} carries a value identity",
                        reference.id
                    ));
                }
                let semantic_call = execution
                    .calls
                    .get(call.as_usize())
                    .filter(|candidate| candidate.id == call)
                    .ok_or_else(|| {
                        format!(
                            "semantic external call reference {} maps to missing call {call}",
                            reference.id
                        )
                    })?;
                let executable = mapped
                    .executable
                    .expressions
                    .get(expression.as_usize())
                    .filter(|candidate| candidate.id == expression)
                    .ok_or_else(|| {
                        format!(
                            "semantic external call reference {} maps to missing executable expression {expression}",
                            reference.id
                        )
                    })?;
                let ExecutableExpressionKind::Call {
                    callable_kind: ExecutableCallableKind::External,
                    name,
                    arguments,
                    ..
                } = &executable.kind
                else {
                    return Err(format!(
                        "semantic external call reference {} maps to non-external executable expression {expression}",
                        reference.id
                    ));
                };
                if name != &reference.canonical_path
                    || semantic_call.function != reference.canonical_path
                    || semantic_call.result != executable.flow_type
                {
                    return Err(format!(
                        "semantic external call reference {} differs from its exact executable call",
                        reference.id
                    ));
                }
                let arguments = arguments
                    .iter()
                    .map(|argument| {
                        let value = mapped
                            .executable
                            .expressions
                            .get(argument.value.as_usize())
                            .filter(|candidate| candidate.id == argument.value)
                            .ok_or_else(|| {
                                format!(
                                    "semantic external call {call} argument `{}` maps to missing executable value {}",
                                    argument.name, argument.value
                                )
                            })?;
                        Ok(crate::DistributedCallArgument {
                            name: argument.name.clone(),
                            value: argument.value,
                            flow_type: value.flow_type.clone(),
                        })
                    })
                    .collect::<Result<Vec<_>, String>>()?;
                let schedules = storage
                    .call_invocations
                    .iter()
                    .filter(|schedule| schedule.call == call && schedule.expression == expression)
                    .collect::<Vec<_>>();
                let [schedule] = schedules.as_slice() else {
                    return Err(format!(
                        "semantic external call {call} expression {expression} maps to {} final invocation schedules",
                        schedules.len()
                    ));
                };
                let index = calls.len();
                if call_ids.insert(reference.id, index).is_some() {
                    return Err(format!(
                        "semantic external call reference {} is mapped more than once",
                        reference.id
                    ));
                }
                calls.push(crate::DistributedCall {
                    expression,
                    owner: executable.owner,
                    occurrence_path: format!(
                        "semantic-call:{}/expression:{}",
                        call.as_usize(),
                        expression.as_usize()
                    ),
                    canonical_function: reference.canonical_path.clone(),
                    producer_role,
                    result: semantic_call.result.clone(),
                    effect: semantic_call.effect,
                    arguments,
                    invocation_arms: schedule.invocation_arms.clone(),
                });
            }
        }
    }

    if value_ids.len() + call_ids.len() != storage.external_references.len() {
        return Err(
            "semantic external references were not partitioned exactly into value and call domains"
                .to_owned(),
        );
    }
    Ok((
        crate::DistributedReferences {
            value_references,
            calls,
        },
        value_ids,
        call_ids,
    ))
}

fn distributed_role_from_path(path: &str) -> Option<boon_typecheck::ProgramRole> {
    let namespace = path.split('/').next()?;
    match namespace {
        "Client" => Some(boon_typecheck::ProgramRole::Client),
        "Session" => Some(boon_typecheck::ProgramRole::Session),
        "Server" => Some(boon_typecheck::ProgramRole::Server),
        _ => None,
    }
}

fn finalize_storage_reads(
    storage: &MappedSemanticStorage,
    resources: &MappedSemanticResources,
    external_values: &ExternalReferenceMap,
) -> Result<Vec<crate::ErasedReadBinding>, String> {
    storage
        .reads
        .iter()
        .map(|read| {
            let target = match &read.target {
                MappedSemanticStorageReadTarget::Binding {
                    binding,
                    projection,
                } => crate::ErasedReadTarget::Binding {
                    binding: *binding,
                    projection: projection.clone(),
                },
                MappedSemanticStorageReadTarget::SourcePayload {
                    binding,
                    source,
                    payload_projection,
                    projection: _,
                } => {
                    let source_port = resources
                        .sources
                        .get(source.as_usize())
                        .filter(|candidate| candidate.id == *source)
                        .ok_or_else(|| {
                            format!(
                                "mapped semantic read {} references missing source {source}",
                                read.id
                            )
                        })?;
                    let Some((field_name, projection)) = payload_projection.split_first() else {
                        return Ok(crate::ErasedReadBinding {
                            id: read.id,
                            expression: read.expression,
                            target: crate::ErasedReadTarget::Binding {
                                binding: *binding,
                                projection: Vec::new(),
                            },
                        });
                    };
                    let fields = source_port
                        .payload_schema
                        .fields
                        .iter()
                        .filter(|field| field.name() == field_name)
                        .collect::<Vec<_>>();
                    let [field] = fields.as_slice() else {
                        return Err(format!(
                            "mapped semantic source read {} payload field `{field_name}` resolves to {} exact source fields",
                            read.id,
                            fields.len()
                        ));
                    };
                    crate::ErasedReadTarget::SourcePayload {
                        binding: *binding,
                        source: *source,
                        field: (*field).clone(),
                        projection: projection.to_vec(),
                    }
                }
                MappedSemanticStorageReadTarget::StateProjection {
                    binding,
                    state,
                    projection,
                } => {
                    if projection.is_empty() {
                        crate::ErasedReadTarget::Binding {
                            binding: *binding,
                            projection: Vec::new(),
                        }
                    } else {
                        crate::ErasedReadTarget::StateProjection {
                            binding: *binding,
                            state: *state,
                            fields: projection.clone(),
                        }
                    }
                }
                MappedSemanticStorageReadTarget::Local {
                    binding,
                    declaration,
                    producer,
                    projection,
                } => crate::ErasedReadTarget::Local {
                    binding: *binding,
                    declaration: *declaration,
                    value: *producer,
                    projection: projection.clone(),
                },
                MappedSemanticStorageReadTarget::BundleExternal { reference } => {
                    let reference = external_values.get(reference).copied().ok_or_else(|| {
                        format!(
                            "mapped semantic read {} external identity {reference} has no value-reference allocation",
                            read.id
                        )
                    })?;
                    crate::ErasedReadTarget::ExternalValue { reference }
                }
                MappedSemanticStorageReadTarget::ElementState {
                    context,
                    projection,
                } => crate::ErasedReadTarget::ElementState {
                    context: *context,
                    projection: projection.clone(),
                },
                MappedSemanticStorageReadTarget::MaterializationLocal {
                    owner,
                    local,
                    projection,
                } => crate::ErasedReadTarget::MaterializationLocal {
                    owner: *owner,
                    local: *local,
                    projection: projection.clone(),
                },
                MappedSemanticStorageReadTarget::FunctionParameter {
                    parameter,
                    projection,
                } => crate::ErasedReadTarget::FunctionParameter {
                    parameter: *parameter,
                    projection: projection.clone(),
                },
            };
            Ok(crate::ErasedReadBinding {
                id: read.id,
                expression: read.expression,
                target,
            })
        })
        .collect()
}

fn finalize_storage_dependency_uses(
    storage: &MappedSemanticStorage,
    external_calls: &ExternalReferenceMap,
) -> Result<Vec<crate::ErasedDependencyUse>, String> {
    storage
        .dependency_uses
        .iter()
        .map(|dependency| {
            let target = match dependency.target {
                MappedSemanticStorageDependencyTarget::BundleExternalRead { read, .. } => {
                    crate::ErasedDependencyTarget::ExternalRead { read }
                }
                MappedSemanticStorageDependencyTarget::BundleExternalCall { reference } => {
                    let reference = external_calls.get(&reference).copied().ok_or_else(|| {
                        format!(
                            "mapped semantic dependency expression {} external identity {reference} has no call allocation",
                            dependency.expression
                        )
                    })?;
                    crate::ErasedDependencyTarget::ExternalCall { reference }
                }
            };
            Ok(crate::ErasedDependencyUse {
                dependent: dependency.dependent,
                expression: dependency.expression,
                target,
                timing: dependency.timing.clone(),
            })
        })
        .collect()
}

fn map_output_values(
    lowering: &SemanticLoweringContractV1,
    ids: &SemanticToExecutableMap,
    storage_ids: &SemanticStorageToErasedMap,
) -> Result<Vec<crate::OutputRootValue>, String> {
    lowering
        .output_contracts
        .iter()
        .enumerate()
        .map(|(index, output)| {
            if output.id.as_usize() != index || output.ordinal != index {
                return Err(format!(
                    "semantic output contract {} is not canonical at index {index}",
                    output.id
                ));
            }
            let contract = match output.contract {
                boon_semantic::SemanticOutputContractKindV1::RetainedVisualDocument => {
                    crate::SemanticOutputContractKind::RetainedVisual {
                        kind: crate::SemanticRetainedVisualKind::Document,
                    }
                }
                boon_semantic::SemanticOutputContractKindV1::RetainedVisualScene => {
                    crate::SemanticOutputContractKind::RetainedVisual {
                        kind: crate::SemanticRetainedVisualKind::Scene,
                    }
                }
                boon_semantic::SemanticOutputContractKindV1::HostValue => {
                    crate::SemanticOutputContractKind::HostValue
                }
            };
            let demand = match output.demand {
                boon_semantic::SemanticOutputDemandPolicyV1::HostDemanded => {
                    crate::SemanticOutputDemandPolicy::HostDemanded
                }
            };
            Ok(crate::OutputRootValue {
                root: output.root.clone(),
                value_path: output.value_path.clone(),
                contract,
                demand,
                data_type: output.data_type.as_ref().map(semantic_data_type),
                statement_id: output.checked_statement.0 as usize,
                executable_statement_id: ids.statement(output.statement)?,
                value_expression_id: ids.expression(output.expression)?,
                binding_id: storage_ids
                    .binding(MappedReactiveBindingId(output.binding.as_usize()))?,
                line: output.line,
                typed_contract_known: output.typed_contract_known,
            })
        })
        .collect()
}

fn map_host_ports(lowering: &SemanticLoweringContractV1) -> Vec<crate::HostPortDeclaration> {
    lowering
        .host_ports
        .iter()
        .map(|port| match &port.kind {
            boon_semantic::SemanticHostPortKindV1::HttpServer {
                request,
                disconnect,
                response,
            } => crate::HostPortDeclaration::HttpServer {
                line: port.line,
                request_source: request.diagnostic_path.clone(),
                disconnect_source: disconnect
                    .as_ref()
                    .map(|binding| binding.diagnostic_path.clone()),
                response_output: response.diagnostic_name.clone(),
            },
            boon_semantic::SemanticHostPortKindV1::WebSocketServer {
                open,
                message,
                close,
                error,
                actions,
            } => crate::HostPortDeclaration::WebSocketServer {
                line: port.line,
                open_source: open.diagnostic_path.clone(),
                message_source: message.diagnostic_path.clone(),
                close_source: close.diagnostic_path.clone(),
                error_source: error.diagnostic_path.clone(),
                actions_output: actions.diagnostic_name.clone(),
            },
        })
        .collect()
}

fn map_view_bindings(
    graph: &boon_semantic::SemanticViewBindingGraphV1,
    ids: &SemanticToExecutableMap,
    storage_ids: &SemanticStorageToErasedMap,
    storage: &MappedSemanticStorage,
) -> Result<Vec<crate::ViewBinding>, String> {
    graph
        .bindings
        .iter()
        .enumerate()
        .map(|(index, binding)| {
            if binding.id.as_usize() != index {
                return Err(format!(
                    "semantic view binding {} is not canonical at index {index}",
                    binding.id
                ));
            }
            let node = graph
                .nodes
                .get(binding.node.as_usize())
                .filter(|candidate| candidate.id == binding.node)
                .ok_or_else(|| {
                    format!(
                        "semantic view binding {} references missing node {}",
                        binding.id, binding.node
                    )
                })?;
            graph
                .arguments
                .get(binding.argument.as_usize())
                .filter(|candidate| candidate.id == binding.argument)
                .ok_or_else(|| {
                    format!(
                        "semantic view binding {} references missing argument {}",
                        binding.id, binding.argument
                    )
                })?;
            let path = match binding.target {
                boon_semantic::SemanticViewBindingTargetV1::Data { read } => mapped_view_read_path(
                    storage,
                    storage_ids.read(MappedReactiveReadId(read.as_usize()))?,
                )
                .unwrap_or_else(|| binding.diagnostic_path.clone()),
                boon_semantic::SemanticViewBindingTargetV1::Event { .. } => {
                    binding.diagnostic_path.clone()
                }
            };
            let target = match binding.target {
                boon_semantic::SemanticViewBindingTargetV1::Data { read } => {
                    crate::ViewBindingTarget::Read {
                        read: storage_ids.read(MappedReactiveReadId(read.as_usize()))?,
                        additional_projection: binding.additional_projection.clone(),
                    }
                }
                boon_semantic::SemanticViewBindingTargetV1::Event { source } => {
                    crate::ViewBindingTarget::Source {
                        source: ids.runtime_source(source)?,
                    }
                }
            };
            let kind = match binding.kind {
                boon_semantic::SemanticViewBindingKindV1::Data => crate::ViewBindingKind::Data,
                boon_semantic::SemanticViewBindingKindV1::Source => crate::ViewBindingKind::Source,
                boon_semantic::SemanticViewBindingKindV1::Target => crate::ViewBindingKind::Target,
            };
            Ok(crate::ViewBinding {
                id: crate::ViewBindingId(index),
                node_kind: node.diagnostic_kind.clone(),
                attr: binding.canonical_attribute.clone(),
                path,
                target,
                kind,
                scope_id: binding
                    .row
                    .map(|row| ids.row_scope(row.scope))
                    .transpose()?,
            })
        })
        .collect()
}

fn mapped_view_read_path(storage: &MappedSemanticStorage, read: ErasedReadId) -> Option<String> {
    let read = storage
        .reads
        .get(read.as_usize())
        .filter(|candidate| candidate.id == read)?;
    let binding_path = |binding: ErasedBindingId, projection: &[String]| {
        let binding = storage
            .bindings
            .get(binding.as_usize())
            .filter(|candidate| candidate.id == binding)?;
        Some(join_diagnostic_projection(
            &binding.diagnostic_path,
            projection,
        ))
    };
    match &read.target {
        MappedSemanticStorageReadTarget::Binding {
            binding,
            projection,
        } => binding_path(*binding, projection),
        MappedSemanticStorageReadTarget::SourcePayload {
            binding,
            payload_projection,
            projection,
            ..
        } => {
            let combined = payload_projection
                .iter()
                .chain(projection)
                .cloned()
                .collect::<Vec<_>>();
            binding_path(*binding, &combined)
        }
        MappedSemanticStorageReadTarget::StateProjection {
            binding,
            projection,
            ..
        } => binding_path(*binding, projection),
        MappedSemanticStorageReadTarget::Local {
            declaration,
            producer,
            projection,
            ..
        } => {
            let fields = storage
                .fields
                .iter()
                .filter(|field| {
                    field.declaration == Some(*declaration) && field.producer == Some(*producer)
                })
                .collect::<Vec<_>>();
            let [field] = fields.as_slice() else {
                return None;
            };
            Some(join_diagnostic_projection(
                &field.diagnostic_path,
                projection,
            ))
        }
        MappedSemanticStorageReadTarget::BundleExternal { reference } => storage
            .external_references
            .get(reference.as_usize())
            .filter(|candidate| candidate.id == *reference)
            .map(|reference| reference.canonical_path.clone()),
        MappedSemanticStorageReadTarget::ElementState { projection, .. } => {
            Some(join_diagnostic_projection("element_state", projection))
        }
        MappedSemanticStorageReadTarget::MaterializationLocal { .. }
        | MappedSemanticStorageReadTarget::FunctionParameter { .. } => None,
    }
}

fn join_diagnostic_projection(base: &str, projection: &[String]) -> String {
    if projection.is_empty() {
        base.to_owned()
    } else if base.is_empty() {
        projection.join(".")
    } else {
        format!("{base}.{}", projection.join("."))
    }
}

fn map_expression_types(
    metadata: &boon_semantic::SemanticLoweringMetadataV1,
) -> boon_typecheck::ExprTypeTable {
    boon_typecheck::ExprTypeTable {
        entries: metadata
            .expression_types
            .iter()
            .map(|entry| boon_typecheck::ExprTypeEntry {
                expr_id: entry.checked_expression.0 as usize,
                flow_type: entry.flow_type.clone(),
            })
            .collect(),
    }
}

fn map_function_types(
    metadata: &boon_semantic::SemanticLoweringMetadataV1,
) -> boon_typecheck::FunctionTypeTable {
    boon_typecheck::FunctionTypeTable {
        entries: metadata
            .function_types
            .iter()
            .map(|entry| boon_typecheck::FunctionTypeEntry {
                callable: entry.checked_callable,
                name: entry.name.clone(),
                parameters: entry
                    .parameters
                    .iter()
                    .map(|parameter| boon_typecheck::FunctionTypeParameterEntry {
                        formal: parameter.formal,
                        ordinal: parameter.ordinal,
                        name: parameter.name.clone(),
                        flow_type: parameter.flow_type.clone(),
                    })
                    .collect(),
                result: entry.result.clone(),
                effect: entry.effect,
            })
            .collect(),
    }
}

fn map_named_value_types(
    metadata: &boon_semantic::SemanticLoweringMetadataV1,
) -> boon_typecheck::NamedValueTypeTable {
    boon_typecheck::NamedValueTypeTable {
        checked_statement_sites: metadata
            .named_value_types
            .iter()
            .map(|entry| entry.checked_statement)
            .collect(),
        entries: metadata
            .named_value_types
            .iter()
            .map(|entry| boon_typecheck::NamedValueTypeEntry {
                path: entry.diagnostic_path.clone(),
                origins: entry
                    .origins
                    .iter()
                    .map(|origin| origin.checked.clone())
                    .collect(),
                flow_type: entry.flow_type.clone(),
            })
            .collect(),
    }
}

fn map_expression_coverage(
    metadata: &boon_semantic::SemanticLoweringMetadataV1,
    distributed: &crate::DistributedReferences,
) -> crate::ExpressionCoverage {
    crate::ExpressionCoverage {
        computed_from: "verified_semantic_program".to_owned(),
        ast_expression_count: metadata.original_source_expression_count,
        distributed_reference_expression_count: distributed.value_references.len()
            + distributed.calls.len(),
        unknown_ast_expression_count: 0,
        ignored_unknown_ast_expression_count: 0,
        unknown_list_initializer_count: 0,
        unknown_list_initial_value_count: 0,
        unknown_list_predicate_count: 0,
        unknown_derived_value_count: 0,
        unknown_labels: Vec::new(),
        ignored_unknown_labels: Vec::new(),
    }
}

#[allow(clippy::too_many_arguments)]
fn map_semantic_index(
    execution: &SemanticExecutionGraphV1,
    lowering: &SemanticLoweringContractV1,
    ids: &SemanticToExecutableMap,
    resources: &MappedSemanticResources,
    storage: &MappedSemanticStorage,
    output_values: &[crate::OutputRootValue],
    view_bindings: &[crate::ViewBinding],
) -> Result<crate::SemanticIndex, String> {
    let metadata = &lowering.metadata;
    let source_units = metadata
        .source_units
        .iter()
        .enumerate()
        .map(|(index, unit)| {
            if unit.id.as_usize() != index {
                return Err(format!(
                    "semantic source unit {} is not canonical at index {index}",
                    unit.id
                ));
            }
            Ok(crate::SemanticSourceUnit {
                id: crate::SourceUnitId(index),
                path: unit.path.clone(),
                module: unit.module.clone(),
                start_line: unit.start_line,
                line_count: unit.line_count,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let output_roots = output_values
        .iter()
        .map(|output| crate::SemanticOutputRootEntry {
            root: output.root.clone(),
            contract: output.contract,
            demand: output.demand,
            data_type: output.data_type.clone(),
            statement_id: output.statement_id,
            line: output.line,
            typed_contract_known: output.typed_contract_known,
        })
        .collect::<Vec<_>>();
    let payload_sources = metadata
        .source_payload_shapes
        .iter()
        .flat_map(|shape| shape.sources.iter().copied())
        .collect::<BTreeSet<_>>();
    let sources = resources
        .sources
        .iter()
        .enumerate()
        .map(|(index, source)| crate::SemanticSourceEntry {
            id: source.id,
            path: source.path.clone(),
            scoped: source.scoped,
            scope_id: source.scope_id,
            payload_schema_known: payload_sources.contains(&SemanticSourceId(index)),
            payload_field_count: source.payload_schema.fields.len(),
        })
        .collect::<Vec<_>>();
    let lists = resources
        .lists
        .iter()
        .map(|list| crate::SemanticListEntry {
            id: list.id,
            name: list.name.clone(),
            row_scope_id: list.row_scope_id,
            capacity: list.capacity,
            initializer_known: !matches!(list.initializer, ListInitializer::Unknown { .. }),
        })
        .collect::<Vec<_>>();
    let row_scopes = resources
        .row_scopes
        .iter()
        .map(|scope| crate::SemanticRowScopeEntry {
            id: scope.id,
            list: scope.list.clone(),
            function: scope.function.clone(),
            row_scope: scope.row_scope.clone(),
        })
        .collect::<Vec<_>>();
    let functions = metadata
        .function_types
        .iter()
        .map(|function| {
            let callable = execution
                .callables
                .get(function.callable.as_usize())
                .filter(|candidate| candidate.id == function.callable)
                .ok_or_else(|| {
                    format!(
                        "semantic function type for `{}` references missing callable {}",
                        function.name, function.callable
                    )
                })?;
            let statement = callable.body.and_then(|body| {
                execution.statements.iter().find(|statement| {
                    matches!(
                        statement.origin,
                        boon_semantic::SemanticStatementOrigin::Checked { statement }
                            if statement == body
                    )
                })
            });
            Ok(crate::SemanticFunctionEntry {
                id: ids.callable(function.callable)?,
                name: function.name.clone(),
                args: function
                    .parameters
                    .iter()
                    .map(|parameter| parameter.name.clone())
                    .collect(),
                statement_id: callable
                    .body
                    .map_or(usize::MAX, |statement| statement.0 as usize),
                line: statement.map_or(0, |statement| statement.span.line),
                type_known: true,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let fields = map_semantic_field_entries(
        &storage.fields,
        &storage.derived_values,
        &resources.state_cells,
    );
    let semantic_view_bindings = view_bindings
        .iter()
        .map(|binding| crate::SemanticViewBindingEntry {
            id: binding.id,
            node_kind: binding.node_kind.clone(),
            attr: binding.attr.clone(),
            path: binding.path.clone(),
            kind: binding.kind,
            scope_id: binding.scope_id,
            source_id: match binding.target {
                crate::ViewBindingTarget::Source { source } => Some(source),
                crate::ViewBindingTarget::Read { .. } => None,
            },
            render_contract_known: true,
        })
        .collect::<Vec<_>>();
    let diagnostic_spans = metadata
        .diagnostics
        .iter()
        .enumerate()
        .map(|(index, diagnostic)| crate::SemanticDiagnosticSpan {
            id: crate::DiagnosticSpanId(index),
            line: diagnostic.line,
            start: diagnostic.start,
            end: diagnostic.end,
            severity: format!("{:?}", diagnostic.severity).to_ascii_lowercase(),
            message: diagnostic.message.clone(),
        })
        .collect::<Vec<_>>();
    let symbols = map_semantic_symbols(
        execution,
        &source_units,
        &output_roots,
        &sources,
        &lists,
        &row_scopes,
        &functions,
        &fields,
        &semantic_view_bindings,
    );
    let readiness = map_semantic_index_readiness(metadata, &sources, &lists, &row_scopes);

    Ok(crate::SemanticIndex {
        version: 1,
        computed_from: "verified_semantic_program".to_owned(),
        parser_policy_phase: "verified_semantics_only".to_owned(),
        reuse_key: metadata.digest.to_string(),
        output_roots,
        source_units,
        sources,
        lists,
        row_scopes,
        functions,
        fields,
        view_bindings: semantic_view_bindings,
        diagnostic_spans,
        symbols,
        readiness,
        reuse: crate::SemanticIndexReuse {
            parser_reused_by_ir: false,
            typecheck_reused_by_ir: false,
            runtime_reports_reuse_index: true,
            shared_tables: vec![
                "SemanticLoweringMetadataV1".to_owned(),
                "SemanticResourceGraphV1".to_owned(),
                "SemanticScopeStorageGraphV1".to_owned(),
                "SemanticViewBindingGraphV1".to_owned(),
            ],
        },
    })
}

fn map_semantic_field_entries(
    fields: &[ErasedFieldDef],
    derived_values: &[DerivedValue],
    state_cells: &[StateCell],
) -> Vec<crate::SemanticFieldEntry> {
    fields
        .iter()
        .filter(|field| field.role.is_value())
        .map(|field| crate::SemanticFieldEntry {
            id: field.id,
            path: field.diagnostic_path.clone(),
            local_name: field.name.clone(),
            parent_path: field
                .parent
                .and_then(|parent| fields.get(parent.as_usize()))
                .map_or_else(String::new, |parent| parent.diagnostic_path.clone()),
            scope_id: field.row.map(|row| row.scope),
            statement_id: field.statement.map_or(usize::MAX, |statement| statement.0),
            line: field
                .statement
                .and_then(|statement| {
                    state_cells
                        .iter()
                        .find(|state| state.statement_id == statement.as_usize())
                })
                .map_or(0, |state| state.source_line),
            kind: if state_cells.iter().any(|state| {
                field
                    .statement
                    .is_some_and(|statement| state.statement_id == statement.as_usize())
            }) {
                "state".to_owned()
            } else if let Some(derived) =
                derived_values.iter().find(|derived| derived.id == field.id)
            {
                match derived.kind {
                    DerivedValueKind::SourceEventTransform => "source_event_transform",
                    DerivedValueKind::ListView => "list_view",
                    DerivedValueKind::Aggregate => "aggregate",
                    DerivedValueKind::Pure => "pure",
                    DerivedValueKind::Unknown => "unknown",
                }
                .to_owned()
            } else if field.row.is_some() {
                "materialized_field".to_owned()
            } else {
                "field".to_owned()
            },
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn map_semantic_symbols(
    execution: &SemanticExecutionGraphV1,
    source_units: &[crate::SemanticSourceUnit],
    output_roots: &[crate::SemanticOutputRootEntry],
    sources: &[crate::SemanticSourceEntry],
    lists: &[crate::SemanticListEntry],
    row_scopes: &[crate::SemanticRowScopeEntry],
    functions: &[crate::SemanticFunctionEntry],
    fields: &[crate::SemanticFieldEntry],
    view_bindings: &[crate::SemanticViewBindingEntry],
) -> Vec<crate::SemanticSymbolEntry> {
    let mut symbols = BTreeSet::<(String, String)>::new();
    let mut insert = |category: &str, text: &str| {
        if !text.is_empty() {
            symbols.insert((category.to_owned(), text.to_owned()));
        }
    };
    for unit in source_units {
        insert("source_unit_path", &unit.path);
        if let Some(module) = &unit.module {
            insert("module_path", module);
        }
    }
    for output in output_roots {
        insert("output_root", &output.root);
        insert("output_kind", output.contract.as_str());
    }
    for source in sources {
        insert("source_label", &source.path);
        for segment in source.path.split('.') {
            insert("source_label_segment", segment);
        }
    }
    for list in lists {
        insert("list_name", &list.name);
    }
    for scope in row_scopes {
        insert("row_scope", &scope.row_scope);
        insert("row_scope_function", &scope.function);
    }
    for function in functions {
        insert("function_name", &function.name);
        for argument in &function.args {
            insert("function_arg", argument);
        }
    }
    for field in fields {
        insert("field_path", &field.path);
        insert("field_name", &field.local_name);
    }
    for expression in &execution.expressions {
        match &expression.kind {
            SemanticExpressionKind::Tag(name) => insert("tag", name),
            SemanticExpressionKind::TaggedObject { tag, fields } => {
                insert("tag", tag);
                for field in fields {
                    insert("document_attr", &field.name);
                }
            }
            SemanticExpressionKind::Object(fields) => {
                for field in fields {
                    insert("document_attr", &field.name);
                    insert("style_attr", &field.name);
                }
            }
            _ => {}
        }
    }
    for call in &execution.calls {
        insert("operator_name", &call.function);
        for entry in &call.entries {
            let name = match entry {
                SemanticCallEntry::Input { name, .. }
                | SemanticCallEntry::FreshOut { name, .. }
                | SemanticCallEntry::ForwardOut { name, .. } => name,
            };
            insert("document_attr", name);
        }
    }
    for binding in view_bindings {
        insert("document_attr", &binding.attr);
        insert("view_node_kind", &binding.node_kind);
    }
    symbols
        .into_iter()
        .enumerate()
        .map(|(index, (category, text))| crate::SemanticSymbolEntry {
            id: crate::SemanticSymbolId(index),
            category,
            text,
        })
        .collect()
}

fn map_semantic_index_readiness(
    metadata: &boon_semantic::SemanticLoweringMetadataV1,
    sources: &[crate::SemanticSourceEntry],
    lists: &[crate::SemanticListEntry],
    row_scopes: &[crate::SemanticRowScopeEntry],
) -> crate::SemanticIndexReadiness {
    let source_fallbacks = sources
        .iter()
        .filter(|source| !source.payload_schema_known)
        .map(|source| format!("{} has no semantic payload shape", source.path))
        .collect::<Vec<_>>();
    let row_fallbacks = if !lists.is_empty() && row_scopes.is_empty() {
        vec!["semantic lists exist without row scopes".to_owned()]
    } else {
        Vec::new()
    };
    let render_fallbacks = metadata
        .render_slots
        .iter()
        .filter(|slot| !slot.diagnostics.is_empty())
        .map(|slot| {
            format!(
                "render slot `{}` has {} diagnostics",
                slot.slot_name,
                slot.diagnostics.len()
            )
        })
        .collect::<Vec<_>>();
    let route_fallbacks = (metadata.dynamic_fallback_count > 0)
        .then(|| {
            format!(
                "{} semantic expressions retain dynamic fallback",
                metadata.dynamic_fallback_count
            )
        })
        .into_iter()
        .collect::<Vec<_>>();
    let known = |known_count| crate::SemanticKnowledgeStatus {
        known_count,
        fallback_count: 0,
        fallback_reasons: Vec::new(),
    };
    crate::SemanticIndexReadiness {
        source_payload_schemas: crate::SemanticKnowledgeStatus {
            known_count: sources.len().saturating_sub(source_fallbacks.len()),
            fallback_count: source_fallbacks.len(),
            fallback_reasons: source_fallbacks,
        },
        source_completions: known(sources.len()),
        route_critical_unknowns: crate::SemanticKnowledgeStatus {
            known_count: metadata.checked_expression_count,
            fallback_count: route_fallbacks.len(),
            fallback_reasons: route_fallbacks,
        },
        row_scopes: crate::SemanticKnowledgeStatus {
            known_count: row_scopes.len(),
            fallback_count: row_fallbacks.len(),
            fallback_reasons: row_fallbacks,
        },
        row_scope_ambiguity: known(row_scopes.len()),
        selectors: known(lists.len()),
        selector_index_ambiguity: known(lists.len()),
        render_contracts: crate::SemanticKnowledgeStatus {
            known_count: metadata
                .render_slots
                .len()
                .saturating_sub(render_fallbacks.len()),
            fallback_count: render_fallbacks.len(),
            fallback_reasons: render_fallbacks,
        },
        bridge_page_descriptors: known(0),
        dynamic_fallback_count: metadata.dynamic_fallback_count,
    }
}

fn map_semantic_memory(
    execution: &SemanticExecutionGraphV1,
    reactive: &SemanticReactiveGraphV1,
    graph: &boon_semantic::SemanticMemoryGraphV1,
    ids: &SemanticToExecutableMap,
    storage_ids: &SemanticStorageToErasedMap,
) -> Result<(Vec<crate::SemanticMemory>, Vec<crate::MigrationEdge>), String> {
    let memories = graph
        .memories
        .iter()
        .enumerate()
        .map(|(index, memory)| {
            if memory.id.as_usize() != index {
                return Err(format!(
                    "semantic memory {} is not canonical at index {index}",
                    memory.id
                ));
            }
            let kind = map_semantic_memory_kind(memory.identity.kind);
            let runtime_backing = match memory.backing {
                boon_semantic::SemanticMemoryBackingV1::State {
                    storage_field,
                    state,
                    row,
                    ..
                } => {
                    let state_id = ids.runtime_state(state)?;
                    let field_id = Some(storage_ids.storage_field(storage_field)?);
                    match (kind, row) {
                        (crate::SemanticMemoryKind::RootScalar, None) => {
                            crate::SemanticMemoryRuntimeBacking::RootState { state_id, field_id }
                        }
                        (crate::SemanticMemoryKind::IndexedField, Some(row)) => {
                            let row = map_row_binding(ids, row)?;
                            crate::SemanticMemoryRuntimeBacking::IndexedState {
                                state_id,
                                field_id,
                                scope_id: row.scope,
                                list_id: Some(row.list),
                            }
                        }
                        _ => {
                            return Err(format!(
                                "semantic memory {} kind {:?} has incompatible state row {:?}",
                                memory.id, memory.identity.kind, row
                            ));
                        }
                    }
                }
                boon_semantic::SemanticMemoryBackingV1::List {
                    storage_field,
                    list,
                    row,
                    ..
                } => {
                    if kind != crate::SemanticMemoryKind::ListOwner {
                        return Err(format!(
                            "semantic memory {} has list backing for non-list kind {:?}",
                            memory.id, memory.identity.kind
                        ));
                    }
                    storage_ids.storage_field(storage_field)?;
                    let row = map_row_binding(ids, row)?;
                    let list_id = ids.list(list)?;
                    if row.list != list_id {
                        return Err(format!(
                            "semantic memory {} list backing differs from its row identity",
                            memory.id
                        ));
                    }
                    crate::SemanticMemoryRuntimeBacking::List {
                        list_id,
                        row_scope_id: Some(row.scope),
                    }
                }
            };
            let status = match memory.status {
                boon_semantic::SemanticMemoryStatusV1::Active => {
                    crate::SemanticMemoryStatus::Active
                }
                boon_semantic::SemanticMemoryStatusV1::Draining { marker } => {
                    let migration = reactive
                        .migration_inputs
                        .get(marker.as_usize())
                        .filter(|candidate| candidate.id == marker)
                        .ok_or_else(|| {
                            format!(
                                "semantic memory {} references missing migration marker {marker}",
                                memory.id
                            )
                        })?;
                    let marker = semantic_execution_expression(execution, migration.marker)?;
                    crate::SemanticMemoryStatus::Draining {
                        marker_expr_id: ExprId(marker.checked_expr_id.0 as usize),
                    }
                }
            };
            Ok(crate::SemanticMemory {
                id: crate::SemanticMemoryId(index),
                identity: crate::SemanticMemoryIdentity {
                    canonical_module: memory.identity.canonical_module.clone(),
                    owner_path: memory.identity.owner_path.clone(),
                    semantic_path: memory.identity.semantic_path.clone(),
                    kind,
                },
                data_type: semantic_data_type(&memory.data_type),
                leaves: memory
                    .leaves
                    .iter()
                    .map(|leaf| crate::SemanticMemoryLeaf {
                        semantic_path: semantic_region_path(
                            &memory.identity.semantic_path,
                            &leaf.projection,
                        ),
                        data_type: semantic_data_type(&leaf.data_type),
                    })
                    .collect(),
                status,
                runtime_backing,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;

    let migration_edges = graph
        .migration_edges
        .iter()
        .enumerate()
        .map(|(index, edge)| {
            if edge.id.as_usize() != index {
                return Err(format!(
                    "semantic migration edge {} is not canonical at index {index}",
                    edge.id
                ));
            }
            let source_leaves = edge
                .inputs
                .iter()
                .map(|input| {
                    let memory = semantic_memory_record(graph, input.source.memory)?;
                    let data_type =
                        semantic_region_type(&memory.data_type, &input.source.projection)?;
                    let expression = semantic_execution_expression(execution, input.expression)?;
                    Ok(crate::MigrationSourceLeaf {
                        memory_id: crate::SemanticMemoryId(input.source.memory.as_usize()),
                        semantic_path: semantic_region_path(
                            &memory.identity.semantic_path,
                            &input.source.projection,
                        ),
                        data_type: semantic_data_type(data_type),
                        drain_expr_id: ExprId(expression.checked_expr_id.0 as usize),
                    })
                })
                .collect::<Result<Vec<_>, String>>()?;
            let destination_memory = semantic_memory_record(graph, edge.destination.memory)?;
            let destination_type =
                semantic_region_type(&destination_memory.data_type, &edge.destination.projection)?;
            let transfer_kind = match edge.transfer {
                boon_semantic::SemanticMigrationTransferV1::Scalar => {
                    crate::MigrationTransferKind::Scalar
                }
                boon_semantic::SemanticMigrationTransferV1::IndexedField { owner } => {
                    map_row_binding(ids, owner)?;
                    crate::MigrationTransferKind::IndexedField
                }
                boon_semantic::SemanticMigrationTransferV1::List {
                    source,
                    destination,
                } => {
                    map_row_binding(ids, source)?;
                    map_row_binding(ids, destination)?;
                    crate::MigrationTransferKind::List
                }
            };
            let transform = match edge.transform {
                boon_semantic::SemanticMigrationTransformV1::Identity { input } => {
                    semantic_execution_expression(execution, input)?;
                    crate::MigrationTransform::Identity
                }
                boon_semantic::SemanticMigrationTransformV1::PureExpression { root } => {
                    let root = semantic_execution_expression(execution, root)?;
                    crate::MigrationTransform::PureExpression {
                        expression_root: ExprId(root.checked_expr_id.0 as usize),
                        pipeline: Vec::new(),
                    }
                }
            };
            Ok(crate::MigrationEdge {
                source_leaves,
                destination: crate::MigrationDestination {
                    memory_id: crate::SemanticMemoryId(edge.destination.memory.as_usize()),
                    semantic_path: semantic_region_path(
                        &destination_memory.identity.semantic_path,
                        &edge.destination.projection,
                    ),
                    data_type: semantic_data_type(destination_type),
                },
                transfer_kind,
                transform,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok((memories, migration_edges))
}

const fn map_semantic_memory_kind(
    kind: boon_semantic::SemanticMemoryKindV1,
) -> crate::SemanticMemoryKind {
    match kind {
        boon_semantic::SemanticMemoryKindV1::RootScalar => crate::SemanticMemoryKind::RootScalar,
        boon_semantic::SemanticMemoryKindV1::IndexedField => {
            crate::SemanticMemoryKind::IndexedField
        }
        boon_semantic::SemanticMemoryKindV1::ListOwner => crate::SemanticMemoryKind::ListOwner,
    }
}

fn semantic_memory_record(
    graph: &boon_semantic::SemanticMemoryGraphV1,
    id: boon_semantic::SemanticMemoryId,
) -> Result<&boon_semantic::SemanticMemoryV1, String> {
    graph
        .memories
        .get(id.as_usize())
        .filter(|candidate| candidate.id == id)
        .ok_or_else(|| format!("missing semantic memory {id}"))
}

fn semantic_region_path(base: &str, projection: &[String]) -> String {
    if projection.is_empty() {
        base.to_owned()
    } else {
        format!("{base}.{}", projection.join("."))
    }
}

fn semantic_region_type<'a>(
    root: &'a boon_typecheck::Type,
    projection: &[String],
) -> Result<&'a boon_typecheck::Type, String> {
    let mut current = root;
    for field in projection {
        current = match current {
            boon_typecheck::Type::Object(shape) => shape.fields.get(field).ok_or_else(|| {
                format!(
                    "semantic memory type projection `{}` has no field `{field}`",
                    projection.join(".")
                )
            })?,
            boon_typecheck::Type::VariantSet(variants) => {
                let projected = variants
                    .iter()
                    .filter_map(|variant| match variant {
                        boon_typecheck::Variant::Tagged { fields, .. } => fields.fields.get(field),
                        boon_typecheck::Variant::Tag(_) => None,
                    })
                    .collect::<Vec<_>>();
                let Some(first) = projected.first().copied() else {
                    return Err(format!(
                        "semantic variant memory projection `{}` has no field `{field}`",
                        projection.join(".")
                    ));
                };
                if projected.iter().any(|candidate| *candidate != first) {
                    return Err(format!(
                        "semantic variant memory projection `{}` has inconsistent field `{field}` types",
                        projection.join(".")
                    ));
                }
                first
            }
            other => {
                return Err(format!(
                    "semantic memory projection `{}` traverses non-record type {other:?}",
                    projection.join(".")
                ));
            }
        };
    }
    Ok(current)
}

fn require_exact_identity_set<T: Copy + Ord>(
    expected: impl IntoIterator<Item = T>,
    emitted: impl IntoIterator<Item = T>,
    label: &str,
) -> Result<(), String> {
    let expected = expected.into_iter().collect::<BTreeSet<_>>();
    let emitted = emitted.into_iter().collect::<BTreeSet<_>>();
    if expected != emitted {
        return Err(format!(
            "semantic-to-executable {label} map does not exactly cover emitted identities"
        ));
    }
    Ok(())
}

fn require_dense(ids: impl IntoIterator<Item = usize>, label: &str) -> Result<(), String> {
    for (expected, actual) in ids.into_iter().enumerate() {
        if actual != expected {
            return Err(format!(
                "{label} ID {actual} is not dense at index {expected}"
            ));
        }
    }
    Ok(())
}

fn exact_map<T: Copy>(
    values: &[T],
    index: usize,
    label: &str,
    id: impl std::fmt::Display,
) -> Result<T, String> {
    values
        .get(index)
        .copied()
        .ok_or_else(|| format!("{label} {id} has no executable mapping"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_dense_semantic_resource_identity_has_no_executable_mapping() {
        let parsed = boon_parser::parse_source(
            "semantic-resource-mapping-invalid.bn",
            "rows: LIST { [value: 1] }",
        )
        .unwrap();
        let checked = boon_typecheck::check_program(&parsed)
            .program
            .expect("fixture typechecks");
        let semantic = boon_semantic::elaborate(checked, &[]).expect("fixture elaborates");
        let mut resources = semantic.resource_graph().clone();
        resources.row_scopes[0].id = SemanticRowScopeId(1);
        let error = map_semantic_execution(semantic.execution_graph(), &resources).unwrap_err();
        assert!(error.contains("not dense"), "{error}");
    }

    #[test]
    fn scalar_list_authority_is_explicitly_erased_without_runtime_row_storage() {
        let parsed = boon_parser::parse_source(
            "semantic-value-list-erasure.bn",
            "numbers: List/range(from: -2, to: 3)",
        )
        .unwrap();
        let checked = boon_typecheck::check_program(&parsed)
            .program
            .expect("fixture typechecks");
        let semantic = boon_semantic::elaborate(checked, &[]).expect("fixture elaborates");
        assert!(semantic.resource_graph().lists.is_empty());
        assert_eq!(semantic.resource_graph().value_list_authorities.len(), 1);

        let execution =
            map_semantic_execution(semantic.execution_graph(), semantic.resource_graph())
                .expect("semantic execution maps");
        let resources = map_semantic_resources(
            semantic.execution_graph(),
            semantic.resource_graph(),
            &execution.id_map,
        )
        .expect("value-only authority erases explicitly");
        assert!(resources.lists.is_empty());
        assert_eq!(resources.erased_value_list_authority_count, 1);

        let mut malformed = semantic.resource_graph().clone();
        malformed.value_list_authorities[0].id = SemanticValueListAuthorityId(1);
        let error = map_semantic_execution(semantic.execution_graph(), &malformed).unwrap_err();
        assert!(error.contains("not dense"), "{error}");

        let mut malformed = semantic.resource_graph().clone();
        let SemanticListInitializerV1::Range { to_expression, .. } =
            &mut malformed.value_list_authorities[0].initializer
        else {
            panic!("fixture value authority is not a range");
        };
        *to_expression = SemanticExprId(usize::MAX);
        let execution = map_semantic_execution(semantic.execution_graph(), &malformed)
            .expect("identity allocation is independent of erased provenance");
        let error =
            map_semantic_resources(semantic.execution_graph(), &malformed, &execution.id_map)
                .unwrap_err();
        assert!(error.contains("has no executable mapping"), "{error}");
    }

    #[test]
    fn non_dense_semantic_identity_has_no_executable_mapping() {
        let parsed = boon_parser::parse_source(
            "semantic-mapping-invalid.bn",
            "value: TEXT { value } |> Text/trim()",
        )
        .unwrap();
        let checked = boon_typecheck::check_program(&parsed)
            .program
            .expect("fixture typechecks");
        let semantic = boon_semantic::elaborate(checked, &[]).expect("fixture elaborates");
        let mut graph = semantic.execution_graph().clone();
        graph.expressions[0].id = SemanticExprId(1);
        let error = map_semantic_execution(&graph, semantic.resource_graph()).unwrap_err();
        assert!(error.contains("not dense"), "{error}");

        let mut graph = semantic.execution_graph().clone();
        graph.expressions[0].value_id = SemanticValueId(1);
        let error = map_semantic_execution(&graph, semantic.resource_graph()).unwrap_err();
        assert!(error.contains("semantic value"), "{error}");
        assert!(error.contains("not dense"), "{error}");

        let mut graph = semantic.execution_graph().clone();
        graph.callables[0].id = SemanticCallableId(1);
        let error = map_semantic_execution(&graph, semantic.resource_graph()).unwrap_err();
        assert!(error.contains("semantic callable"), "{error}");
        assert!(error.contains("not dense"), "{error}");

        let mut graph = semantic.execution_graph().clone();
        graph.calls[0].id = SemanticCallId(1);
        let error = map_semantic_execution(&graph, semantic.resource_graph()).unwrap_err();
        assert!(error.contains("semantic call"), "{error}");
        assert!(error.contains("not dense"), "{error}");
    }

    #[test]
    fn local_binding_allocation_rejects_noncanonical_duplicate_and_dangling_ids() {
        let parsed =
            boon_parser::parse_source("semantic-local-map-invalid.bn", "value: 1").unwrap();
        let checked = boon_typecheck::check_program(&parsed)
            .program
            .expect("fixture typechecks");
        let semantic = boon_semantic::elaborate(checked, &[]).expect("fixture elaborates");
        let declaration = semantic.execution_graph().statements[0]
            .declaration
            .expect("fixture statement declaration");
        let value = semantic.execution_graph().expressions[0].id;

        let mut noncanonical = semantic.execution_graph().clone();
        noncanonical.expressions[0].kind = SemanticExpressionKind::Block {
            bindings: vec![SemanticBlockBinding {
                id: SemanticLocalBindingId(1),
                declaration,
                value,
            }],
            result: value,
        };
        let error = map_semantic_execution(&noncanonical, semantic.resource_graph()).unwrap_err();
        assert!(error.contains("noncanonical"), "{error}");

        let mut duplicate = semantic.execution_graph().clone();
        duplicate.expressions[0].kind = SemanticExpressionKind::Block {
            bindings: vec![
                SemanticBlockBinding {
                    id: SemanticLocalBindingId(0),
                    declaration,
                    value,
                },
                SemanticBlockBinding {
                    id: SemanticLocalBindingId(0),
                    declaration,
                    value,
                },
            ],
            result: value,
        };
        let error = map_semantic_execution(&duplicate, semantic.resource_graph()).unwrap_err();
        assert!(error.contains("defined by both expressions"), "{error}");

        let mut dangling = semantic.execution_graph().clone();
        dangling.expressions[0].kind = SemanticExpressionKind::LocalRead {
            binding: SemanticLocalBindingId(0),
            declaration,
            projection: Vec::new(),
        };
        let error = map_semantic_execution(&dangling, semantic.resource_graph()).unwrap_err();
        assert!(error.contains("referenced without a definition"), "{error}");
    }

    #[test]
    fn call_identity_allocation_rejects_duplicate_noncanonical_and_dangling_ids() {
        let parsed = boon_parser::parse_source(
            "semantic-call-map-invalid.bn",
            "joined: TEXT { a } |> Text/concat(with: TEXT { b })\nspare: 0",
        )
        .unwrap();
        let checked = boon_typecheck::check_program(&parsed)
            .program
            .expect("fixture typechecks");
        let semantic = boon_semantic::elaborate(checked, &[]).expect("fixture elaborates");
        let valid = semantic.execution_graph().clone();
        let call_index = valid
            .expressions
            .iter()
            .position(|expression| matches!(&expression.kind, SemanticExpressionKind::Call { .. }))
            .expect("fixture has a call expression");
        let call_expression = valid.expressions[call_index].clone();
        let SemanticExpressionKind::Call {
            call,
            callable,
            instance: call_instance,
            ..
        } = &call_expression.kind
        else {
            unreachable!()
        };
        let call = *call;
        let callable = *callable;
        let call_instance = *call_instance;
        let spare_index = valid
            .expressions
            .iter()
            .position(|expression| {
                matches!(&expression.kind, SemanticExpressionKind::Number(value) if value == "0")
            })
            .expect("fixture has an unrelated spare expression");

        let mut duplicate = valid.clone();
        duplicate.expressions[spare_index].kind = call_expression.kind.clone();
        let error = map_semantic_execution(&duplicate, semantic.resource_graph()).unwrap_err();
        assert!(error.contains("defined by both expressions"), "{error}");

        let mut dangling = valid.clone();
        dangling.expressions[spare_index].kind = SemanticExpressionKind::ElementState {
            context: SemanticCallContextId {
                call_instance,
                ordinal: usize::MAX,
            },
            projection: Vec::new(),
        };
        let error = map_semantic_execution(&dangling, semantic.resource_graph()).unwrap_err();
        assert!(
            error.contains("referenced without a call definition"),
            "{error}"
        );

        let mut mapped = map_semantic_execution(&valid, semantic.resource_graph())
            .expect("valid call identities map");
        assert_eq!(
            mapped
                .id_map
                .call_expression(call, call_expression.id)
                .unwrap(),
            mapped.id_map.expression(call_expression.id).unwrap()
        );
        assert_eq!(
            mapped.id_map.callable(callable).unwrap(),
            FunctionId(callable.as_usize())
        );
        for expression in &valid.expressions {
            assert_eq!(
                mapped.id_map.value(expression.value_id).unwrap(),
                mapped.id_map.expression(expression.id).unwrap()
            );
        }
        let instance = mapped
            .executable
            .expressions
            .iter_mut()
            .find_map(|expression| match &mut expression.kind {
                ExecutableExpressionKind::Call { instance, .. } => Some(instance),
                _ => None,
            })
            .expect("mapped call expression");
        *instance = usize::MAX;
        let error = mapped.validate_totality().unwrap_err();
        assert!(error.contains("call instance"), "{error}");

        let mut missing_binding = valid.clone();
        let SemanticExpressionKind::Call {
            parameter_bindings, ..
        } = &mut missing_binding.expressions[call_index].kind
        else {
            unreachable!()
        };
        parameter_bindings.clear();
        let error =
            map_semantic_execution(&missing_binding, semantic.resource_graph()).unwrap_err();
        assert!(error.contains("parameter bindings"), "{error}");

        let mut mismatched_binding = valid.clone();
        let SemanticExpressionKind::Call {
            parameter_bindings, ..
        } = &mut mismatched_binding.expressions[call_index].kind
        else {
            unreachable!()
        };
        let SemanticCallParameterBindingKind::Explicit { from_pipe, .. } = &mut parameter_bindings
            .iter_mut()
            .find(|binding| {
                matches!(
                    &binding.kind,
                    SemanticCallParameterBindingKind::Explicit { .. }
                )
            })
            .expect("fixture has an explicit binding")
            .kind
        else {
            unreachable!()
        };
        *from_pipe = !*from_pipe;
        let error =
            map_semantic_execution(&mismatched_binding, semantic.resource_graph()).unwrap_err();
        assert!(error.contains("differs from its argument"), "{error}");

        let mut required_omission = valid.clone();
        let SemanticExpressionKind::Call {
            arguments,
            parameter_bindings,
            ..
        } = &mut required_omission.expressions[call_index].kind
        else {
            unreachable!()
        };
        let required = parameter_bindings
            .iter_mut()
            .find(|binding| {
                binding.requirement == CheckedParameterRequirement::Required
                    && matches!(
                        &binding.kind,
                        SemanticCallParameterBindingKind::Explicit { .. }
                    )
            })
            .expect("fixture has a required explicit binding");
        arguments.retain(|argument| argument.formal != required.formal);
        required.kind = SemanticCallParameterBindingKind::Omitted;
        let error =
            map_semantic_execution(&required_omission, semantic.resource_graph()).unwrap_err();
        assert!(error.contains("omits required parameter"), "{error}");

        let SemanticExpressionKind::Call {
            parameter_bindings, ..
        } = &valid.expressions[call_index].kind
        else {
            unreachable!()
        };
        assert!(
            parameter_bindings
                .iter()
                .any(|binding| matches!(&binding.kind, SemanticCallParameterBindingKind::Omitted)),
            "fixture exercises a real callable-profile omission"
        );

        let mut ambiguous = valid.clone();
        let new_instance = OutCallInstanceId::from_usize(call_instance.as_usize() + 1);
        ambiguous.expressions[spare_index].checked_expr_id = call_expression.checked_expr_id;
        ambiguous.expressions[spare_index].flow_type = call_expression.flow_type.clone();
        ambiguous.expressions[spare_index].effect = call_expression.effect;
        ambiguous.expressions[spare_index].kind = call_expression.kind.clone();
        let SemanticExpressionKind::Call { instance, .. } =
            &mut ambiguous.expressions[spare_index].kind
        else {
            unreachable!()
        };
        *instance = new_instance;
        let ambiguous = map_semantic_execution(&ambiguous, semantic.resource_graph())
            .expect("multiple genuine call expressions map without inventing a singular ID");
        assert_eq!(
            ambiguous
                .id_map
                .call_expression(call, call_expression.id)
                .unwrap(),
            ambiguous.id_map.expression(call_expression.id).unwrap()
        );
        let error = ambiguous.id_map.unique_call_expression(call).unwrap_err();
        assert!(error.contains("no exact singular mapping"), "{error}");
    }

    #[test]
    fn expanded_user_calls_report_that_no_executable_call_counterpart_exists() {
        let parsed = boon_parser::parse_source(
            "semantic-expanded-user-call-map.bn",
            r#"
FUNCTION identity(value) {
    value
}

result: identity(value: 1)
"#,
        )
        .unwrap();
        let checked = boon_typecheck::check_program(&parsed)
            .program
            .expect("fixture typechecks");
        let semantic = boon_semantic::elaborate(checked, &[]).expect("fixture elaborates");
        let graph = semantic.execution_graph();
        let user_call = graph
            .calls
            .iter()
            .find(|call| {
                semantic_callable(graph, call.callable).is_ok_and(|callable| {
                    callable.kind == boon_typecheck::CheckedCallableKind::User
                })
            })
            .expect("fixture has a user call");
        let mapped = map_semantic_execution(graph, semantic.resource_graph())
            .expect("expanded user call graph maps");
        let error = mapped
            .id_map
            .unique_call_expression(user_call.id)
            .unwrap_err();
        assert!(
            error.contains("no executable call-expression counterpart"),
            "{error}"
        );
    }

    #[test]
    fn external_identity_erasure_is_kind_checked_and_call_provenance_is_exact() {
        let parsed = boon_parser::parse_source(
            "semantic-external-identity-map.bn",
            "called: TEXT { value } |> Text/trim()\nread: 1",
        )
        .unwrap();
        let checked = boon_typecheck::check_program(&parsed)
            .program
            .expect("fixture typechecks");
        let semantic = boon_semantic::elaborate(checked, &[]).expect("fixture elaborates");
        let identity = boon_typecheck::CheckedExternalDeclarationIdentityV1 {
            producer_role: boon_typecheck::ProgramRole::Server,
            producer_source_bundle_digest_v1: semantic.source_bundle_digest_v1(),
            producer_declaration: semantic.execution_graph().callables[0].checked_callable,
            kind: CheckedExternalDeclarationKind::Callable,
        };

        let mut stale_call = semantic.execution_graph().clone();
        stale_call.calls[0].external_identity = Some(identity);
        let error = map_semantic_execution(&stale_call, semantic.resource_graph()).unwrap_err();
        assert!(error.contains("external identity differs"), "{error}");

        let mut illegal_builtin = semantic.execution_graph().clone();
        let callable = illegal_builtin.calls[0].callable;
        illegal_builtin.callables[callable.as_usize()].external_identity = Some(identity);
        illegal_builtin.calls[0].external_identity = Some(identity);
        let error =
            map_semantic_execution(&illegal_builtin, semantic.resource_graph()).unwrap_err();
        assert!(error.contains("non-external callable"), "{error}");

        let mut wrong_value_kind = semantic.execution_graph().clone();
        let read = wrong_value_kind
            .expressions
            .iter_mut()
            .find(|expression| matches!(&expression.kind, SemanticExpressionKind::Number(value) if value == "1"))
            .expect("fixture has a read replacement expression");
        read.kind = SemanticExpressionKind::ExternalRead {
            canonical_path: "Server/value".to_owned(),
            external_identity: Some(identity),
        };
        let error =
            map_semantic_execution(&wrong_value_kind, semantic.resource_graph()).unwrap_err();
        assert!(error.contains("non-value external identity"), "{error}");

        let mut exact_value = semantic.execution_graph().clone();
        let read = exact_value
            .expressions
            .iter_mut()
            .find(|expression| matches!(&expression.kind, SemanticExpressionKind::Number(value) if value == "1"))
            .expect("fixture has a read replacement expression");
        read.kind = SemanticExpressionKind::ExternalRead {
            canonical_path: "Server/value".to_owned(),
            external_identity: Some(boon_typecheck::CheckedExternalDeclarationIdentityV1 {
                kind: CheckedExternalDeclarationKind::Value,
                ..identity
            }),
        };
        map_semantic_execution(&exact_value, semantic.resource_graph())
            .expect("exact value identity is validated before executable erasure");
    }

    #[test]
    fn sealed_external_identities_map_only_after_exact_semantic_validation() {
        let producer = boon_parser::parse_source(
            "semantic-external-producer.bn",
            "store: [count: 1]\nFUNCTION add(value) { value }\n",
        )
        .unwrap();
        let consumer = boon_parser::parse_source(
            "semantic-external-consumer.bn",
            "count: Session/store.count\nnext: Session/add(value: count)\n",
        )
        .unwrap();
        let number = boon_typecheck::FlowType {
            mode: boon_typecheck::FlowMode::Continuous,
            ty: boon_typecheck::Type::Number,
        };
        let value_identity = boon_typecheck::CheckedExternalDeclarationIdentityV1 {
            producer_role: boon_typecheck::ProgramRole::Session,
            producer_source_bundle_digest_v1: producer.source_bundle_digest_v1,
            producer_declaration: boon_typecheck::DeclId(41),
            kind: CheckedExternalDeclarationKind::Value,
        };
        let callable_identity = boon_typecheck::CheckedExternalDeclarationIdentityV1 {
            producer_declaration: boon_typecheck::DeclId(42),
            kind: CheckedExternalDeclarationKind::Callable,
            ..value_identity
        };
        let mut environment =
            boon_typecheck::ExternalTypeEnvironment::sealed(boon_typecheck::ProgramRole::Client);
        environment
            .values
            .insert("Session/store.count".to_owned(), number.clone());
        environment.functions.insert(
            "Session/add".to_owned(),
            boon_typecheck::ExternalFunctionType {
                args: vec![boon_typecheck::ExternalFunctionArgument {
                    name: "value".to_owned(),
                    flow_type: number.clone(),
                }],
                result: number,
                effect: boon_typecheck::CheckedEffectSummary::default(),
            },
        );
        environment
            .external_identities
            .insert("Session/store.count".to_owned(), value_identity);
        environment
            .external_identities
            .insert("Session/add".to_owned(), callable_identity);
        let checked = boon_typecheck::check_program_with_external_types(&consumer, &environment);
        assert!(
            !checked.report.has_errors(),
            "diagnostics: {:#?}",
            checked.report.diagnostics
        );
        let semantic = boon_semantic::elaborate(
            checked
                .program
                .expect("sealed external fixture has a checked program"),
            &[],
        )
        .expect("sealed external fixture elaborates");
        assert!(
            semantic
                .execution_graph()
                .expressions
                .iter()
                .any(|expression| {
                    matches!(
                        &expression.kind,
                        SemanticExpressionKind::ExternalRead {
                            external_identity: Some(identity),
                            ..
                        } if *identity == value_identity
                    )
                })
        );
        let external_call = semantic
            .execution_graph()
            .expressions
            .iter()
            .find(|expression| {
                matches!(
                    &expression.kind,
                    SemanticExpressionKind::Call {
                        callable_kind: SemanticCallableKind::External,
                        ..
                    }
                )
            })
            .expect("sealed external fixture has an external call");
        let SemanticExpressionKind::Call { call, callable, .. } = &external_call.kind else {
            unreachable!()
        };
        assert_eq!(
            semantic.execution_graph().calls[call.as_usize()].external_identity,
            Some(callable_identity)
        );
        assert_eq!(
            semantic.execution_graph().callables[callable.as_usize()].external_identity,
            Some(callable_identity)
        );
        let mapped = map_semantic_execution(semantic.execution_graph(), semantic.resource_graph())
            .expect("sealed external identities validate and erase");
        assert_eq!(
            mapped
                .id_map
                .call_expression(*call, external_call.id)
                .unwrap(),
            mapped.id_map.expression(external_call.id).unwrap()
        );
        assert!(matches!(
            &mapped.executable.expressions[external_call.id.as_usize()].kind,
            ExecutableExpressionKind::Call {
                callable_kind: ExecutableCallableKind::External,
                ..
            }
        ));
        let mapped_resources = map_semantic_resources(
            semantic.execution_graph(),
            semantic.resource_graph(),
            &mapped.id_map,
        )
        .expect("sealed external resources map");
        let mapped_reactive = map_semantic_reactive(
            semantic.execution_graph(),
            semantic.resource_graph(),
            semantic.reactive_graph(),
            &mapped.id_map,
            &mapped_resources,
        )
        .expect("sealed external reactive graph maps");
        let mapped_storage = map_semantic_storage_join(
            semantic.execution_graph(),
            semantic.resource_graph(),
            semantic.reactive_graph(),
            semantic.scope_storage_graph(),
            semantic.lowering_contract(),
            &mapped.id_map,
            &mapped_resources,
            &mapped_reactive,
        )
        .expect("sealed external storage joins without assigning bundle ordinals");
        assert_eq!(mapped_storage.external_references.len(), 2);
        assert_eq!(mapped_storage.call_invocations.len(), 1);
        assert!(
            mapped_storage.external_references.iter().all(|reference| {
                reference.bundle_ready && reference.external_identity.is_some()
            })
        );

        let map_storage = |storage: &SemanticScopeStorageGraphV1| {
            map_semantic_storage_join(
                semantic.execution_graph(),
                semantic.resource_graph(),
                semantic.reactive_graph(),
                storage,
                semantic.lowering_contract(),
                &mapped.id_map,
                &mapped_resources,
                &mapped_reactive,
            )
        };
        let mut noncanonical = semantic.scope_storage_graph().clone();
        noncanonical.external_references[0].id = SemanticStorageExternalReferenceId(usize::MAX);
        let error = map_storage(&noncanonical).unwrap_err();
        assert!(error.contains("external reference"), "{error}");
        assert!(error.contains("not dense"), "{error}");

        let mut stale_identity = semantic.scope_storage_graph().clone();
        let identity = stale_identity.external_references[0]
            .external_identity
            .as_mut()
            .expect("sealed reference has identity");
        identity.producer_declaration = DeclId(u32::MAX);
        let error = map_storage(&stale_identity).unwrap_err();
        assert!(error.contains("exact staged identity"), "{error}");

        let mut stale_schedule = mapped_storage.clone();
        stale_schedule.call_invocations[0].expression = ExecutableExprId(usize::MAX);
        let error = stale_schedule
            .validate_totality(
                semantic.scope_storage_graph(),
                semantic.reactive_graph(),
                &mapped_resources,
                &mapped.id_map,
            )
            .unwrap_err();
        assert!(error.contains("call-invocation schedule"), "{error}");

        let mut stale_mapped = mapped_storage;
        stale_mapped.external_references[0].external_identity = None;
        let error = stale_mapped
            .validate_totality(
                semantic.scope_storage_graph(),
                semantic.reactive_graph(),
                &mapped_resources,
                &mapped.id_map,
            )
            .unwrap_err();
        assert!(error.contains("external reference"), "{error}");
    }

    #[test]
    fn producer_function_ids_are_derived_from_all_callable_identity() {
        let parsed = boon_parser::parse_source(
            "semantic-producer-callable-map.bn",
            r#"
FUNCTION serve(value) {
    value + 0
}

seed: 0
"#,
        )
        .unwrap();
        let checked = boon_typecheck::check_program(&parsed)
            .program
            .expect("fixture typechecks");
        let callable = SemanticCallableId(
            checked
                .callables
                .iter()
                .position(|callable| callable.name == "serve")
                .expect("fixture has serve callable"),
        );
        let semantic = boon_semantic::elaborate(
            checked,
            &[boon_semantic::ProducerMaterializationRequest {
                identity: [7; 32],
                callable,
                local_function: "serve".to_owned(),
                mode: boon_semantic::ProducerMaterializationMode::Current,
            }],
        )
        .expect("producer fixture elaborates");
        let mapped = map_semantic_execution(semantic.execution_graph(), semantic.resource_graph())
            .expect("producer fixture maps");
        let function = &semantic.execution_graph().functions[0];
        let expected = mapped.id_map.callable(function.callable).unwrap();
        assert_eq!(
            mapped.id_map.producer_function(function.producer).unwrap(),
            expected
        );
        assert_eq!(mapped.executable.functions[0].id, expected);
        assert_eq!(expected, FunctionId(function.callable.as_usize()));
        mapped.validate_totality().unwrap();

        let resources = map_semantic_resources(
            semantic.execution_graph(),
            semantic.resource_graph(),
            &mapped.id_map,
        )
        .expect("producer resources map");
        let reactive = map_semantic_reactive(
            semantic.execution_graph(),
            semantic.resource_graph(),
            semantic.reactive_graph(),
            &mapped.id_map,
            &resources,
        )
        .expect("producer reactive graph maps");
        let storage = map_semantic_storage_join(
            semantic.execution_graph(),
            semantic.resource_graph(),
            semantic.reactive_graph(),
            semantic.scope_storage_graph(),
            semantic.lowering_contract(),
            &mapped.id_map,
            &resources,
            &reactive,
        )
        .expect("producer result joins through D storage");
        assert_eq!(storage.producer_function_instances.len(), 1);
        assert_eq!(
            storage.producer_function_instances[0].result_field,
            storage.id_map.storage_fields[semantic.scope_storage_graph().producer_result_fields[0]
                .storage_field
                .as_usize()]
        );

        let mut stale_result = semantic.scope_storage_graph().clone();
        stale_result.producer_result_fields[0].storage_field = SemanticStorageFieldId(usize::MAX);
        let error = map_semantic_storage_join(
            semantic.execution_graph(),
            semantic.resource_graph(),
            semantic.reactive_graph(),
            &stale_result,
            semantic.lowering_contract(),
            &mapped.id_map,
            &resources,
            &reactive,
        )
        .unwrap_err();
        assert!(error.contains("storage field"), "{error}");
    }

    #[test]
    fn materialization_local_allocation_rejects_noncanonical_overflow_and_dangling_ids() {
        let parsed = boon_parser::parse_source(
            "semantic-materialization-local-map-invalid.bn",
            r#"
rows: LIST { [value: 1] }
result: rows |> List/map(item, new: item.value + 1)
"#,
        )
        .unwrap();
        let checked = boon_typecheck::check_program(&parsed)
            .program
            .expect("fixture typechecks");
        let semantic = boon_semantic::elaborate(checked, &[]).expect("fixture elaborates");
        let owner = semantic.execution_graph().materializations[0].owner;

        let mut noncanonical = semantic.execution_graph().clone();
        rewrite_materialization_local(
            &mut noncanonical,
            owner,
            SemanticMaterializationLocalId(0),
            SemanticMaterializationLocalId(1),
        );
        let error = map_semantic_execution(&noncanonical, semantic.resource_graph()).unwrap_err();
        assert!(error.contains("noncanonical"), "{error}");

        let mut overflow = semantic.execution_graph().clone();
        rewrite_materialization_local(
            &mut overflow,
            owner,
            SemanticMaterializationLocalId(0),
            SemanticMaterializationLocalId(usize::MAX),
        );
        let error = map_semantic_execution(&overflow, semantic.resource_graph()).unwrap_err();
        assert!(error.contains("exceeds executable u32"), "{error}");

        let mut dangling = semantic.execution_graph().clone();
        dangling
            .expressions
            .iter_mut()
            .next()
            .expect("fixture expression")
            .kind = SemanticExpressionKind::MaterializationLocal {
            owner,
            local: SemanticMaterializationLocalId(1),
            projection: Vec::new(),
        };
        let error = map_semantic_execution(&dangling, semantic.resource_graph()).unwrap_err();
        assert!(error.contains("referenced without a definition"), "{error}");

        let mut mapped =
            map_semantic_execution(semantic.execution_graph(), semantic.resource_graph())
                .expect("valid materialization locals map");
        mapped.materializations[0].row_local = MaterializationLocalId(1);
        let error = mapped.validate_totality().unwrap_err();
        assert!(error.contains("materialization local"), "{error}");
    }

    fn rewrite_materialization_local(
        graph: &mut SemanticExecutionGraphV1,
        owner: StaticOwnerId,
        from: SemanticMaterializationLocalId,
        to: SemanticMaterializationLocalId,
    ) {
        for materialization in &mut graph.materializations {
            if materialization.owner == owner && materialization.row_local == from {
                materialization.row_local = to;
            }
        }
        for expression in &mut graph.expressions {
            if let SemanticExpressionKind::MaterializationLocal {
                owner: candidate,
                local,
                ..
            } = &mut expression.kind
                && *candidate == owner
                && *local == from
            {
                *local = to;
            }
            for member in &mut expression.provenance.members {
                if let SemanticValueOrigin::MaterializationLocal {
                    owner: candidate,
                    local,
                    ..
                } = &mut member.origin
                    && *candidate == owner
                    && *local == from
                {
                    *local = to;
                }
            }
        }
    }

    #[test]
    fn runtime_resource_allocation_rejects_noncanonical_and_missing_domains() {
        let parsed = boon_parser::parse_source(
            "semantic-runtime-resource-map-invalid.bn",
            r#"
pulse: SOURCE
selected:
    False |> HOLD selected {
        pulse |> THEN { True }
    }
"#,
        )
        .unwrap();
        let checked = boon_typecheck::check_program(&parsed)
            .program
            .expect("fixture typechecks");
        let semantic = boon_semantic::elaborate(checked, &[]).expect("fixture elaborates");

        let mut noncanonical_source = semantic.resource_graph().clone();
        noncanonical_source.sources[0].id = SemanticSourceId(1);
        let error =
            map_semantic_execution(semantic.execution_graph(), &noncanonical_source).unwrap_err();
        assert!(error.contains("runtime source"), "{error}");
        assert!(error.contains("noncanonical"), "{error}");

        let mut noncanonical_state = semantic.resource_graph().clone();
        noncanonical_state.states[0].id = SemanticStateId(1);
        let error =
            map_semantic_execution(semantic.execution_graph(), &noncanonical_state).unwrap_err();
        assert!(error.contains("runtime state"), "{error}");
        assert!(error.contains("noncanonical"), "{error}");

        let mut missing_source = semantic.resource_graph().clone();
        missing_source.sources.clear();
        let error =
            map_semantic_execution(semantic.execution_graph(), &missing_source).unwrap_err();
        assert!(error.contains("runtime source domain"), "{error}");

        let execution =
            map_semantic_execution(semantic.execution_graph(), semantic.resource_graph())
                .expect("valid execution mapping");
        let mut resources = map_semantic_resources(
            semantic.execution_graph(),
            semantic.resource_graph(),
            &execution.id_map,
        )
        .expect("valid resource mapping");
        resources.sources[0].id = SourceId(usize::MAX);
        let error = resources
            .validate_totality(semantic.resource_graph(), &execution.id_map)
            .unwrap_err();
        assert!(error.contains("runtime ID"), "{error}");
    }

    #[test]
    fn semantic_storage_join_rejects_owner_ancestry_order_mutation() {
        let parsed = boon_parser::parse_source(
            "semantic-storage-owner-order.bn",
            r#"
flavors: LIST { [suffix: TEXT { left }] }
rows: LIST { [name: TEXT { A }] }
projected:
    flavors |> List/map(item, new: projected_flavor(flavor: item))

FUNCTION projected_flavor(flavor) {
    [
        detail_label:
            rows
            |> List/map(item, new:
                detail_row(row: item, suffix: flavor.suffix).label
            )
            |> List/latest()
    ]
}

FUNCTION detail_row(row, suffix) {
    [label: row.name |> Text/concat(with: suffix, separator: ":")]
}
"#,
        )
        .unwrap();
        let checked = boon_typecheck::check_program(&parsed)
            .program
            .expect("nested owner fixture typechecks");
        let semantic =
            boon_semantic::elaborate(checked, &[]).expect("nested owner fixture elaborates");
        let execution =
            map_semantic_execution(semantic.execution_graph(), semantic.resource_graph())
                .expect("nested execution maps");
        let resources = map_semantic_resources(
            semantic.execution_graph(),
            semantic.resource_graph(),
            &execution.id_map,
        )
        .expect("nested resources map");
        let reactive = map_semantic_reactive(
            semantic.execution_graph(),
            semantic.resource_graph(),
            semantic.reactive_graph(),
            &execution.id_map,
            &resources,
        )
        .expect("nested reactive graph maps");
        map_semantic_storage_join(
            semantic.execution_graph(),
            semantic.resource_graph(),
            semantic.reactive_graph(),
            semantic.scope_storage_graph(),
            semantic.lowering_contract(),
            &execution.id_map,
            &resources,
            &reactive,
        )
        .expect("root-to-leaf D ancestry maps directly");

        let mut noncanonical_owner = semantic.scope_storage_graph().clone();
        noncanonical_owner.owners[0].id = StaticOwnerId(usize::MAX);
        let error = map_semantic_storage_join(
            semantic.execution_graph(),
            semantic.resource_graph(),
            semantic.reactive_graph(),
            &noncanonical_owner,
            semantic.lowering_contract(),
            &execution.id_map,
            &resources,
            &reactive,
        )
        .unwrap_err();
        assert!(error.contains("storage owner"), "{error}");

        let mut reversed = semantic.scope_storage_graph().clone();
        let (canonical, expected_error) = if let Some(binding) = reversed
            .bindings
            .iter_mut()
            .find(|binding| binding.owner_ancestry.len() > 1)
        {
            let canonical = binding.owner_ancestry.clone();
            binding.owner_ancestry.reverse();
            (canonical, "owner/path metadata")
        } else if let Some(source) = reversed
            .sources
            .iter_mut()
            .find(|source| source.owner_ancestry.len() > 1)
        {
            let canonical = source.owner_ancestry.clone();
            source.owner_ancestry.reverse();
            (canonical, "exact resource/staged identity")
        } else {
            let child = reversed
                .owners
                .iter()
                .find(|owner| owner.parent.is_some())
                .expect("nested fixture has a child storage owner");
            let canonical = vec![child.parent.expect("child owner has a parent"), child.id];
            let mut noncanonical = canonical.clone();
            noncanonical.reverse();
            if let Some(binding) = reversed.bindings.first_mut() {
                binding.owner_ancestry = noncanonical;
                (canonical, "owner/path metadata")
            } else if let Some(source) = reversed.sources.first_mut() {
                source.owner_ancestry = noncanonical;
                (canonical, "exact resource/staged identity")
            } else {
                panic!("nested fixture has no storage carrier");
            }
        };
        assert!(
            canonical
                .windows(2)
                .all(|pair| { reversed.owners[pair[1].as_usize()].parent == Some(pair[0]) })
        );
        let error = map_semantic_storage_join(
            semantic.execution_graph(),
            semantic.resource_graph(),
            semantic.reactive_graph(),
            &reversed,
            semantic.lowering_contract(),
            &execution.id_map,
            &resources,
            &reactive,
        )
        .unwrap_err();
        assert!(error.contains(expected_error), "{error}");
    }

    #[test]
    fn semantic_storage_join_rejects_detached_capture_identity_mutation() {
        let parsed = boon_parser::parse_source(
            "semantic-storage-detached-capture.bn",
            r#"
flavors: LIST { [suffix: TEXT { left }] }
rows: LIST { [name: TEXT { A }] }
projected:
    flavors |> List/map(item, new: projected_flavor(flavor: item))

FUNCTION projected_flavor(flavor) {
    [
        nested:
            rows
            |> List/map(item, new: [
                name: item.name
                retained:
                    flavor.suffix |> HOLD retained { LATEST {} }
            ])
    ]
}
"#,
        )
        .unwrap();
        let checked = boon_typecheck::check_program(&parsed)
            .program
            .expect("detached-capture fixture typechecks");
        let semantic =
            boon_semantic::elaborate(checked, &[]).expect("detached-capture fixture elaborates");
        let storage = semantic.scope_storage_graph();
        assert!(
            storage
                .locals
                .iter()
                .flat_map(|local| &local.captures)
                .next()
                .is_some(),
            "fixture derives at least one exact detached capture"
        );
        let execution =
            map_semantic_execution(semantic.execution_graph(), semantic.resource_graph())
                .expect("detached-capture execution maps");
        let resources = map_semantic_resources(
            semantic.execution_graph(),
            semantic.resource_graph(),
            &execution.id_map,
        )
        .expect("detached-capture resources map");
        let reactive = map_semantic_reactive(
            semantic.execution_graph(),
            semantic.resource_graph(),
            semantic.reactive_graph(),
            &execution.id_map,
            &resources,
        )
        .expect("detached-capture reactive graph maps");
        map_semantic_storage_join(
            semantic.execution_graph(),
            semantic.resource_graph(),
            semantic.reactive_graph(),
            storage,
            semantic.lowering_contract(),
            &execution.id_map,
            &resources,
            &reactive,
        )
        .expect("detached capture joins to its final FieldId");

        let mut noncanonical = storage.clone();
        noncanonical
            .locals
            .iter_mut()
            .flat_map(|local| &mut local.captures)
            .next()
            .expect("fixture has a capture")
            .id = boon_semantic::SemanticStorageCaptureId(usize::MAX);
        let error = map_semantic_storage_join(
            semantic.execution_graph(),
            semantic.resource_graph(),
            semantic.reactive_graph(),
            &noncanonical,
            semantic.lowering_contract(),
            &execution.id_map,
            &resources,
            &reactive,
        )
        .unwrap_err();
        assert!(error.contains("semantic storage capture"), "{error}");
        assert!(error.contains("not dense"), "{error}");
    }

    #[test]
    fn semantic_storage_join_rejects_row_source_projection_identity_mutation() {
        let parsed = boon_parser::parse_source(
            "semantic-storage-row-source.bn",
            r#"
seed: LIST { [key: TEXT { row }] }
rows:
    seed |> List/map(item, new: selectable_row(seed: item))

FUNCTION selectable_row(seed) {
    [key: seed.key, select: SOURCE]
}
"#,
        )
        .unwrap();
        let checked = boon_typecheck::check_program(&parsed)
            .program
            .expect("indexed source fixture typechecks");
        let semantic =
            boon_semantic::elaborate(checked, &[]).expect("indexed source fixture elaborates");
        assert!(
            !semantic
                .scope_storage_graph()
                .row_source_projections
                .is_empty(),
            "fixture has exact row source projections"
        );
        assert!(
            !semantic.scope_storage_graph().row_values.is_empty(),
            "fixture has exact row-value projections"
        );
        let execution =
            map_semantic_execution(semantic.execution_graph(), semantic.resource_graph())
                .expect("indexed execution maps");
        let resources = map_semantic_resources(
            semantic.execution_graph(),
            semantic.resource_graph(),
            &execution.id_map,
        )
        .expect("indexed resources map");
        let reactive = map_semantic_reactive(
            semantic.execution_graph(),
            semantic.resource_graph(),
            semantic.reactive_graph(),
            &execution.id_map,
            &resources,
        )
        .expect("indexed reactive graph maps");
        map_semantic_storage_join(
            semantic.execution_graph(),
            semantic.resource_graph(),
            semantic.reactive_graph(),
            semantic.scope_storage_graph(),
            semantic.lowering_contract(),
            &execution.id_map,
            &resources,
            &reactive,
        )
        .expect("indexed storage joins");

        let mut mutated = semantic.scope_storage_graph().clone();
        mutated.row_source_projections[0].source = SemanticSourceId(usize::MAX);
        let error = map_semantic_storage_join(
            semantic.execution_graph(),
            semantic.resource_graph(),
            semantic.reactive_graph(),
            &mutated,
            semantic.lowering_contract(),
            &execution.id_map,
            &resources,
            &reactive,
        )
        .unwrap_err();
        assert!(error.contains("runtime source"), "{error}");
        assert!(error.contains("no executable mapping"), "{error}");

        let mut stale_row_value = semantic.scope_storage_graph().clone();
        stale_row_value.row_values[0].expression = SemanticExprId(usize::MAX);
        let error = map_semantic_storage_join(
            semantic.execution_graph(),
            semantic.resource_graph(),
            semantic.reactive_graph(),
            &stale_row_value,
            semantic.lowering_contract(),
            &execution.id_map,
            &resources,
            &reactive,
        )
        .unwrap_err();
        assert!(error.contains("semantic expression"), "{error}");
        assert!(error.contains("no executable mapping"), "{error}");
    }

    #[test]
    fn semantic_storage_join_maps_host_effect_schedules_without_rediscovery() {
        let parsed = boon_parser::parse_source(
            "semantic-storage-host-effect-schedule.bn",
            r#"
start: SOURCE
result:
    NotRequested |> HOLD result {
        start |> THEN { Clock/wall() }
    }
"#,
        )
        .unwrap();
        let checked = boon_typecheck::check_program(&parsed)
            .program
            .expect("host-effect fixture typechecks");
        let semantic =
            boon_semantic::elaborate(checked, &[]).expect("host-effect fixture elaborates");
        assert_eq!(semantic.reactive_graph().host_effect_schedules.len(), 1);
        let execution =
            map_semantic_execution(semantic.execution_graph(), semantic.resource_graph())
                .expect("host-effect execution maps");
        let resources = map_semantic_resources(
            semantic.execution_graph(),
            semantic.resource_graph(),
            &execution.id_map,
        )
        .expect("host-effect resources map");
        let reactive = map_semantic_reactive(
            semantic.execution_graph(),
            semantic.resource_graph(),
            semantic.reactive_graph(),
            &execution.id_map,
            &resources,
        )
        .expect("host-effect reactive graph maps");
        let mut storage = map_semantic_storage_join(
            semantic.execution_graph(),
            semantic.resource_graph(),
            semantic.reactive_graph(),
            semantic.scope_storage_graph(),
            semantic.lowering_contract(),
            &execution.id_map,
            &resources,
            &reactive,
        )
        .expect("host-effect schedule joins exact state-update arms");
        assert_eq!(storage.host_effect_schedules.len(), 1);
        assert_eq!(
            storage.host_effect_schedules[0].state_update_arms,
            storage.state_update_arms
        );

        storage.host_effect_schedules[0]
            .operation
            .push_str("/stale");
        let error = storage
            .validate_totality(
                semantic.scope_storage_graph(),
                semantic.reactive_graph(),
                &resources,
                &execution.id_map,
            )
            .unwrap_err();
        assert!(error.contains("host-effect schedule"), "{error}");
    }

    #[test]
    fn semantic_storage_join_maps_list_mutation_schedule_ordinals() {
        let parsed = boon_parser::parse_source(
            "semantic-storage-list-mutation-schedule.bn",
            r#"
store: [
    elements: [create: SOURCE]
    group_to_create:
        elements.create.event.press |> THEN { TEXT { core } }
    groups:
        LIST {}
        |> List/append(item: group_to_create |> THEN {
            [name: group_to_create]
        })
        |> List/map(item, new: [name: item.name])
]
"#,
        )
        .unwrap();
        let checked = boon_typecheck::check_program(&parsed)
            .program
            .expect("list-mutation fixture typechecks");
        let semantic =
            boon_semantic::elaborate(checked, &[]).expect("list-mutation fixture elaborates");
        assert_eq!(semantic.reactive_graph().list_mutations.len(), 1);
        let execution =
            map_semantic_execution(semantic.execution_graph(), semantic.resource_graph())
                .expect("list-mutation execution maps");
        let resources = map_semantic_resources(
            semantic.execution_graph(),
            semantic.resource_graph(),
            &execution.id_map,
        )
        .expect("list-mutation resources map");
        let reactive = map_semantic_reactive(
            semantic.execution_graph(),
            semantic.resource_graph(),
            semantic.reactive_graph(),
            &execution.id_map,
            &resources,
        )
        .expect("list-mutation reactive graph maps");
        let mut storage = map_semantic_storage_join(
            semantic.execution_graph(),
            semantic.resource_graph(),
            semantic.reactive_graph(),
            semantic.scope_storage_graph(),
            semantic.lowering_contract(),
            &execution.id_map,
            &resources,
            &reactive,
        )
        .expect("list mutation joins to its final schedule ordinal");
        assert_eq!(storage.list_mutations[0].ordinal, 0);

        storage.list_mutations[0].ordinal = u32::MAX;
        let error = storage
            .validate_totality(
                semantic.scope_storage_graph(),
                semantic.reactive_graph(),
                &resources,
                &execution.id_map,
            )
            .unwrap_err();
        assert!(error.contains("mapped list mutation"), "{error}");
    }

    #[test]
    fn semantic_storage_join_preserves_fixed_bytes_representation_paths() {
        let object = |fields: Vec<(&str, boon_typecheck::Type)>| {
            let field_order = fields
                .iter()
                .map(|(name, _)| (*name).to_owned())
                .collect::<Vec<_>>();
            boon_typecheck::Type::Object(boon_typecheck::ObjectShape {
                fields: fields
                    .into_iter()
                    .map(|(name, ty)| (name.to_owned(), ty))
                    .collect(),
                field_order,
                open: false,
            })
        };
        let dynamic = object(vec![(
            "envelope",
            boon_typecheck::Type::List(Box::new(object(vec![(
                "body",
                boon_typecheck::Type::Bytes(boon_typecheck::BytesType::Dynamic),
            )]))),
        )]);
        let fixed = object(vec![(
            "envelope",
            boon_typecheck::Type::List(Box::new(object(vec![(
                "body",
                boon_typecheck::Type::Bytes(boon_typecheck::BytesType::Fixed(8)),
            )]))),
        )]);
        let semantic = boon_semantic::SemanticStorageRepresentationV1::CheckedFixedBytes {
            refinements: vec![boon_semantic::SemanticStorageFixedBytesRefinementV1 {
                path: vec![
                    boon_semantic::SemanticStorageTypePathSegmentV1::ObjectField {
                        selector: "envelope".to_owned(),
                        field_ordinal: 0,
                    },
                    boon_semantic::SemanticStorageTypePathSegmentV1::ListItem,
                    boon_semantic::SemanticStorageTypePathSegmentV1::ObjectField {
                        selector: "body".to_owned(),
                        field_ordinal: 0,
                    },
                ],
                fixed_len: 8,
            }],
        };
        let mapped = map_storage_representation(&semantic, &dynamic, &fixed)
            .expect("fixed-BYTES representation joins by structural type segments");
        let MappedSemanticStorageRepresentation::CheckedFixedBytes { refinements } = mapped else {
            panic!("mapped representation lost fixed-BYTES refinements")
        };
        assert_eq!(refinements[0].fixed_len, 8);
        assert!(matches!(
            refinements[0].path.as_slice(),
            [
                MappedSemanticStorageTypePathSegment::ObjectField {
                    selector,
                    field_ordinal: 0,
                },
                MappedSemanticStorageTypePathSegment::ListItem,
                MappedSemanticStorageTypePathSegment::ObjectField {
                    selector: body,
                    field_ordinal: 0,
                },
            ] if selector == "envelope" && body == "body"
        ));

        let mut stale_length = semantic.clone();
        let boon_semantic::SemanticStorageRepresentationV1::CheckedFixedBytes { refinements } =
            &mut stale_length
        else {
            unreachable!()
        };
        refinements[0].fixed_len = 9;
        let error = map_storage_representation(&stale_length, &dynamic, &fixed).unwrap_err();
        assert!(error.contains("storage representation"), "{error}");

        let mut stale_path = semantic;
        let boon_semantic::SemanticStorageRepresentationV1::CheckedFixedBytes { refinements } =
            &mut stale_path
        else {
            unreachable!()
        };
        let ordinal = refinements[0]
            .path
            .iter_mut()
            .find_map(|segment| match segment {
                boon_semantic::SemanticStorageTypePathSegmentV1::ObjectField {
                    field_ordinal,
                    ..
                } => Some(field_ordinal),
                boon_semantic::SemanticStorageTypePathSegmentV1::ListItem => None,
            })
            .expect("fixture refinement has an object-field segment");
        *ordinal += 1;
        let error = map_storage_representation(&stale_path, &dynamic, &fixed).unwrap_err();
        assert!(error.contains("storage representation"), "{error}");
    }

    #[test]
    fn semantic_storage_join_preserves_structural_named_value_projections() {
        let parsed = boon_parser::parse_source(
            "semantic-storage-named-projection.bn",
            "group: [value: 1]\n",
        )
        .unwrap();
        let checked = boon_typecheck::check_program(&parsed)
            .program
            .expect("named-projection fixture typechecks");
        let semantic =
            boon_semantic::elaborate(checked, &[]).expect("named-projection fixture elaborates");
        assert!(
            semantic
                .scope_storage_graph()
                .named_values
                .iter()
                .all(|row| row.projection.is_empty()),
            "current exact-site table starts without synthetic projections"
        );
        let execution =
            map_semantic_execution(semantic.execution_graph(), semantic.resource_graph())
                .expect("named-projection execution maps");
        let resources = map_semantic_resources(
            semantic.execution_graph(),
            semantic.resource_graph(),
            &execution.id_map,
        )
        .expect("named-projection resources map");
        let reactive = map_semantic_reactive(
            semantic.execution_graph(),
            semantic.resource_graph(),
            semantic.reactive_graph(),
            &execution.id_map,
            &resources,
        )
        .expect("named-projection reactive graph maps");
        let mut lowering = semantic.lowering_contract().clone();
        let mut storage = semantic.scope_storage_graph().clone();
        let (row_index, parent, child) = storage
            .named_values
            .iter()
            .enumerate()
            .filter_map(|(row_index, row)| {
                let SemanticNamedValueStorageTargetV1::Field { field: parent, .. } = row.target
                else {
                    return None;
                };
                storage
                    .fields
                    .iter()
                    .find(|field| field.parent == Some(parent) && field.name == "value")
                    .map(|child| (row_index, parent, child.id))
            })
            .next()
            .expect("fixture has an object field with a structural child");
        let parent_type = storage.fields[parent.as_usize()].flow_type.ty.clone();
        let boon_typecheck::Type::Object(shape) = &parent_type else {
            panic!("projection parent is not an object")
        };
        let field_order = canonical_type_field_order(shape);
        let field_ordinal = field_order
            .iter()
            .position(|field| field == "value")
            .expect("value has a structural ordinal");
        let output_type = shape.fields["value"].clone();
        let child_field = &storage.fields[child.as_usize()];
        let expression = child_field.producer;
        let value = expression.map(|expression| {
            semantic_execution_expression(semantic.execution_graph(), expression)
                .expect("child producer exists")
                .value_id
        });
        let named_value = storage.named_values[row_index].named_value;
        let origin_ordinal = storage.named_values[row_index].origin_ordinal;
        lowering.metadata.named_value_types[named_value.as_usize()].origins[origin_ordinal]
            .checked
            .projection = vec!["value".to_owned()];
        storage.named_values[row_index].projection =
            vec![boon_semantic::SemanticStorageProjectionStepV1 {
                id: SemanticStorageProjectionId(0),
                ordinal: 0,
                selector: "value".to_owned(),
                field_ordinal,
                input_type: parent_type,
                output_type: output_type.clone(),
                storage_field: Some(child),
                expression,
                value,
            }];
        storage.named_values[row_index].flow_type.ty = output_type;
        storage.named_values[row_index].representation =
            boon_semantic::SemanticStorageRepresentationV1::Exact;
        let mapped = map_semantic_storage_join(
            semantic.execution_graph(),
            semantic.resource_graph(),
            semantic.reactive_graph(),
            &storage,
            &lowering,
            &execution.id_map,
            &resources,
            &reactive,
        )
        .expect("structural named-value projection maps without a path lookup");
        assert_eq!(
            mapped.named_values[row_index].projection[0].storage_field,
            Some(mapped.id_map.storage_field(child).unwrap())
        );

        let mut noncanonical = storage.clone();
        noncanonical.named_values[row_index].projection[0].id =
            SemanticStorageProjectionId(usize::MAX);
        let error = map_semantic_storage_join(
            semantic.execution_graph(),
            semantic.resource_graph(),
            semantic.reactive_graph(),
            &noncanonical,
            &lowering,
            &execution.id_map,
            &resources,
            &reactive,
        )
        .unwrap_err();
        assert!(error.contains("named-value projection"), "{error}");
        assert!(error.contains("not dense"), "{error}");

        let mut stale_ordinal = storage;
        stale_ordinal.named_values[row_index].projection[0].field_ordinal = usize::MAX;
        let error = map_semantic_storage_join(
            semantic.execution_graph(),
            semantic.resource_graph(),
            semantic.reactive_graph(),
            &stale_ordinal,
            &lowering,
            &execution.id_map,
            &resources,
            &reactive,
        )
        .unwrap_err();
        assert!(error.contains("structural field ordinal"), "{error}");
    }

    #[test]
    fn semantic_reactive_records_map_once_without_fabricating_contract_topology() {
        let parsed = boon_parser::parse_source(
            "semantic-reactive-mapping.bn",
            r#"
rows: LIST {
    [value: 1]
    [value: 2]
}
page: rows |> List/chunk(size: 1)
store: [
    pulse: SOURCE
    selected:
        False |> HOLD selected {
            pulse |> THEN { True }
        }
]
"#,
        )
        .unwrap();
        let checked = boon_typecheck::check_program(&parsed);
        assert!(
            !checked.report.has_errors(),
            "diagnostics: {:#?}",
            checked.report.diagnostics
        );
        let semantic = boon_semantic::elaborate(
            checked
                .program
                .expect("reactive fixture has checked program"),
            &[],
        )
        .expect("reactive fixture elaborates");
        let execution =
            map_semantic_execution(semantic.execution_graph(), semantic.resource_graph())
                .expect("semantic execution maps");
        let resources = map_semantic_resources(
            semantic.execution_graph(),
            semantic.resource_graph(),
            &execution.id_map,
        )
        .expect("semantic resources map");
        let reactive = map_semantic_reactive(
            semantic.execution_graph(),
            semantic.resource_graph(),
            semantic.reactive_graph(),
            &execution.id_map,
            &resources,
        )
        .expect("stable semantic reactive records map");
        reactive
            .validate_totality(
                semantic.reactive_graph(),
                semantic.resource_graph(),
                &execution.id_map,
            )
            .unwrap();

        assert_eq!(
            reactive.fields.len(),
            semantic.reactive_graph().fields.len()
        );
        assert_eq!(
            reactive.bindings.len(),
            semantic.reactive_graph().bindings.len()
        );
        assert_eq!(reactive.reads.len(), semantic.reactive_graph().reads.len());
        assert_eq!(
            reactive.list_mutations.len(),
            semantic.reactive_graph().list_mutations.len()
        );
        assert!(
            !reactive.state_update_arms.is_empty(),
            "fixture exercises exact trigger/state-arm mapping"
        );
        assert!(
            reactive
                .fields
                .iter()
                .enumerate()
                .all(|(index, field)| field.id == MappedReactiveFieldId(index))
        );
        assert!(
            reactive
                .trigger_arms
                .iter()
                .enumerate()
                .all(|(index, trigger)| trigger.id == MappedReactiveTriggerId(index))
        );

        let mut extra_trigger = semantic.reactive_graph().clone();
        let mut trigger = extra_trigger.trigger_arms[0].clone();
        trigger.id = SemanticTriggerArmId(extra_trigger.trigger_arms.len());
        extra_trigger.trigger_arms.push(trigger);
        let error = map_semantic_reactive(
            semantic.execution_graph(),
            semantic.resource_graph(),
            &extra_trigger,
            &execution.id_map,
            &resources,
        )
        .unwrap_err();
        assert!(error.contains("expected exact set"), "{error}");

        let mut dangling_trigger = semantic.reactive_graph().clone();
        dangling_trigger.state_update_arms[0].trigger = SemanticTriggerArmId(usize::MAX);
        let error = map_semantic_reactive(
            semantic.execution_graph(),
            semantic.resource_graph(),
            &dangling_trigger,
            &execution.id_map,
            &resources,
        )
        .unwrap_err();
        assert!(error.contains("references missing trigger"), "{error}");

        assert!(
            !semantic.reactive_graph().dependencies.is_empty(),
            "fixture exercises exact dependency closure"
        );
        let mut extra_dependency = semantic.reactive_graph().clone();
        let mut dependency = extra_dependency.dependencies[0].clone();
        dependency.id =
            boon_semantic::SemanticExternalDependencyId(extra_dependency.dependencies.len());
        extra_dependency.dependencies.push(dependency);
        let error = map_semantic_reactive(
            semantic.execution_graph(),
            semantic.resource_graph(),
            &extra_dependency,
            &execution.id_map,
            &resources,
        )
        .unwrap_err();
        assert!(error.contains("dependency closure"), "{error}");

        let mut dangling_dependency = semantic.reactive_graph().clone();
        dangling_dependency.dependencies[0].to = SemanticStateId(usize::MAX);
        let error = map_semantic_reactive(
            semantic.execution_graph(),
            semantic.resource_graph(),
            &dangling_dependency,
            &execution.id_map,
            &resources,
        )
        .unwrap_err();
        assert!(error.contains("dependency edge"), "{error}");

        let mapped_storage = map_semantic_storage_join(
            semantic.execution_graph(),
            semantic.resource_graph(),
            semantic.reactive_graph(),
            semantic.scope_storage_graph(),
            semantic.lowering_contract(),
            &execution.id_map,
            &resources,
            &reactive,
        )
        .expect("complete semantic storage topology joins mechanically");
        mapped_storage
            .validate_totality(
                semantic.scope_storage_graph(),
                semantic.reactive_graph(),
                &resources,
                &execution.id_map,
            )
            .unwrap();
        assert_eq!(
            mapped_storage.fields.len(),
            semantic.scope_storage_graph().fields.len()
        );
        assert_eq!(
            mapped_storage.bindings.len(),
            semantic.reactive_graph().bindings.len()
        );
        assert_eq!(
            mapped_storage.reads.len(),
            semantic.reactive_graph().reads.len()
        );
        assert_eq!(
            mapped_storage.named_values.len(),
            semantic.scope_storage_graph().named_values.len()
        );
        assert_eq!(
            mapped_storage.call_invocations.len(),
            semantic.reactive_graph().call_invocations.len()
        );
        assert!(
            !mapped_storage.derived_values.is_empty(),
            "fixture exercises final derived-value FieldId joins"
        );

        let map_storage = |storage: &SemanticScopeStorageGraphV1| {
            map_semantic_storage_join(
                semantic.execution_graph(),
                semantic.resource_graph(),
                semantic.reactive_graph(),
                storage,
                semantic.lowering_contract(),
                &execution.id_map,
                &resources,
                &reactive,
            )
        };

        let mut noncanonical_field = semantic.scope_storage_graph().clone();
        noncanonical_field.fields[0].id = SemanticStorageFieldId(usize::MAX);
        let error = map_storage(&noncanonical_field).unwrap_err();
        assert!(error.contains("storage field"), "{error}");
        assert!(error.contains("not dense"), "{error}");

        let mut broken_reactive_join = semantic.scope_storage_graph().clone();
        let reactive_field = broken_reactive_join
            .fields
            .iter_mut()
            .find(|field| field.reactive_field.is_some())
            .expect("fixture has a reactive storage field");
        reactive_field.reactive_field = None;
        let error = map_storage(&broken_reactive_join).unwrap_err();
        assert!(
            error.contains("reactive origin") || error.contains("no exact semantic storage field"),
            "{error}"
        );

        let mut missing_binding = semantic.scope_storage_graph().clone();
        missing_binding.bindings.pop();
        let error = map_storage(&missing_binding).unwrap_err();
        assert!(
            error.contains("no exact semantic storage binding"),
            "{error}"
        );

        let mut bad_source = semantic.scope_storage_graph().clone();
        bad_source
            .sources
            .first_mut()
            .expect("fixture has a storage source")
            .binding = SemanticBindingId(usize::MAX);
        let error = map_storage(&bad_source).unwrap_err();
        assert!(error.contains("storage source"), "{error}");

        let mut bad_named_target = semantic.scope_storage_graph().clone();
        bad_named_target
            .named_values
            .first_mut()
            .expect("fixture has named-value storage")
            .target_ordinal = usize::MAX;
        let error = map_storage(&bad_named_target).unwrap_err();
        assert!(error.contains("target ordinals"), "{error}");

        let mut stale_named_target = semantic.scope_storage_graph().clone();
        let target = &mut stale_named_target
            .named_values
            .iter_mut()
            .find(|value| {
                !matches!(
                    value.target,
                    SemanticNamedValueStorageTargetV1::DiagnosticOnly { .. }
                ) && {
                    let origin = &semantic.lowering_contract().metadata.named_value_types
                        [value.named_value.as_usize()]
                    .origins[value.origin_ordinal];
                    !origin.expressions.is_empty()
                        || !origin.bindings.is_empty()
                        || !origin.sources.is_empty()
                        || !origin.states.is_empty()
                        || !origin.lists.is_empty()
                }
            })
            .expect("fixture has an executable named-value target")
            .target;
        match target {
            SemanticNamedValueStorageTargetV1::Field { field, .. } => {
                *field = SemanticStorageFieldId(usize::MAX);
            }
            SemanticNamedValueStorageTargetV1::Source { binding, .. }
            | SemanticNamedValueStorageTargetV1::State { binding, .. }
            | SemanticNamedValueStorageTargetV1::List { binding, .. } => {
                *binding = SemanticBindingId(usize::MAX);
            }
            SemanticNamedValueStorageTargetV1::Value { expression, .. } => {
                *expression = SemanticExprId(usize::MAX);
            }
            SemanticNamedValueStorageTargetV1::DiagnosticOnly { .. } => {
                unreachable!("filtered executable target")
            }
        };
        let error = map_storage(&stale_named_target).unwrap_err();
        assert!(error.contains("named value"), "{error}");

        let mut bad_representation = semantic.scope_storage_graph().clone();
        bad_representation
            .named_values
            .first_mut()
            .expect("fixture has named-value storage")
            .representation = boon_semantic::SemanticStorageRepresentationV1::CheckedFixedBytes {
            refinements: vec![boon_semantic::SemanticStorageFixedBytesRefinementV1 {
                path: Vec::new(),
                fixed_len: 1,
            }],
        };
        let error = map_storage(&bad_representation).unwrap_err();
        assert!(error.contains("storage representation"), "{error}");

        let mut bad_checked_site = mapped_storage.clone();
        bad_checked_site.named_values[0].checked_statement =
            boon_typecheck::CheckedStatementId(u32::MAX);
        let error = bad_checked_site
            .validate_totality(
                semantic.scope_storage_graph(),
                semantic.reactive_graph(),
                &resources,
                &execution.id_map,
            )
            .unwrap_err();
        assert!(error.contains("mapped named-value target"), "{error}");

        let mut bad_mapped_field = mapped_storage.clone();
        bad_mapped_field.fields[0].id = FieldId(usize::MAX);
        let error = bad_mapped_field
            .validate_totality(
                semantic.scope_storage_graph(),
                semantic.reactive_graph(),
                &resources,
                &execution.id_map,
            )
            .unwrap_err();
        assert!(error.contains("mapped storage field"), "{error}");

        let mut bad_derived = mapped_storage.clone();
        bad_derived
            .derived_values
            .first_mut()
            .expect("fixture has a derived value")
            .id = FieldId(usize::MAX);
        let error = bad_derived
            .validate_totality(
                semantic.scope_storage_graph(),
                semantic.reactive_graph(),
                &resources,
                &execution.id_map,
            )
            .unwrap_err();
        assert!(error.contains("mapped derived value"), "{error}");

        let mut bad_mapped_read = mapped_storage;
        bad_mapped_read
            .reads
            .first_mut()
            .expect("fixture has a staged read")
            .id = ErasedReadId(usize::MAX);
        let error = bad_mapped_read
            .validate_totality(
                semantic.scope_storage_graph(),
                semantic.reactive_graph(),
                &resources,
                &execution.id_map,
            )
            .unwrap_err();
        assert!(error.contains("mapped storage read"), "{error}");
    }
}
