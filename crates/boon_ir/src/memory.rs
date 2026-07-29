use super::{
    ExecutableExprId, ExprId, FieldId, ListId, ScopeId, SemanticMemoryId, StateId, StaticOwnerId,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticMemoryKind {
    RootScalar,
    IndexedField,
    ListOwner,
    Map,
    Set,
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct SemanticMemoryIdentity {
    pub canonical_module: String,
    pub owner_path: String,
    pub semantic_path: String,
    pub kind: SemanticMemoryKind,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticMemory {
    pub id: SemanticMemoryId,
    pub identity: SemanticMemoryIdentity,
    pub data_type: SemanticDataType,
    pub leaves: Vec<SemanticMemoryLeaf>,
    pub status: SemanticMemoryStatus,
    pub runtime_backing: SemanticMemoryRuntimeBacking,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub structural_owner_rows: Vec<super::ErasedRowBinding>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticMemoryLeaf {
    pub semantic_path: String,
    pub data_type: SemanticDataType,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SemanticMemoryStatus {
    Active,
    Draining { marker_expr_id: ExprId },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SemanticMemoryRuntimeBacking {
    RootState {
        state_id: StateId,
        field_id: Option<FieldId>,
    },
    IndexedState {
        state_id: StateId,
        field_id: Option<FieldId>,
        scope_id: ScopeId,
        list_id: Option<ListId>,
    },
    List {
        list_id: ListId,
        row_scope_id: Option<ScopeId>,
    },
    Collection {
        expression: ExecutableExprId,
        owner: Option<StaticOwnerId>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SemanticDataType {
    Number,
    Text,
    Bytes {
        fixed_len: Option<usize>,
    },
    Variant {
        variants: Vec<SemanticVariantType>,
    },
    Record {
        fields: Vec<SemanticTypeField>,
        open: bool,
    },
    List {
        item: Box<SemanticDataType>,
    },
    Map {
        key: Box<SemanticDataType>,
        value: Box<SemanticDataType>,
    },
    Set {
        item: Box<SemanticDataType>,
    },
    Unknown {
        reason: String,
    },
    Union {
        members: Vec<SemanticDataType>,
    },
    Bits {
        width: u32,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticTypeField {
    pub name: String,
    pub data_type: SemanticDataType,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticVariantType {
    pub tag: String,
    pub fields: Vec<SemanticTypeField>,
    pub open: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct MigrationEdge {
    pub source_leaves: Vec<MigrationSourceLeaf>,
    pub destination: MigrationDestination,
    pub transfer_kind: MigrationTransferKind,
    pub transform: MigrationTransform,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct MigrationSourceLeaf {
    pub memory_id: SemanticMemoryId,
    pub semantic_path: String,
    pub data_type: SemanticDataType,
    pub drain_expr_id: ExprId,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct MigrationDestination {
    pub memory_id: SemanticMemoryId,
    pub semantic_path: String,
    pub data_type: SemanticDataType,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MigrationTransferKind {
    Scalar,
    List,
    IndexedField,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MigrationTransform {
    Identity,
    PureExpression {
        expression_root: ExprId,
        pipeline: Vec<ExprId>,
    },
}
