use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt;
use std::path::{Component, Path};

mod binary;
mod document;

pub use document::*;

pub const PLAN_MAJOR_VERSION: u32 = 3;
pub const PLAN_MINOR_VERSION: u32 = 0;
pub const PERSISTENCE_FORMAT_VERSION: u32 = 1;
pub const DEFAULT_PERSISTENCE_SCHEMA_VERSION: u64 = 1;
pub const INLINE_BYTE_CONSTANT_LIMIT: usize = 1024;

pub const DEFAULT_APPLICATION_PACKAGE_ID: &str = "boon.compiler.unspecified";
pub const DEFAULT_APPLICATION_STATE_NAMESPACE: &str = "default";
pub const DEFAULT_APPLICATION_DEPLOYMENT_DOMAIN: &str = "local";

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

macro_rules! plan_digest_ids {
    ($($name:ident),+ $(,)?) => {
        $(
            #[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
            #[serde(transparent)]
            pub struct $name(pub [u8; 32]);

            impl $name {
                pub fn as_bytes(&self) -> &[u8; 32] {
                    &self.0
                }
            }

            impl fmt::Display for $name {
                fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                    formatter.write_str(&digest_hex(&self.0))
                }
            }
        )+
    };
}

plan_digest_ids!(
    EffectId,
    EffectInvocationId,
    OutputRootId,
    MemoryId,
    MemoryLeafId,
    MigrationInputId,
    MigrationRecipeId,
    MigrationEdgeId
);

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct ApplicationIdentity {
    pub package_id: String,
    pub state_namespace: String,
    pub deployment_domain: String,
}

impl ApplicationIdentity {
    pub fn new(
        package_id: impl Into<String>,
        state_namespace: impl Into<String>,
        deployment_domain: impl Into<String>,
    ) -> Self {
        Self {
            package_id: package_id.into(),
            state_namespace: state_namespace.into(),
            deployment_domain: deployment_domain.into(),
        }
    }

    /// Compatibility identity for compile boundaries that have no host identity.
    /// Hosts that persist state must use an identity-aware compiler API instead.
    pub fn compiler_default() -> Self {
        Self::new(
            DEFAULT_APPLICATION_PACKAGE_ID,
            DEFAULT_APPLICATION_STATE_NAMESPACE,
            DEFAULT_APPLICATION_DEPLOYMENT_DOMAIN,
        )
    }

    pub fn is_valid(&self) -> bool {
        [
            self.package_id.as_str(),
            self.state_namespace.as_str(),
            self.deployment_domain.as_str(),
        ]
        .into_iter()
        .all(|component| !component.trim().is_empty())
    }
}

