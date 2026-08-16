use crate::program_core;
use crate::{
    OutCallInstanceId, ProducerFunctionId, SemanticBindingId, SemanticBindingTargetV1,
    SemanticBlockBinding, SemanticCall, SemanticCallArgument, SemanticCallContextId,
    SemanticCallId, SemanticCallable, SemanticCallableId, SemanticCallableKind,
    SemanticContextualMaterialization, SemanticContextualOperationKind, SemanticContextualOrderKey,
    SemanticContextualRowPredecessor, SemanticDependencyTargetV1, SemanticDependencyTimingV1,
    SemanticDerivedValueKindV1, SemanticEventCauseV1, SemanticExecutionImageColumnsV1,
    SemanticExprId, SemanticExpression, SemanticExpressionKind, SemanticFieldId, SemanticFunction,
    SemanticFunctionParameter, SemanticInitialValueV1, SemanticListId, SemanticListInitializerV1,
    SemanticListKeyPolicyV1, SemanticListMutationKindV1, SemanticListProjectionKindV1,
    SemanticLocalBindingId, SemanticLoweringContractV2, SemanticMaterializationId,
    SemanticMaterializationLocalId, SemanticMaterializationResultKind, SemanticParameterId,
    SemanticPatternBinding, SemanticReactiveGraphV1, SemanticReadId, SemanticReadTargetV1,
    SemanticRecordField, SemanticResourceGraphV2, SemanticRoot, SemanticRowBinding,
    SemanticRowScopeId, SemanticScopeStorageGraphV1, SemanticSelectArm, SemanticSourceDef,
    SemanticSourceId, SemanticSourceOrigin, SemanticSourceRead, SemanticStateDef, SemanticStateId,
    SemanticStatement, SemanticStatementId, SemanticStatementKind, SemanticStorageBindingTargetV1,
    SemanticStorageExternalReferenceId, SemanticStorageExternalReferenceKindV1,
    SemanticStorageFieldId, SemanticStorageFieldOriginV1, SemanticStorageFieldRoleV1,
    SemanticStorageLocalMemberForwardingV1, SemanticStorageLocalMemberTargetV1,
    SemanticTextSegment, SemanticTriggerArmId, SemanticValueId, SemanticValueListAuthorityId,
    SemanticValueMember, SemanticValueOrigin, SemanticValueProvenance, StaticOwnerDef,
    StaticOwnerId,
};
use boon_checked::{
    CheckedExternalDeclarationIdentityV1, CheckedExternalDeclarationKind, DeclId, FlowMode,
    FlowType,
};
use program_core::{
    ContextualMaterialization, ContextualOperationKind, ContextualOrderKey,
    ContextualRowPredecessor, DependencyEdge, DerivedValue, DerivedValueKind, ErasedBinding,
    ErasedBindingId, ErasedBindingTarget, ErasedDependencyTiming, ErasedFieldDef, ErasedFieldRole,
    ErasedLocalCapture, ErasedLocalDef, ErasedLocalMember, ErasedLocalMemberForwarding,
    ErasedLocalMemberTarget, ErasedOwnerDef, ErasedReadId, ErasedRowBinding,
    ErasedRowSourceProjection, ErasedRowValue, ErasedSourceDef, ErasedSourceOrigin,
    ErasedTemporalBoundary, EventCause, ExecutableBlockBinding, ExecutableCallArgument,
    ExecutableCallContextId, ExecutableCallOccurrence, ExecutableCallableKind, ExecutableExprId,
    ExecutableExpression, ExecutableExpressionKind, ExecutableFunction,
    ExecutableFunctionParameter, ExecutableLocalBindingId, ExecutableOrdinaryFunction,
    ExecutableParameterId, ExecutablePatternBinding, ExecutableProgram, ExecutableRecordField,
    ExecutableRoot, ExecutableSelectArm, ExecutableSourceDef, ExecutableSourceId,
    ExecutableSourceOrigin, ExecutableStateDef, ExecutableStateId, ExecutableStatement,
    ExecutableStatementId, ExecutableStatementKind, ExecutableTextSegment, ExecutableValueMember,
    ExecutableValueOrigin, ExecutableValueProvenance, ExprId, FieldId, FunctionId, InitialValue,
    ListId, ListInitialRecord, ListInitializer, ListInitializerInput, ListMemory, ListMutation,
    ListMutationKind, ListProjection, ListProjectionKind, ListRowInitialField,
    MaterializationLocalId, MaterializationResultKind, ProducerFunctionArgument,
    ProducerFunctionInstance, ScopeId, SourceId, SourcePayloadDescriptor, SourcePayloadField,
    SourcePayloadSchema, SourcePort, StateCell, StateId, StateUpdateArm, TriggerOwnedArm,
};
use std::collections::{BTreeMap, BTreeSet};

type ExecutableCallInstanceMap = BTreeMap<OutCallInstanceId, usize>;
type ExecutableCallContextMap = BTreeMap<SemanticCallContextId, ExecutableCallContextId>;
type AllocatedCallIdentities = (ExecutableCallInstanceMap, ExecutableCallContextMap);
type RuntimeSourceMap = BTreeMap<SemanticSourceId, SourceId>;
type RuntimeStateMap = BTreeMap<SemanticStateId, StateId>;
type AllocatedRuntimeResourceIds = (RuntimeSourceMap, RuntimeStateMap);

