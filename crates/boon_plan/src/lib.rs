use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::error::Error;
use std::fmt;
use std::path::{Component, Path};

mod binary;
mod document;

pub use document::*;

pub const PLAN_MAJOR_VERSION: u32 = 2;
pub const PLAN_MINOR_VERSION: u32 = 0;
pub const INLINE_BYTE_CONSTANT_LIMIT: usize = 1024;

fn is_zero(value: &usize) -> bool {
    *value == 0
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PlanVersion {
    pub major: u32,
    pub minor: u32,
}

impl Default for PlanVersion {
    fn default() -> Self {
        Self {
            major: PLAN_MAJOR_VERSION,
            minor: PLAN_MINOR_VERSION,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetProfile {
    SoftwareDefault,
    SoftwareBounded,
    FpgaTodomvc,
}

impl TargetProfile {
    pub fn from_name(name: &str) -> Result<Self, PlanError> {
        match name {
            "software_default" | "default" => Ok(Self::SoftwareDefault),
            "software_bounded" => Ok(Self::SoftwareBounded),
            "fpga_todomvc" => Ok(Self::FpgaTodomvc),
            other => Err(PlanError::new(format!(
                "unknown MachinePlan target profile `{other}`"
            ))),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::SoftwareDefault => "software_default",
            Self::SoftwareBounded => "software_bounded",
            Self::FpgaTodomvc => "fpga_todomvc",
        }
    }
}

macro_rules! plan_usize_ids {
    ($($name:ident),+ $(,)?) => {
        $(
            #[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
            #[serde(transparent)]
            pub struct $name(pub usize);

            impl $name {
                pub fn as_usize(self) -> usize {
                    self.0
                }
            }
        )+
    };
}

plan_usize_ids!(
    PlanSourceRouteId,
    PlanConstantId,
    PlanStorageId,
    PlanRegionId,
    PlanOpId,
    PlanDeltaId,
    SourceId,
    StateId,
    ListId,
    FieldId,
    ScopeId,
);

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SourcePayloadSchema {
    pub fields: Vec<SourcePayloadField>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub typed_fields: Vec<SourcePayloadDescriptor>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub row_lookup_field: Option<String>,
}

impl SourcePayloadSchema {
    pub fn row_lookup_field_name(&self) -> Option<&str> {
        self.row_lookup_field.as_deref()
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SourcePayloadDescriptor {
    pub field: SourcePayloadField,
    pub value_type: SourcePayloadValueType,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourcePayloadValueType {
    Bytes,
    Bool,
    Text,
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub enum SourcePayloadField {
    Address,
    Bytes,
    Key,
    Named(String),
    Text,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct MachinePlan {
    pub version: PlanVersion,
    pub target_profile: TargetProfile,
    pub demand: DemandPlan,
    pub document: Option<DocumentPlan>,
    pub constants: Vec<PlanConstant>,
    pub source_routes: Vec<SourceRoute>,
    pub storage_layout: StorageLayout,
    pub regions: Vec<OperationRegion>,
    pub dirty_plan: DirtyPlan,
    pub commit_plan: CommitPlan,
    pub delta_plan: DeltaPlan,
    pub capability_summary: CapabilitySummary,
    pub debug_map: DebugMap,
}

impl MachinePlan {
    pub fn document_plan(&self) -> Option<&DocumentPlan> {
        self.document.as_ref()
    }

    pub fn initial_document_patch_batch(&self) -> Option<&DocumentInitialPatchBatch> {
        self.document
            .as_ref()
            .map(|document| &document.initial_patch_batch)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DemandPlan {
    pub root_derived_outputs: RootOutputDemand,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "field_ids", rename_all = "snake_case")]
pub enum RootOutputDemand {
    All,
    Selected(Vec<FieldId>),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SourceRoute {
    pub id: PlanSourceRouteId,
    pub source_id: SourceId,
    pub path: String,
    pub scoped: bool,
    pub scope_id: Option<ScopeId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interval_ms: Option<u64>,
    pub payload_schema: SourcePayloadSchema,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PlanConstant {
    pub id: PlanConstantId,
    pub value: PlanConstantValue,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PlanConstantValue {
    Text {
        value: String,
    },
    Number {
        value: i64,
    },
    Byte {
        value: u8,
    },
    Bool {
        value: bool,
    },
    Bytes {
        byte_len: u64,
        sha256: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        inline_bytes: Option<Vec<u8>>,
    },
    Enum {
        value: String,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct StorageLayout {
    pub scalar_slots: Vec<ScalarStorageSlot>,
    pub list_slots: Vec<ListStorageSlot>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub byte_banks: Vec<ByteStorageBank>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ScalarStorageSlot {
    pub id: PlanStorageId,
    pub state_id: StateId,
    pub value_type: PlanValueType,
    pub scope_id: Option<ScopeId>,
    pub indexed: bool,
    pub initial_value_kind: InitialValueKind,
    pub initial_constant_id: Option<PlanConstantId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub initial_root_field_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub initial_row_field_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub initial_row_expression: Option<PlanRowExpression>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ListStorageSlot {
    pub id: PlanStorageId,
    pub list_id: ListId,
    pub scope_id: Option<ScopeId>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub row_field_ids: Vec<FieldId>,
    pub capacity: Option<usize>,
    pub hidden_key_type: String,
    pub has_generation: bool,
    pub initializer_kind: ListInitializerKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub range: Option<PlanRangeInitializer>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub initial_rows: Vec<PlanInitialListRow>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ByteStorageBank {
    pub id: PlanStorageId,
    pub state_storage_id: PlanStorageId,
    pub state_id: StateId,
    pub scope_id: Option<ScopeId>,
    pub indexed: bool,
    pub fixed_len: u64,
    pub capacity: Option<usize>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PlanRangeInitializer {
    pub from: i64,
    pub to: i64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PlanInitialListRow {
    pub fields: Vec<PlanInitialListField>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PlanInitialListField {
    pub name: String,
    pub field_id: Option<FieldId>,
    pub value: PlanConstantValue,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InitialValueKind {
    Text,
    Number,
    Byte,
    Bool,
    Bytes,
    Enum,
    RootInitialField,
    RowInitialField,
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PlanValueType {
    Text,
    Number,
    Byte,
    Bool,
    Bytes {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        fixed_len: Option<u64>,
    },
    Enum,
    RootInitialField,
    RowInitialField,
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ListInitializerKind {
    RecordLiteral,
    Range,
    Empty,
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct OperationRegion {
    pub id: PlanRegionId,
    pub kind: RegionKind,
    pub ops: Vec<PlanOp>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RegionKind {
    SourceRouting,
    StateInitialization,
    DerivedEvaluation,
    UpdateBranches,
    ListOperations,
    ListProjections,
    DependencyEdges,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PlanOp {
    pub id: PlanOpId,
    pub kind: PlanOpKind,
    pub inputs: Vec<ValueRef>,
    pub output: Option<ValueRef>,
    pub indexed: bool,
    pub unresolved_executable_ref_count: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanOpKind {
    SourceRoute,
    StateInitialize {
        initial_value_kind: InitialValueKind,
        initial_constant_id: Option<PlanConstantId>,
    },
    DerivedValue {
        derived_kind: PlanDerivedKind,
        #[serde(default = "default_derived_startup_recompute")]
        startup_recompute: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expression: Option<PlanDerivedExpression>,
    },
    UpdateBranch {
        expression_kind: PlanExpressionKind,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        ordered_inputs: Vec<ValueRef>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        source_payload_field: Option<SourcePayloadField>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        update_constant_id: Option<PlanConstantId>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        source_guard: Option<PlanSourceGuard>,
    },
    ListOperation {
        operation_kind: PlanListOperationKind,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        append: Option<PlanListAppend>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        remove: Option<PlanListRemove>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        retain: Option<PlanListRetain>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        count: Option<PlanListCount>,
    },
    ListProjection {
        projection: PlanListProjection,
    },
    DependencyEdge,
}

fn default_derived_startup_recompute() -> bool {
    true
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PlanListProjection {
    Find {
        source_list: ListId,
        field: String,
        value: ValueRef,
    },
    Chunk {
        source_list: ListId,
        size: usize,
        item_field: String,
        label_field: String,
    },
    ChunkValue {
        source: ValueRef,
        size: usize,
        item_field: String,
        label_field: String,
    },
    Unknown {
        summary: String,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanDerivedKind {
    SourceEventTransform,
    ListView,
    Aggregate,
    Pure,
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PlanDerivedExpression {
    SourceKeyTextTrimNonEmpty {
        source_id: SourceId,
        key_field: SourcePayloadField,
        required_key: String,
        state: ValueRef,
        skip_empty: bool,
    },
    SourceEventTransform {
        default: Box<PlanRowExpression>,
        arms: Vec<PlanSourceEventTransformArm>,
        #[serde(default)]
        router_route: bool,
    },
    BoolNot {
        input: ValueRef,
    },
    NumberCompareConst {
        left: ValueRef,
        op: String,
        right: i64,
    },
    BoolAnd {
        left: Box<PlanDerivedExpression>,
        right: Box<PlanDerivedExpression>,
    },
    BoolNotExpression {
        input: Box<PlanDerivedExpression>,
    },
    RowExpression {
        expression: PlanRowExpression,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PlanSourceEventTransformArm {
    pub source_id: SourceId,
    pub value: PlanRowExpression,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PlanRowExpression {
    Field {
        input: ValueRef,
    },
    Constant {
        constant_id: PlanConstantId,
    },
    TextTrim {
        input: Box<PlanRowExpression>,
    },
    TextIsEmpty {
        input: Box<PlanRowExpression>,
    },
    TextStartsWith {
        input: Box<PlanRowExpression>,
        prefix: Box<PlanRowExpression>,
    },
    TextLength {
        input: Box<PlanRowExpression>,
    },
    TextToNumber {
        input: Box<PlanRowExpression>,
    },
    TextSubstring {
        input: Box<PlanRowExpression>,
        start: Box<PlanRowExpression>,
        length: Box<PlanRowExpression>,
    },
    TextToBytes {
        input: Box<PlanRowExpression>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        encoding: Option<Box<PlanRowExpression>>,
    },
    BytesToText {
        input: Box<PlanRowExpression>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        encoding: Option<Box<PlanRowExpression>>,
    },
    BytesToHex {
        input: Box<PlanRowExpression>,
    },
    BytesToBase64 {
        input: Box<PlanRowExpression>,
    },
    BytesFromHex {
        input: Box<PlanRowExpression>,
    },
    BytesFromBase64 {
        input: Box<PlanRowExpression>,
    },
    BytesIsEmpty {
        input: Box<PlanRowExpression>,
    },
    BytesLength {
        input: Box<PlanRowExpression>,
    },
    BytesGet {
        input: Box<PlanRowExpression>,
        index: Box<PlanRowExpression>,
    },
    BytesSlice {
        input: Box<PlanRowExpression>,
        offset: Box<PlanRowExpression>,
        byte_count: Box<PlanRowExpression>,
    },
    BytesTake {
        input: Box<PlanRowExpression>,
        byte_count: Box<PlanRowExpression>,
    },
    BytesDrop {
        input: Box<PlanRowExpression>,
        byte_count: Box<PlanRowExpression>,
    },
    BytesZeros {
        byte_count: Box<PlanRowExpression>,
    },
    BytesReadUnsigned {
        input: Box<PlanRowExpression>,
        offset: Box<PlanRowExpression>,
        byte_count: Box<PlanRowExpression>,
        endian: Box<PlanRowExpression>,
    },
    BytesReadSigned {
        input: Box<PlanRowExpression>,
        offset: Box<PlanRowExpression>,
        byte_count: Box<PlanRowExpression>,
        endian: Box<PlanRowExpression>,
    },
    BytesSet {
        input: Box<PlanRowExpression>,
        index: Box<PlanRowExpression>,
        value: Box<PlanRowExpression>,
    },
    BytesWriteUnsigned {
        input: Box<PlanRowExpression>,
        offset: Box<PlanRowExpression>,
        byte_count: Box<PlanRowExpression>,
        endian: Box<PlanRowExpression>,
        value: Box<PlanRowExpression>,
    },
    BytesWriteSigned {
        input: Box<PlanRowExpression>,
        offset: Box<PlanRowExpression>,
        byte_count: Box<PlanRowExpression>,
        endian: Box<PlanRowExpression>,
        value: Box<PlanRowExpression>,
    },
    BytesFind {
        input: Box<PlanRowExpression>,
        needle: Box<PlanRowExpression>,
    },
    BytesStartsWith {
        input: Box<PlanRowExpression>,
        prefix: Box<PlanRowExpression>,
    },
    BytesEndsWith {
        input: Box<PlanRowExpression>,
        suffix: Box<PlanRowExpression>,
    },
    BytesConcat {
        left: Box<PlanRowExpression>,
        right: Box<PlanRowExpression>,
    },
    BytesEqual {
        left: Box<PlanRowExpression>,
        right: Box<PlanRowExpression>,
    },
    NumberInfix {
        op: String,
        left: Box<PlanRowExpression>,
        right: Box<PlanRowExpression>,
    },
    TextConcat {
        parts: Vec<PlanRowExpression>,
    },
    ListGetField {
        list_id: ListId,
        index: Box<PlanRowExpression>,
        field: FieldId,
    },
    ListRef {
        list_id: ListId,
    },
    ListFindValue {
        list_id: ListId,
        field: FieldId,
        value: Box<PlanRowExpression>,
        target: FieldId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        fallback: Option<Box<PlanRowExpression>>,
    },
    ListRange {
        from: Box<PlanRowExpression>,
        to: Box<PlanRowExpression>,
    },
    ListLiteral {
        items: Vec<PlanRowExpression>,
    },
    ListMap {
        input: Box<PlanRowExpression>,
        binding: String,
        value: Box<PlanRowExpression>,
    },
    ListMapItem {
        binding: String,
    },
    ListSum {
        input: Box<PlanRowExpression>,
    },
    Object {
        fields: Vec<PlanRowObjectField>,
    },
    ObjectField {
        object: Box<PlanRowExpression>,
        field: String,
    },
    BuiltinCall {
        function: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        input: Option<Box<PlanRowExpression>>,
        args: Vec<PlanRowCallArg>,
    },
    Select {
        input: Box<PlanRowExpression>,
        arms: Vec<PlanRowSelectArm>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PlanRowObjectField {
    pub name: String,
    pub value: PlanRowExpression,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PlanRowCallArg {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub value: PlanRowExpression,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PlanRowSelectArm {
    pub pattern: PlanRowSelectPattern,
    pub value: PlanRowExpression,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PlanRowSelectPattern {
    Bool { value: bool },
    Text { value: String },
    Number { value: i64 },
    NaN,
    Wildcard,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PlanSourceGuard {
    SourcePayloadOneOf {
        source_id: SourceId,
        field: SourcePayloadField,
        values: Vec<String>,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanExpressionKind {
    SourcePayload,
    Const,
    NumberInfix,
    ProjectTime,
    PreviousValue,
    ReadPath,
    TextTrimOrPrevious,
    PrefixPayloadConcat,
    PrefixRootConcat,
    BoolNot,
    BytesLength,
    BytesIsEmpty,
    BytesGet,
    BytesSet,
    BytesSlice,
    BytesTake,
    BytesDrop,
    BytesZeros,
    BytesToHex,
    BytesFromHex,
    BytesToBase64,
    BytesFromBase64,
    BytesReadUnsigned,
    BytesReadSigned,
    BytesWriteUnsigned,
    BytesWriteSigned,
    FileReadBytes,
    FileWriteBytes,
    TextToBytes,
    BytesToText,
    BytesConcat,
    BytesEqual,
    BytesFind,
    BytesStartsWith,
    BytesEndsWith,
    MatchConst,
    MatchValueConst,
    MatchTextIsEmptyConst,
    MatchInfixConst,
    ListFindValue,
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanListOperationKind {
    Append,
    Remove,
    Retain,
    Count,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PlanListAppend {
    pub trigger: ValueRef,
    pub fields: Vec<PlanListAppendField>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PlanListAppendField {
    pub name: String,
    pub field_id: Option<FieldId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value_ref: Option<ValueRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub constant_id: Option<PlanConstantId>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PlanListRemove {
    pub source: ValueRef,
    pub predicate: PlanListRemovePredicate,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PlanListRetain {
    pub target: ValueRef,
    pub predicate: PlanListRemovePredicate,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PlanListCount {
    pub target: ValueRef,
    pub predicate: PlanListRemovePredicate,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PlanListRemovePredicate {
    AlwaysTrue,
    RowFieldBool {
        input: ValueRef,
    },
    RowFieldBoolNot {
        input: ValueRef,
    },
    SelectedFilterVisibility {
        selector: ValueRef,
        row_field: ValueRef,
    },
    Unknown {
        summary: String,
    },
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(tag = "kind", content = "id", rename_all = "snake_case")]
pub enum ValueRef {
    Source(SourceId),
    SourcePayload {
        source_id: SourceId,
        field: SourcePayloadField,
    },
    State(StateId),
    Field(FieldId),
    List(ListId),
    Constant(PlanConstantId),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DirtyPlan {
    pub dependency_edges: usize,
    pub unresolved_dependency_edges: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CommitPlan {
    pub update_branch_count: usize,
    pub unresolved_update_branch_count: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DeltaPlan {
    pub deltas: Vec<DeltaRoute>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DeltaRoute {
    pub id: PlanDeltaId,
    pub output: ValueRef,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CapabilitySummary {
    pub executable: bool,
    pub typed_lowering_executable: bool,
    pub cpu_plan_executor_complete: bool,
    pub constant_count: usize,
    pub source_route_count: usize,
    pub scalar_storage_count: usize,
    pub list_storage_count: usize,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub byte_bank_storage_count: usize,
    pub operation_count: usize,
    pub typed_value_ref_count: usize,
    pub executable_string_path_count: usize,
    pub unresolved_executable_ref_count: usize,
    pub unknown_plan_op_count: usize,
    pub cpu_plan_executor_unsupported_op_count: usize,
    pub runtime_ast_dependency_count: usize,
    pub graph_rebuild_count: usize,
    pub graph_clones_per_item: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DebugMap {
    pub source_units: Vec<DebugEntry>,
    pub source_routes: Vec<DebugEntry>,
    pub state_slots: Vec<DebugEntry>,
    pub list_slots: Vec<DebugEntry>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub derived_values: Vec<DebugEntry>,
    pub fields: Vec<DebugEntry>,
    pub unresolved_executable_refs: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DebugEntry {
    pub id: String,
    pub label: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PlanVerification {
    pub status: String,
    pub plan_version: PlanVersion,
    pub plan_hash: String,
    pub error_count: usize,
    pub warning_count: usize,
    pub checks: Vec<PlanCheck>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PlanCheck {
    pub id: String,
    pub pass: bool,
    pub detail: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlanError {
    message: String,
}

impl PlanError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for PlanError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.message.fmt(formatter)
    }
}

impl Error for PlanError {}

impl serde::ser::Error for PlanError {
    fn custom<T>(message: T) -> Self
    where
        T: fmt::Display,
    {
        Self::new(message.to_string())
    }
}

pub fn verify_plan(plan: &MachinePlan) -> Result<PlanVerification, PlanError> {
    let mut checks = Vec::new();
    checks.push(PlanCheck {
        id: "plan-version-supported".to_owned(),
        pass: plan.version.major == PLAN_MAJOR_VERSION,
        detail: format!("plan version {}.{}", plan.version.major, plan.version.minor),
    });
    checks.push(PlanCheck {
        id: "source-routes-use-typed-ids".to_owned(),
        pass: plan
            .source_routes
            .iter()
            .enumerate()
            .all(|(index, route)| route.id.0 == index),
        detail: format!("{} source routes", plan.source_routes.len()),
    });
    checks.push(PlanCheck {
        id: "scheduled-source-intervals-positive".to_owned(),
        pass: plan
            .source_routes
            .iter()
            .all(|route| route.interval_ms.is_none_or(|interval| interval > 0)),
        detail: format!(
            "{} scheduled source routes",
            plan.source_routes
                .iter()
                .filter(|route| route.interval_ms.is_some())
                .count()
        ),
    });
    checks.push(PlanCheck {
        id: "storage-slots-unique".to_owned(),
        pass: unique_storage_ids(&plan.storage_layout),
        detail: format!(
            "{} scalar slots, {} list slots",
            plan.storage_layout.scalar_slots.len(),
            plan.storage_layout.list_slots.len()
        ),
    });
    checks.push(PlanCheck {
        id: "operation-ids-unique".to_owned(),
        pass: unique_operation_ids(&plan.regions),
        detail: format!("{} operation regions", plan.regions.len()),
    });
    checks.push(PlanCheck {
        id: "root-output-demand-resolves".to_owned(),
        pass: root_output_demand_resolves(plan),
        detail: match &plan.demand.root_derived_outputs {
            RootOutputDemand::All => "all root-derived outputs are demanded".to_owned(),
            RootOutputDemand::Selected(field_ids) => format!(
                "{} sorted unique root-derived output field id(s) are demanded",
                field_ids.len()
            ),
        },
    });
    let document_failure = plan
        .document
        .as_ref()
        .and_then(|document| document.verify(plan).err());
    checks.push(PlanCheck {
        id: "document-plan-typed-and-resolved".to_owned(),
        pass: document_failure.is_none(),
        detail: document_failure.unwrap_or_else(|| match &plan.document {
            Some(document) => format!(
                "{} document expression(s), {} template(s), {} materialization point(s)",
                document.expressions.len(),
                document.templates.len(),
                document.materializations.len()
            ),
            None => "no document output root".to_owned(),
        }),
    });
    checks.push(PlanCheck {
        id: "byte-constants-match-hashes".to_owned(),
        pass: byte_constants_match_hashes(&plan.constants),
        detail: format!("{} constant(s)", plan.constants.len()),
    });
    checks.push(PlanCheck {
        id: "plan-constants-deduplicated".to_owned(),
        pass: plan_constants_are_deduplicated(&plan.constants),
        detail: format!("{} constant(s)", plan.constants.len()),
    });
    let byte_bank_mismatch_count = byte_bank_layout_mismatch_count(plan);
    checks.push(PlanCheck {
        id: "byte-bank-slots-match-fixed-bytes".to_owned(),
        pass: byte_bank_mismatch_count == 0,
        detail: format!(
            "{} byte bank(s), {byte_bank_mismatch_count} fixed-BYTES storage mismatch(es)",
            plan.storage_layout.byte_banks.len()
        ),
    });
    let constant_refs_failure = constant_refs_resolve_and_match_storage_types_failure(plan);
    checks.push(PlanCheck {
        id: "constant-refs-resolve-and-match-storage-types".to_owned(),
        pass: constant_refs_failure.is_none(),
        detail: constant_refs_failure.unwrap_or_else(|| {
            "initial and update constant refs resolve to compatible typed constants".to_owned()
        }),
    });
    checks.push(PlanCheck {
        id: "initial-field-paths-resolve".to_owned(),
        pass: initial_field_paths_resolve(plan),
        detail: "root-initial and row-initial scalar slots carry source field paths".to_owned(),
    });
    checks.push(PlanCheck {
        id: "list-initial-row-fields-resolve".to_owned(),
        pass: list_initial_row_fields_resolve(plan),
        detail: "record-literal list initial rows carry typed row field ids".to_owned(),
    });
    checks.push(PlanCheck {
        id: "list-range-bounds-resolve".to_owned(),
        pass: list_range_bounds_resolve(plan),
        detail: "range list initializers carry typed finite bounds".to_owned(),
    });
    checks.push(PlanCheck {
        id: "list-append-refs-resolve".to_owned(),
        pass: list_append_refs_resolve(plan),
        detail:
            "append triggers, destination row fields, and values use typed refs or typed constants"
                .to_owned(),
    });
    checks.push(PlanCheck {
        id: "list-remove-refs-resolve".to_owned(),
        pass: list_remove_refs_resolve(plan),
        detail: "remove sources and predicates use typed refs".to_owned(),
    });
    checks.push(PlanCheck {
        id: "list-retain-refs-resolve".to_owned(),
        pass: list_retain_refs_resolve(plan),
        detail: "retain targets and predicates use typed refs".to_owned(),
    });
    checks.push(PlanCheck {
        id: "list-count-refs-resolve".to_owned(),
        pass: list_count_refs_resolve(plan),
        detail: "count targets and predicates use typed refs".to_owned(),
    });
    checks.push(PlanCheck {
        id: "list-projection-refs-resolve".to_owned(),
        pass: list_projection_refs_resolve(plan),
        detail: "list projections carry typed source list, value, and output refs".to_owned(),
    });
    let malformed_file_read_bytes_op_count = malformed_file_read_bytes_op_count(plan);
    checks.push(PlanCheck {
        id: "file-read-bytes-ops-well-formed".to_owned(),
        pass: malformed_file_read_bytes_op_count == 0,
        detail: format!("{malformed_file_read_bytes_op_count} malformed FileReadBytes op(s)"),
    });
    let malformed_file_write_bytes_op_count = malformed_file_write_bytes_op_count(plan);
    checks.push(PlanCheck {
        id: "file-write-bytes-ops-well-formed".to_owned(),
        pass: malformed_file_write_bytes_op_count == 0,
        detail: format!("{malformed_file_write_bytes_op_count} malformed FileWriteBytes op(s)"),
    });
    checks.push(PlanCheck {
        id: "derived-expression-refs-resolve".to_owned(),
        pass: derived_expression_refs_resolve(plan),
        detail: "derived expression operands are present as typed refs".to_owned(),
    });
    checks.push(PlanCheck {
        id: "row-expression-list-fields-resolve".to_owned(),
        pass: row_expression_list_fields_resolve(plan),
        detail: "row list lookup expressions use field ids owned by their source list".to_owned(),
    });
    let expected_capabilities = computed_capability_summary(plan);
    checks.push(PlanCheck {
        id: "capability-summary-derived-counts".to_owned(),
        pass: plan.capability_summary == expected_capabilities,
        detail: format!(
            "reported executable={}, expected executable={}, reported typed_lowering_executable={}, expected typed_lowering_executable={}, reported cpu_plan_executor_unsupported_op_count={}, expected cpu_plan_executor_unsupported_op_count={}, reported unknown_plan_op_count={}, expected unknown_plan_op_count={}",
            plan.capability_summary.executable,
            expected_capabilities.executable,
            plan.capability_summary.typed_lowering_executable,
            expected_capabilities.typed_lowering_executable,
            plan.capability_summary.cpu_plan_executor_unsupported_op_count,
            expected_capabilities.cpu_plan_executor_unsupported_op_count,
            plan.capability_summary.unknown_plan_op_count,
            expected_capabilities.unknown_plan_op_count
        ),
    });
    checks.push(PlanCheck {
        id: "debug-map-separated-from-operation-refs".to_owned(),
        pass: true,
        detail: "human-readable labels are isolated in debug_map".to_owned(),
    });
    let error_count = checks.iter().filter(|check| !check.pass).count();
    Ok(PlanVerification {
        status: if error_count == 0 { "pass" } else { "fail" }.to_owned(),
        plan_version: plan.version,
        plan_hash: plan_sha256(plan)?,
        error_count,
        warning_count: usize::from(!plan.capability_summary.typed_lowering_executable),
        checks,
    })
}

fn root_output_demand_resolves(plan: &MachinePlan) -> bool {
    let RootOutputDemand::Selected(field_ids) = &plan.demand.root_derived_outputs else {
        return true;
    };
    if !field_ids.windows(2).all(|pair| pair[0] < pair[1]) {
        return false;
    }
    let root_outputs = plan
        .regions
        .iter()
        .filter(|region| region.kind == RegionKind::DerivedEvaluation)
        .flat_map(|region| &region.ops)
        .filter(|op| !op.indexed)
        .filter_map(|op| match op.output {
            Some(ValueRef::Field(field_id)) => Some(field_id),
            _ => None,
        })
        .collect::<BTreeSet<_>>();
    field_ids
        .iter()
        .all(|field_id| root_outputs.contains(field_id))
}

pub fn plan_sha256(plan: &MachinePlan) -> Result<String, PlanError> {
    let bytes = binary::encode(plan)?;
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    Ok(format!("{:x}", hasher.finalize()))
}

fn computed_capability_summary(plan: &MachinePlan) -> CapabilitySummary {
    let operation_count = plan
        .regions
        .iter()
        .map(|region| region.ops.len())
        .sum::<usize>();
    let unresolved_executable_ref_count = plan
        .regions
        .iter()
        .flat_map(|region| &region.ops)
        .map(|op| op.unresolved_executable_ref_count)
        .sum::<usize>();
    let typed_value_ref_count = plan
        .regions
        .iter()
        .flat_map(|region| &region.ops)
        .map(|op| op.inputs.len() + usize::from(op.output.is_some()))
        .sum::<usize>();
    let unknown_region_op_count = plan
        .regions
        .iter()
        .flat_map(|region| &region.ops)
        .filter(|op| is_unknown_op(op))
        .count();
    let unknown_storage_op_count = plan
        .storage_layout
        .scalar_slots
        .iter()
        .filter(|slot| matches!(slot.initial_value_kind, InitialValueKind::Unknown))
        .count()
        + plan
            .storage_layout
            .list_slots
            .iter()
            .filter(|slot| matches!(slot.initializer_kind, ListInitializerKind::Unknown))
            .count()
        + non_executable_constant_payload_count(&plan.constants);
    let unknown_plan_op_count = unknown_region_op_count + unknown_storage_op_count;
    let typed_lowering_executable =
        unresolved_executable_ref_count == 0 && unknown_plan_op_count == 0;
    let cpu_plan_executor_unsupported_op_count = cpu_plan_executor_unsupported_op_count(
        &plan.regions,
        &plan.storage_layout.list_slots,
        &plan.storage_layout.scalar_slots,
        &plan.constants,
    );
    let cpu_plan_executor_complete =
        typed_lowering_executable && cpu_plan_executor_unsupported_op_count == 0;
    CapabilitySummary {
        executable: cpu_plan_executor_complete,
        typed_lowering_executable,
        cpu_plan_executor_complete,
        constant_count: plan.constants.len(),
        source_route_count: plan.source_routes.len(),
        scalar_storage_count: plan.storage_layout.scalar_slots.len(),
        list_storage_count: plan.storage_layout.list_slots.len(),
        byte_bank_storage_count: plan.storage_layout.byte_banks.len(),
        operation_count,
        typed_value_ref_count,
        executable_string_path_count: unresolved_executable_ref_count,
        unresolved_executable_ref_count,
        unknown_plan_op_count,
        cpu_plan_executor_unsupported_op_count,
        runtime_ast_dependency_count: 0,
        graph_rebuild_count: 0,
        graph_clones_per_item: 0,
    }
}

pub fn cpu_plan_executor_unsupported_op_count(
    regions: &[OperationRegion],
    list_slots: &[ListStorageSlot],
    scalar_slots: &[ScalarStorageSlot],
    constants: &[PlanConstant],
) -> usize {
    let supported_list_projection_outputs = regions
        .iter()
        .flat_map(|region| &region.ops)
        .filter_map(|op| match (&op.kind, op.output.clone()) {
            (PlanOpKind::ListProjection { projection }, Some(ValueRef::Field(field_id)))
                if cpu_plan_executor_supports_list_projection_op(op, projection) =>
            {
                Some(field_id)
            }
            _ => None,
        })
        .collect::<BTreeSet<_>>();
    let supported_list_count_outputs = regions
        .iter()
        .flat_map(|region| &region.ops)
        .filter_map(|op| match (&op.kind, op.output.clone()) {
            (
                PlanOpKind::ListOperation {
                    operation_kind,
                    append,
                    remove,
                    retain,
                    count: Some(count),
                    ..
                },
                Some(ValueRef::List(_)),
            ) if cpu_plan_executor_supports_list_operation_op(
                op,
                *operation_kind,
                append.as_ref(),
                remove.as_ref(),
                retain.as_ref(),
                Some(count),
                constants,
            ) =>
            {
                match count.target {
                    ValueRef::Field(field_id) => Some(field_id),
                    _ => None,
                }
            }
            _ => None,
        })
        .collect::<BTreeSet<_>>();
    let supported_list_retain_outputs = regions
        .iter()
        .flat_map(|region| &region.ops)
        .filter_map(|op| match (&op.kind, op.output.clone()) {
            (
                PlanOpKind::ListOperation {
                    operation_kind,
                    append,
                    remove,
                    retain: Some(retain),
                    count,
                    ..
                },
                Some(ValueRef::List(_)),
            ) if cpu_plan_executor_supports_list_operation_op(
                op,
                *operation_kind,
                append.as_ref(),
                remove.as_ref(),
                Some(retain),
                count.as_ref(),
                constants,
            ) =>
            {
                match retain.target {
                    ValueRef::Field(field_id) => Some(field_id),
                    _ => None,
                }
            }
            _ => None,
        })
        .collect::<BTreeSet<_>>();
    regions
        .iter()
        .flat_map(|region| &region.ops)
        .filter(|op| {
            !cpu_plan_executor_supports_whole_plan_op(
                scalar_slots,
                list_slots,
                constants,
                op,
                &supported_list_projection_outputs,
                &supported_list_count_outputs,
                &supported_list_retain_outputs,
            )
        })
        .count()
}

pub fn cpu_plan_executor_supports_whole_plan_op(
    scalar_slots: &[ScalarStorageSlot],
    list_slots: &[ListStorageSlot],
    constants: &[PlanConstant],
    op: &PlanOp,
    supported_list_projection_outputs: &BTreeSet<FieldId>,
    supported_list_count_outputs: &BTreeSet<FieldId>,
    supported_list_retain_outputs: &BTreeSet<FieldId>,
) -> bool {
    match &op.kind {
        PlanOpKind::SourceRoute
        | PlanOpKind::StateInitialize { .. }
        | PlanOpKind::DependencyEdge => true,
        PlanOpKind::UpdateBranch {
            expression_kind,
            ordered_inputs: _,
            source_payload_field,
            update_constant_id,
            source_guard,
        } => {
            if op.unresolved_executable_ref_count != 0 {
                return false;
            }
            if op.indexed {
                return cpu_plan_executor_supports_indexed_update_op(
                    scalar_slots,
                    list_slots,
                    constants,
                    op,
                    expression_kind,
                    source_payload_field,
                    update_constant_id,
                    source_guard,
                );
            }
            match expression_kind {
                PlanExpressionKind::SourcePayload => {
                    let Some(field) = source_payload_field.as_ref() else {
                        return false;
                    };
                    update_constant_id.is_none()
                        && source_payload_output_type_is_supported(scalar_slots, op, field)
                        && update_branch_source_ids(op).len() == 1
                        && source_payload_input_matches_single_source(op, field)
                        && state_input_ids(op).is_empty()
                }
                PlanExpressionKind::Const => {
                    source_payload_field.is_none()
                        && matches!(
                            (update_constant_id, output_state_type(scalar_slots, op)),
                            (Some(constant_id), Some(output_type))
                                if plan_constant_by_id(constants, *constant_id)
                                    .is_some_and(|constant| {
                                        constant_value_matches_plan_type(
                                            &constant.value,
                                            output_type,
                                        )
                                    })
                        )
                        && update_branch_source_ids(op).len() == 1
                        && state_input_ids(op).is_empty()
                        && source_payload_inputs_are_empty_or_guard_only(op, source_guard)
                }
                PlanExpressionKind::BoolNot => {
                    source_payload_field.is_none()
                        && update_constant_id.is_none()
                        && output_state_type_is(scalar_slots, op, &PlanValueType::Bool)
                        && update_branch_source_ids(op).len() == 1
                        && single_state_input_type_is(scalar_slots, op, &PlanValueType::Bool)
                        && source_payload_input_ids(op).is_empty()
                }
                PlanExpressionKind::BytesLength => {
                    source_payload_field.is_none()
                        && update_constant_id.is_none()
                        && output_state_type_is(scalar_slots, op, &PlanValueType::Number)
                        && update_branch_source_ids(op).len() == 1
                        && bytes_length_input_is_supported(scalar_slots, op)
                        && source_payload_input_ids(op).is_empty()
                }
                PlanExpressionKind::BytesIsEmpty => {
                    source_payload_field.is_none()
                        && update_constant_id.is_none()
                        && output_state_type_is(scalar_slots, op, &PlanValueType::Bool)
                        && update_branch_source_ids(op).len() == 1
                        && bytes_length_input_is_supported(scalar_slots, op)
                        && source_payload_input_ids(op).is_empty()
                }
                PlanExpressionKind::BytesGet => {
                    source_payload_field.is_none()
                        && update_constant_id
                            .and_then(|constant_id| plan_constant_by_id(constants, constant_id))
                            .is_some_and(|constant| {
                                matches!(
                                    constant.value,
                                    PlanConstantValue::Number { value } if value >= 0
                                )
                            })
                        && output_state_type_is(scalar_slots, op, &PlanValueType::Byte)
                        && update_branch_source_ids(op).len() == 1
                        && bytes_length_input_is_supported(scalar_slots, op)
                        && source_payload_input_ids(op).is_empty()
                }
                PlanExpressionKind::BytesSet => {
                    source_payload_field.is_none()
                        && update_constant_id.is_none()
                        && output_state_is_bytes(scalar_slots, op)
                        && update_branch_source_ids(op).len() == 1
                        && bytes_set_inputs_are_supported(scalar_slots, constants, op)
                        && bytes_set_fixed_lengths_match(scalar_slots, op)
                        && source_payload_input_ids(op).is_empty()
                }
                PlanExpressionKind::BytesSlice => {
                    source_payload_field.is_none()
                        && update_constant_id.is_none()
                        && output_state_is_bytes(scalar_slots, op)
                        && update_branch_source_ids(op).len() == 1
                        && bytes_slice_inputs_are_supported(scalar_slots, constants, op)
                        && bytes_slice_fixed_lengths_match(scalar_slots, constants, op)
                        && source_payload_input_ids(op).is_empty()
                }
                PlanExpressionKind::BytesTake => {
                    source_payload_field.is_none()
                        && update_constant_id.is_none()
                        && output_state_is_bytes(scalar_slots, op)
                        && update_branch_source_ids(op).len() == 1
                        && bytes_take_inputs_are_supported(scalar_slots, constants, op)
                        && bytes_take_fixed_lengths_match(scalar_slots, constants, op)
                        && source_payload_input_ids(op).is_empty()
                }
                PlanExpressionKind::BytesDrop => {
                    source_payload_field.is_none()
                        && update_constant_id.is_none()
                        && output_state_is_bytes(scalar_slots, op)
                        && update_branch_source_ids(op).len() == 1
                        && bytes_drop_inputs_are_supported(scalar_slots, constants, op)
                        && bytes_drop_fixed_lengths_match(scalar_slots, constants, op)
                        && source_payload_input_ids(op).is_empty()
                }
                PlanExpressionKind::BytesZeros => {
                    source_payload_field.is_none()
                        && update_constant_id.is_none()
                        && output_state_is_bytes(scalar_slots, op)
                        && update_branch_source_ids(op).len() == 1
                        && bytes_zeros_inputs_are_supported(scalar_slots, constants, op)
                        && bytes_zeros_fixed_length_matches(scalar_slots, constants, op)
                        && source_payload_input_ids(op).is_empty()
                }
                PlanExpressionKind::BytesToHex | PlanExpressionKind::BytesToBase64 => {
                    source_payload_field.is_none()
                        && update_constant_id.is_none()
                        && output_state_type_is(scalar_slots, op, &PlanValueType::Text)
                        && update_branch_source_ids(op).len() == 1
                        && single_state_input_type_matches(scalar_slots, op, |value_type| {
                            matches!(value_type, PlanValueType::Bytes { .. })
                        })
                        && source_payload_input_ids(op).is_empty()
                }
                PlanExpressionKind::BytesFromHex | PlanExpressionKind::BytesFromBase64 => {
                    source_payload_field.is_none()
                        && update_constant_id.is_none()
                        && output_state_is_bytes(scalar_slots, op)
                        && update_branch_source_ids(op).len() == 1
                        && single_state_input_type_is(scalar_slots, op, &PlanValueType::Text)
                        && source_payload_input_ids(op).is_empty()
                }
                PlanExpressionKind::BytesReadUnsigned | PlanExpressionKind::BytesReadSigned => {
                    source_payload_field.is_none()
                        && update_constant_id.is_none()
                        && output_state_type_is(scalar_slots, op, &PlanValueType::Number)
                        && update_branch_source_ids(op).len() == 1
                        && bytes_numeric_read_inputs_are_supported(scalar_slots, constants, op)
                        && source_payload_input_ids(op).is_empty()
                }
                PlanExpressionKind::BytesWriteUnsigned => {
                    source_payload_field.is_none()
                        && update_constant_id.is_none()
                        && output_state_is_bytes(scalar_slots, op)
                        && update_branch_source_ids(op).len() == 1
                        && bytes_numeric_write_inputs_are_supported(
                            scalar_slots,
                            constants,
                            op,
                            false,
                        )
                        && bytes_numeric_write_fixed_lengths_match(scalar_slots, op)
                        && source_payload_input_ids(op).is_empty()
                }
                PlanExpressionKind::BytesWriteSigned => {
                    source_payload_field.is_none()
                        && update_constant_id.is_none()
                        && output_state_is_bytes(scalar_slots, op)
                        && update_branch_source_ids(op).len() == 1
                        && bytes_numeric_write_inputs_are_supported(
                            scalar_slots,
                            constants,
                            op,
                            true,
                        )
                        && bytes_numeric_write_fixed_lengths_match(scalar_slots, op)
                        && source_payload_input_ids(op).is_empty()
                }
                PlanExpressionKind::FileReadBytes => {
                    source_payload_field.is_none()
                        && update_constant_id.is_none()
                        && output_state_is_bytes(scalar_slots, op)
                        && update_branch_source_ids(op).len() == 1
                        && file_read_bytes_inputs_are_supported(scalar_slots, constants, op)
                        && source_payload_input_ids(op).is_empty()
                }
                PlanExpressionKind::FileWriteBytes => {
                    source_payload_field.is_none()
                        && update_constant_id.is_none()
                        && output_state_type_is(scalar_slots, op, &PlanValueType::Text)
                        && update_branch_source_ids(op).len() == 1
                        && file_write_bytes_inputs_are_supported(scalar_slots, constants, op)
                        && source_payload_input_ids(op).is_empty()
                }
                PlanExpressionKind::TextToBytes => {
                    source_payload_field.is_none()
                        && update_constant_id.is_none()
                        && output_state_is_bytes(scalar_slots, op)
                        && update_branch_source_ids(op).len() == 1
                        && text_to_bytes_inputs_are_supported(scalar_slots, constants, op)
                        && source_payload_input_ids(op).is_empty()
                }
                PlanExpressionKind::BytesToText => {
                    source_payload_field.is_none()
                        && update_constant_id.is_none()
                        && output_state_type_is(scalar_slots, op, &PlanValueType::Text)
                        && update_branch_source_ids(op).len() == 1
                        && bytes_to_text_inputs_are_supported(scalar_slots, constants, op)
                        && source_payload_input_ids(op).is_empty()
                }
                PlanExpressionKind::BytesEqual => {
                    source_payload_field.is_none()
                        && update_constant_id.is_none()
                        && output_state_type_is(scalar_slots, op, &PlanValueType::Bool)
                        && update_branch_source_ids(op).len() == 1
                        && bytes_equal_inputs_are_supported(scalar_slots, op)
                        && source_payload_input_ids(op).is_empty()
                }
                PlanExpressionKind::BytesFind => {
                    source_payload_field.is_none()
                        && update_constant_id.is_none()
                        && output_state_type_is(scalar_slots, op, &PlanValueType::Number)
                        && update_branch_source_ids(op).len() == 1
                        && bytes_search_inputs_are_supported(scalar_slots, op)
                        && source_payload_input_ids(op).is_empty()
                }
                PlanExpressionKind::BytesStartsWith | PlanExpressionKind::BytesEndsWith => {
                    source_payload_field.is_none()
                        && update_constant_id.is_none()
                        && output_state_type_is(scalar_slots, op, &PlanValueType::Bool)
                        && update_branch_source_ids(op).len() == 1
                        && bytes_search_inputs_are_supported(scalar_slots, op)
                        && source_payload_input_ids(op).is_empty()
                }
                PlanExpressionKind::BytesConcat => {
                    source_payload_field.is_none()
                        && update_constant_id.is_none()
                        && output_state_is_bytes(scalar_slots, op)
                        && update_branch_source_ids(op).len() == 1
                        && bytes_concat_inputs_are_supported(scalar_slots, op)
                        && bytes_concat_fixed_lengths_match(scalar_slots, op)
                        && source_payload_input_ids(op).is_empty()
                }
                PlanExpressionKind::ReadPath => {
                    source_payload_field.is_none()
                        && update_constant_id.is_none()
                        && update_branch_source_ids(op).len() == 1
                        && read_path_inputs_supported(scalar_slots, op)
                }
                PlanExpressionKind::PreviousValue => {
                    source_payload_field.is_none()
                        && update_constant_id.is_none()
                        && update_branch_source_ids(op).len() == 1
                        && previous_value_inputs_supported(scalar_slots, op)
                        && source_payload_input_ids(op).is_empty()
                }
                PlanExpressionKind::TextTrimOrPrevious => {
                    update_constant_id.is_none()
                        && output_state_type_is(scalar_slots, op, &PlanValueType::Text)
                        && update_branch_source_ids(op).len() == 1
                        && text_trim_or_previous_inputs_supported(
                            scalar_slots,
                            constants,
                            op,
                            source_payload_field,
                        )
                        && (source_payload_field.is_some()
                            || source_payload_inputs_are_empty_or_guard_only(op, source_guard))
                }
                PlanExpressionKind::PrefixPayloadConcat => {
                    let Some(field) = source_payload_field.as_ref() else {
                        return false;
                    };
                    update_constant_id.is_none()
                        && output_state_type_is(scalar_slots, op, &PlanValueType::Text)
                        && update_branch_source_ids(op).len() == 1
                        && source_payload_input_matches_single_source(op, field)
                        && prefix_concat_inputs_supported(scalar_slots, constants, op, true)
                }
                PlanExpressionKind::PrefixRootConcat => {
                    source_payload_field.is_none()
                        && update_constant_id.is_none()
                        && output_state_type_is(scalar_slots, op, &PlanValueType::Text)
                        && update_branch_source_ids(op).len() == 1
                        && source_payload_input_ids(op).is_empty()
                        && prefix_concat_inputs_supported(scalar_slots, constants, op, false)
                }
                PlanExpressionKind::MatchConst => {
                    source_payload_field.is_none()
                        && update_constant_id.is_none()
                        && update_branch_source_ids(op).len() == 1
                        && root_match_const_inputs_supported(scalar_slots, constants, op)
                }
                PlanExpressionKind::MatchValueConst => {
                    source_payload_field.is_none()
                        && update_constant_id.is_none()
                        && update_branch_source_ids(op).len() == 1
                        && root_match_value_const_inputs_supported(scalar_slots, constants, op)
                }
                PlanExpressionKind::NumberInfix => {
                    source_payload_field.is_none()
                        && update_constant_id.is_none()
                        && output_state_type_is(scalar_slots, op, &PlanValueType::Number)
                        && update_branch_source_ids(op).len() == 1
                        && root_number_infix_inputs_supported(scalar_slots, constants, op)
                }
                PlanExpressionKind::ProjectTime
                | PlanExpressionKind::MatchTextIsEmptyConst
                | PlanExpressionKind::MatchInfixConst
                | PlanExpressionKind::ListFindValue
                | PlanExpressionKind::Unknown => false,
            }
        }
        PlanOpKind::DerivedValue { .. } => cpu_plan_executor_supports_derived_value_op(
            scalar_slots,
            op,
            supported_list_projection_outputs,
            supported_list_count_outputs,
            supported_list_retain_outputs,
        ),
        PlanOpKind::ListOperation {
            operation_kind,
            append,
            remove,
            retain,
            count,
            ..
        } => cpu_plan_executor_supports_list_operation_op(
            op,
            *operation_kind,
            append.as_ref(),
            remove.as_ref(),
            retain.as_ref(),
            count.as_ref(),
            constants,
        ),
        PlanOpKind::ListProjection { projection } => {
            cpu_plan_executor_supports_list_projection_op(op, projection)
        }
    }
}

pub fn cpu_plan_executor_supports_list_operation_op(
    op: &PlanOp,
    operation_kind: PlanListOperationKind,
    append: Option<&PlanListAppend>,
    remove: Option<&PlanListRemove>,
    retain: Option<&PlanListRetain>,
    count: Option<&PlanListCount>,
    constants: &[PlanConstant],
) -> bool {
    if op.unresolved_executable_ref_count != 0 || !op.indexed {
        return false;
    }
    let Some(ValueRef::List(_)) = op.output else {
        return false;
    };
    match operation_kind {
        PlanListOperationKind::Append => {
            let Some(append) = append else {
                return false;
            };
            op.inputs.contains(&append.trigger)
                && append.fields.iter().all(|field| {
                    field.field_id.is_some()
                        && match (&field.value_ref, field.constant_id) {
                            (Some(value_ref), None) => op.inputs.contains(value_ref),
                            (None, Some(constant_id)) => {
                                plan_constant_by_id(constants, constant_id).is_some()
                            }
                            _ => false,
                        }
                })
        }
        PlanListOperationKind::Remove => remove.is_some_and(|remove| {
            op.inputs.contains(&remove.source)
                && cpu_plan_executor_supports_list_predicate(op, &remove.predicate)
        }),
        PlanListOperationKind::Count => count.is_some_and(|count| {
            matches!(count.target, ValueRef::Field(_))
                && op.inputs.contains(&count.target)
                && cpu_plan_executor_supports_list_predicate(op, &count.predicate)
        }),
        PlanListOperationKind::Retain => {
            let Some(retain) = retain else {
                return false;
            };
            matches!(retain.target, ValueRef::Field(_))
                && op.inputs.contains(&retain.target)
                && cpu_plan_executor_supports_list_predicate(op, &retain.predicate)
        }
    }
}

fn cpu_plan_executor_supports_list_predicate(
    op: &PlanOp,
    predicate: &PlanListRemovePredicate,
) -> bool {
    match predicate {
        PlanListRemovePredicate::AlwaysTrue => true,
        PlanListRemovePredicate::RowFieldBool { input }
        | PlanListRemovePredicate::RowFieldBoolNot { input } => {
            matches!(input, ValueRef::State(_)) && op.inputs.contains(input)
        }
        PlanListRemovePredicate::SelectedFilterVisibility {
            selector,
            row_field,
        } => {
            matches!(selector, ValueRef::State(_))
                && matches!(row_field, ValueRef::State(_))
                && op.inputs.contains(selector)
                && op.inputs.contains(row_field)
        }
        PlanListRemovePredicate::Unknown { .. } => false,
    }
}

fn cpu_plan_executor_supports_list_projection_op(
    op: &PlanOp,
    projection: &PlanListProjection,
) -> bool {
    if op.unresolved_executable_ref_count != 0 || !op.indexed {
        return false;
    }
    let Some(ValueRef::Field(_)) = op.output else {
        return false;
    };
    match projection {
        PlanListProjection::Find {
            source_list, value, ..
        } => {
            op.inputs.contains(&ValueRef::List(*source_list))
                && op.inputs.contains(value)
                && matches!(value, ValueRef::State(_))
        }
        PlanListProjection::Chunk {
            source_list, size, ..
        } => *size > 0 && op.inputs.contains(&ValueRef::List(*source_list)),
        PlanListProjection::ChunkValue { source, size, .. } => {
            *size > 0 && op.inputs.contains(source)
        }
        PlanListProjection::Unknown { .. } => false,
    }
}

fn cpu_plan_executor_supports_derived_value_op(
    scalar_slots: &[ScalarStorageSlot],
    op: &PlanOp,
    supported_list_projection_outputs: &BTreeSet<FieldId>,
    supported_list_count_outputs: &BTreeSet<FieldId>,
    supported_list_retain_outputs: &BTreeSet<FieldId>,
) -> bool {
    let Some(ValueRef::Field(_)) = op.output else {
        return false;
    };
    let PlanOpKind::DerivedValue {
        derived_kind,
        expression,
        ..
    } = &op.kind
    else {
        return false;
    };
    if matches!(derived_kind, PlanDerivedKind::ListView)
        && expression.is_none()
        && op.unresolved_executable_ref_count == 0
    {
        let Some(ValueRef::Field(field_id)) = op.output else {
            return false;
        };
        return supported_list_projection_outputs.contains(&field_id)
            || supported_list_retain_outputs.contains(&field_id);
    }
    if matches!(derived_kind, PlanDerivedKind::Aggregate)
        && expression.is_none()
        && op.unresolved_executable_ref_count == 0
    {
        let Some(ValueRef::Field(field_id)) = op.output else {
            return false;
        };
        return supported_list_count_outputs.contains(&field_id);
    }
    let Some(expression) = expression else {
        return false;
    };
    match (op.indexed, derived_kind, expression) {
        (
            false,
            PlanDerivedKind::SourceEventTransform,
            PlanDerivedExpression::SourceKeyTextTrimNonEmpty {
                source_id,
                key_field,
                state,
                ..
            },
        ) => {
            op.inputs.contains(&ValueRef::SourcePayload {
                source_id: *source_id,
                field: key_field.clone(),
            }) && match state {
                ValueRef::State(state_id) => {
                    plan_value_type_for_state_slots(scalar_slots, *state_id)
                        == Some(&PlanValueType::Text)
                        && op.inputs.contains(state)
                }
                ValueRef::SourcePayload {
                    source_id: state_source_id,
                    field: SourcePayloadField::Text,
                } => *state_source_id == *source_id && op.inputs.contains(state),
                _ => false,
            }
        }
        (
            false,
            PlanDerivedKind::SourceEventTransform,
            PlanDerivedExpression::SourceEventTransform { default, arms, .. },
        ) => {
            root_row_expression_cpu_evaluable(default)
                && row_expression_refs_resolve(op, default)
                && !arms.is_empty()
                && arms.iter().all(|arm| {
                    op.inputs.contains(&ValueRef::Source(arm.source_id))
                        && root_row_expression_cpu_evaluable(&arm.value)
                        && row_expression_refs_resolve(op, &arm.value)
                })
        }
        (true, PlanDerivedKind::Pure, PlanDerivedExpression::BoolNot { input }) => {
            matches!(
                input,
                ValueRef::State(state_id)
                    if plan_value_type_for_state_slots(scalar_slots, *state_id)
                        == Some(&PlanValueType::Bool)
                        && scalar_slots
                            .iter()
                            .find(|slot| slot.state_id == *state_id)
                            .is_some_and(|slot| slot.indexed)
                        && op.inputs.contains(input)
            )
        }
        (true, PlanDerivedKind::Pure, PlanDerivedExpression::RowExpression { expression }) => {
            row_expression_refs_resolve(op, expression) && row_expression_cpu_evaluable(expression)
        }
        (false, PlanDerivedKind::Pure, PlanDerivedExpression::RowExpression { expression }) => {
            row_expression_refs_resolve(op, expression)
                && root_row_expression_cpu_evaluable(expression)
        }
        (false, PlanDerivedKind::ListView, PlanDerivedExpression::RowExpression { expression }) => {
            row_expression_refs_resolve(op, expression)
                && root_row_expression_cpu_evaluable(expression)
        }
        (false, PlanDerivedKind::Pure, expression) => {
            root_bool_expression_cpu_supported(op, expression, supported_list_count_outputs)
        }
        _ => false,
    }
}

fn root_bool_expression_cpu_supported(
    op: &PlanOp,
    expression: &PlanDerivedExpression,
    supported_list_count_outputs: &BTreeSet<FieldId>,
) -> bool {
    match expression {
        PlanDerivedExpression::NumberCompareConst {
            left, op: op_name, ..
        } => {
            matches!(op_name.as_str(), ">" | ">=" | "<" | "<=" | "==" | "!=")
                && matches!(
                    left,
                    ValueRef::Field(field_id)
                        if supported_list_count_outputs.contains(field_id)
                            && op.inputs.contains(left)
                )
        }
        PlanDerivedExpression::BoolAnd { left, right } => {
            root_bool_expression_cpu_supported(op, left, supported_list_count_outputs)
                && root_bool_expression_cpu_supported(op, right, supported_list_count_outputs)
        }
        PlanDerivedExpression::BoolNotExpression { input } => {
            root_bool_expression_cpu_supported(op, input, supported_list_count_outputs)
        }
        _ => false,
    }
}

fn cpu_plan_executor_supports_indexed_update_op(
    scalar_slots: &[ScalarStorageSlot],
    list_slots: &[ListStorageSlot],
    constants: &[PlanConstant],
    op: &PlanOp,
    expression_kind: &PlanExpressionKind,
    source_payload_field: &Option<SourcePayloadField>,
    update_constant_id: &Option<PlanConstantId>,
    source_guard: &Option<PlanSourceGuard>,
) -> bool {
    if !output_state_is_indexed(scalar_slots, op) || update_branch_source_ids(op).len() != 1 {
        return false;
    }
    match expression_kind {
        PlanExpressionKind::SourcePayload => {
            let Some(field) = source_payload_field.as_ref() else {
                return false;
            };
            update_constant_id.is_none()
                && source_payload_output_type_is_supported(scalar_slots, op, field)
                && source_payload_input_matches_single_source(op, field)
                && state_input_ids(op).is_empty()
        }
        PlanExpressionKind::Const => {
            source_payload_field.is_none()
                && matches!(
                    (update_constant_id, output_state_type(scalar_slots, op)),
                    (Some(constant_id), Some(output_type))
                        if plan_constant_by_id(constants, *constant_id).is_some_and(|constant| {
                            constant_value_matches_plan_type(&constant.value, output_type)
                        })
                )
                && state_input_ids(op).is_empty()
                && source_payload_inputs_are_empty_or_guard_only(op, source_guard)
        }
        PlanExpressionKind::BoolNot => {
            source_payload_field.is_none()
                && update_constant_id.is_none()
                && output_state_type_is(scalar_slots, op, &PlanValueType::Bool)
                && indexed_bool_not_inputs_are_supported(scalar_slots, op)
                && source_payload_input_ids(op).is_empty()
        }
        PlanExpressionKind::BytesLength => {
            source_payload_field.is_none()
                && update_constant_id.is_none()
                && output_state_type_is(scalar_slots, op, &PlanValueType::Number)
                && bytes_length_input_is_supported(scalar_slots, op)
                && source_payload_input_ids(op).is_empty()
        }
        PlanExpressionKind::BytesIsEmpty => {
            source_payload_field.is_none()
                && update_constant_id.is_none()
                && output_state_type_is(scalar_slots, op, &PlanValueType::Bool)
                && bytes_length_input_is_supported(scalar_slots, op)
                && source_payload_input_ids(op).is_empty()
        }
        PlanExpressionKind::BytesGet => {
            source_payload_field.is_none()
                && update_constant_id
                    .and_then(|constant_id| plan_constant_by_id(constants, constant_id))
                    .is_some_and(|constant| {
                        matches!(constant.value, PlanConstantValue::Number { value } if value >= 0)
                    })
                && output_state_type_is(scalar_slots, op, &PlanValueType::Byte)
                && bytes_length_input_is_supported(scalar_slots, op)
                && source_payload_input_ids(op).is_empty()
        }
        PlanExpressionKind::BytesSet => {
            source_payload_field.is_none()
                && update_constant_id.is_none()
                && output_state_is_bytes(scalar_slots, op)
                && bytes_set_inputs_are_supported(scalar_slots, constants, op)
                && bytes_set_fixed_lengths_match(scalar_slots, op)
                && source_payload_input_ids(op).is_empty()
        }
        PlanExpressionKind::BytesEqual => {
            source_payload_field.is_none()
                && update_constant_id.is_none()
                && output_state_type_is(scalar_slots, op, &PlanValueType::Bool)
                && indexed_bytes_equal_inputs_are_supported(scalar_slots, list_slots, op)
                && source_payload_input_ids(op).is_empty()
        }
        PlanExpressionKind::BytesFind => {
            source_payload_field.is_none()
                && update_constant_id.is_none()
                && output_state_type_is(scalar_slots, op, &PlanValueType::Number)
                && indexed_bytes_ordered_inputs_are_supported(scalar_slots, list_slots, op)
                && source_payload_input_ids(op).is_empty()
        }
        PlanExpressionKind::BytesStartsWith | PlanExpressionKind::BytesEndsWith => {
            source_payload_field.is_none()
                && update_constant_id.is_none()
                && output_state_type_is(scalar_slots, op, &PlanValueType::Bool)
                && indexed_bytes_ordered_inputs_are_supported(scalar_slots, list_slots, op)
                && source_payload_input_ids(op).is_empty()
        }
        PlanExpressionKind::BytesToHex | PlanExpressionKind::BytesToBase64 => {
            source_payload_field.is_none()
                && update_constant_id.is_none()
                && output_state_type_is(scalar_slots, op, &PlanValueType::Text)
                && bytes_length_input_is_supported(scalar_slots, op)
                && source_payload_input_ids(op).is_empty()
        }
        PlanExpressionKind::BytesToText => {
            source_payload_field.is_none()
                && update_constant_id.is_none()
                && output_state_type_is(scalar_slots, op, &PlanValueType::Text)
                && bytes_to_text_inputs_are_supported(scalar_slots, constants, op)
                && source_payload_input_ids(op).is_empty()
        }
        PlanExpressionKind::TextToBytes => {
            source_payload_field.is_none()
                && update_constant_id.is_none()
                && output_state_is_bytes(scalar_slots, op)
                && indexed_text_to_bytes_inputs_are_supported(scalar_slots, list_slots, constants, op)
                && source_payload_input_ids(op).is_empty()
        }
        PlanExpressionKind::BytesFromHex | PlanExpressionKind::BytesFromBase64 => {
            source_payload_field.is_none()
                && update_constant_id.is_none()
                && output_state_is_bytes(scalar_slots, op)
                && indexed_single_text_input_is_supported(scalar_slots, list_slots, op)
                && source_payload_input_ids(op).is_empty()
        }
        PlanExpressionKind::BytesSlice => {
            source_payload_field.is_none()
                && update_constant_id.is_none()
                && output_state_is_bytes(scalar_slots, op)
                && bytes_slice_inputs_are_supported(scalar_slots, constants, op)
                && bytes_slice_fixed_lengths_match(scalar_slots, constants, op)
                && source_payload_input_ids(op).is_empty()
        }
        PlanExpressionKind::BytesTake => {
            source_payload_field.is_none()
                && update_constant_id.is_none()
                && output_state_is_bytes(scalar_slots, op)
                && bytes_take_inputs_are_supported(scalar_slots, constants, op)
                && bytes_take_fixed_lengths_match(scalar_slots, constants, op)
                && source_payload_input_ids(op).is_empty()
        }
        PlanExpressionKind::BytesDrop => {
            source_payload_field.is_none()
                && update_constant_id.is_none()
                && output_state_is_bytes(scalar_slots, op)
                && bytes_drop_inputs_are_supported(scalar_slots, constants, op)
                && bytes_drop_fixed_lengths_match(scalar_slots, constants, op)
                && source_payload_input_ids(op).is_empty()
        }
        PlanExpressionKind::BytesZeros => {
            source_payload_field.is_none()
                && update_constant_id.is_none()
                && output_state_is_bytes(scalar_slots, op)
                && bytes_zeros_inputs_are_supported(scalar_slots, constants, op)
                && bytes_zeros_fixed_length_matches(scalar_slots, constants, op)
                && source_payload_input_ids(op).is_empty()
        }
        PlanExpressionKind::BytesReadUnsigned | PlanExpressionKind::BytesReadSigned => {
            source_payload_field.is_none()
                && update_constant_id.is_none()
                && output_state_type_is(scalar_slots, op, &PlanValueType::Number)
                && bytes_numeric_read_inputs_are_supported(scalar_slots, constants, op)
                && source_payload_input_ids(op).is_empty()
        }
        PlanExpressionKind::BytesWriteUnsigned => {
            source_payload_field.is_none()
                && update_constant_id.is_none()
                && output_state_is_bytes(scalar_slots, op)
                && bytes_numeric_write_inputs_are_supported(scalar_slots, constants, op, false)
                && bytes_numeric_write_fixed_lengths_match(scalar_slots, op)
                && source_payload_input_ids(op).is_empty()
        }
        PlanExpressionKind::BytesWriteSigned => {
            source_payload_field.is_none()
                && update_constant_id.is_none()
                && output_state_is_bytes(scalar_slots, op)
                && bytes_numeric_write_inputs_are_supported(scalar_slots, constants, op, true)
                && bytes_numeric_write_fixed_lengths_match(scalar_slots, op)
                && source_payload_input_ids(op).is_empty()
        }
        PlanExpressionKind::BytesConcat => {
            source_payload_field.is_none()
                && update_constant_id.is_none()
                && output_state_is_bytes(scalar_slots, op)
                && indexed_bytes_ordered_inputs_are_supported(scalar_slots, list_slots, op)
                && indexed_bytes_concat_fixed_lengths_match(scalar_slots, list_slots, op)
                && source_payload_input_ids(op).is_empty()
        }
        PlanExpressionKind::ReadPath => {
            source_payload_field.is_none()
                && update_constant_id.is_none()
                && indexed_read_path_inputs_supported(scalar_slots, op)
        }
        PlanExpressionKind::PreviousValue => {
            source_payload_field.is_none()
                && update_constant_id.is_none()
                && previous_value_inputs_supported(scalar_slots, op)
                && source_payload_inputs_are_empty_or_guard_only(op, source_guard)
        }
        PlanExpressionKind::TextTrimOrPrevious => {
            update_constant_id.is_none()
                && output_state_type_is(scalar_slots, op, &PlanValueType::Text)
                && text_trim_or_previous_inputs_supported(
                    scalar_slots,
                    constants,
                    op,
                    source_payload_field,
                )
                && (source_payload_field.is_some()
                    || source_payload_inputs_are_empty_or_guard_only(op, source_guard))
        }
        PlanExpressionKind::PrefixPayloadConcat => {
            let Some(field) = source_payload_field.as_ref() else {
                return false;
            };
            update_constant_id.is_none()
                && output_state_type_is(scalar_slots, op, &PlanValueType::Text)
                && source_payload_input_matches_single_source(op, field)
                && prefix_concat_inputs_supported(scalar_slots, constants, op, true)
        }
        PlanExpressionKind::PrefixRootConcat => {
            source_payload_field.is_none()
                && update_constant_id.is_none()
                && output_state_type_is(scalar_slots, op, &PlanValueType::Text)
                && source_payload_input_ids(op).is_empty()
                && prefix_concat_inputs_supported(scalar_slots, constants, op, false)
        }
        PlanExpressionKind::MatchTextIsEmptyConst => {
            source_payload_field.is_none()
                && update_constant_id.is_none()
                && output_state_type_is(scalar_slots, op, &PlanValueType::Text)
                && indexed_match_text_is_empty_const_inputs_supported(scalar_slots, constants, op)
                && source_payload_inputs_are_empty_or_guard_only(op, source_guard)
        }
        PlanExpressionKind::FileWriteBytes => {
            source_payload_field.is_none()
                && update_constant_id.is_none()
                && output_state_type_is(scalar_slots, op, &PlanValueType::Text)
                && file_write_bytes_inputs_are_supported(scalar_slots, constants, op)
        }
        PlanExpressionKind::FileReadBytes => {
            source_payload_field.is_none()
                && update_constant_id.is_none()
                && output_state_is_bytes(scalar_slots, op)
                && file_read_bytes_inputs_are_supported(scalar_slots, constants, op)
                && source_payload_input_ids(op).is_empty()
        }
        PlanExpressionKind::NumberInfix
        | PlanExpressionKind::ProjectTime
        | PlanExpressionKind::MatchConst
        | PlanExpressionKind::MatchValueConst
        | PlanExpressionKind::MatchInfixConst
        | PlanExpressionKind::ListFindValue
        | PlanExpressionKind::Unknown => false,
    }
}

fn root_number_infix_inputs_supported(
    scalar_slots: &[ScalarStorageSlot],
    constants: &[PlanConstant],
    op: &PlanOp,
) -> bool {
    let PlanOpKind::UpdateBranch { ordered_inputs, .. } = &op.kind else {
        return false;
    };
    let [left, operator, right] = ordered_inputs.as_slice() else {
        return false;
    };
    let number_operand = |value: &ValueRef| match value {
        ValueRef::State(state) => {
            plan_value_type_for_state_slots(scalar_slots, *state) == Some(&PlanValueType::Number)
        }
        ValueRef::Constant(id) => constants.iter().any(|constant| {
            constant.id == *id && matches!(constant.value, PlanConstantValue::Number { .. })
        }),
        _ => false,
    };
    let operator_supported = match operator {
        ValueRef::Constant(id) => constants.iter().any(|constant| {
            constant.id == *id
                && matches!(
                    &constant.value,
                    PlanConstantValue::Text { value }
                        if matches!(value.as_str(), "+" | "-" | "*" | "/" | "%")
                )
        }),
        _ => false,
    };
    number_operand(left) && operator_supported && number_operand(right)
}

fn source_payload_inputs_are_empty_or_guard_only(
    op: &PlanOp,
    source_guard: &Option<PlanSourceGuard>,
) -> bool {
    let payload_inputs = source_payload_input_ids(op);
    if payload_inputs.is_empty() {
        return true;
    }
    let Some(PlanSourceGuard::SourcePayloadOneOf {
        source_id,
        field,
        values,
    }) = source_guard
    else {
        return false;
    };
    !values.is_empty()
        && matches!(
            payload_inputs.as_slice(),
            [(payload_source_id, payload_field)]
                if payload_source_id == source_id && payload_field == field
        )
        && op
            .inputs
            .iter()
            .any(|input| matches!(input, ValueRef::Source(id) if id == source_id))
}

fn indexed_bool_not_inputs_are_supported(scalar_slots: &[ScalarStorageSlot], op: &PlanOp) -> bool {
    if single_state_input_type_is(scalar_slots, op, &PlanValueType::Bool) {
        return true;
    }
    state_input_ids(op).is_empty()
        && op
            .inputs
            .iter()
            .filter(|input| matches!(input, ValueRef::Field(_)))
            .count()
            == 1
}

fn indexed_read_path_inputs_supported(scalar_slots: &[ScalarStorageSlot], op: &PlanOp) -> bool {
    let Some(ValueRef::State(output_state_id)) = op.output else {
        return false;
    };
    let Some(output_type) = output_state_type(scalar_slots, op) else {
        return false;
    };
    let inputs = op
        .inputs
        .iter()
        .filter_map(|input| match input {
            ValueRef::State(state_id) if *state_id != output_state_id => Some(*state_id),
            _ => None,
        })
        .collect::<Vec<_>>();
    let [input] = inputs.as_slice() else {
        return false;
    };
    source_payload_input_ids(op).is_empty()
        && plan_value_type_for_state_slots(scalar_slots, *input) == Some(output_type)
}

fn bytes_length_input_is_supported(scalar_slots: &[ScalarStorageSlot], op: &PlanOp) -> bool {
    let state_inputs = op
        .inputs
        .iter()
        .filter_map(|input| match input {
            ValueRef::State(state_id) => Some(*state_id),
            _ => None,
        })
        .collect::<Vec<_>>();
    let [state_id] = state_inputs.as_slice() else {
        return false;
    };
    matches!(
        plan_value_type_for_state_slots(scalar_slots, *state_id),
        Some(PlanValueType::Bytes { .. })
    )
}

fn bytes_equal_inputs_are_supported(scalar_slots: &[ScalarStorageSlot], op: &PlanOp) -> bool {
    let state_inputs = state_input_ids(op);
    state_inputs.len() == 2
        && state_inputs.iter().all(|state_id| {
            matches!(
                plan_value_type_for_state_slots(scalar_slots, *state_id),
                Some(PlanValueType::Bytes { .. })
            )
        })
}

fn indexed_bytes_equal_inputs_are_supported(
    scalar_slots: &[ScalarStorageSlot],
    list_slots: &[ListStorageSlot],
    op: &PlanOp,
) -> bool {
    let mut inputs = Vec::new();
    for input in &op.inputs {
        if matches!(input, ValueRef::State(_) | ValueRef::Field(_)) && !inputs.contains(input) {
            inputs.push(input.clone());
        }
    }
    let [left, right] = inputs.as_slice() else {
        return false;
    };
    left != right
        && indexed_bytes_operand_is_supported(scalar_slots, list_slots, op, left)
        && indexed_bytes_operand_is_supported(scalar_slots, list_slots, op, right)
}

fn indexed_bytes_ordered_inputs_are_supported(
    scalar_slots: &[ScalarStorageSlot],
    list_slots: &[ListStorageSlot],
    op: &PlanOp,
) -> bool {
    let ordered_inputs = update_branch_ordered_inputs(op);
    let [left, right] = ordered_inputs else {
        return false;
    };
    op.inputs.contains(left)
        && op.inputs.contains(right)
        && indexed_bytes_operand_is_supported(scalar_slots, list_slots, op, left)
        && indexed_bytes_operand_is_supported(scalar_slots, list_slots, op, right)
}

fn indexed_bytes_operand_is_supported(
    scalar_slots: &[ScalarStorageSlot],
    list_slots: &[ListStorageSlot],
    op: &PlanOp,
    input: &ValueRef,
) -> bool {
    if !op.inputs.contains(input) {
        return false;
    }
    match input {
        ValueRef::State(state_id) => matches!(
            plan_value_type_for_state_slots(scalar_slots, *state_id),
            Some(PlanValueType::Bytes { .. })
        ),
        ValueRef::Field(field_id) => {
            op.indexed
                && indexed_output_scope_owns_row_field_from_slots(
                    scalar_slots,
                    list_slots,
                    op,
                    *field_id,
                )
                && list_field_initial_value_matches_type(
                    list_slots,
                    *field_id,
                    &PlanValueType::Bytes { fixed_len: None },
                )
        }
        _ => false,
    }
}

fn indexed_text_operand_is_supported(
    scalar_slots: &[ScalarStorageSlot],
    list_slots: &[ListStorageSlot],
    op: &PlanOp,
    input: &ValueRef,
) -> bool {
    if !op.inputs.contains(input) {
        return false;
    }
    match input {
        ValueRef::State(state_id) => {
            plan_value_type_for_state_slots(scalar_slots, *state_id) == Some(&PlanValueType::Text)
        }
        ValueRef::Field(field_id) => {
            op.indexed
                && indexed_output_scope_owns_row_field_from_slots(
                    scalar_slots,
                    list_slots,
                    op,
                    *field_id,
                )
                && list_field_initial_value_matches_type(
                    list_slots,
                    *field_id,
                    &PlanValueType::Text,
                )
        }
        _ => false,
    }
}

fn indexed_single_text_input_is_supported(
    scalar_slots: &[ScalarStorageSlot],
    list_slots: &[ListStorageSlot],
    op: &PlanOp,
) -> bool {
    let ordered_inputs = update_branch_ordered_inputs(op);
    let [input] = ordered_inputs else {
        return false;
    };
    indexed_text_operand_is_supported(scalar_slots, list_slots, op, input)
}

fn indexed_text_to_bytes_inputs_are_supported(
    scalar_slots: &[ScalarStorageSlot],
    list_slots: &[ListStorageSlot],
    constants: &[PlanConstant],
    op: &PlanOp,
) -> bool {
    let ordered_inputs = update_branch_ordered_inputs(op);
    let [input, ValueRef::Constant(encoding_constant_id)] = ordered_inputs else {
        return false;
    };
    indexed_text_operand_is_supported(scalar_slots, list_slots, op, input)
        && encoding_constant_is_supported(constants, *encoding_constant_id)
}

fn bytes_concat_inputs_are_supported(scalar_slots: &[ScalarStorageSlot], op: &PlanOp) -> bool {
    let ordered_inputs = update_branch_ordered_inputs(op);
    matches!(
        ordered_inputs,
        [ValueRef::State(left), ValueRef::State(right)]
            if left != right
                && ordered_inputs.iter().all(|input| op.inputs.contains(input))
                && matches!(
                    plan_value_type_for_state_slots(scalar_slots, *left),
                    Some(PlanValueType::Bytes { .. })
                )
                && matches!(
                    plan_value_type_for_state_slots(scalar_slots, *right),
                    Some(PlanValueType::Bytes { .. })
                )
    )
}

fn bytes_search_inputs_are_supported(scalar_slots: &[ScalarStorageSlot], op: &PlanOp) -> bool {
    let ordered_inputs = update_branch_ordered_inputs(op);
    matches!(
        ordered_inputs,
        [ValueRef::State(left), ValueRef::State(right)]
            if op.inputs.contains(&ValueRef::State(*left))
                && op.inputs.contains(&ValueRef::State(*right))
                && matches!(
                    plan_value_type_for_state_slots(scalar_slots, *left),
                    Some(PlanValueType::Bytes { .. })
                )
                && matches!(
                    plan_value_type_for_state_slots(scalar_slots, *right),
                    Some(PlanValueType::Bytes { .. })
                )
    )
}

fn bytes_set_inputs_are_supported(
    scalar_slots: &[ScalarStorageSlot],
    constants: &[PlanConstant],
    op: &PlanOp,
) -> bool {
    let ordered_inputs = update_branch_ordered_inputs(op);
    matches!(
        ordered_inputs,
        [
            ValueRef::State(input),
            ValueRef::Constant(index_constant_id),
            ValueRef::Constant(value_constant_id)
        ] if op.inputs.contains(&ValueRef::State(*input))
            && matches!(
                plan_value_type_for_state_slots(scalar_slots, *input),
                Some(PlanValueType::Bytes { .. })
            )
            && plan_constant_by_id(constants, *index_constant_id).is_some_and(|constant| {
                let PlanConstantValue::Number { value } = constant.value else {
                    return false;
                };
                if value < 0 {
                    return false;
                }
                match plan_value_type_for_state_slots(scalar_slots, *input) {
                    Some(PlanValueType::Bytes {
                        fixed_len: Some(len),
                    }) => u64::try_from(value).is_ok_and(|index| index < *len),
                    Some(PlanValueType::Bytes { fixed_len: None }) => true,
                    _ => false,
                }
            })
            && plan_constant_by_id(constants, *value_constant_id).is_some_and(|constant| {
                matches!(constant.value, PlanConstantValue::Byte { .. })
            })
    )
}

fn plan_number_constant_u64(
    constants: &[PlanConstant],
    constant_id: PlanConstantId,
) -> Option<u64> {
    let constant = plan_constant_by_id(constants, constant_id)?;
    let PlanConstantValue::Number { value } = constant.value else {
        return None;
    };
    u64::try_from(value).ok()
}

fn plan_number_constant_i64(
    constants: &[PlanConstant],
    constant_id: PlanConstantId,
) -> Option<i64> {
    let constant = plan_constant_by_id(constants, constant_id)?;
    let PlanConstantValue::Number { value } = constant.value else {
        return None;
    };
    Some(value)
}

fn plan_text_constant_value(
    constants: &[PlanConstant],
    constant_id: PlanConstantId,
) -> Option<&str> {
    let constant = plan_constant_by_id(constants, constant_id)?;
    let PlanConstantValue::Text { value } = &constant.value else {
        return None;
    };
    Some(value)
}

fn state_bytes_fixed_len(
    scalar_slots: &[ScalarStorageSlot],
    state_id: StateId,
) -> Option<Option<u64>> {
    match plan_value_type_for_state_slots(scalar_slots, state_id)? {
        PlanValueType::Bytes { fixed_len } => Some(*fixed_len),
        _ => None,
    }
}

fn bytes_slice_inputs_are_supported(
    scalar_slots: &[ScalarStorageSlot],
    constants: &[PlanConstant],
    op: &PlanOp,
) -> bool {
    let ordered_inputs = update_branch_ordered_inputs(op);
    matches!(
        ordered_inputs,
        [
            ValueRef::State(input),
            offset_ref,
            byte_count_ref
        ] if op.inputs.contains(&ValueRef::State(*input))
            && bytes_number_operand_is_supported(scalar_slots, constants, op, offset_ref)
            && bytes_number_operand_is_supported(scalar_slots, constants, op, byte_count_ref)
            && state_bytes_fixed_len(scalar_slots, *input).is_some_and(|input_len| {
                match (
                    input_len,
                    bytes_number_operand_constant_u64(constants, offset_ref),
                    bytes_number_operand_constant_u64(constants, byte_count_ref),
                ) {
                    (Some(len), Some(offset), Some(byte_count)) => {
                        offset.checked_add(byte_count).is_some_and(|end| end <= len)
                    }
                    (Some(_), Some(_), None) => true,
                    (Some(_), None, _) => true,
                    (None, _, _) => true,
                }
            })
    )
}

fn bytes_number_operand_is_supported(
    scalar_slots: &[ScalarStorageSlot],
    constants: &[PlanConstant],
    op: &PlanOp,
    value_ref: &ValueRef,
) -> bool {
    match value_ref {
        ValueRef::Constant(constant_id) => {
            plan_number_constant_u64(constants, *constant_id).is_some()
        }
        ValueRef::State(state_id) => {
            op.inputs.contains(&ValueRef::State(*state_id))
                && plan_value_type_for_state_slots(scalar_slots, *state_id)
                    == Some(&PlanValueType::Number)
        }
        _ => false,
    }
}

fn bytes_number_operand_constant_u64(
    constants: &[PlanConstant],
    value_ref: &ValueRef,
) -> Option<u64> {
    match value_ref {
        ValueRef::Constant(constant_id) => plan_number_constant_u64(constants, *constant_id),
        _ => None,
    }
}

fn numeric_byte_count_is_valid(byte_count: u64) -> bool {
    matches!(byte_count, 1 | 2 | 4 | 8)
}

fn endian_constant_is_supported(constants: &[PlanConstant], constant_id: PlanConstantId) -> bool {
    matches!(
        plan_text_constant_value(constants, constant_id),
        Some("Little" | "Big")
    )
}

fn byte_range_is_supported(
    scalar_slots: &[ScalarStorageSlot],
    constants: &[PlanConstant],
    input: StateId,
    offset_constant_id: PlanConstantId,
    byte_count_constant_id: PlanConstantId,
) -> bool {
    let Some(offset) = plan_number_constant_u64(constants, offset_constant_id) else {
        return false;
    };
    let Some(byte_count) = plan_number_constant_u64(constants, byte_count_constant_id) else {
        return false;
    };
    if !numeric_byte_count_is_valid(byte_count) {
        return false;
    }
    state_bytes_fixed_len(scalar_slots, input).is_some_and(|input_len| match input_len {
        Some(len) => offset.checked_add(byte_count).is_some_and(|end| end <= len),
        None => true,
    })
}

fn bytes_numeric_read_inputs_are_supported(
    scalar_slots: &[ScalarStorageSlot],
    constants: &[PlanConstant],
    op: &PlanOp,
) -> bool {
    let ordered_inputs = update_branch_ordered_inputs(op);
    matches!(
        ordered_inputs,
        [
            ValueRef::State(input),
            ValueRef::Constant(offset_constant_id),
            ValueRef::Constant(byte_count_constant_id),
            ValueRef::Constant(endian_constant_id)
        ] if op.inputs.contains(&ValueRef::State(*input))
            && byte_range_is_supported(
                scalar_slots,
                constants,
                *input,
                *offset_constant_id,
                *byte_count_constant_id,
            )
            && endian_constant_is_supported(constants, *endian_constant_id)
    )
}

fn numeric_unsigned_value_is_supported(byte_count: u64, value: i64) -> bool {
    if value < 0 {
        return false;
    }
    if byte_count == 8 {
        return true;
    }
    let max = (1_i128 << (byte_count * 8)) - 1;
    i128::from(value) <= max
}

fn numeric_signed_value_is_supported(byte_count: u64, value: i64) -> bool {
    match byte_count {
        1 => (i8::MIN as i64..=i8::MAX as i64).contains(&value),
        2 => (i16::MIN as i64..=i16::MAX as i64).contains(&value),
        4 => (i32::MIN as i64..=i32::MAX as i64).contains(&value),
        8 => true,
        _ => false,
    }
}

fn bytes_numeric_write_inputs_are_supported(
    scalar_slots: &[ScalarStorageSlot],
    constants: &[PlanConstant],
    op: &PlanOp,
    signed: bool,
) -> bool {
    let ordered_inputs = update_branch_ordered_inputs(op);
    matches!(
        ordered_inputs,
        [
            ValueRef::State(input),
            ValueRef::Constant(offset_constant_id),
            ValueRef::Constant(byte_count_constant_id),
            ValueRef::Constant(endian_constant_id),
            ValueRef::Constant(value_constant_id)
        ] if op.inputs.contains(&ValueRef::State(*input))
            && byte_range_is_supported(
                scalar_slots,
                constants,
                *input,
                *offset_constant_id,
                *byte_count_constant_id,
            )
            && endian_constant_is_supported(constants, *endian_constant_id)
            && plan_number_constant_u64(constants, *byte_count_constant_id).is_some_and(|byte_count| {
                plan_number_constant_i64(constants, *value_constant_id).is_some_and(|value| {
                    if signed {
                        numeric_signed_value_is_supported(byte_count, value)
                    } else {
                        numeric_unsigned_value_is_supported(byte_count, value)
                    }
                })
            })
    )
}

fn bytes_take_inputs_are_supported(
    scalar_slots: &[ScalarStorageSlot],
    constants: &[PlanConstant],
    op: &PlanOp,
) -> bool {
    let ordered_inputs = update_branch_ordered_inputs(op);
    matches!(
        ordered_inputs,
        [ValueRef::State(input), byte_count_ref]
            if op.inputs.contains(&ValueRef::State(*input))
                && bytes_number_operand_is_supported(scalar_slots, constants, op, byte_count_ref)
                && state_bytes_fixed_len(scalar_slots, *input).is_some_and(|input_len| {
                    match (input_len, bytes_number_operand_constant_u64(constants, byte_count_ref)) {
                        (Some(len), Some(byte_count)) => byte_count <= len,
                        (Some(_), None) => true,
                        (None, _) => true,
                    }
                })
    )
}

fn bytes_drop_inputs_are_supported(
    scalar_slots: &[ScalarStorageSlot],
    constants: &[PlanConstant],
    op: &PlanOp,
) -> bool {
    bytes_take_inputs_are_supported(scalar_slots, constants, op)
}

fn bytes_zeros_inputs_are_supported(
    _scalar_slots: &[ScalarStorageSlot],
    constants: &[PlanConstant],
    op: &PlanOp,
) -> bool {
    let ordered_inputs = update_branch_ordered_inputs(op);
    matches!(
        ordered_inputs,
        [ValueRef::Constant(byte_count_constant_id)]
            if state_input_ids(op).is_empty()
                && plan_number_constant_u64(constants, *byte_count_constant_id).is_some()
    )
}

fn text_to_bytes_inputs_are_supported(
    scalar_slots: &[ScalarStorageSlot],
    constants: &[PlanConstant],
    op: &PlanOp,
) -> bool {
    let ordered_inputs = update_branch_ordered_inputs(op);
    matches!(
        ordered_inputs,
        [ValueRef::State(input), ValueRef::Constant(encoding_constant_id)]
            if op.inputs.contains(&ValueRef::State(*input))
                && matches!(
                    plan_value_type_for_state_slots(scalar_slots, *input),
                    Some(PlanValueType::Text)
                )
                && encoding_constant_is_supported(constants, *encoding_constant_id)
    )
}

fn bytes_to_text_inputs_are_supported(
    scalar_slots: &[ScalarStorageSlot],
    constants: &[PlanConstant],
    op: &PlanOp,
) -> bool {
    let ordered_inputs = update_branch_ordered_inputs(op);
    matches!(
        ordered_inputs,
        [ValueRef::State(input), ValueRef::Constant(encoding_constant_id)]
            if op.inputs.contains(&ValueRef::State(*input))
                && matches!(
                    plan_value_type_for_state_slots(scalar_slots, *input),
                    Some(PlanValueType::Bytes { .. })
                )
                && encoding_constant_is_supported(constants, *encoding_constant_id)
    )
}

fn file_read_bytes_inputs_are_supported(
    scalar_slots: &[ScalarStorageSlot],
    constants: &[PlanConstant],
    op: &PlanOp,
) -> bool {
    let ordered_inputs = update_branch_ordered_inputs(op);
    let [path_ref] = ordered_inputs else {
        return false;
    };
    match path_ref {
        ValueRef::Constant(path_constant_id) => {
            state_input_ids(op).is_empty()
                && plan_constant_by_id(constants, *path_constant_id).is_some_and(|constant| {
                    matches!(
                        &constant.value,
                        PlanConstantValue::Text { value } if file_read_bytes_path_is_static(value)
                    )
                })
        }
        ValueRef::State(path_state_id) => {
            state_input_ids(op).as_slice() == [*path_state_id]
                && matches!(
                    plan_value_type_for_state_slots(scalar_slots, *path_state_id),
                    Some(PlanValueType::Text)
                )
        }
        ValueRef::Field(_) => op.indexed,
        _ => false,
    }
}

fn file_write_bytes_inputs_are_supported(
    scalar_slots: &[ScalarStorageSlot],
    constants: &[PlanConstant],
    op: &PlanOp,
) -> bool {
    let ordered_inputs = update_branch_ordered_inputs(op);
    let [ValueRef::State(input), path_ref] = ordered_inputs else {
        return false;
    };
    if !op.inputs.contains(&ValueRef::State(*input))
        || !matches!(
            plan_value_type_for_state_slots(scalar_slots, *input),
            Some(PlanValueType::Bytes { .. })
        )
    {
        return false;
    }
    match path_ref {
        ValueRef::Constant(path_constant_id) => plan_constant_by_id(constants, *path_constant_id)
            .is_some_and(|constant| {
                matches!(
                    &constant.value,
                    PlanConstantValue::Text { value } if file_read_bytes_path_is_static(value)
                )
            }),
        ValueRef::State(path_state_id) => {
            op.inputs.contains(&ValueRef::State(*path_state_id))
                && matches!(
                    plan_value_type_for_state_slots(scalar_slots, *path_state_id),
                    Some(PlanValueType::Text)
                )
        }
        ValueRef::Field(_) => op.indexed,
        _ => false,
    }
}

fn file_read_bytes_path_is_static(value: &str) -> bool {
    let path = Path::new(value);
    !value.is_empty()
        && !path.is_absolute()
        && path
            .components()
            .all(|component| matches!(component, Component::CurDir | Component::Normal(_)))
}

fn encoding_constant_is_supported(constants: &[PlanConstant], constant_id: PlanConstantId) -> bool {
    plan_constant_by_id(constants, constant_id).is_some_and(|constant| {
        matches!(
            &constant.value,
            PlanConstantValue::Text { value } if canonical_encoding_is_supported(value)
        )
    })
}

fn canonical_encoding_is_supported(value: &str) -> bool {
    matches!(
        value
            .trim()
            .trim_matches('"')
            .replace(['-', '_'], "")
            .to_ascii_lowercase()
            .as_str(),
        "utf8" | "ascii"
    )
}

fn bytes_set_fixed_lengths_match(scalar_slots: &[ScalarStorageSlot], op: &PlanOp) -> bool {
    let output_type = output_state_type(scalar_slots, op);
    let ordered_inputs = update_branch_ordered_inputs(op);
    let [ValueRef::State(input), _, _] = ordered_inputs else {
        return false;
    };
    let Some(PlanValueType::Bytes {
        fixed_len: input_len,
    }) = plan_value_type_for_state_slots(scalar_slots, *input)
    else {
        return false;
    };
    let Some(PlanValueType::Bytes {
        fixed_len: output_len,
    }) = output_type
    else {
        return false;
    };
    match (input_len, output_len) {
        (Some(input), Some(output)) => input == output,
        _ => true,
    }
}

fn bytes_concat_fixed_lengths_match(scalar_slots: &[ScalarStorageSlot], op: &PlanOp) -> bool {
    let output_type = output_state_type(scalar_slots, op);
    let ordered_inputs = update_branch_ordered_inputs(op);
    let [ValueRef::State(left), ValueRef::State(right)] = ordered_inputs else {
        return false;
    };
    let Some(PlanValueType::Bytes {
        fixed_len: left_len,
    }) = plan_value_type_for_state_slots(scalar_slots, *left)
    else {
        return false;
    };
    let Some(PlanValueType::Bytes {
        fixed_len: right_len,
    }) = plan_value_type_for_state_slots(scalar_slots, *right)
    else {
        return false;
    };
    let Some(PlanValueType::Bytes {
        fixed_len: output_len,
    }) = output_type
    else {
        return false;
    };
    match (left_len, right_len, output_len) {
        (Some(left), Some(right), Some(output)) => {
            left.checked_add(*right).is_some_and(|sum| sum == *output)
        }
        _ => true,
    }
}

fn indexed_bytes_concat_fixed_lengths_match(
    scalar_slots: &[ScalarStorageSlot],
    list_slots: &[ListStorageSlot],
    op: &PlanOp,
) -> bool {
    let Some(PlanValueType::Bytes {
        fixed_len: output_len,
    }) = output_state_type(scalar_slots, op)
    else {
        return false;
    };
    let ordered_inputs = update_branch_ordered_inputs(op);
    let [left, right] = ordered_inputs else {
        return false;
    };
    match (
        indexed_bytes_operand_fixed_len(scalar_slots, list_slots, left),
        indexed_bytes_operand_fixed_len(scalar_slots, list_slots, right),
        output_len,
    ) {
        (Some(Some(left)), Some(Some(right)), Some(output)) => {
            left.checked_add(right).is_some_and(|sum| sum == *output)
        }
        (Some(_), Some(_), _) => true,
        _ => false,
    }
}

fn indexed_bytes_operand_fixed_len(
    scalar_slots: &[ScalarStorageSlot],
    list_slots: &[ListStorageSlot],
    input: &ValueRef,
) -> Option<Option<u64>> {
    match input {
        ValueRef::State(state_id) => match plan_value_type_for_state_slots(scalar_slots, *state_id)
        {
            Some(PlanValueType::Bytes { fixed_len }) => Some(*fixed_len),
            _ => None,
        },
        ValueRef::Field(field_id) => {
            let mut len = None;
            let mut saw_field = false;
            for field in list_slots
                .iter()
                .flat_map(|slot| &slot.initial_rows)
                .flat_map(|row| &row.fields)
                .filter(|field| field.field_id == Some(*field_id))
            {
                saw_field = true;
                let PlanConstantValue::Bytes { byte_len, .. } = &field.value else {
                    return None;
                };
                match len {
                    Some(existing) if existing != *byte_len => return Some(None),
                    Some(_) => {}
                    None => len = Some(*byte_len),
                }
            }
            if saw_field { Some(len) } else { None }
        }
        _ => None,
    }
}

fn output_bytes_fixed_len(scalar_slots: &[ScalarStorageSlot], op: &PlanOp) -> Option<Option<u64>> {
    let Some(PlanValueType::Bytes { fixed_len }) = output_state_type(scalar_slots, op) else {
        return None;
    };
    Some(*fixed_len)
}

fn bytes_slice_fixed_lengths_match(
    scalar_slots: &[ScalarStorageSlot],
    constants: &[PlanConstant],
    op: &PlanOp,
) -> bool {
    let ordered_inputs = update_branch_ordered_inputs(op);
    let [ValueRef::State(_input), _offset_ref, byte_count_ref] = ordered_inputs else {
        return false;
    };
    match output_bytes_fixed_len(scalar_slots, op) {
        Some(Some(output_len)) => {
            bytes_number_operand_constant_u64(constants, byte_count_ref) == Some(output_len)
        }
        Some(None) => true,
        None => false,
    }
}

fn bytes_take_fixed_lengths_match(
    scalar_slots: &[ScalarStorageSlot],
    constants: &[PlanConstant],
    op: &PlanOp,
) -> bool {
    let ordered_inputs = update_branch_ordered_inputs(op);
    let [ValueRef::State(_input), byte_count_ref] = ordered_inputs else {
        return false;
    };
    match output_bytes_fixed_len(scalar_slots, op) {
        Some(Some(output_len)) => {
            bytes_number_operand_constant_u64(constants, byte_count_ref) == Some(output_len)
        }
        Some(None) => true,
        None => false,
    }
}

fn bytes_drop_fixed_lengths_match(
    scalar_slots: &[ScalarStorageSlot],
    constants: &[PlanConstant],
    op: &PlanOp,
) -> bool {
    let ordered_inputs = update_branch_ordered_inputs(op);
    let [ValueRef::State(input), byte_count_ref] = ordered_inputs else {
        return false;
    };
    let Some(output_len) = output_bytes_fixed_len(scalar_slots, op) else {
        return false;
    };
    match (
        state_bytes_fixed_len(scalar_slots, *input),
        output_len,
        bytes_number_operand_constant_u64(constants, byte_count_ref),
    ) {
        (Some(Some(input_len)), Some(output_len), Some(byte_count)) => input_len
            .checked_sub(byte_count)
            .is_some_and(|expected| expected == output_len),
        (Some(Some(_)), Some(_), None) => false,
        (Some(Some(_)), None, _) => true,
        (Some(None), None, _) => true,
        (Some(None), Some(_), _) => false,
        _ => false,
    }
}

fn bytes_zeros_fixed_length_matches(
    scalar_slots: &[ScalarStorageSlot],
    constants: &[PlanConstant],
    op: &PlanOp,
) -> bool {
    let [ValueRef::Constant(byte_count_constant_id)] = update_branch_ordered_inputs(op) else {
        return false;
    };
    let Some(byte_count) = plan_number_constant_u64(constants, *byte_count_constant_id) else {
        return false;
    };
    match output_bytes_fixed_len(scalar_slots, op) {
        Some(Some(output_len)) => output_len == byte_count,
        Some(None) => true,
        None => false,
    }
}

fn bytes_numeric_write_fixed_lengths_match(
    scalar_slots: &[ScalarStorageSlot],
    op: &PlanOp,
) -> bool {
    let [
        ValueRef::State(input),
        ValueRef::Constant(_offset_constant_id),
        ValueRef::Constant(_byte_count_constant_id),
        ValueRef::Constant(_endian_constant_id),
        ValueRef::Constant(_value_constant_id),
    ] = update_branch_ordered_inputs(op)
    else {
        return false;
    };
    match (
        state_bytes_fixed_len(scalar_slots, *input),
        output_bytes_fixed_len(scalar_slots, op),
    ) {
        (Some(Some(input_len)), Some(Some(output_len))) => input_len == output_len,
        (Some(Some(_)), Some(None)) => true,
        (Some(None), Some(_)) => true,
        _ => false,
    }
}

fn output_state_is_bytes(scalar_slots: &[ScalarStorageSlot], op: &PlanOp) -> bool {
    matches!(
        output_state_type(scalar_slots, op),
        Some(PlanValueType::Bytes { .. })
    )
}

fn output_state_type<'a>(
    scalar_slots: &'a [ScalarStorageSlot],
    op: &PlanOp,
) -> Option<&'a PlanValueType> {
    let Some(ValueRef::State(state_id)) = op.output else {
        return None;
    };
    plan_value_type_for_state_slots(scalar_slots, state_id)
}

fn output_state_is_indexed(scalar_slots: &[ScalarStorageSlot], op: &PlanOp) -> bool {
    let Some(ValueRef::State(state_id)) = op.output else {
        return false;
    };
    scalar_slots
        .iter()
        .find(|slot| slot.state_id == state_id)
        .is_some_and(|slot| slot.indexed)
}

fn output_state_type_is(
    scalar_slots: &[ScalarStorageSlot],
    op: &PlanOp,
    expected: &PlanValueType,
) -> bool {
    output_state_type(scalar_slots, op) == Some(expected)
}

fn update_branch_source_ids(op: &PlanOp) -> Vec<SourceId> {
    let mut sources = Vec::new();
    for input in &op.inputs {
        if let ValueRef::Source(source_id) = input
            && !sources.contains(source_id)
        {
            sources.push(*source_id);
        }
    }
    sources
}

fn state_input_ids(op: &PlanOp) -> Vec<StateId> {
    let mut states = Vec::new();
    for input in &op.inputs {
        if let ValueRef::State(state_id) = input
            && !states.contains(state_id)
        {
            states.push(*state_id);
        }
    }
    states
}

fn source_payload_input_ids(op: &PlanOp) -> Vec<(SourceId, SourcePayloadField)> {
    let mut fields = Vec::new();
    for input in &op.inputs {
        if let ValueRef::SourcePayload { source_id, field } = input {
            let candidate = (*source_id, field.clone());
            if !fields.contains(&candidate) {
                fields.push(candidate);
            }
        }
    }
    fields
}

pub fn update_branch_ordered_inputs(op: &PlanOp) -> &[ValueRef] {
    match &op.kind {
        PlanOpKind::UpdateBranch { ordered_inputs, .. } => ordered_inputs,
        _ => &[],
    }
}

fn source_payload_input_matches_single_source(op: &PlanOp, expected: &SourcePayloadField) -> bool {
    let sources = update_branch_source_ids(op);
    let [source_id] = sources.as_slice() else {
        return false;
    };
    let payload_inputs = source_payload_input_ids(op);
    matches!(
        payload_inputs.as_slice(),
        [(payload_source_id, field)] if payload_source_id == source_id && field == expected
    )
}

fn source_payload_refs_are_declared_and_typed(
    plan: &MachinePlan,
    op: &PlanOp,
    expected: &SourcePayloadField,
) -> bool {
    let Some(output_type) = output_state_type(&plan.storage_layout.scalar_slots, op) else {
        return false;
    };
    let payload_inputs = source_payload_input_ids(op);
    if payload_inputs.is_empty() {
        return false;
    }
    for (source_id, field) in payload_inputs {
        if &field != expected {
            return false;
        }
        let Some(route) = plan
            .source_routes
            .iter()
            .find(|route| route.source_id == source_id)
        else {
            return false;
        };
        let Some(payload_type) = route
            .payload_schema
            .typed_fields
            .iter()
            .find(|descriptor| descriptor.field == field)
            .map(|descriptor| descriptor.value_type)
        else {
            return false;
        };
        if !source_payload_value_type_matches_plan_type(payload_type, output_type) {
            return false;
        }
    }
    true
}

fn source_payload_ref_mismatch_detail(
    plan: &MachinePlan,
    op: &PlanOp,
    expected: &SourcePayloadField,
) -> String {
    let Some(output_type) = output_state_type(&plan.storage_layout.scalar_slots, op) else {
        return "missing output state type".to_owned();
    };
    let payload_inputs = source_payload_input_ids(op);
    if payload_inputs.is_empty() {
        return "no source payload input".to_owned();
    }
    for (source_id, field) in payload_inputs {
        if &field != expected {
            return format!(
                "payload input field {:?} does not match {:?}",
                field, expected
            );
        }
        let Some(route) = plan
            .source_routes
            .iter()
            .find(|route| route.source_id == source_id)
        else {
            return format!("missing source route for source {}", source_id.0);
        };
        let Some(payload_type) = route
            .payload_schema
            .typed_fields
            .iter()
            .find(|descriptor| descriptor.field == field)
            .map(|descriptor| descriptor.value_type)
        else {
            return format!(
                "source route {} has no typed schema entry for {:?}",
                source_id.0, field
            );
        };
        if !source_payload_value_type_matches_plan_type(payload_type, output_type) {
            return format!(
                "source route {} field {:?} payload_type={:?} does not match output_type={:?}",
                source_id.0, field, payload_type, output_type
            );
        }
    }
    "unknown source-payload mismatch".to_owned()
}

fn source_payload_value_type_matches_plan_type(
    payload_type: SourcePayloadValueType,
    plan_type: &PlanValueType,
) -> bool {
    match payload_type {
        SourcePayloadValueType::Bytes => matches!(plan_type, PlanValueType::Bytes { .. }),
        SourcePayloadValueType::Bool => plan_type == &PlanValueType::Bool,
        SourcePayloadValueType::Text => plan_type == &PlanValueType::Text,
    }
}

fn source_payload_output_type_is_supported(
    scalar_slots: &[ScalarStorageSlot],
    op: &PlanOp,
    field: &SourcePayloadField,
) -> bool {
    let Some(output_type) = output_state_type(scalar_slots, op) else {
        return false;
    };
    match field {
        SourcePayloadField::Bytes => matches!(output_type, PlanValueType::Bytes { .. }),
        SourcePayloadField::Named(name) if name == "press" => {
            output_type == &PlanValueType::Bool || output_type == &PlanValueType::Text
        }
        SourcePayloadField::Address | SourcePayloadField::Key | SourcePayloadField::Text => {
            output_type == &PlanValueType::Text
        }
        SourcePayloadField::Named(_) => output_type == &PlanValueType::Text,
    }
}

fn single_state_input_type_is(
    scalar_slots: &[ScalarStorageSlot],
    op: &PlanOp,
    expected: &PlanValueType,
) -> bool {
    single_state_input_type_matches(scalar_slots, op, |value_type| value_type == expected)
}

fn single_state_input_type_matches(
    scalar_slots: &[ScalarStorageSlot],
    op: &PlanOp,
    matches_type: impl FnOnce(&PlanValueType) -> bool,
) -> bool {
    let state_inputs = state_input_ids(op);
    let [state_id] = state_inputs.as_slice() else {
        return false;
    };
    plan_value_type_for_state_slots(scalar_slots, *state_id).is_some_and(matches_type)
}

fn read_path_inputs_supported(scalar_slots: &[ScalarStorageSlot], op: &PlanOp) -> bool {
    if let Some(output_type) = output_state_type(scalar_slots, op) {
        if source_payload_input_ids(op).is_empty() {
            let state_inputs = state_input_ids(op);
            let field_inputs = op
                .inputs
                .iter()
                .filter(|input| matches!(input, ValueRef::Field(_)))
                .count();
            match (state_inputs.as_slice(), field_inputs) {
                ([state_id], 0) => {
                    plan_value_type_for_state_slots(scalar_slots, *state_id) == Some(output_type)
                }
                ([], 1) => !matches!(output_type, PlanValueType::Bytes { .. }),
                _ => false,
            }
        } else {
            source_payload_input_ids(op).len() == 1
                && state_input_ids(op).is_empty()
                && output_type == &PlanValueType::Text
        }
    } else {
        false
    }
}

fn previous_value_inputs_supported(scalar_slots: &[ScalarStorageSlot], op: &PlanOp) -> bool {
    let Some(output_type) = output_state_type(scalar_slots, op) else {
        return false;
    };
    let state_inputs = state_input_ids(op);
    let [state_id] = state_inputs.as_slice() else {
        return false;
    };
    plan_value_type_for_state_slots(scalar_slots, *state_id) == Some(output_type)
}

fn text_trim_or_previous_inputs_supported(
    scalar_slots: &[ScalarStorageSlot],
    constants: &[PlanConstant],
    op: &PlanOp,
    source_payload_field: &Option<SourcePayloadField>,
) -> bool {
    let PlanOpKind::UpdateBranch { ordered_inputs, .. } = &op.kind else {
        return false;
    };
    let [input, previous] = ordered_inputs.as_slice() else {
        return false;
    };
    let inputs_supported = text_operand_ref_supported(scalar_slots, constants, input, true)
        && text_operand_ref_supported(scalar_slots, constants, previous, false);
    if let Some(field) = source_payload_field {
        source_payload_input_matches_single_source(op, field) && inputs_supported
    } else {
        inputs_supported
    }
}

fn prefix_concat_inputs_supported(
    scalar_slots: &[ScalarStorageSlot],
    constants: &[PlanConstant],
    op: &PlanOp,
    allow_source_payload: bool,
) -> bool {
    let PlanOpKind::UpdateBranch { ordered_inputs, .. } = &op.kind else {
        return false;
    };
    let [prefix, input, separator] = ordered_inputs.as_slice() else {
        return false;
    };
    text_operand_ref_supported(scalar_slots, constants, prefix, false)
        && text_operand_ref_supported(scalar_slots, constants, input, allow_source_payload)
        && text_operand_ref_supported(scalar_slots, constants, separator, false)
}

fn indexed_match_text_is_empty_const_inputs_supported(
    scalar_slots: &[ScalarStorageSlot],
    constants: &[PlanConstant],
    op: &PlanOp,
) -> bool {
    let PlanOpKind::UpdateBranch { ordered_inputs, .. } = &op.kind else {
        return false;
    };
    let [input, first_arm, rest @ ..] = ordered_inputs.as_slice() else {
        return false;
    };
    if rest.len() > 1 {
        return false;
    }
    if !text_operand_ref_supported(scalar_slots, constants, input, true) {
        return false;
    }
    if !text_operand_ref_supported(scalar_slots, constants, first_arm, false) {
        return false;
    }
    rest.iter()
        .all(|operand| text_operand_ref_supported(scalar_slots, constants, operand, false))
}

fn root_match_const_inputs_supported(
    scalar_slots: &[ScalarStorageSlot],
    constants: &[PlanConstant],
    op: &PlanOp,
) -> bool {
    let Some(output_type) = output_state_type(scalar_slots, op) else {
        return false;
    };
    if matches!(
        output_type,
        PlanValueType::Bytes { .. }
            | PlanValueType::RootInitialField
            | PlanValueType::RowInitialField
            | PlanValueType::Unknown
    ) {
        return false;
    }
    let PlanOpKind::UpdateBranch { ordered_inputs, .. } = &op.kind else {
        return false;
    };
    let [input, arm_operands @ ..] = ordered_inputs.as_slice() else {
        return false;
    };
    if arm_operands.is_empty() || arm_operands.len() % 2 != 0 {
        return false;
    }
    if !match_const_input_ref_supported(scalar_slots, op, input) {
        return false;
    }
    arm_operands.chunks_exact(2).all(|pair| {
        match_const_pattern_ref_supported(constants, &pair[0])
            && match_const_output_ref_supported(constants, output_type, &pair[1])
    })
}

fn root_match_value_const_inputs_supported(
    scalar_slots: &[ScalarStorageSlot],
    constants: &[PlanConstant],
    op: &PlanOp,
) -> bool {
    let Some(output_type) = output_state_type(scalar_slots, op) else {
        return false;
    };
    if matches!(
        output_type,
        PlanValueType::Bytes { .. } | PlanValueType::Unknown
    ) {
        return false;
    }
    let PlanOpKind::UpdateBranch { ordered_inputs, .. } = &op.kind else {
        return false;
    };
    let [input, arm_operands @ ..] = ordered_inputs.as_slice() else {
        return false;
    };
    if arm_operands.is_empty() || arm_operands.len() % 2 != 0 {
        return false;
    }
    if !match_const_input_ref_supported(scalar_slots, op, input) {
        return false;
    }
    arm_operands.chunks_exact(2).all(|pair| {
        match_const_pattern_ref_supported(constants, &pair[0])
            && root_update_value_ref_supported(scalar_slots, constants, op, output_type, &pair[1])
    })
}

fn root_update_value_ref_supported(
    scalar_slots: &[ScalarStorageSlot],
    constants: &[PlanConstant],
    op: &PlanOp,
    output_type: &PlanValueType,
    value_ref: &ValueRef,
) -> bool {
    match value_ref {
        ValueRef::State(state_id) => {
            op.inputs.contains(value_ref)
                && plan_value_type_for_state_slots(scalar_slots, *state_id) == Some(output_type)
        }
        ValueRef::SourcePayload { field, .. } => {
            op.inputs.contains(value_ref)
                && match field {
                    SourcePayloadField::Bytes => false,
                    SourcePayloadField::Named(name) if name == "press" => {
                        output_type == &PlanValueType::Bool || output_type == &PlanValueType::Text
                    }
                    _ => output_type == &PlanValueType::Text,
                }
        }
        ValueRef::Constant(constant_id) => plan_constant_by_id(constants, *constant_id)
            .is_some_and(|constant| constant_value_matches_plan_type(&constant.value, output_type)),
        ValueRef::Field(_) => op.inputs.contains(value_ref),
        ValueRef::Source(_) | ValueRef::List(_) => false,
    }
}

fn match_const_input_ref_supported(
    scalar_slots: &[ScalarStorageSlot],
    op: &PlanOp,
    value_ref: &ValueRef,
) -> bool {
    match value_ref {
        ValueRef::State(state_id) => {
            op.inputs.contains(value_ref)
                && matches!(
                    plan_value_type_for_state_slots(scalar_slots, *state_id),
                    Some(PlanValueType::Text | PlanValueType::Enum)
                )
        }
        ValueRef::SourcePayload { field, .. } => {
            op.inputs.contains(value_ref) && *field != SourcePayloadField::Bytes
        }
        ValueRef::Field(_) => op.inputs.contains(value_ref),
        ValueRef::Constant(_) | ValueRef::Source(_) | ValueRef::List(_) => false,
    }
}

fn match_const_pattern_ref_supported(constants: &[PlanConstant], value_ref: &ValueRef) -> bool {
    let ValueRef::Constant(constant_id) = value_ref else {
        return false;
    };
    plan_constant_by_id(constants, *constant_id).is_some_and(|constant| {
        matches!(
            constant.value,
            PlanConstantValue::Text { .. } | PlanConstantValue::Enum { .. }
        )
    })
}

fn match_const_output_ref_supported(
    constants: &[PlanConstant],
    output_type: &PlanValueType,
    value_ref: &ValueRef,
) -> bool {
    let ValueRef::Constant(constant_id) = value_ref else {
        return false;
    };
    let Some(constant) = plan_constant_by_id(constants, *constant_id) else {
        return false;
    };
    if matches!(
        &constant.value,
        PlanConstantValue::Text { value } if value == "SKIP"
    ) {
        return true;
    }
    constant_value_matches_plan_type(&constant.value, output_type)
}

fn text_operand_ref_supported(
    scalar_slots: &[ScalarStorageSlot],
    constants: &[PlanConstant],
    value_ref: &ValueRef,
    allow_source_payload: bool,
) -> bool {
    match value_ref {
        ValueRef::State(state_id) => {
            plan_value_type_for_state_slots(scalar_slots, *state_id) == Some(&PlanValueType::Text)
        }
        ValueRef::Constant(constant_id) => plan_constant_by_id(constants, *constant_id)
            .is_some_and(|constant| matches!(constant.value, PlanConstantValue::Text { .. })),
        ValueRef::SourcePayload { .. } => allow_source_payload,
        ValueRef::Field(_) | ValueRef::Source(_) | ValueRef::List(_) => false,
    }
}

pub fn is_unknown_op(op: &PlanOp) -> bool {
    matches!(
        &op.kind,
        PlanOpKind::DerivedValue {
            derived_kind: PlanDerivedKind::Unknown,
            ..
        } | PlanOpKind::UpdateBranch {
            expression_kind: PlanExpressionKind::Unknown,
            ..
        } | PlanOpKind::ListProjection {
            projection: PlanListProjection::Unknown { .. },
        }
    )
}

fn unique_storage_ids(storage: &StorageLayout) -> bool {
    let mut ids = BTreeSet::new();
    storage
        .scalar_slots
        .iter()
        .map(|slot| slot.id)
        .chain(storage.list_slots.iter().map(|slot| slot.id))
        .chain(storage.byte_banks.iter().map(|bank| bank.id))
        .all(|id| ids.insert(id))
}

fn byte_bank_layout_mismatch_count(plan: &MachinePlan) -> usize {
    let fixed_bytes_slots = plan
        .storage_layout
        .scalar_slots
        .iter()
        .filter_map(|slot| match slot.value_type {
            PlanValueType::Bytes {
                fixed_len: Some(fixed_len),
            } => Some((slot, fixed_len)),
            _ => None,
        })
        .collect::<Vec<_>>();
    let missing_or_mismatched = fixed_bytes_slots
        .iter()
        .filter(|(slot, fixed_len)| {
            !plan.storage_layout.byte_banks.iter().any(|bank| {
                bank.state_storage_id == slot.id
                    && bank.state_id == slot.state_id
                    && bank.scope_id == slot.scope_id
                    && bank.indexed == slot.indexed
                    && bank.fixed_len == *fixed_len
            })
        })
        .count();
    let extra_or_mismatched = plan
        .storage_layout
        .byte_banks
        .iter()
        .filter(|bank| {
            !fixed_bytes_slots.iter().any(|(slot, fixed_len)| {
                bank.state_storage_id == slot.id
                    && bank.state_id == slot.state_id
                    && bank.scope_id == slot.scope_id
                    && bank.indexed == slot.indexed
                    && bank.fixed_len == *fixed_len
            })
        })
        .count();
    missing_or_mismatched + extra_or_mismatched
}

fn unique_operation_ids(regions: &[OperationRegion]) -> bool {
    let mut ids = BTreeSet::new();
    regions
        .iter()
        .flat_map(|region| region.ops.iter().map(|op| op.id))
        .all(|id| ids.insert(id))
}

fn byte_constants_match_hashes(constants: &[PlanConstant]) -> bool {
    constants.iter().all(|constant| match &constant.value {
        PlanConstantValue::Bytes {
            byte_len,
            sha256,
            inline_bytes: Some(bytes),
        } => {
            let mut hasher = Sha256::new();
            hasher.update(bytes);
            *byte_len == bytes.len() as u64 && *sha256 == format!("{:x}", hasher.finalize())
        }
        PlanConstantValue::Bytes {
            inline_bytes: None, ..
        } => true,
        PlanConstantValue::Text { .. }
        | PlanConstantValue::Number { .. }
        | PlanConstantValue::Byte { .. }
        | PlanConstantValue::Bool { .. }
        | PlanConstantValue::Enum { .. } => true,
    })
}

fn plan_constants_are_deduplicated(constants: &[PlanConstant]) -> bool {
    let mut values = Vec::<&PlanConstantValue>::new();
    for constant in constants {
        if values.iter().any(|value| **value == constant.value) {
            return false;
        }
        values.push(&constant.value);
    }
    true
}

fn constant_refs_resolve_and_match_storage_types_failure(plan: &MachinePlan) -> Option<String> {
    let mut ids = BTreeSet::new();
    for (index, constant) in plan.constants.iter().enumerate() {
        if constant.id.0 != index || !ids.insert(constant.id) {
            return Some(format!(
                "constant id order/uniqueness mismatch at index {index}: id={}",
                constant.id.0
            ));
        }
    }

    for slot in &plan.storage_layout.scalar_slots {
        let Some(constant_id) = slot.initial_constant_id else {
            continue;
        };
        let Some(constant) = plan_constant_by_id(&plan.constants, constant_id) else {
            return Some(format!(
                "scalar slot state {} references missing initial constant {}",
                slot.state_id.0, constant_id.0
            ));
        };
        if !constant_value_matches_plan_type(&constant.value, &slot.value_type) {
            return Some(format!(
                "scalar slot state {} initial constant {} type mismatch: constant={:?}, slot_type={:?}",
                slot.state_id.0, constant_id.0, constant.value, slot.value_type
            ));
        }
    }

    for op in plan.regions.iter().flat_map(|region| &region.ops) {
        match &op.kind {
            PlanOpKind::UpdateBranch {
                expression_kind,
                source_payload_field,
                update_constant_id,
                source_guard,
                ..
            } => match expression_kind {
                _ if !source_guard_refs_resolve(op, source_guard) => {
                    return Some(format!(
                        "update op {} has unresolved source guard {:?}",
                        op.id.0, source_guard
                    ));
                }
                PlanExpressionKind::Const => {
                    if source_payload_field.is_some() {
                        return Some(format!(
                            "const update op {} unexpectedly has source payload field {:?}",
                            op.id.0, source_payload_field
                        ));
                    }
                    let Some(constant_id) = update_constant_id else {
                        return Some(format!("const update op {} has no constant id", op.id.0));
                    };
                    let Some(constant) = plan_constant_by_id(&plan.constants, *constant_id) else {
                        return Some(format!(
                            "const update op {} references missing constant {}",
                            op.id.0, constant_id.0
                        ));
                    };
                    if let Some(ValueRef::State(state_id)) = op.output {
                        let Some(value_type) = plan_value_type_for_state(plan, state_id) else {
                            return Some(format!(
                                "const update op {} outputs missing state {}",
                                op.id.0, state_id.0
                            ));
                        };
                        if !constant_value_matches_plan_type(&constant.value, value_type) {
                            return Some(format!(
                                "const update op {} output state {} type mismatch: constant {}={:?}, state_type={:?}",
                                op.id.0, state_id.0, constant_id.0, constant.value, value_type
                            ));
                        }
                    }
                }
                PlanExpressionKind::SourcePayload => {
                    let Some(field) = source_payload_field.as_ref() else {
                        return Some(format!(
                            "source-payload update op {} has no payload field",
                            op.id.0
                        ));
                    };
                    if !source_payload_refs_are_declared_and_typed(plan, op, field) {
                        return Some(format!(
                            "source-payload update op {} has undeclared or type-mismatched field {:?}: {}",
                            op.id.0,
                            field,
                            source_payload_ref_mismatch_detail(plan, op, field)
                        ));
                    }
                    if update_constant_id.is_some() {
                        return Some(format!(
                            "source-payload update op {} unexpectedly has constant {:?}",
                            op.id.0, update_constant_id
                        ));
                    }
                    if let Some(ValueRef::State(state_id)) = op.output {
                        let Some(output_type) = plan_value_type_for_state(plan, state_id) else {
                            return Some(format!(
                                "source-payload update op {} outputs missing state {}",
                                op.id.0, state_id.0
                            ));
                        };
                        match field {
                            SourcePayloadField::Bytes
                                if !matches!(output_type, PlanValueType::Bytes { .. }) =>
                            {
                                return Some(format!(
                                    "source-payload update op {} bytes field outputs non-BYTES state {} type {:?}",
                                    op.id.0, state_id.0, output_type
                                ));
                            }
                            SourcePayloadField::Named(name) if name == "press" => {
                                if output_type != &PlanValueType::Bool
                                    && output_type != &PlanValueType::Text
                                {
                                    return Some(format!(
                                        "source-payload update op {} press field outputs unsupported state {} type {:?}",
                                        op.id.0, state_id.0, output_type
                                    ));
                                }
                            }
                            SourcePayloadField::Address
                            | SourcePayloadField::Key
                            | SourcePayloadField::Text
                                if output_type != &PlanValueType::Text =>
                            {
                                return Some(format!(
                                    "source-payload update op {} text field {:?} outputs non-TEXT state {} type {:?}",
                                    op.id.0, field, state_id.0, output_type
                                ));
                            }
                            SourcePayloadField::Named(_) if output_type != &PlanValueType::Text => {
                                return Some(format!(
                                    "source-payload update op {} named field {:?} outputs non-TEXT state {} type {:?}",
                                    op.id.0, field, state_id.0, output_type
                                ));
                            }
                            _ => {}
                        }
                    }
                }
                PlanExpressionKind::BytesGet => {
                    if source_payload_field.is_some() {
                        return Some(format!(
                            "bytes-get update op {} unexpectedly has source payload field {:?}",
                            op.id.0, source_payload_field
                        ));
                    }
                    let Some(constant_id) = update_constant_id else {
                        return Some(format!(
                            "bytes-get update op {} has no index constant",
                            op.id.0
                        ));
                    };
                    let Some(constant) = plan_constant_by_id(&plan.constants, *constant_id) else {
                        return Some(format!(
                            "bytes-get update op {} references missing index constant {}",
                            op.id.0, constant_id.0
                        ));
                    };
                    if !matches!(
                        constant.value,
                        PlanConstantValue::Number { value } if value >= 0
                    ) {
                        return Some(format!(
                            "bytes-get update op {} index constant {} is not a non-negative number: {:?}",
                            op.id.0, constant_id.0, constant.value
                        ));
                    }
                    if let Some(ValueRef::State(state_id)) = op.output {
                        let Some(value_type) = plan_value_type_for_state(plan, state_id) else {
                            return Some(format!(
                                "bytes-get update op {} outputs missing state {}",
                                op.id.0, state_id.0
                            ));
                        };
                        if value_type != &PlanValueType::Byte {
                            return Some(format!(
                                "bytes-get update op {} outputs non-BYTE state {} type {:?}",
                                op.id.0, state_id.0, value_type
                            ));
                        }
                    }
                }
                PlanExpressionKind::BytesIsEmpty => {
                    if source_payload_field.is_some() || update_constant_id.is_some() {
                        return Some(format!(
                            "bytes-is-empty update op {} unexpectedly has payload/constant",
                            op.id.0
                        ));
                    }
                    if let Some(ValueRef::State(state_id)) = op.output {
                        let Some(value_type) = plan_value_type_for_state(plan, state_id) else {
                            return Some(format!(
                                "bytes-is-empty update op {} outputs missing state {}",
                                op.id.0, state_id.0
                            ));
                        };
                        if value_type != &PlanValueType::Bool {
                            return Some(format!(
                                "bytes-is-empty update op {} outputs non-BOOL state {} type {:?}",
                                op.id.0, state_id.0, value_type
                            ));
                        }
                    }
                    if !bytes_length_input_is_supported(&plan.storage_layout.scalar_slots, op) {
                        return Some(format!(
                            "bytes-is-empty update op {} has unsupported BYTES input",
                            op.id.0
                        ));
                    }
                }
                PlanExpressionKind::BytesFind => {
                    if source_payload_field.is_some() || update_constant_id.is_some() {
                        return Some(format!(
                            "bytes-find update op {} unexpectedly has payload/constant",
                            op.id.0
                        ));
                    }
                    if let Some(ValueRef::State(state_id)) = op.output {
                        let Some(value_type) = plan_value_type_for_state(plan, state_id) else {
                            return Some(format!(
                                "bytes-find update op {} outputs missing state {}",
                                op.id.0, state_id.0
                            ));
                        };
                        if value_type != &PlanValueType::Number {
                            return Some(format!(
                                "bytes-find update op {} outputs non-NUMBER state {} type {:?}",
                                op.id.0, state_id.0, value_type
                            ));
                        }
                    }
                    if !indexed_bytes_ordered_inputs_are_supported(
                        &plan.storage_layout.scalar_slots,
                        &plan.storage_layout.list_slots,
                        op,
                    ) {
                        return Some(format!(
                            "bytes-find update op {} has unsupported ordered BYTES inputs",
                            op.id.0
                        ));
                    }
                }
                PlanExpressionKind::BytesStartsWith | PlanExpressionKind::BytesEndsWith => {
                    if source_payload_field.is_some() || update_constant_id.is_some() {
                        return Some(format!(
                            "{:?} update op {} unexpectedly has payload/constant",
                            expression_kind, op.id.0
                        ));
                    }
                    if let Some(ValueRef::State(state_id)) = op.output {
                        let Some(value_type) = plan_value_type_for_state(plan, state_id) else {
                            return Some(format!(
                                "{:?} update op {} outputs missing state {}",
                                expression_kind, op.id.0, state_id.0
                            ));
                        };
                        if value_type != &PlanValueType::Bool {
                            return Some(format!(
                                "{:?} update op {} outputs non-BOOL state {} type {:?}",
                                expression_kind, op.id.0, state_id.0, value_type
                            ));
                        }
                    }
                    if !indexed_bytes_ordered_inputs_are_supported(
                        &plan.storage_layout.scalar_slots,
                        &plan.storage_layout.list_slots,
                        op,
                    ) {
                        return Some(format!(
                            "{:?} update op {} has unsupported ordered BYTES inputs",
                            expression_kind, op.id.0
                        ));
                    }
                }
                PlanExpressionKind::BytesSet => {
                    if source_payload_field.is_some() || update_constant_id.is_some() {
                        return Some(constant_ref_update_op_failure(*expression_kind, op));
                    }
                    let ordered_inputs = update_branch_ordered_inputs(op);
                    let [
                        ValueRef::State(input),
                        ValueRef::Constant(index_constant_id),
                        ValueRef::Constant(value_constant_id),
                    ] = ordered_inputs
                    else {
                        return Some(constant_ref_update_op_failure(*expression_kind, op));
                    };
                    if !op.inputs.contains(&ValueRef::State(*input)) {
                        return Some(constant_ref_update_op_failure(*expression_kind, op));
                    }
                    let Some(input_type) = plan_value_type_for_state(plan, *input) else {
                        return Some(constant_ref_update_op_failure(*expression_kind, op));
                    };
                    if !matches!(input_type, PlanValueType::Bytes { .. }) {
                        return Some(constant_ref_update_op_failure(*expression_kind, op));
                    }
                    let Some(index_constant) =
                        plan_constant_by_id(&plan.constants, *index_constant_id)
                    else {
                        return Some(constant_ref_update_op_failure(*expression_kind, op));
                    };
                    if !matches!(
                        index_constant.value,
                        PlanConstantValue::Number { value } if value >= 0
                    ) {
                        return Some(constant_ref_update_op_failure(*expression_kind, op));
                    }
                    if let PlanConstantValue::Number { value } = &index_constant.value
                        && let PlanValueType::Bytes {
                            fixed_len: Some(len),
                        } = input_type
                        && !u64::try_from(*value).is_ok_and(|index| index < *len)
                    {
                        return Some(constant_ref_update_op_failure(*expression_kind, op));
                    }
                    let Some(value_constant) =
                        plan_constant_by_id(&plan.constants, *value_constant_id)
                    else {
                        return Some(constant_ref_update_op_failure(*expression_kind, op));
                    };
                    if !matches!(value_constant.value, PlanConstantValue::Byte { .. }) {
                        return Some(constant_ref_update_op_failure(*expression_kind, op));
                    }
                    if let Some(ValueRef::State(state_id)) = op.output {
                        let Some(value_type) = plan_value_type_for_state(plan, state_id) else {
                            return Some(constant_ref_update_op_failure(*expression_kind, op));
                        };
                        if !matches!(value_type, PlanValueType::Bytes { .. }) {
                            return Some(constant_ref_update_op_failure(*expression_kind, op));
                        }
                    }
                    if !bytes_set_fixed_lengths_match(&plan.storage_layout.scalar_slots, op) {
                        return Some(constant_ref_update_op_failure(*expression_kind, op));
                    }
                }
                PlanExpressionKind::BytesSlice => {
                    if source_payload_field.is_some() || update_constant_id.is_some() {
                        return Some(constant_ref_update_op_failure(*expression_kind, op));
                    }
                    let ordered_inputs = update_branch_ordered_inputs(op);
                    let [ValueRef::State(input), offset_ref, byte_count_ref] = ordered_inputs
                    else {
                        return Some(constant_ref_update_op_failure(*expression_kind, op));
                    };
                    if !op.inputs.contains(&ValueRef::State(*input)) {
                        return Some(constant_ref_update_op_failure(*expression_kind, op));
                    }
                    let Some(input_type) = plan_value_type_for_state(plan, *input) else {
                        return Some(constant_ref_update_op_failure(*expression_kind, op));
                    };
                    let PlanValueType::Bytes {
                        fixed_len: input_len,
                    } = input_type
                    else {
                        return Some(constant_ref_update_op_failure(*expression_kind, op));
                    };
                    if !bytes_number_operand_is_supported(
                        &plan.storage_layout.scalar_slots,
                        &plan.constants,
                        op,
                        offset_ref,
                    ) || !bytes_number_operand_is_supported(
                        &plan.storage_layout.scalar_slots,
                        &plan.constants,
                        op,
                        byte_count_ref,
                    ) {
                        return Some(constant_ref_update_op_failure(*expression_kind, op));
                    }
                    if let (Some(len), Some(offset), Some(byte_count)) = (
                        input_len,
                        bytes_number_operand_constant_u64(&plan.constants, offset_ref),
                        bytes_number_operand_constant_u64(&plan.constants, byte_count_ref),
                    ) && !offset
                        .checked_add(byte_count)
                        .is_some_and(|end| end <= *len)
                    {
                        return Some(constant_ref_update_op_failure(*expression_kind, op));
                    }
                    if let Some(ValueRef::State(state_id)) = op.output {
                        let Some(value_type) = plan_value_type_for_state(plan, state_id) else {
                            return Some(constant_ref_update_op_failure(*expression_kind, op));
                        };
                        if !matches!(value_type, PlanValueType::Bytes { .. }) {
                            return Some(constant_ref_update_op_failure(*expression_kind, op));
                        }
                    }
                    if !bytes_slice_fixed_lengths_match(
                        &plan.storage_layout.scalar_slots,
                        &plan.constants,
                        op,
                    ) {
                        return Some(constant_ref_update_op_failure(*expression_kind, op));
                    }
                }
                PlanExpressionKind::BytesTake => {
                    if source_payload_field.is_some() || update_constant_id.is_some() {
                        return Some(constant_ref_update_op_failure(*expression_kind, op));
                    }
                    let ordered_inputs = update_branch_ordered_inputs(op);
                    let [ValueRef::State(input), byte_count_ref] = ordered_inputs else {
                        return Some(constant_ref_update_op_failure(*expression_kind, op));
                    };
                    if !op.inputs.contains(&ValueRef::State(*input)) {
                        return Some(constant_ref_update_op_failure(*expression_kind, op));
                    }
                    let Some(input_type) = plan_value_type_for_state(plan, *input) else {
                        return Some(constant_ref_update_op_failure(*expression_kind, op));
                    };
                    let PlanValueType::Bytes {
                        fixed_len: input_len,
                    } = input_type
                    else {
                        return Some(constant_ref_update_op_failure(*expression_kind, op));
                    };
                    if !bytes_number_operand_is_supported(
                        &plan.storage_layout.scalar_slots,
                        &plan.constants,
                        op,
                        byte_count_ref,
                    ) {
                        return Some(constant_ref_update_op_failure(*expression_kind, op));
                    }
                    if let (Some(len), Some(byte_count)) = (
                        input_len,
                        bytes_number_operand_constant_u64(&plan.constants, byte_count_ref),
                    ) && byte_count > *len
                    {
                        return Some(constant_ref_update_op_failure(*expression_kind, op));
                    }
                    if let Some(ValueRef::State(state_id)) = op.output {
                        let Some(value_type) = plan_value_type_for_state(plan, state_id) else {
                            return Some(constant_ref_update_op_failure(*expression_kind, op));
                        };
                        if !matches!(value_type, PlanValueType::Bytes { .. }) {
                            return Some(constant_ref_update_op_failure(*expression_kind, op));
                        }
                    }
                    if !bytes_take_fixed_lengths_match(
                        &plan.storage_layout.scalar_slots,
                        &plan.constants,
                        op,
                    ) {
                        return Some(constant_ref_update_op_failure(*expression_kind, op));
                    }
                }
                PlanExpressionKind::BytesDrop => {
                    if source_payload_field.is_some() || update_constant_id.is_some() {
                        return Some(constant_ref_update_op_failure(*expression_kind, op));
                    }
                    let ordered_inputs = update_branch_ordered_inputs(op);
                    let [ValueRef::State(input), byte_count_ref] = ordered_inputs else {
                        return Some(constant_ref_update_op_failure(*expression_kind, op));
                    };
                    if !op.inputs.contains(&ValueRef::State(*input)) {
                        return Some(constant_ref_update_op_failure(*expression_kind, op));
                    }
                    let Some(input_type) = plan_value_type_for_state(plan, *input) else {
                        return Some(constant_ref_update_op_failure(*expression_kind, op));
                    };
                    let PlanValueType::Bytes {
                        fixed_len: input_len,
                    } = input_type
                    else {
                        return Some(constant_ref_update_op_failure(*expression_kind, op));
                    };
                    if !bytes_number_operand_is_supported(
                        &plan.storage_layout.scalar_slots,
                        &plan.constants,
                        op,
                        byte_count_ref,
                    ) {
                        return Some(constant_ref_update_op_failure(*expression_kind, op));
                    }
                    if let (Some(len), Some(byte_count)) = (
                        input_len,
                        bytes_number_operand_constant_u64(&plan.constants, byte_count_ref),
                    ) && byte_count > *len
                    {
                        return Some(constant_ref_update_op_failure(*expression_kind, op));
                    }
                    if let Some(ValueRef::State(state_id)) = op.output {
                        let Some(value_type) = plan_value_type_for_state(plan, state_id) else {
                            return Some(constant_ref_update_op_failure(*expression_kind, op));
                        };
                        if !matches!(value_type, PlanValueType::Bytes { .. }) {
                            return Some(constant_ref_update_op_failure(*expression_kind, op));
                        }
                    }
                    if !bytes_drop_fixed_lengths_match(
                        &plan.storage_layout.scalar_slots,
                        &plan.constants,
                        op,
                    ) {
                        return Some(constant_ref_update_op_failure(*expression_kind, op));
                    }
                }
                PlanExpressionKind::BytesZeros => {
                    if source_payload_field.is_some() || update_constant_id.is_some() {
                        return Some(constant_ref_update_op_failure(*expression_kind, op));
                    }
                    let ordered_inputs = update_branch_ordered_inputs(op);
                    let [ValueRef::Constant(byte_count_constant_id)] = ordered_inputs else {
                        return Some(constant_ref_update_op_failure(*expression_kind, op));
                    };
                    if !state_input_ids(op).is_empty() {
                        return Some(constant_ref_update_op_failure(*expression_kind, op));
                    }
                    if plan_number_constant_u64(&plan.constants, *byte_count_constant_id).is_none()
                    {
                        return Some(constant_ref_update_op_failure(*expression_kind, op));
                    }
                    if let Some(ValueRef::State(state_id)) = op.output {
                        let Some(value_type) = plan_value_type_for_state(plan, state_id) else {
                            return Some(constant_ref_update_op_failure(*expression_kind, op));
                        };
                        if !matches!(value_type, PlanValueType::Bytes { .. }) {
                            return Some(constant_ref_update_op_failure(*expression_kind, op));
                        }
                    }
                    if !bytes_zeros_fixed_length_matches(
                        &plan.storage_layout.scalar_slots,
                        &plan.constants,
                        op,
                    ) {
                        return Some(constant_ref_update_op_failure(*expression_kind, op));
                    }
                }
                PlanExpressionKind::BytesToHex | PlanExpressionKind::BytesToBase64 => {
                    if source_payload_field.is_some() || update_constant_id.is_some() {
                        return Some(constant_ref_update_op_failure(*expression_kind, op));
                    }
                    let ordered_inputs = update_branch_ordered_inputs(op);
                    let [ValueRef::State(input)] = ordered_inputs else {
                        return Some(constant_ref_update_op_failure(*expression_kind, op));
                    };
                    if !op.inputs.contains(&ValueRef::State(*input)) {
                        return Some(constant_ref_update_op_failure(*expression_kind, op));
                    }
                    let Some(input_type) = plan_value_type_for_state(plan, *input) else {
                        return Some(constant_ref_update_op_failure(*expression_kind, op));
                    };
                    if !matches!(input_type, PlanValueType::Bytes { .. }) {
                        return Some(constant_ref_update_op_failure(*expression_kind, op));
                    }
                    if let Some(ValueRef::State(state_id)) = op.output {
                        let Some(value_type) = plan_value_type_for_state(plan, state_id) else {
                            return Some(constant_ref_update_op_failure(*expression_kind, op));
                        };
                        if value_type != &PlanValueType::Text {
                            return Some(constant_ref_update_op_failure(*expression_kind, op));
                        }
                    }
                }
                PlanExpressionKind::BytesFromHex | PlanExpressionKind::BytesFromBase64 => {
                    if source_payload_field.is_some() || update_constant_id.is_some() {
                        return Some(constant_ref_update_op_failure(*expression_kind, op));
                    }
                    let input_supported = if op.indexed {
                        indexed_single_text_input_is_supported(
                            &plan.storage_layout.scalar_slots,
                            &plan.storage_layout.list_slots,
                            op,
                        )
                    } else {
                        let ordered_inputs = update_branch_ordered_inputs(op);
                        let [ValueRef::State(input)] = ordered_inputs else {
                            return Some(constant_ref_update_op_failure(*expression_kind, op));
                        };
                        if !op.inputs.contains(&ValueRef::State(*input)) {
                            return Some(constant_ref_update_op_failure(*expression_kind, op));
                        }
                        plan_value_type_for_state(plan, *input) == Some(&PlanValueType::Text)
                    };
                    if !input_supported {
                        return Some(constant_ref_update_op_failure(*expression_kind, op));
                    }
                    if let Some(ValueRef::State(state_id)) = op.output {
                        let Some(value_type) = plan_value_type_for_state(plan, state_id) else {
                            return Some(constant_ref_update_op_failure(*expression_kind, op));
                        };
                        if !matches!(value_type, PlanValueType::Bytes { .. }) {
                            return Some(constant_ref_update_op_failure(*expression_kind, op));
                        }
                    }
                }
                PlanExpressionKind::BytesReadUnsigned | PlanExpressionKind::BytesReadSigned => {
                    if source_payload_field.is_some() || update_constant_id.is_some() {
                        return Some(constant_ref_update_op_failure(*expression_kind, op));
                    }
                    let ordered_inputs = update_branch_ordered_inputs(op);
                    let [
                        ValueRef::State(input),
                        ValueRef::Constant(offset_constant_id),
                        ValueRef::Constant(byte_count_constant_id),
                        ValueRef::Constant(endian_constant_id),
                    ] = ordered_inputs
                    else {
                        return Some(constant_ref_update_op_failure(*expression_kind, op));
                    };
                    if !op.inputs.contains(&ValueRef::State(*input)) {
                        return Some(constant_ref_update_op_failure(*expression_kind, op));
                    }
                    if !byte_range_is_supported(
                        &plan.storage_layout.scalar_slots,
                        &plan.constants,
                        *input,
                        *offset_constant_id,
                        *byte_count_constant_id,
                    ) {
                        return Some(constant_ref_update_op_failure(*expression_kind, op));
                    }
                    if !endian_constant_is_supported(&plan.constants, *endian_constant_id) {
                        return Some(constant_ref_update_op_failure(*expression_kind, op));
                    }
                    if let Some(ValueRef::State(state_id)) = op.output {
                        let Some(value_type) = plan_value_type_for_state(plan, state_id) else {
                            return Some(constant_ref_update_op_failure(*expression_kind, op));
                        };
                        if value_type != &PlanValueType::Number {
                            return Some(constant_ref_update_op_failure(*expression_kind, op));
                        }
                    }
                }
                PlanExpressionKind::BytesWriteUnsigned | PlanExpressionKind::BytesWriteSigned => {
                    if source_payload_field.is_some() || update_constant_id.is_some() {
                        return Some(constant_ref_update_op_failure(*expression_kind, op));
                    }
                    let ordered_inputs = update_branch_ordered_inputs(op);
                    let [
                        ValueRef::State(input),
                        ValueRef::Constant(offset_constant_id),
                        ValueRef::Constant(byte_count_constant_id),
                        ValueRef::Constant(endian_constant_id),
                        ValueRef::Constant(value_constant_id),
                    ] = ordered_inputs
                    else {
                        return Some(constant_ref_update_op_failure(*expression_kind, op));
                    };
                    if !op.inputs.contains(&ValueRef::State(*input)) {
                        return Some(constant_ref_update_op_failure(*expression_kind, op));
                    }
                    if !byte_range_is_supported(
                        &plan.storage_layout.scalar_slots,
                        &plan.constants,
                        *input,
                        *offset_constant_id,
                        *byte_count_constant_id,
                    ) {
                        return Some(constant_ref_update_op_failure(*expression_kind, op));
                    }
                    if !endian_constant_is_supported(&plan.constants, *endian_constant_id) {
                        return Some(constant_ref_update_op_failure(*expression_kind, op));
                    }
                    let Some(byte_count) =
                        plan_number_constant_u64(&plan.constants, *byte_count_constant_id)
                    else {
                        return Some(constant_ref_update_op_failure(*expression_kind, op));
                    };
                    let Some(value) = plan_number_constant_i64(&plan.constants, *value_constant_id)
                    else {
                        return Some(constant_ref_update_op_failure(*expression_kind, op));
                    };
                    let value_is_supported = match expression_kind {
                        PlanExpressionKind::BytesWriteUnsigned => {
                            numeric_unsigned_value_is_supported(byte_count, value)
                        }
                        PlanExpressionKind::BytesWriteSigned => {
                            numeric_signed_value_is_supported(byte_count, value)
                        }
                        _ => false,
                    };
                    if !value_is_supported {
                        return Some(constant_ref_update_op_failure(*expression_kind, op));
                    }
                    if let Some(ValueRef::State(state_id)) = op.output {
                        let Some(value_type) = plan_value_type_for_state(plan, state_id) else {
                            return Some(constant_ref_update_op_failure(*expression_kind, op));
                        };
                        if !matches!(value_type, PlanValueType::Bytes { .. }) {
                            return Some(constant_ref_update_op_failure(*expression_kind, op));
                        }
                    }
                    if !bytes_numeric_write_fixed_lengths_match(
                        &plan.storage_layout.scalar_slots,
                        op,
                    ) {
                        return Some(constant_ref_update_op_failure(*expression_kind, op));
                    }
                }
                PlanExpressionKind::TextToBytes => {
                    if source_payload_field.is_some() || update_constant_id.is_some() {
                        return Some(constant_ref_update_op_failure(*expression_kind, op));
                    }
                    let input_supported = if op.indexed {
                        indexed_text_to_bytes_inputs_are_supported(
                            &plan.storage_layout.scalar_slots,
                            &plan.storage_layout.list_slots,
                            &plan.constants,
                            op,
                        )
                    } else {
                        text_to_bytes_inputs_are_supported(
                            &plan.storage_layout.scalar_slots,
                            &plan.constants,
                            op,
                        )
                    };
                    if !input_supported {
                        return Some(constant_ref_update_op_failure(*expression_kind, op));
                    }
                    if let Some(ValueRef::State(state_id)) = op.output {
                        let Some(value_type) = plan_value_type_for_state(plan, state_id) else {
                            return Some(constant_ref_update_op_failure(*expression_kind, op));
                        };
                        if !matches!(value_type, PlanValueType::Bytes { .. }) {
                            return Some(constant_ref_update_op_failure(*expression_kind, op));
                        }
                    }
                }
                PlanExpressionKind::BytesToText => {
                    if source_payload_field.is_some() || update_constant_id.is_some() {
                        return Some(constant_ref_update_op_failure(*expression_kind, op));
                    }
                    let ordered_inputs = update_branch_ordered_inputs(op);
                    let [
                        ValueRef::State(input),
                        ValueRef::Constant(encoding_constant_id),
                    ] = ordered_inputs
                    else {
                        return Some(constant_ref_update_op_failure(*expression_kind, op));
                    };
                    if !op.inputs.contains(&ValueRef::State(*input)) {
                        return Some(constant_ref_update_op_failure(*expression_kind, op));
                    }
                    let Some(input_type) = plan_value_type_for_state(plan, *input) else {
                        return Some(constant_ref_update_op_failure(*expression_kind, op));
                    };
                    if !matches!(input_type, PlanValueType::Bytes { .. }) {
                        return Some(constant_ref_update_op_failure(*expression_kind, op));
                    }
                    if !encoding_constant_is_supported(&plan.constants, *encoding_constant_id) {
                        return Some(constant_ref_update_op_failure(*expression_kind, op));
                    }
                    if let Some(ValueRef::State(state_id)) = op.output {
                        let Some(value_type) = plan_value_type_for_state(plan, state_id) else {
                            return Some(constant_ref_update_op_failure(*expression_kind, op));
                        };
                        if value_type != &PlanValueType::Text {
                            return Some(constant_ref_update_op_failure(*expression_kind, op));
                        }
                    }
                }
                PlanExpressionKind::BytesEqual => {
                    if source_payload_field.is_some() || update_constant_id.is_some() {
                        return Some(constant_ref_update_op_failure(*expression_kind, op));
                    }
                    if let Some(ValueRef::State(state_id)) = op.output {
                        let Some(value_type) = plan_value_type_for_state(plan, state_id) else {
                            return Some(constant_ref_update_op_failure(*expression_kind, op));
                        };
                        if value_type != &PlanValueType::Bool {
                            return Some(constant_ref_update_op_failure(*expression_kind, op));
                        }
                    }
                }
                PlanExpressionKind::BytesConcat => {
                    if source_payload_field.is_some() || update_constant_id.is_some() {
                        return Some(constant_ref_update_op_failure(*expression_kind, op));
                    }
                    if let Some(ValueRef::State(state_id)) = op.output {
                        let Some(value_type) = plan_value_type_for_state(plan, state_id) else {
                            return Some(constant_ref_update_op_failure(*expression_kind, op));
                        };
                        if !matches!(value_type, PlanValueType::Bytes { .. }) {
                            return Some(constant_ref_update_op_failure(*expression_kind, op));
                        }
                    }
                    if !indexed_bytes_ordered_inputs_are_supported(
                        &plan.storage_layout.scalar_slots,
                        &plan.storage_layout.list_slots,
                        op,
                    ) {
                        return Some(constant_ref_update_op_failure(*expression_kind, op));
                    }
                    if !indexed_bytes_concat_fixed_lengths_match(
                        &plan.storage_layout.scalar_slots,
                        &plan.storage_layout.list_slots,
                        op,
                    ) {
                        return Some(constant_ref_update_op_failure(*expression_kind, op));
                    }
                }
                PlanExpressionKind::MatchConst if !op.indexed => {
                    if source_payload_field.is_some()
                        || update_constant_id.is_some()
                        || !root_match_const_inputs_supported(
                            &plan.storage_layout.scalar_slots,
                            &plan.constants,
                            op,
                        )
                    {
                        return Some(constant_ref_update_op_failure(*expression_kind, op));
                    }
                }
                PlanExpressionKind::FileReadBytes => {
                    if source_payload_field.is_some()
                        || update_constant_id.is_some()
                        || !file_read_bytes_inputs_are_supported(
                            &plan.storage_layout.scalar_slots,
                            &plan.constants,
                            op,
                        )
                    {
                        return Some(constant_ref_update_op_failure(*expression_kind, op));
                    }
                    if let Some(ValueRef::State(state_id)) = op.output {
                        let Some(value_type) = plan_value_type_for_state(plan, state_id) else {
                            return Some(constant_ref_update_op_failure(*expression_kind, op));
                        };
                        if !matches!(value_type, PlanValueType::Bytes { .. }) {
                            return Some(constant_ref_update_op_failure(*expression_kind, op));
                        }
                    }
                }
                PlanExpressionKind::FileWriteBytes => {
                    if !file_write_bytes_op_is_well_formed(plan, op) {
                        return Some(constant_ref_update_op_failure(*expression_kind, op));
                    }
                }
                _ => {}
            },
            _ => {}
        }
    }
    None
}

fn constant_ref_update_op_failure(kind: PlanExpressionKind, op: &PlanOp) -> String {
    format!(
        "{kind:?} update op {} failed typed constant/ref validation",
        op.id.0
    )
}

fn initial_field_paths_resolve(plan: &MachinePlan) -> bool {
    plan.storage_layout
        .scalar_slots
        .iter()
        .all(|slot| match slot.initial_value_kind {
            InitialValueKind::RootInitialField => {
                slot.initial_root_field_path
                    .as_deref()
                    .is_some_and(|path| !path.trim().is_empty())
                    && slot.initial_row_field_path.is_none()
            }
            InitialValueKind::RowInitialField => {
                slot.initial_row_field_path
                    .as_deref()
                    .is_some_and(|path| !path.trim().is_empty())
                    && slot.initial_root_field_path.is_none()
            }
            _ => slot.initial_root_field_path.is_none() && slot.initial_row_field_path.is_none(),
        })
}

fn list_append_refs_resolve(plan: &MachinePlan) -> bool {
    plan.regions
        .iter()
        .flat_map(|region| &region.ops)
        .all(|op| {
            let PlanOpKind::ListOperation {
                operation_kind: PlanListOperationKind::Append,
                append,
                ..
            } = &op.kind
            else {
                return true;
            };
            let Some(append) = append else {
                return false;
            };
            if !op.inputs.contains(&append.trigger) {
                return false;
            }
            append
                .fields
                .iter()
                .all(|field| match (&field.value_ref, field.constant_id) {
                    (Some(value_ref), None) => {
                        field.field_id.is_some() && op.inputs.contains(value_ref)
                    }
                    (None, Some(constant_id)) => {
                        field.field_id.is_some()
                            && plan_constant_by_id(&plan.constants, constant_id).is_some()
                    }
                    _ => false,
                })
        })
}

fn list_remove_refs_resolve(plan: &MachinePlan) -> bool {
    plan.regions
        .iter()
        .flat_map(|region| &region.ops)
        .all(|op| {
            let PlanOpKind::ListOperation {
                operation_kind: PlanListOperationKind::Remove,
                remove,
                ..
            } = &op.kind
            else {
                return true;
            };
            let Some(remove) = remove else {
                return false;
            };
            if !op.inputs.contains(&remove.source) {
                return false;
            }
            match &remove.predicate {
                PlanListRemovePredicate::AlwaysTrue => true,
                PlanListRemovePredicate::RowFieldBool { input }
                | PlanListRemovePredicate::RowFieldBoolNot { input } => op.inputs.contains(input),
                PlanListRemovePredicate::SelectedFilterVisibility {
                    selector,
                    row_field,
                } => op.inputs.contains(selector) && op.inputs.contains(row_field),
                PlanListRemovePredicate::Unknown { summary } => !summary.trim().is_empty(),
            }
        })
}

fn list_retain_refs_resolve(plan: &MachinePlan) -> bool {
    plan.regions
        .iter()
        .flat_map(|region| &region.ops)
        .all(|op| {
            let PlanOpKind::ListOperation {
                operation_kind: PlanListOperationKind::Retain,
                retain,
                ..
            } = &op.kind
            else {
                return true;
            };
            let Some(retain) = retain else {
                return false;
            };
            if !op.inputs.contains(&retain.target) {
                return false;
            }
            match &retain.predicate {
                PlanListRemovePredicate::AlwaysTrue => true,
                PlanListRemovePredicate::RowFieldBool { input }
                | PlanListRemovePredicate::RowFieldBoolNot { input } => op.inputs.contains(input),
                PlanListRemovePredicate::SelectedFilterVisibility {
                    selector,
                    row_field,
                } => op.inputs.contains(selector) && op.inputs.contains(row_field),
                PlanListRemovePredicate::Unknown { summary } => !summary.trim().is_empty(),
            }
        })
}

fn list_count_refs_resolve(plan: &MachinePlan) -> bool {
    plan.regions
        .iter()
        .flat_map(|region| &region.ops)
        .all(|op| {
            let PlanOpKind::ListOperation {
                operation_kind: PlanListOperationKind::Count,
                count,
                ..
            } = &op.kind
            else {
                return true;
            };
            let Some(count) = count else {
                return false;
            };
            if !op.inputs.contains(&count.target) {
                return false;
            }
            match &count.predicate {
                PlanListRemovePredicate::AlwaysTrue => true,
                PlanListRemovePredicate::RowFieldBool { input }
                | PlanListRemovePredicate::RowFieldBoolNot { input } => op.inputs.contains(input),
                PlanListRemovePredicate::SelectedFilterVisibility {
                    selector,
                    row_field,
                } => op.inputs.contains(selector) && op.inputs.contains(row_field),
                PlanListRemovePredicate::Unknown { summary } => !summary.trim().is_empty(),
            }
        })
}

fn list_projection_refs_resolve(plan: &MachinePlan) -> bool {
    plan.regions
        .iter()
        .flat_map(|region| &region.ops)
        .all(|op| {
            let PlanOpKind::ListProjection { projection } = &op.kind else {
                return true;
            };
            if !matches!(op.output, Some(ValueRef::Field(_))) {
                return false;
            }
            match projection {
                PlanListProjection::Find {
                    source_list,
                    field,
                    value,
                } => {
                    !field.trim().is_empty()
                        && op.inputs.contains(&ValueRef::List(*source_list))
                        && op.inputs.contains(value)
                }
                PlanListProjection::Chunk {
                    source_list,
                    size,
                    item_field,
                    label_field,
                } => {
                    *size > 0
                        && !item_field.trim().is_empty()
                        && !label_field.trim().is_empty()
                        && op.inputs.contains(&ValueRef::List(*source_list))
                }
                PlanListProjection::ChunkValue {
                    source,
                    size,
                    item_field,
                    label_field,
                } => {
                    *size > 0
                        && !item_field.trim().is_empty()
                        && !label_field.trim().is_empty()
                        && op.inputs.contains(source)
                }
                PlanListProjection::Unknown { summary } => !summary.trim().is_empty(),
            }
        })
}

fn list_initial_row_fields_resolve(plan: &MachinePlan) -> bool {
    plan.storage_layout
        .list_slots
        .iter()
        .flat_map(|slot| &slot.initial_rows)
        .flat_map(|row| &row.fields)
        .all(|field| field.field_id.is_some())
}

fn list_range_bounds_resolve(plan: &MachinePlan) -> bool {
    plan.storage_layout
        .list_slots
        .iter()
        .all(|slot| match slot.initializer_kind {
            ListInitializerKind::Range => slot.range.is_some_and(|range| range.from <= range.to),
            ListInitializerKind::RecordLiteral | ListInitializerKind::Empty => slot.range.is_none(),
            ListInitializerKind::Unknown => true,
        })
}

fn malformed_file_read_bytes_op_count(plan: &MachinePlan) -> usize {
    plan.regions
        .iter()
        .flat_map(|region| &region.ops)
        .filter(|op| {
            matches!(
                &op.kind,
                PlanOpKind::UpdateBranch {
                    expression_kind: PlanExpressionKind::FileReadBytes,
                    ..
                }
            ) && !file_read_bytes_op_is_well_formed(plan, op)
        })
        .count()
}

fn file_read_bytes_op_is_well_formed(plan: &MachinePlan, op: &PlanOp) -> bool {
    let PlanOpKind::UpdateBranch {
        expression_kind: PlanExpressionKind::FileReadBytes,
        ordered_inputs,
        source_payload_field,
        update_constant_id,
        ..
    } = &op.kind
    else {
        return true;
    };
    if source_payload_field.is_some()
        || update_constant_id.is_some()
        || !output_state_is_bytes(&plan.storage_layout.scalar_slots, op)
        || update_branch_source_ids(op).len() != 1
        || !source_payload_input_ids(op).is_empty()
    {
        return false;
    }
    let [path_ref] = ordered_inputs.as_slice() else {
        return false;
    };
    match path_ref {
        ValueRef::Constant(path_constant_id) => {
            state_input_ids(op).is_empty()
                && plan_constant_by_id(&plan.constants, *path_constant_id).is_some_and(|constant| {
                    matches!(
                        &constant.value,
                        PlanConstantValue::Text { value } if file_read_bytes_path_is_static(value)
                    )
                })
        }
        ValueRef::State(path_state_id) => {
            state_input_ids(op).as_slice() == [*path_state_id]
                && matches!(
                    plan_value_type_for_state_slots(
                        &plan.storage_layout.scalar_slots,
                        *path_state_id
                    ),
                    Some(PlanValueType::Text)
                )
        }
        ValueRef::Field(field_id) => {
            op.indexed
                && op.inputs.contains(&ValueRef::Field(*field_id))
                && indexed_output_scope_owns_row_field(plan, op, *field_id)
                && plan_field_initial_value_matches_type(plan, *field_id, &PlanValueType::Text)
        }
        _ => false,
    }
}

fn malformed_file_write_bytes_op_count(plan: &MachinePlan) -> usize {
    plan.regions
        .iter()
        .flat_map(|region| &region.ops)
        .filter(|op| {
            matches!(
                &op.kind,
                PlanOpKind::UpdateBranch {
                    expression_kind: PlanExpressionKind::FileWriteBytes,
                    ..
                }
            ) && !file_write_bytes_op_is_well_formed(plan, op)
        })
        .count()
}

fn file_write_bytes_op_is_well_formed(plan: &MachinePlan, op: &PlanOp) -> bool {
    let PlanOpKind::UpdateBranch {
        expression_kind: PlanExpressionKind::FileWriteBytes,
        ordered_inputs,
        source_payload_field,
        update_constant_id,
        ..
    } = &op.kind
    else {
        return true;
    };
    if source_payload_field.is_some()
        || update_constant_id.is_some()
        || !output_state_type_is(&plan.storage_layout.scalar_slots, op, &PlanValueType::Text)
        || update_branch_source_ids(op).len() != 1
        || !source_payload_input_ids(op).is_empty()
    {
        return false;
    }
    let [ValueRef::State(input_state_id), path_ref] = ordered_inputs.as_slice() else {
        return false;
    };
    if !op.inputs.contains(&ValueRef::State(*input_state_id))
        || !matches!(
            plan_value_type_for_state_slots(&plan.storage_layout.scalar_slots, *input_state_id),
            Some(PlanValueType::Bytes { .. })
        )
    {
        return false;
    }
    match path_ref {
        ValueRef::Constant(path_constant_id) => {
            plan_constant_by_id(&plan.constants, *path_constant_id).is_some_and(|constant| {
                matches!(
                    &constant.value,
                    PlanConstantValue::Text { value } if file_read_bytes_path_is_static(value)
                )
            })
        }
        ValueRef::State(path_state_id) => {
            op.inputs.contains(&ValueRef::State(*path_state_id))
                && matches!(
                    plan_value_type_for_state_slots(
                        &plan.storage_layout.scalar_slots,
                        *path_state_id
                    ),
                    Some(PlanValueType::Text)
                )
        }
        ValueRef::Field(field_id) => {
            op.indexed
                && op.inputs.contains(&ValueRef::Field(*field_id))
                && indexed_output_scope_owns_row_field(plan, op, *field_id)
                && plan_field_initial_value_matches_type(plan, *field_id, &PlanValueType::Text)
        }
        _ => false,
    }
}

fn indexed_output_scope_owns_row_field(plan: &MachinePlan, op: &PlanOp, field_id: FieldId) -> bool {
    let Some(ValueRef::State(output_state_id)) = op.output else {
        return false;
    };
    let Some(output_slot) = plan
        .storage_layout
        .scalar_slots
        .iter()
        .find(|slot| slot.state_id == output_state_id)
    else {
        return false;
    };
    if !output_slot.indexed {
        return false;
    }
    let Some(output_scope_id) = output_slot.scope_id else {
        return false;
    };
    plan.storage_layout
        .list_slots
        .iter()
        .filter(|slot| slot.scope_id == Some(output_scope_id))
        .any(|slot| slot.row_field_ids.contains(&field_id))
}

fn indexed_output_scope_owns_row_field_from_slots(
    scalar_slots: &[ScalarStorageSlot],
    list_slots: &[ListStorageSlot],
    op: &PlanOp,
    field_id: FieldId,
) -> bool {
    let Some(ValueRef::State(output_state_id)) = op.output else {
        return false;
    };
    let Some(output_slot) = scalar_slots
        .iter()
        .find(|slot| slot.state_id == output_state_id)
    else {
        return false;
    };
    if !output_slot.indexed {
        return false;
    }
    let Some(output_scope_id) = output_slot.scope_id else {
        return false;
    };
    list_slots
        .iter()
        .filter(|slot| slot.scope_id == Some(output_scope_id))
        .any(|slot| slot.row_field_ids.contains(&field_id))
}

fn plan_field_initial_value_matches_type(
    plan: &MachinePlan,
    field_id: FieldId,
    value_type: &PlanValueType,
) -> bool {
    plan.storage_layout
        .list_slots
        .iter()
        .flat_map(|slot| &slot.initial_rows)
        .flat_map(|row| &row.fields)
        .filter(|field| field.field_id == Some(field_id))
        .all(|field| constant_value_matches_plan_type(&field.value, value_type))
        && plan
            .storage_layout
            .list_slots
            .iter()
            .flat_map(|slot| &slot.initial_rows)
            .flat_map(|row| &row.fields)
            .any(|field| field.field_id == Some(field_id))
}

fn list_field_initial_value_matches_type(
    list_slots: &[ListStorageSlot],
    field_id: FieldId,
    value_type: &PlanValueType,
) -> bool {
    list_slots
        .iter()
        .flat_map(|slot| &slot.initial_rows)
        .flat_map(|row| &row.fields)
        .filter(|field| field.field_id == Some(field_id))
        .all(|field| constant_value_matches_plan_type(&field.value, value_type))
        && list_slots
            .iter()
            .flat_map(|slot| &slot.initial_rows)
            .flat_map(|row| &row.fields)
            .any(|field| field.field_id == Some(field_id))
}

fn derived_expression_refs_resolve(plan: &MachinePlan) -> bool {
    plan.regions
        .iter()
        .flat_map(|region| &region.ops)
        .all(|op| {
            let PlanOpKind::DerivedValue {
                expression: Some(expression),
                ..
            } = &op.kind
            else {
                return true;
            };
            match expression {
                PlanDerivedExpression::SourceKeyTextTrimNonEmpty {
                    source_id,
                    key_field,
                    state,
                    ..
                } => {
                    op.inputs.contains(&ValueRef::SourcePayload {
                        source_id: *source_id,
                        field: key_field.clone(),
                    }) && op.inputs.contains(state)
                }
                PlanDerivedExpression::SourceEventTransform { default, arms, .. } => {
                    row_expression_refs_resolve(op, default)
                        && arms.iter().all(|arm| {
                            op.inputs.contains(&ValueRef::Source(arm.source_id))
                                && row_expression_refs_resolve(op, &arm.value)
                        })
                }
                PlanDerivedExpression::BoolNot { input } => op.inputs.contains(input),
                PlanDerivedExpression::NumberCompareConst { left, .. } => op.inputs.contains(left),
                PlanDerivedExpression::BoolAnd { left, right } => {
                    derived_expression_refs_resolve_for_op(op, left)
                        && derived_expression_refs_resolve_for_op(op, right)
                }
                PlanDerivedExpression::BoolNotExpression { input } => {
                    derived_expression_refs_resolve_for_op(op, input)
                }
                PlanDerivedExpression::RowExpression { expression } => {
                    row_expression_refs_resolve(op, expression)
                }
            }
        })
}

fn derived_expression_refs_resolve_for_op(op: &PlanOp, expression: &PlanDerivedExpression) -> bool {
    match expression {
        PlanDerivedExpression::SourceKeyTextTrimNonEmpty {
            source_id,
            key_field,
            state,
            ..
        } => {
            op.inputs.contains(&ValueRef::SourcePayload {
                source_id: *source_id,
                field: key_field.clone(),
            }) && op.inputs.contains(state)
        }
        PlanDerivedExpression::SourceEventTransform { default, arms, .. } => {
            row_expression_refs_resolve(op, default)
                && arms.iter().all(|arm| {
                    op.inputs.contains(&ValueRef::Source(arm.source_id))
                        && row_expression_refs_resolve(op, &arm.value)
                })
        }
        PlanDerivedExpression::BoolNot { input } => op.inputs.contains(input),
        PlanDerivedExpression::NumberCompareConst { left, .. } => op.inputs.contains(left),
        PlanDerivedExpression::BoolAnd { left, right } => {
            derived_expression_refs_resolve_for_op(op, left)
                && derived_expression_refs_resolve_for_op(op, right)
        }
        PlanDerivedExpression::BoolNotExpression { input } => {
            derived_expression_refs_resolve_for_op(op, input)
        }
        PlanDerivedExpression::RowExpression { expression } => {
            row_expression_refs_resolve(op, expression)
        }
    }
}

fn row_expression_refs_resolve(op: &PlanOp, expression: &PlanRowExpression) -> bool {
    match expression {
        PlanRowExpression::Field { input } => op.inputs.contains(input),
        PlanRowExpression::Constant { constant_id } => {
            op.inputs.contains(&ValueRef::Constant(*constant_id))
        }
        PlanRowExpression::TextTrim { input } | PlanRowExpression::TextIsEmpty { input } => {
            row_expression_refs_resolve(op, input)
        }
        PlanRowExpression::TextLength { input } | PlanRowExpression::TextToNumber { input } => {
            row_expression_refs_resolve(op, input)
        }
        PlanRowExpression::TextStartsWith { input, prefix } => {
            row_expression_refs_resolve(op, input) && row_expression_refs_resolve(op, prefix)
        }
        PlanRowExpression::TextSubstring {
            input,
            start,
            length,
        } => {
            row_expression_refs_resolve(op, input)
                && row_expression_refs_resolve(op, start)
                && row_expression_refs_resolve(op, length)
        }
        PlanRowExpression::TextToBytes { input, encoding } => {
            row_expression_refs_resolve(op, input)
                && encoding
                    .as_deref()
                    .is_none_or(|encoding| row_expression_refs_resolve(op, encoding))
        }
        PlanRowExpression::BytesToText { input, encoding } => {
            row_expression_refs_resolve(op, input)
                && encoding
                    .as_deref()
                    .is_none_or(|encoding| row_expression_refs_resolve(op, encoding))
        }
        PlanRowExpression::BytesToHex { input }
        | PlanRowExpression::BytesToBase64 { input }
        | PlanRowExpression::BytesFromHex { input }
        | PlanRowExpression::BytesFromBase64 { input } => row_expression_refs_resolve(op, input),
        PlanRowExpression::BytesIsEmpty { input } => row_expression_refs_resolve(op, input),
        PlanRowExpression::BytesLength { input } => row_expression_refs_resolve(op, input),
        PlanRowExpression::BytesGet { input, index } => {
            row_expression_refs_resolve(op, input) && row_expression_refs_resolve(op, index)
        }
        PlanRowExpression::BytesSlice {
            input,
            offset,
            byte_count,
        } => {
            row_expression_refs_resolve(op, input)
                && row_expression_refs_resolve(op, offset)
                && row_expression_refs_resolve(op, byte_count)
        }
        PlanRowExpression::BytesTake { input, byte_count }
        | PlanRowExpression::BytesDrop { input, byte_count } => {
            row_expression_refs_resolve(op, input) && row_expression_refs_resolve(op, byte_count)
        }
        PlanRowExpression::BytesZeros { byte_count } => row_expression_refs_resolve(op, byte_count),
        PlanRowExpression::BytesReadUnsigned {
            input,
            offset,
            byte_count,
            endian,
        }
        | PlanRowExpression::BytesReadSigned {
            input,
            offset,
            byte_count,
            endian,
        } => {
            row_expression_refs_resolve(op, input)
                && row_expression_refs_resolve(op, offset)
                && row_expression_refs_resolve(op, byte_count)
                && row_expression_refs_resolve(op, endian)
        }
        PlanRowExpression::BytesSet {
            input,
            index,
            value,
        } => {
            row_expression_refs_resolve(op, input)
                && row_expression_refs_resolve(op, index)
                && row_expression_refs_resolve(op, value)
        }
        PlanRowExpression::BytesWriteUnsigned {
            input,
            offset,
            byte_count,
            endian,
            value,
        }
        | PlanRowExpression::BytesWriteSigned {
            input,
            offset,
            byte_count,
            endian,
            value,
        } => {
            row_expression_refs_resolve(op, input)
                && row_expression_refs_resolve(op, offset)
                && row_expression_refs_resolve(op, byte_count)
                && row_expression_refs_resolve(op, endian)
                && row_expression_refs_resolve(op, value)
        }
        PlanRowExpression::BytesFind { input, needle } => {
            row_expression_refs_resolve(op, input) && row_expression_refs_resolve(op, needle)
        }
        PlanRowExpression::BytesStartsWith { input, prefix } => {
            row_expression_refs_resolve(op, input) && row_expression_refs_resolve(op, prefix)
        }
        PlanRowExpression::BytesEndsWith { input, suffix } => {
            row_expression_refs_resolve(op, input) && row_expression_refs_resolve(op, suffix)
        }
        PlanRowExpression::BytesConcat { left, right } => {
            row_expression_refs_resolve(op, left) && row_expression_refs_resolve(op, right)
        }
        PlanRowExpression::BytesEqual { left, right } => {
            row_expression_refs_resolve(op, left) && row_expression_refs_resolve(op, right)
        }
        PlanRowExpression::NumberInfix { left, right, .. } => {
            row_expression_refs_resolve(op, left) && row_expression_refs_resolve(op, right)
        }
        PlanRowExpression::TextConcat { parts } => parts
            .iter()
            .all(|part| row_expression_refs_resolve(op, part)),
        PlanRowExpression::ListGetField { list_id, index, .. } => {
            op.inputs.contains(&ValueRef::List(*list_id)) && row_expression_refs_resolve(op, index)
        }
        PlanRowExpression::ListRef { list_id } => op.inputs.contains(&ValueRef::List(*list_id)),
        PlanRowExpression::ListFindValue {
            list_id,
            value,
            fallback,
            ..
        } => {
            op.inputs.contains(&ValueRef::List(*list_id))
                && row_expression_refs_resolve(op, value)
                && fallback
                    .as_deref()
                    .is_none_or(|fallback| row_expression_refs_resolve(op, fallback))
        }
        PlanRowExpression::ListRange { from, to } => {
            row_expression_refs_resolve(op, from) && row_expression_refs_resolve(op, to)
        }
        PlanRowExpression::ListLiteral { items } => items
            .iter()
            .all(|item| row_expression_refs_resolve(op, item)),
        PlanRowExpression::ListMap { input, value, .. } => {
            row_expression_refs_resolve(op, input) && row_expression_refs_resolve(op, value)
        }
        PlanRowExpression::ListMapItem { .. } => true,
        PlanRowExpression::ListSum { input } => row_expression_refs_resolve(op, input),
        PlanRowExpression::Object { fields } => fields
            .iter()
            .all(|field| row_expression_refs_resolve(op, &field.value)),
        PlanRowExpression::ObjectField { object, .. } => row_expression_refs_resolve(op, object),
        PlanRowExpression::BuiltinCall { input, args, .. } => {
            input
                .as_deref()
                .is_none_or(|input| row_expression_refs_resolve(op, input))
                && args
                    .iter()
                    .all(|arg| row_expression_refs_resolve(op, &arg.value))
        }
        PlanRowExpression::Select { input, arms } => {
            row_expression_refs_resolve(op, input)
                && arms
                    .iter()
                    .all(|arm| row_expression_refs_resolve(op, &arm.value))
        }
    }
}

fn row_expression_list_fields_resolve(plan: &MachinePlan) -> bool {
    plan.regions
        .iter()
        .flat_map(|region| &region.ops)
        .all(|op| match &op.kind {
            PlanOpKind::DerivedValue {
                expression: Some(PlanDerivedExpression::RowExpression { expression }),
                ..
            } => row_expression_list_fields_resolve_inner(plan, expression),
            _ => true,
        })
}

fn row_expression_list_fields_resolve_inner(
    plan: &MachinePlan,
    expression: &PlanRowExpression,
) -> bool {
    match expression {
        PlanRowExpression::Field { .. }
        | PlanRowExpression::Constant { .. }
        | PlanRowExpression::ListRef { .. }
        | PlanRowExpression::ListMapItem { .. } => true,
        PlanRowExpression::Object { fields } => fields
            .iter()
            .all(|field| row_expression_list_fields_resolve_inner(plan, &field.value)),
        PlanRowExpression::TextTrim { input }
        | PlanRowExpression::TextIsEmpty { input }
        | PlanRowExpression::TextLength { input }
        | PlanRowExpression::TextToNumber { input }
        | PlanRowExpression::ObjectField { object: input, .. }
        | PlanRowExpression::ListSum { input } => {
            row_expression_list_fields_resolve_inner(plan, input)
        }
        PlanRowExpression::TextStartsWith { input, prefix } => {
            row_expression_list_fields_resolve_inner(plan, input)
                && row_expression_list_fields_resolve_inner(plan, prefix)
        }
        PlanRowExpression::TextSubstring {
            input,
            start,
            length,
        } => {
            row_expression_list_fields_resolve_inner(plan, input)
                && row_expression_list_fields_resolve_inner(plan, start)
                && row_expression_list_fields_resolve_inner(plan, length)
        }
        PlanRowExpression::TextToBytes { input, encoding } => {
            row_expression_list_fields_resolve_inner(plan, input)
                && encoding
                    .as_deref()
                    .is_none_or(|encoding| row_expression_list_fields_resolve_inner(plan, encoding))
        }
        PlanRowExpression::BytesToText { input, encoding } => {
            row_expression_list_fields_resolve_inner(plan, input)
                && encoding
                    .as_deref()
                    .is_none_or(|encoding| row_expression_list_fields_resolve_inner(plan, encoding))
        }
        PlanRowExpression::BytesToHex { input }
        | PlanRowExpression::BytesToBase64 { input }
        | PlanRowExpression::BytesFromHex { input }
        | PlanRowExpression::BytesFromBase64 { input } => {
            row_expression_list_fields_resolve_inner(plan, input)
        }
        PlanRowExpression::BytesIsEmpty { input } => {
            row_expression_list_fields_resolve_inner(plan, input)
        }
        PlanRowExpression::BytesLength { input } => {
            row_expression_list_fields_resolve_inner(plan, input)
        }
        PlanRowExpression::BytesGet { input, index } => {
            row_expression_list_fields_resolve_inner(plan, input)
                && row_expression_list_fields_resolve_inner(plan, index)
        }
        PlanRowExpression::BytesSlice {
            input,
            offset,
            byte_count,
        } => {
            row_expression_list_fields_resolve_inner(plan, input)
                && row_expression_list_fields_resolve_inner(plan, offset)
                && row_expression_list_fields_resolve_inner(plan, byte_count)
        }
        PlanRowExpression::BytesTake { input, byte_count }
        | PlanRowExpression::BytesDrop { input, byte_count } => {
            row_expression_list_fields_resolve_inner(plan, input)
                && row_expression_list_fields_resolve_inner(plan, byte_count)
        }
        PlanRowExpression::BytesZeros { byte_count } => {
            row_expression_list_fields_resolve_inner(plan, byte_count)
        }
        PlanRowExpression::BytesReadUnsigned {
            input,
            offset,
            byte_count,
            endian,
        }
        | PlanRowExpression::BytesReadSigned {
            input,
            offset,
            byte_count,
            endian,
        } => {
            row_expression_list_fields_resolve_inner(plan, input)
                && row_expression_list_fields_resolve_inner(plan, offset)
                && row_expression_list_fields_resolve_inner(plan, byte_count)
                && row_expression_list_fields_resolve_inner(plan, endian)
        }
        PlanRowExpression::BytesSet {
            input,
            index,
            value,
        } => {
            row_expression_list_fields_resolve_inner(plan, input)
                && row_expression_list_fields_resolve_inner(plan, index)
                && row_expression_list_fields_resolve_inner(plan, value)
        }
        PlanRowExpression::BytesWriteUnsigned {
            input,
            offset,
            byte_count,
            endian,
            value,
        }
        | PlanRowExpression::BytesWriteSigned {
            input,
            offset,
            byte_count,
            endian,
            value,
        } => {
            row_expression_list_fields_resolve_inner(plan, input)
                && row_expression_list_fields_resolve_inner(plan, offset)
                && row_expression_list_fields_resolve_inner(plan, byte_count)
                && row_expression_list_fields_resolve_inner(plan, endian)
                && row_expression_list_fields_resolve_inner(plan, value)
        }
        PlanRowExpression::BytesFind { input, needle } => {
            row_expression_list_fields_resolve_inner(plan, input)
                && row_expression_list_fields_resolve_inner(plan, needle)
        }
        PlanRowExpression::BytesStartsWith { input, prefix } => {
            row_expression_list_fields_resolve_inner(plan, input)
                && row_expression_list_fields_resolve_inner(plan, prefix)
        }
        PlanRowExpression::BytesEndsWith { input, suffix } => {
            row_expression_list_fields_resolve_inner(plan, input)
                && row_expression_list_fields_resolve_inner(plan, suffix)
        }
        PlanRowExpression::BytesConcat { left, right } => {
            row_expression_list_fields_resolve_inner(plan, left)
                && row_expression_list_fields_resolve_inner(plan, right)
        }
        PlanRowExpression::BytesEqual { left, right } => {
            row_expression_list_fields_resolve_inner(plan, left)
                && row_expression_list_fields_resolve_inner(plan, right)
        }
        PlanRowExpression::NumberInfix { left, right, .. } => {
            row_expression_list_fields_resolve_inner(plan, left)
                && row_expression_list_fields_resolve_inner(plan, right)
        }
        PlanRowExpression::TextConcat { parts } => parts
            .iter()
            .all(|part| row_expression_list_fields_resolve_inner(plan, part)),
        PlanRowExpression::ListGetField {
            list_id,
            index,
            field,
        } => {
            list_has_row_field(plan, *list_id, *field)
                && row_expression_list_fields_resolve_inner(plan, index)
        }
        PlanRowExpression::ListFindValue {
            list_id,
            field,
            value,
            target,
            fallback,
        } => {
            list_has_row_field(plan, *list_id, *field)
                && list_has_row_field(plan, *list_id, *target)
                && row_expression_list_fields_resolve_inner(plan, value)
                && fallback
                    .as_deref()
                    .is_none_or(|fallback| row_expression_list_fields_resolve_inner(plan, fallback))
        }
        PlanRowExpression::ListRange { from, to } => {
            row_expression_list_fields_resolve_inner(plan, from)
                && row_expression_list_fields_resolve_inner(plan, to)
        }
        PlanRowExpression::ListLiteral { items } => items
            .iter()
            .all(|item| row_expression_list_fields_resolve_inner(plan, item)),
        PlanRowExpression::ListMap { input, value, .. } => {
            row_expression_list_fields_resolve_inner(plan, input)
                && row_expression_list_fields_resolve_inner(plan, value)
        }
        PlanRowExpression::BuiltinCall { input, args, .. } => {
            input
                .as_deref()
                .is_none_or(|input| row_expression_list_fields_resolve_inner(plan, input))
                && args
                    .iter()
                    .all(|arg| row_expression_list_fields_resolve_inner(plan, &arg.value))
        }
        PlanRowExpression::Select { input, arms } => {
            row_expression_list_fields_resolve_inner(plan, input)
                && arms
                    .iter()
                    .all(|arm| row_expression_list_fields_resolve_inner(plan, &arm.value))
        }
    }
}

fn list_has_row_field(plan: &MachinePlan, list_id: ListId, field_id: FieldId) -> bool {
    plan.storage_layout
        .list_slots
        .iter()
        .find(|slot| slot.list_id == list_id)
        .is_some_and(|slot| slot.row_field_ids.contains(&field_id))
}

fn row_expression_cpu_evaluable(expression: &PlanRowExpression) -> bool {
    match expression {
        PlanRowExpression::ListFindValue {
            value, fallback, ..
        } => {
            row_expression_cpu_evaluable(value)
                && fallback.as_deref().is_none_or(row_expression_cpu_evaluable)
        }
        PlanRowExpression::Field { .. }
        | PlanRowExpression::Constant { .. }
        | PlanRowExpression::ListRef { .. }
        | PlanRowExpression::ListRange { .. }
        | PlanRowExpression::ListMapItem { .. } => true,
        PlanRowExpression::ListLiteral { items } => items.iter().all(row_expression_cpu_evaluable),
        PlanRowExpression::Object { fields } => fields
            .iter()
            .all(|field| row_expression_cpu_evaluable(&field.value)),
        PlanRowExpression::TextTrim { input }
        | PlanRowExpression::TextIsEmpty { input }
        | PlanRowExpression::TextLength { input }
        | PlanRowExpression::TextToNumber { input }
        | PlanRowExpression::ObjectField { object: input, .. }
        | PlanRowExpression::ListSum { input } => row_expression_cpu_evaluable(input),
        PlanRowExpression::TextStartsWith { input, prefix } => {
            row_expression_cpu_evaluable(input) && row_expression_cpu_evaluable(prefix)
        }
        PlanRowExpression::TextSubstring {
            input,
            start,
            length,
        } => {
            row_expression_cpu_evaluable(input)
                && row_expression_cpu_evaluable(start)
                && row_expression_cpu_evaluable(length)
        }
        PlanRowExpression::TextToBytes { input, encoding } => {
            row_expression_cpu_evaluable(input)
                && encoding
                    .as_deref()
                    .is_some_and(row_expression_cpu_evaluable)
        }
        PlanRowExpression::BytesToText { input, encoding } => {
            row_expression_cpu_evaluable(input)
                && encoding
                    .as_deref()
                    .is_some_and(row_expression_cpu_evaluable)
        }
        PlanRowExpression::BytesToHex { input }
        | PlanRowExpression::BytesToBase64 { input }
        | PlanRowExpression::BytesFromHex { input }
        | PlanRowExpression::BytesFromBase64 { input } => row_expression_cpu_evaluable(input),
        PlanRowExpression::BytesIsEmpty { input } => row_expression_cpu_evaluable(input),
        PlanRowExpression::BytesLength { input } => row_expression_cpu_evaluable(input),
        PlanRowExpression::BytesGet { input, index } => {
            row_expression_cpu_evaluable(input) && row_expression_cpu_evaluable(index)
        }
        PlanRowExpression::BytesSlice {
            input,
            offset,
            byte_count,
        } => {
            row_expression_cpu_evaluable(input)
                && row_expression_cpu_evaluable(offset)
                && row_expression_cpu_evaluable(byte_count)
        }
        PlanRowExpression::BytesTake { input, byte_count }
        | PlanRowExpression::BytesDrop { input, byte_count } => {
            row_expression_cpu_evaluable(input) && row_expression_cpu_evaluable(byte_count)
        }
        PlanRowExpression::BytesZeros { byte_count } => row_expression_cpu_evaluable(byte_count),
        PlanRowExpression::BytesReadUnsigned {
            input,
            offset,
            byte_count,
            endian,
        }
        | PlanRowExpression::BytesReadSigned {
            input,
            offset,
            byte_count,
            endian,
        } => {
            row_expression_cpu_evaluable(input)
                && row_expression_cpu_evaluable(offset)
                && row_expression_cpu_evaluable(byte_count)
                && row_expression_cpu_evaluable(endian)
        }
        PlanRowExpression::BytesSet {
            input,
            index,
            value,
        } => {
            row_expression_cpu_evaluable(input)
                && row_expression_cpu_evaluable(index)
                && row_expression_cpu_evaluable(value)
        }
        PlanRowExpression::BytesWriteUnsigned {
            input,
            offset,
            byte_count,
            endian,
            value,
        }
        | PlanRowExpression::BytesWriteSigned {
            input,
            offset,
            byte_count,
            endian,
            value,
        } => {
            row_expression_cpu_evaluable(input)
                && row_expression_cpu_evaluable(offset)
                && row_expression_cpu_evaluable(byte_count)
                && row_expression_cpu_evaluable(endian)
                && row_expression_cpu_evaluable(value)
        }
        PlanRowExpression::BytesFind { input, needle } => {
            row_expression_cpu_evaluable(input) && row_expression_cpu_evaluable(needle)
        }
        PlanRowExpression::BytesStartsWith { input, prefix } => {
            row_expression_cpu_evaluable(input) && row_expression_cpu_evaluable(prefix)
        }
        PlanRowExpression::BytesEndsWith { input, suffix } => {
            row_expression_cpu_evaluable(input) && row_expression_cpu_evaluable(suffix)
        }
        PlanRowExpression::BytesConcat { left, right } => {
            row_expression_cpu_evaluable(left) && row_expression_cpu_evaluable(right)
        }
        PlanRowExpression::BytesEqual { left, right } => {
            row_expression_cpu_evaluable(left) && row_expression_cpu_evaluable(right)
        }
        PlanRowExpression::NumberInfix { left, right, .. } => {
            row_expression_cpu_evaluable(left) && row_expression_cpu_evaluable(right)
        }
        PlanRowExpression::TextConcat { parts } => parts.iter().all(row_expression_cpu_evaluable),
        PlanRowExpression::ListGetField { index, .. } => row_expression_cpu_evaluable(index),
        PlanRowExpression::ListMap { input, value, .. } => {
            row_expression_cpu_evaluable(input) && row_expression_cpu_evaluable(value)
        }
        PlanRowExpression::BuiltinCall {
            function,
            input,
            args,
        } => {
            matches!(
                function.as_str(),
                "Text/empty"
                    | "Error/new"
                    | "Error/text"
                    | "Router/route"
                    | "List/count"
                    | "List/length"
                    | "List/retain"
                    | "List/filter_field_equal"
                    | "List/filter_field_not_equal"
                    | "List/filter_text_contains"
                    | "List/join_field"
            ) && input.as_deref().is_none_or(row_expression_cpu_evaluable)
                && args
                    .iter()
                    .all(|arg| row_expression_cpu_evaluable(&arg.value))
        }
        PlanRowExpression::Select { input, arms } => {
            row_expression_cpu_evaluable(input)
                && arms
                    .iter()
                    .all(|arm| row_expression_cpu_evaluable(&arm.value))
        }
    }
}

fn root_row_expression_cpu_evaluable(expression: &PlanRowExpression) -> bool {
    match expression {
        PlanRowExpression::Field { .. } | PlanRowExpression::Constant { .. } => true,
        PlanRowExpression::TextTrim { input }
        | PlanRowExpression::TextToNumber { input }
        | PlanRowExpression::ObjectField { object: input, .. } => {
            root_row_expression_cpu_evaluable(input)
        }
        PlanRowExpression::TextConcat { parts } => {
            parts.iter().all(root_row_expression_cpu_evaluable)
        }
        PlanRowExpression::ListFindValue {
            value, fallback, ..
        } => {
            root_row_expression_cpu_evaluable(value)
                && fallback
                    .as_deref()
                    .is_none_or(root_row_expression_cpu_evaluable)
        }
        PlanRowExpression::BuiltinCall {
            function,
            input,
            args,
        } => function == "Router/route" && input.is_none() && args.is_empty(),
        PlanRowExpression::Select { input, arms } => {
            root_row_expression_cpu_evaluable(input)
                && arms
                    .iter()
                    .all(|arm| root_row_expression_cpu_evaluable(&arm.value))
        }
        _ => false,
    }
}

pub fn plan_constant_by_id(
    constants: &[PlanConstant],
    id: PlanConstantId,
) -> Option<&PlanConstant> {
    constants.iter().find(|constant| constant.id == id)
}

fn plan_value_type_for_state(plan: &MachinePlan, state_id: StateId) -> Option<&PlanValueType> {
    plan_value_type_for_state_slots(&plan.storage_layout.scalar_slots, state_id)
}

pub fn plan_value_type_for_state_slots(
    scalar_slots: &[ScalarStorageSlot],
    state_id: StateId,
) -> Option<&PlanValueType> {
    scalar_slots
        .iter()
        .find(|slot| slot.state_id == state_id)
        .map(|slot| &slot.value_type)
}

fn source_guard_refs_resolve(op: &PlanOp, guard: &Option<PlanSourceGuard>) -> bool {
    let Some(guard) = guard else {
        return true;
    };
    match guard {
        PlanSourceGuard::SourcePayloadOneOf {
            source_id,
            field,
            values,
        } => {
            !values.is_empty()
                && op
                    .inputs
                    .iter()
                    .any(|input| matches!(input, ValueRef::Source(id) if id == source_id))
                && op.inputs.iter().any(|input| {
                    matches!(
                        input,
                        ValueRef::SourcePayload {
                            source_id: input_source_id,
                            field: input_field
                        } if input_source_id == source_id && input_field == field
                    )
                })
        }
    }
}

fn constant_value_matches_plan_type(value: &PlanConstantValue, value_type: &PlanValueType) -> bool {
    match (value, value_type) {
        (PlanConstantValue::Text { .. }, PlanValueType::Text) => true,
        (PlanConstantValue::Number { .. }, PlanValueType::Number) => true,
        (PlanConstantValue::Byte { .. }, PlanValueType::Byte) => true,
        (PlanConstantValue::Bool { .. }, PlanValueType::Bool) => true,
        (PlanConstantValue::Enum { .. }, PlanValueType::Enum) => true,
        (
            PlanConstantValue::Bytes { byte_len, .. },
            PlanValueType::Bytes {
                fixed_len: Some(fixed_len),
            },
        ) => byte_len == fixed_len,
        (PlanConstantValue::Bytes { .. }, PlanValueType::Bytes { fixed_len: None }) => true,
        (
            _,
            PlanValueType::RootInitialField
            | PlanValueType::RowInitialField
            | PlanValueType::Unknown,
        ) => true,
        _ => false,
    }
}

pub fn non_executable_constant_payload_count(constants: &[PlanConstant]) -> usize {
    constants
        .iter()
        .filter(|constant| {
            matches!(
                constant.value,
                PlanConstantValue::Bytes {
                    inline_bytes: None,
                    ..
                }
            )
        })
        .count()
}

#[cfg(test)]
mod tests;