impl Default for ApplicationIdentity {
    fn default() -> Self {
        Self::compiler_default()
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ApplicationPlan {
    pub identity: ApplicationIdentity,
    pub identity_hash: [u8; 32],
}

impl ApplicationPlan {
    pub fn new(identity: ApplicationIdentity) -> Result<Self, PlanError> {
        let identity_hash = canonical_sha256(&identity)?;
        Ok(Self {
            identity,
            identity_hash,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryKind {
    Scalar,
    IndexedField,
    List,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct MemoryOwnerPath {
    pub canonical_module: String,
    pub named_owner_path: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DataTypePlan {
    Null,
    Bool,
    Number,
    Byte,
    Text,
    Bytes {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        fixed_len: Option<u64>,
    },
    Variant {
        variants: Vec<DataVariantPlan>,
    },
    Record {
        fields: Vec<DataTypeFieldPlan>,
        open: bool,
    },
    List {
        item: Box<DataTypePlan>,
    },
    Error {
        fields: Vec<DataTypeFieldPlan>,
        open: bool,
    },
    Unknown,
}

impl DataTypePlan {
    pub fn canonicalized(&self) -> Self {
        match self {
            Self::Variant { variants } => {
                let mut variants = variants
                    .iter()
                    .map(DataVariantPlan::canonicalized)
                    .collect::<Vec<_>>();
                variants.sort_by(|left, right| left.tag.cmp(&right.tag));
                variants.dedup_by(|left, right| left.tag == right.tag);
                Self::Variant { variants }
            }
            Self::Record { fields, open } => Self::Record {
                fields: canonical_data_type_fields(fields),
                open: *open,
            },
            Self::List { item } => Self::List {
                item: Box::new(item.canonicalized()),
            },
            Self::Error { fields, open } => Self::Error {
                fields: canonical_data_type_fields(fields),
                open: *open,
            },
            Self::Null
            | Self::Bool
            | Self::Number
            | Self::Byte
            | Self::Text
            | Self::Bytes { .. }
            | Self::Unknown => self.clone(),
        }
    }

    pub fn is_canonical(&self) -> bool {
        self == &self.canonicalized()
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DataTypeFieldPlan {
    pub name: String,
    pub data_type: DataTypePlan,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DataVariantPlan {
    pub tag: String,
    pub fields: Vec<DataTypeFieldPlan>,
    pub open: bool,
}

impl DataVariantPlan {
    fn canonicalized(&self) -> Self {
        Self {
            tag: self.tag.clone(),
            fields: canonical_data_type_fields(&self.fields),
            open: self.open,
        }
    }
}

fn canonical_data_type_fields(fields: &[DataTypeFieldPlan]) -> Vec<DataTypeFieldPlan> {
    let mut fields = fields
        .iter()
        .map(|field| DataTypeFieldPlan {
            name: field.name.clone(),
            data_type: field.data_type.canonicalized(),
        })
        .collect::<Vec<_>>();
    fields.sort_by(|left, right| left.name.cmp(&right.name));
    fields.dedup_by(|left, right| left.name == right.name);
    fields
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct EffectContract {
    pub effect_id: EffectId,
    pub host_operation: String,
    pub replay: EffectReplay,
    pub barrier: EffectBarrier,
    pub result_policy: EffectResultPolicy,
}

impl EffectContract {
    pub fn new(
        host_operation: impl Into<String>,
        replay: EffectReplay,
        barrier: EffectBarrier,
        result_policy: EffectResultPolicy,
    ) -> Result<Self, PlanError> {
        let host_operation = host_operation.into();
        let contract = Self {
            effect_id: EffectId::from_host_operation(&host_operation)?,
            host_operation,
            replay,
            barrier,
            result_policy,
        };
        contract.validate()?;
        Ok(contract)
    }

    pub fn validate(&self) -> Result<(), PlanError> {
        if self.host_operation.trim().is_empty()
            || self.host_operation.trim() != self.host_operation
        {
            return Err(PlanError::new(
                "effect host operation must be a non-empty canonical name",
            ));
        }
        if self.effect_id != EffectId::from_host_operation(&self.host_operation)? {
            return Err(PlanError::new(
                "effect ID does not match its canonical host operation",
            ));
        }
        if let EffectReplay::Idempotent { key_type } = &self.replay
            && (!key_type.is_canonical() || data_type_contains_unknown(key_type))
        {
            return Err(PlanError::new(
                "idempotent effect key type must be canonical and closed",
            ));
        }
        match (&self.replay, self.barrier, self.result_policy) {
            (EffectReplay::ReadOnly, EffectBarrier::None, _)
            | (EffectReplay::NonReplayable, EffectBarrier::None, EffectResultPolicy::Discarded)
            | (EffectReplay::Idempotent { .. }, EffectBarrier::Before, _)
            | (EffectReplay::Idempotent { .. }, EffectBarrier::BeforeAndAfter, _) => Ok(()),
            (EffectReplay::ReadOnly, _, _) => Err(PlanError::new(
                "read-only effects cannot require a persistence barrier",
            )),
            (EffectReplay::Idempotent { .. }, EffectBarrier::None, _) => Err(PlanError::new(
                "idempotent consequential effects require a persistence barrier",
            )),
            (EffectReplay::NonReplayable, _, _) => Err(PlanError::new(
                "non-replayable consequential effects have no safe persistence contract",
            )),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EffectReplay {
    ReadOnly,
    Idempotent { key_type: DataTypePlan },
    NonReplayable,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EffectBarrier {
    None,
    Before,
    BeforeAndAfter,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EffectResultPolicy {
    ReturnValue,
    Acknowledgement,
    CorrelatedSource,
    Discarded,
}

pub fn builtin_effect_contract(host_operation: &str) -> Result<Option<EffectContract>, PlanError> {
    let Some(spec) = boon_effect_schema::host_effect_spec(host_operation) else {
        return Ok(None);
    };
    let replay = match spec.replay {
        boon_effect_schema::ReplaySpec::ReadOnly => EffectReplay::ReadOnly,
        boon_effect_schema::ReplaySpec::IdempotentBytesKey => EffectReplay::Idempotent {
            key_type: DataTypePlan::Bytes {
                fixed_len: Some(32),
            },
        },
        boon_effect_schema::ReplaySpec::NonReplayable => EffectReplay::NonReplayable,
    };
    let contract = EffectContract {
        effect_id: EffectId::from_host_operation(spec.operation)?,
        host_operation: spec.operation.to_owned(),
        replay,
        barrier: match spec.barrier {
            boon_effect_schema::BarrierSpec::None => EffectBarrier::None,
            boon_effect_schema::BarrierSpec::Before => EffectBarrier::Before,
            boon_effect_schema::BarrierSpec::BeforeAndAfter => EffectBarrier::BeforeAndAfter,
        },
        result_policy: match spec.result_policy {
            boon_effect_schema::ResultPolicySpec::ReturnValue => EffectResultPolicy::ReturnValue,
            boon_effect_schema::ResultPolicySpec::Acknowledgement => {
                EffectResultPolicy::Acknowledgement
            }
            boon_effect_schema::ResultPolicySpec::CorrelatedSource => {
                EffectResultPolicy::CorrelatedSource
            }
            boon_effect_schema::ResultPolicySpec::Discarded => EffectResultPolicy::Discarded,
        },
    };
    Ok(Some(contract))
}

pub fn builtin_effect_outbox_schema(
    host_operation: &str,
) -> Result<Option<EffectOutboxSchema>, PlanError> {
    let Some(spec) = boon_effect_schema::host_effect_spec(host_operation) else {
        return Ok(None);
    };
    let Some(schema) = spec.durable_schema else {
        return Ok(None);
    };
    let intent_type = effect_schema_type_to_plan(&schema.intent);
    let result_type = effect_schema_type_to_plan(&schema.result);
    let contract = builtin_effect_contract(host_operation)?.ok_or_else(|| {
        PlanError::new(format!(
            "effect outbox schema `{host_operation}` has no built-in contract"
        ))
    })?;
    let EffectReplay::Idempotent { key_type } = contract.replay else {
        return Err(PlanError::new(format!(
            "effect outbox schema `{host_operation}` is not idempotent"
        )));
    };
    EffectOutboxSchema::new(contract.effect_id, intent_type, key_type, result_type).map(Some)
}

fn effect_schema_type_to_plan(value_type: &boon_effect_schema::ValueType) -> DataTypePlan {
    match value_type {
        boon_effect_schema::ValueType::Bool => DataTypePlan::Bool,
        boon_effect_schema::ValueType::Number => DataTypePlan::Number,
        boon_effect_schema::ValueType::Text => DataTypePlan::Text,
        boon_effect_schema::ValueType::Bytes { fixed_len } => DataTypePlan::Bytes {
            fixed_len: *fixed_len,
        },
        boon_effect_schema::ValueType::Record { fields, open } => DataTypePlan::Record {
            fields: fields
                .iter()
                .map(|field| DataTypeFieldPlan {
                    name: field.name.to_owned(),
                    data_type: effect_schema_type_to_plan(&field.value_type),
                })
                .collect(),
            open: *open,
        },
        boon_effect_schema::ValueType::Variant { variants } => DataTypePlan::Variant {
            variants: variants
                .iter()
                .map(|variant| DataVariantPlan {
                    tag: variant.tag.to_owned(),
                    fields: variant
                        .fields
                        .iter()
                        .map(|field| DataTypeFieldPlan {
                            name: field.name.to_owned(),
                            data_type: effect_schema_type_to_plan(&field.value_type),
                        })
                        .collect(),
                    open: false,
                })
                .collect(),
        },
    }
}

fn data_type_contains_unknown(data_type: &DataTypePlan) -> bool {
    match data_type {
        DataTypePlan::Unknown => true,
        DataTypePlan::Variant { variants } => variants.iter().any(|variant| {
            variant
                .fields
                .iter()
                .any(|field| data_type_contains_unknown(&field.data_type))
        }),
        DataTypePlan::Record { fields, .. } | DataTypePlan::Error { fields, .. } => fields
            .iter()
            .any(|field| data_type_contains_unknown(&field.data_type)),
        DataTypePlan::List { item } => data_type_contains_unknown(item),
        DataTypePlan::Null
        | DataTypePlan::Bool
        | DataTypePlan::Number
        | DataTypePlan::Byte
        | DataTypePlan::Text
        | DataTypePlan::Bytes { .. } => false,
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InitialProvenance {
    ReconstructableDefault,
    MaterializedAuthority,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct MemoryLeafPlan {
    pub leaf_id: MemoryLeafId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_field_id: Option<FieldId>,
    pub semantic_path: String,
    pub data_type: DataTypePlan,
    pub type_fingerprint: [u8; 32],
}

impl MemoryLeafPlan {
    pub fn new(
        memory_id: MemoryId,
        runtime_field_id: Option<FieldId>,
        semantic_path: impl Into<String>,
        data_type: DataTypePlan,
    ) -> Result<Self, PlanError> {
        let semantic_path = semantic_path.into();
        let data_type = data_type.canonicalized();
        Ok(Self {
            leaf_id: MemoryLeafId::from_memory_path(memory_id, &semantic_path)?,
            runtime_field_id,
            semantic_path,
            type_fingerprint: data_type_fingerprint(&data_type)?,
            data_type,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct MemoryPlan {
    pub runtime_slot: PlanStorageId,
    pub memory_id: MemoryId,
    pub kind: MemoryKind,
    pub semantic_path: String,
    pub data_type: DataTypePlan,
    pub type_fingerprint: [u8; 32],
    pub initial_provenance: InitialProvenance,
    pub owner: MemoryOwnerPath,
    pub leaves: Vec<MemoryLeafPlan>,
}

impl MemoryPlan {
    pub fn new(
        runtime_slot: PlanStorageId,
        kind: MemoryKind,
        semantic_path: impl Into<String>,
        data_type: DataTypePlan,
        initial_provenance: InitialProvenance,
        owner: MemoryOwnerPath,
    ) -> Result<Self, PlanError> {
        let semantic_path = semantic_path.into();
        let data_type = data_type.canonicalized();
        let memory_id = MemoryId::from_identity(&owner, &semantic_path, kind)?;
        let leaves = vec![MemoryLeafPlan::new(
            memory_id,
            None,
            semantic_path.clone(),
            data_type.clone(),
        )?];
        Ok(Self {
            runtime_slot,
            memory_id,
            kind,
            semantic_path,
            type_fingerprint: data_type_fingerprint(&data_type)?,
            data_type,
            initial_provenance,
            owner,
            leaves,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ListMemoryPlan {
    pub runtime_slot: PlanStorageId,
    pub memory_id: MemoryId,
    pub semantic_path: String,
    pub data_type: DataTypePlan,
    pub type_fingerprint: [u8; 32],
    pub initial_provenance: InitialProvenance,
    pub owner: MemoryOwnerPath,
    pub hidden_key_type: String,
    pub has_generation: bool,
    pub row_fields: Vec<MemoryLeafPlan>,
}

impl ListMemoryPlan {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        runtime_slot: PlanStorageId,
        semantic_path: impl Into<String>,
        data_type: DataTypePlan,
        initial_provenance: InitialProvenance,
        owner: MemoryOwnerPath,
        hidden_key_type: impl Into<String>,
        has_generation: bool,
        mut row_fields: Vec<MemoryLeafPlan>,
    ) -> Result<Self, PlanError> {
        let semantic_path = semantic_path.into();
        let data_type = data_type.canonicalized();
        let memory_id = MemoryId::from_identity(&owner, &semantic_path, MemoryKind::List)?;
        row_fields.sort_by_key(|field| field.leaf_id);
        Ok(Self {
            runtime_slot,
            memory_id,
            semantic_path,
            type_fingerprint: data_type_fingerprint(&data_type)?,
            data_type,
            initial_provenance,
            owner,
            hidden_key_type: hidden_key_type.into(),
            has_generation,
            row_fields,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MigrationTransferKindPlan {
    Scalar,
    List,
    IndexedRowField,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct MigrationLeafRefPlan {
    pub memory_id: MemoryId,
    pub leaf_id: MemoryLeafId,
    pub semantic_path: String,
    pub data_type: DataTypePlan,
    pub type_fingerprint: [u8; 32],
}

impl MigrationLeafRefPlan {
    pub fn new(
        memory_id: MemoryId,
        semantic_path: impl Into<String>,
        data_type: DataTypePlan,
    ) -> Result<Self, PlanError> {
        let semantic_path = semantic_path.into();
        let data_type = data_type.canonicalized();
        Ok(Self {
            memory_id,
            leaf_id: MemoryLeafId::from_memory_path(memory_id, &semantic_path)?,
            semantic_path,
            type_fingerprint: data_type_fingerprint(&data_type)?,
            data_type,
        })
    }

    fn is_canonical(&self) -> Result<bool, PlanError> {
        Ok(!self.semantic_path.trim().is_empty()
            && self.data_type.is_canonical()
            && self.leaf_id == MemoryLeafId::from_memory_path(self.memory_id, &self.semantic_path)?
            && self.type_fingerprint == data_type_fingerprint(&self.data_type)?)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct MigrationInputPlan {
    pub input_id: MigrationInputId,
    pub leaves: Vec<MigrationLeafRefPlan>,
    pub data_type: DataTypePlan,
    pub type_fingerprint: [u8; 32],
}

impl MigrationInputPlan {
    pub fn new(
        mut leaves: Vec<MigrationLeafRefPlan>,
        data_type: DataTypePlan,
    ) -> Result<Self, PlanError> {
        leaves.sort_by_key(|leaf| (leaf.memory_id, leaf.leaf_id));
        let data_type = data_type.canonicalized();
        let input_id = MigrationInputId::from_content(&leaves, &data_type)?;
        Ok(Self {
            input_id,
            leaves,
            type_fingerprint: data_type_fingerprint(&data_type)?,
            data_type,
        })
    }

    fn is_canonical(&self) -> Result<bool, PlanError> {
        Ok(!self.leaves.is_empty()
            && self.leaves.windows(2).all(|pair| {
                (pair[0].memory_id, pair[0].leaf_id) < (pair[1].memory_id, pair[1].leaf_id)
            })
            && self
                .leaves
                .iter()
                .map(|leaf| leaf.is_canonical())
                .collect::<Result<Vec<_>, _>>()?
                .into_iter()
                .all(|valid| valid)
            && self.data_type.is_canonical()
            && self.type_fingerprint == data_type_fingerprint(&self.data_type)?
            && self.input_id == MigrationInputId::from_content(&self.leaves, &self.data_type)?)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct MigrationDestinationPlan {
    pub memory_id: MemoryId,
    pub leaf_id: MemoryLeafId,
    pub semantic_path: String,
    pub data_type: DataTypePlan,
    pub type_fingerprint: [u8; 32],
}

impl MigrationDestinationPlan {
    pub fn new(
        memory_id: MemoryId,
        semantic_path: impl Into<String>,
        data_type: DataTypePlan,
    ) -> Result<Self, PlanError> {
        let semantic_path = semantic_path.into();
        let data_type = data_type.canonicalized();
        Ok(Self {
            memory_id,
            leaf_id: MemoryLeafId::from_memory_path(memory_id, &semantic_path)?,
            semantic_path,
            type_fingerprint: data_type_fingerprint(&data_type)?,
            data_type,
        })
    }

    fn is_canonical(&self) -> Result<bool, PlanError> {
        Ok(!self.semantic_path.trim().is_empty()
            && self.data_type.is_canonical()
            && self.leaf_id == MemoryLeafId::from_memory_path(self.memory_id, &self.semantic_path)?
            && self.type_fingerprint == data_type_fingerprint(&self.data_type)?)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct MigrationObjectFieldPlan {
    pub name: String,
    pub value: MigrationExpressionPlan,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct MigrationCallArgumentPlan {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub value: MigrationArgumentValuePlan,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MigrationArgumentValuePlan {
    Expression {
        value: Box<MigrationExpressionPlan>,
    },
    Lambda {
        parameter_count: u16,
        body: Box<MigrationExpressionPlan>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct MigrationMatchArmPlan {
    pub pattern: Vec<String>,
    pub output: MigrationExpressionPlan,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MigrationExpressionPlan {
    Input {
        input_id: MigrationInputId,
    },
    Parameter {
        index: u16,
    },
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
    Variant {
        tag: String,
    },
    Tagged {
        tag: String,
        fields: Vec<MigrationObjectFieldPlan>,
    },
    Project {
        input: Box<MigrationExpressionPlan>,
        fields: Vec<String>,
    },
    Call {
        function: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        input: Option<Box<MigrationExpressionPlan>>,
        arguments: Vec<MigrationCallArgumentPlan>,
    },
    Infix {
        operator: String,
        left: Box<MigrationExpressionPlan>,
        right: Box<MigrationExpressionPlan>,
    },
    Record {
        fields: Vec<MigrationObjectFieldPlan>,
    },
    List {
        items: Vec<MigrationExpressionPlan>,
    },
    Bytes {
        items: Vec<MigrationExpressionPlan>,
    },
    Match {
        input: Box<MigrationExpressionPlan>,
        arms: Vec<MigrationMatchArmPlan>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MigrationTransformPlan {
    Identity { input_id: MigrationInputId },
    Expression { root: MigrationExpressionPlan },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct MigrationTransferPlan {
    pub transfer_kind: MigrationTransferKindPlan,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub indexed_list_owner: Option<MigrationListOwnerPlan>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub list_row_fields: Vec<MigrationListRowFieldPlan>,
    pub inputs: Vec<MigrationInputPlan>,
    pub destination: MigrationDestinationPlan,
    pub transform: MigrationTransformPlan,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct MigrationListRowFieldPlan {
    pub source: MigrationLeafRefPlan,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub destination: Option<MigrationDestinationPlan>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct MigrationListOwnerPlan {
    pub memory_id: MemoryId,
    pub semantic_path: String,
    pub owner: MemoryOwnerPath,
}

impl MigrationListOwnerPlan {
    pub fn new(
        owner: MemoryOwnerPath,
        semantic_path: impl Into<String>,
    ) -> Result<Self, PlanError> {
        let semantic_path = semantic_path.into();
        Ok(Self {
            memory_id: MemoryId::from_identity(&owner, &semantic_path, MemoryKind::List)?,
            semantic_path,
            owner,
        })
    }

    fn is_canonical(&self) -> Result<bool, PlanError> {
        Ok(!self.owner.canonical_module.trim().is_empty()
            && !self.owner.named_owner_path.trim().is_empty()
            && !self.semantic_path.trim().is_empty()
            && self.memory_id
                == MemoryId::from_identity(&self.owner, &self.semantic_path, MemoryKind::List)?)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct MigrationRecipePlan {
    pub migration_recipe_id: MigrationRecipeId,
    /// Empty transfers are the canonical representation of a compiler-proven
    /// compatible schema transition. They perform no value transformation.
    pub transfers: Vec<MigrationTransferPlan>,
}

impl MigrationRecipePlan {
    pub fn new(mut transfers: Vec<MigrationTransferPlan>) -> Result<Self, PlanError> {
        canonicalize_migration_transfers(&mut transfers)?;
        let migration_recipe_id = MigrationRecipeId::from_transfers(&transfers)?;
        let recipe = Self {
            migration_recipe_id,
            transfers,
        };
        recipe.validate()?;
        Ok(recipe)
    }

    pub fn validate(&self) -> Result<(), PlanError> {
        let mut canonical = self.transfers.clone();
        canonicalize_migration_transfers(&mut canonical)?;
        if canonical != self.transfers {
            return Err(PlanError::new(
                "migration recipe transfers are not canonical",
            ));
        }
        if self.migration_recipe_id != MigrationRecipeId::from_transfers(&self.transfers)? {
            return Err(PlanError::new(
                "migration recipe ID does not match canonical transfer content",
            ));
        }
        validate_migration_transfers(&self.transfers)
    }

    pub fn is_noop(&self) -> bool {
        self.transfers.is_empty()
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct MigrationEdgePlan {
    pub migration_edge_id: MigrationEdgeId,
    pub source_schema_version: u64,
    pub target_schema_version: u64,
    pub source_schema_hash: [u8; 32],
    pub migration_recipe_id: MigrationRecipeId,
}

impl MigrationEdgePlan {
    pub fn new(
        source_schema_version: u64,
        target_schema_version: u64,
        source_schema_hash: [u8; 32],
        migration_recipe_id: MigrationRecipeId,
    ) -> Result<Self, PlanError> {
        Ok(Self {
            migration_edge_id: MigrationEdgeId::from_schema_transition(
                source_schema_version,
                target_schema_version,
                source_schema_hash,
                migration_recipe_id,
            )?,
            source_schema_version,
            target_schema_version,
            source_schema_hash,
            migration_recipe_id,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct EffectOutboxSchema {
    pub effect_id: EffectId,
    pub intent_type: DataTypePlan,
    pub idempotency_key_type: DataTypePlan,
    pub result_type: DataTypePlan,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub invocation_ids: Vec<EffectInvocationId>,
}

impl EffectOutboxSchema {
    pub fn new(
        effect_id: EffectId,
        intent_type: DataTypePlan,
        idempotency_key_type: DataTypePlan,
        result_type: DataTypePlan,
    ) -> Result<Self, PlanError> {
        let schema = Self {
            effect_id,
            intent_type: intent_type.canonicalized(),
            idempotency_key_type: idempotency_key_type.canonicalized(),
            result_type: result_type.canonicalized(),
            invocation_ids: Vec::new(),
        };
        schema.validate()?;
        Ok(schema)
    }

    pub fn validate(&self) -> Result<(), PlanError> {
        if [
            &self.intent_type,
            &self.idempotency_key_type,
            &self.result_type,
        ]
        .into_iter()
        .any(|data_type| !data_type.is_canonical() || data_type_contains_unknown(data_type))
        {
            return Err(PlanError::new(
                "effect outbox schemas must use canonical closed data types",
            ));
        }
        if self
            .invocation_ids
            .windows(2)
            .any(|pair| pair[0] >= pair[1])
        {
            return Err(PlanError::new(
                "effect outbox invocation IDs must be unique and canonically ordered",
            ));
        }
        Ok(())
    }

    pub fn bind_invocations(
        &mut self,
        invocation_ids: impl IntoIterator<Item = EffectInvocationId>,
    ) {
        self.invocation_ids.extend(invocation_ids);
        self.invocation_ids.sort();
        self.invocation_ids.dedup();
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PersistencePlan {
    pub format_version: u32,
    pub schema_version: u64,
    pub schema_hash: [u8; 32],
    pub migration_recipe_hash: [u8; 32],
    pub migration_catalog_hash: [u8; 32],
    pub memory: Vec<MemoryPlan>,
    pub lists: Vec<ListMemoryPlan>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub effect_outbox: Vec<EffectOutboxSchema>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub migration_recipes: Vec<MigrationRecipePlan>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_migration_recipe_id: Option<MigrationRecipeId>,
    pub migration_edges: Vec<MigrationEdgePlan>,
}

impl PersistencePlan {
    pub fn new(
        application: &ApplicationPlan,
        schema_version: u64,
        memory: Vec<MemoryPlan>,
        lists: Vec<ListMemoryPlan>,
        migration_edges: Vec<MigrationEdgePlan>,
    ) -> Result<Self, PlanError> {
        Self::new_with_migrations(
            application,
            schema_version,
            memory,
            lists,
            Vec::new(),
            None,
            migration_edges,
        )
    }

    pub fn new_with_migrations(
        application: &ApplicationPlan,
        schema_version: u64,
        memory: Vec<MemoryPlan>,
        lists: Vec<ListMemoryPlan>,
        migration_recipes: Vec<MigrationRecipePlan>,
        current_migration_recipe_id: Option<MigrationRecipeId>,
        migration_edges: Vec<MigrationEdgePlan>,
    ) -> Result<Self, PlanError> {
        Self::new_with_migrations_and_effect_outbox(
            application,
            schema_version,
            memory,
            lists,
            Vec::new(),
            migration_recipes,
            current_migration_recipe_id,
            migration_edges,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new_with_migrations_and_effect_outbox(
        application: &ApplicationPlan,
        schema_version: u64,
        mut memory: Vec<MemoryPlan>,
        mut lists: Vec<ListMemoryPlan>,
        mut effect_outbox: Vec<EffectOutboxSchema>,
        mut migration_recipes: Vec<MigrationRecipePlan>,
        current_migration_recipe_id: Option<MigrationRecipeId>,
        mut migration_edges: Vec<MigrationEdgePlan>,
    ) -> Result<Self, PlanError> {
        if schema_version == 0 {
            return Err(PlanError::new(
                "persistence schema version must be positive",
            ));
        }
        memory.sort_by_key(|memory| memory.memory_id);
        for memory in &mut memory {
            memory.leaves.sort_by_key(|leaf| leaf.leaf_id);
        }
        lists.sort_by_key(|list| list.memory_id);
        for list in &mut lists {
            list.row_fields.sort_by_key(|leaf| leaf.leaf_id);
        }
        effect_outbox.sort_by_key(|schema| schema.effect_id);
        migration_recipes.sort_by_key(|recipe| recipe.migration_recipe_id);
        migration_edges.sort_by_key(|edge| edge.migration_edge_id);
        let mut persistence = Self {
            format_version: PERSISTENCE_FORMAT_VERSION,
            schema_version,
            schema_hash: [0; 32],
            migration_recipe_hash: [0; 32],
            migration_catalog_hash: [0; 32],
            memory,
            lists,
            effect_outbox,
            migration_recipes,
            current_migration_recipe_id,
            migration_edges,
        };
        validate_effect_outbox_schemas(&persistence.effect_outbox)?;
        validate_migration_catalog(&persistence)?;
        persistence.schema_hash = persistence_schema_hash(application, &persistence)?;
        persistence.migration_recipe_hash = migration_recipe_hash(&persistence)?;
        persistence.migration_catalog_hash = migration_catalog_hash(&persistence)?;
        Ok(persistence)
    }

    pub fn validate_for_application(&self, application: &ApplicationPlan) -> Result<(), PlanError> {
        if *application != ApplicationPlan::new(application.identity.clone())? {
            return Err(PlanError::new(
                "predecessor application identity hash is invalid",
            ));
        }
        if self.format_version != PERSISTENCE_FORMAT_VERSION || self.schema_version == 0 {
            return Err(PlanError::new(
                "predecessor persistence format or schema version is invalid",
            ));
        }
        if !persistence_identities_unique(self) {
            return Err(PlanError::new(
                "predecessor persistence identities are not unique",
            ));
        }
        if !persistence_identities_match(self)? {
            return Err(PlanError::new(
                "predecessor persistence identities are not canonical",
            ));
        }
        if !persistence_type_fingerprints_match(self)? {
            return Err(PlanError::new(
                "predecessor persistence type fingerprints are invalid",
            ));
        }
        if !persistence_ordering_is_deterministic(self) {
            return Err(PlanError::new(
                "predecessor persistence entries are not canonically ordered",
            ));
        }
        validate_effect_outbox_schemas(&self.effect_outbox)?;
        validate_migration_catalog(self)?;
        if self.schema_hash != persistence_schema_hash(application, self)? {
            return Err(PlanError::new(
                "predecessor persistence schema hash is invalid",
            ));
        }
        if self.migration_recipe_hash != migration_recipe_hash(self)? {
            return Err(PlanError::new(
                "predecessor migration recipe hash is invalid",
            ));
        }
        if self.migration_catalog_hash != migration_catalog_hash(self)? {
            return Err(PlanError::new(
                "predecessor migration catalog hash is invalid",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct MigrationPredecessorBinding {
    /// The complete predecessor schema and inherited migration catalog. This
    /// lets compilation prove compatibility and preserve skipped-version paths.
    pub application: ApplicationPlan,
    pub persistence: PersistencePlan,
}

impl MigrationPredecessorBinding {
    pub fn from_machine_plan(plan: &MachinePlan) -> Self {
        Self {
            application: plan.application.clone(),
            persistence: plan.persistence.clone(),
        }
    }

    pub fn source_schema_version(&self) -> u64 {
        self.persistence.schema_version
    }

    pub fn source_schema_hash(&self) -> [u8; 32] {
        self.persistence.schema_hash
    }
}

fn canonicalize_migration_transfers(
    transfers: &mut [MigrationTransferPlan],
) -> Result<(), PlanError> {
    for transfer in transfers.iter_mut() {
        transfer.inputs.sort_by_key(|input| input.input_id);
        transfer.list_row_fields.sort_by_key(|field| {
            (
                field.source.memory_id,
                field.source.leaf_id,
                field.destination.as_ref().map(|value| value.memory_id),
                field.destination.as_ref().map(|value| value.leaf_id),
            )
        });
        canonicalize_migration_transform(&mut transfer.transform)?;
    }
    transfers.sort_by_key(|transfer| {
        (
            transfer.destination.memory_id,
            transfer.destination.leaf_id,
            transfer.transfer_kind,
        )
    });
    Ok(())
}

fn canonicalize_migration_transform(
    transform: &mut MigrationTransformPlan,
) -> Result<(), PlanError> {
    if let MigrationTransformPlan::Expression { root } = transform {
        canonicalize_migration_expression(root)?;
    }
    Ok(())
}

fn canonicalize_migration_expression(
    expression: &mut MigrationExpressionPlan,
) -> Result<(), PlanError> {
    match expression {
        MigrationExpressionPlan::Tagged { fields, .. }
        | MigrationExpressionPlan::Record { fields } => {
            canonicalize_migration_fields(fields)?;
        }
        MigrationExpressionPlan::Project { input, .. } => {
            canonicalize_migration_expression(input)?;
        }
        MigrationExpressionPlan::Call {
            input, arguments, ..
        } => {
            if let Some(input) = input {
                canonicalize_migration_expression(input)?;
            }
            for argument in arguments.iter_mut() {
                match &mut argument.value {
                    MigrationArgumentValuePlan::Expression { value } => {
                        canonicalize_migration_expression(value)?;
                    }
                    MigrationArgumentValuePlan::Lambda { body, .. } => {
                        canonicalize_migration_expression(body)?;
                    }
                }
            }
            let first_named = arguments
                .iter()
                .position(|argument| argument.name.is_some());
            let split = first_named.unwrap_or(arguments.len());
            if arguments[split..]
                .iter()
                .any(|argument| argument.name.is_none())
            {
                return Err(PlanError::new(
                    "migration call positional arguments must precede named arguments",
                ));
            }
            arguments[split..].sort_by(|left, right| left.name.cmp(&right.name));
            if arguments[split..]
                .windows(2)
                .any(|pair| pair[0].name == pair[1].name)
            {
                return Err(PlanError::new(
                    "migration call contains duplicate named arguments",
                ));
            }
        }
        MigrationExpressionPlan::Infix { left, right, .. } => {
            canonicalize_migration_expression(left)?;
            canonicalize_migration_expression(right)?;
        }
        MigrationExpressionPlan::List { items } | MigrationExpressionPlan::Bytes { items } => {
            for item in items {
                canonicalize_migration_expression(item)?;
            }
        }
        MigrationExpressionPlan::Match { input, arms } => {
            canonicalize_migration_expression(input)?;
            for arm in arms.iter_mut() {
                canonicalize_migration_expression(&mut arm.output)?;
            }
            arms.sort_by(|left, right| left.pattern.cmp(&right.pattern));
            if arms
                .windows(2)
                .any(|pair| pair[0].pattern == pair[1].pattern)
            {
                return Err(PlanError::new(
                    "migration match contains duplicate patterns",
                ));
            }
        }
        MigrationExpressionPlan::Input { .. }
        | MigrationExpressionPlan::Parameter { .. }
        | MigrationExpressionPlan::Text { .. }
        | MigrationExpressionPlan::Number { .. }
        | MigrationExpressionPlan::Byte { .. }
        | MigrationExpressionPlan::Bool { .. }
        | MigrationExpressionPlan::Variant { .. } => {}
    }
    Ok(())
}

fn canonicalize_migration_fields(fields: &mut [MigrationObjectFieldPlan]) -> Result<(), PlanError> {
    for field in fields.iter_mut() {
        canonicalize_migration_expression(&mut field.value)?;
    }
    fields.sort_by(|left, right| left.name.cmp(&right.name));
    if fields.windows(2).any(|pair| pair[0].name == pair[1].name) {
        return Err(PlanError::new(
            "migration record contains duplicate field names",
        ));
    }
    Ok(())
}

fn validate_migration_transfers(transfers: &[MigrationTransferPlan]) -> Result<(), PlanError> {
    let mut destinations = BTreeSet::new();
    let mut consumed_leaves = BTreeSet::new();
    let mut dependency_graph = std::collections::BTreeMap::<MemoryId, BTreeSet<MemoryId>>::new();
    for transfer in transfers {
        match (&transfer.transfer_kind, &transfer.indexed_list_owner) {
            (MigrationTransferKindPlan::IndexedRowField, Some(owner))
                if owner.is_canonical()? =>
            {
                if owner.memory_id != transfer.destination.memory_id
                    || transfer
                        .inputs
                        .iter()
                        .flat_map(|input| &input.leaves)
                        .any(|leaf| leaf.memory_id != owner.memory_id)
                {
                    return Err(PlanError::new(
                        "indexed migration row leaves must use their list-owner durable identity",
                    ));
                }
            }
            (MigrationTransferKindPlan::IndexedRowField, _) => {
                return Err(PlanError::new(
                    "indexed migration requires a canonical list-owner identity",
                ));
            }
            (_, None) => {}
            (_, Some(_)) => {
                return Err(PlanError::new(
                    "only indexed migration may declare a list-owner identity",
                ));
            }
        }
        if !transfer.destination.is_canonical()? {
            return Err(PlanError::new(
                "migration destination does not match its canonical identity or type",
            ));
        }
        if !destinations.insert((transfer.destination.memory_id, transfer.destination.leaf_id)) {
            return Err(PlanError::new(
                "migration recipe writes a destination more than once",
            ));
        }
        if matches!(transfer.transfer_kind, MigrationTransferKindPlan::List)
            != matches!(transfer.destination.data_type, DataTypePlan::List { .. })
        {
            return Err(PlanError::new(
                "migration list transfer kind does not match destination type",
            ));
        }
        if transfer.transfer_kind != MigrationTransferKindPlan::List
            && !transfer.list_row_fields.is_empty()
        {
            return Err(PlanError::new(
                "only whole-list migration may declare row-field identity mappings",
            ));
        }
        let mut input_ids = BTreeSet::new();
        for input in &transfer.inputs {
            if !input.is_canonical()? || !input_ids.insert(input.input_id) {
                return Err(PlanError::new(
                    "migration input is non-canonical or duplicated",
                ));
            }
            for leaf in &input.leaves {
                if !consumed_leaves.insert((leaf.memory_id, leaf.leaf_id)) {
                    return Err(PlanError::new(
                        "migration recipe consumes a source leaf more than once",
                    ));
                }
                if leaf.memory_id == transfer.destination.memory_id
                    && leaf.leaf_id == transfer.destination.leaf_id
                {
                    return Err(PlanError::new(
                        "migration recipe cannot transfer a leaf onto itself",
                    ));
                }
                if leaf.memory_id != transfer.destination.memory_id {
                    dependency_graph
                        .entry(leaf.memory_id)
                        .or_default()
                        .insert(transfer.destination.memory_id);
                }
            }
        }
        if input_ids.is_empty() {
            return Err(PlanError::new(
                "migration transfer must consume at least one input",
            ));
        }
        if transfer.transfer_kind == MigrationTransferKindPlan::List {
            let source_memory_id = transfer
                .inputs
                .first()
                .filter(|_| transfer.inputs.len() == 1)
                .and_then(|input| input.leaves.first().filter(|_| input.leaves.len() == 1))
                .map(|leaf| leaf.memory_id)
                .ok_or_else(|| {
                    PlanError::new("whole-list migration must consume one canonical list input")
                })?;
            let mut source_fields = BTreeSet::new();
            let mut destination_fields = BTreeSet::new();
            for field in &transfer.list_row_fields {
                if !field.source.is_canonical()?
                    || !field
                        .destination
                        .as_ref()
                        .map(|destination| destination.is_canonical())
                        .transpose()?
                        .unwrap_or(true)
                {
                    return Err(PlanError::new(
                        "whole-list row-field mapping is not canonical",
                    ));
                }
                if field.source.memory_id != source_memory_id {
                    return Err(PlanError::new(
                        "whole-list row-field mapping crosses its list transfer identities",
                    ));
                }
                if !source_fields.insert(field.source.leaf_id) {
                    return Err(PlanError::new(
                        "whole-list row-field identity mapping is not one-to-one",
                    ));
                }
                if let Some(destination) = &field.destination {
                    if destination.memory_id != transfer.destination.memory_id {
                        return Err(PlanError::new(
                            "whole-list row-field mapping crosses its list transfer identities",
                        ));
                    }
                    if field.source.data_type != destination.data_type
                        || field.source.type_fingerprint != destination.type_fingerprint
                    {
                        return Err(PlanError::new(
                            "whole-list row-field identity mapping changes field type",
                        ));
                    }
                    if !destination_fields.insert(destination.leaf_id) {
                        return Err(PlanError::new(
                            "whole-list row-field identity mapping is not one-to-one",
                        ));
                    }
                }
            }
        }
        match &transfer.transform {
            MigrationTransformPlan::Identity { input_id } => {
                if transfer.inputs.len() != 1 || !input_ids.contains(input_id) {
                    return Err(PlanError::new(
                        "identity migration must reference its single canonical input",
                    ));
                }
            }
            MigrationTransformPlan::Expression { root } => {
                let mut used_inputs = BTreeSet::new();
                validate_migration_expression(root, &input_ids, 0, &mut used_inputs)?;
                if used_inputs != input_ids {
                    return Err(PlanError::new(
                        "migration expression must consume every declared input exactly by identity",
                    ));
                }
            }
        }
    }
    if migration_memory_graph_has_cycle(&dependency_graph) {
        return Err(PlanError::new("migration recipe contains a memory cycle"));
    }
    Ok(())
}

fn validate_migration_expression(
    expression: &MigrationExpressionPlan,
    inputs: &BTreeSet<MigrationInputId>,
    parameter_depth: u32,
    used_inputs: &mut BTreeSet<MigrationInputId>,
) -> Result<(), PlanError> {
    match expression {
        MigrationExpressionPlan::Input { input_id } => {
            if !inputs.contains(input_id) {
                return Err(PlanError::new(
                    "migration expression references an undeclared input",
                ));
            }
            used_inputs.insert(*input_id);
        }
        MigrationExpressionPlan::Parameter { index } => {
            if u32::from(*index) >= parameter_depth {
                return Err(PlanError::new(
                    "migration expression references an out-of-scope lambda parameter",
                ));
            }
        }
        MigrationExpressionPlan::Variant { tag } if tag.trim().is_empty() => {
            return Err(PlanError::new("migration variant tag must not be empty"));
        }
        MigrationExpressionPlan::Tagged { tag, fields } => {
            if tag.trim().is_empty() {
                return Err(PlanError::new("migration tagged value must have a tag"));
            }
            validate_migration_fields(fields, inputs, parameter_depth, used_inputs)?;
        }
        MigrationExpressionPlan::Project { input, fields } => {
            if fields.is_empty() || fields.iter().any(|field| field.trim().is_empty()) {
                return Err(PlanError::new(
                    "migration projection must contain non-empty field names",
                ));
            }
            validate_migration_expression(input, inputs, parameter_depth, used_inputs)?;
        }
        MigrationExpressionPlan::Call {
            function,
            input,
            arguments,
        } => {
            if !migration_call_is_supported(function) {
                return Err(PlanError::new(format!(
                    "migration recipe contains non-target-neutral call `{function}`"
                )));
            }
            if let Some(input) = input {
                validate_migration_expression(input, inputs, parameter_depth, used_inputs)?;
            }
            for argument in arguments {
                match &argument.value {
                    MigrationArgumentValuePlan::Expression { value } => {
                        validate_migration_expression(value, inputs, parameter_depth, used_inputs)?;
                    }
                    MigrationArgumentValuePlan::Lambda {
                        parameter_count,
                        body,
                    } => {
                        if *parameter_count == 0 {
                            return Err(PlanError::new(
                                "migration lambda must declare at least one parameter",
                            ));
                        }
                        validate_migration_expression(
                            body,
                            inputs,
                            parameter_depth + u32::from(*parameter_count),
                            used_inputs,
                        )?;
                    }
                }
            }
        }
        MigrationExpressionPlan::Infix {
            operator,
            left,
            right,
        } => {
            if operator.trim().is_empty() {
                return Err(PlanError::new("migration infix operator must not be empty"));
            }
            validate_migration_expression(left, inputs, parameter_depth, used_inputs)?;
            validate_migration_expression(right, inputs, parameter_depth, used_inputs)?;
        }
        MigrationExpressionPlan::Record { fields } => {
            validate_migration_fields(fields, inputs, parameter_depth, used_inputs)?;
        }
        MigrationExpressionPlan::List { items } | MigrationExpressionPlan::Bytes { items } => {
            for item in items {
                validate_migration_expression(item, inputs, parameter_depth, used_inputs)?;
            }
        }
        MigrationExpressionPlan::Match { input, arms } => {
            if arms.is_empty()
                || arms.iter().any(|arm| {
                    arm.pattern.is_empty() || arm.pattern.iter().any(|part| part.trim().is_empty())
                })
            {
                return Err(PlanError::new(
                    "migration match must contain non-empty canonical arms",
                ));
            }
            validate_migration_expression(input, inputs, parameter_depth, used_inputs)?;
            for arm in arms {
                validate_migration_expression(&arm.output, inputs, parameter_depth, used_inputs)?;
            }
        }
        MigrationExpressionPlan::Text { .. }
        | MigrationExpressionPlan::Number { .. }
        | MigrationExpressionPlan::Byte { .. }
        | MigrationExpressionPlan::Bool { .. }
        | MigrationExpressionPlan::Variant { .. } => {}
    }
    Ok(())
}

fn validate_migration_fields(
    fields: &[MigrationObjectFieldPlan],
    inputs: &BTreeSet<MigrationInputId>,
    parameter_depth: u32,
    used_inputs: &mut BTreeSet<MigrationInputId>,
) -> Result<(), PlanError> {
    if fields.iter().any(|field| field.name.trim().is_empty()) {
        return Err(PlanError::new(
            "migration record field name must not be empty",
        ));
    }
    for field in fields {
        validate_migration_expression(&field.value, inputs, parameter_depth, used_inputs)?;
    }
    Ok(())
}

pub fn migration_call_is_supported(function: &str) -> bool {
    matches!(
        function,
        "Text/empty"
            | "Text/space"
            | "Text/trim"
            | "Text/to_uppercase"
            | "Text/concat"
            | "Text/substring"
            | "Text/is_empty"
            | "Text/is_not_empty"
            | "Text/starts_with"
            | "Text/contains"
            | "Text/find"
            | "Text/length"
            | "Text/to_number"
            | "Text/to_bytes"
            | "Number/add"
            | "Number/subtract"
            | "Number/min"
            | "Number/max"
            | "Number/to_text"
            | "Bool/not"
            | "Bool/and"
            | "Bytes/length"
            | "Bytes/is_empty"
            | "Bytes/get"
            | "Bytes/set"
            | "Bytes/slice"
            | "Bytes/take"
            | "Bytes/drop"
            | "Bytes/concat"
            | "Bytes/equal"
            | "Bytes/find"
            | "Bytes/starts_with"
            | "Bytes/ends_with"
            | "Bytes/to_text"
            | "Bytes/to_hex"
            | "Bytes/to_base64"
            | "Bytes/from_hex"
            | "Bytes/from_base64"
            | "Bytes/zeros"
            | "Bytes/read_unsigned"
            | "Bytes/read_signed"
            | "Bytes/write_unsigned"
            | "Bytes/write_signed"
            | "List/map"
            | "List/retain"
            | "List/range"
            | "List/chunk"
            | "List/get"
            | "List/count"
            | "List/length"
            | "List/sum"
            | "List/every"
            | "List/any"
            | "List/is_not_empty"
    )
}

fn migration_memory_graph_has_cycle(
    graph: &std::collections::BTreeMap<MemoryId, BTreeSet<MemoryId>>,
) -> bool {
    fn visit(
        node: MemoryId,
        graph: &std::collections::BTreeMap<MemoryId, BTreeSet<MemoryId>>,
        active: &mut BTreeSet<MemoryId>,
        complete: &mut BTreeSet<MemoryId>,
    ) -> bool {
        if complete.contains(&node) {
            return false;
        }
        if !active.insert(node) {
            return true;
        }
        if graph
            .get(&node)
            .into_iter()
            .flatten()
            .copied()
            .any(|next| visit(next, graph, active, complete))
        {
            return true;
        }
        active.remove(&node);
        complete.insert(node);
        false
    }

    let mut active = BTreeSet::new();
    let mut complete = BTreeSet::new();
    graph
        .keys()
        .copied()
        .any(|node| visit(node, graph, &mut active, &mut complete))
}

fn validate_effect_outbox_schemas(schemas: &[EffectOutboxSchema]) -> Result<(), PlanError> {
    let mut effect_ids = BTreeSet::new();
    for schema in schemas {
        schema.validate()?;
        if !effect_ids.insert(schema.effect_id) {
            return Err(PlanError::new(
                "effect outbox schema contains a duplicate effect ID",
            ));
        }
    }
    if schemas
        .windows(2)
        .any(|pair| pair[0].effect_id > pair[1].effect_id)
    {
        return Err(PlanError::new(
            "effect outbox schemas are not canonically ordered",
        ));
    }
    Ok(())
}

fn validate_migration_catalog(persistence: &PersistencePlan) -> Result<(), PlanError> {
    let mut recipe_ids = BTreeSet::new();
    for recipe in &persistence.migration_recipes {
        recipe.validate()?;
        if !recipe_ids.insert(recipe.migration_recipe_id) {
            return Err(PlanError::new(
                "migration catalog contains a duplicate recipe ID",
            ));
        }
    }
    if let Some(current) = persistence.current_migration_recipe_id
        && !recipe_ids.contains(&current)
    {
        return Err(PlanError::new(
            "current migration recipe ID is absent from the recipe catalog",
        ));
    }
    let mut predecessor_bindings = BTreeSet::new();
    for edge in &persistence.migration_edges {
        if edge.source_schema_version == 0
            || edge.source_schema_version >= edge.target_schema_version
            || edge.target_schema_version > persistence.schema_version
            || !recipe_ids.contains(&edge.migration_recipe_id)
            || edge.migration_edge_id
                != MigrationEdgeId::from_schema_transition(
                    edge.source_schema_version,
                    edge.target_schema_version,
                    edge.source_schema_hash,
                    edge.migration_recipe_id,
                )?
        {
            return Err(PlanError::new(
                "migration catalog edge is not a canonical forward predecessor binding",
            ));
        }
        if !predecessor_bindings.insert((
            edge.source_schema_version,
            edge.source_schema_hash,
            edge.target_schema_version,
        )) {
            return Err(PlanError::new(
                "migration catalog binds the same predecessor more than once",
            ));
        }
    }
    Ok(())
}

#[derive(Serialize)]
struct MemoryIdentityInput<'a> {
    canonical_module: &'a str,
    named_owner_path: &'a str,
    semantic_memory_path: &'a str,
    memory_kind: MemoryKind,
}

#[derive(Serialize)]
struct EffectIdentityInput<'a> {
    namespace: &'static str,
    host_operation: &'a str,
}

impl EffectId {
    pub fn from_host_operation(host_operation: &str) -> Result<Self, PlanError> {
        if host_operation.trim().is_empty() || host_operation.trim() != host_operation {
            return Err(PlanError::new(
                "effect host operation must be a non-empty canonical name",
            ));
        }
        Ok(Self(canonical_sha256(&EffectIdentityInput {
            namespace: "boon.host-effect.v1",
            host_operation,
        })?))
    }
}

#[derive(Serialize)]
struct EffectInvocationIdentityInput<'a> {
    namespace: &'static str,
    effect_id: EffectId,
    source_path: &'a str,
    target_path: &'a str,
}

impl EffectInvocationId {
    pub fn from_semantic_route(
        effect_id: EffectId,
        source_path: &str,
        target_path: &str,
    ) -> Result<Self, PlanError> {
        if source_path.trim().is_empty() || target_path.trim().is_empty() {
            return Err(PlanError::new(
                "effect invocation source and target paths must be non-empty",
            ));
        }
        Ok(Self(canonical_sha256(&EffectInvocationIdentityInput {
            namespace: "boon.effect-invocation.v1",
            effect_id,
            source_path,
            target_path,
        })?))
    }
}

#[derive(Serialize)]
struct OutputRootIdentityInput<'a> {
    namespace: &'static str,
    name: &'a str,
}

impl OutputRootId {
    pub fn from_name(name: &str) -> Result<Self, PlanError> {
        if name.trim().is_empty() || name.trim() != name {
            return Err(PlanError::new(
                "output root name must be a non-empty canonical name",
            ));
        }
        Ok(Self(canonical_sha256(&OutputRootIdentityInput {
            namespace: "boon.output-root.v1",
            name,
        })?))
    }
}

impl MemoryId {
    pub fn from_identity(
        owner: &MemoryOwnerPath,
        semantic_path: &str,
        kind: MemoryKind,
    ) -> Result<Self, PlanError> {
        Ok(Self(canonical_sha256(&MemoryIdentityInput {
            canonical_module: &owner.canonical_module,
            named_owner_path: &owner.named_owner_path,
            semantic_memory_path: semantic_path,
            memory_kind: kind,
        })?))
    }
}

#[derive(Serialize)]
struct MemoryLeafIdentityInput<'a> {
    memory_id: MemoryId,
    semantic_leaf_path: &'a str,
}

impl MemoryLeafId {
    pub fn from_memory_path(memory_id: MemoryId, semantic_path: &str) -> Result<Self, PlanError> {
        Ok(Self(canonical_sha256(&MemoryLeafIdentityInput {
            memory_id,
            semantic_leaf_path: semantic_path,
        })?))
    }
}

#[derive(Serialize)]
struct MigrationInputIdentityInput<'a> {
    leaves: &'a [MigrationLeafRefPlan],
    data_type: DataTypePlan,
}

impl MigrationInputId {
    pub fn from_content(
        leaves: &[MigrationLeafRefPlan],
        data_type: &DataTypePlan,
    ) -> Result<Self, PlanError> {
        Ok(Self(canonical_sha256(&MigrationInputIdentityInput {
            leaves,
            data_type: data_type.canonicalized(),
        })?))
    }
}

#[derive(Serialize)]
struct MigrationRecipeIdentityInput<'a> {
    transfers: &'a [MigrationTransferPlan],
}

impl MigrationRecipeId {
    pub fn from_transfers(transfers: &[MigrationTransferPlan]) -> Result<Self, PlanError> {
        Ok(Self(canonical_sha256(&MigrationRecipeIdentityInput {
            transfers,
        })?))
    }
}

#[derive(Serialize)]
struct MigrationEdgeIdentityInput {
    source_schema_version: u64,
    target_schema_version: u64,
    source_schema_hash: [u8; 32],
    migration_recipe_id: MigrationRecipeId,
}

impl MigrationEdgeId {
    pub fn from_schema_transition(
        source_schema_version: u64,
        target_schema_version: u64,
        source_schema_hash: [u8; 32],
        migration_recipe_id: MigrationRecipeId,
    ) -> Result<Self, PlanError> {
        Ok(Self(canonical_sha256(&MigrationEdgeIdentityInput {
            source_schema_version,
            target_schema_version,
            source_schema_hash,
            migration_recipe_id,
        })?))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SourcePayloadSchema {
    pub fields: Vec<SourcePayloadField>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub typed_fields: Vec<SourcePayloadDescriptor>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub row_lookup_field: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub row_lookup_field_id: Option<FieldId>,
}

impl SourcePayloadSchema {
    pub fn row_lookup_field_name(&self) -> Option<&str> {
        self.row_lookup_field.as_deref()
    }

    pub fn row_lookup_field_id(&self) -> Option<FieldId> {
        self.row_lookup_field_id
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
    Number,
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
#[serde(rename_all = "snake_case")]
pub enum OutputContractKind {
    Document,
    Scene,
    HostValue { data_type: DataTypePlan },
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputDemandPolicy {
    HostDemanded,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OutputValueRef {
    RetainedVisual { expression: DocumentExprId },
    RuntimeValue { value: ValueRef },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct OutputRootPlan {
    pub id: OutputRootId,
    pub name: String,
    pub contract: OutputContractKind,
    pub demand: OutputDemandPolicy,
    pub value: OutputValueRef,
}

impl OutputRootPlan {
    pub fn new(
        name: impl Into<String>,
        contract: OutputContractKind,
        demand: OutputDemandPolicy,
        value: OutputValueRef,
    ) -> Result<Self, PlanError> {
        let name = name.into();
        Ok(Self {
            id: OutputRootId::from_name(&name)?,
            name,
            contract,
            demand,
            value,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct MachinePlan {
    pub version: PlanVersion,
    pub target_profile: TargetProfile,
    pub application: ApplicationPlan,
    pub persistence: PersistencePlan,
    pub effects: Vec<EffectContract>,
    pub outputs: Vec<OutputRootPlan>,
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
    pub fn output_root(&self, name: &str) -> Option<&OutputRootPlan> {
        self.outputs.iter().find(|output| output.name == name)
    }

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
pub struct EffectInvocationPlan {
    pub invocation_id: EffectInvocationId,
    pub effect_id: EffectId,
    pub intent_fields: Vec<EffectIntentFieldPlan>,
    pub idempotency_key: EffectIdempotencyKeyPlan,
    pub result: EffectResultRoute,
    pub barrier: EffectBarrier,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct EffectIntentFieldPlan {
    pub name: String,
    pub input: ValueRef,
    pub data_type: DataTypePlan,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EffectIdempotencyKeyPlan {
    InvocationTurnIntentSha256,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EffectResultRoute {
    Target {
        target: ValueRef,
        policy: EffectResultPolicy,
    },
    CorrelatedSources {
        variants: Vec<EffectResultVariantRoute>,
    },
}

impl EffectResultRoute {
    pub fn policy(&self) -> EffectResultPolicy {
        match self {
            Self::Target { policy, .. } => *policy,
            Self::CorrelatedSources { .. } => EffectResultPolicy::CorrelatedSource,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct EffectResultVariantRoute {
    pub tag: String,
    pub source_id: SourceId,
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
        #[serde(default, skip_serializing_if = "Option::is_none")]
        effect: Option<EffectInvocationPlan>,
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
    ValueCompare {
        left: ValueRef,
        op: String,
        right: ValueRef,
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
    ListRowField {
        row: Box<PlanRowExpression>,
        list_id: ListId,
        field: FieldId,
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
    TextToNumber,
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
    HostEffect,
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
    let expected_application_hash = canonical_sha256(&plan.application.identity)?;
    checks.push(PlanCheck {
        id: "application-identity-valid".to_owned(),
        pass: plan.application.identity.is_valid()
            && plan.application.identity_hash == expected_application_hash,
        detail: format!(
            "package `{}`, namespace `{}`, deployment `{}`",
            plan.application.identity.package_id,
            plan.application.identity.state_namespace,
            plan.application.identity.deployment_domain
        ),
    });
    checks.push(PlanCheck {
        id: "persistence-format-supported".to_owned(),
        pass: plan.persistence.format_version == PERSISTENCE_FORMAT_VERSION
            && plan.persistence.schema_version > 0,
        detail: format!(
            "persistence format {}, schema version {}",
            plan.persistence.format_version, plan.persistence.schema_version
        ),
    });
    checks.push(PlanCheck {
        id: "persistence-identities-unique".to_owned(),
        pass: persistence_identities_unique(&plan.persistence),
        detail: format!(
            "{} scalar/indexed memory node(s), {} list memory node(s), {} migration recipe(s), {} migration edge(s)",
            plan.persistence.memory.len(),
            plan.persistence.lists.len(),
            plan.persistence.migration_recipes.len(),
            plan.persistence.migration_edges.len()
        ),
    });
    checks.push(PlanCheck {
        id: "persistence-identities-match-canonical-inputs".to_owned(),
        pass: persistence_identities_match(&plan.persistence)?,
        detail: "memory, leaf, and migration IDs match their readable canonical inputs".to_owned(),
    });
    checks.push(PlanCheck {
        id: "persistence-type-fingerprints-match".to_owned(),
        pass: persistence_type_fingerprints_match(&plan.persistence)?,
        detail: "memory and leaf type fingerprints match canonical recursive schemas".to_owned(),
    });
    checks.push(PlanCheck {
        id: "persistence-runtime-slots-consistent".to_owned(),
        pass: persistence_runtime_slots_consistent(plan),
        detail:
            "active persistence memory maps uniquely to executable slots and authoritative row fields"
                .to_owned(),
    });
    checks.push(PlanCheck {
        id: "list-authority-fields-have-stable-persistence-leaves".to_owned(),
        pass: list_authority_fields_have_persistence_leaves(plan),
        detail: "initial and appended row authority fields resolve to stable list memory leaves"
            .to_owned(),
    });
    checks.push(PlanCheck {
        id: "persistence-ordering-deterministic".to_owned(),
        pass: persistence_ordering_is_deterministic(&plan.persistence),
        detail: "memory, leaf, migration, and recursive type entries use canonical ordering"
            .to_owned(),
    });
    checks.push(PlanCheck {
        id: "migration-recipes-well-formed".to_owned(),
        pass: plan
            .persistence
            .migration_recipes
            .iter()
            .all(|recipe| recipe.validate().is_ok()),
        detail: format!(
            "{} target-neutral migration recipe(s)",
            plan.persistence.migration_recipes.len()
        ),
    });
    checks.push(PlanCheck {
        id: "migration-catalog-well-formed".to_owned(),
        pass: migration_edges_well_formed(&plan.persistence)
            && validate_migration_catalog(&plan.persistence).is_ok(),
        detail: format!(
            "{} predecessor-bound migration edge(s)",
            plan.persistence.migration_edges.len()
        ),
    });
    let expected_schema_hash = persistence_schema_hash(&plan.application, &plan.persistence)?;
    checks.push(PlanCheck {
        id: "persistence-schema-hash-consistent".to_owned(),
        pass: plan.persistence.schema_hash == expected_schema_hash,
        detail: format!(
            "schema hash {}, expected {}",
            digest_hex(&plan.persistence.schema_hash),
            digest_hex(&expected_schema_hash)
        ),
    });
    let expected_recipe_hash = migration_recipe_hash(&plan.persistence)?;
    checks.push(PlanCheck {
        id: "migration-recipe-hash-consistent".to_owned(),
        pass: plan.persistence.migration_recipe_hash == expected_recipe_hash,
        detail: format!(
            "recipe hash {}, expected {}",
            digest_hex(&plan.persistence.migration_recipe_hash),
            digest_hex(&expected_recipe_hash)
        ),
    });
    let expected_catalog_hash = migration_catalog_hash(&plan.persistence)?;
    checks.push(PlanCheck {
        id: "migration-catalog-hash-consistent".to_owned(),
        pass: plan.persistence.migration_catalog_hash == expected_catalog_hash,
        detail: format!(
            "catalog hash {}, expected {}",
            digest_hex(&plan.persistence.migration_catalog_hash),
            digest_hex(&expected_catalog_hash)
        ),
    });
    let effect_contract_failure = effect_contracts_failure(plan);
    checks.push(PlanCheck {
        id: "effect-contracts-canonical-and-safe".to_owned(),
        pass: effect_contract_failure.is_none(),
        detail: effect_contract_failure.unwrap_or_else(|| {
            format!(
                "{} canonical host effect contract(s), {} durable outbox schema(s)",
                plan.effects.len(),
                plan.persistence.effect_outbox.len()
            )
        }),
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
        id: "source-route-row-lookups-use-owned-field-ids".to_owned(),
        pass: source_route_row_lookup_fields_resolve(plan),
        detail: format!(
            "{} scoped row lookup route(s)",
            plan.source_routes
                .iter()
                .filter(|route| route.payload_schema.row_lookup_field.is_some())
                .count()
        ),
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
    let output_root_failure = output_roots_failure(plan);
    checks.push(PlanCheck {
        id: "output-roots-typed-canonical-and-resolved".to_owned(),
        pass: output_root_failure.is_none(),
        detail: output_root_failure
            .unwrap_or_else(|| format!("{} typed host output root(s)", plan.outputs.len())),
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

fn persistence_identities_unique(persistence: &PersistencePlan) -> bool {
    let memory_ids = persistence
        .memory
        .iter()
        .map(|memory| memory.memory_id)
        .chain(persistence.lists.iter().map(|list| list.memory_id))
        .collect::<Vec<_>>();
    let leaf_ids = persistence
        .memory
        .iter()
        .flat_map(|memory| memory.leaves.iter().map(|leaf| leaf.leaf_id))
        .chain(
            persistence
                .lists
                .iter()
                .flat_map(|list| list.row_fields.iter().map(|leaf| leaf.leaf_id)),
        )
        .collect::<Vec<_>>();
    let edge_ids = persistence
        .migration_edges
        .iter()
        .map(|edge| edge.migration_edge_id)
        .collect::<Vec<_>>();
    let recipe_ids = persistence
        .migration_recipes
        .iter()
        .map(|recipe| recipe.migration_recipe_id)
        .collect::<Vec<_>>();
    memory_ids.iter().copied().collect::<BTreeSet<_>>().len() == memory_ids.len()
        && leaf_ids.iter().copied().collect::<BTreeSet<_>>().len() == leaf_ids.len()
        && recipe_ids.iter().copied().collect::<BTreeSet<_>>().len() == recipe_ids.len()
        && edge_ids.iter().copied().collect::<BTreeSet<_>>().len() == edge_ids.len()
}

fn persistence_identities_match(persistence: &PersistencePlan) -> Result<bool, PlanError> {
    for memory in &persistence.memory {
        if memory.memory_id
            != MemoryId::from_identity(&memory.owner, &memory.semantic_path, memory.kind)?
        {
            return Ok(false);
        }
        for leaf in &memory.leaves {
            if leaf.leaf_id
                != MemoryLeafId::from_memory_path(memory.memory_id, &leaf.semantic_path)?
            {
                return Ok(false);
            }
        }
    }
    for list in &persistence.lists {
        if list.memory_id
            != MemoryId::from_identity(&list.owner, &list.semantic_path, MemoryKind::List)?
        {
            return Ok(false);
        }
        for leaf in &list.row_fields {
            if leaf.leaf_id != MemoryLeafId::from_memory_path(list.memory_id, &leaf.semantic_path)?
            {
                return Ok(false);
            }
        }
    }
    for recipe in &persistence.migration_recipes {
        if recipe.migration_recipe_id != MigrationRecipeId::from_transfers(&recipe.transfers)? {
            return Ok(false);
        }
        for transfer in &recipe.transfers {
            for input in &transfer.inputs {
                if input.input_id
                    != MigrationInputId::from_content(&input.leaves, &input.data_type)?
                {
                    return Ok(false);
                }
            }
        }
    }
    for edge in &persistence.migration_edges {
        if edge.migration_edge_id
            != MigrationEdgeId::from_schema_transition(
                edge.source_schema_version,
                edge.target_schema_version,
                edge.source_schema_hash,
                edge.migration_recipe_id,
            )?
        {
            return Ok(false);
        }
    }
    Ok(true)
}

fn persistence_type_fingerprints_match(persistence: &PersistencePlan) -> Result<bool, PlanError> {
    for memory in &persistence.memory {
        if memory.type_fingerprint != data_type_fingerprint(&memory.data_type)? {
            return Ok(false);
        }
        for leaf in &memory.leaves {
            if leaf.type_fingerprint != data_type_fingerprint(&leaf.data_type)? {
                return Ok(false);
            }
        }
    }
    for list in &persistence.lists {
        if list.type_fingerprint != data_type_fingerprint(&list.data_type)? {
            return Ok(false);
        }
        for leaf in &list.row_fields {
            if leaf.type_fingerprint != data_type_fingerprint(&leaf.data_type)? {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

fn list_authority_fields_have_persistence_leaves(plan: &MachinePlan) -> bool {
    plan.persistence.lists.iter().all(|memory| {
        let Some(slot) = plan
            .storage_layout
            .list_slots
            .iter()
            .find(|slot| slot.id == memory.runtime_slot)
        else {
            return false;
        };
        let stable_fields = memory
            .row_fields
            .iter()
            .filter_map(|field| field.runtime_field_id)
            .collect::<BTreeSet<_>>();
        let mut authoritative_fields = slot
            .initial_rows
            .iter()
            .flat_map(|row| &row.fields)
            .filter_map(|field| field.field_id)
            .collect::<BTreeSet<_>>();
        for append in plan
            .regions
            .iter()
            .flat_map(|region| &region.ops)
            .filter_map(|op| {
                let PlanOpKind::ListOperation {
                    append: Some(append),
                    ..
                } = &op.kind
                else {
                    return None;
                };
                (op.output == Some(ValueRef::List(slot.list_id))).then_some(append)
            })
        {
            authoritative_fields.extend(append.fields.iter().filter_map(|field| field.field_id));
        }
        authoritative_fields.is_subset(&stable_fields)
    })
}

fn effect_contracts_failure(plan: &MachinePlan) -> Option<String> {
    if plan
        .effects
        .windows(2)
        .any(|pair| pair[0].effect_id >= pair[1].effect_id)
    {
        return Some("effect contracts are not uniquely ordered by stable effect ID".to_owned());
    }
    for contract in &plan.effects {
        if let Err(error) = contract.validate() {
            return Some(format!(
                "effect contract `{}` is unsafe or malformed: {error}",
                contract.host_operation
            ));
        }
        if let Ok(Some(expected)) = builtin_effect_contract(&contract.host_operation)
            && expected != *contract
        {
            return Some(format!(
                "effect contract `{}` differs from the centralized host contract",
                contract.host_operation
            ));
        }
    }
    if let Err(error) = validate_effect_outbox_schemas(&plan.persistence.effect_outbox) {
        return Some(error.to_string());
    }
    for outbox in &plan.persistence.effect_outbox {
        let Some(contract) = plan
            .effects
            .iter()
            .find(|contract| contract.effect_id == outbox.effect_id)
        else {
            return Some(format!(
                "outbox schema for effect {} has no host contract",
                outbox.effect_id
            ));
        };
        let EffectReplay::Idempotent { key_type } = &contract.replay else {
            return Some(format!(
                "outbox schema for `{}` is not attached to an idempotent effect",
                contract.host_operation
            ));
        };
        if key_type != &outbox.idempotency_key_type {
            return Some(format!(
                "outbox key schema for `{}` differs from its effect contract",
                contract.host_operation
            ));
        }
        if let Ok(Some(expected)) = builtin_effect_outbox_schema(&contract.host_operation) {
            let mut actual_value_schema = outbox.clone();
            actual_value_schema.invocation_ids.clear();
            if expected != actual_value_schema {
                return Some(format!(
                    "outbox schema for `{}` differs from the centralized host schema",
                    contract.host_operation
                ));
            }
        }
    }
    for contract in &plan.effects {
        if matches!(contract.replay, EffectReplay::Idempotent { .. })
            && !plan
                .persistence
                .effect_outbox
                .iter()
                .any(|schema| schema.effect_id == contract.effect_id)
        {
            return Some(format!(
                "idempotent effect `{}` has no durable outbox schema",
                contract.host_operation
            ));
        }
    }
    let available = plan
        .effects
        .iter()
        .map(|contract| contract.host_operation.as_str())
        .collect::<BTreeSet<_>>();
    for required in required_effect_operations(plan) {
        if !available.contains(required) {
            return Some(format!(
                "executable host operation `{required}` has no effect contract"
            ));
        }
    }
    let mut invocation_ids = BTreeSet::new();
    for op in plan.regions.iter().flat_map(|region| &region.ops) {
        let PlanOpKind::UpdateBranch {
            expression_kind,
            ordered_inputs,
            effect,
            ..
        } = &op.kind
        else {
            continue;
        };
        let consequential = matches!(
            expression_kind,
            PlanExpressionKind::FileWriteBytes | PlanExpressionKind::HostEffect
        );
        if consequential != effect.is_some() {
            return Some(format!(
                "effectful update op {} has inconsistent invocation metadata",
                op.id.0
            ));
        }
        let Some(invocation) = effect else {
            continue;
        };
        if !invocation_ids.insert(invocation.invocation_id) {
            return Some(format!(
                "effect invocation {} is used more than once",
                invocation.invocation_id
            ));
        }
        let Some(contract) = plan
            .effects
            .iter()
            .find(|contract| contract.effect_id == invocation.effect_id)
        else {
            return Some(format!(
                "effect invocation {} has no matching host contract",
                invocation.invocation_id
            ));
        };
        let Some(outbox) = plan
            .persistence
            .effect_outbox
            .iter()
            .find(|schema| schema.effect_id == invocation.effect_id)
        else {
            return Some(format!(
                "effect invocation {} has no outbox schema",
                invocation.invocation_id
            ));
        };
        if !outbox.invocation_ids.contains(&invocation.invocation_id) {
            return Some(format!(
                "effect invocation {} is absent from its durable outbox schema",
                invocation.invocation_id
            ));
        }
        let intent_inputs = invocation
            .intent_fields
            .iter()
            .map(|field| field.input.clone())
            .collect::<Vec<_>>();
        if intent_inputs != *ordered_inputs
            || !effect_intent_fields_match_schema(&invocation.intent_fields, &outbox.intent_type)
            || !effect_result_route_matches(plan, op, &invocation.result, &outbox.result_type)
            || invocation.result.policy() != contract.result_policy
            || invocation.barrier != contract.barrier
            || invocation.idempotency_key != EffectIdempotencyKeyPlan::InvocationTurnIntentSha256
        {
            return Some(format!(
                "effect invocation {} disagrees with its update, contract, or outbox schema",
                invocation.invocation_id
            ));
        }
    }
    let bound_invocation_ids = plan
        .persistence
        .effect_outbox
        .iter()
        .flat_map(|schema| schema.invocation_ids.iter().copied())
        .collect::<BTreeSet<_>>();
    if bound_invocation_ids != invocation_ids {
        return Some(
            "durable outbox invocation identities differ from executable effect invocations"
                .to_owned(),
        );
    }
    None
}

fn effect_intent_fields_match_schema(
    fields: &[EffectIntentFieldPlan],
    intent_type: &DataTypePlan,
) -> bool {
    let DataTypePlan::Record {
        fields: schema_fields,
        open: false,
    } = intent_type
    else {
        return false;
    };
    fields.windows(2).all(|pair| pair[0].name < pair[1].name)
        && fields
            .iter()
            .map(|field| (&field.name, &field.data_type))
            .eq(schema_fields
                .iter()
                .map(|field| (&field.name, &field.data_type)))
}

fn effect_result_route_matches(
    plan: &MachinePlan,
    op: &PlanOp,
    route: &EffectResultRoute,
    result_type: &DataTypePlan,
) -> bool {
    match route {
        EffectResultRoute::Target { target, policy } => {
            *policy != EffectResultPolicy::CorrelatedSource && op.output.as_ref() == Some(target)
        }
        EffectResultRoute::CorrelatedSources { variants } => {
            let DataTypePlan::Variant {
                variants: result_variants,
            } = result_type
            else {
                return false;
            };
            op.output.is_none()
                && variants.windows(2).all(|pair| pair[0].tag < pair[1].tag)
                && variants
                    .iter()
                    .map(|route| route.tag.as_str())
                    .eq(result_variants.iter().map(|variant| variant.tag.as_str()))
                && variants
                    .iter()
                    .zip(result_variants)
                    .all(|(route, variant)| {
                        plan.source_routes
                            .iter()
                            .find(|source| source.source_id == route.source_id)
                            .is_some_and(|source| source_payload_matches_variant(source, variant))
                    })
        }
    }
}

fn source_payload_matches_variant(source: &SourceRoute, variant: &DataVariantPlan) -> bool {
    let expected = variant
        .fields
        .iter()
        .map(|field| {
            Some((
                source_payload_field_from_schema_name(&field.name),
                source_payload_type_from_data_type(&field.data_type)?,
            ))
        })
        .collect::<Option<BTreeMap<_, _>>>();
    let Some(expected) = expected else {
        return false;
    };
    let actual = source
        .payload_schema
        .typed_fields
        .iter()
        .map(|field| (field.field.clone(), field.value_type))
        .collect::<BTreeMap<_, _>>();
    actual == expected
        && source
            .payload_schema
            .fields
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>()
            == expected.keys().cloned().collect()
}

fn source_route_row_lookup_fields_resolve(plan: &MachinePlan) -> bool {
    plan.source_routes.iter().all(|route| {
        match (
            route.payload_schema.row_lookup_field.as_ref(),
            route.payload_schema.row_lookup_field_id,
        ) {
            (None, None) => true,
            (Some(_), Some(field_id)) => {
                let Some(scope_id) = route.scope_id else {
                    return false;
                };
                plan.storage_layout.list_slots.iter().any(|slot| {
                    slot.scope_id == Some(scope_id) && slot.row_field_ids.contains(&field_id)
                })
            }
            (None, Some(_)) | (Some(_), None) => false,
        }
    })
}

fn source_payload_field_from_schema_name(name: &str) -> SourcePayloadField {
    match name {
        "address" => SourcePayloadField::Address,
        "bytes" => SourcePayloadField::Bytes,
        "key" => SourcePayloadField::Key,
        "text" => SourcePayloadField::Text,
        _ => SourcePayloadField::Named(name.to_owned()),
    }
}

fn source_payload_type_from_data_type(data_type: &DataTypePlan) -> Option<SourcePayloadValueType> {
    match data_type {
        DataTypePlan::Bytes { .. } => Some(SourcePayloadValueType::Bytes),
        DataTypePlan::Bool => Some(SourcePayloadValueType::Bool),
        DataTypePlan::Number | DataTypePlan::Byte => Some(SourcePayloadValueType::Number),
        DataTypePlan::Text => Some(SourcePayloadValueType::Text),
        _ => None,
    }
}

fn required_effect_operations(plan: &MachinePlan) -> BTreeSet<&'static str> {
    let mut operations = BTreeSet::new();
    for op in plan.regions.iter().flat_map(|region| &region.ops) {
        match &op.kind {
            PlanOpKind::UpdateBranch {
                expression_kind: PlanExpressionKind::FileReadBytes,
                ..
            } => {
                operations.insert("File/read_bytes");
            }
            PlanOpKind::UpdateBranch {
                expression_kind: PlanExpressionKind::FileWriteBytes,
                ..
            } => {
                operations.insert("File/write_bytes");
            }
            _ => {}
        }
    }
    if let Some(document) = &plan.document {
        for expression in &document.expressions {
            let DocumentExprOp::Builtin { builtin, .. } = expression.op else {
                continue;
            };
            let operation = match builtin {
                DocumentBuiltin::DirectoryEntries => Some("Directory/entries"),
                DocumentBuiltin::FileReadBytes => Some("File/read_bytes"),
                _ => None,
            };
            operations.extend(operation);
        }
    }
    operations
}

fn output_roots_failure(plan: &MachinePlan) -> Option<String> {
    if plan
        .outputs
        .windows(2)
        .any(|pair| pair[0].name >= pair[1].name)
    {
        return Some("output roots are not uniquely ordered by name".to_owned());
    }
    for output in &plan.outputs {
        if output.name.trim().is_empty() || output.name.trim() != output.name {
            return Some("output root names must be non-empty and canonical".to_owned());
        }
        if output.id != OutputRootId::from_name(&output.name).ok()? {
            return Some(format!(
                "output root `{}` has a non-canonical stable identity",
                output.name
            ));
        }
        match (&output.contract, &output.value) {
            (
                OutputContractKind::Document | OutputContractKind::Scene,
                OutputValueRef::RetainedVisual { expression },
            ) => {
                let Some(document) = &plan.document else {
                    return Some(format!(
                        "retained visual output root `{}` references an absent document plan",
                        output.name
                    ));
                };
                let expected_contract = match document.root.kind {
                    DocumentRootKind::Document => OutputContractKind::Document,
                    DocumentRootKind::Scene => OutputContractKind::Scene,
                };
                if output.contract != expected_contract
                    || *expression != document.root.expression
                    || document.expressions.get(expression.0).is_none()
                {
                    return Some(format!(
                        "retained visual output root `{}` does not resolve to its document plan value",
                        output.name
                    ));
                }
            }
            (
                OutputContractKind::HostValue { data_type },
                OutputValueRef::RuntimeValue { value },
            ) => {
                if !data_type.is_canonical() || !data_type_is_closed(data_type) {
                    return Some(format!(
                        "host output root `{}` does not have a closed canonical data type",
                        output.name
                    ));
                }
                if !runtime_output_ref_resolves(plan, value) {
                    return Some(format!(
                        "host output root `{}` does not resolve to an executable runtime value",
                        output.name
                    ));
                }
            }
            _ => {
                return Some(format!(
                    "output root `{}` mixes retained-visual and host-value contracts",
                    output.name
                ));
            }
        }
    }
    let retained_count = plan
        .outputs
        .iter()
        .filter(|output| {
            matches!(
                output.contract,
                OutputContractKind::Document | OutputContractKind::Scene
            )
        })
        .count();
    match (&plan.document, retained_count) {
        (None, 0) | (Some(_), 1) => None,
        (None, _) => {
            Some("output registry has a retained visual root without a document plan".to_owned())
        }
        (Some(_), count) => Some(format!(
            "document plan must have exactly one retained visual output root, found {count}"
        )),
    }
}

fn data_type_is_closed(data_type: &DataTypePlan) -> bool {
    match data_type {
        DataTypePlan::Unknown => false,
        DataTypePlan::Record { fields, open } | DataTypePlan::Error { fields, open } => {
            !open
                && fields
                    .iter()
                    .all(|field| data_type_is_closed(&field.data_type))
        }
        DataTypePlan::Variant { variants } => variants.iter().all(|variant| {
            !variant.open
                && variant
                    .fields
                    .iter()
                    .all(|field| data_type_is_closed(&field.data_type))
        }),
        DataTypePlan::List { item } => data_type_is_closed(item),
        DataTypePlan::Null
        | DataTypePlan::Bool
        | DataTypePlan::Number
        | DataTypePlan::Byte
        | DataTypePlan::Text
        | DataTypePlan::Bytes { .. } => true,
    }
}

fn runtime_output_ref_resolves(plan: &MachinePlan, value: &ValueRef) -> bool {
    match value {
        ValueRef::State(state) => plan
            .storage_layout
            .scalar_slots
            .iter()
            .any(|slot| slot.state_id == *state && !slot.indexed),
        ValueRef::Field(field) => plan
            .regions
            .iter()
            .flat_map(|region| &region.ops)
            .any(|op| !op.indexed && op.output.as_ref() == Some(&ValueRef::Field(*field))),
        ValueRef::List(list) => plan
            .storage_layout
            .list_slots
            .iter()
            .any(|slot| slot.list_id == *list),
        ValueRef::Constant(constant) => plan.constants.iter().any(|item| item.id == *constant),
        ValueRef::Source(_) | ValueRef::SourcePayload { .. } => false,
    }
}

fn persistence_runtime_slots_consistent(plan: &MachinePlan) -> bool {
    let mut linked_slots = BTreeSet::new();
    for memory in &plan.persistence.memory {
        let Some(slot) = plan
            .storage_layout
            .scalar_slots
            .iter()
            .find(|slot| slot.id == memory.runtime_slot)
        else {
            return false;
        };
        let expected_kind = if slot.indexed {
            MemoryKind::IndexedField
        } else {
            MemoryKind::Scalar
        };
        if memory.kind != expected_kind
            || memory.leaves.is_empty()
            || memory
                .leaves
                .iter()
                .any(|leaf| leaf.runtime_field_id.is_some())
            || !linked_slots.insert(memory.runtime_slot)
        {
            return false;
        }
    }

    for list in &plan.persistence.lists {
        let Some(slot) = plan
            .storage_layout
            .list_slots
            .iter()
            .find(|slot| slot.id == list.runtime_slot)
        else {
            return false;
        };
        let runtime_fields = list
            .row_fields
            .iter()
            .filter_map(|leaf| leaf.runtime_field_id)
            .collect::<BTreeSet<_>>();
        let expected_fields = slot.row_field_ids.iter().copied().collect::<BTreeSet<_>>();
        if runtime_fields.len() != list.row_fields.len()
            || !runtime_fields.is_subset(&expected_fields)
            || !linked_slots.insert(list.runtime_slot)
        {
            return false;
        }
    }

    linked_slots.len() == plan.persistence.memory.len() + plan.persistence.lists.len()
}

fn persistence_ordering_is_deterministic(persistence: &PersistencePlan) -> bool {
    persistence
        .memory
        .windows(2)
        .all(|pair| pair[0].memory_id < pair[1].memory_id)
        && persistence.memory.iter().all(|memory| {
            memory.data_type.is_canonical()
                && memory
                    .leaves
                    .windows(2)
                    .all(|pair| pair[0].leaf_id < pair[1].leaf_id)
                && memory
                    .leaves
                    .iter()
                    .all(|leaf| leaf.data_type.is_canonical())
        })
        && persistence
            .lists
            .windows(2)
            .all(|pair| pair[0].memory_id < pair[1].memory_id)
        && persistence.lists.iter().all(|list| {
            list.data_type.is_canonical()
                && list
                    .row_fields
                    .windows(2)
                    .all(|pair| pair[0].leaf_id < pair[1].leaf_id)
                && list
                    .row_fields
                    .iter()
                    .all(|leaf| leaf.data_type.is_canonical())
        })
        && persistence
            .effect_outbox
            .windows(2)
            .all(|pair| pair[0].effect_id < pair[1].effect_id)
        && persistence
            .effect_outbox
            .iter()
            .all(|schema| schema.validate().is_ok())
        && persistence
            .migration_recipes
            .windows(2)
            .all(|pair| pair[0].migration_recipe_id < pair[1].migration_recipe_id)
        && persistence.migration_recipes.iter().all(|recipe| {
            recipe.validate().is_ok()
                && recipe.transfers.iter().all(|transfer| {
                    transfer
                        .inputs
                        .windows(2)
                        .all(|pair| pair[0].input_id < pair[1].input_id)
                })
        })
        && persistence
            .migration_edges
            .windows(2)
            .all(|pair| pair[0].migration_edge_id < pair[1].migration_edge_id)
}

fn migration_edges_well_formed(persistence: &PersistencePlan) -> bool {
    persistence.migration_edges.iter().all(|edge| {
        edge.source_schema_version > 0
            && edge.source_schema_version < edge.target_schema_version
            && edge.target_schema_version <= persistence.schema_version
            && persistence
                .migration_recipes
                .iter()
                .any(|recipe| recipe.migration_recipe_id == edge.migration_recipe_id)
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

pub fn plan_binary(plan: &MachinePlan) -> Result<Vec<u8>, PlanError> {
    binary::encode(plan)
}

pub fn plan_sha256(plan: &MachinePlan) -> Result<String, PlanError> {
    let bytes = plan_binary(plan)?;
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    Ok(format!("{:x}", hasher.finalize()))
}

pub fn data_type_fingerprint(data_type: &DataTypePlan) -> Result<[u8; 32], PlanError> {
    canonical_sha256(&data_type.canonicalized())
}

#[derive(Serialize)]
struct CanonicalMemoryLeafSchema {
    leaf_id: MemoryLeafId,
    semantic_path: String,
    data_type: DataTypePlan,
    type_fingerprint: [u8; 32],
}

impl CanonicalMemoryLeafSchema {
    fn from_leaf(leaf: &MemoryLeafPlan) -> Self {
        Self {
            leaf_id: leaf.leaf_id,
            semantic_path: leaf.semantic_path.clone(),
            data_type: leaf.data_type.canonicalized(),
            type_fingerprint: leaf.type_fingerprint,
        }
    }
}

#[derive(Serialize)]
struct CanonicalMemorySchema {
    memory_id: MemoryId,
    kind: MemoryKind,
    semantic_path: String,
    data_type: DataTypePlan,
    type_fingerprint: [u8; 32],
    initial_provenance: InitialProvenance,
    owner: MemoryOwnerPath,
    leaves: Vec<CanonicalMemoryLeafSchema>,
}

impl CanonicalMemorySchema {
    fn from_memory(memory: &MemoryPlan) -> Self {
        let mut leaves = memory
            .leaves
            .iter()
            .map(CanonicalMemoryLeafSchema::from_leaf)
            .collect::<Vec<_>>();
        leaves.sort_by_key(|leaf| leaf.leaf_id);
        Self {
            memory_id: memory.memory_id,
            kind: memory.kind,
            semantic_path: memory.semantic_path.clone(),
            data_type: memory.data_type.canonicalized(),
            type_fingerprint: memory.type_fingerprint,
            initial_provenance: memory.initial_provenance,
            owner: memory.owner.clone(),
            leaves,
        }
    }
}

#[derive(Serialize)]
struct CanonicalListMemorySchema {
    memory_id: MemoryId,
    semantic_path: String,
    data_type: DataTypePlan,
    type_fingerprint: [u8; 32],
    initial_provenance: InitialProvenance,
    owner: MemoryOwnerPath,
    hidden_key_type: String,
    has_generation: bool,
    row_fields: Vec<CanonicalMemoryLeafSchema>,
}

impl CanonicalListMemorySchema {
    fn from_list(list: &ListMemoryPlan) -> Self {
        let mut row_fields = list
            .row_fields
            .iter()
            .map(CanonicalMemoryLeafSchema::from_leaf)
            .collect::<Vec<_>>();
        row_fields.sort_by_key(|field| field.leaf_id);
        Self {
            memory_id: list.memory_id,
            semantic_path: list.semantic_path.clone(),
            data_type: list.data_type.canonicalized(),
            type_fingerprint: list.type_fingerprint,
            initial_provenance: list.initial_provenance,
            owner: list.owner.clone(),
            hidden_key_type: list.hidden_key_type.clone(),
            has_generation: list.has_generation,
            row_fields,
        }
    }
}

#[derive(Serialize)]
struct CanonicalPersistenceSchema<'a> {
    application: &'a ApplicationIdentity,
    format_version: u32,
    schema_version: u64,
    memory: Vec<CanonicalMemorySchema>,
    lists: Vec<CanonicalListMemorySchema>,
    effect_outbox: &'a [EffectOutboxSchema],
}

/// Hashes only canonical persistence schema data. Runtime numeric slots and
/// fields are intentionally omitted because they are executable-plan links,
/// not durable identity.
pub fn persistence_schema_hash(
    application: &ApplicationPlan,
    persistence: &PersistencePlan,
) -> Result<[u8; 32], PlanError> {
    let mut memory = persistence
        .memory
        .iter()
        .map(CanonicalMemorySchema::from_memory)
        .collect::<Vec<_>>();
    memory.sort_by_key(|memory| memory.memory_id);
    let mut lists = persistence
        .lists
        .iter()
        .map(CanonicalListMemorySchema::from_list)
        .collect::<Vec<_>>();
    lists.sort_by_key(|list| list.memory_id);
    canonical_sha256(&CanonicalPersistenceSchema {
        application: &application.identity,
        format_version: persistence.format_version,
        schema_version: persistence.schema_version,
        memory,
        lists,
        effect_outbox: &persistence.effect_outbox,
    })
}

#[derive(Serialize)]
struct CanonicalCurrentMigrationRecipe<'a> {
    current_migration_recipe_id: Option<MigrationRecipeId>,
    current_recipe: Option<&'a MigrationRecipePlan>,
}

pub fn migration_recipe_hash(persistence: &PersistencePlan) -> Result<[u8; 32], PlanError> {
    let current_recipe = persistence.current_migration_recipe_id.and_then(|current| {
        persistence
            .migration_recipes
            .iter()
            .find(|recipe| recipe.migration_recipe_id == current)
    });
    canonical_sha256(&CanonicalCurrentMigrationRecipe {
        current_migration_recipe_id: persistence.current_migration_recipe_id,
        current_recipe,
    })
}

#[derive(Serialize)]
struct CanonicalMigrationCatalog<'a> {
    recipes: &'a [MigrationRecipePlan],
    edges: &'a [MigrationEdgePlan],
}

pub fn migration_catalog_hash(persistence: &PersistencePlan) -> Result<[u8; 32], PlanError> {
    canonical_sha256(&CanonicalMigrationCatalog {
        recipes: &persistence.migration_recipes,
        edges: &persistence.migration_edges,
    })
}

pub fn canonical_persistence_schema_hash(
    application: &ApplicationPlan,
    persistence: &PersistencePlan,
) -> Result<[u8; 32], PlanError> {
    persistence_schema_hash(application, persistence)
}

fn canonical_sha256<T>(value: &T) -> Result<[u8; 32], PlanError>
where
    T: Serialize,
{
    let mut hasher = Sha256::new();
    hasher.update(binary::encode(value)?);
    Ok(hasher.finalize().into())
}

fn digest_hex(digest: &[u8; 32]) -> String {
    let mut output = String::with_capacity(64);
    for byte in digest {
        use fmt::Write as _;
        let _ = write!(output, "{byte:02x}");
    }
    output
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
            ordered_inputs,
            source_payload_field,
            update_constant_id,
            source_guard,
            effect,
        } => {
            if op.unresolved_executable_ref_count != 0 {
                return false;
            }
            if *expression_kind == PlanExpressionKind::HostEffect {
                return effect.is_some()
                    && op.output.is_none()
                    && source_payload_field.is_none()
                    && update_constant_id.is_none()
                    && source_guard.is_none()
                    && update_branch_source_ids(op).len() == 1
                    && !ordered_inputs.is_empty();
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
                PlanExpressionKind::TextToNumber => {
                    update_constant_id.is_none()
                        && output_state_type_is(scalar_slots, op, &PlanValueType::Number)
                        && update_branch_source_ids(op).len() == 1
                        && text_to_number_inputs_are_supported(
                            scalar_slots,
                            op,
                            source_payload_field,
                        )
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
                PlanExpressionKind::MatchTextIsEmptyConst => {
                    source_payload_field.is_none()
                        && update_constant_id.is_none()
                        && update_branch_source_ids(op).len() == 1
                        && root_match_text_is_empty_const_inputs_supported(
                            scalar_slots,
                            constants,
                            op,
                        )
                }
                PlanExpressionKind::NumberInfix => {
                    source_payload_field.is_none()
                        && update_constant_id.is_none()
                        && output_state_type_is(scalar_slots, op, &PlanValueType::Number)
                        && update_branch_source_ids(op).len() == 1
                        && root_number_infix_inputs_supported(scalar_slots, constants, op)
                }
                PlanExpressionKind::ProjectTime
                | PlanExpressionKind::MatchInfixConst
                | PlanExpressionKind::ListFindValue
                | PlanExpressionKind::HostEffect
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
        PlanDerivedExpression::ValueCompare {
            left,
            op: op_name,
            right,
        } => {
            matches!(op_name.as_str(), ">" | ">=" | "<" | "<=" | "==" | "!=")
                && op.inputs.contains(left)
                && op.inputs.contains(right)
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

#[allow(clippy::too_many_arguments)]
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
        PlanExpressionKind::TextToNumber => {
            update_constant_id.is_none()
                && output_state_type_is(scalar_slots, op, &PlanValueType::Number)
                && indexed_text_to_number_inputs_are_supported(
                    scalar_slots,
                    list_slots,
                    op,
                    source_payload_field,
                )
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
        | PlanExpressionKind::HostEffect
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

fn text_to_number_inputs_are_supported(
    scalar_slots: &[ScalarStorageSlot],
    op: &PlanOp,
    source_payload_field: &Option<SourcePayloadField>,
) -> bool {
    let [input] = update_branch_ordered_inputs(op) else {
        return false;
    };
    if !op.inputs.contains(input) {
        return false;
    }
    match (source_payload_field, input) {
        (Some(expected), ValueRef::SourcePayload { field: actual, .. }) => {
            expected == actual
                && source_payload_input_matches_single_source(op, expected)
                && state_input_ids(op).is_empty()
        }
        (None, ValueRef::State(state_id)) => {
            source_payload_input_ids(op).is_empty()
                && state_input_ids(op).as_slice() == [*state_id]
                && plan_value_type_for_state_slots(scalar_slots, *state_id)
                    == Some(&PlanValueType::Text)
        }
        _ => false,
    }
}

fn indexed_text_to_number_inputs_are_supported(
    scalar_slots: &[ScalarStorageSlot],
    list_slots: &[ListStorageSlot],
    op: &PlanOp,
    source_payload_field: &Option<SourcePayloadField>,
) -> bool {
    if source_payload_field.is_some() {
        return text_to_number_inputs_are_supported(scalar_slots, op, source_payload_field);
    }
    let [input] = update_branch_ordered_inputs(op) else {
        return false;
    };
    source_payload_input_ids(op).is_empty()
        && indexed_text_operand_is_supported(scalar_slots, list_slots, op, input)
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

fn source_payload_refs_are_declared_as(
    plan: &MachinePlan,
    op: &PlanOp,
    expected_field: &SourcePayloadField,
    expected_type: SourcePayloadValueType,
) -> bool {
    let payload_inputs = source_payload_input_ids(op);
    if payload_inputs.is_empty() {
        return false;
    }
    payload_inputs.into_iter().all(|(source_id, field)| {
        field == *expected_field
            && plan
                .source_routes
                .iter()
                .find(|route| route.source_id == source_id)
                .and_then(|route| {
                    route
                        .payload_schema
                        .typed_fields
                        .iter()
                        .find(|descriptor| descriptor.field == field)
                })
                .is_some_and(|descriptor| descriptor.value_type == expected_type)
    })
}

fn text_to_number_op_is_well_formed(
    plan: &MachinePlan,
    op: &PlanOp,
    source_payload_field: &Option<SourcePayloadField>,
    update_constant_id: &Option<PlanConstantId>,
) -> bool {
    if update_constant_id.is_some()
        || output_state_type(&plan.storage_layout.scalar_slots, op) != Some(&PlanValueType::Number)
    {
        return false;
    }
    let [input] = update_branch_ordered_inputs(op) else {
        return false;
    };
    if !op.inputs.contains(input) {
        return false;
    }
    match (source_payload_field, input) {
        (Some(expected), ValueRef::SourcePayload { field: actual, .. }) => {
            expected == actual
                && source_payload_input_matches_single_source(op, expected)
                && source_payload_refs_are_declared_as(
                    plan,
                    op,
                    expected,
                    SourcePayloadValueType::Text,
                )
                && state_input_ids(op).is_empty()
        }
        (None, ValueRef::State(state_id)) => {
            source_payload_input_ids(op).is_empty()
                && state_input_ids(op).as_slice() == [*state_id]
                && plan_value_type_for_state(plan, *state_id) == Some(&PlanValueType::Text)
        }
        (None, ValueRef::Field(field_id)) => {
            op.indexed
                && source_payload_input_ids(op).is_empty()
                && indexed_output_scope_owns_row_field(plan, op, *field_id)
                && plan_field_initial_value_matches_type(plan, *field_id, &PlanValueType::Text)
        }
        _ => false,
    }
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
        SourcePayloadValueType::Number => plan_type == &PlanValueType::Number,
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
    if !match_const_input_ref_supported(scalar_slots, op, input) {
        return false;
    }
    root_encoded_match_arms_supported(scalar_slots, constants, op, output_type, arm_operands)
}

fn root_match_text_is_empty_const_inputs_supported(
    scalar_slots: &[ScalarStorageSlot],
    constants: &[PlanConstant],
    op: &PlanOp,
) -> bool {
    let Some(output_type) = output_state_type(scalar_slots, op) else {
        return false;
    };
    let PlanOpKind::UpdateBranch { ordered_inputs, .. } = &op.kind else {
        return false;
    };
    let [input, arm_operands @ ..] = ordered_inputs.as_slice() else {
        return false;
    };
    text_operand_ref_supported(scalar_slots, constants, input, true)
        && root_encoded_match_arms_supported(scalar_slots, constants, op, output_type, arm_operands)
}

fn root_encoded_match_arms_supported(
    scalar_slots: &[ScalarStorageSlot],
    constants: &[PlanConstant],
    op: &PlanOp,
    output_type: &PlanValueType,
    inputs: &[ValueRef],
) -> bool {
    let mut cursor = 0usize;
    let mut arm_count = 0usize;
    while cursor < inputs.len() {
        if !match_const_pattern_ref_supported(constants, &inputs[cursor]) {
            return false;
        }
        cursor += 1;
        if !root_encoded_update_supported(
            scalar_slots,
            constants,
            op,
            output_type,
            inputs,
            &mut cursor,
        ) {
            return false;
        }
        arm_count += 1;
    }
    arm_count > 0
}

fn root_encoded_update_supported(
    scalar_slots: &[ScalarStorageSlot],
    constants: &[PlanConstant],
    op: &PlanOp,
    output_type: &PlanValueType,
    inputs: &[ValueRef],
    cursor: &mut usize,
) -> bool {
    let Some(ValueRef::Constant(tag_id)) = inputs.get(*cursor) else {
        return false;
    };
    let Some(tag) = plan_text_constant_value(constants, *tag_id) else {
        return false;
    };
    *cursor += 1;
    match tag {
        "ref" => {
            let Some(value) = inputs.get(*cursor) else {
                return false;
            };
            *cursor += 1;
            root_update_value_ref_supported(scalar_slots, constants, op, output_type, value)
        }
        "number_infix" => {
            let Some(operands) = inputs.get(*cursor..*cursor + 3) else {
                return false;
            };
            *cursor += 3;
            output_type == &PlanValueType::Number
                && root_number_operand_supported(scalar_slots, constants, op, &operands[0])
                && plan_text_constant_value_ref(constants, &operands[1])
                    .is_some_and(|operator| matches!(operator, "+" | "-" | "*" | "/" | "%"))
                && root_number_operand_supported(scalar_slots, constants, op, &operands[2])
        }
        "match_const" => root_encoded_nested_match_supported(
            scalar_slots,
            constants,
            op,
            output_type,
            inputs,
            cursor,
            false,
        ),
        "match_text_is_empty_const" => root_encoded_nested_match_supported(
            scalar_slots,
            constants,
            op,
            output_type,
            inputs,
            cursor,
            false,
        ),
        "match_infix_const" => root_encoded_nested_match_supported(
            scalar_slots,
            constants,
            op,
            output_type,
            inputs,
            cursor,
            true,
        ),
        _ => false,
    }
}

fn root_encoded_nested_match_supported(
    scalar_slots: &[ScalarStorageSlot],
    constants: &[PlanConstant],
    op: &PlanOp,
    output_type: &PlanValueType,
    inputs: &[ValueRef],
    cursor: &mut usize,
    infix: bool,
) -> bool {
    let header_len = if infix { 4 } else { 2 };
    let Some(header) = inputs.get(*cursor..*cursor + header_len) else {
        return false;
    };
    let valid_header = if infix {
        root_number_operand_supported(scalar_slots, constants, op, &header[0])
            && plan_text_constant_value_ref(constants, &header[1])
                .is_some_and(|operator| matches!(operator, "==" | "!=" | ">" | ">=" | "<" | "<="))
            && root_number_operand_supported(scalar_slots, constants, op, &header[2])
    } else {
        match_const_input_ref_supported(scalar_slots, op, &header[0])
    };
    let count_ref = &header[header_len - 1];
    let ValueRef::Constant(count_id) = count_ref else {
        return false;
    };
    let Some(arm_count) = plan_number_constant_u64(constants, *count_id)
        .and_then(|count| usize::try_from(count).ok())
    else {
        return false;
    };
    if !valid_header || arm_count == 0 {
        return false;
    }
    *cursor += header_len;
    for _ in 0..arm_count {
        let Some(pattern) = inputs.get(*cursor) else {
            return false;
        };
        if !match_const_pattern_ref_supported(constants, pattern) {
            return false;
        }
        *cursor += 1;
        if !root_encoded_update_supported(scalar_slots, constants, op, output_type, inputs, cursor)
        {
            return false;
        }
    }
    true
}

fn root_number_operand_supported(
    scalar_slots: &[ScalarStorageSlot],
    constants: &[PlanConstant],
    op: &PlanOp,
    value: &ValueRef,
) -> bool {
    match value {
        ValueRef::State(state) => {
            op.inputs.contains(value)
                && plan_value_type_for_state_slots(scalar_slots, *state)
                    == Some(&PlanValueType::Number)
        }
        ValueRef::Constant(id) => plan_constant_by_id(constants, *id)
            .is_some_and(|constant| matches!(constant.value, PlanConstantValue::Number { .. })),
        ValueRef::Field(_) | ValueRef::SourcePayload { .. } => op.inputs.contains(value),
        ValueRef::Source(_) | ValueRef::List(_) => false,
    }
}

fn plan_text_constant_value_ref<'a>(
    constants: &'a [PlanConstant],
    value: &ValueRef,
) -> Option<&'a str> {
    let ValueRef::Constant(id) = value else {
        return None;
    };
    plan_text_constant_value(constants, *id)
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

#[allow(clippy::single_match)]
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
                PlanExpressionKind::TextToNumber => {
                    if !text_to_number_op_is_well_formed(
                        plan,
                        op,
                        source_payload_field,
                        update_constant_id,
                    ) {
                        return Some(format!(
                            "TextToNumber update op {} requires one declared TEXT input and a NUMBER output",
                            op.id.0
                        ));
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
                    ) && offset.checked_add(byte_count).is_none_or(|end| end > *len)
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
                PlanExpressionKind::FileWriteBytes
                    if !file_write_bytes_op_is_well_formed(plan, op) =>
                {
                    return Some(constant_ref_update_op_failure(*expression_kind, op));
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
                (slot
                    .initial_row_field_path
                    .as_deref()
                    .is_some_and(|path| !path.trim().is_empty())
                    || slot.initial_row_expression.is_some())
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
                PlanDerivedExpression::ValueCompare { left, right, .. } => {
                    op.inputs.contains(left) && op.inputs.contains(right)
                }
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
        PlanDerivedExpression::ValueCompare { left, right, .. } => {
            op.inputs.contains(left) && op.inputs.contains(right)
        }
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
        PlanRowExpression::ListRowField { row, list_id, .. } => {
            op.inputs.contains(&ValueRef::List(*list_id)) && row_expression_refs_resolve(op, row)
        }
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
        PlanRowExpression::ListRowField {
            row,
            list_id,
            field,
        } => {
            list_has_row_field(plan, *list_id, *field)
                && row_expression_list_fields_resolve_inner(plan, row)
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
        PlanRowExpression::ListRowField { row, .. } => row_expression_cpu_evaluable(row),
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
        PlanRowExpression::ListRowField { .. } => false,
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