fn producer_identity_text(identity: [u8; 32]) -> String {
    identity.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn hidden_key_type(name: &str) -> String {
    let singular = name
        .strip_suffix("ies")
        .map(|prefix| format!("{prefix}y"))
        .or_else(|| name.strip_suffix('s').map(ToOwned::to_owned))
        .unwrap_or_else(|| name.to_owned());
    let mut output = String::new();
    let mut uppercase_next = true;
    for ch in singular.chars() {
        if ch == '_' || ch == '-' {
            uppercase_next = true;
        } else if uppercase_next {
            output.push(ch.to_ascii_uppercase());
            uppercase_next = false;
        } else {
            output.push(ch);
        }
    }
    output.push_str("Key");
    output
}

fn semantic_data_type(value: &boon_checked::Type) -> program_core::SemanticDataType {
    match value {
        boon_checked::Type::Text => program_core::SemanticDataType::Text,
        boon_checked::Type::Number => program_core::SemanticDataType::Number,
        boon_checked::Type::Bytes(boon_checked::BytesType::Dynamic) => {
            program_core::SemanticDataType::Bytes { fixed_len: None }
        }
        boon_checked::Type::Bytes(boon_checked::BytesType::Fixed(fixed_len)) => {
            program_core::SemanticDataType::Bytes {
                fixed_len: Some(*fixed_len),
            }
        }
        boon_checked::Type::Bits { width } => {
            program_core::SemanticDataType::Bits { width: *width }
        }
        boon_checked::Type::Absent => program_core::SemanticDataType::Unknown {
            reason: "private absence is not semantic data".to_owned(),
        },
        boon_checked::Type::VariantSet(variants) => {
            let mut variants = variants
                .iter()
                .map(|variant| match variant {
                    boon_checked::Variant::Tag(tag) => program_core::SemanticVariantType {
                        tag: tag.clone(),
                        fields: Vec::new(),
                        open: false,
                    },
                    boon_checked::Variant::Tagged { tag, fields } => {
                        program_core::SemanticVariantType {
                            tag: tag.clone(),
                            fields: semantic_type_fields(&fields.fields),
                            open: fields.open,
                        }
                    }
                })
                .collect::<Vec<_>>();
            variants.sort_by(|left, right| left.tag.cmp(&right.tag));
            program_core::SemanticDataType::Variant { variants }
        }
        boon_checked::Type::Object(shape) => program_core::SemanticDataType::Record {
            fields: semantic_type_fields(&shape.fields),
            open: shape.open,
        },
        boon_checked::Type::List(item) => program_core::SemanticDataType::List {
            item: Box::new(semantic_data_type(item)),
        },
        boon_checked::Type::Map { key, value } => program_core::SemanticDataType::Map {
            key: Box::new(semantic_data_type(key)),
            value: Box::new(semantic_data_type(value)),
        },
        boon_checked::Type::Set(item) => program_core::SemanticDataType::Set {
            item: Box::new(semantic_data_type(item)),
        },
        boon_checked::Type::Union(members) => program_core::SemanticDataType::Union {
            members: members.iter().map(semantic_data_type).collect(),
        },
        boon_checked::Type::Function { .. } => program_core::SemanticDataType::Unknown {
            reason: "function values are not semantic memory data".to_owned(),
        },
        boon_checked::Type::RenderContract => program_core::SemanticDataType::Unknown {
            reason: "render contracts are not semantic memory data".to_owned(),
        },
        boon_checked::Type::UnresolvedShape { reason } => program_core::SemanticDataType::Unknown {
            reason: reason.clone(),
        },
        boon_checked::Type::Var(var) => program_core::SemanticDataType::Unknown {
            reason: format!("unresolved type variable {}", var.0),
        },
        boon_checked::Type::Unknown => program_core::SemanticDataType::Unknown {
            reason: "unknown type".to_owned(),
        },
    }
}

fn semantic_type_fields(
    fields: &BTreeMap<String, boon_checked::Type>,
) -> Vec<program_core::SemanticTypeField> {
    fields
        .iter()
        .map(|(name, data_type)| program_core::SemanticTypeField {
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
/// [`program_core::FieldId`]: the lowering contract's complete storage-field domain
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
/// both currently use `usize`. Canonical identity domains retain one checked
/// bound instead of an allocated `0..N` mirror; genuinely non-identity domains
/// retain explicit maps. Every conversion below must go through this table so
/// a later executable allocator can change without reopening semantic discovery
/// in `boon_ir`.
#[derive(Clone, Debug)]
pub(super) struct SemanticToExecutableMap {
    expression_count: usize,
    statement_count: usize,
    lexical_scope_count: usize,
    source_count: usize,
    state_count: usize,
    callable_count: usize,
    call_expressions: Vec<Vec<ExecutableExprId>>,
    producer_functions: BTreeMap<ProducerFunctionId, FunctionId>,
    materialization_count: usize,
    local_bindings: BTreeMap<SemanticLocalBindingId, ExecutableLocalBindingId>,
    call_instances: BTreeMap<OutCallInstanceId, usize>,
    call_contexts: BTreeMap<SemanticCallContextId, ExecutableCallContextId>,
    materialization_locals:
        BTreeMap<(StaticOwnerId, SemanticMaterializationLocalId), MaterializationLocalId>,
    list_count: usize,
    row_scope_count: usize,
    value_list_authority_count: usize,
    runtime_sources: BTreeMap<SemanticSourceId, SourceId>,
    /// Exact event-valued external-read occurrences mapped to the distributed
    /// ingress SOURCE allocated after the local semantic source domain.
    external_event_sources: BTreeMap<SemanticExprId, SourceId>,
    /// One canonical runtime path per distinct distributed ingress SOURCE.
    external_event_source_paths: BTreeMap<SourceId, String>,
    runtime_states: BTreeMap<SemanticStateId, StateId>,
}

#[derive(Debug)]
pub(super) struct MappedSemanticExecution {
    pub executable: ExecutableProgram,
    pub materializations: Vec<ContextualMaterialization>,
    pub static_owners: Vec<StaticOwnerDef>,
    pub id_map: SemanticToExecutableMap,
}

pub(crate) struct CanonicalProgramCoreBuildV2 {
    pub(crate) core: program_core::CanonicalProgramCoreV2,
    pub(crate) execution_handoff: crate::semantic_image::ExecutionImageHandoffV3,
}

#[derive(Clone, Debug)]
pub(super) struct MappedSemanticResources {
    pub lists: Vec<ListMemory>,
    pub sources: Vec<SourcePort>,
    pub state_cells: Vec<StateCell>,
    pub list_projections: Vec<ListProjection>,
}

/// Exact mechanically mapped field identity. Parent/role topology remains a
/// lowering-contract responsibility, so this is intentionally not an
/// [`program_core::ErasedFieldDef`].
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
    pub result_type: FlowType,
    pub effect: boon_checked::CheckedEffectSummary,
    pub result_field: MappedReactiveFieldId,
    pub result_path: String,
    pub root: ExecutableExprId,
    pub mode: crate::ProducerMaterializationMode,
    pub invocation_source: Option<SourceId>,
    pub arguments: Vec<ProducerFunctionArgument>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct MappedSemanticTriggerArm {
    pub id: MappedReactiveTriggerId,
    pub cause: EventCause,
    pub gate_checked_expression: boon_checked::CheckedExprId,
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
    pub producer: ExecutableExprId,
    pub path: String,
    pub kind: DerivedValueKind,
    pub state_backing: Option<StateId>,
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
    field_count: usize,
    binding_count: usize,
    read_count: usize,
    trigger_arm_count: usize,
    state_update_arm_count: usize,
    list_mutation_count: usize,
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
    id_map: SemanticReactiveToMappedMap,
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
    pub checked_expression: boon_checked::CheckedExprId,
    pub owner: Option<StaticOwnerId>,
    pub operation: String,
    pub state_update_arms: Vec<StateUpdateArm>,
    pub transient_result: Option<usize>,
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

#[derive(Clone, Debug)]
struct SemanticStorageToErasedMap {
    storage_field_count: usize,
    reactive_fields: Vec<FieldId>,
    binding_count: usize,
    read_count: usize,
    external_reference_count: usize,
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
    pub producer_function_instances: Vec<ProducerFunctionInstance>,
    pub derived_values: Vec<DerivedValue>,
    pub trigger_arms: Vec<TriggerOwnedArm>,
    pub state_update_arms: Vec<StateUpdateArm>,
    pub list_mutations: Vec<ListMutation>,
    pub dependencies: Vec<DependencyEdge>,
    id_map: SemanticStorageToErasedMap,
}

impl SemanticToExecutableMap {
    fn allocate_with_external_events(
        graph: &SemanticExecutionImageColumnsV1,
        resources: &SemanticResourceGraphV2,
        external_event_identities: &[CheckedExternalDeclarationIdentityV1],
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

        let call_expressions = allocate_call_expressions(graph)?;
        let mut producer_functions = BTreeMap::new();
        for (index, function) in graph.functions.iter().enumerate() {
            // A semantic callable is a syntax-wide definition. A producer
            // function is one concrete materialized occurrence and therefore
            // needs its own executable identity even when several occurrences
            // share the same callable.
            exact_dense_index(
                function.callable.as_usize(),
                graph.callables.len(),
                "semantic producer callable",
                function.callable,
            )?;
            let executable = FunctionId(index);
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
        let (external_event_sources, external_event_source_paths) =
            allocate_external_event_source_ids(
                graph,
                runtime_sources.len(),
                external_event_identities,
            )?;

        let allocated = Self {
            expression_count: graph.expressions.len(),
            statement_count: graph.statements.len(),
            lexical_scope_count: graph.scopes.len(),
            source_count: graph.sources.len(),
            state_count: graph.states.len(),
            callable_count: graph.callables.len(),
            call_expressions,
            producer_functions,
            materialization_count: graph.materializations.len(),
            local_bindings,
            call_instances,
            call_contexts,
            materialization_locals,
            list_count: resources.lists.len(),
            row_scope_count: resources.row_scopes.len(),
            value_list_authority_count: resources.value_list_authorities.len(),
            runtime_sources,
            external_event_sources,
            external_event_source_paths,
            runtime_states,
        };
        allocated.validate_allocation_bijections()?;
        Ok(allocated)
    }

    fn validate_allocation_bijections(&self) -> Result<(), String> {
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
        require_unique_allocation(
            self.runtime_sources
                .values()
                .copied()
                .chain(self.external_event_source_paths.keys().copied()),
            self.runtime_sources.len() + self.external_event_source_paths.len(),
            "runtime source",
        )?;
        require_unique_allocation(
            self.runtime_states.values().copied(),
            self.runtime_states.len(),
            "runtime state",
        )
    }

    pub(super) fn expression(&self, id: SemanticExprId) -> Result<ExecutableExprId, String> {
        exact_dense_index(
            id.as_usize(),
            self.expression_count,
            "semantic expression",
            id,
        )
        .map(ExecutableExprId)
    }

    /// V1 has exactly one value per semantic expression, so the executable
    /// value handle is the expression that produces it. The checked-domain
    /// lookup never reinterprets `SemanticValueId` without validating its bound.
    pub(super) fn value(&self, id: SemanticValueId) -> Result<ExecutableExprId, String> {
        exact_dense_index(id.as_usize(), self.expression_count, "semantic value", id)
            .map(ExecutableExprId)
    }

    fn statement(&self, id: SemanticStatementId) -> Result<ExecutableStatementId, String> {
        exact_dense_index(
            id.as_usize(),
            self.statement_count,
            "semantic statement",
            id,
        )
        .map(ExecutableStatementId)
    }

    pub(super) fn lexical_scope(
        &self,
        id: crate::SemanticScopeId,
    ) -> Result<ExecutableLexicalScopeId, String> {
        exact_dense_index(
            id.as_usize(),
            self.lexical_scope_count,
            "semantic lexical scope",
            id,
        )
        .map(ExecutableLexicalScopeId)
    }

    fn source(&self, id: SemanticSourceId) -> Result<ExecutableSourceId, String> {
        exact_dense_index(id.as_usize(), self.source_count, "semantic source", id)
            .map(ExecutableSourceId)
    }

    fn state(&self, id: SemanticStateId) -> Result<ExecutableStateId, String> {
        exact_dense_index(id.as_usize(), self.state_count, "semantic state", id)
            .map(ExecutableStateId)
    }

    pub(super) fn callable(&self, id: SemanticCallableId) -> Result<FunctionId, String> {
        exact_dense_index(id.as_usize(), self.callable_count, "semantic callable", id)
            .map(FunctionId)
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

    pub(super) fn producer_function(&self, id: ProducerFunctionId) -> Result<FunctionId, String> {
        self.producer_functions
            .get(&id)
            .copied()
            .ok_or_else(|| format!("semantic producer function {id} has no executable mapping"))
    }

    fn materialization(&self, id: SemanticMaterializationId) -> Result<usize, String> {
        exact_dense_index(
            id.as_usize(),
            self.materialization_count,
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
        exact_dense_index(id.as_usize(), self.list_count, "semantic list", id).map(ListId)
    }

    fn row_scope(&self, id: SemanticRowScopeId) -> Result<ScopeId, String> {
        exact_dense_index(
            id.as_usize(),
            self.row_scope_count,
            "semantic row scope",
            id,
        )
        .map(ScopeId)
    }

    fn value_list_authority(&self, id: SemanticValueListAuthorityId) -> Result<(), String> {
        exact_dense_index(
            id.as_usize(),
            self.value_list_authority_count,
            "semantic value-list authority",
            id,
        )
        .map(|_| ())
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
            .ok_or_else(|| format!("semantic runtime source {id} has no executable mapping"))
    }

    fn external_event_source(&self, expression: SemanticExprId) -> Result<SourceId, String> {
        self.external_event_sources
            .get(&expression)
            .copied()
            .ok_or_else(|| {
                format!(
                    "semantic external event expression {expression} has no executable ingress SOURCE mapping"
                )
            })
    }

    fn runtime_state(&self, id: SemanticStateId) -> Result<StateId, String> {
        self.runtime_states
            .get(&id)
            .copied()
            .ok_or_else(|| format!("semantic runtime state {id} has no executable mapping"))
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

fn allocate_call_expressions(
    graph: &SemanticExecutionImageColumnsV1,
) -> Result<Vec<Vec<ExecutableExprId>>, String> {
    let mut allocated = vec![Vec::new(); graph.calls.len()];
    for expression in &graph.expressions {
        let SemanticExpressionKind::Call { call, .. } = &expression.kind else {
            continue;
        };
        let executable = exact_dense_index(
            expression.id.as_usize(),
            graph.expressions.len(),
            "semantic call expression",
            expression.id,
        )
        .map(ExecutableExprId)?;
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

fn semantic_callable(
    graph: &SemanticExecutionImageColumnsV1,
    id: SemanticCallableId,
) -> Result<&SemanticCallable, String> {
    graph
        .callables
        .get(id.as_usize())
        .filter(|callable| callable.id == id)
        .ok_or_else(|| format!("missing semantic callable {id}"))
}

fn semantic_call(
    graph: &SemanticExecutionImageColumnsV1,
    id: SemanticCallId,
) -> Result<&SemanticCall, String> {
    graph
        .calls
        .get(id.as_usize())
        .filter(|call| call.id == id)
        .ok_or_else(|| format!("missing semantic call {id}"))
}

fn allocate_local_bindings(
    graph: &SemanticExecutionImageColumnsV1,
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
    graph: &SemanticExecutionImageColumnsV1,
) -> Result<AllocatedCallIdentities, String> {
    let mut call_instances = BTreeMap::new();
    let mut context_definitions = BTreeSet::new();
    for (index, occurrence) in graph.call_occurrences.iter().enumerate() {
        if occurrence.id.as_usize() != index {
            return Err(format!(
                "semantic call occurrence {} is noncanonical at allocation index {index}",
                occurrence.id
            ));
        }
        if let Some(parent) = occurrence.parent
            && parent.as_usize() >= index
        {
            return Err(format!(
                "semantic call occurrence {} has nonpreceding parent {parent}",
                occurrence.id
            ));
        }
        if occurrence.call.is_none() && !occurrence.context_ordinals.is_empty() {
            return Err(format!(
                "synthetic call occurrence {} owns checked-call contexts",
                occurrence.id
            ));
        }
        if let Some(call) = occurrence.call {
            semantic_call(graph, call)?;
        }
        let mut ordinals = BTreeSet::new();
        for ordinal in occurrence.context_ordinals.iter().copied() {
            if !ordinals.insert(ordinal) {
                return Err(format!(
                    "semantic call occurrence {} repeats context ordinal {ordinal}",
                    occurrence.id
                ));
            }
            context_definitions.insert(SemanticCallContextId {
                call_instance: occurrence.id,
                ordinal,
            });
        }
        call_instances.insert(occurrence.id, index);
    }

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
                let Some(instance) = *instance else {
                    if !contexts.is_empty() {
                        return Err(format!(
                            "semantic call expression {} has contexts without a concrete OUT instance",
                            expression.id
                        ));
                    }
                    continue;
                };
                let occurrence = graph
                    .call_occurrences
                    .get(instance.as_usize())
                    .filter(|occurrence| occurrence.id == instance)
                    .ok_or_else(|| {
                        format!(
                            "semantic call expression {} references missing occurrence {instance}",
                            expression.id
                        )
                    })?;
                if occurrence.call != Some(*call) {
                    return Err(format!(
                        "semantic call expression {} call {call} differs from occurrence {instance} call {:?}",
                        expression.id, occurrence.call
                    ));
                }
                let call_definition = semantic_call(graph, *call)?;
                if call_definition.callable != *callable {
                    return Err(format!(
                        "semantic call expression {} callable {callable} differs from call {call} callable {}",
                        expression.id, call_definition.callable
                    ));
                }
                let mut expression_contexts = BTreeSet::new();
                for context in contexts {
                    if context.call_instance != instance {
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
                }
                let expected_contexts = occurrence
                    .context_ordinals
                    .iter()
                    .copied()
                    .map(|ordinal| SemanticCallContextId {
                        call_instance: instance,
                        ordinal,
                    })
                    .collect::<BTreeSet<_>>();
                if expression_contexts != expected_contexts {
                    return Err(format!(
                        "semantic call expression {} contexts differ from occurrence {instance}",
                        expression.id
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
    graph: &SemanticExecutionImageColumnsV1,
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
    graph: &SemanticExecutionImageColumnsV1,
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
    graph: &SemanticExecutionImageColumnsV1,
    resources: &SemanticResourceGraphV2,
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

fn allocate_external_event_source_ids(
    graph: &SemanticExecutionImageColumnsV1,
    local_source_count: usize,
    external_event_identities: &[CheckedExternalDeclarationIdentityV1],
) -> Result<
    (
        BTreeMap<SemanticExprId, SourceId>,
        BTreeMap<SourceId, String>,
    ),
    String,
> {
    let external_event_identities = external_event_identities
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    let mut events_by_path =
        BTreeMap::<String, (CheckedExternalDeclarationIdentityV1, Vec<SemanticExprId>)>::new();
    for expression in &graph.expressions {
        let SemanticExpressionKind::ExternalRead {
            canonical_path,
            external_identity,
        } = &expression.kind
        else {
            continue;
        };
        if !matches!(
            expression.flow_type.mode,
            FlowMode::TickPresent | FlowMode::PresentOrAbsent
        ) {
            continue;
        }
        let Some(identity) = *external_identity else {
            continue;
        };
        if !external_event_identities.contains(&identity) {
            continue;
        }
        if identity.kind != CheckedExternalDeclarationKind::Value {
            return Err(format!(
                "event-valued semantic external read `{canonical_path}` expression {} carries a non-value declaration identity",
                expression.id
            ));
        }
        match events_by_path.entry(canonical_path.clone()) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert((identity, vec![expression.id]));
            }
            std::collections::btree_map::Entry::Occupied(mut entry) => {
                if entry.get().0 != identity {
                    return Err(format!(
                        "event-valued semantic external path `{canonical_path}` resolves to multiple sealed declaration identities"
                    ));
                }
                entry.get_mut().1.push(expression.id);
            }
        }
    }
    let matched_identities = events_by_path
        .values()
        .map(|(identity, _)| *identity)
        .collect::<BTreeSet<_>>();
    if matched_identities != external_event_identities {
        return Err(format!(
            "semantic external-event SOURCE allocation matched identities {matched_identities:?}, expected {external_event_identities:?}"
        ));
    }

    let mut by_expression = BTreeMap::new();
    let mut paths = BTreeMap::new();
    for (ordinal, (canonical_path, (_, expressions))) in events_by_path.into_iter().enumerate() {
        let source = SourceId(
            local_source_count
                .checked_add(ordinal)
                .ok_or_else(|| "distributed ingress SOURCE IDs exhausted".to_owned())?,
        );
        let runtime_path = program_core::distributed_event_source_path(&canonical_path);
        if paths.insert(source, runtime_path).is_some() {
            return Err(format!(
                "distributed ingress SOURCE {source} was allocated more than once"
            ));
        }
        for expression in expressions {
            if by_expression.insert(expression, source).is_some() {
                return Err(format!(
                    "event-valued semantic external expression {expression} was allocated more than once"
                ));
            }
        }
    }
    Ok((by_expression, paths))
}

pub(super) fn map_semantic_execution_with_reactive(
    graph: &SemanticExecutionImageColumnsV1,
    resources: &SemanticResourceGraphV2,
    reactive: &SemanticReactiveGraphV1,
    receipts: &mut crate::semantic_image::ExecutionReceiptPublisherV3<'_>,
) -> Result<MappedSemanticExecution, String> {
    map_semantic_execution_with_external_events(
        graph,
        resources,
        &reactive.external_event_identities,
        receipts,
    )
}

fn map_semantic_execution_with_external_events(
    graph: &SemanticExecutionImageColumnsV1,
    resources: &SemanticResourceGraphV2,
    external_event_identities: &[CheckedExternalDeclarationIdentityV1],
    receipts: &mut crate::semantic_image::ExecutionReceiptPublisherV3<'_>,
) -> Result<MappedSemanticExecution, String> {
    let id_map = SemanticToExecutableMap::allocate_with_external_events(
        graph,
        resources,
        external_event_identities,
    )?;
    receipts.publish_scopes(graph)?;
    let mut expressions = Vec::with_capacity(graph.expressions.len());
    for semantic in &graph.expressions {
        let executable = map_expression(graph, &id_map, semantic)?;
        if executable.id.as_usize() != semantic.id.as_usize() {
            return Err(format!(
                "execution expression {} maps to non-dense executable {}",
                semantic.id, executable.id
            ));
        }
        receipts.publish_expression(graph, semantic, &executable)?;
        expressions.push(executable);
    }
    let mut statements = Vec::with_capacity(graph.statements.len());
    for semantic in &graph.statements {
        let executable = map_statement(&id_map, semantic)?;
        if executable.id.as_usize() != semantic.id.as_usize() {
            return Err(format!(
                "execution statement {} maps to non-dense executable {}",
                semantic.id, executable.id
            ));
        }
        receipts.publish_statement(graph, semantic, &executable)?;
        statements.push(executable);
    }
    receipts.publish_callables_and_calls(graph)?;

    let mut call_occurrences = Vec::with_capacity(graph.call_occurrences.len());
    for semantic in &graph.call_occurrences {
        let executable = ExecutableCallOccurrence {
            id: id_map.call_instance(semantic.id)?,
            parent: semantic
                .parent
                .map(|parent| id_map.call_instance(parent))
                .transpose()?,
            checked_call: semantic
                .call
                .map(|call| semantic_call(graph, call).map(|call| call.checked_call))
                .transpose()?,
            context_ordinals: semantic.context_ordinals.clone(),
        };
        if executable.id != semantic.id.as_usize() {
            return Err(format!(
                "call occurrence {} maps to executable {}",
                semantic.id, executable.id
            ));
        }
        receipts.publish_call_occurrence(graph, semantic, &executable)?;
        call_occurrences.push(executable);
    }

    let mut sources = Vec::with_capacity(graph.sources.len());
    for semantic in &graph.sources {
        let executable = map_source(&id_map, semantic)?;
        receipts.publish_source(semantic, &executable)?;
        sources.push(executable);
    }
    let mut states = Vec::with_capacity(graph.states.len());
    for semantic in &graph.states {
        let executable = map_state(&id_map, semantic)?;
        receipts.publish_state(semantic, &executable)?;
        states.push(executable);
    }
    let mut roots = Vec::with_capacity(graph.roots.len());
    for semantic in &graph.roots {
        let executable = map_root(graph, &id_map, semantic)?;
        receipts.publish_root(semantic, &executable)?;
        roots.push(executable);
    }
    let mut functions = Vec::with_capacity(graph.functions.len());
    for (index, semantic) in graph.functions.iter().enumerate() {
        let executable = map_function(graph, &id_map, semantic)?;
        receipts.publish_function(graph, semantic, &executable, index)?;
        functions.push(executable);
    }
    let ordinary_functions = graph
        .callables
        .iter()
        .filter(|callable| callable.semantic_root.is_some())
        .map(|callable| map_ordinary_function(&id_map, callable))
        .collect::<Result<Vec<_>, _>>()?;
    let mut materializations = Vec::with_capacity(graph.materializations.len());
    for semantic in &graph.materializations {
        let binding = resources
            .materialization_binding(semantic.id)
            .ok_or_else(|| {
                format!(
                    "semantic materialization {} has no resource binding",
                    semantic.id
                )
            })?;
        let executable = map_materialization(&id_map, semantic, binding)?;
        receipts.publish_materialization(semantic, &executable)?;
        materializations.push(executable);
    }
    let static_owners = graph
        .static_owners
        .iter()
        .map(|owner| StaticOwnerDef {
            id: owner.id,
            parent: owner.parent,
            child_ordinal: owner.child_ordinal,
        })
        .collect::<Vec<_>>();

    Ok(MappedSemanticExecution {
        executable: ExecutableProgram {
            expressions,
            statements,
            sources,
            states,
            roots,
            functions,
            ordinary_functions,
            call_occurrences,
        },
        materializations,
        static_owners,
        id_map,
    })
}

pub(super) fn map_semantic_resources(
    execution: &SemanticExecutionImageColumnsV1,
    graph: &SemanticResourceGraphV2,
    ids: &SemanticToExecutableMap,
) -> Result<MappedSemanticResources, String> {
    for authority in &graph.value_list_authorities {
        ids.value_list_authority(authority.id)?;
        ids.statement(authority.statement)?;
        ids.expression(authority.producer)?;
        match &authority.origin {
            crate::SemanticListResourceOriginV1::CheckedLiteral { .. } => {}
            crate::SemanticListResourceOriginV1::Derived {
                statement,
                producer,
            } => {
                ids.statement(*statement)?;
                ids.expression(*producer)?;
            }
        }
        validate_initializer_references(ids, &authority.initializer)?;
    }
    let lists = graph
        .lists
        .iter()
        .map(|list| {
            ids.statement(list.statement)?;
            ids.expression(list.producer)?;
            match &list.origin {
                crate::SemanticListResourceOriginV1::CheckedLiteral { .. } => {}
                crate::SemanticListResourceOriginV1::Derived {
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
                    (has_generation, hidden_key_type(&list.semantic_path))
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
                initializer_inputs: Vec::new(),
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let mut sources = graph
        .sources
        .iter()
        .map(|source| map_source_resource(execution, ids, source))
        .collect::<Result<Vec<_>, String>>()?;
    for (source, path) in &ids.external_event_source_paths {
        if source.as_usize() != sources.len() {
            return Err(format!(
                "distributed ingress SOURCE {source} is not dense after {} local/previous sources",
                sources.len()
            ));
        }
        sources.push(SourcePort {
            id: *source,
            path: path.clone(),
            binding_path: path.clone(),
            executable_source_id: None,
            static_owner: None,
            source_expr_id: None,
            source_line: 0,
            scoped: false,
            scope_id: None,
            interval_ms: None,
            payload_schema: SourcePayloadSchema {
                fields: Vec::new(),
                typed_fields: Vec::new(),
            },
        });
    }
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
        lists,
        sources,
        state_cells,
        list_projections,
    };
    Ok(mapped)
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
            field_count: graph.fields.len(),
            binding_count: graph.bindings.len(),
            read_count: graph.reads.len(),
            trigger_arm_count: graph.trigger_arms.len(),
            state_update_arm_count: graph.state_update_arms.len(),
            list_mutation_count: graph.list_mutations.len(),
        })
    }

    fn field(&self, id: SemanticFieldId) -> Result<MappedReactiveFieldId, String> {
        exact_dense_index(id.as_usize(), self.field_count, "semantic field", id)
            .map(MappedReactiveFieldId)
    }

    fn binding(&self, id: SemanticBindingId) -> Result<MappedReactiveBindingId, String> {
        exact_dense_index(id.as_usize(), self.binding_count, "semantic binding", id)
            .map(MappedReactiveBindingId)
    }

    fn read(&self, id: SemanticReadId) -> Result<MappedReactiveReadId, String> {
        exact_dense_index(id.as_usize(), self.read_count, "semantic read", id)
            .map(MappedReactiveReadId)
    }

    fn trigger(&self, id: SemanticTriggerArmId) -> Result<MappedReactiveTriggerId, String> {
        exact_dense_index(
            id.as_usize(),
            self.trigger_arm_count,
            "semantic trigger arm",
            id,
        )
        .map(MappedReactiveTriggerId)
    }

    fn state_update_arm(&self, id: crate::SemanticStateUpdateArmId) -> Result<usize, String> {
        exact_dense_index(
            id.as_usize(),
            self.state_update_arm_count,
            "semantic state update arm",
            id,
        )
    }

    fn list_mutation(&self, id: crate::SemanticListMutationId) -> Result<usize, String> {
        exact_dense_index(
            id.as_usize(),
            self.list_mutation_count,
            "semantic list mutation",
            id,
        )
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn map_semantic_reactive(
    execution: &SemanticExecutionImageColumnsV1,
    resource_graph: &SemanticResourceGraphV2,
    graph: &SemanticReactiveGraphV1,
    ids: &SemanticToExecutableMap,
    resources: &MappedSemanticResources,
) -> Result<MappedSemanticReactive, String> {
    let reactive_ids = SemanticReactiveToMappedMap::allocate(graph)?;
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
            let id = reactive_ids.state_update_arm(arm.id)?;
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
    for schedule in &graph.call_invocations {
        for trigger in &schedule.invocation_arms {
            referenced_trigger_ids.insert(reactive_ids.trigger(*trigger)?);
        }
    }
    for batch in &graph.pulse_batches {
        if let crate::SemanticPulseStartV1::Triggered { arms } = &batch.start {
            for arm in arms {
                let trigger = reactive_ids.trigger(*arm)?;
                trigger_arms.get(trigger.0).ok_or_else(|| {
                    format!(
                        "semantic pulse batch {} start maps to missing trigger {}",
                        batch.id, arm
                    )
                })?;
                referenced_trigger_ids.insert(trigger);
            }
        }
    }
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
    let producer_function_instances = graph
        .producer_instances
        .iter()
        .map(|instance| {
            map_reactive_producer_instance(execution, resource_graph, ids, &fields, instance)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let expected_trigger_ids = (0..graph.trigger_arms.len())
        .map(MappedReactiveTriggerId)
        .collect::<BTreeSet<_>>();
    if referenced_trigger_ids != expected_trigger_ids {
        return Err(format!(
            "mapped reactive and pulse records reference trigger IDs {referenced_trigger_ids:?}, expected exact set {expected_trigger_ids:?}"
        ));
    }

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
        id_map: reactive_ids,
    };
    Ok(mapped)
}

fn semantic_execution_statement(
    graph: &SemanticExecutionImageColumnsV1,
    id: SemanticStatementId,
) -> Result<&SemanticStatement, String> {
    graph
        .statements
        .get(id.as_usize())
        .filter(|statement| statement.id == id)
        .ok_or_else(|| format!("missing semantic statement {id}"))
}

fn semantic_execution_expression(
    graph: &SemanticExecutionImageColumnsV1,
    id: SemanticExprId,
) -> Result<&SemanticExpression, String> {
    graph
        .expressions
        .get(id.as_usize())
        .filter(|expression| expression.id == id)
        .ok_or_else(|| format!("missing semantic expression {id}"))
}

fn semantic_source_resource(
    graph: &SemanticResourceGraphV2,
    id: SemanticSourceId,
) -> Result<&crate::SemanticSourceResourceV1, String> {
    graph
        .sources
        .get(id.as_usize())
        .filter(|source| source.id == id)
        .ok_or_else(|| format!("missing semantic source resource {id}"))
}

fn semantic_state_resource(
    graph: &SemanticResourceGraphV2,
    id: SemanticStateId,
) -> Result<&crate::SemanticStateResourceV2, String> {
    graph
        .states
        .get(id.as_usize())
        .filter(|state| state.id == id)
        .ok_or_else(|| format!("missing semantic state resource {id}"))
}

fn mapped_owner_ancestry(
    graph: &SemanticExecutionImageColumnsV1,
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

fn first_type_difference(
    left: &boon_checked::Type,
    right: &boon_checked::Type,
    path: &str,
) -> Option<String> {
    use boon_checked::Type;
    if left == right {
        return None;
    }
    let child_path = |segment: &str| {
        if path.is_empty() {
            segment.to_owned()
        } else {
            format!("{path}.{segment}")
        }
    };
    match (left, right) {
        (Type::Object(left), Type::Object(right)) => {
            if left.open != right.open {
                return Some(format!(
                    "{path}: object openness {} != {}",
                    left.open, right.open
                ));
            }
            let names = left
                .fields
                .keys()
                .chain(right.fields.keys())
                .collect::<BTreeSet<_>>();
            for name in names {
                match (left.fields.get(name), right.fields.get(name)) {
                    (Some(left), Some(right)) => {
                        if let Some(difference) =
                            first_type_difference(left, right, &child_path(name))
                        {
                            return Some(difference);
                        }
                    }
                    (left, right) => {
                        return Some(format!(
                            "{}: field presence {} != {}",
                            child_path(name),
                            left.is_some(),
                            right.is_some()
                        ));
                    }
                }
            }
            Some(format!(
                "{path}: object field order {:?} != {:?}",
                left.field_order, right.field_order
            ))
        }
        (Type::List(left), Type::List(right)) | (Type::Set(left), Type::Set(right)) => {
            first_type_difference(left, right, &format!("{path}[]"))
        }
        (
            Type::Map {
                key: left_key,
                value: left_value,
            },
            Type::Map {
                key: right_key,
                value: right_value,
            },
        ) => first_type_difference(left_key, right_key, &format!("{path}.key"))
            .or_else(|| first_type_difference(left_value, right_value, &format!("{path}.value"))),
        (Type::Union(left), Type::Union(right)) => {
            if left.len() != right.len() {
                return Some(format!(
                    "{path}: union member count {} != {}",
                    left.len(),
                    right.len()
                ));
            }
            left.iter()
                .zip(right)
                .enumerate()
                .find_map(|(index, (left, right))| {
                    first_type_difference(left, right, &format!("{path}|{index}"))
                })
        }
        (Type::VariantSet(left), Type::VariantSet(right)) => {
            if left.len() != right.len() {
                return Some(format!(
                    "{path}: variant count {} != {}",
                    left.len(),
                    right.len()
                ));
            }
            left.iter()
                .zip(right)
                .enumerate()
                .find_map(|(index, (left, right))| match (left, right) {
                    (
                        boon_checked::Variant::Tagged {
                            tag: left_tag,
                            fields: left_fields,
                        },
                        boon_checked::Variant::Tagged {
                            tag: right_tag,
                            fields: right_fields,
                        },
                    ) if left_tag == right_tag => first_type_difference(
                        &Type::Object(left_fields.clone()),
                        &Type::Object(right_fields.clone()),
                        &format!("{path}.{left_tag}"),
                    ),
                    (left, right) if left == right => None,
                    (left, right) => Some(format!("{path}#{index}: variant {left:?} != {right:?}")),
                })
        }
        (left, right) => Some(format!("{path}: {left:?} != {right:?}")),
    }
}

fn map_reactive_field(
    execution: &SemanticExecutionImageColumnsV1,
    ids: &SemanticToExecutableMap,
    reactive_ids: &SemanticReactiveToMappedMap,
    field: &crate::SemanticFieldV1,
) -> Result<MappedSemanticField, String> {
    let statement = semantic_execution_statement(execution, field.statement)?;
    if statement.declaration != Some(field.declaration)
        || !semantic_field_producer_matches_statement(
            execution,
            statement,
            field.declaration,
            field.producer,
        )?
        || statement.flow_type.as_ref() != Some(&field.flow_type)
    {
        let type_difference = statement
            .flow_type
            .as_ref()
            .map(|flow| {
                if flow.mode != field.flow_type.mode {
                    format!("mode {:?} != {:?}", flow.mode, field.flow_type.mode)
                } else {
                    first_type_difference(&flow.ty, &field.flow_type.ty, "$")
                        .unwrap_or_else(|| "none".to_owned())
                }
            })
            .unwrap_or_else(|| "statement has no flow type".to_owned());
        return Err(format!(
            "semantic field {} has stale statement/declaration/value/type provenance: statement declaration {:?}, value {:?}; field declaration {}, producer {}; first type difference: {type_difference}",
            field.id, statement.declaration, statement.value, field.declaration.0, field.producer,
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

fn semantic_field_producer_matches_statement(
    execution: &SemanticExecutionImageColumnsV1,
    statement: &SemanticStatement,
    declaration: DeclId,
    producer: SemanticExprId,
) -> Result<bool, String> {
    if statement.value == Some(producer) {
        return Ok(true);
    }
    let Some(parent) = statement.parent else {
        return Ok(false);
    };
    let parent = semantic_execution_statement(execution, parent)?;
    let mut parent_value = parent.value;
    loop {
        let Some(value) = parent_value else {
            return Ok(false);
        };
        let expression = semantic_execution_expression(execution, value)?;
        match &expression.kind {
            SemanticExpressionKind::FlushBoundary { input } => parent_value = Some(*input),
            SemanticExpressionKind::Object(fields)
            | SemanticExpressionKind::TaggedObject { fields, .. } => {
                let matching = fields
                    .iter()
                    .filter(|candidate| candidate.declaration == Some(declaration))
                    .map(|candidate| candidate.value)
                    .collect::<Vec<_>>();
                return Ok(matching.as_slice() == [producer]);
            }
            _ => return Ok(false),
        }
    }
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
    execution: &SemanticExecutionImageColumnsV1,
    resources: &SemanticResourceGraphV2,
    ids: &SemanticToExecutableMap,
    reactive_ids: &SemanticReactiveToMappedMap,
    fields: &[MappedSemanticField],
    binding: &crate::SemanticBindingV1,
) -> Result<MappedSemanticBinding, String> {
    let statement = semantic_execution_statement(execution, binding.statement)?;
    let expression = semantic_execution_expression(execution, binding.producer)?;
    let producer_matches_statement = match binding.target {
        SemanticBindingTargetV1::Source { .. } => true,
        SemanticBindingTargetV1::State { state } => {
            semantic_state_resource(resources, state)?.expression == binding.producer
        }
        SemanticBindingTargetV1::Field { .. } => semantic_field_producer_matches_statement(
            execution,
            statement,
            binding.declaration,
            binding.producer,
        )?,
        SemanticBindingTargetV1::List { .. } => statement.value == Some(binding.producer),
    };
    let statement_declaration = match binding.target {
        SemanticBindingTargetV1::State { state } => {
            semantic_state_resource(resources, state)?.declaration
        }
        SemanticBindingTargetV1::Field { .. }
        | SemanticBindingTargetV1::Source { .. }
        | SemanticBindingTargetV1::List { .. } => binding.declaration,
    };
    if statement.declaration != Some(statement_declaration)
        || !producer_matches_statement
        || expression.value_id != binding.value
        || expression.owner != binding.owner
        || expression.flow_type != binding.flow_type
    {
        return Err(format!(
            "semantic binding {} has stale statement/producer/value/owner/type provenance: target={:?}, producer_matches_statement={}, statement={} declaration={:?}/{}, lexical_binding_declaration={}, value={:?}/{}, expression_value={}/{}, owner={:?}/{:?}, flow={:?}/{:?}",
            binding.id,
            binding.target,
            producer_matches_statement,
            binding.statement,
            statement.declaration,
            statement_declaration.0,
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
                || state.binding_declaration != binding.declaration
                || state.expression != binding.producer
            {
                return Err(format!(
                    "semantic binding {} state target {} has stale provenance: statement={}/{}, declaration={:?}/{:?}, expression={}/{}",
                    binding.id,
                    state.id,
                    state.statement,
                    binding.statement,
                    state.binding_declaration,
                    binding.declaration,
                    state.expression,
                    binding.producer,
                ));
            }
            let field = unique_mapped_field_for_statement(
                fields,
                executable_statement,
                state.declaration,
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
    execution: &SemanticExecutionImageColumnsV1,
    resources: &SemanticResourceGraphV2,
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
    execution: &SemanticExecutionImageColumnsV1,
    resources: &SemanticResourceGraphV2,
    ids: &SemanticToExecutableMap,
    reactive_ids: &SemanticReactiveToMappedMap,
    read: &crate::SemanticReadBindingV1,
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
            parameter: executable_parameter_for_occurrence(execution, ids, expression, *parameter)?,
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
        SemanticEventCauseV1::Pulse(pulse) => Ok(EventCause::Pulse(program_core::PulseBatchId(
            pulse.as_usize(),
        ))),
        SemanticEventCauseV1::ExternalRead(expression) => {
            Ok(EventCause::Source(ids.external_event_source(expression)?))
        }
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
        EventCause::Pulse(pulse) => Ok(format!("$pulse.p{}", pulse.as_usize())),
    }
}

fn map_reactive_trigger(
    execution: &SemanticExecutionImageColumnsV1,
    ids: &SemanticToExecutableMap,
    reactive_ids: &SemanticReactiveToMappedMap,
    trigger: &crate::SemanticTriggerOwnedArmV1,
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
    mutation: &crate::SemanticListMutationV1,
) -> Result<&'a crate::SemanticTriggerOwnedArmV1, String> {
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
    execution: &SemanticExecutionImageColumnsV1,
    graph: &SemanticReactiveGraphV1,
    ids: &SemanticToExecutableMap,
    reactive_ids: &SemanticReactiveToMappedMap,
    mutation: &crate::SemanticListMutationV1,
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
    execution: &'a SemanticExecutionImageColumnsV1,
    resource_graph: &'a SemanticResourceGraphV2,
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
    derived: &crate::SemanticDerivedValueV1,
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
    let state_backing = derived
        .state_backing
        .map(|state| context.ids.runtime_state(state))
        .transpose()?;
    if let Some(state) = state_backing {
        if kind != DerivedValueKind::Pure || !causes.is_empty() || !mapped_triggers.is_empty() {
            return Err(format!(
                "semantic derived value {} has eventful scheduling for exact state backing {}",
                derived.id, state.0
            ));
        }
        let mapped_state = context
            .resources
            .state_cells
            .get(state.as_usize())
            .filter(|candidate| candidate.id == state)
            .ok_or_else(|| {
                format!(
                    "semantic derived value {} maps to missing runtime state {}",
                    derived.id, state.0
                )
            })?;
        let [member] = expression.provenance.members.as_slice() else {
            return Err(format!(
                "semantic derived value {} state backing has non-exact value provenance",
                derived.id
            ));
        };
        let crate::SemanticValueOrigin::State {
            state: semantic_state,
            owner,
        } = &member.origin
        else {
            return Err(format!(
                "semantic derived value {} state backing is not state provenance",
                derived.id
            ));
        };
        if !member.path.is_empty()
            || context.ids.runtime_state(*semantic_state)? != state
            || *owner != mapped_state.static_owner
        {
            return Err(format!(
                "semantic derived value {} has stale exact state backing provenance",
                derived.id
            ));
        }
    }
    // A materialized list field carries its target row identity so storage can
    // own the list's row fields.  That does not make the list-producing
    // operation row-indexed: the operation computes the whole list and writes
    // it into the keyed materialization.  Only scalar fields owned by an
    // existing row execute once per row.
    let indexed = field.row.is_some() && materialized_list_id.is_none();
    let scope_id = indexed.then(|| field.row.expect("indexed field has a row").scope);
    Ok(MappedSemanticDerivedValue {
        field: field.id,
        executable_statement_id: statement,
        producer,
        path: field.path.clone(),
        kind,
        state_backing,
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
                    EventCause::Pulse(pulse) => Ok(ErasedTemporalBoundary::Pulse(pulse)),
                })
                .collect::<Result<Vec<_>, String>>()?,
        },
    })
}

fn map_reactive_dependency_use(
    execution: &SemanticExecutionImageColumnsV1,
    graph: &SemanticReactiveGraphV1,
    ids: &SemanticToExecutableMap,
    reactive_ids: &SemanticReactiveToMappedMap,
    dependency: &crate::SemanticDependencyUseV1,
) -> Result<MappedSemanticDependencyUse, String> {
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

fn map_reactive_producer_instance(
    execution: &SemanticExecutionImageColumnsV1,
    resources: &SemanticResourceGraphV2,
    ids: &SemanticToExecutableMap,
    fields: &[MappedSemanticField],
    instance: &crate::SemanticProducerInstanceV1,
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
    let root_expression = semantic_execution_expression(execution, instance.root_expression)?;
    if root_expression.value_id != instance.root_value
        || root_expression.flow_type != function.result_type
    {
        return Err(format!(
            "semantic producer instance {} has stale root value/type provenance",
            producer_identity_text(instance.identity)
        ));
    }
    let function_id = ids.producer_function(instance.function)?;
    ids.callable(instance.callable)?;
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
            ids.parameter(parameter.parameter)?;
            Ok(ProducerFunctionArgument {
                name: parameter.name.clone(),
                parameter: ExecutableParameterId {
                    function: function_id,
                    ordinal: parameter.parameter.ordinal,
                },
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
        result_type: function.result_type.clone(),
        effect: callable.effect,
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

#[allow(clippy::too_many_arguments)]
pub(super) fn map_semantic_storage_join(
    execution: &SemanticExecutionImageColumnsV1,
    resource_graph: &SemanticResourceGraphV2,
    reactive_graph: &SemanticReactiveGraphV1,
    storage_graph: &SemanticScopeStorageGraphV1,
    ids: &SemanticToExecutableMap,
    _resources: &MappedSemanticResources,
    reactive: &MappedSemanticReactive,
) -> Result<MappedSemanticStorage, String> {
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
    let fields = map_storage_fields(
        execution,
        resource_graph,
        storage_graph,
        ids,
        reactive,
        &storage_ids,
    )?;
    let locals = map_storage_locals(
        execution,
        resource_graph,
        storage_graph,
        ids,
        &storage_ids,
        &fields,
    )?;
    let bindings = map_storage_bindings(storage_graph, ids, reactive, &storage_ids, &fields)?;
    let sources = map_storage_sources(resource_graph, storage_graph, ids, reactive, &storage_ids)?;
    let reads = map_storage_reads(
        reactive_graph,
        reactive,
        &storage_ids,
        &bindings,
        &external_references,
    )?;
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
        producer_function_instances,
        derived_values,
        trigger_arms,
        state_update_arms: finalized_state_transitions,
        list_mutations,
        dependencies: reactive.dependencies.clone(),
        id_map: storage_ids,
    };
    Ok(mapped)
}

fn storage_field_is_runtime_row_value(
    storage: &MappedSemanticStorage,
    field: &ErasedFieldDef,
) -> bool {
    !field.resource_only
        && !storage.bindings.iter().any(|binding| {
            (matches!(binding.target, ErasedBindingTarget::Source { .. })
                && field.producer == Some(binding.producer))
                || (matches!(
                    binding.target,
                    ErasedBindingTarget::Value {
                        field: None,
                        row: Some(row),
                    } if Some(row) == field.row
                ) && field.producer == Some(binding.producer)
                    && field.declaration == Some(binding.declaration))
                || (matches!(
                    binding.target,
                    ErasedBindingTarget::State {
                        field: Some(authority),
                        row: Some(row),
                        ..
                    } if Some(row) == field.row && authority != field.id
                ) && field.producer == Some(binding.producer)
                    && field.declaration == Some(binding.declaration))
        })
}

fn mapped_row_field_depth(storage: &MappedSemanticStorage, field: &ErasedFieldDef) -> usize {
    let Some(row) = field.row else {
        return usize::MAX;
    };
    let mut depth = 0usize;
    let mut parent = field.parent;
    let mut remaining = storage.fields.len().saturating_add(1);
    while let Some(parent_id) = parent {
        if remaining == 0 {
            return usize::MAX;
        }
        remaining -= 1;
        let Some(parent_field) = storage
            .fields
            .get(parent_id.as_usize())
            .filter(|candidate| candidate.id == parent_id)
        else {
            return usize::MAX;
        };
        if parent_field.row != Some(row) {
            break;
        }
        depth = depth.saturating_add(1);
        parent = parent_field.parent;
    }
    depth
}

fn initializer_required_input_names(initializer: &ListInitializer) -> BTreeSet<String> {
    let ListInitializer::RecordLiteral { rows } = initializer else {
        return BTreeSet::new();
    };
    let mut names = BTreeSet::new();
    for row in rows {
        for field in &row.fields {
            if field.value == InitialValue::ResourceOnly {
                continue;
            }
            names.insert(field.name.clone());
            if let InitialValue::RowInitialField { path } = &field.value
                && let Some(root) = path.split('.').next().filter(|root| !root.is_empty())
            {
                names.insert(root.to_owned());
            }
        }
    }
    names
}

fn bind_list_initializer_inputs(
    lists: &mut [ListMemory],
    storage: &MappedSemanticStorage,
) -> Result<(), String> {
    for list in lists {
        let Some(scope) = list.row_scope_id else {
            if matches!(
                list.initializer,
                ListInitializer::RecordLiteral { ref rows } if !rows.is_empty()
            ) {
                return Err(format!(
                    "list `{}` has record initial rows without an exact row scope",
                    list.name
                ));
            }
            continue;
        };
        let row = ErasedRowBinding {
            list: list.id,
            scope,
        };
        let required = initializer_required_input_names(&list.initializer);
        let mut names = required.clone();
        names.extend(
            storage
                .fields
                .iter()
                .filter(|field| field.row == Some(row) && field.row_path.len() == 1)
                .map(|field| field.row_path[0].clone()),
        );
        for local in storage.locals.iter().filter(|local| local.row == Some(row)) {
            names.extend(
                local
                    .members
                    .iter()
                    .filter(|member| {
                        member.path.len() == 1
                            && matches!(member.target, ErasedLocalMemberTarget::Field(_))
                    })
                    .map(|member| member.path[0].clone()),
            );
            names.extend(
                local
                    .captures
                    .iter()
                    .filter(|capture| capture.projection.len() == 1)
                    .map(|capture| capture.projection[0].clone()),
            );
        }

        let mut inputs = Vec::new();
        for name in names {
            let source_path = [name.clone()];
            let constructor_candidates = storage
                .fields
                .iter()
                .filter(|field| {
                    field.row == Some(row)
                        && field.role == ErasedFieldRole::ListAuthority
                        && field.row_path == source_path
                })
                .map(|field| field.id)
                .collect::<BTreeSet<_>>();
            let direct = match constructor_candidates.len() {
                0 => storage
                    .fields
                    .iter()
                    .filter(|field| {
                        field.row == Some(row)
                            && field.name == name
                            && field.row_path == source_path
                            && storage_field_is_runtime_row_value(storage, field)
                    })
                    .min_by_key(|field| {
                        let role_priority = match field.role {
                            ErasedFieldRole::Value => 0,
                            ErasedFieldRole::ValueAuthority => 1,
                            ErasedFieldRole::ListAuthority => 2,
                            ErasedFieldRole::Capture => 3,
                        };
                        (
                            role_priority,
                            mapped_row_field_depth(storage, field),
                            field.id,
                        )
                    })
                    .map(|field| field.id),
                1 => constructor_candidates.iter().next().copied(),
                count => {
                    return Err(format!(
                        "list `{}` initializer input `{name}` resolves to {count} constructor authority fields: {constructor_candidates:?}",
                        list.name
                    ));
                }
            };
            let field = if let Some(field) = direct {
                Some(field)
            } else {
                let mut forwarded = BTreeSet::new();
                for local in storage.locals.iter().filter(|local| local.row == Some(row)) {
                    for member in local
                        .members
                        .iter()
                        .filter(|member| member.path == source_path)
                    {
                        let ErasedLocalMemberTarget::Field(field) = member.target else {
                            continue;
                        };
                        let Some(ErasedLocalMemberForwarding::Row {
                            row: forwarded_row,
                            path: forwarded_path,
                        }) = member.forwarded_from.as_ref()
                        else {
                            continue;
                        };
                        let target = storage.fields.get(field.as_usize()).filter(|candidate| {
                            candidate.id == field
                                && candidate.row == Some(row)
                                && candidate.row_path == *forwarded_path
                                && *forwarded_row == row
                                && storage_field_is_runtime_row_value(storage, candidate)
                        });
                        if target.is_some() {
                            forwarded.insert(field);
                        }
                    }
                    for capture in local
                        .captures
                        .iter()
                        .filter(|capture| capture.projection == source_path)
                    {
                        let field = capture.field;
                        let target = storage.fields.get(field.as_usize()).filter(|candidate| {
                            candidate.id == field
                                && candidate.row == Some(row)
                                && candidate.role == ErasedFieldRole::Capture
                                && storage_field_is_runtime_row_value(storage, candidate)
                        });
                        if target.is_some() {
                            forwarded.insert(field);
                        }
                    }
                }
                match forwarded.len() {
                    0 => None,
                    1 => forwarded.iter().next().copied(),
                    count => {
                        return Err(format!(
                            "list `{}` initializer input `{name}` resolves to {count} exact forwarded fields: {forwarded:?}",
                            list.name
                        ));
                    }
                }
            };
            match field {
                Some(field) => inputs.push(ListInitializerInput { name, field }),
                None if required.contains(&name) => {
                    return Err(format!(
                        "list `{}` initializer input `{name}` has no exact semantic storage field",
                        list.name
                    ));
                }
                None => {}
            }
        }
        list.initializer_inputs = inputs;
    }
    Ok(())
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
                    if slot.replace(FieldId(field.id.as_usize())).is_some() {
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

        if storage.bindings.len() != reactive.bindings.len() {
            return Err(
                "semantic storage binding map does not exactly cover reactive bindings".to_owned(),
            );
        }
        require_exact_identity_set(
            (0..reactive.bindings.len()).map(SemanticBindingId),
            storage.bindings.iter().map(|binding| binding.binding),
            "semantic storage binding",
        )?;

        Ok(Self {
            storage_field_count: storage.fields.len(),
            reactive_fields,
            binding_count: reactive.bindings.len(),
            read_count: reactive.reads.len(),
            external_reference_count: storage.external_references.len(),
        })
    }

    fn storage_field(&self, id: SemanticStorageFieldId) -> Result<FieldId, String> {
        exact_dense_index(
            id.as_usize(),
            self.storage_field_count,
            "semantic storage field",
            id,
        )
        .map(FieldId)
    }

    fn reactive_field(&self, id: MappedReactiveFieldId) -> Result<FieldId, String> {
        exact_map(&self.reactive_fields, id.0, "mapped reactive field", id)
    }

    fn binding(&self, id: MappedReactiveBindingId) -> Result<ErasedBindingId, String> {
        exact_dense_index(id.0, self.binding_count, "mapped reactive binding", id)
            .map(ErasedBindingId)
    }

    fn read(&self, id: MappedReactiveReadId) -> Result<ErasedReadId, String> {
        exact_dense_index(id.0, self.read_count, "mapped reactive read", id).map(ErasedReadId)
    }

    fn external_reference(
        &self,
        id: SemanticStorageExternalReferenceId,
    ) -> Result<SemanticStorageExternalReferenceId, String> {
        exact_dense_index(
            id.as_usize(),
            self.external_reference_count,
            "semantic storage external reference",
            id,
        )
        .map(SemanticStorageExternalReferenceId)
    }
}

fn map_storage_owners(
    execution: &SemanticExecutionImageColumnsV1,
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
    execution: &SemanticExecutionImageColumnsV1,
    resources: &SemanticResourceGraphV2,
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
            let row_path = crate::storage_contract::storage_field_row_path(
                resources,
                &storage.fields,
                field,
            )
            .map_err(|error| error.to_string())?;
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
                SemanticStorageFieldOriginV1::StateAuthority { state } => {
                    ids.state(*state)?;
                    if field.role != SemanticStorageFieldRoleV1::ValueAuthority
                        || field.reactive_field.is_some()
                    {
                        return Err(format!(
                            "state-authority storage field {} has inconsistent role or reactive identity",
                            field.id
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
                row_path,
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
    target: &SemanticStorageLocalMemberTargetV1,
    ids: &SemanticToExecutableMap,
    storage_ids: &SemanticStorageToErasedMap,
) -> Result<ErasedLocalMemberTarget, String> {
    Ok(match target {
        SemanticStorageLocalMemberTargetV1::Field(field) => {
            ErasedLocalMemberTarget::Field(storage_ids.storage_field(*field)?)
        }
        SemanticStorageLocalMemberTargetV1::Sources(sources) => ErasedLocalMemberTarget::Sources(
            sources
                .iter()
                .map(|source| ids.runtime_source(*source))
                .collect::<Result<Vec<_>, _>>()?,
        ),
        SemanticStorageLocalMemberTargetV1::State(state) => {
            ErasedLocalMemberTarget::State(ids.runtime_state(*state)?)
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
    execution: &SemanticExecutionImageColumnsV1,
    resources: &SemanticResourceGraphV2,
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
            let binding = resources
                .materialization_binding(materialization.id)
                .ok_or_else(|| {
                    format!(
                        "semantic storage local {}:{} has no resource binding",
                        local.owner, local.local
                    )
                })?;
            let target_row = binding
                .target
                .map(|row| map_row_binding(ids, row))
                .transpose()?;
            let authority_row = target_row.or(row);
            let members = local
                .members
                .iter()
                .map(|member| {
                    Ok(ErasedLocalMember {
                        path: member.path.clone(),
                        target: map_storage_local_member_target(&member.target, ids, storage_ids)?,
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
                                && candidate.row == authority_row
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
    storage: &SemanticScopeStorageGraphV1,
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
            let executable = ids.source(*source)?;
            let runtime = ids.runtime_source(*source)?;
            match &mapped.target {
                MappedSemanticBindingTarget::Source {
                    executable: staged_executable,
                    runtime: staged_runtime,
                } => {
                    if *staged_executable != executable || *staged_runtime != runtime {
                        return Err(format!(
                            "storage binding {} source {} differs from its staged allocation",
                            mapped.id, source
                        ));
                    }
                }
                MappedSemanticBindingTarget::Field {
                    field: reactive_field,
                } => {
                    let field = storage_ids.reactive_field(*reactive_field)?;
                    let definition = fields
                        .get(field.as_usize())
                        .filter(|candidate| candidate.id == field)
                        .ok_or_else(|| {
                            format!(
                                "storage binding {} direct source alias references missing FieldId {field}",
                                mapped.id
                            )
                        })?;
                    if !definition.resource_only {
                        return Err(format!(
                            "storage binding {} redirects non-resource FieldId {field} to source {source}",
                            mapped.id
                        ));
                    }
                }
                _ => {
                    return Err(format!(
                        "storage binding {} is a source but staged target is {:?}",
                        mapped.id, mapped.target
                    ));
                }
            }
            Ok(ErasedBindingTarget::Source {
                executable,
                runtime,
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
                    let is_public_field =
                        storage_ids.reactive_field(*reactive_field)? == final_field;
                    let is_state_authority = storage
                        .fields
                        .get(field.as_usize())
                        .filter(|candidate| candidate.id == field)
                        .is_some_and(|candidate| {
                            matches!(
                                candidate.origin,
                                SemanticStorageFieldOriginV1::StateAuthority {
                                    state: candidate_state
                                } if candidate_state == *state
                            )
                        });
                    if !is_public_field && !is_state_authority {
                        return Err(format!(
                            "storage binding {} state field {} is neither the public reactive field {} nor state {} authority",
                            mapped.id, field, reactive_field, state
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
                storage,
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
    resources: &SemanticResourceGraphV2,
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
    execution: &SemanticExecutionImageColumnsV1,
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
    bindings: &[ErasedBinding],
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
                } => {
                    let binding = storage_ids.binding(*binding)?;
                    let storage_binding = bindings
                        .get(binding.as_usize())
                        .filter(|candidate| candidate.id == binding)
                        .ok_or_else(|| {
                            format!(
                                "semantic read {} maps to missing storage binding {binding}",
                                semantic.id
                            )
                        })?;
                    match storage_binding.target {
                        ErasedBindingTarget::Source { runtime: _, .. } => {
                            // Source payload reads are already explicit in the
                            // semantic graph. A plain binding read that joins a
                            // source is therefore a structural resource facade;
                            // its object path is routing metadata, not a runtime
                            // payload projection.
                            MappedSemanticStorageReadTarget::Binding {
                                binding,
                                projection: Vec::new(),
                            }
                        }
                        ErasedBindingTarget::State { runtime: state, .. } => {
                            MappedSemanticStorageReadTarget::StateProjection {
                                binding,
                                state,
                                projection: projection.clone(),
                            }
                        }
                        ErasedBindingTarget::Value { .. } => {
                            MappedSemanticStorageReadTarget::Binding {
                                binding,
                                projection: projection.clone(),
                            }
                        }
                    }
                }
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
            let allocated = schedule.id.as_usize();
            if allocated != index {
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
            let transient_result = schedule
                .transient_result
                .map(|derived| {
                    exact_dense_index(
                        derived.as_usize(),
                        reactive.derived_values.len(),
                        "semantic host-effect transient derived value",
                        derived,
                    )
                })
                .transpose()?;
            Ok(MappedSemanticHostEffectSchedule {
                id: allocated,
                expression,
                value,
                call: schedule.call,
                checked_expression: schedule.checked_expression,
                owner: schedule.owner,
                operation: schedule.operation.clone(),
                state_update_arms: mapped_arms,
                transient_result,
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
                result_type: mapped.result_type.clone(),
                effect: mapped.effect,
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
                producer: mapped.producer,
                path: mapped.path.clone(),
                kind: mapped.kind.clone(),
                state_backing: mapped.state_backing,
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

fn semantic_list_resource(
    graph: &SemanticResourceGraphV2,
    id: SemanticListId,
) -> Result<&crate::SemanticListResourceV1, String> {
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
        SemanticInitialValueV1::ExpressionAuthority => InitialValue::ExpressionAuthority,
        SemanticInitialValueV1::ResourceOnly => InitialValue::ResourceOnly,
    }
}

fn map_source_resource(
    execution: &SemanticExecutionImageColumnsV1,
    ids: &SemanticToExecutableMap,
    source: &crate::SemanticSourceResourceV1,
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

fn map_source_payload_schema(source: &crate::SemanticSourceResourceV1) -> SourcePayloadSchema {
    let typed_fields = source
        .payload_fields
        .iter()
        .filter_map(|field| {
            let data_type = semantic_data_type(&field.data_type);
            (!matches!(data_type, program_core::SemanticDataType::Unknown { .. })).then(|| {
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
    execution: &SemanticExecutionImageColumnsV1,
    ids: &SemanticToExecutableMap,
    state: &crate::SemanticStateResourceV2,
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
        lifetime: match state.lifetime {
            crate::SemanticStateLifetimeV1::Persistent => {
                program_core::StateCellLifetimeV1::Persistent
            }
            crate::SemanticStateLifetimeV1::ActivationLocal { then_expression } => {
                program_core::StateCellLifetimeV1::ActivationLocal {
                    then_expression: ids.expression(then_expression)?,
                }
            }
        },
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
    graph: &SemanticExecutionImageColumnsV1,
    id: SemanticExprId,
) -> Result<&SemanticExpression, String> {
    graph
        .expressions
        .get(id.as_usize())
        .filter(|expression| expression.id == id)
        .ok_or_else(|| format!("missing semantic expression {id}"))
}

fn runtime_checked_expression_id(id: boon_checked::CheckedExprId) -> Result<ExprId, String> {
    let value = usize::try_from(id.0).map_err(|_| {
        format!(
            "checked expression {} exceeds executable usize identity space",
            id.0
        )
    })?;
    Ok(ExprId(value))
}

fn map_expression(
    graph: &SemanticExecutionImageColumnsV1,
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
    graph: &SemanticExecutionImageColumnsV1,
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
        SemanticExpressionKind::Text(value) => ExecutableExpressionKind::Text {
            value: value.clone(),
        },
        SemanticExpressionKind::TextTemplate { segments } => {
            ExecutableExpressionKind::TextTemplate {
                segments: segments
                    .iter()
                    .map(|segment| map_text_segment(ids, segment))
                    .collect::<Result<Vec<_>, _>>()?,
            }
        }
        SemanticExpressionKind::Number(value) => ExecutableExpressionKind::Number {
            value: value.clone(),
        },
        SemanticExpressionKind::Bits(value) => ExecutableExpressionKind::Bits {
            value: value.clone(),
        },
        SemanticExpressionKind::BytesByte(value) => {
            ExecutableExpressionKind::BytesByte { value: *value }
        }
        SemanticExpressionKind::Absent => ExecutableExpressionKind::Absent,
        SemanticExpressionKind::Flush { payload } => ExecutableExpressionKind::Flush {
            payload: ids.expression(*payload)?,
        },
        SemanticExpressionKind::FlushBoundary { input } => {
            ExecutableExpressionKind::FlushBoundary {
                input: ids.expression(*input)?,
            }
        }
        SemanticExpressionKind::Tag(value) => ExecutableExpressionKind::Tag {
            value: value.clone(),
        },
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
            call,
            callable,
            callable_kind,
            name,
            intrinsic,
            instance,
            arguments,
            context_argument,
            contexts,
            ..
        } => match callable_kind {
            SemanticCallableKind::User => ExecutableExpressionKind::UserCall {
                checked_call: semantic_call(graph, *call)?.checked_call,
                function: ids.callable(*callable)?,
                name: name.clone(),
                instance: instance
                    .map(|instance| ids.call_instance(instance))
                    .transpose()?,
                arguments: {
                    let mut mapped = arguments
                        .iter()
                        .map(|argument| map_call_argument(ids, argument))
                        .collect::<Result<Vec<_>, _>>()?;
                    if let Some(argument) = context_argument {
                        let callable = semantic_callable(graph, *callable)?;
                        mapped.push(ExecutableCallArgument {
                            ordinal: callable.parameters.len(),
                            name: "PASSED".to_owned(),
                            value: ids.expression(argument.value)?,
                            from_pipe: false,
                        });
                    }
                    mapped
                },
                type_substitutions: semantic_call(graph, *call)?.type_substitutions.clone(),
            },
            SemanticCallableKind::Builtin | SemanticCallableKind::External => {
                ExecutableExpressionKind::Call {
                    checked_call: semantic_call(graph, *call)?.checked_call,
                    callable_kind: match callable_kind {
                        SemanticCallableKind::Builtin => ExecutableCallableKind::Builtin,
                        SemanticCallableKind::External => ExecutableCallableKind::External,
                        SemanticCallableKind::User => unreachable!(),
                    },
                    name: name.clone(),
                    intrinsic: *intrinsic,
                    instance: instance
                        .map(|instance| ids.call_instance(instance))
                        .transpose()?,
                    arguments: arguments
                        .iter()
                        .map(|argument| map_call_argument(ids, argument))
                        .collect::<Result<Vec<_>, _>>()?,
                    contexts: contexts
                        .iter()
                        .map(|context| ids.call_context(*context))
                        .collect::<Result<Vec<_>, _>>()?,
                    context_ordinals: semantic_call(graph, *call)?
                        .contexts
                        .iter()
                        .map(|context| context.signature)
                        .collect(),
                }
            }
        },
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
        SemanticExpressionKind::Object(fields) => ExecutableExpressionKind::Object {
            fields: fields
                .iter()
                .map(|field| map_record_field(ids, field))
                .collect::<Result<Vec<_>, _>>()?,
        },
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
        SemanticExpressionKind::MapEntry { key, value } => ExecutableExpressionKind::MapEntry {
            key: ids.expression(*key)?,
            value: ids.expression(*value)?,
        },
        SemanticExpressionKind::Map { entries } => ExecutableExpressionKind::Map {
            entries: entries
                .iter()
                .map(|entry| ids.expression(*entry))
                .collect::<Result<Vec<_>, _>>()?,
        },
        SemanticExpressionKind::Set { items } => ExecutableExpressionKind::Set {
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
        SemanticExpressionKind::Project { input, fields } => {
            let mut input = *input;
            let mut projection = fields.clone();
            loop {
                let input_expression = semantic_expression(graph, input)?;
                match &input_expression.kind {
                    SemanticExpressionKind::Project {
                        input: nested_input,
                        fields: nested_fields,
                    } => {
                        let mut flattened = nested_fields.clone();
                        flattened.extend(projection);
                        projection = flattened;
                        input = *nested_input;
                    }
                    SemanticExpressionKind::MaterializationLocal {
                        owner,
                        local,
                        projection: local_projection,
                        constructor_projection,
                    } => {
                        let mut local_projection = local_projection.clone();
                        local_projection.extend(projection);
                        break ExecutableExpressionKind::MaterializationLocal {
                            owner: *owner,
                            local: ids.materialization_local(*owner, *local)?,
                            projection: local_projection,
                            constructor_projection: constructor_projection.clone(),
                        };
                    }
                    SemanticExpressionKind::FunctionParameter {
                        parameter,
                        projection: parameter_projection,
                    } => {
                        let mut parameter_projection = parameter_projection.clone();
                        parameter_projection.extend(projection);
                        break ExecutableExpressionKind::FunctionParameter {
                            parameter: executable_parameter_for_occurrence(
                                graph,
                                ids,
                                input_expression,
                                *parameter,
                            )?,
                            projection: parameter_projection,
                        };
                    }
                    _ => {
                        break ExecutableExpressionKind::Project {
                            input: ids.expression(input)?,
                            fields: projection,
                        };
                    }
                }
            }
        }
        SemanticExpressionKind::MaterializationLocal {
            owner,
            local,
            projection,
            constructor_projection,
        } => ExecutableExpressionKind::MaterializationLocal {
            owner: *owner,
            local: ids.materialization_local(*owner, *local)?,
            projection: projection.clone(),
            constructor_projection: constructor_projection.clone(),
        },
        SemanticExpressionKind::FunctionParameter {
            parameter,
            projection,
        } => ExecutableExpressionKind::FunctionParameter {
            parameter: executable_parameter_for_occurrence(graph, ids, expression, *parameter)?,
            projection: projection.clone(),
        },
    })
}

fn executable_parameter_for_occurrence(
    graph: &SemanticExecutionImageColumnsV1,
    ids: &SemanticToExecutableMap,
    expression: &SemanticExpression,
    parameter: SemanticParameterId,
) -> Result<ExecutableParameterId, String> {
    ids.parameter(parameter)?;
    let functions = graph
        .functions
        .iter()
        .filter(|function| {
            function.parameters.iter().any(|candidate| {
                candidate.id == parameter && candidate.input_expressions.contains(&expression.id)
            })
        })
        .collect::<Vec<_>>();
    if let [function] = functions.as_slice() {
        return Ok(ExecutableParameterId {
            function: ids.producer_function(function.producer)?,
            ordinal: parameter.ordinal,
        });
    }
    if !functions.is_empty() {
        return Err(format!(
            "semantic function-parameter expression {} resolves to {} exact producer occurrences",
            expression.id,
            functions.len()
        ));
    }
    let callable = semantic_callable(graph, parameter.callable)?;
    let Some(root) = callable.semantic_root else {
        return Err(format!(
            "semantic function-parameter expression {} has neither a producer nor ordinary callable root",
            expression.id
        ));
    };
    let mut pending = vec![root];
    let mut visited = BTreeSet::new();
    while let Some(candidate) = pending.pop() {
        if !visited.insert(candidate) {
            continue;
        }
        if candidate == expression.id {
            return ids.parameter(parameter);
        }
        pending.extend(
            semantic_expression(graph, candidate)?
                .kind
                .direct_children(),
        );
    }
    Err(format!(
        "semantic function-parameter expression {} is outside ordinary callable {} root {}",
        expression.id, parameter.callable, root
    ))
}

fn map_provenance(
    graph: &SemanticExecutionImageColumnsV1,
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
    graph: &SemanticExecutionImageColumnsV1,
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
                ids.callable(*function)?;
                let executable_function = ids.producer_function(*producer)?;
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
    graph: &SemanticExecutionImageColumnsV1,
    source: &SemanticSourceRead,
) -> Result<boon_checked::CheckedSourceRead, String> {
    Ok(boon_checked::CheckedSourceRead {
        source: checked_source_id(graph, source.source)?,
        payload_projection: source.payload_projection.clone(),
    })
}

fn checked_source_id(
    graph: &SemanticExecutionImageColumnsV1,
    source: SemanticSourceId,
) -> Result<boon_checked::CheckedSourceId, String> {
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
    graph: &SemanticExecutionImageColumnsV1,
    state: SemanticStateId,
) -> Result<boon_checked::CheckedStateId, String> {
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
    identity: Option<&boon_checked::CheckedExternalDeclarationIdentityV1>,
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
                ids.callable(function)?;
                let executable_function = ids.producer_function(producer)?;
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
    graph: &SemanticExecutionImageColumnsV1,
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
    graph: &SemanticExecutionImageColumnsV1,
    ids: &SemanticToExecutableMap,
    function: &SemanticFunction,
) -> Result<ExecutableFunction, String> {
    let callable = semantic_callable(graph, function.callable)?;
    if callable.kind != boon_checked::CheckedCallableKind::User {
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
    ids.callable(function.callable)?;
    let id = ids.producer_function(function.producer)?;
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
            .map(|parameter| map_function_parameter(ids, id, parameter))
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
    function: FunctionId,
    parameter: &SemanticFunctionParameter,
) -> Result<ExecutableFunctionParameter, String> {
    ids.parameter(parameter.id)?;
    Ok(ExecutableFunctionParameter {
        id: ExecutableParameterId {
            function,
            ordinal: parameter.id.ordinal,
        },
        name: parameter.name.clone(),
        flow_type: parameter.flow_type.clone(),
    })
}

fn map_ordinary_function(
    ids: &SemanticToExecutableMap,
    callable: &SemanticCallable,
) -> Result<ExecutableOrdinaryFunction, String> {
    let root = callable.semantic_root.ok_or_else(|| {
        format!(
            "ordinary semantic callable {} has no retained root",
            callable.id
        )
    })?;
    let function = ids.callable(callable.id)?;
    let mut parameters = callable
        .parameters
        .iter()
        .map(|parameter| {
            ids.parameter(parameter.id)?;
            Ok(ExecutableFunctionParameter {
                id: ExecutableParameterId {
                    function,
                    ordinal: parameter.ordinal,
                },
                name: parameter.name.clone(),
                flow_type: parameter.flow_type.clone(),
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    if let Some(parameter) = &callable.context_parameter {
        ids.parameter(parameter.id)?;
        parameters.push(ExecutableFunctionParameter {
            id: ExecutableParameterId {
                function,
                ordinal: parameter.id.ordinal,
            },
            name: parameter.name.clone(),
            flow_type: parameter.flow_type.clone(),
        });
    }
    Ok(ExecutableOrdinaryFunction {
        id: function,
        name: callable.name.clone(),
        parameters,
        result_type: callable.result.clone(),
        root: ids.expression(root)?,
    })
}

fn map_materialization(
    ids: &SemanticToExecutableMap,
    materialization: &SemanticContextualMaterialization,
    binding: &crate::SemanticMaterializationResourceBindingV1,
) -> Result<ContextualMaterialization, String> {
    Ok(ContextualMaterialization {
        id: ids.materialization(materialization.id)?,
        operation: map_operation(materialization.operation),
        source: ids.expression(materialization.source)?,
        source_row_predecessors: binding
            .predecessors
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
        source_list_id: binding
            .source
            .map(|row| row.list)
            .map(|id| ids.list(id))
            .transpose()?,
        source_scope_id: binding
            .source
            .map(|row| row.scope)
            .map(|id| ids.row_scope(id))
            .transpose()?,
        target_list_id: binding
            .target
            .map(|row| row.list)
            .map(|id| ids.list(id))
            .transpose()?,
        target_scope_id: binding
            .target
            .map(|row| row.scope)
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
) -> Result<program_core::ErasedRowBinding, String> {
    Ok(program_core::ErasedRowBinding {
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

fn map_activation_sites(
    graph: &SemanticReactiveGraphV1,
    ids: &SemanticToExecutableMap,
) -> Result<Vec<program_core::ActivationSite>, String> {
    require_dense(
        graph
            .activations
            .iter()
            .map(|activation| activation.id.as_usize()),
        "semantic activation",
    )?;
    graph
        .activations
        .iter()
        .enumerate()
        .map(|(index, activation)| {
            let then_expression = ids.expression(activation.then_expression)?;
            let input_expression = ids.expression(activation.input_expression)?;
            let output_expression = ids.expression(activation.output_expression)?;
            if ids.value(activation.input_value)? != input_expression
                || ids.value(activation.output_value)? != output_expression
            {
                return Err(format!(
                    "semantic activation {} value identities do not map to their producing expressions",
                    activation.id
                ));
            }
            ids.lexical_scope(activation.route_scope)?;
            let states = activation
                .states
                .iter()
                .map(|state| ids.runtime_state(*state))
                .collect::<Result<Vec<_>, String>>()?;
            Ok(program_core::ActivationSite {
                id: program_core::ActivationId(index),
                then_expression,
                input_expression,
                output_expression,
                static_owner: activation.owner,
                states,
            })
        })
        .collect()
}

fn map_pulse_batches(
    graph: &SemanticReactiveGraphV1,
    ids: &SemanticToExecutableMap,
    storage: &MappedSemanticStorage,
) -> Result<Vec<program_core::PulseBatch>, String> {
    require_dense(
        graph.pulse_batches.iter().map(|batch| batch.id.as_usize()),
        "semantic pulse batch",
    )?;
    graph
        .pulse_batches
        .iter()
        .enumerate()
        .map(|(index, batch)| {
            let id = program_core::PulseBatchId(index);
            let enclosing_activation = batch
                .enclosing_activation
                .map(|activation| {
                    graph
                        .activations
                        .get(activation.as_usize())
                        .filter(|candidate| candidate.id == activation)
                        .map(|_| program_core::ActivationId(activation.as_usize()))
                        .ok_or_else(|| {
                            format!(
                                "semantic pulse batch {} references missing activation {}",
                                batch.id, activation
                            )
                        })
                })
                .transpose()?;
            let state = batch
                .state
                .map(|state| ids.runtime_state(state))
                .transpose()?;
            let hold_expression = match (batch.hold_expression, batch.hold_value) {
                (None, None) => None,
                (Some(expression), Some(value)) => {
                    let expression = ids.expression(expression)?;
                    if ids.value(value)? != expression {
                        return Err(format!(
                            "semantic pulse batch {} HOLD value does not map to its producing expression",
                            batch.id
                        ));
                    }
                    Some(expression)
                }
                _ => {
                    return Err(format!(
                        "semantic pulse batch {} has incomplete HOLD expression/value provenance",
                        batch.id
                    ));
                }
            };
            let call_expression = ids.call_expression(batch.call, batch.call_expression)?;
            if ids.value(batch.call_value)? != call_expression {
                return Err(format!(
                    "semantic pulse batch {} call value does not map to its producing expression",
                    batch.id
                ));
            }
            let count_expression = ids.expression(batch.count_expression)?;
            if ids.value(batch.count_value)? != count_expression {
                return Err(format!(
                    "semantic pulse batch {} count value does not map to its producing expression",
                    batch.id
                ));
            }
            let transition_expression = batch
                .transition_expression
                .map(|expression| ids.expression(expression))
                .transpose()?;
            let transition_output = batch
                .transition_output
                .map(|expression| ids.expression(expression))
                .transpose()?;
            let map_trigger_arm = |trigger_id: &crate::SemanticTriggerArmId| {
                    let semantic = graph
                        .trigger_arms
                        .get(trigger_id.as_usize())
                        .filter(|candidate| candidate.id == *trigger_id)
                        .ok_or_else(|| {
                            format!(
                                "semantic pulse batch {} references missing trigger arm {}",
                                batch.id, trigger_id
                            )
                        })?;
                    let mapped = storage
                        .trigger_arms
                        .get(trigger_id.as_usize())
                        .cloned()
                        .ok_or_else(|| {
                            format!(
                                "semantic pulse batch {} references missing finalized trigger arm {}",
                                batch.id, trigger_id
                            )
                        })?;
                    if mapped.cause != map_semantic_event_cause(semantic.cause, ids)? {
                        return Err(format!(
                            "semantic pulse batch {} trigger arm {} changed cause during lowering",
                            batch.id, trigger_id
                        ));
                    }
                    Ok(mapped)
                };
            let start = match &batch.start {
                crate::SemanticPulseStartV1::Startup => program_core::PulseStart::Startup,
                crate::SemanticPulseStartV1::Triggered { arms } => {
                    let arms = arms
                        .iter()
                        .map(&map_trigger_arm)
                        .collect::<Result<Vec<_>, String>>()?;
                    if arms.is_empty()
                        || arms
                            .iter()
                            .any(|arm| matches!(arm.cause, program_core::EventCause::Pulse(_)))
                    {
                        return Err(format!(
                            "semantic pulse batch {} has an empty or pulse-recursive start",
                            batch.id
                        ));
                    }
                    program_core::PulseStart::Triggered { arms }
                }
            };
            let trigger_arms = batch
                .trigger_arms
                .iter()
                .map(map_trigger_arm)
                .collect::<Result<Vec<_>, String>>()?;
            let mapped_state_update_arms = batch
                .state_update_arms
                .iter()
                .map(|arm_id| {
                    let semantic = graph
                        .state_update_arms
                        .get(arm_id.as_usize())
                        .filter(|candidate| candidate.id == *arm_id)
                        .ok_or_else(|| {
                            format!(
                                "semantic pulse batch {} references missing state update arm {}",
                                batch.id, arm_id
                            )
                        })?;
                    let mapped = storage
                        .state_update_arms
                        .get(arm_id.as_usize())
                        .cloned()
                        .ok_or_else(|| {
                            format!(
                                "semantic pulse batch {} references missing finalized state update arm {}",
                                batch.id, arm_id
                            )
                        })?;
                    if mapped.state != ids.runtime_state(semantic.state)?
                        || mapped.cause != program_core::EventCause::Pulse(id)
                    {
                        return Err(format!(
                            "semantic pulse batch {} state update arm {} changed target or cause during lowering",
                            batch.id, arm_id
                        ));
                    }
                    Ok(mapped)
                })
                .collect::<Result<Vec<_>, String>>()?;
            let list_mutations = batch
                .list_mutations
                .iter()
                .map(|mutation_id| {
                    let semantic = graph
                        .list_mutations
                        .get(mutation_id.as_usize())
                        .filter(|candidate| candidate.id == *mutation_id)
                        .ok_or_else(|| {
                            format!(
                                "semantic pulse batch {} references missing list mutation {}",
                                batch.id, mutation_id
                            )
                        })?;
                    let mapped = storage
                        .list_mutations
                        .get(mutation_id.as_usize())
                        .cloned()
                        .ok_or_else(|| {
                            format!(
                                "semantic pulse batch {} references missing finalized list mutation {}",
                                batch.id, mutation_id
                            )
                        })?;
                    if semantic.cause != crate::SemanticEventCauseV1::Pulse(batch.id)
                        || mapped.cause != program_core::EventCause::Pulse(id)
                    {
                        return Err(format!(
                            "semantic pulse batch {} list mutation {} changed cause during lowering",
                            batch.id, mutation_id
                        ));
                    }
                    Ok(mapped)
                })
                .collect::<Result<Vec<_>, String>>()?;
            let derived_value_indices = batch
                .derived_values
                .iter()
                .map(|derived_id| {
                    graph
                        .derived_values
                        .get(derived_id.as_usize())
                        .filter(|candidate| candidate.id == *derived_id)
                        .ok_or_else(|| {
                            format!(
                                "semantic pulse batch {} references missing derived value {}",
                                batch.id, derived_id
                            )
                        })?;
                    storage
                        .derived_values
                        .get(derived_id.as_usize())
                        .filter(|derived| derived.causes.contains(&program_core::EventCause::Pulse(id)))
                        .map(|_| derived_id.as_usize())
                        .ok_or_else(|| {
                            format!(
                                "semantic pulse batch {} references missing finalized derived value {} with its pulse cause",
                                batch.id, derived_id
                            )
                        })
                })
                .collect::<Result<Vec<_>, String>>()?;
            let host_effects = batch
                .host_effect_schedules
                .iter()
                .map(|schedule_id| {
                    graph
                        .host_effect_schedules
                        .get(schedule_id.as_usize())
                        .filter(|candidate| candidate.id == *schedule_id)
                        .ok_or_else(|| {
                            format!(
                                "semantic pulse batch {} references missing host-effect schedule {}",
                                batch.id, schedule_id
                            )
                        })?;
                    let schedule = storage
                        .host_effect_schedules
                        .get(schedule_id.as_usize())
                        .filter(|candidate| candidate.id == schedule_id.as_usize())
                        .ok_or_else(|| {
                            format!(
                                "semantic pulse batch {} references missing finalized host-effect schedule {}",
                                batch.id, schedule_id
                            )
                        })?;
                    Ok(program_core::PulseHostEffect {
                        expression: schedule.expression,
                        operation: schedule.operation.clone(),
                    })
                })
                .collect::<Result<Vec<_>, String>>()?;
            let flush_roots = batch
                .flush_roots
                .iter()
                .map(|expression| ids.expression(*expression))
                .collect::<Result<Vec<_>, String>>()?;
            let emission_routes = batch
                .emission_routes
                .iter()
                .map(|route| {
                    let consumer = route
                        .consumer
                        .map(|expression| ids.expression(expression))
                        .transpose()?;
                    let filter = match &route.filter {
                        crate::SemanticPulseEmissionFilterV1::Passthrough => {
                            program_core::PulseEmissionFilter::Passthrough
                        }
                        crate::SemanticPulseEmissionFilterV1::Skip {
                            call,
                            expression,
                            count_expression,
                            count_value,
                        } => {
                            let expression = ids.call_expression(*call, *expression)?;
                            if consumer != Some(expression) {
                                return Err(format!(
                                    "semantic pulse batch {} skip filter does not own its consumer",
                                    batch.id
                                ));
                            }
                            let count_expression = ids.expression(*count_expression)?;
                            if ids.value(*count_value)? != count_expression {
                                return Err(format!(
                                    "semantic pulse batch {} skip count does not map to its producing expression",
                                    batch.id
                                ));
                            }
                            program_core::PulseEmissionFilter::Skip {
                                expression,
                                count_expression,
                            }
                        }
                    };
                    Ok(program_core::PulseEmissionRoute { consumer, filter })
                })
                .collect::<Result<Vec<_>, String>>()?;
            Ok(program_core::PulseBatch {
                id,
                enclosing_activation,
                state,
                hold_expression,
                call_expression,
                count_expression,
                start,
                transition_expression,
                transition_output,
                trigger_arms,
                state_update_arms: mapped_state_update_arms,
                list_mutations,
                derived_value_indices,
                host_effects,
                flush_roots,
                emission_routes,
                schedule: match batch.schedule {
                    crate::SemanticPulseScheduleV1::StageArbitrateCommitPublishBeforeNext => {
                        program_core::PulseSchedule::StageArbitrateCommitPublishBeforeNext
                    }
                },
                flush_policy: match batch.flush_policy {
                    crate::SemanticPulseFlushPolicyV1::DiscardCurrentStopRemainingKeepPriorCommits => {
                        program_core::PulseFlushPolicy::DiscardCurrentStopRemainingKeepPriorCommits
                    }
                },
                fusion: program_core::PulseFusionEligibility::PendingVerification,
                semantic_slice_digest: batch.slice_digest.0,
            })
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn build_canonical_program_core(
    execution_graph: &SemanticExecutionImageColumnsV1,
    resource_graph: &SemanticResourceGraphV2,
    reactive_graph: &SemanticReactiveGraphV1,
    lowering_contract: &SemanticLoweringContractV2,
    view_binding_graph: &crate::SemanticViewBindingGraphV1,
    scope_storage_graph: &SemanticScopeStorageGraphV1,
    memory_graph: &crate::SemanticMemoryGraphV1,
    mut receipts: crate::semantic_image::ExecutionReceiptPublisherV3<'_>,
) -> Result<CanonicalProgramCoreBuildV2, String> {
    let mapped = map_semantic_execution_with_reactive(
        execution_graph,
        resource_graph,
        reactive_graph,
        &mut receipts,
    )?;
    let resources = map_semantic_resources(execution_graph, resource_graph, &mapped.id_map)?;
    finish_canonical_program_core(
        execution_graph,
        resource_graph,
        reactive_graph,
        lowering_contract,
        view_binding_graph,
        scope_storage_graph,
        memory_graph,
        mapped,
        resources,
        receipts,
    )
}

#[allow(clippy::too_many_arguments)]
fn finish_canonical_program_core(
    execution_graph: &SemanticExecutionImageColumnsV1,
    resource_graph: &SemanticResourceGraphV2,
    reactive_graph: &SemanticReactiveGraphV1,
    lowering_contract: &SemanticLoweringContractV2,
    view_binding_graph: &crate::SemanticViewBindingGraphV1,
    scope_storage_graph: &SemanticScopeStorageGraphV1,
    memory_graph: &crate::SemanticMemoryGraphV1,
    mapped: MappedSemanticExecution,
    resources: MappedSemanticResources,
    mut receipts: crate::semantic_image::ExecutionReceiptPublisherV3<'_>,
) -> Result<CanonicalProgramCoreBuildV2, String> {
    let mut resources = resources;
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
        &mapped.id_map,
        &resources,
        &reactive,
    )?;
    bind_list_initializer_inputs(&mut resources.lists, &storage)?;
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
    let transient_collections =
        map_transient_collections(lowering_contract, memory_graph, &mapped.id_map)?;
    let named_value_interfaces = map_named_value_interfaces(&lowering_contract.metadata);
    let debug_source_units = map_debug_source_units(&lowering_contract.metadata)?;
    let debug_fields = map_semantic_field_entries(
        &storage.fields,
        &storage.derived_values,
        &resources.state_cells,
    );
    let activations = map_activation_sites(reactive_graph, &mapped.id_map)?;
    let pulse_batches = map_pulse_batches(reactive_graph, &mapped.id_map, &storage)?;
    let external_storage_sources = mapped
        .id_map
        .external_event_source_paths
        .keys()
        .copied()
        .map(|source| ErasedSourceDef {
            source,
            static_owner: None,
            owner_ancestry: Vec::new(),
            origin: ErasedSourceOrigin::DistributedImport,
        })
        .collect::<Vec<_>>();

    let MappedSemanticExecution {
        executable,
        materializations,
        ..
    } = mapped;
    let MappedSemanticResources {
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
        sources: mut storage_sources,
        row_values,
        row_source_projections,
        producer_function_instances,
        derived_values,
        state_update_arms: finalized_state_transitions,
        host_effect_schedules,
        list_mutations,
        dependencies,
        ..
    } = storage;
    for source in external_storage_sources {
        if source.source.as_usize() != storage_sources.len() {
            return Err(format!(
                "distributed ingress source {} is not dense after {} local/previous source definitions",
                source.source,
                storage_sources.len()
            ));
        }
        storage_sources.push(source);
    }
    let graph_node_count = executable.expressions.len();

    let core = program_core::CanonicalProgramCoreV2 {
        executable,
        scope_index: program_core::ErasedScopeIndex {
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
        distributed_references: mapped_role_references,
        producer_function_instances,
        debug_source_units,
        debug_fields,
        graph_node_count,
        sources,
        host_ports,
        state_cells,
        activations,
        pulse_batches,
        lists,
        semantic_memory,
        migration_edges,
        transient_collections,
        output_values,
        derived_values,
        dependencies,
        state_update_arms: finalized_state_transitions,
        host_effect_schedules: host_effect_schedules
            .into_iter()
            .map(|schedule| program_core::HostEffectSchedule {
                id: schedule.id,
                expression: schedule.expression,
                checked_expression: schedule.checked_expression,
                owner: schedule.owner,
                operation: schedule.operation,
                state_update_arms: schedule.state_update_arms,
                transient_result: schedule.transient_result,
            })
            .collect(),
        list_mutations,
        list_projections,
        materializations,
        view_bindings,
        named_value_interfaces,
    };
    for (semantic, executable) in execution_graph
        .static_owners
        .iter()
        .zip(&core.scope_index.owners)
    {
        if (semantic.id, semantic.parent, semantic.child_ordinal)
            != (executable.id, executable.parent, executable.child_ordinal)
        {
            return Err(format!(
                "static owner {} disagrees with executable owner",
                semantic.id
            ));
        }
        receipts.publish_static_owner(semantic, executable)?;
    }
    let execution_handoff = receipts.finish()?;
    Ok(CanonicalProgramCoreBuildV2 {
        core,
        execution_handoff,
    })
}

type ExternalReferenceMap = BTreeMap<SemanticStorageExternalReferenceId, usize>;

fn map_distributed_references(
    execution: &SemanticExecutionImageColumnsV1,
    mapped: &MappedSemanticExecution,
    storage: &MappedSemanticStorage,
) -> Result<
    (
        program_core::DistributedReferences,
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
                let mut local_alias_paths = storage
                    .bindings
                    .iter()
                    .filter(|binding| {
                        binding.producer == expression
                            && binding.flow_type == executable.flow_type
                            && matches!(
                                binding.target,
                                program_core::ErasedBindingTarget::Value {
                                    field: Some(_),
                                    row: None,
                                }
                            )
                    })
                    .map(|binding| binding.diagnostic_path.clone())
                    .collect::<Vec<_>>();
                local_alias_paths.sort();
                local_alias_paths.dedup();
                value_references.push(program_core::DistributedValueReference {
                    expression,
                    canonical_path: reference.canonical_path.clone(),
                    local_alias_paths,
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
                        Ok(program_core::DistributedCallArgument {
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
                calls.push(program_core::DistributedCall {
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
        program_core::DistributedReferences {
            value_references,
            calls,
        },
        value_ids,
        call_ids,
    ))
}

fn distributed_role_from_path(path: &str) -> Option<boon_checked::ProgramRole> {
    let namespace = path.split('/').next()?;
    match namespace {
        "Client" => Some(boon_checked::ProgramRole::Client),
        "Session" => Some(boon_checked::ProgramRole::Session),
        "Server" => Some(boon_checked::ProgramRole::Server),
        _ => None,
    }
}

fn finalize_storage_reads(
    storage: &MappedSemanticStorage,
    resources: &MappedSemanticResources,
    external_values: &ExternalReferenceMap,
) -> Result<Vec<program_core::ErasedReadBinding>, String> {
    storage
        .reads
        .iter()
        .map(|read| {
            let target = match &read.target {
                MappedSemanticStorageReadTarget::Binding {
                    binding,
                    projection,
                } => program_core::ErasedReadTarget::Binding {
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
                        return Ok(program_core::ErasedReadBinding {
                            id: read.id,
                            expression: read.expression,
                            target: program_core::ErasedReadTarget::Binding {
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
                    program_core::ErasedReadTarget::SourcePayload {
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
                        program_core::ErasedReadTarget::Binding {
                            binding: *binding,
                            projection: Vec::new(),
                        }
                    } else {
                        program_core::ErasedReadTarget::StateProjection {
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
                } => program_core::ErasedReadTarget::Local {
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
                    program_core::ErasedReadTarget::ExternalValue { reference }
                }
                MappedSemanticStorageReadTarget::ElementState {
                    context,
                    projection,
                } => program_core::ErasedReadTarget::ElementState {
                    context: *context,
                    projection: projection.clone(),
                },
                MappedSemanticStorageReadTarget::MaterializationLocal {
                    owner,
                    local,
                    projection,
                } => program_core::ErasedReadTarget::MaterializationLocal {
                    owner: *owner,
                    local: *local,
                    projection: projection.clone(),
                },
                MappedSemanticStorageReadTarget::FunctionParameter {
                    parameter,
                    projection,
                } => program_core::ErasedReadTarget::FunctionParameter {
                    parameter: *parameter,
                    projection: projection.clone(),
                },
            };
            Ok(program_core::ErasedReadBinding {
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
) -> Result<Vec<program_core::ErasedDependencyUse>, String> {
    storage
        .dependency_uses
        .iter()
        .map(|dependency| {
            let target = match dependency.target {
                MappedSemanticStorageDependencyTarget::BundleExternalRead { read, .. } => {
                    program_core::ErasedDependencyTarget::ExternalRead { read }
                }
                MappedSemanticStorageDependencyTarget::BundleExternalCall { reference } => {
                    let reference = external_calls.get(&reference).copied().ok_or_else(|| {
                        format!(
                            "mapped semantic dependency expression {} external identity {reference} has no call allocation",
                            dependency.expression
                        )
                    })?;
                    program_core::ErasedDependencyTarget::ExternalCall { reference }
                }
            };
            Ok(program_core::ErasedDependencyUse {
                dependent: dependency.dependent,
                expression: dependency.expression,
                target,
                timing: dependency.timing.clone(),
            })
        })
        .collect()
}

fn map_output_values(
    lowering: &SemanticLoweringContractV2,
    ids: &SemanticToExecutableMap,
    storage_ids: &SemanticStorageToErasedMap,
) -> Result<Vec<program_core::OutputRootValue>, String> {
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
                crate::SemanticOutputContractKindV1::RetainedVisualDocument => {
                    program_core::SemanticOutputContractKind::RetainedVisual {
                        kind: program_core::SemanticRetainedVisualKind::Document,
                    }
                }
                crate::SemanticOutputContractKindV1::RetainedVisualScene => {
                    program_core::SemanticOutputContractKind::RetainedVisual {
                        kind: program_core::SemanticRetainedVisualKind::Scene,
                    }
                }
                crate::SemanticOutputContractKindV1::HostValue => {
                    program_core::SemanticOutputContractKind::HostValue
                }
            };
            let demand = match output.demand {
                crate::SemanticOutputDemandPolicyV1::HostDemanded => {
                    program_core::SemanticOutputDemandPolicy::HostDemanded
                }
            };
            Ok(program_core::OutputRootValue {
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

fn map_host_ports(lowering: &SemanticLoweringContractV2) -> Vec<program_core::HostPortDeclaration> {
    lowering
        .host_ports
        .iter()
        .map(|port| match &port.kind {
            crate::SemanticHostPortKindV1::HttpServer {
                request,
                disconnect,
                response,
            } => program_core::HostPortDeclaration::HttpServer {
                line: port.line,
                request_source: request.diagnostic_path.clone(),
                disconnect_source: disconnect
                    .as_ref()
                    .map(|binding| binding.diagnostic_path.clone()),
                response_output: response.diagnostic_name.clone(),
            },
            crate::SemanticHostPortKindV1::WebSocketServer {
                open,
                message,
                close,
                error,
                actions,
            } => program_core::HostPortDeclaration::WebSocketServer {
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
    graph: &crate::SemanticViewBindingGraphV1,
    ids: &SemanticToExecutableMap,
    storage_ids: &SemanticStorageToErasedMap,
    storage: &MappedSemanticStorage,
) -> Result<Vec<program_core::ViewBinding>, String> {
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
            let argument = graph
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
                crate::SemanticViewBindingTargetV1::Data { read } => mapped_view_read_path(
                    storage,
                    storage_ids.read(MappedReactiveReadId(read.as_usize()))?,
                )
                .unwrap_or_else(|| binding.diagnostic_path.clone()),
                crate::SemanticViewBindingTargetV1::Event { .. } => binding.diagnostic_path.clone(),
            };
            let target = match binding.target {
                crate::SemanticViewBindingTargetV1::Data { read } => {
                    program_core::ViewBindingTarget::Read {
                        read: storage_ids.read(MappedReactiveReadId(read.as_usize()))?,
                        additional_projection: binding.additional_projection.clone(),
                    }
                }
                crate::SemanticViewBindingTargetV1::Event { source } => {
                    program_core::ViewBindingTarget::Source {
                        source: ids.runtime_source(source)?,
                    }
                }
            };
            let kind = match binding.kind {
                crate::SemanticViewBindingKindV1::Data => program_core::ViewBindingKind::Data,
                crate::SemanticViewBindingKindV1::Source => program_core::ViewBindingKind::Source,
                crate::SemanticViewBindingKindV1::Target => program_core::ViewBindingKind::Target,
            };
            Ok(program_core::ViewBinding {
                id: program_core::ViewBindingId(index),
                node_expression: ids.expression(node.expression)?,
                argument_expression: ids.expression(argument.expression)?,
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

fn map_named_value_interfaces(
    metadata: &crate::SemanticLoweringMetadataV2,
) -> Vec<program_core::NamedValueInterface> {
    metadata
        .named_value_types
        .iter()
        .map(|entry| program_core::NamedValueInterface {
            canonical_path: entry.diagnostic_path.clone(),
            flow_type: entry.flow_type.clone(),
        })
        .collect()
}

fn map_debug_source_units(
    metadata: &crate::SemanticLoweringMetadataV2,
) -> Result<Vec<program_core::SemanticSourceUnit>, String> {
    metadata
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
            Ok(program_core::SemanticSourceUnit {
                id: program_core::SourceUnitId(index),
                path: unit.path.clone(),
                module: unit.module.clone(),
                start_line: unit.start_line,
                line_count: unit.line_count,
            })
        })
        .collect()
}

fn map_semantic_field_entries(
    fields: &[ErasedFieldDef],
    derived_values: &[DerivedValue],
    state_cells: &[StateCell],
) -> Vec<program_core::SemanticFieldEntry> {
    fields
        .iter()
        .filter(|field| field.role.is_value())
        .map(|field| program_core::SemanticFieldEntry {
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

fn map_semantic_memory(
    execution: &SemanticExecutionImageColumnsV1,
    reactive: &SemanticReactiveGraphV1,
    graph: &crate::SemanticMemoryGraphV1,
    ids: &SemanticToExecutableMap,
    storage_ids: &SemanticStorageToErasedMap,
) -> Result<
    (
        Vec<program_core::SemanticMemory>,
        Vec<program_core::MigrationEdge>,
    ),
    String,
> {
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
                crate::SemanticMemoryBackingV1::State {
                    storage_field,
                    state,
                    row,
                    ..
                } => {
                    let state_id = ids.runtime_state(state)?;
                    let field_id = Some(storage_ids.storage_field(storage_field)?);
                    match (kind, row) {
                        (program_core::SemanticMemoryKind::RootScalar, None) => {
                            program_core::SemanticMemoryRuntimeBacking::RootState { state_id, field_id }
                        }
                        (program_core::SemanticMemoryKind::IndexedField, Some(row)) => {
                            let row = map_row_binding(ids, row)?;
                            program_core::SemanticMemoryRuntimeBacking::IndexedState {
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
                crate::SemanticMemoryBackingV1::List {
                    storage_field,
                    list,
                    row,
                    ..
                } => {
                    if kind != program_core::SemanticMemoryKind::ListOwner {
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
                    program_core::SemanticMemoryRuntimeBacking::List {
                        list_id,
                        row_scope_id: Some(row.scope),
                    }
                }
                crate::SemanticMemoryBackingV1::Collection { expression, owner } => {
                    let semantic_expression =
                        semantic_execution_expression(execution, expression)?;
                    let kind_matches = matches!(
                        (kind, &semantic_expression.kind),
                        (
                            program_core::SemanticMemoryKind::Map,
                            SemanticExpressionKind::Map { .. }
                        ) | (
                            program_core::SemanticMemoryKind::Set,
                            SemanticExpressionKind::Set { .. }
                        )
                    );
                    if !kind_matches || semantic_expression.owner != owner {
                        return Err(format!(
                            "semantic memory {} collection backing does not match kind {:?} and owner {:?}",
                            memory.id, memory.identity.kind, owner
                        ));
                    }
                    program_core::SemanticMemoryRuntimeBacking::Collection {
                        expression: ids.expression(expression)?,
                        owner,
                    }
                }
            };
            let status = match memory.status {
                crate::SemanticMemoryStatusV1::Active => {
                    program_core::SemanticMemoryStatus::Active
                }
                crate::SemanticMemoryStatusV1::Draining { marker } => {
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
                    program_core::SemanticMemoryStatus::Draining {
                        marker_expr_id: ExprId(marker.checked_expr_id.0 as usize),
                    }
                }
            };
            Ok(program_core::SemanticMemory {
                id: program_core::SemanticMemoryId(index),
                identity: program_core::SemanticMemoryIdentity {
                    canonical_module: memory.identity.canonical_module.clone(),
                    owner_path: memory.identity.owner_path.clone(),
                    semantic_path: memory.identity.semantic_path.clone(),
                    kind,
                },
                data_type: semantic_data_type(&memory.data_type),
                leaves: memory
                    .leaves
                    .iter()
                    .map(|leaf| program_core::SemanticMemoryLeaf {
                        semantic_path: semantic_region_path(
                            &memory.identity.semantic_path,
                            &leaf.projection,
                        ),
                        data_type: semantic_data_type(&leaf.data_type),
                    })
                    .collect(),
                status,
                runtime_backing,
                structural_owner_rows: memory
                    .structural_owner_rows
                    .iter()
                    .copied()
                    .map(|row| map_row_binding(ids, row))
                    .collect::<Result<Vec<_>, _>>()?,
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
                    Ok(program_core::MigrationSourceLeaf {
                        memory_id: program_core::SemanticMemoryId(input.source.memory.as_usize()),
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
                crate::SemanticMigrationTransferV1::Scalar => {
                    program_core::MigrationTransferKind::Scalar
                }
                crate::SemanticMigrationTransferV1::IndexedField { owner } => {
                    map_row_binding(ids, owner)?;
                    program_core::MigrationTransferKind::IndexedField
                }
                crate::SemanticMigrationTransferV1::List {
                    source,
                    destination,
                } => {
                    map_row_binding(ids, source)?;
                    map_row_binding(ids, destination)?;
                    program_core::MigrationTransferKind::List
                }
            };
            let transform = match edge.transform {
                crate::SemanticMigrationTransformV1::Identity { input } => {
                    semantic_execution_expression(execution, input)?;
                    program_core::MigrationTransform::Identity
                }
                crate::SemanticMigrationTransformV1::PureExpression { root } => {
                    let root = semantic_execution_expression(execution, root)?;
                    program_core::MigrationTransform::PureExpression {
                        expression_root: ExprId(root.checked_expr_id.0 as usize),
                        pipeline: Vec::new(),
                    }
                }
            };
            Ok(program_core::MigrationEdge {
                source_leaves,
                destination: program_core::MigrationDestination {
                    memory_id: program_core::SemanticMemoryId(edge.destination.memory.as_usize()),
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

fn map_transient_collections(
    lowering: &SemanticLoweringContractV2,
    memory: &crate::SemanticMemoryGraphV1,
    ids: &SemanticToExecutableMap,
) -> Result<Vec<program_core::TransientCollection>, String> {
    let durable_constructors = memory
        .memories
        .iter()
        .filter_map(|memory| match memory.backing {
            crate::SemanticMemoryBackingV1::Collection { expression, .. } => Some(expression),
            _ => None,
        })
        .collect::<BTreeSet<_>>();
    let mut seen_constructors = BTreeSet::new();
    let mut seen_results = BTreeSet::new();
    lowering
        .transient_collections
        .iter()
        .map(|region| {
            if durable_constructors.contains(&region.constructor) {
                return Err(format!(
                    "transient collection constructor {} also owns durable semantic memory",
                    region.constructor
                ));
            }
            if !seen_constructors.insert(region.constructor)
                || !seen_results.insert(region.result.expression())
                || region.snapshot_copy_budget != 0
                || region.authority_flow.first() != Some(&region.constructor)
                || region.authority_flow.last() != Some(&region.result.expression())
            {
                return Err(format!(
                    "transient collection constructor {} has non-canonical proof identity or budgets",
                    region.constructor
                ));
            }
            let kind = match region.kind {
                crate::SemanticTransientCollectionKindV1::List => {
                    program_core::TransientCollectionKind::List
                }
                crate::SemanticTransientCollectionKindV1::Map => {
                    program_core::TransientCollectionKind::Map
                }
                crate::SemanticTransientCollectionKindV1::Set => {
                    program_core::TransientCollectionKind::Set
                }
            };
            let list_items = region
                .list_items
                .iter()
                .copied()
                .map(|item| ids.expression(item))
                .collect::<Result<Vec<_>, String>>()?;
            let map_entries = region
                .map_entries
                .iter()
                .map(|entry| {
                    Ok(program_core::TransientMapEntry {
                        key: ids.expression(entry.key)?,
                        value: ids.expression(entry.value)?,
                    })
                })
                .collect::<Result<Vec<_>, String>>()?;
            let set_items = region
                .set_items
                .iter()
                .copied()
                .map(|item| ids.expression(item))
                .collect::<Result<Vec<_>, String>>()?;
            let steps = region
                .steps
                .iter()
                .map(|step| {
                    Ok(match step {
                        crate::SemanticTransientCollectionStepV1::ListAppend {
                            expression,
                            item,
                        } => program_core::TransientCollectionStep::ListAppend {
                            expression: ids.expression(*expression)?,
                            item: ids.expression(*item)?,
                        },
                        crate::SemanticTransientCollectionStepV1::MapUpsert {
                            expression,
                            key,
                            value,
                        } => program_core::TransientCollectionStep::MapUpsert {
                            expression: ids.expression(*expression)?,
                            key: ids.expression(*key)?,
                            value: ids.expression(*value)?,
                        },
                        crate::SemanticTransientCollectionStepV1::MapRemove {
                            expression,
                            key,
                        } => program_core::TransientCollectionStep::MapRemove {
                            expression: ids.expression(*expression)?,
                            key: ids.expression(*key)?,
                        },
                        crate::SemanticTransientCollectionStepV1::SetAdd {
                            expression,
                            item,
                        } => program_core::TransientCollectionStep::SetAdd {
                            expression: ids.expression(*expression)?,
                            item: ids.expression(*item)?,
                        },
                        crate::SemanticTransientCollectionStepV1::SetRemove {
                            expression,
                            item,
                        } => program_core::TransientCollectionStep::SetRemove {
                            expression: ids.expression(*expression)?,
                            item: ids.expression(*item)?,
                        },
                    })
                })
                .collect::<Result<Vec<_>, String>>()?;
            let result = match &region.result {
                crate::SemanticTransientCollectionResultV1::ListGet {
                    expression,
                    position,
                } => program_core::TransientCollectionResult::ListGet {
                    expression: ids.expression(*expression)?,
                    position: ids.expression(*position)?,
                },
                crate::SemanticTransientCollectionResultV1::ListLength {
                    expression,
                } => program_core::TransientCollectionResult::ListLength {
                    expression: ids.expression(*expression)?,
                },
                crate::SemanticTransientCollectionResultV1::ListIsNotEmpty {
                    expression,
                } => program_core::TransientCollectionResult::ListIsNotEmpty {
                    expression: ids.expression(*expression)?,
                },
                crate::SemanticTransientCollectionResultV1::MapGet {
                    expression,
                    key,
                } => program_core::TransientCollectionResult::MapGet {
                    expression: ids.expression(*expression)?,
                    key: ids.expression(*key)?,
                },
                crate::SemanticTransientCollectionResultV1::SetContains {
                    expression,
                    item,
                } => program_core::TransientCollectionResult::SetContains {
                    expression: ids.expression(*expression)?,
                    item: ids.expression(*item)?,
                },
            };
            Ok(program_core::TransientCollection {
                kind,
                constructor: ids.expression(region.constructor)?,
                declared_capacity: region.declared_capacity,
                list_items,
                map_entries,
                set_items,
                steps,
                result,
                authority_flow: region
                    .authority_flow
                    .iter()
                    .copied()
                    .map(|expression| ids.expression(expression))
                    .collect::<Result<Vec<_>, String>>()?,
                operation_work_budget: region.operation_work_budget,
                storage_growth_budget: region.storage_growth_budget,
                snapshot_copy_budget: region.snapshot_copy_budget,
            })
        })
        .collect()
}

const fn map_semantic_memory_kind(
    kind: crate::SemanticMemoryKindV1,
) -> program_core::SemanticMemoryKind {
    match kind {
        crate::SemanticMemoryKindV1::RootScalar => program_core::SemanticMemoryKind::RootScalar,
        crate::SemanticMemoryKindV1::IndexedField => program_core::SemanticMemoryKind::IndexedField,
        crate::SemanticMemoryKindV1::ListOwner => program_core::SemanticMemoryKind::ListOwner,
        crate::SemanticMemoryKindV1::Map => program_core::SemanticMemoryKind::Map,
        crate::SemanticMemoryKindV1::Set => program_core::SemanticMemoryKind::Set,
    }
}

fn semantic_memory_record(
    graph: &crate::SemanticMemoryGraphV1,
    id: crate::SemanticMemoryId,
) -> Result<&crate::SemanticMemoryV1, String> {
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
    root: &'a boon_checked::Type,
    projection: &[String],
) -> Result<&'a boon_checked::Type, String> {
    let mut current = root;
    for field in projection {
        current = match current {
            boon_checked::Type::Object(shape) => shape.fields.get(field).ok_or_else(|| {
                format!(
                    "semantic memory type projection `{}` has no field `{field}`",
                    projection.join(".")
                )
            })?,
            boon_checked::Type::VariantSet(variants) => {
                let projected = variants
                    .iter()
                    .filter_map(|variant| match variant {
                        boon_checked::Variant::Tagged { fields, .. } => fields.fields.get(field),
                        boon_checked::Variant::Tag(_) => None,
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

fn exact_dense_index(
    index: usize,
    count: usize,
    label: &str,
    id: impl std::fmt::Display,
) -> Result<usize, String> {
    (index < count)
        .then_some(index)
        .ok_or_else(|| format!("{label} {id} has no executable mapping"))
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
