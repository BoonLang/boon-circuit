use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt;

mod binary;
mod document;
mod host;

pub use boon_data::{FiniteReal, FiniteRealError};
pub use boon_document_model::{
    ListId, OwnerInstanceId, OwnerInstanceRow, PlanStaticOwnerId, ProgramRole, SourceId,
    SourceRouteToken,
};
pub use document::*;
pub use host::*;

pub const PLAN_MAJOR_VERSION: u32 = 6;
pub const PLAN_MINOR_VERSION: u32 = 0;
pub const PERSISTENCE_FORMAT_VERSION: u32 = 2;
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

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetProfile {
    SoftwareDefault,
    SoftwareBounded,
    FpgaTodomvc,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TypedListIndexResourceLimits {
    pub max_indexes: usize,
    pub max_indexes_per_list: usize,
    pub max_key_components: usize,
    pub max_entries_per_index: u64,
    pub max_encoded_key_bytes: u64,
    pub max_total_payload_bytes: u64,
    pub max_affected_indexes_per_mutation: usize,
    pub max_startup_rebuild_entries: u64,
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

    /// Hard generated-index limits owned by the target profile. Static plan
    /// validation uses inventory and known-capacity limits; the executor also
    /// applies the concrete key/payload limits to restored and live values.
    pub const fn typed_list_index_limits(self) -> TypedListIndexResourceLimits {
        match self {
            Self::SoftwareDefault => TypedListIndexResourceLimits {
                max_indexes: 256,
                max_indexes_per_list: 16,
                max_key_components: 8,
                max_entries_per_index: 1_000_000,
                max_encoded_key_bytes: 16 * 1024,
                max_total_payload_bytes: 512 * 1024 * 1024,
                max_affected_indexes_per_mutation: 16,
                max_startup_rebuild_entries: 4_000_000,
            },
            Self::SoftwareBounded => TypedListIndexResourceLimits {
                max_indexes: 64,
                max_indexes_per_list: 8,
                max_key_components: 8,
                max_entries_per_index: 100_000,
                max_encoded_key_bytes: 4 * 1024,
                max_total_payload_bytes: 64 * 1024 * 1024,
                max_affected_indexes_per_mutation: 8,
                max_startup_rebuild_entries: 500_000,
            },
            Self::FpgaTodomvc => TypedListIndexResourceLimits {
                max_indexes: 0,
                max_indexes_per_list: 0,
                max_key_components: 0,
                max_entries_per_index: 0,
                max_encoded_key_bytes: 0,
                max_total_payload_bytes: 0,
                max_affected_indexes_per_mutation: 0,
                max_startup_rebuild_entries: 0,
            },
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
    StateId,
    FieldId,
    ScopeId,
    PlanLocalId,
    PlanListIndexId,
    PlanRowExpressionId,
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
    DistributedGraphId,
    DistributedEndpointId,
    DistributedDeclarationId,
    ExportId,
    ImportId,
    DistributedArgumentId,
    RemoteCallSiteId,
    MemoryId,
    MemoryLeafId,
    MigrationInputId,
    MigrationRecipeId,
    MigrationEdgeId
);

/// Opaque correlation identity for one distributed call instance.
///
/// The bytes remain serializable at trusted plan/runtime boundaries, but must
/// never appear in diagnostics, reports, or application-visible data.
#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DistributedCallInstanceId(pub [u8; 32]);

impl DistributedCallInstanceId {
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl fmt::Debug for DistributedCallInstanceId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("DistributedCallInstanceId(..)")
    }
}

impl fmt::Display for DistributedCallInstanceId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self, formatter)
    }
}

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

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DistributedExportKind {
    Value,
    Event,
    Function,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DistributedRouteScopePlan {
    SessionLocal,
    OriginScoped,
    SharedSubscription,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DistributedGraphIdentityPlan {
    pub graph_id: DistributedGraphId,
    pub stable_identity: DistributedDeclarationId,
    pub revision: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DistributedValueExportPlan {
    pub export_id: ExportId,
    pub stable_identity: DistributedDeclarationId,
    pub revision: u64,
    pub producer_role: ProgramRole,
    pub origin_scoped: bool,
    pub value: ValueRef,
    pub data_type: DataTypePlan,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DistributedValueImportPlan {
    pub import_id: ImportId,
    pub stable_identity: DistributedDeclarationId,
    pub revision: u64,
    pub consumer_role: ProgramRole,
    pub producer_role: ProgramRole,
    pub scope: DistributedRouteScopePlan,
    pub source_export_id: ExportId,
    pub source_revision: u64,
    pub source_origin_scoped: bool,
    pub data_type: DataTypePlan,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DistributedEventExportPlan {
    pub export_id: ExportId,
    pub stable_identity: DistributedDeclarationId,
    pub revision: u64,
    pub producer_role: ProgramRole,
    pub source_id: SourceId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload_field: Option<SourcePayloadField>,
    pub payload_type: DataTypePlan,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DistributedEventImportPlan {
    pub import_id: ImportId,
    pub stable_identity: DistributedDeclarationId,
    pub revision: u64,
    pub consumer_role: ProgramRole,
    pub producer_role: ProgramRole,
    pub scope: DistributedRouteScopePlan,
    pub source_export_id: ExportId,
    pub source_revision: u64,
    pub local_source_id: SourceId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload_field: Option<SourcePayloadField>,
    pub payload_type: DataTypePlan,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DistributedFunctionParameterPlan {
    pub argument_id: DistributedArgumentId,
    pub name: String,
    pub data_type: DataTypePlan,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DistributedFunctionExportPlan {
    pub export_id: ExportId,
    pub stable_identity: DistributedDeclarationId,
    pub revision: u64,
    pub producer_role: ProgramRole,
    pub parameters: Vec<DistributedFunctionParameterPlan>,
    pub result_type: DataTypePlan,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DistributedCallMode {
    Current,
    Invocation,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DistributedCallResultPlan {
    Current {
        import_id: ImportId,
    },
    Invocation {
        source_id: SourceId,
        payload_field: SourcePayloadField,
    },
}

impl DistributedCallResultPlan {
    pub fn value_ref(&self) -> ValueRef {
        match self {
            Self::Current { import_id } => ValueRef::DistributedImport(*import_id),
            Self::Invocation {
                source_id,
                payload_field,
            } => ValueRef::SourcePayload {
                source_id: *source_id,
                field: payload_field.clone(),
            },
        }
    }

    pub fn current_import_id(&self) -> Option<ImportId> {
        match self {
            Self::Current { import_id } => Some(*import_id),
            Self::Invocation { .. } => None,
        }
    }

    pub fn invocation_source(&self) -> Option<(SourceId, &SourcePayloadField)> {
        match self {
            Self::Current { .. } => None,
            Self::Invocation {
                source_id,
                payload_field,
            } => Some((*source_id, payload_field)),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DistributedInvocationArmPlan {
    pub trigger: ValueRef,
    pub gate: PlanRowExpressionId,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DistributedCallArgumentPlan {
    pub argument_id: DistributedArgumentId,
    pub name: String,
    pub data_type: DataTypePlan,
    pub value: PlanRowExpressionId,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct DistributedCallRowBindingPlan {
    pub owner: PlanStaticOwnerId,
    pub local: PlanLocalId,
    pub list: ListId,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct DistributedCallInstanceRow {
    pub owner: PlanStaticOwnerId,
    pub local: PlanLocalId,
    pub row: OwnerInstanceRow,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RemoteCallSitePlan {
    pub call_site_id: RemoteCallSiteId,
    pub owner: PlanOwner,
    pub result: DistributedCallResultPlan,
    pub stable_identity: DistributedDeclarationId,
    pub revision: u64,
    pub caller_role: ProgramRole,
    pub callee_role: ProgramRole,
    pub scope: DistributedRouteScopePlan,
    pub function_export_id: ExportId,
    pub function_revision: u64,
    pub mode: DistributedCallMode,
    pub arguments: Vec<DistributedCallArgumentPlan>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub row_bindings: Vec<DistributedCallRowBindingPlan>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub invocation_arms: Vec<DistributedInvocationArmPlan>,
    pub result_type: DataTypePlan,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProducerFunctionArgumentPlan {
    pub argument_id: DistributedArgumentId,
    pub name: String,
    pub import_id: ImportId,
    pub data_type: DataTypePlan,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProducerFunctionOwnershipPlan {
    pub static_owners: Vec<PlanStaticOwnerId>,
    pub sources: Vec<SourceId>,
    pub states: Vec<StateId>,
    pub fields: Vec<FieldId>,
    pub lists: Vec<ListId>,
    pub indexes: Vec<PlanListIndexId>,
    pub effects: Vec<EffectInvocationId>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProducerFunctionInstancePlan {
    pub call_site_id: RemoteCallSiteId,
    pub function_export_id: ExportId,
    pub owner: PlanOwner,
    pub mode: DistributedCallMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invocation_source: Option<SourceId>,
    pub ownership: ProducerFunctionOwnershipPlan,
    pub arguments: Vec<ProducerFunctionArgumentPlan>,
    pub result: ValueRef,
    pub result_type: DataTypePlan,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DistributedWireEndpointPlan {
    pub endpoint_id: DistributedEndpointId,
    pub role: ProgramRole,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DistributedWireValueEdgePlan {
    pub export_id: ExportId,
    pub import_id: ImportId,
    pub producer_role: ProgramRole,
    pub consumer_role: ProgramRole,
    pub scope: DistributedRouteScopePlan,
    pub data_type: DataTypePlan,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DistributedWireEventEdgePlan {
    pub export_id: ExportId,
    pub import_id: ImportId,
    pub producer_role: ProgramRole,
    pub consumer_role: ProgramRole,
    pub scope: DistributedRouteScopePlan,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload_field: Option<SourcePayloadField>,
    pub payload_type: DataTypePlan,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DistributedWireCallEdgePlan {
    pub call_site_id: RemoteCallSiteId,
    pub caller_role: ProgramRole,
    pub callee_role: ProgramRole,
    pub scope: DistributedRouteScopePlan,
    pub function_export_id: ExportId,
    pub mode: DistributedCallMode,
    pub parameters: Vec<DistributedFunctionParameterPlan>,
    pub result_type: DataTypePlan,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DistributedWireSchemaPlan {
    pub graph_id: DistributedGraphId,
    pub endpoints: Vec<DistributedWireEndpointPlan>,
    pub value_edges: Vec<DistributedWireValueEdgePlan>,
    pub event_edges: Vec<DistributedWireEventEdgePlan>,
    pub call_edges: Vec<DistributedWireCallEdgePlan>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DistributedEndpointContractPlan {
    pub endpoint_id: DistributedEndpointId,
    pub stable_identity: DistributedDeclarationId,
    pub revision: u64,
    pub role: ProgramRole,
    pub value_exports: Vec<DistributedValueExportPlan>,
    pub value_imports: Vec<DistributedValueImportPlan>,
    pub event_exports: Vec<DistributedEventExportPlan>,
    pub event_imports: Vec<DistributedEventImportPlan>,
    pub function_exports: Vec<DistributedFunctionExportPlan>,
    pub remote_call_sites: Vec<RemoteCallSitePlan>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DistributedEndpointPlan {
    pub graph: DistributedGraphIdentityPlan,
    pub endpoint: DistributedEndpointContractPlan,
    pub wire_schema: DistributedWireSchemaPlan,
    pub wire_schema_hash: [u8; 32],
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DistributedGraphPlan {
    pub graph: DistributedGraphIdentityPlan,
    pub endpoints: Vec<DistributedEndpointContractPlan>,
    pub wire_schema: DistributedWireSchemaPlan,
    pub wire_schema_hash: [u8; 32],
}

impl DistributedGraphIdentityPlan {
    pub fn new(
        application: &ApplicationIdentity,
        stable_identity: DistributedDeclarationId,
        revision: u64,
    ) -> Result<Self, PlanError> {
        let plan = Self {
            graph_id: DistributedGraphId::from_identity(application, stable_identity)?,
            stable_identity,
            revision,
        };
        plan.validate(application)?;
        Ok(plan)
    }

    pub fn validate(&self, application: &ApplicationIdentity) -> Result<(), PlanError> {
        if self.revision == 0
            || digest_is_zero(self.stable_identity.as_bytes())
            || self.graph_id
                != DistributedGraphId::from_identity(application, self.stable_identity)?
        {
            return Err(PlanError::new(
                "distributed graph identity or revision is not canonical",
            ));
        }
        Ok(())
    }
}

impl DistributedValueExportPlan {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        graph_id: DistributedGraphId,
        endpoint_id: DistributedEndpointId,
        stable_identity: DistributedDeclarationId,
        revision: u64,
        producer_role: ProgramRole,
        origin_scoped: bool,
        value: ValueRef,
        data_type: DataTypePlan,
    ) -> Result<Self, PlanError> {
        let plan = Self {
            export_id: ExportId::from_identity(
                graph_id,
                endpoint_id,
                DistributedExportKind::Value,
                stable_identity,
            )?,
            stable_identity,
            revision,
            producer_role,
            origin_scoped,
            value,
            data_type: data_type.canonicalized(),
        };
        validate_distributed_value_export(graph_id, endpoint_id, producer_role, &plan)?;
        Ok(plan)
    }
}

impl DistributedValueImportPlan {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        graph_id: DistributedGraphId,
        endpoint_id: DistributedEndpointId,
        stable_identity: DistributedDeclarationId,
        revision: u64,
        consumer_role: ProgramRole,
        source: &DistributedValueExportPlan,
    ) -> Result<Self, PlanError> {
        let plan = Self {
            import_id: ImportId::from_value_identity(graph_id, endpoint_id, stable_identity)?,
            stable_identity,
            revision,
            consumer_role,
            producer_role: source.producer_role,
            scope: distributed_value_route_scope(
                consumer_role,
                source.producer_role,
                source.origin_scoped,
            )?,
            source_export_id: source.export_id,
            source_revision: source.revision,
            source_origin_scoped: source.origin_scoped,
            data_type: source.data_type.clone(),
        };
        validate_distributed_value_import(graph_id, endpoint_id, consumer_role, &plan)?;
        Ok(plan)
    }
}

impl DistributedEventExportPlan {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        graph_id: DistributedGraphId,
        endpoint_id: DistributedEndpointId,
        stable_identity: DistributedDeclarationId,
        revision: u64,
        producer_role: ProgramRole,
        source_id: SourceId,
        payload_field: Option<SourcePayloadField>,
        payload_type: DataTypePlan,
    ) -> Result<Self, PlanError> {
        let plan = Self {
            export_id: ExportId::from_identity(
                graph_id,
                endpoint_id,
                DistributedExportKind::Event,
                stable_identity,
            )?,
            stable_identity,
            revision,
            producer_role,
            source_id,
            payload_field,
            payload_type: payload_type.canonicalized(),
        };
        validate_distributed_event_export(graph_id, endpoint_id, producer_role, &plan)?;
        Ok(plan)
    }
}

impl DistributedEventImportPlan {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        graph_id: DistributedGraphId,
        endpoint_id: DistributedEndpointId,
        stable_identity: DistributedDeclarationId,
        revision: u64,
        consumer_role: ProgramRole,
        source: &DistributedEventExportPlan,
        local_source_id: SourceId,
    ) -> Result<Self, PlanError> {
        let plan = Self {
            import_id: ImportId::from_event_identity(graph_id, endpoint_id, stable_identity)?,
            stable_identity,
            revision,
            consumer_role,
            producer_role: source.producer_role,
            scope: distributed_event_route_scope(consumer_role, source.producer_role)?,
            source_export_id: source.export_id,
            source_revision: source.revision,
            local_source_id,
            payload_field: source.payload_field.clone(),
            payload_type: source.payload_type.clone(),
        };
        validate_distributed_event_import(graph_id, endpoint_id, consumer_role, &plan)?;
        Ok(plan)
    }
}

impl DistributedFunctionParameterPlan {
    pub fn new(
        export_id: ExportId,
        name: impl Into<String>,
        data_type: DataTypePlan,
    ) -> Result<Self, PlanError> {
        let name = name.into();
        let plan = Self {
            argument_id: DistributedArgumentId::from_parameter_name(export_id, &name)?,
            name,
            data_type: data_type.canonicalized(),
        };
        if !distributed_data_type_is_supported(&plan.data_type) {
            return Err(PlanError::new(
                "distributed function parameters require canonical closed value types",
            ));
        }
        Ok(plan)
    }
}

impl DistributedFunctionExportPlan {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        graph_id: DistributedGraphId,
        endpoint_id: DistributedEndpointId,
        stable_identity: DistributedDeclarationId,
        revision: u64,
        producer_role: ProgramRole,
        parameters: Vec<(String, DataTypePlan)>,
        result_type: DataTypePlan,
    ) -> Result<Self, PlanError> {
        let export_id = ExportId::from_identity(
            graph_id,
            endpoint_id,
            DistributedExportKind::Function,
            stable_identity,
        )?;
        let mut parameters = parameters
            .into_iter()
            .map(|(name, data_type)| {
                DistributedFunctionParameterPlan::new(export_id, name, data_type)
            })
            .collect::<Result<Vec<_>, _>>()?;
        parameters.sort_by(|left, right| left.name.cmp(&right.name));
        let plan = Self {
            export_id,
            stable_identity,
            revision,
            producer_role,
            parameters,
            result_type: result_type.canonicalized(),
        };
        validate_distributed_function_export(graph_id, endpoint_id, producer_role, &plan)?;
        Ok(plan)
    }
}

impl RemoteCallSitePlan {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        graph_id: DistributedGraphId,
        endpoint_id: DistributedEndpointId,
        stable_identity: DistributedDeclarationId,
        revision: u64,
        caller_role: ProgramRole,
        owner: PlanOwner,
        function: &DistributedFunctionExportPlan,
        arguments: Vec<(String, PlanRowExpressionId)>,
        mut row_bindings: Vec<DistributedCallRowBindingPlan>,
        mode: DistributedCallMode,
        invocation_result_source: Option<SourceId>,
        invocation_arms: Vec<DistributedInvocationArmPlan>,
    ) -> Result<Self, PlanError> {
        let call_site_id = RemoteCallSiteId::from_identity(graph_id, endpoint_id, stable_identity)?;
        let mut arguments = arguments
            .into_iter()
            .map(|(name, value)| {
                let parameter = function
                    .parameters
                    .iter()
                    .find(|parameter| parameter.name == name)
                    .ok_or_else(|| {
                        PlanError::new(format!(
                            "remote call argument `{name}` does not exist in the target signature"
                        ))
                    })?;
                Ok(DistributedCallArgumentPlan {
                    argument_id: parameter.argument_id,
                    name,
                    data_type: parameter.data_type.clone(),
                    value,
                })
            })
            .collect::<Result<Vec<_>, PlanError>>()?;
        arguments.sort_by(|left, right| left.name.cmp(&right.name));
        row_bindings.sort();
        let result = match (mode, invocation_result_source) {
            (DistributedCallMode::Current, None) => DistributedCallResultPlan::Current {
                import_id: ImportId::from_remote_call_result(call_site_id)?,
            },
            (DistributedCallMode::Invocation, Some(source_id)) => {
                DistributedCallResultPlan::Invocation {
                    source_id,
                    payload_field: SourcePayloadField::Named("result".to_owned()),
                }
            }
            (DistributedCallMode::Current, Some(_)) => {
                return Err(PlanError::new(
                    "current remote calls cannot declare an invocation result source",
                ));
            }
            (DistributedCallMode::Invocation, None) => {
                return Err(PlanError::new(
                    "invocation remote calls require a private result source",
                ));
            }
        };
        let plan = Self {
            call_site_id,
            owner,
            result,
            stable_identity,
            revision,
            caller_role,
            callee_role: function.producer_role,
            scope: distributed_call_route_scope(caller_role, function.producer_role)?,
            function_export_id: function.export_id,
            function_revision: function.revision,
            mode,
            arguments,
            row_bindings,
            invocation_arms,
            result_type: function.result_type.clone(),
        };
        validate_remote_call_site(graph_id, endpoint_id, caller_role, &plan, None, None)?;
        Ok(plan)
    }
}

impl ProducerFunctionArgumentPlan {
    pub fn new(
        call_site_id: RemoteCallSiteId,
        argument_id: DistributedArgumentId,
        name: impl Into<String>,
        data_type: DataTypePlan,
    ) -> Result<Self, PlanError> {
        let name = name.into();
        let plan = Self {
            argument_id,
            name,
            import_id: ImportId::from_producer_argument(call_site_id, argument_id)?,
            data_type: data_type.canonicalized(),
        };
        validate_producer_function_argument(call_site_id, &plan)?;
        Ok(plan)
    }
}

impl ProducerFunctionOwnershipPlan {
    pub fn new(
        static_owners: Vec<PlanStaticOwnerId>,
        sources: Vec<SourceId>,
        states: Vec<StateId>,
        fields: Vec<FieldId>,
        lists: Vec<ListId>,
        indexes: Vec<PlanListIndexId>,
        effects: Vec<EffectInvocationId>,
    ) -> Self {
        Self {
            static_owners,
            sources,
            states,
            fields,
            lists,
            indexes,
            effects,
        }
        .canonicalized()
    }

    pub fn canonicalized(&self) -> Self {
        let mut ownership = self.clone();
        canonicalize_producer_ownership_ids(&mut ownership.static_owners);
        canonicalize_producer_ownership_ids(&mut ownership.sources);
        canonicalize_producer_ownership_ids(&mut ownership.states);
        canonicalize_producer_ownership_ids(&mut ownership.fields);
        canonicalize_producer_ownership_ids(&mut ownership.lists);
        canonicalize_producer_ownership_ids(&mut ownership.indexes);
        canonicalize_producer_ownership_ids(&mut ownership.effects);
        ownership
    }

    fn is_canonical(&self) -> bool {
        producer_ownership_ids_are_canonical(&self.static_owners)
            && producer_ownership_ids_are_canonical(&self.sources)
            && producer_ownership_ids_are_canonical(&self.states)
            && producer_ownership_ids_are_canonical(&self.fields)
            && producer_ownership_ids_are_canonical(&self.lists)
            && producer_ownership_ids_are_canonical(&self.indexes)
            && producer_ownership_ids_are_canonical(&self.effects)
    }
}

fn canonicalize_producer_ownership_ids<T: Ord>(ids: &mut Vec<T>) {
    ids.sort_unstable();
    ids.dedup();
}

fn producer_ownership_ids_are_canonical<T: Ord>(ids: &[T]) -> bool {
    ids.windows(2).all(|pair| pair[0] < pair[1])
}

impl ProducerFunctionInstancePlan {
    pub fn new(
        call_site_id: RemoteCallSiteId,
        function: &DistributedFunctionExportPlan,
        owner: PlanOwner,
        mode: DistributedCallMode,
        invocation_source: Option<SourceId>,
        ownership: ProducerFunctionOwnershipPlan,
        result: ValueRef,
    ) -> Result<Self, PlanError> {
        let arguments = function
            .parameters
            .iter()
            .map(|parameter| {
                ProducerFunctionArgumentPlan::new(
                    call_site_id,
                    parameter.argument_id,
                    parameter.name.clone(),
                    parameter.data_type.clone(),
                )
            })
            .collect::<Result<Vec<_>, _>>()?;
        let plan = Self {
            call_site_id,
            function_export_id: function.export_id,
            owner,
            mode,
            invocation_source,
            ownership: ownership.canonicalized(),
            arguments,
            result,
            result_type: function.result_type.clone(),
        };
        validate_producer_function_instance_signature(&plan, function)?;
        Ok(plan)
    }
}

impl DistributedEndpointContractPlan {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        graph: &DistributedGraphIdentityPlan,
        stable_identity: DistributedDeclarationId,
        revision: u64,
        role: ProgramRole,
        mut value_exports: Vec<DistributedValueExportPlan>,
        mut value_imports: Vec<DistributedValueImportPlan>,
        mut event_exports: Vec<DistributedEventExportPlan>,
        mut event_imports: Vec<DistributedEventImportPlan>,
        mut function_exports: Vec<DistributedFunctionExportPlan>,
        mut remote_call_sites: Vec<RemoteCallSitePlan>,
    ) -> Result<Self, PlanError> {
        let endpoint_id =
            DistributedEndpointId::from_identity(graph.graph_id, role, stable_identity)?;
        value_exports.sort_by_key(|export| export.export_id);
        value_imports.sort_by_key(|import| import.import_id);
        event_exports.sort_by_key(|export| export.export_id);
        event_imports.sort_by_key(|import| import.import_id);
        function_exports.sort_by_key(|export| export.export_id);
        remote_call_sites.sort_by_key(|call| call.call_site_id);
        let plan = Self {
            endpoint_id,
            stable_identity,
            revision,
            role,
            value_exports,
            value_imports,
            event_exports,
            event_imports,
            function_exports,
            remote_call_sites,
        };
        validate_distributed_endpoint_contract(graph, &plan, None, None)?;
        Ok(plan)
    }
}

impl DistributedEndpointPlan {
    pub fn new(
        application: &ApplicationIdentity,
        graph: &DistributedGraphPlan,
        role: ProgramRole,
    ) -> Result<Self, PlanError> {
        graph.validate(application)?;
        let plan = graph.endpoint_plan(role).ok_or_else(|| {
            PlanError::new("linked distributed graph does not contain the requested endpoint role")
        })?;
        plan.validate(application, role)?;
        Ok(plan)
    }

    pub fn validate(
        &self,
        application: &ApplicationIdentity,
        machine_role: ProgramRole,
    ) -> Result<(), PlanError> {
        self.graph.validate(application)?;
        if self.endpoint.role != machine_role {
            return Err(PlanError::new(
                "distributed endpoint role does not match the MachinePlan role",
            ));
        }
        validate_distributed_endpoint_contract(&self.graph, &self.endpoint, None, None)?;
        validate_distributed_wire_schema(&self.graph, &self.wire_schema)?;
        if self.wire_schema_hash != distributed_wire_schema_hash(&self.wire_schema)? {
            return Err(PlanError::new(
                "distributed endpoint wire schema hash does not match its linked projection",
            ));
        }
        validate_distributed_endpoint_wire_contract(&self.endpoint, &self.wire_schema)
    }

    pub fn value_import_route(&self, import_id: ImportId) -> Option<&DistributedWireValueEdgePlan> {
        self.wire_schema
            .value_edges
            .iter()
            .find(|edge| edge.consumer_role == self.endpoint.role && edge.import_id == import_id)
    }

    pub fn event_import_route(&self, import_id: ImportId) -> Option<&DistributedWireEventEdgePlan> {
        self.wire_schema
            .event_edges
            .iter()
            .find(|edge| edge.consumer_role == self.endpoint.role && edge.import_id == import_id)
    }

    pub fn outbound_call_route(
        &self,
        call_site_id: RemoteCallSiteId,
    ) -> Option<&DistributedWireCallEdgePlan> {
        self.wire_schema.call_edges.iter().find(|edge| {
            edge.caller_role == self.endpoint.role && edge.call_site_id == call_site_id
        })
    }

    pub fn inbound_call_route(
        &self,
        call_site_id: RemoteCallSiteId,
    ) -> Option<&DistributedWireCallEdgePlan> {
        self.wire_schema.call_edges.iter().find(|edge| {
            edge.callee_role == self.endpoint.role && edge.call_site_id == call_site_id
        })
    }
}

impl DistributedGraphPlan {
    pub fn new(
        application: &ApplicationIdentity,
        graph: DistributedGraphIdentityPlan,
        mut endpoints: Vec<DistributedEndpointContractPlan>,
    ) -> Result<Self, PlanError> {
        endpoints.sort_by_key(|endpoint| endpoint.role);
        graph.validate(application)?;
        validate_distributed_graph_endpoints(&graph, &endpoints)?;
        let wire_schema = distributed_wire_schema_projection(&graph, &endpoints)?;
        validate_distributed_wire_schema(&graph, &wire_schema)?;
        let wire_schema_hash = distributed_wire_schema_hash(&wire_schema)?;
        let plan = Self {
            graph,
            endpoints,
            wire_schema,
            wire_schema_hash,
        };
        plan.validate(application)?;
        Ok(plan)
    }

    pub fn validate(&self, application: &ApplicationIdentity) -> Result<(), PlanError> {
        self.graph.validate(application)?;
        validate_distributed_graph_endpoints(&self.graph, &self.endpoints)?;
        let expected_wire_schema =
            distributed_wire_schema_projection(&self.graph, &self.endpoints)?;
        if self.wire_schema != expected_wire_schema {
            return Err(PlanError::new(
                "distributed graph wire schema is not the canonical linked projection",
            ));
        }
        validate_distributed_wire_schema(&self.graph, &self.wire_schema)?;
        if self.wire_schema_hash != distributed_wire_schema_hash(&self.wire_schema)? {
            return Err(PlanError::new(
                "distributed graph wire schema hash does not match its linked projection",
            ));
        }
        Ok(())
    }

    pub fn endpoint_plan(&self, role: ProgramRole) -> Option<DistributedEndpointPlan> {
        self.endpoints
            .iter()
            .find(|endpoint| endpoint.role == role)
            .cloned()
            .map(|endpoint| DistributedEndpointPlan {
                graph: self.graph.clone(),
                endpoint,
                wire_schema: self.wire_schema.clone(),
                wire_schema_hash: self.wire_schema_hash,
            })
    }
}

fn distributed_name_is_canonical(value: &str) -> bool {
    !value.is_empty() && value.trim() == value && !value.chars().any(char::is_control)
}

fn distributed_value_route_scope(
    consumer: ProgramRole,
    producer: ProgramRole,
    producer_origin_scoped: bool,
) -> Result<DistributedRouteScopePlan, PlanError> {
    match (consumer, producer) {
        (ProgramRole::Client, ProgramRole::Session)
        | (ProgramRole::Session, ProgramRole::Client) => {
            Ok(DistributedRouteScopePlan::SessionLocal)
        }
        (ProgramRole::Session, ProgramRole::Server) => Ok(if producer_origin_scoped {
            DistributedRouteScopePlan::OriginScoped
        } else {
            DistributedRouteScopePlan::SharedSubscription
        }),
        (ProgramRole::Server, ProgramRole::Session) => Ok(DistributedRouteScopePlan::OriginScoped),
        _ => Err(PlanError::new(
            "distributed current-value route is not an adjacent role edge",
        )),
    }
}

fn distributed_event_route_scope(
    consumer: ProgramRole,
    producer: ProgramRole,
) -> Result<DistributedRouteScopePlan, PlanError> {
    match (consumer, producer) {
        (ProgramRole::Client, ProgramRole::Session)
        | (ProgramRole::Session, ProgramRole::Client) => {
            Ok(DistributedRouteScopePlan::SessionLocal)
        }
        (ProgramRole::Session, ProgramRole::Server)
        | (ProgramRole::Server, ProgramRole::Session) => {
            Ok(DistributedRouteScopePlan::OriginScoped)
        }
        _ => Err(PlanError::new(
            "distributed event route is not an adjacent role edge",
        )),
    }
}

fn distributed_call_route_scope(
    caller: ProgramRole,
    callee: ProgramRole,
) -> Result<DistributedRouteScopePlan, PlanError> {
    match (caller, callee) {
        (ProgramRole::Client, ProgramRole::Session)
        | (ProgramRole::Session, ProgramRole::Client) => {
            Ok(DistributedRouteScopePlan::SessionLocal)
        }
        (ProgramRole::Session, ProgramRole::Server)
        | (ProgramRole::Server, ProgramRole::Session) => {
            Ok(DistributedRouteScopePlan::OriginScoped)
        }
        _ => Err(PlanError::new(
            "distributed call route is not an adjacent role edge",
        )),
    }
}

fn digest_is_zero(digest: &[u8; 32]) -> bool {
    digest.iter().all(|byte| *byte == 0)
}

fn distributed_data_type_is_supported(data_type: &DataTypePlan) -> bool {
    if !data_type.is_canonical() {
        return false;
    }
    let fields_supported = |fields: &[DataTypeFieldPlan]| {
        fields.iter().all(|field| {
            distributed_name_is_canonical(&field.name)
                && distributed_data_type_is_supported(&field.data_type)
        })
    };
    match data_type {
        DataTypePlan::Variant { variants } => variants.iter().all(|variant| {
            distributed_name_is_canonical(&variant.tag)
                && !variant.open
                && fields_supported(&variant.fields)
        }),
        DataTypePlan::Record { fields, open } | DataTypePlan::Error { fields, open } => {
            !open && fields_supported(fields)
        }
        DataTypePlan::List { item } => distributed_data_type_is_supported(item),
        DataTypePlan::Unknown => false,
        DataTypePlan::Null
        | DataTypePlan::Bool
        | DataTypePlan::Number
        | DataTypePlan::Text
        | DataTypePlan::Bytes { .. } => true,
    }
}

fn distributed_external_value_ref_is_supported(value: &ValueRef) -> bool {
    match value {
        ValueRef::State(_) | ValueRef::Field(_) | ValueRef::Constant(_) => true,
        ValueRef::StateProjection { field_path, .. } => {
            !field_path.is_empty()
                && field_path
                    .iter()
                    .all(|part| distributed_name_is_canonical(part))
        }
        ValueRef::DistributedImport(_) => true,
        ValueRef::Source(_) | ValueRef::SourcePayload { .. } | ValueRef::List(_) => false,
    }
}

fn validate_distributed_value_export(
    graph_id: DistributedGraphId,
    endpoint_id: DistributedEndpointId,
    endpoint_role: ProgramRole,
    export: &DistributedValueExportPlan,
) -> Result<(), PlanError> {
    if export.revision == 0
        || digest_is_zero(export.stable_identity.as_bytes())
        || export.producer_role != endpoint_role
        || (export.origin_scoped && export.producer_role != ProgramRole::Server)
        || export.export_id
            != ExportId::from_identity(
                graph_id,
                endpoint_id,
                DistributedExportKind::Value,
                export.stable_identity,
            )?
        || !distributed_data_type_is_supported(&export.data_type)
        || !distributed_external_value_ref_is_supported(&export.value)
    {
        return Err(PlanError::new(
            "distributed value export has a noncanonical ID, revision, role, type, or value ref",
        ));
    }
    Ok(())
}

fn validate_distributed_value_import(
    graph_id: DistributedGraphId,
    endpoint_id: DistributedEndpointId,
    endpoint_role: ProgramRole,
    import: &DistributedValueImportPlan,
) -> Result<(), PlanError> {
    if import.revision == 0
        || import.source_revision == 0
        || digest_is_zero(import.stable_identity.as_bytes())
        || import.consumer_role != endpoint_role
        || !import.consumer_role.can_depend_on(import.producer_role)
        || import.scope
            != distributed_value_route_scope(
                import.consumer_role,
                import.producer_role,
                import.source_origin_scoped,
            )?
        || import.import_id
            != ImportId::from_value_identity(graph_id, endpoint_id, import.stable_identity)?
        || !distributed_data_type_is_supported(&import.data_type)
    {
        return Err(PlanError::new(
            "distributed value import has a noncanonical ID, revision, direction, role, or type",
        ));
    }
    Ok(())
}

fn validate_distributed_event_export(
    graph_id: DistributedGraphId,
    endpoint_id: DistributedEndpointId,
    endpoint_role: ProgramRole,
    export: &DistributedEventExportPlan,
) -> Result<(), PlanError> {
    if export.revision == 0
        || digest_is_zero(export.stable_identity.as_bytes())
        || export.producer_role != endpoint_role
        || export.export_id
            != ExportId::from_identity(
                graph_id,
                endpoint_id,
                DistributedExportKind::Event,
                export.stable_identity,
            )?
        || !distributed_data_type_is_supported(&export.payload_type)
    {
        return Err(PlanError::new(
            "distributed event export has a noncanonical ID, revision, role, or payload type",
        ));
    }
    Ok(())
}

fn validate_distributed_event_import(
    graph_id: DistributedGraphId,
    endpoint_id: DistributedEndpointId,
    endpoint_role: ProgramRole,
    import: &DistributedEventImportPlan,
) -> Result<(), PlanError> {
    if import.revision == 0
        || import.source_revision == 0
        || digest_is_zero(import.stable_identity.as_bytes())
        || import.consumer_role != endpoint_role
        || !import.consumer_role.can_depend_on(import.producer_role)
        || import.scope
            != distributed_event_route_scope(import.consumer_role, import.producer_role)?
        || import.import_id
            != ImportId::from_event_identity(graph_id, endpoint_id, import.stable_identity)?
        || !distributed_data_type_is_supported(&import.payload_type)
    {
        return Err(PlanError::new(
            "distributed event import has a noncanonical ID, revision, direction, role, or payload type",
        ));
    }
    Ok(())
}

fn validate_distributed_function_export(
    graph_id: DistributedGraphId,
    endpoint_id: DistributedEndpointId,
    endpoint_role: ProgramRole,
    export: &DistributedFunctionExportPlan,
) -> Result<(), PlanError> {
    if export.revision == 0
        || digest_is_zero(export.stable_identity.as_bytes())
        || export.producer_role != endpoint_role
        || export.export_id
            != ExportId::from_identity(
                graph_id,
                endpoint_id,
                DistributedExportKind::Function,
                export.stable_identity,
            )?
        || !distributed_data_type_is_supported(&export.result_type)
        || !export
            .parameters
            .windows(2)
            .all(|pair| pair[0].name < pair[1].name)
    {
        return Err(PlanError::new(
            "distributed function has a noncanonical ID, revision, role, signature, or parameter order",
        ));
    }
    let mut argument_ids = BTreeSet::new();
    for parameter in &export.parameters {
        if !distributed_name_is_canonical(&parameter.name)
            || !distributed_data_type_is_supported(&parameter.data_type)
            || parameter.argument_id
                != DistributedArgumentId::from_parameter_name(export.export_id, &parameter.name)?
            || !argument_ids.insert(parameter.argument_id)
        {
            return Err(PlanError::new(
                "distributed function parameters must be named, typed, unique, and canonically identified",
            ));
        }
    }
    Ok(())
}

fn validate_producer_function_argument(
    call_site_id: RemoteCallSiteId,
    argument: &ProducerFunctionArgumentPlan,
) -> Result<(), PlanError> {
    if digest_is_zero(call_site_id.as_bytes())
        || digest_is_zero(argument.argument_id.as_bytes())
        || digest_is_zero(argument.import_id.as_bytes())
        || !distributed_name_is_canonical(&argument.name)
        || !distributed_data_type_is_supported(&argument.data_type)
        || argument.import_id
            != ImportId::from_producer_argument(call_site_id, argument.argument_id)?
    {
        return Err(PlanError::new(
            "producer function argument has a noncanonical ID, name, import, or closed type",
        ));
    }
    Ok(())
}

fn validate_producer_function_ownership(
    instance: &ProducerFunctionInstancePlan,
) -> Result<(), PlanError> {
    let ownership = &instance.ownership;
    if instance.owner.static_owner.is_root()
        || ownership.static_owners.iter().any(|owner| owner.is_root())
    {
        return Err(PlanError::new(
            "producer function ownership cannot contain the ROOT static owner",
        ));
    }
    if !ownership.is_canonical() {
        return Err(PlanError::new(
            "producer function ownership IDs must be unique and canonically ordered",
        ));
    }
    if ownership.static_owners.first() != Some(&instance.owner.static_owner) {
        return Err(PlanError::new(
            "producer function ownership must begin with the instance static owner",
        ));
    }
    match &instance.result {
        ValueRef::Field(field_id) if !ownership.fields.contains(field_id) => {
            return Err(PlanError::new(
                "producer function field result must be present in its ownership manifest",
            ));
        }
        ValueRef::List(list_id) if !ownership.lists.contains(list_id) => {
            return Err(PlanError::new(
                "producer function list result must be present in its ownership manifest",
            ));
        }
        _ => {}
    }
    Ok(())
}

fn validate_producer_function_instance_signature(
    instance: &ProducerFunctionInstancePlan,
    function: &DistributedFunctionExportPlan,
) -> Result<(), PlanError> {
    validate_producer_function_ownership(instance)?;
    let function_argument_ids = function
        .parameters
        .iter()
        .map(|parameter| parameter.argument_id)
        .collect::<BTreeSet<_>>();
    if digest_is_zero(instance.call_site_id.as_bytes())
        || digest_is_zero(instance.function_export_id.as_bytes())
        || digest_is_zero(function.export_id.as_bytes())
        || digest_is_zero(function.stable_identity.as_bytes())
        || function.revision == 0
        || instance.function_export_id != function.export_id
        || match instance.mode {
            DistributedCallMode::Current => instance.invocation_source.is_some(),
            DistributedCallMode::Invocation => instance
                .invocation_source
                .is_none_or(|source| !instance.ownership.sources.contains(&source)),
        }
        || !function
            .parameters
            .windows(2)
            .all(|pair| pair[0].name < pair[1].name)
        || function_argument_ids.len() != function.parameters.len()
        || !function.parameters.iter().all(|parameter| {
            distributed_name_is_canonical(&parameter.name)
                && distributed_data_type_is_supported(&parameter.data_type)
                && DistributedArgumentId::from_parameter_name(function.export_id, &parameter.name)
                    .is_ok_and(|argument_id| argument_id == parameter.argument_id)
        })
        || !distributed_data_type_is_supported(&function.result_type)
        || !distributed_data_type_is_supported(&instance.result_type)
        || instance.result_type != function.result_type
        || instance.arguments.len() != function.parameters.len()
        || !instance
            .arguments
            .iter()
            .zip(&function.parameters)
            .all(|(argument, parameter)| {
                argument.argument_id == parameter.argument_id
                    && argument.name == parameter.name
                    && argument.data_type == parameter.data_type
                    && validate_producer_function_argument(instance.call_site_id, argument).is_ok()
            })
        || matches!(
            &instance.result,
            ValueRef::DistributedImport(import_id)
                if instance.arguments.iter().any(|argument| {
                    argument.import_id == *import_id
                        && argument.data_type != instance.result_type
                })
        )
    {
        return Err(PlanError::new(
            "producer function instance must have a canonical call owner and exactly match its function signature",
        ));
    }
    let argument_ids = instance
        .arguments
        .iter()
        .map(|argument| argument.argument_id)
        .collect::<BTreeSet<_>>();
    let import_ids = instance
        .arguments
        .iter()
        .map(|argument| argument.import_id)
        .collect::<BTreeSet<_>>();
    if argument_ids.len() != instance.arguments.len()
        || import_ids.len() != instance.arguments.len()
    {
        return Err(PlanError::new(
            "producer function instance arguments and imports must be unique",
        ));
    }
    Ok(())
}

fn validate_remote_call_site(
    graph_id: DistributedGraphId,
    endpoint_id: DistributedEndpointId,
    endpoint_role: ProgramRole,
    call: &RemoteCallSitePlan,
    constants: Option<&[PlanConstant]>,
    arena: Option<&PlanRowExpressionArena>,
) -> Result<(), PlanError> {
    let contextual_bindings =
        distributed_call_contextual_bindings(call, arena).ok_or_else(|| {
            PlanError::new(
                "remote call contextual locals must belong to one declared structural owner chain",
            )
        })?;
    let result_is_canonical = match (&call.mode, &call.result) {
        (DistributedCallMode::Current, DistributedCallResultPlan::Current { import_id }) => {
            *import_id == ImportId::from_remote_call_result(call.call_site_id)?
        }
        (
            DistributedCallMode::Invocation,
            DistributedCallResultPlan::Invocation {
                payload_field: SourcePayloadField::Named(name),
                ..
            },
        ) => name == "result",
        _ => false,
    };
    if call.revision == 0
        || call.function_revision == 0
        || digest_is_zero(call.stable_identity.as_bytes())
        || call.caller_role != endpoint_role
        || !call.caller_role.can_depend_on(call.callee_role)
        || call.scope != distributed_call_route_scope(call.caller_role, call.callee_role)?
        || call.call_site_id
            != RemoteCallSiteId::from_identity(graph_id, endpoint_id, call.stable_identity)?
        || !result_is_canonical
        || !distributed_data_type_is_supported(&call.result_type)
        || (call.owner.static_owner.is_root() && !call.owner.ancestors.is_empty())
        || !call
            .arguments
            .windows(2)
            .all(|pair| pair[0].name < pair[1].name)
        || match call.mode {
            DistributedCallMode::Current => !call.invocation_arms.is_empty(),
            DistributedCallMode::Invocation => call.invocation_arms.is_empty(),
        }
    {
        return Err(PlanError::new(
            "remote call has a noncanonical ID, revision, direction, role, result, or argument order",
        ));
    }
    let mut argument_ids = BTreeSet::new();
    for argument in &call.arguments {
        if !distributed_name_is_canonical(&argument.name) {
            return Err(PlanError::new("remote call argument name is not canonical"));
        }
        if !distributed_data_type_is_supported(&argument.data_type) {
            return Err(PlanError::new(format!(
                "remote call argument `{}` has an unsupported boundary type",
                argument.name
            )));
        }
        if let Some(arena) = arena {
            if !distributed_call_expression_is_safe(
                arena,
                argument.value,
                constants,
                call.mode == DistributedCallMode::Invocation,
                &contextual_bindings,
                &call.row_bindings,
            ) {
                return Err(PlanError::new(format!(
                    "remote call argument `{}` has an unsupported bounded expression id {}",
                    argument.name, argument.value.0
                )));
            }
            if !arena.contextual_locals_resolve_with_bindings(
                argument.value,
                contextual_bindings
                    .iter()
                    .map(|(owner, local)| (*owner, *local)),
            )? {
                return Err(PlanError::new(format!(
                    "remote call argument `{}` has unresolved contextual locals",
                    argument.name
                )));
            }
        }
        if !argument_ids.insert(argument.argument_id) {
            return Err(PlanError::new(format!(
                "remote call argument `{}` repeats a boundary argument ID",
                argument.name
            )));
        }
    }
    for arm in &call.invocation_arms {
        if !matches!(arm.trigger, ValueRef::Source(_) | ValueRef::State(_)) {
            return Err(PlanError::new(
                "distributed invocation arms require a SOURCE or state trigger",
            ));
        }
        if arena.is_some_and(|arena| {
            !distributed_invocation_gate_expression_is_safe(arena, arm.gate, constants)
        }) {
            return Err(PlanError::new(format!(
                "distributed invocation arm has an unsupported gate expression: {:?}",
                arm.gate
            )));
        }
    }
    Ok(())
}

fn distributed_invocation_gate_expression_is_safe(
    arena: &PlanRowExpressionArena,
    expression: PlanRowExpressionId,
    constants: Option<&[PlanConstant]>,
) -> bool {
    distributed_call_expression_is_safe(arena, expression, constants, true, &BTreeMap::new(), &[])
}

fn distributed_call_expression_is_safe(
    arena: &PlanRowExpressionArena,
    expression: PlanRowExpressionId,
    constants: Option<&[PlanConstant]>,
    allow_source_input: bool,
    contextual_bindings: &BTreeMap<PlanStaticOwnerId, PlanLocalId>,
    row_bindings: &[DistributedCallRowBindingPlan],
) -> bool {
    let Ok(order) = arena.walk_postorder(expression) else {
        return false;
    };
    let mut valid = BTreeMap::new();
    for id in order {
        let Ok(node) = arena.node(id) else {
            return false;
        };
        let child_is_valid =
            |child: &PlanRowExpressionId| valid.get(child).copied().unwrap_or(false);
        let node_is_valid = match node {
            PlanRowExpressionNode::Field { input } => {
                distributed_external_value_ref_is_supported(input)
                    || (allow_source_input
                        && matches!(input, ValueRef::Source(_) | ValueRef::SourcePayload { .. }))
            }
            PlanRowExpressionNode::Constant { constant_id } => constants.is_none_or(|constants| {
                constants.iter().any(|constant| constant.id == *constant_id)
            }),
            PlanRowExpressionNode::TextTrim { input }
            | PlanRowExpressionNode::TextIsEmpty { input }
            | PlanRowExpressionNode::TextLength { input }
            | PlanRowExpressionNode::TextToNumber { input }
            | PlanRowExpressionNode::BytesToHex { input }
            | PlanRowExpressionNode::BytesToBase64 { input }
            | PlanRowExpressionNode::BytesFromHex { input }
            | PlanRowExpressionNode::BytesFromBase64 { input }
            | PlanRowExpressionNode::BytesIsEmpty { input }
            | PlanRowExpressionNode::BytesLength { input } => child_is_valid(input),
            PlanRowExpressionNode::TextStartsWith { input, prefix }
            | PlanRowExpressionNode::BytesStartsWith { input, prefix } => {
                child_is_valid(input) && child_is_valid(prefix)
            }
            PlanRowExpressionNode::BytesEndsWith { input, suffix } => {
                child_is_valid(input) && child_is_valid(suffix)
            }
            PlanRowExpressionNode::BytesConcat { left, right }
            | PlanRowExpressionNode::BytesEqual { left, right }
            | PlanRowExpressionNode::NumberInfix { left, right, .. } => {
                child_is_valid(left) && child_is_valid(right)
            }
            PlanRowExpressionNode::TextSubstring {
                input,
                start,
                length,
            } => child_is_valid(input) && child_is_valid(start) && child_is_valid(length),
            PlanRowExpressionNode::TextToBytes { input, encoding }
            | PlanRowExpressionNode::BytesToText { input, encoding } => {
                child_is_valid(input) && encoding.as_ref().is_none_or(child_is_valid)
            }
            PlanRowExpressionNode::BytesGet { input, index } => {
                child_is_valid(input) && child_is_valid(index)
            }
            PlanRowExpressionNode::BytesSlice {
                input,
                offset,
                byte_count,
            } => child_is_valid(input) && child_is_valid(offset) && child_is_valid(byte_count),
            PlanRowExpressionNode::BytesTake { input, byte_count }
            | PlanRowExpressionNode::BytesDrop { input, byte_count } => {
                child_is_valid(input) && child_is_valid(byte_count)
            }
            PlanRowExpressionNode::BytesZeros { byte_count } => child_is_valid(byte_count),
            PlanRowExpressionNode::BytesReadUnsigned {
                input,
                offset,
                byte_count,
                endian,
            }
            | PlanRowExpressionNode::BytesReadSigned {
                input,
                offset,
                byte_count,
                endian,
            } => {
                child_is_valid(input)
                    && child_is_valid(offset)
                    && child_is_valid(byte_count)
                    && child_is_valid(endian)
            }
            PlanRowExpressionNode::BytesSet {
                input,
                index,
                value,
            } => child_is_valid(input) && child_is_valid(index) && child_is_valid(value),
            PlanRowExpressionNode::BytesWriteUnsigned {
                input,
                offset,
                byte_count,
                endian,
                value,
            }
            | PlanRowExpressionNode::BytesWriteSigned {
                input,
                offset,
                byte_count,
                endian,
                value,
            } => {
                child_is_valid(input)
                    && child_is_valid(offset)
                    && child_is_valid(byte_count)
                    && child_is_valid(endian)
                    && child_is_valid(value)
            }
            PlanRowExpressionNode::BytesFind { input, needle } => {
                child_is_valid(input) && child_is_valid(needle)
            }
            PlanRowExpressionNode::TextConcat { parts } => parts.iter().all(child_is_valid),
            PlanRowExpressionNode::Object { fields } => {
                fields.windows(2).all(|pair| pair[0].name < pair[1].name)
                    && fields.iter().all(|field| {
                        distributed_name_is_canonical(&field.name) && child_is_valid(&field.value)
                    })
            }
            PlanRowExpressionNode::TaggedObject { tag, fields } => {
                distributed_name_is_canonical(tag)
                    && fields.windows(2).all(|pair| pair[0].name < pair[1].name)
                    && fields.iter().all(|field| {
                        distributed_name_is_canonical(&field.name) && child_is_valid(&field.value)
                    })
            }
            PlanRowExpressionNode::ObjectField { object, field } => {
                distributed_name_is_canonical(field) && child_is_valid(object)
            }
            PlanRowExpressionNode::Select { input, arms } => {
                !arms.is_empty()
                    && child_is_valid(input)
                    && arms.iter().all(|arm| child_is_valid(&arm.value))
            }
            PlanRowExpressionNode::BuiltinCall {
                function,
                input,
                args,
            } => {
                function.validate_call(*input, args).is_ok()
                    && input.as_ref().is_none_or(child_is_valid)
                    && args.iter().all(|argument| child_is_valid(&argument.value))
            }
            PlanRowExpressionNode::Local { owner, local, .. }
            | PlanRowExpressionNode::LocalRow { owner, local } => {
                contextual_bindings.get(owner) == Some(local)
            }
            PlanRowExpressionNode::ListRowField { row, list_id, .. } => {
                child_is_valid(row) && row_bindings.iter().any(|binding| binding.list == *list_id)
            }
            PlanRowExpressionNode::Intrinsic { .. }
            | PlanRowExpressionNode::ListGetField { .. }
            | PlanRowExpressionNode::ListRef { .. }
            | PlanRowExpressionNode::AuthorityListRef { .. }
            | PlanRowExpressionNode::ListRange { .. }
            | PlanRowExpressionNode::ListLiteral { .. }
            | PlanRowExpressionNode::ContextualCollection { .. }
            | PlanRowExpressionNode::ContextualOrder { .. }
            | PlanRowExpressionNode::ListAccess { .. }
            | PlanRowExpressionNode::ListPage { .. }
            | PlanRowExpressionNode::BoundedListPage { .. }
            | PlanRowExpressionNode::EventRow { .. }
            | PlanRowExpressionNode::ListSum { .. } => false,
        };
        valid.insert(id, node_is_valid);
    }
    valid.get(&expression).copied().unwrap_or(false)
}

fn distributed_call_contextual_bindings(
    call: &RemoteCallSitePlan,
    arena: Option<&PlanRowExpressionArena>,
) -> Option<BTreeMap<PlanStaticOwnerId, PlanLocalId>> {
    let owner_lists = call
        .row_bindings
        .iter()
        .map(|binding| (binding.owner, binding.list))
        .collect::<BTreeMap<_, _>>();
    let bindings = call
        .row_bindings
        .iter()
        .map(|binding| (binding.owner, binding.local))
        .collect::<BTreeMap<_, _>>();
    if owner_lists.len() != call.row_bindings.len()
        || bindings.len() != call.row_bindings.len()
        || !call.row_bindings.windows(2).all(|pair| pair[0] < pair[1])
    {
        return None;
    }
    let Some(arena) = arena else {
        return Some(bindings);
    };
    let mut referenced = BTreeMap::new();
    for argument in &call.arguments {
        let mut valid = true;
        collect_contextual_local_bindings(
            arena,
            argument.value,
            &owner_lists,
            &mut referenced,
            &mut valid,
        );
        if !valid {
            return None;
        }
    }
    if referenced
        .iter()
        .any(|(owner, local)| bindings.get(owner) != Some(local))
    {
        return None;
    }
    Some(bindings)
}

fn collect_contextual_local_bindings(
    arena: &PlanRowExpressionArena,
    expression: PlanRowExpressionId,
    owner_lists: &BTreeMap<PlanStaticOwnerId, ListId>,
    bindings: &mut BTreeMap<PlanStaticOwnerId, PlanLocalId>,
    valid: &mut bool,
) {
    if !*valid {
        return;
    }
    let result = arena.visit(expression, &mut |_, node| {
        if let PlanRowExpressionNode::Local { owner, local, .. }
        | PlanRowExpressionNode::LocalRow { owner, local } = node
            && (!owner_lists.contains_key(owner)
                || bindings
                    .insert(*owner, *local)
                    .is_some_and(|existing| existing != *local))
        {
            *valid = false;
        }
    });
    *valid &= result.is_ok();
}

fn validate_distributed_endpoint_contract(
    graph: &DistributedGraphIdentityPlan,
    endpoint: &DistributedEndpointContractPlan,
    constants: Option<&[PlanConstant]>,
    arena: Option<&PlanRowExpressionArena>,
) -> Result<(), PlanError> {
    if endpoint.revision == 0
        || digest_is_zero(endpoint.stable_identity.as_bytes())
        || endpoint.endpoint_id
            != DistributedEndpointId::from_identity(
                graph.graph_id,
                endpoint.role,
                endpoint.stable_identity,
            )?
        || !endpoint
            .value_exports
            .windows(2)
            .all(|pair| pair[0].export_id < pair[1].export_id)
        || !endpoint
            .value_imports
            .windows(2)
            .all(|pair| pair[0].import_id < pair[1].import_id)
        || !endpoint
            .event_exports
            .windows(2)
            .all(|pair| pair[0].export_id < pair[1].export_id)
        || !endpoint
            .event_imports
            .windows(2)
            .all(|pair| pair[0].import_id < pair[1].import_id)
        || !endpoint
            .function_exports
            .windows(2)
            .all(|pair| pair[0].export_id < pair[1].export_id)
        || !endpoint
            .remote_call_sites
            .windows(2)
            .all(|pair| pair[0].call_site_id < pair[1].call_site_id)
    {
        return Err(PlanError::new(
            "distributed endpoint identity, revision, or member ordering is not canonical",
        ));
    }
    for export in &endpoint.value_exports {
        validate_distributed_value_export(
            graph.graph_id,
            endpoint.endpoint_id,
            endpoint.role,
            export,
        )?;
    }
    for import in &endpoint.value_imports {
        validate_distributed_value_import(
            graph.graph_id,
            endpoint.endpoint_id,
            endpoint.role,
            import,
        )?;
    }
    for export in &endpoint.event_exports {
        validate_distributed_event_export(
            graph.graph_id,
            endpoint.endpoint_id,
            endpoint.role,
            export,
        )?;
    }
    for import in &endpoint.event_imports {
        validate_distributed_event_import(
            graph.graph_id,
            endpoint.endpoint_id,
            endpoint.role,
            import,
        )?;
    }
    for export in &endpoint.function_exports {
        validate_distributed_function_export(
            graph.graph_id,
            endpoint.endpoint_id,
            endpoint.role,
            export,
        )?;
    }
    for call in &endpoint.remote_call_sites {
        validate_remote_call_site(
            graph.graph_id,
            endpoint.endpoint_id,
            endpoint.role,
            call,
            constants,
            arena,
        )?;
    }
    let mut export_ids = BTreeSet::new();
    if endpoint
        .value_exports
        .iter()
        .map(|export| export.export_id)
        .chain(
            endpoint
                .function_exports
                .iter()
                .map(|export| export.export_id),
        )
        .chain(endpoint.event_exports.iter().map(|export| export.export_id))
        .any(|id| !export_ids.insert(id))
    {
        return Err(PlanError::new("distributed endpoint repeats an export ID"));
    }
    let local_import_ids = endpoint
        .value_imports
        .iter()
        .map(|import| import.import_id)
        .chain(endpoint.event_imports.iter().map(|import| import.import_id))
        .chain(
            endpoint
                .remote_call_sites
                .iter()
                .filter_map(|call| call.result.current_import_id()),
        )
        .collect::<BTreeSet<_>>();
    let current_call_count = endpoint
        .remote_call_sites
        .iter()
        .filter(|call| call.mode == DistributedCallMode::Current)
        .count();
    if local_import_ids.len()
        != endpoint.value_imports.len()
            + endpoint.event_imports.len()
            + current_call_count
        || endpoint.value_exports.iter().any(|export| {
            matches!(export.value, ValueRef::DistributedImport(id) if !local_import_ids.contains(&id))
        })
        || arena.is_some_and(|arena| {
            !distributed_call_dependencies_are_acyclic(arena, &endpoint.remote_call_sites)
        })
    {
        return Err(PlanError::new(
            "distributed endpoint import refs must resolve uniquely without call-result cycles",
        ));
    }
    Ok(())
}

fn validate_distributed_graph_endpoints(
    graph: &DistributedGraphIdentityPlan,
    endpoints: &[DistributedEndpointContractPlan],
) -> Result<(), PlanError> {
    if endpoints.len() != 3
        || !endpoints.iter().map(|endpoint| endpoint.role).eq([
            ProgramRole::Client,
            ProgramRole::Session,
            ProgramRole::Server,
        ])
    {
        return Err(PlanError::new(
            "distributed graph requires one canonically ordered client, session, and server endpoint",
        ));
    }
    for endpoint in endpoints {
        validate_distributed_endpoint_contract(graph, endpoint, None, None)?;
    }
    let mut endpoint_ids = BTreeSet::new();
    let mut export_ids = BTreeSet::new();
    for endpoint in endpoints {
        if !endpoint_ids.insert(endpoint.endpoint_id)
            || endpoint
                .value_exports
                .iter()
                .map(|export| export.export_id)
                .chain(
                    endpoint
                        .function_exports
                        .iter()
                        .map(|export| export.export_id),
                )
                .chain(endpoint.event_exports.iter().map(|export| export.export_id))
                .any(|id| !export_ids.insert(id))
        {
            return Err(PlanError::new(
                "distributed graph repeats an endpoint or export ID",
            ));
        }
    }
    for endpoint in endpoints {
        for import in &endpoint.value_imports {
            let Some(source) = endpoints
                .iter()
                .flat_map(|endpoint| &endpoint.value_exports)
                .find(|source| source.export_id == import.source_export_id)
            else {
                return Err(PlanError::new(
                    "distributed value import references a missing value export",
                ));
            };
            if source.producer_role != import.producer_role
                || source.revision != import.source_revision
                || source.origin_scoped != import.source_origin_scoped
                || source.data_type != import.data_type
            {
                return Err(PlanError::new(
                    "distributed value import does not exactly match its export",
                ));
            }
        }
        for import in &endpoint.event_imports {
            let Some(source) = endpoints
                .iter()
                .flat_map(|endpoint| &endpoint.event_exports)
                .find(|source| source.export_id == import.source_export_id)
            else {
                return Err(PlanError::new(
                    "distributed event import references a missing event export",
                ));
            };
            if source.producer_role != import.producer_role
                || source.revision != import.source_revision
                || source.payload_field != import.payload_field
                || source.payload_type != import.payload_type
            {
                return Err(PlanError::new(
                    "distributed event import does not exactly match its export",
                ));
            }
        }
        for call in &endpoint.remote_call_sites {
            let Some(function) = endpoints
                .iter()
                .flat_map(|endpoint| &endpoint.function_exports)
                .find(|function| function.export_id == call.function_export_id)
            else {
                return Err(PlanError::new(
                    "remote call references a missing function export",
                ));
            };
            let arguments_match =
                call.arguments.len() == function.parameters.len()
                    && call.arguments.iter().zip(&function.parameters).all(
                        |(argument, parameter)| {
                            argument.argument_id == parameter.argument_id
                                && argument.name == parameter.name
                                && argument.data_type == parameter.data_type
                        },
                    );
            if function.producer_role != call.callee_role
                || function.revision != call.function_revision
                || function.result_type != call.result_type
                || !arguments_match
            {
                return Err(PlanError::new(
                    "remote call does not exactly match its function export",
                ));
            }
        }
    }
    Ok(())
}

fn distributed_wire_schema_projection(
    graph: &DistributedGraphIdentityPlan,
    endpoints: &[DistributedEndpointContractPlan],
) -> Result<DistributedWireSchemaPlan, PlanError> {
    let mut wire_endpoints = endpoints
        .iter()
        .map(|endpoint| DistributedWireEndpointPlan {
            endpoint_id: endpoint.endpoint_id,
            role: endpoint.role,
        })
        .collect::<Vec<_>>();
    wire_endpoints.sort_by_key(|endpoint| endpoint.role);

    let mut value_edges = endpoints
        .iter()
        .flat_map(|endpoint| &endpoint.value_imports)
        .map(|import| DistributedWireValueEdgePlan {
            export_id: import.source_export_id,
            import_id: import.import_id,
            producer_role: import.producer_role,
            consumer_role: import.consumer_role,
            scope: import.scope,
            data_type: import.data_type.clone(),
        })
        .collect::<Vec<_>>();
    value_edges.sort_by_key(|edge| edge.import_id);

    let mut event_edges = endpoints
        .iter()
        .flat_map(|endpoint| &endpoint.event_imports)
        .map(|import| DistributedWireEventEdgePlan {
            export_id: import.source_export_id,
            import_id: import.import_id,
            producer_role: import.producer_role,
            consumer_role: import.consumer_role,
            scope: import.scope,
            payload_field: import.payload_field.clone(),
            payload_type: import.payload_type.clone(),
        })
        .collect::<Vec<_>>();
    event_edges.sort_by_key(|edge| edge.import_id);

    let mut call_edges = Vec::new();
    for call in endpoints
        .iter()
        .flat_map(|endpoint| &endpoint.remote_call_sites)
    {
        let function = endpoints
            .iter()
            .flat_map(|endpoint| &endpoint.function_exports)
            .find(|function| function.export_id == call.function_export_id)
            .ok_or_else(|| {
                PlanError::new("cannot project a remote call with no linked function export")
            })?;
        call_edges.push(DistributedWireCallEdgePlan {
            call_site_id: call.call_site_id,
            caller_role: call.caller_role,
            callee_role: call.callee_role,
            scope: call.scope,
            function_export_id: call.function_export_id,
            mode: call.mode,
            parameters: function.parameters.clone(),
            result_type: function.result_type.clone(),
        });
    }
    call_edges.sort_by_key(|edge| edge.call_site_id);

    Ok(DistributedWireSchemaPlan {
        graph_id: graph.graph_id,
        endpoints: wire_endpoints,
        value_edges,
        event_edges,
        call_edges,
    })
}

fn validate_distributed_wire_schema(
    graph: &DistributedGraphIdentityPlan,
    schema: &DistributedWireSchemaPlan,
) -> Result<(), PlanError> {
    if schema.graph_id != graph.graph_id
        || schema.endpoints.len() != 3
        || !schema.endpoints.iter().map(|endpoint| endpoint.role).eq([
            ProgramRole::Client,
            ProgramRole::Session,
            ProgramRole::Server,
        ])
        || !schema
            .value_edges
            .windows(2)
            .all(|pair| pair[0].import_id < pair[1].import_id)
        || !schema
            .event_edges
            .windows(2)
            .all(|pair| pair[0].import_id < pair[1].import_id)
        || !schema
            .call_edges
            .windows(2)
            .all(|pair| pair[0].call_site_id < pair[1].call_site_id)
    {
        return Err(PlanError::new(
            "distributed wire schema graph, endpoints, or edge ordering is not canonical",
        ));
    }

    let mut endpoint_ids = BTreeSet::new();
    for endpoint in &schema.endpoints {
        if digest_is_zero(endpoint.endpoint_id.as_bytes())
            || !endpoint_ids.insert(endpoint.endpoint_id)
        {
            return Err(PlanError::new(
                "distributed wire schema endpoint IDs must be nonzero and unique",
            ));
        }
    }
    let has_role = |role| {
        schema
            .endpoints
            .iter()
            .any(|endpoint| endpoint.role == role)
    };
    let mut import_ids = BTreeSet::new();
    for edge in &schema.value_edges {
        let producer_origin_scoped = edge.producer_role == ProgramRole::Server
            && edge.scope == DistributedRouteScopePlan::OriginScoped;
        if digest_is_zero(edge.export_id.as_bytes())
            || digest_is_zero(edge.import_id.as_bytes())
            || !has_role(edge.producer_role)
            || !has_role(edge.consumer_role)
            || edge.scope
                != distributed_value_route_scope(
                    edge.consumer_role,
                    edge.producer_role,
                    producer_origin_scoped,
                )?
            || !distributed_data_type_is_supported(&edge.data_type)
            || !import_ids.insert(edge.import_id)
        {
            return Err(PlanError::new(
                "distributed wire value edge has a noncanonical ID, role, scope, or type",
            ));
        }
    }
    for edge in &schema.event_edges {
        if digest_is_zero(edge.export_id.as_bytes())
            || digest_is_zero(edge.import_id.as_bytes())
            || !has_role(edge.producer_role)
            || !has_role(edge.consumer_role)
            || edge.scope
                != distributed_event_route_scope(edge.consumer_role, edge.producer_role)?
            || edge.payload_field.as_ref().is_some_and(|field| {
                matches!(field, SourcePayloadField::Named(name) if !distributed_name_is_canonical(name))
            })
            || !distributed_data_type_is_supported(&edge.payload_type)
            || !import_ids.insert(edge.import_id)
        {
            return Err(PlanError::new(
                "distributed wire event edge has a noncanonical ID, role, scope, payload, or type",
            ));
        }
    }
    for edge in &schema.call_edges {
        if digest_is_zero(edge.call_site_id.as_bytes())
            || digest_is_zero(edge.function_export_id.as_bytes())
            || !has_role(edge.caller_role)
            || !has_role(edge.callee_role)
            || edge.scope != distributed_call_route_scope(edge.caller_role, edge.callee_role)?
            || !distributed_data_type_is_supported(&edge.result_type)
            || !edge
                .parameters
                .windows(2)
                .all(|pair| pair[0].name < pair[1].name)
        {
            return Err(PlanError::new(
                "distributed wire call edge has a noncanonical ID, role, scope, result, or parameter order",
            ));
        }
        let mut argument_ids = BTreeSet::new();
        for parameter in &edge.parameters {
            if !distributed_name_is_canonical(&parameter.name)
                || parameter.argument_id
                    != DistributedArgumentId::from_parameter_name(
                        edge.function_export_id,
                        &parameter.name,
                    )?
                || !distributed_data_type_is_supported(&parameter.data_type)
                || !argument_ids.insert(parameter.argument_id)
            {
                return Err(PlanError::new(
                    "distributed wire call parameters must have canonical names, IDs, and types",
                ));
            }
        }
    }
    Ok(())
}

fn distributed_wire_value_edge_matches_import(
    edge: &DistributedWireValueEdgePlan,
    import: &DistributedValueImportPlan,
) -> bool {
    edge.export_id == import.source_export_id
        && edge.import_id == import.import_id
        && edge.producer_role == import.producer_role
        && edge.consumer_role == import.consumer_role
        && edge.scope == import.scope
        && edge.data_type == import.data_type
}

fn distributed_wire_value_edge_matches_export(
    edge: &DistributedWireValueEdgePlan,
    export: &DistributedValueExportPlan,
) -> bool {
    edge.export_id == export.export_id
        && edge.producer_role == export.producer_role
        && edge.data_type == export.data_type
        && distributed_value_route_scope(
            edge.consumer_role,
            export.producer_role,
            export.origin_scoped,
        )
        .is_ok_and(|scope| scope == edge.scope)
}

fn distributed_wire_event_edge_matches_import(
    edge: &DistributedWireEventEdgePlan,
    import: &DistributedEventImportPlan,
) -> bool {
    edge.export_id == import.source_export_id
        && edge.import_id == import.import_id
        && edge.producer_role == import.producer_role
        && edge.consumer_role == import.consumer_role
        && edge.scope == import.scope
        && edge.payload_field == import.payload_field
        && edge.payload_type == import.payload_type
}

fn distributed_wire_event_edge_matches_export(
    edge: &DistributedWireEventEdgePlan,
    export: &DistributedEventExportPlan,
) -> bool {
    edge.export_id == export.export_id
        && edge.producer_role == export.producer_role
        && edge.payload_field == export.payload_field
        && edge.payload_type == export.payload_type
        && distributed_event_route_scope(edge.consumer_role, export.producer_role)
            .is_ok_and(|scope| scope == edge.scope)
}

fn distributed_wire_call_edge_matches_site(
    edge: &DistributedWireCallEdgePlan,
    call: &RemoteCallSitePlan,
) -> bool {
    edge.call_site_id == call.call_site_id
        && edge.caller_role == call.caller_role
        && edge.callee_role == call.callee_role
        && edge.scope == call.scope
        && edge.function_export_id == call.function_export_id
        && edge.mode == call.mode
        && edge.result_type == call.result_type
        && edge.parameters.len() == call.arguments.len()
        && edge
            .parameters
            .iter()
            .zip(&call.arguments)
            .all(|(parameter, argument)| {
                parameter.argument_id == argument.argument_id
                    && parameter.name == argument.name
                    && parameter.data_type == argument.data_type
            })
}

fn distributed_wire_call_edge_matches_function(
    edge: &DistributedWireCallEdgePlan,
    function: &DistributedFunctionExportPlan,
) -> bool {
    edge.function_export_id == function.export_id
        && edge.callee_role == function.producer_role
        && edge.parameters == function.parameters
        && edge.result_type == function.result_type
}

fn validate_distributed_endpoint_wire_contract(
    endpoint: &DistributedEndpointContractPlan,
    schema: &DistributedWireSchemaPlan,
) -> Result<(), PlanError> {
    if !schema.endpoints.iter().any(|wire_endpoint| {
        wire_endpoint.role == endpoint.role && wire_endpoint.endpoint_id == endpoint.endpoint_id
    }) {
        return Err(PlanError::new(
            "distributed endpoint is not present in its linked wire schema",
        ));
    }

    let value_contract_matches = schema.value_edges.iter().all(|edge| {
        (edge.consumer_role != endpoint.role
            || endpoint
                .value_imports
                .iter()
                .any(|import| distributed_wire_value_edge_matches_import(edge, import)))
            && (edge.producer_role != endpoint.role
                || endpoint
                    .value_exports
                    .iter()
                    .any(|export| distributed_wire_value_edge_matches_export(edge, export)))
    }) && endpoint.value_imports.iter().all(|import| {
        schema
            .value_edges
            .iter()
            .any(|edge| distributed_wire_value_edge_matches_import(edge, import))
    });
    let event_contract_matches = schema.event_edges.iter().all(|edge| {
        (edge.consumer_role != endpoint.role
            || endpoint
                .event_imports
                .iter()
                .any(|import| distributed_wire_event_edge_matches_import(edge, import)))
            && (edge.producer_role != endpoint.role
                || endpoint
                    .event_exports
                    .iter()
                    .any(|export| distributed_wire_event_edge_matches_export(edge, export)))
    }) && endpoint.event_imports.iter().all(|import| {
        schema
            .event_edges
            .iter()
            .any(|edge| distributed_wire_event_edge_matches_import(edge, import))
    });
    let call_contract_matches = schema.call_edges.iter().all(|edge| {
        (edge.caller_role != endpoint.role
            || endpoint
                .remote_call_sites
                .iter()
                .any(|call| distributed_wire_call_edge_matches_site(edge, call)))
            && (edge.callee_role != endpoint.role
                || endpoint
                    .function_exports
                    .iter()
                    .any(|function| distributed_wire_call_edge_matches_function(edge, function)))
    }) && endpoint.remote_call_sites.iter().all(|call| {
        schema
            .call_edges
            .iter()
            .any(|edge| distributed_wire_call_edge_matches_site(edge, call))
    });

    if !value_contract_matches || !event_contract_matches || !call_contract_matches {
        return Err(PlanError::new(
            "distributed endpoint executable contract does not match its linked wire routes",
        ));
    }
    Ok(())
}

fn distributed_call_dependencies_are_acyclic(
    arena: &PlanRowExpressionArena,
    calls: &[RemoteCallSitePlan],
) -> bool {
    fn visit(
        arena: &PlanRowExpressionArena,
        call: &RemoteCallSitePlan,
        calls: &[RemoteCallSitePlan],
        visiting: &mut BTreeSet<RemoteCallSiteId>,
        visited: &mut BTreeSet<RemoteCallSiteId>,
    ) -> bool {
        if visited.contains(&call.call_site_id) {
            return true;
        }
        if !visiting.insert(call.call_site_id) {
            return false;
        }
        for import_id in call
            .arguments
            .iter()
            .flat_map(|argument| distributed_expression_import_ids(arena, argument.value))
        {
            if let Some(dependency) = calls
                .iter()
                .find(|candidate| candidate.result.current_import_id() == Some(import_id))
                && !visit(arena, dependency, calls, visiting, visited)
            {
                return false;
            }
        }
        visiting.remove(&call.call_site_id);
        visited.insert(call.call_site_id);
        true
    }

    let mut visiting = BTreeSet::new();
    let mut visited = BTreeSet::new();
    calls
        .iter()
        .all(|call| visit(arena, call, calls, &mut visiting, &mut visited))
}

fn distributed_expression_import_ids(
    arena: &PlanRowExpressionArena,
    expression: PlanRowExpressionId,
) -> BTreeSet<ImportId> {
    let mut imports = BTreeSet::new();
    let _ = arena.visit_value_refs(expression, &mut |value_ref| {
        if let ValueRef::DistributedImport(import_id) = value_ref {
            imports.insert(*import_id);
        }
    });
    imports
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct EffectContract {
    pub effect_id: EffectId,
    pub host_operation: String,
    pub replay: EffectReplay,
    pub barrier: EffectBarrier,
    pub result_policy: EffectResultPolicy,
    #[serde(default, skip_serializing_if = "EffectDeliveryCardinality::is_single")]
    pub delivery: EffectDeliveryCardinality,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<EffectSchemaPlan>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct EffectSchemaPlan {
    pub intent_type: DataTypePlan,
    pub result_type: DataTypePlan,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub intent_constraints: Vec<EffectIntentConstraintPlan>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub intent_defaults: Vec<EffectIntentDefaultPlan>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct EffectIntentDefaultPlan {
    pub field_name: String,
    pub value: EffectIntentDefaultValuePlan,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EffectIntentDefaultValuePlan {
    Bool { value: bool },
    Number { value: FiniteReal },
    Text { value: String },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EffectIntentConstraintPlan {
    UnsignedIntegerRange {
        field_path: Vec<String>,
        min_inclusive: u64,
        max_inclusive: u64,
    },
    BytesLengthRange {
        field_path: Vec<String>,
        min_inclusive: u64,
        max_inclusive: u64,
    },
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EffectDeliveryCardinality {
    #[default]
    Single,
    Stream {
        initial_credits: u32,
        max_in_flight: u32,
        credit_result_tags: Vec<String>,
        terminal_result_tags: Vec<String>,
    },
}

impl EffectDeliveryCardinality {
    fn is_single(&self) -> bool {
        matches!(self, Self::Single)
    }
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
            delivery: EffectDeliveryCardinality::Single,
            schema: None,
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
        if let Some(schema) = &self.schema
            && [&schema.intent_type, &schema.result_type]
                .into_iter()
                .any(|data_type| !data_type.is_canonical() || data_type_contains_unknown(data_type))
        {
            return Err(PlanError::new(
                "effect schemas must use canonical closed data types",
            ));
        }
        if let Some(schema) = &self.schema {
            validate_effect_intent_constraints(schema)?;
            validate_effect_intent_defaults(schema)?;
        }
        if let EffectDeliveryCardinality::Stream {
            initial_credits,
            max_in_flight,
            credit_result_tags,
            terminal_result_tags,
        } = &self.delivery
        {
            if !matches!(
                self.replay,
                EffectReplay::ReadOnly | EffectReplay::ProcessScoped
            ) || self.barrier != EffectBarrier::None
                || self.result_policy != EffectResultPolicy::ReturnValue
            {
                return Err(PlanError::new(
                    "stream effects must be process-local, barrier-free return-value effects",
                ));
            }
            if *initial_credits == 0
                || *initial_credits > boon_effect_schema::MAX_STREAM_INITIAL_CREDITS
                || *max_in_flight == 0
                || *max_in_flight > boon_effect_schema::MAX_STREAM_IN_FLIGHT
                || initial_credits > max_in_flight
            {
                return Err(PlanError::new(
                    "stream effect credit limits must be nonzero, bounded, and ordered",
                ));
            }
            if credit_result_tags.is_empty()
                || credit_result_tags.windows(2).any(|pair| pair[0] >= pair[1])
                || credit_result_tags
                    .iter()
                    .any(|tag| tag.trim().is_empty() || tag.trim() != tag)
            {
                return Err(PlanError::new(
                    "stream credit result tags must be nonempty, canonical, unique, and ordered",
                ));
            }
            if terminal_result_tags.is_empty()
                || terminal_result_tags
                    .windows(2)
                    .any(|pair| pair[0] >= pair[1])
                || terminal_result_tags
                    .iter()
                    .any(|tag| tag.trim().is_empty() || tag.trim() != tag)
            {
                return Err(PlanError::new(
                    "stream terminal result tags must be nonempty, canonical, unique, and ordered",
                ));
            }
            let Some(EffectSchemaPlan {
                result_type: DataTypePlan::Variant { variants },
                ..
            }) = &self.schema
            else {
                return Err(PlanError::new(
                    "stream effects require a closed variant result schema",
                ));
            };
            if variants.iter().any(|variant| variant.open)
                || credit_result_tags
                    .iter()
                    .any(|credit| !variants.iter().any(|variant| variant.tag == *credit))
            {
                return Err(PlanError::new(
                    "stream credit result tags must exist in the closed result schema",
                ));
            }
            if terminal_result_tags
                .iter()
                .any(|terminal| !variants.iter().any(|variant| variant.tag == *terminal))
            {
                return Err(PlanError::new(
                    "stream terminal result tags must exist in the closed result schema",
                ));
            }
            if credit_result_tags
                .iter()
                .any(|credit| terminal_result_tags.contains(credit))
            {
                return Err(PlanError::new(
                    "stream credit and terminal result tags must be disjoint",
                ));
            }
            if terminal_result_tags.len() == variants.len() {
                return Err(PlanError::new(
                    "stream effects require at least one nonterminal result variant",
                ));
            }
        }
        match (&self.replay, self.barrier, self.result_policy) {
            (EffectReplay::ReadOnly, EffectBarrier::None, _)
            | (EffectReplay::ProcessScoped, EffectBarrier::None, _)
            | (EffectReplay::NonReplayable, EffectBarrier::None, EffectResultPolicy::Discarded)
            | (EffectReplay::Idempotent { .. }, EffectBarrier::Before, _)
            | (EffectReplay::Idempotent { .. }, EffectBarrier::BeforeAndAfter, _) => Ok(()),
            (EffectReplay::ReadOnly, _, _) => Err(PlanError::new(
                "read-only effects cannot require a persistence barrier",
            )),
            (EffectReplay::ProcessScoped, _, _) => Err(PlanError::new(
                "process-scoped effects cannot require a persistence barrier",
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

fn validate_effect_intent_constraints(schema: &EffectSchemaPlan) -> Result<(), PlanError> {
    let mut previous_path: Option<&[String]> = None;
    for constraint in &schema.intent_constraints {
        let (field_path, min_inclusive, max_inclusive, expected_type, error) = match constraint {
            EffectIntentConstraintPlan::UnsignedIntegerRange {
                field_path,
                min_inclusive,
                max_inclusive,
            } => (
                field_path,
                min_inclusive,
                max_inclusive,
                DataTypePlan::Number,
                "unsigned integer constraints must target numeric intent fields",
            ),
            EffectIntentConstraintPlan::BytesLengthRange {
                field_path,
                min_inclusive,
                max_inclusive,
            } => (
                field_path,
                min_inclusive,
                max_inclusive,
                DataTypePlan::Bytes { fixed_len: None },
                "byte length constraints must target Bytes intent fields",
            ),
        };
        if field_path.is_empty()
            || field_path
                .iter()
                .any(|part| part.trim().is_empty() || part.trim() != part)
            || min_inclusive > max_inclusive
        {
            return Err(PlanError::new(
                "effect intent constraints must have a canonical field path and valid range",
            ));
        }
        if previous_path.is_some_and(|previous| previous >= field_path.as_slice()) {
            return Err(PlanError::new(
                "effect intent constraints must be uniquely ordered by field path",
            ));
        }
        previous_path = Some(field_path);
        let type_matches = matches!(
            (
                data_type_at_record_path(&schema.intent_type, field_path),
                expected_type
            ),
            (Some(DataTypePlan::Number), DataTypePlan::Number)
                | (Some(DataTypePlan::Bytes { .. }), DataTypePlan::Bytes { .. })
        );
        if !type_matches {
            return Err(PlanError::new(error));
        }
    }
    Ok(())
}

fn validate_effect_intent_defaults(schema: &EffectSchemaPlan) -> Result<(), PlanError> {
    if schema
        .intent_defaults
        .windows(2)
        .any(|pair| pair[0].field_name >= pair[1].field_name)
    {
        return Err(PlanError::new(
            "effect intent defaults must be uniquely ordered by field name",
        ));
    }
    for default in &schema.intent_defaults {
        if default.field_name.trim().is_empty() || default.field_name.trim() != default.field_name {
            return Err(PlanError::new(
                "effect intent default field names must be canonical",
            ));
        }
        let Some(field_type) = data_type_at_record_path(
            &schema.intent_type,
            std::slice::from_ref(&default.field_name),
        ) else {
            return Err(PlanError::new(
                "effect intent default field must exist in the intent schema",
            ));
        };
        let type_matches = matches!(
            (&default.value, field_type),
            (
                EffectIntentDefaultValuePlan::Bool { .. },
                DataTypePlan::Bool
            ) | (
                EffectIntentDefaultValuePlan::Number { .. },
                DataTypePlan::Number
            ) | (
                EffectIntentDefaultValuePlan::Text { .. },
                DataTypePlan::Text
            )
        );
        if !type_matches {
            return Err(PlanError::new(
                "effect intent default value must match its field type",
            ));
        }
        for constraint in &schema.intent_constraints {
            let EffectIntentConstraintPlan::UnsignedIntegerRange {
                field_path,
                min_inclusive,
                max_inclusive,
            } = constraint
            else {
                continue;
            };
            if field_path.as_slice() == [default.field_name.as_str()]
                && let EffectIntentDefaultValuePlan::Number { value } = default.value
            {
                let Ok(value) = value.to_i64_exact() else {
                    return Err(PlanError::new(
                        "effect intent default violates its unsigned integer constraint",
                    ));
                };
                let Ok(value) = u64::try_from(value) else {
                    return Err(PlanError::new(
                        "effect intent default violates its unsigned integer constraint",
                    ));
                };
                if value < *min_inclusive || value > *max_inclusive {
                    return Err(PlanError::new(
                        "effect intent default violates its unsigned integer constraint",
                    ));
                }
            }
        }
    }
    Ok(())
}

fn data_type_at_record_path<'a>(
    root: &'a DataTypePlan,
    field_path: &[String],
) -> Option<&'a DataTypePlan> {
    field_path.iter().try_fold(root, |data_type, part| {
        let DataTypePlan::Record {
            fields,
            open: false,
        } = data_type
        else {
            return None;
        };
        fields
            .iter()
            .find(|field| field.name == *part)
            .map(|field| &field.data_type)
    })
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EffectReplay {
    ReadOnly,
    ProcessScoped,
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
    Discarded,
}

pub fn builtin_effect_contract(host_operation: &str) -> Result<Option<EffectContract>, PlanError> {
    let Some(spec) = boon_effect_schema::host_effect_spec(host_operation) else {
        return Ok(None);
    };
    spec.validate().map_err(PlanError::new)?;
    let replay = match spec.replay {
        boon_effect_schema::ReplaySpec::ReadOnly => EffectReplay::ReadOnly,
        boon_effect_schema::ReplaySpec::ProcessScoped => EffectReplay::ProcessScoped,
        boon_effect_schema::ReplaySpec::IdempotentBytesKey => EffectReplay::Idempotent {
            key_type: DataTypePlan::Bytes {
                fixed_len: Some(32),
            },
        },
        boon_effect_schema::ReplaySpec::NonReplayable => EffectReplay::NonReplayable,
    };
    let schema = spec
        .schema
        .as_ref()
        .map(effect_schema_to_plan)
        .transpose()?;
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
            boon_effect_schema::ResultPolicySpec::Discarded => EffectResultPolicy::Discarded,
        },
        delivery: match &spec.delivery {
            boon_effect_schema::DeliveryCardinalitySpec::Single => {
                EffectDeliveryCardinality::Single
            }
            boon_effect_schema::DeliveryCardinalitySpec::Stream {
                initial_credits,
                max_in_flight,
                credit_result_tags,
                terminal_result_tags,
            } => EffectDeliveryCardinality::Stream {
                initial_credits: *initial_credits,
                max_in_flight: *max_in_flight,
                credit_result_tags: credit_result_tags
                    .iter()
                    .map(|tag| (*tag).to_owned())
                    .collect(),
                terminal_result_tags: terminal_result_tags
                    .iter()
                    .map(|tag| (*tag).to_owned())
                    .collect(),
            },
        },
        schema,
    };
    contract.validate()?;
    Ok(Some(contract))
}

fn effect_schema_to_plan(
    schema: &boon_effect_schema::EffectSchema,
) -> Result<EffectSchemaPlan, PlanError> {
    let intent_defaults = schema
        .intent_defaults
        .iter()
        .map(|default| {
            let value = match default.value {
                boon_effect_schema::IntentDefaultValueSpec::Bool(value) => {
                    EffectIntentDefaultValuePlan::Bool { value }
                }
                boon_effect_schema::IntentDefaultValueSpec::ExactInteger(value) => {
                    EffectIntentDefaultValuePlan::Number {
                        value: FiniteReal::from_i64_exact(value)
                            .map_err(|error| PlanError::new(error.to_string()))?,
                    }
                }
                boon_effect_schema::IntentDefaultValueSpec::Text(value) => {
                    EffectIntentDefaultValuePlan::Text {
                        value: value.to_owned(),
                    }
                }
            };
            Ok(EffectIntentDefaultPlan {
                field_name: default.field_name.to_owned(),
                value,
            })
        })
        .collect::<Result<Vec<_>, PlanError>>()?;
    Ok(EffectSchemaPlan {
        intent_type: effect_schema_type_to_plan(&schema.intent).canonicalized(),
        result_type: effect_schema_type_to_plan(&schema.result).canonicalized(),
        intent_constraints: schema
            .intent_constraints
            .iter()
            .map(|constraint| match constraint {
                boon_effect_schema::IntentConstraintSpec::UnsignedIntegerRange {
                    field_path,
                    min_inclusive,
                    max_inclusive,
                } => EffectIntentConstraintPlan::UnsignedIntegerRange {
                    field_path: field_path.iter().map(|part| (*part).to_owned()).collect(),
                    min_inclusive: *min_inclusive,
                    max_inclusive: *max_inclusive,
                },
                boon_effect_schema::IntentConstraintSpec::BytesLengthRange {
                    field_path,
                    min_inclusive,
                    max_inclusive,
                } => EffectIntentConstraintPlan::BytesLengthRange {
                    field_path: field_path.iter().map(|part| (*part).to_owned()).collect(),
                    min_inclusive: *min_inclusive,
                    max_inclusive: *max_inclusive,
                },
            })
            .collect(),
        intent_defaults,
    })
}

pub fn builtin_effect_outbox_schema(
    host_operation: &str,
) -> Result<Option<EffectOutboxSchema>, PlanError> {
    let Some(spec) = boon_effect_schema::host_effect_spec(host_operation) else {
        return Ok(None);
    };
    let contract = builtin_effect_contract(host_operation)?.ok_or_else(|| {
        PlanError::new(format!(
            "effect outbox schema `{host_operation}` has no built-in contract"
        ))
    })?;
    let EffectReplay::Idempotent { key_type } = contract.replay else {
        return Ok(None);
    };
    let Some(schema) = spec.schema else {
        return Err(PlanError::new(format!(
            "idempotent effect `{host_operation}` has no typed schema"
        )));
    };
    let intent_type = effect_schema_type_to_plan(&schema.intent);
    let result_type = effect_schema_type_to_plan(&schema.result);
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
        boon_effect_schema::ValueType::List { item } => DataTypePlan::List {
            item: Box::new(effect_schema_type_to_plan(item)),
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
    pub name: String,
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
    pub pattern: PlanRowSelectPattern,
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
    TextConcat {
        parts: Vec<MigrationExpressionPlan>,
    },
    Number {
        value: FiniteReal,
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
            if arguments.iter().any(|argument| argument.name.is_empty()) {
                return Err(PlanError::new(
                    "migration call argument names must not be empty",
                ));
            }
            if arguments
                .iter()
                .map(|argument| argument.name.as_str())
                .collect::<BTreeSet<_>>()
                .len()
                != arguments.len()
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
        MigrationExpressionPlan::TextConcat { parts: items }
        | MigrationExpressionPlan::List { items }
        | MigrationExpressionPlan::Bytes { items } => {
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
        MigrationExpressionPlan::TextConcat { parts: items }
        | MigrationExpressionPlan::List { items }
        | MigrationExpressionPlan::Bytes { items } => {
            for item in items {
                validate_migration_expression(item, inputs, parameter_depth, used_inputs)?;
            }
        }
        MigrationExpressionPlan::Match { input, arms } => {
            if arms.is_empty() {
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
            | "List/range"
            | "List/chunk"
            | "List/get"
            | "List/count"
            | "List/length"
            | "List/sum"
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
    target_path: &'a str,
}

impl EffectInvocationId {
    pub fn from_result_owner(effect_id: EffectId, target_path: &str) -> Result<Self, PlanError> {
        if target_path.trim().is_empty() || target_path.trim() != target_path {
            return Err(PlanError::new(
                "effect invocation target path must be non-empty and canonical",
            ));
        }
        Ok(Self(canonical_sha256(&EffectInvocationIdentityInput {
            namespace: "boon.effect-result-owner.v2",
            effect_id,
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

#[derive(Serialize)]
struct DistributedDeclarationIdentityInput<'a> {
    namespace: &'static str,
    canonical_module: &'a str,
    semantic_path: &'a str,
}

impl DistributedDeclarationId {
    pub fn from_semantic_path(
        canonical_module: &str,
        semantic_path: &str,
    ) -> Result<Self, PlanError> {
        if !distributed_name_is_canonical(canonical_module)
            || !distributed_name_is_canonical(semantic_path)
        {
            return Err(PlanError::new(
                "distributed declaration identity components must be non-empty canonical names",
            ));
        }
        Ok(Self(canonical_sha256(
            &DistributedDeclarationIdentityInput {
                namespace: "boon.distributed-declaration.v1",
                canonical_module,
                semantic_path,
            },
        )?))
    }
}

#[derive(Serialize)]
struct DistributedGraphIdentityInput<'a> {
    namespace: &'static str,
    package_id: &'a str,
    deployment_domain: &'a str,
    stable_identity: DistributedDeclarationId,
}

impl DistributedGraphId {
    pub fn from_identity(
        application: &ApplicationIdentity,
        stable_identity: DistributedDeclarationId,
    ) -> Result<Self, PlanError> {
        if !application.is_valid() || digest_is_zero(stable_identity.as_bytes()) {
            return Err(PlanError::new(
                "distributed graph identity requires a valid application and declaration",
            ));
        }
        Ok(Self(canonical_sha256(&DistributedGraphIdentityInput {
            namespace: "boon.distributed-graph.v1",
            package_id: &application.package_id,
            deployment_domain: &application.deployment_domain,
            stable_identity,
        })?))
    }
}

#[derive(Serialize)]
struct DistributedEndpointIdentityInput {
    namespace: &'static str,
    graph_id: DistributedGraphId,
    role: ProgramRole,
    stable_identity: DistributedDeclarationId,
}

impl DistributedEndpointId {
    pub fn from_identity(
        graph_id: DistributedGraphId,
        role: ProgramRole,
        stable_identity: DistributedDeclarationId,
    ) -> Result<Self, PlanError> {
        if digest_is_zero(graph_id.as_bytes()) || digest_is_zero(stable_identity.as_bytes()) {
            return Err(PlanError::new(
                "distributed endpoint identity requires nonzero graph and declaration IDs",
            ));
        }
        Ok(Self(canonical_sha256(&DistributedEndpointIdentityInput {
            namespace: "boon.distributed-endpoint.v1",
            graph_id,
            role,
            stable_identity,
        })?))
    }
}

#[derive(Serialize)]
struct DistributedExportIdentityInput {
    namespace: &'static str,
    graph_id: DistributedGraphId,
    endpoint_id: DistributedEndpointId,
    kind: DistributedExportKind,
    stable_identity: DistributedDeclarationId,
}

impl ExportId {
    pub fn from_identity(
        graph_id: DistributedGraphId,
        endpoint_id: DistributedEndpointId,
        kind: DistributedExportKind,
        stable_identity: DistributedDeclarationId,
    ) -> Result<Self, PlanError> {
        Ok(Self(canonical_sha256(&DistributedExportIdentityInput {
            namespace: "boon.distributed-export.v1",
            graph_id,
            endpoint_id,
            kind,
            stable_identity,
        })?))
    }
}

#[derive(Serialize)]
struct DistributedImportIdentityInput {
    namespace: &'static str,
    graph_id: DistributedGraphId,
    endpoint_id: DistributedEndpointId,
    stable_identity: DistributedDeclarationId,
}

impl ImportId {
    pub fn from_value_identity(
        graph_id: DistributedGraphId,
        endpoint_id: DistributedEndpointId,
        stable_identity: DistributedDeclarationId,
    ) -> Result<Self, PlanError> {
        Ok(Self(canonical_sha256(&DistributedImportIdentityInput {
            namespace: "boon.distributed-value-import.v1",
            graph_id,
            endpoint_id,
            stable_identity,
        })?))
    }

    pub fn from_event_identity(
        graph_id: DistributedGraphId,
        endpoint_id: DistributedEndpointId,
        stable_identity: DistributedDeclarationId,
    ) -> Result<Self, PlanError> {
        Ok(Self(canonical_sha256(&DistributedImportIdentityInput {
            namespace: "boon.distributed-event-import.v1",
            graph_id,
            endpoint_id,
            stable_identity,
        })?))
    }

    pub fn from_remote_call_result(call_site_id: RemoteCallSiteId) -> Result<Self, PlanError> {
        #[derive(Serialize)]
        struct Input {
            namespace: &'static str,
            call_site_id: RemoteCallSiteId,
        }
        Ok(Self(canonical_sha256(&Input {
            namespace: "boon.distributed-call-result.v1",
            call_site_id,
        })?))
    }

    pub fn from_producer_argument(
        call_site_id: RemoteCallSiteId,
        argument_id: DistributedArgumentId,
    ) -> Result<Self, PlanError> {
        #[derive(Serialize)]
        struct Input {
            namespace: &'static str,
            call_site_id: RemoteCallSiteId,
            argument_id: DistributedArgumentId,
        }
        if digest_is_zero(call_site_id.as_bytes()) || digest_is_zero(argument_id.as_bytes()) {
            return Err(PlanError::new(
                "producer argument import identity requires nonzero call-site and argument IDs",
            ));
        }
        Ok(Self(canonical_sha256(&Input {
            namespace: "boon.producer-function-argument-import.v1",
            call_site_id,
            argument_id,
        })?))
    }
}

impl DistributedArgumentId {
    pub fn from_parameter_name(export_id: ExportId, name: &str) -> Result<Self, PlanError> {
        #[derive(Serialize)]
        struct Input<'a> {
            namespace: &'static str,
            export_id: ExportId,
            name: &'a str,
        }
        if !distributed_name_is_canonical(name) {
            return Err(PlanError::new(
                "distributed function parameter name must be non-empty and canonical",
            ));
        }
        Ok(Self(canonical_sha256(&Input {
            namespace: "boon.distributed-function-argument.v1",
            export_id,
            name,
        })?))
    }
}

impl RemoteCallSiteId {
    pub fn from_identity(
        graph_id: DistributedGraphId,
        endpoint_id: DistributedEndpointId,
        stable_identity: DistributedDeclarationId,
    ) -> Result<Self, PlanError> {
        #[derive(Serialize)]
        struct Input {
            namespace: &'static str,
            graph_id: DistributedGraphId,
            endpoint_id: DistributedEndpointId,
            stable_identity: DistributedDeclarationId,
        }
        Ok(Self(canonical_sha256(&Input {
            namespace: "boon.distributed-call-site.v1",
            graph_id,
            endpoint_id,
            stable_identity,
        })?))
    }
}

impl DistributedCallInstanceId {
    pub fn from_rows(
        call_site_id: RemoteCallSiteId,
        rows: &[DistributedCallInstanceRow],
    ) -> Result<Self, PlanError> {
        Self::from_context(call_site_id, None, rows)
    }

    pub fn from_context(
        call_site_id: RemoteCallSiteId,
        parent: Option<DistributedCallInstanceId>,
        rows: &[DistributedCallInstanceRow],
    ) -> Result<Self, PlanError> {
        #[derive(Serialize)]
        struct Input<'a> {
            namespace: &'static str,
            call_site_id: RemoteCallSiteId,
            parent: Option<DistributedCallInstanceId>,
            rows: &'a [DistributedCallInstanceRow],
        }
        if digest_is_zero(call_site_id.as_bytes()) {
            return Err(PlanError::new(
                "distributed call instance requires a nonzero call-site ID",
            ));
        }
        if parent.is_some_and(|parent| digest_is_zero(parent.as_bytes())) {
            return Err(PlanError::new(
                "distributed call instance requires a nonzero parent instance ID",
            ));
        }
        if !rows.windows(2).all(|pair| pair[0] < pair[1])
            || rows.iter().any(|binding| binding.row.generation == 0)
        {
            return Err(PlanError::new(
                "distributed call instance rows must be unique, ordered, and current",
            ));
        }
        Ok(Self(canonical_sha256(&Input {
            namespace: "boon.distributed-call-instance.v3",
            call_site_id,
            parent,
            rows,
        })?))
    }

    pub fn from_owner(
        call_site_id: RemoteCallSiteId,
        owner: &OwnerInstanceId,
    ) -> Result<Self, PlanError> {
        #[derive(Serialize)]
        struct Input<'a> {
            namespace: &'static str,
            call_site_id: RemoteCallSiteId,
            owner: &'a OwnerInstanceId,
        }
        owner
            .validate()
            .map_err(|detail| PlanError::new(detail.to_owned()))?;
        if digest_is_zero(call_site_id.as_bytes()) {
            return Err(PlanError::new(
                "distributed call instance requires a nonzero call-site ID",
            ));
        }
        Ok(Self(canonical_sha256(&Input {
            namespace: "boon.distributed-call-instance.v1",
            call_site_id,
            owner,
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
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SourcePayloadDescriptor {
    pub field: SourcePayloadField,
    pub data_type: DataTypePlan,
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
    RetainedVisual {
        expression: DocumentExprId,
    },
    RuntimeValue {
        value: ValueRef,
        list_fields: Vec<OutputListFieldRef>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct OutputListFieldRef {
    pub list_id: ListId,
    pub name: String,
    pub field_id: FieldId,
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
    #[serde(default)]
    pub program_role: ProgramRole,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub distributed_endpoint: Option<DistributedEndpointPlan>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub producer_function_instances: Vec<ProducerFunctionInstancePlan>,
    pub application: ApplicationPlan,
    pub persistence: PersistencePlan,
    pub effects: Vec<EffectContract>,
    pub outputs: Vec<OutputRootPlan>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub host_ports: Vec<HostPortPlan>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub list_indexes: Vec<PlanListIndex>,
    pub demand: DemandPlan,
    pub document: Option<DocumentPlan>,
    pub row_expressions: PlanRowExpressionArena,
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
    pub fn row_expression(
        &self,
        id: PlanRowExpressionId,
    ) -> Result<&PlanRowExpressionNode, PlanError> {
        self.row_expressions.node(id)
    }

    pub fn row_expression_builder(&mut self) -> PlanRowExpressionBuilder<'_> {
        self.row_expressions.builder()
    }

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
pub struct PlanOwnerAncestor {
    pub static_owner: PlanStaticOwnerId,
    pub scope: ScopeId,
    pub list: ListId,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PlanOwner {
    pub static_owner: PlanStaticOwnerId,
    pub ancestors: Vec<PlanOwnerAncestor>,
}

impl PlanOwner {
    pub fn root() -> Self {
        Self {
            static_owner: PlanStaticOwnerId::ROOT,
            ancestors: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SourceRoute {
    pub id: PlanSourceRouteId,
    pub source_id: SourceId,
    pub owner: PlanOwner,
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
        value: FiniteReal,
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
    Data {
        value: boon_data::Value,
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
    pub owner: PlanOwner,
    pub value_type: PlanValueType,
    pub scope_id: Option<ScopeId>,
    pub indexed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub indexed_field_id: Option<FieldId>,
    pub initializer: ScalarInitializerPlan,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ScalarInitializerPlan {
    Constant { constant_id: PlanConstantId },
    Expression { expression: PlanRowExpressionId },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ListStorageSlot {
    pub id: PlanStorageId,
    pub list_id: ListId,
    pub scope_id: Option<ScopeId>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub row_fields: Vec<PlanListRowField>,
    pub capacity: Option<usize>,
    pub hidden_key_type: String,
    pub has_generation: bool,
    pub initializer_kind: ListInitializerKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub range: Option<PlanRangeInitializer>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub initial_rows: Vec<PlanInitialListRow>,
}

impl ListStorageSlot {
    pub fn row_field_ids(&self) -> impl Iterator<Item = FieldId> + '_ {
        self.row_fields.iter().map(|field| field.field_id)
    }

    pub fn contains_row_field(&self, field: FieldId) -> bool {
        self.row_fields
            .iter()
            .any(|candidate| candidate.field_id == field)
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanListRowFieldRole {
    Value,
    Authority,
    ValueAuthority,
    Capture,
}

impl PlanListRowFieldRole {
    pub const fn is_value(self) -> bool {
        matches!(self, Self::Value | Self::ValueAuthority)
    }

    pub const fn is_authority(self) -> bool {
        matches!(self, Self::Authority | Self::ValueAuthority)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PlanListRowField {
    pub field_id: FieldId,
    pub name: String,
    pub role: PlanListRowFieldRole,
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
    pub initializer: PlanInitialListFieldInitializer,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PlanInitialListFieldInitializer {
    Constant { value: PlanConstantValue },
    Expression { expression: PlanRowExpressionId },
}

impl PlanInitialListFieldInitializer {
    pub fn constant(&self) -> Option<&PlanConstantValue> {
        match self {
            Self::Constant { value } => Some(value),
            Self::Expression { .. } => None,
        }
    }

    pub fn expression(&self) -> Option<PlanRowExpressionId> {
        match self {
            Self::Constant { .. } => None,
            Self::Expression { expression } => Some(*expression),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanValueType {
    Text,
    Number,
    Bool,
    Bytes {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        fixed_len: Option<u64>,
    },
    Enum,
    Data,
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
    DerivedEvaluation,
    StateUpdates,
    ListMutations,
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
    pub owner: PlanOwner,
    pub gate: PlanRowExpressionId,
    pub intent_fields: Vec<EffectIntentFieldPlan>,
    pub idempotency_key: EffectIdempotencyKeyPlan,
    pub result: EffectResultRoute,
    pub barrier: EffectBarrier,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct EffectIntentFieldPlan {
    pub name: String,
    pub expression: PlanRowExpressionId,
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
}

impl EffectResultRoute {
    pub fn policy(&self) -> EffectResultPolicy {
        match self {
            Self::Target { policy, .. } => *policy,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanOpKind {
    SourceRoute,
    DerivedValue {
        derived_kind: PlanDerivedKind,
        #[serde(default = "default_derived_startup_recompute")]
        startup_recompute: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expression: Option<PlanDerivedExpression>,
    },
    StateUpdate {
        trigger: ValueRef,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        value: Option<PlanRowExpressionId>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        effect: Option<EffectInvocationPlan>,
    },
    ListMutation {
        mutation: PlanListMutation,
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
    Chunk { source_list: ListId, size: usize },
    ChunkValue { source: ValueRef, size: usize },
    Unknown { summary: String },
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
    MaterializeList {
        target_list: ListId,
        /// When present, the derived view is backed by authoritative target
        /// rows whose key and generation come from this logical source list.
        /// Reconciliation updates fields and order without creating a second
        /// row authority.
        authority_source_list: Option<ListId>,
        fields: BTreeMap<String, FieldId>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        row_field_copies: Vec<PlanMaterializedRowFieldCopy>,
        /// Non-keyed typed LIST values are promoted to this keyed authority
        /// before row-preserving contextual operators evaluate. The authority
        /// survives filtering so hidden row identity and row-owned state do
        /// not depend on output position.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        value_list_authorities: Vec<PlanValueListAuthority>,
        expression: Box<PlanDerivedExpression>,
    },
    SourceKeyTextTrimNonEmpty {
        source_id: SourceId,
        key_field: SourcePayloadField,
        required_key: String,
        state: ValueRef,
        skip_empty: bool,
    },
    SourceEventTransform {
        default: PlanRowExpressionId,
        arms: Vec<PlanSourceEventTransformArm>,
        #[serde(default)]
        router_route: bool,
    },
    BoolNot {
        input: ValueRef,
    },
    NumberCompareConst {
        left: ValueRef,
        op: PlanInfixOp,
        right: FiniteReal,
    },
    ValueCompare {
        left: ValueRef,
        op: PlanInfixOp,
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
        expression: PlanRowExpressionId,
    },
    /// Evaluates one field of a keyed materialized row without evaluating the
    /// complete collection map. Expressions that retain a contextual row
    /// local bind it to the exact output row; expressions that only read keyed
    /// state or fields need no synthetic local.
    MaterializedRowField {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        local: Option<PlanMaterializedRowLocal>,
        expression: PlanRowExpressionId,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PlanMaterializedRowLocal {
    pub owner: PlanStaticOwnerId,
    pub row_local: PlanLocalId,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PlanMaterializedRowFieldCopy {
    pub source_list: ListId,
    pub source_field: FieldId,
    pub target_field: FieldId,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PlanValueListAuthority {
    pub owner: PlanStaticOwnerId,
    pub list_id: ListId,
    pub fields: BTreeMap<String, FieldId>,
}

impl PlanDerivedExpression {
    pub fn visit_inputs(
        &self,
        arena: &PlanRowExpressionArena,
        visitor: &mut impl FnMut(ValueRef),
    ) -> Result<(), PlanError> {
        match self {
            Self::MaterializeList { expression, .. } => {
                expression.visit_inputs(arena, visitor)?;
            }
            Self::SourceKeyTextTrimNonEmpty {
                source_id,
                key_field,
                state,
                ..
            } => {
                visitor(ValueRef::SourcePayload {
                    source_id: *source_id,
                    field: key_field.clone(),
                });
                visitor(state.clone());
            }
            Self::SourceEventTransform { default, arms, .. } => {
                arena.visit_inputs(*default, visitor)?;
                for arm in arms {
                    visitor(arm.trigger.clone());
                    arena.visit_inputs(arm.value, visitor)?;
                }
            }
            Self::BoolNot { input } | Self::NumberCompareConst { left: input, .. } => {
                visitor(input.clone());
            }
            Self::ValueCompare { left, right, .. } => {
                visitor(left.clone());
                visitor(right.clone());
            }
            Self::BoolAnd { left, right } => {
                left.visit_inputs(arena, visitor)?;
                right.visit_inputs(arena, visitor)?;
            }
            Self::BoolNotExpression { input } => input.visit_inputs(arena, visitor)?,
            Self::RowExpression { expression } | Self::MaterializedRowField { expression, .. } => {
                arena.visit_inputs(*expression, visitor)?;
            }
        }
        Ok(())
    }

    pub fn visit_intrinsics(
        &self,
        arena: &PlanRowExpressionArena,
        visitor: &mut impl FnMut(PlanIntrinsic),
    ) -> Result<(), PlanError> {
        match self {
            Self::MaterializeList { expression, .. } => {
                expression.visit_intrinsics(arena, visitor)?;
            }
            Self::SourceEventTransform { default, arms, .. } => {
                arena.visit_intrinsics(*default, visitor)?;
                for arm in arms {
                    arena.visit_intrinsics(arm.value, visitor)?;
                }
            }
            Self::BoolAnd { left, right } => {
                left.visit_intrinsics(arena, visitor)?;
                right.visit_intrinsics(arena, visitor)?;
            }
            Self::BoolNotExpression { input } => input.visit_intrinsics(arena, visitor)?,
            Self::RowExpression { expression } | Self::MaterializedRowField { expression, .. } => {
                arena.visit_intrinsics(*expression, visitor)?;
            }
            Self::SourceKeyTextTrimNonEmpty { .. }
            | Self::BoolNot { .. }
            | Self::NumberCompareConst { .. }
            | Self::ValueCompare { .. } => {}
        }
        Ok(())
    }
}

impl PlanOp {
    /// Completes and canonically orders the dependency inputs implied by this
    /// operation's typed expressions. Lowering passes may rewrite expressions,
    /// so the compiler runs this once after all such rewrites; verification
    /// still rejects serialized plans that omit required inputs.
    pub fn synchronize_expression_inputs(
        &mut self,
        arena: &PlanRowExpressionArena,
    ) -> Result<(), PlanError> {
        let mut inputs = self.inputs.iter().cloned().collect::<BTreeSet<_>>();
        let mut insert = |input| {
            inputs.insert(input);
        };

        match &self.kind {
            PlanOpKind::SourceRoute | PlanOpKind::DependencyEdge => {}
            PlanOpKind::DerivedValue {
                expression: Some(expression),
                ..
            } => expression.visit_inputs(arena, &mut insert)?,
            PlanOpKind::DerivedValue {
                expression: None, ..
            } => {}
            PlanOpKind::StateUpdate {
                trigger,
                value,
                effect,
            } => {
                insert(trigger.clone());
                if let Some(value) = value {
                    arena.visit_inputs(*value, &mut insert)?;
                }
                if let Some(effect) = effect {
                    arena.visit_inputs(effect.gate, &mut insert)?;
                    for field in &effect.intent_fields {
                        arena.visit_inputs(field.expression, &mut insert)?;
                    }
                }
            }
            PlanOpKind::ListMutation { mutation } => match mutation {
                PlanListMutation::Append(append) => {
                    insert(append.trigger.clone());
                    arena.visit_inputs(append.gate, &mut insert)?;
                    arena.visit_inputs(append.item, &mut insert)?;
                }
                PlanListMutation::Remove(remove) => {
                    insert(remove.trigger.clone());
                    arena.visit_inputs(remove.gate, &mut insert)?;
                    arena.visit_inputs(remove.predicate, &mut insert)?;
                }
            },
            PlanOpKind::ListProjection { projection } => match projection {
                PlanListProjection::Chunk { source_list, .. } => {
                    insert(ValueRef::List(*source_list));
                }
                PlanListProjection::ChunkValue { source, .. } => insert(source.clone()),
                PlanListProjection::Unknown { .. } => {}
            },
        }

        self.inputs = inputs.into_iter().collect();
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PlanSourceEventTransformArm {
    pub trigger: ValueRef,
    pub value: PlanRowExpressionId,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanIntrinsic {
    SessionInfoStatus,
    SessionInfoPrincipal,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanContextualOperationKind {
    Map,
    Filter,
    Retain,
    Remove,
    Every,
    Any,
    Find,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanOrderOperationKind {
    SortBy,
    ThenBy,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanListIndexKeyKind {
    Number,
    Text,
    Bool,
    ClosedTag { type_id: [u8; 16] },
}

#[derive(
    Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum PlanListIndexKeyMultiplicity {
    #[default]
    One,
    ListItems {
        max_items: u16,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PlanListIndexKey {
    pub owner: PlanStaticOwnerId,
    pub row_local: PlanLocalId,
    pub expression: PlanRowExpressionId,
    pub kind: PlanListIndexKeyKind,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub closed_tags: Vec<String>,
    pub direction: PlanOrderDirection,
    #[serde(default)]
    pub multiplicity: PlanListIndexKeyMultiplicity,
}

pub fn closed_tag_type_id(tags: &[String]) -> Option<[u8; 16]> {
    if tags.is_empty()
        || tags.iter().any(|tag| tag.is_empty())
        || tags.windows(2).any(|pair| pair[0] >= pair[1])
    {
        return None;
    }
    let mut hasher = Sha256::new();
    hasher.update(b"boon.closed-tag-index.v1\0");
    for tag in tags {
        hasher.update((tag.len() as u64).to_be_bytes());
        hasher.update(tag.as_bytes());
    }
    let digest = hasher.finalize();
    let mut id = [0_u8; 16];
    id.copy_from_slice(&digest[..16]);
    Some(id)
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanOrderDirection {
    Ascending,
    Descending,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PlanListIndex {
    pub id: PlanListIndexId,
    pub source_list: ListId,
    pub keys: Vec<PlanListIndexKey>,
}

const MAX_TYPED_LIST_INDEX_KEYS: usize = 8;
pub const MAX_TYPED_LIST_EXPANDED_KEYS_PER_ROW: u16 = 16;
const MAX_TYPED_LIST_SELECTION_DEPTH: usize = 8;
const MAX_TYPED_LIST_SELECTION_LEAVES: usize = 64;
const STATIC_INDEX_IDENTITY_BYTES_PER_ENTRY: u64 = 32;
const STATIC_ESTIMATED_KEY_BYTES_PER_COMPONENT: u64 = 64;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PlanListFilter {
    pub owner: PlanStaticOwnerId,
    pub row_local: PlanLocalId,
    pub predicate: PlanRowExpressionId,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PlanListMap {
    pub owner: PlanStaticOwnerId,
    pub row_local: PlanLocalId,
    pub body: PlanRowExpressionId,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub captures: Vec<PlanRowCapture>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PlanListAccessSelection {
    OrderedStart,
    KeyPrefix {
        values: Vec<PlanRowExpressionId>,
    },
    TextPrefix {
        leading: Vec<PlanRowExpressionId>,
        prefix: PlanRowExpressionId,
    },
    ComponentRange {
        leading: Vec<PlanRowExpressionId>,
        lower: Option<PlanListAccessBound>,
        upper: Option<PlanListAccessBound>,
    },
    Union {
        branches: Vec<PlanListAccessSelection>,
    },
    Intersection {
        branches: Vec<PlanListAccessSelection>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PlanListAccessBound {
    pub value: PlanRowExpressionId,
    pub inclusive: bool,
}

impl PlanListAccessSelection {
    pub fn visit_expressions(&self, visitor: &mut impl FnMut(PlanRowExpressionId)) {
        let mut stack = vec![self];
        while let Some(selection) = stack.pop() {
            match selection {
                Self::OrderedStart => {}
                Self::KeyPrefix { values } => values.iter().copied().for_each(&mut *visitor),
                Self::TextPrefix { leading, prefix } => {
                    leading.iter().copied().for_each(&mut *visitor);
                    visitor(*prefix);
                }
                Self::ComponentRange {
                    leading,
                    lower,
                    upper,
                } => {
                    leading.iter().copied().for_each(&mut *visitor);
                    if let Some(lower) = lower {
                        visitor(lower.value);
                    }
                    if let Some(upper) = upper {
                        visitor(upper.value);
                    }
                }
                Self::Union { branches } | Self::Intersection { branches } => {
                    stack.extend(branches.iter().rev());
                }
            }
        }
    }

    pub fn visit_expressions_mut(&mut self, visitor: &mut impl FnMut(&mut PlanRowExpressionId)) {
        match self {
            Self::OrderedStart => {}
            Self::KeyPrefix { values } => values.iter_mut().for_each(visitor),
            Self::TextPrefix { leading, prefix } => {
                leading.iter_mut().for_each(&mut *visitor);
                visitor(prefix);
            }
            Self::ComponentRange {
                leading,
                lower,
                upper,
            } => {
                leading.iter_mut().for_each(&mut *visitor);
                if let Some(lower) = lower {
                    visitor(&mut lower.value);
                }
                if let Some(upper) = upper {
                    visitor(&mut upper.value);
                }
            }
            Self::Union { branches } | Self::Intersection { branches } => {
                for branch in branches {
                    branch.visit_expressions_mut(visitor);
                }
            }
        }
    }

    fn all_expressions(&self, predicate: &mut impl FnMut(PlanRowExpressionId) -> bool) -> bool {
        let mut valid = true;
        self.visit_expressions(&mut |expression| {
            if valid {
                valid = predicate(expression);
            }
        });
        valid
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PlanListAccess {
    pub index: PlanListIndexId,
    pub semantic_order: Vec<PlanListIndexKey>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exhaustive_candidate_limit: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub guard: Option<PlanRowExpressionId>,
    pub filters: Vec<PlanListFilter>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub maps: Vec<PlanListMap>,
    pub selection: PlanListAccessSelection,
    pub limit: PlanRowExpressionId,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PlanListPage {
    pub access: PlanListAccess,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub view_limit: Option<PlanRowExpressionId>,
    pub after: PlanRowExpressionId,
    pub view_fingerprint: [u8; 32],
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PlanBoundedListPage {
    pub view: PlanRowExpressionId,
    pub size: PlanRowExpressionId,
    pub after: PlanRowExpressionId,
    pub max_items: u32,
    pub view_fingerprint: [u8; 32],
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum CanonicalTypedListReference {
    Constant {
        value: PlanConstantValue,
    },
    Source {
        path: String,
    },
    SourcePayload {
        path: String,
        field: SourcePayloadField,
    },
    State {
        memory_id: MemoryId,
        type_fingerprint: [u8; 32],
    },
    StateProjection {
        memory_id: MemoryId,
        type_fingerprint: [u8; 32],
        field_path: Vec<String>,
    },
    PersistentField {
        leaf_id: MemoryLeafId,
        type_fingerprint: [u8; 32],
    },
    DerivedField {
        semantic_path: String,
    },
    List {
        memory_id: MemoryId,
        type_fingerprint: [u8; 32],
    },
    DistributedImport {
        import_id: ImportId,
    },
}

#[derive(Clone)]
pub struct TypedListViewFingerprintContext<'a> {
    row_expressions: &'a PlanRowExpressionArena,
    constants: BTreeMap<PlanConstantId, PlanConstantValue>,
    sources: BTreeMap<SourceId, String>,
    states: BTreeMap<StateId, (MemoryId, [u8; 32])>,
    fields: BTreeMap<FieldId, CanonicalTypedListReference>,
    lists: BTreeMap<ListId, (MemoryId, [u8; 32])>,
}

impl<'a> TypedListViewFingerprintContext<'a> {
    pub fn new(plan: &'a MachinePlan) -> Result<Self, PlanError> {
        let constants = plan
            .constants
            .iter()
            .map(|constant| (constant.id, constant.value.clone()))
            .collect();
        let sources = plan
            .source_routes
            .iter()
            .map(|route| (route.source_id, route.path.clone()))
            .collect();
        let scalar_memory = plan
            .persistence
            .memory
            .iter()
            .map(|memory| {
                (
                    memory.runtime_slot,
                    (memory.memory_id, memory.type_fingerprint),
                )
            })
            .collect::<BTreeMap<_, _>>();
        let states = plan
            .storage_layout
            .scalar_slots
            .iter()
            .filter_map(|slot| {
                scalar_memory
                    .get(&slot.id)
                    .copied()
                    .map(|identity| (slot.state_id, identity))
            })
            .collect();
        let list_memory = plan
            .persistence
            .lists
            .iter()
            .map(|memory| (memory.runtime_slot, memory))
            .collect::<BTreeMap<_, _>>();
        let mut lists = BTreeMap::new();
        let mut fields = BTreeMap::new();
        for slot in &plan.storage_layout.list_slots {
            let Some(memory) = list_memory.get(&slot.id).copied() else {
                continue;
            };
            lists.insert(slot.list_id, (memory.memory_id, memory.type_fingerprint));
            for leaf in &memory.row_fields {
                if let Some(field) = leaf.runtime_field_id {
                    fields.insert(
                        field,
                        CanonicalTypedListReference::PersistentField {
                            leaf_id: leaf.leaf_id,
                            type_fingerprint: leaf.type_fingerprint,
                        },
                    );
                }
            }
        }
        for entry in &plan.debug_map.fields {
            let Some(field) = entry
                .id
                .strip_prefix("field:")
                .and_then(|value| value.parse::<usize>().ok())
                .map(FieldId)
            else {
                continue;
            };
            fields
                .entry(field)
                .or_insert_with(|| CanonicalTypedListReference::DerivedField {
                    semantic_path: entry.label.clone(),
                });
        }
        Ok(Self {
            row_expressions: &plan.row_expressions,
            constants,
            sources,
            states,
            fields,
            lists,
        })
    }

    pub fn fingerprint(
        &self,
        source_list: ListId,
        semantic_order: &[PlanListIndexKey],
        guard: &Option<PlanRowExpressionId>,
        filters: &[PlanListFilter],
        maps: &[PlanListMap],
        view_limit: &Option<PlanRowExpressionId>,
    ) -> Result<[u8; 32], PlanError> {
        let (source_memory_id, source_type_fingerprint) =
            self.lists.get(&source_list).copied().ok_or_else(|| {
                PlanError::new(format!(
                    "typed list fingerprint references list {} without semantic identity",
                    source_list.0
                ))
            })?;
        let mut normalizer = TypedListViewNormalizer::new(self);
        let semantic_order = semantic_order
            .iter()
            .map(|key| normalizer.normalize_key(key))
            .collect::<Result<Vec<_>, _>>()?;
        let guard = guard
            .as_ref()
            .map(|expression| normalizer.normalize_expression_clone(*expression))
            .transpose()?;
        let filters = filters
            .iter()
            .map(|filter| normalizer.normalize_filter(filter))
            .collect::<Result<Vec<_>, _>>()?;
        let maps = maps
            .iter()
            .map(|map| normalizer.normalize_map(map))
            .collect::<Result<Vec<_>, _>>()?;
        let view_limit = view_limit
            .as_ref()
            .map(|expression| normalizer.normalize_expression_clone(*expression))
            .transpose()?;
        canonical_sha256(&CanonicalTypedListView {
            namespace: "boon.typed-list-view.v6",
            source_memory_id,
            source_type_fingerprint,
            row_expressions: normalizer.expressions,
            references: normalizer.references,
            semantic_order,
            guard,
            filters,
            maps,
            view_limit,
        })
    }

    pub fn bounded_fingerprint(&self, view: PlanRowExpressionId) -> Result<[u8; 32], PlanError> {
        let mut normalizer = TypedListViewNormalizer::new(self);
        let view = normalizer.normalize_expression_clone(view)?;
        canonical_sha256(&CanonicalBoundedListView {
            namespace: "boon.bounded-list-view.v1",
            row_expressions: normalizer.expressions,
            references: normalizer.references,
            view,
        })
    }
}

#[derive(Serialize)]
struct CanonicalTypedListView {
    namespace: &'static str,
    source_memory_id: MemoryId,
    source_type_fingerprint: [u8; 32],
    row_expressions: PlanRowExpressionArena,
    references: Vec<CanonicalTypedListReference>,
    semantic_order: Vec<PlanListIndexKey>,
    guard: Option<PlanRowExpressionId>,
    filters: Vec<PlanListFilter>,
    maps: Vec<PlanListMap>,
    view_limit: Option<PlanRowExpressionId>,
}

#[derive(Serialize)]
struct CanonicalBoundedListView {
    namespace: &'static str,
    row_expressions: PlanRowExpressionArena,
    references: Vec<CanonicalTypedListReference>,
    view: PlanRowExpressionId,
}

struct TypedListViewNormalizer<'a, 'plan> {
    context: &'a TypedListViewFingerprintContext<'plan>,
    references: Vec<CanonicalTypedListReference>,
    reference_index: BTreeMap<[u8; 32], Vec<usize>>,
    locals: BTreeMap<(PlanStaticOwnerId, PlanLocalId), usize>,
    expressions: PlanRowExpressionArena,
    normalized_expressions: BTreeMap<PlanRowExpressionId, PlanRowExpressionId>,
}

impl<'a, 'plan> TypedListViewNormalizer<'a, 'plan> {
    fn new(context: &'a TypedListViewFingerprintContext<'plan>) -> Self {
        Self {
            context,
            references: Vec::new(),
            reference_index: BTreeMap::new(),
            locals: BTreeMap::new(),
            expressions: PlanRowExpressionArena::new(),
            normalized_expressions: BTreeMap::new(),
        }
    }

    fn normalize_key(&mut self, key: &PlanListIndexKey) -> Result<PlanListIndexKey, PlanError> {
        let (owner, row_local) = self.normalize_local(key.owner, key.row_local);
        Ok(PlanListIndexKey {
            owner,
            row_local,
            expression: self.normalize_expression_clone(key.expression)?,
            kind: key.kind,
            closed_tags: key.closed_tags.clone(),
            direction: key.direction,
            multiplicity: key.multiplicity,
        })
    }

    fn normalize_filter(&mut self, filter: &PlanListFilter) -> Result<PlanListFilter, PlanError> {
        let (owner, row_local) = self.normalize_local(filter.owner, filter.row_local);
        Ok(PlanListFilter {
            owner,
            row_local,
            predicate: self.normalize_expression_clone(filter.predicate)?,
        })
    }

    fn normalize_map(&mut self, map: &PlanListMap) -> Result<PlanListMap, PlanError> {
        let (owner, row_local) = self.normalize_local(map.owner, map.row_local);
        Ok(PlanListMap {
            owner,
            row_local,
            body: self.normalize_expression_clone(map.body)?,
            captures: map
                .captures
                .iter()
                .map(|capture| {
                    Ok(PlanRowCapture {
                        field: self.normalize_field(capture.field)?,
                        value: self.normalize_expression_clone(capture.value)?,
                    })
                })
                .collect::<Result<Vec<_>, PlanError>>()?,
        })
    }

    fn normalize_expression_clone(
        &mut self,
        expression: PlanRowExpressionId,
    ) -> Result<PlanRowExpressionId, PlanError> {
        if let Some(normalized) = self.normalized_expressions.get(&expression).copied() {
            return Ok(normalized);
        }
        let missing = self
            .context
            .row_expressions
            .walk_canonical_postorder_filtered(expression, |id| {
                self.normalized_expressions.contains_key(&id)
            })?;
        for original_id in missing {
            let mut node = self.context.row_expressions.node(original_id)?.clone();
            let mut missing_child = None;
            node.visit_children_mut(&mut |child| {
                if let Some(normalized) = self.normalized_expressions.get(child).copied() {
                    *child = normalized;
                } else {
                    missing_child = Some(*child);
                }
            });
            if let Some(missing_child) = missing_child {
                return Err(PlanError::new(format!(
                    "typed list normalization reached parent {} before child {}",
                    original_id.0, missing_child.0
                )));
            }
            self.normalize_expression(&mut node)?;
            let normalized_id = self.expressions.intern(node)?;
            self.normalized_expressions
                .insert(original_id, normalized_id);
        }
        self.normalized_expressions
            .get(&expression)
            .copied()
            .ok_or_else(|| {
                PlanError::new(format!(
                    "typed list normalization did not produce expression {}",
                    expression.0
                ))
            })
    }

    fn normalize_expression(
        &mut self,
        expression: &mut PlanRowExpressionNode,
    ) -> Result<(), PlanError> {
        match expression {
            PlanRowExpressionNode::Field { input } => self.normalize_value_ref(input)?,
            PlanRowExpressionNode::Constant { constant_id } => {
                let reference = self.constant_reference(*constant_id)?;
                *constant_id = PlanConstantId(self.intern_reference(reference)?);
            }
            PlanRowExpressionNode::ListGetField { list_id, field, .. }
            | PlanRowExpressionNode::ListRowField { list_id, field, .. } => {
                *list_id = self.normalize_list(*list_id)?;
                *field = self.normalize_field(*field)?;
            }
            PlanRowExpressionNode::ListRef { list_id }
            | PlanRowExpressionNode::AuthorityListRef { list_id } => {
                *list_id = self.normalize_list(*list_id)?;
            }
            PlanRowExpressionNode::ContextualCollection {
                owner,
                row_local,
                indexed_access,
                ..
            } => {
                (*owner, *row_local) = self.normalize_local(*owner, *row_local);
                if let Some(indexed_access) = indexed_access {
                    indexed_access.index = PlanListIndexId(0);
                }
            }
            PlanRowExpressionNode::ContextualOrder {
                owner, row_local, ..
            } => {
                (*owner, *row_local) = self.normalize_local(*owner, *row_local);
            }
            PlanRowExpressionNode::ListAccess { access } => self.normalize_access_metadata(access),
            PlanRowExpressionNode::ListPage { page } => {
                self.normalize_access_metadata(&mut page.access);
                page.view_fingerprint = [0; 32];
            }
            PlanRowExpressionNode::BoundedListPage { page } => {
                page.view_fingerprint = [0; 32];
            }
            PlanRowExpressionNode::Local { owner, local, .. }
            | PlanRowExpressionNode::LocalRow { owner, local } => {
                (*owner, *local) = self.normalize_local(*owner, *local);
            }
            PlanRowExpressionNode::EventRow { source, list_id } => {
                *source = self.normalize_source(*source)?;
                *list_id = self.normalize_list(*list_id)?;
            }
            _ => {}
        }
        Ok(())
    }

    fn normalize_access_metadata(&mut self, access: &mut PlanListAccess) {
        access.index = PlanListIndexId(0);
        access.exhaustive_candidate_limit = None;
        for key in &mut access.semantic_order {
            (key.owner, key.row_local) = self.normalize_local(key.owner, key.row_local);
        }
        for filter in &mut access.filters {
            (filter.owner, filter.row_local) = self.normalize_local(filter.owner, filter.row_local);
        }
        for map in &mut access.maps {
            (map.owner, map.row_local) = self.normalize_local(map.owner, map.row_local);
        }
    }

    fn normalize_value_ref(&mut self, value: &mut ValueRef) -> Result<(), PlanError> {
        let reference = match value.clone() {
            ValueRef::Source(source) => CanonicalTypedListReference::Source {
                path: self.source_path(source)?,
            },
            ValueRef::SourcePayload { source_id, field } => {
                CanonicalTypedListReference::SourcePayload {
                    path: self.source_path(source_id)?,
                    field,
                }
            }
            ValueRef::State(state) => {
                let (memory_id, type_fingerprint) = self.state_identity(state)?;
                CanonicalTypedListReference::State {
                    memory_id,
                    type_fingerprint,
                }
            }
            ValueRef::StateProjection {
                state_id,
                field_path,
            } => {
                let (memory_id, type_fingerprint) = self.state_identity(state_id)?;
                CanonicalTypedListReference::StateProjection {
                    memory_id,
                    type_fingerprint,
                    field_path,
                }
            }
            ValueRef::Field(field) => self.field_reference(field)?,
            ValueRef::List(list) => self.list_reference(list)?,
            ValueRef::Constant(constant) => self.constant_reference(constant)?,
            ValueRef::DistributedImport(import_id) => {
                CanonicalTypedListReference::DistributedImport { import_id }
            }
        };
        *value = ValueRef::Constant(PlanConstantId(self.intern_reference(reference)?));
        Ok(())
    }

    fn normalize_list(&mut self, list: ListId) -> Result<ListId, PlanError> {
        let reference = self.list_reference(list)?;
        Ok(ListId(self.intern_reference(reference)?))
    }

    fn normalize_field(&mut self, field: FieldId) -> Result<FieldId, PlanError> {
        let reference = self.field_reference(field)?;
        Ok(FieldId(self.intern_reference(reference)?))
    }

    fn normalize_source(&mut self, source: SourceId) -> Result<SourceId, PlanError> {
        let reference = CanonicalTypedListReference::Source {
            path: self.source_path(source)?,
        };
        Ok(SourceId(self.intern_reference(reference)?))
    }

    fn normalize_local(
        &mut self,
        owner: PlanStaticOwnerId,
        local: PlanLocalId,
    ) -> (PlanStaticOwnerId, PlanLocalId) {
        let key = (owner, local);
        let position = self.locals.get(&key).copied().unwrap_or_else(|| {
            let position = self.locals.len();
            self.locals.insert(key, position);
            position
        });
        (PlanStaticOwnerId(position), PlanLocalId(0))
    }

    fn intern_reference(
        &mut self,
        reference: CanonicalTypedListReference,
    ) -> Result<usize, PlanError> {
        let structural_key = canonical_sha256(&reference)?;
        if let Some(position) = self
            .reference_index
            .get(&structural_key)
            .and_then(|candidates| {
                candidates
                    .iter()
                    .copied()
                    .find(|position| self.references[*position] == reference)
            })
        {
            return Ok(position);
        }
        let position = self.references.len();
        self.references.push(reference);
        self.reference_index
            .entry(structural_key)
            .or_default()
            .push(position);
        Ok(position)
    }

    fn constant_reference(
        &self,
        constant: PlanConstantId,
    ) -> Result<CanonicalTypedListReference, PlanError> {
        self.context
            .constants
            .get(&constant)
            .cloned()
            .map(|value| CanonicalTypedListReference::Constant { value })
            .ok_or_else(|| {
                PlanError::new(format!(
                    "typed list fingerprint references missing constant {}",
                    constant.0
                ))
            })
    }

    fn source_path(&self, source: SourceId) -> Result<String, PlanError> {
        self.context.sources.get(&source).cloned().ok_or_else(|| {
            PlanError::new(format!(
                "typed list fingerprint references missing source {}",
                source.0
            ))
        })
    }

    fn state_identity(&self, state: StateId) -> Result<(MemoryId, [u8; 32]), PlanError> {
        self.context.states.get(&state).copied().ok_or_else(|| {
            PlanError::new(format!(
                "typed list fingerprint references state {} without semantic identity",
                state.0
            ))
        })
    }

    fn field_reference(&self, field: FieldId) -> Result<CanonicalTypedListReference, PlanError> {
        self.context.fields.get(&field).cloned().ok_or_else(|| {
            PlanError::new(format!(
                "typed list fingerprint references field {} without semantic identity",
                field.0
            ))
        })
    }

    fn list_reference(&self, list: ListId) -> Result<CanonicalTypedListReference, PlanError> {
        self.context
            .lists
            .get(&list)
            .copied()
            .map(
                |(memory_id, type_fingerprint)| CanonicalTypedListReference::List {
                    memory_id,
                    type_fingerprint,
                },
            )
            .ok_or_else(|| {
                PlanError::new(format!(
                    "typed list fingerprint references list {} without semantic identity",
                    list.0
                ))
            })
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PlanContextualIndexedAccess {
    pub index: PlanListIndexId,
    pub selection: PlanListAccessSelection,
}

impl PlanIntrinsic {
    pub const fn allowed_in_unscoped_role(self, role: ProgramRole) -> bool {
        match self {
            Self::SessionInfoStatus => {
                matches!(role, ProgramRole::Client | ProgramRole::Session)
            }
            Self::SessionInfoPrincipal => matches!(role, ProgramRole::Session),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum PlanInfixOp {
    Add,
    Subtract,
    Multiply,
    Divide,
    Remainder,
    Equal,
    NotEqual,
    Less,
    LessOrEqual,
    Greater,
    GreaterOrEqual,
}

impl PlanInfixOp {
    pub const ALL: &'static [Self] = &[
        Self::Add,
        Self::Subtract,
        Self::Multiply,
        Self::Divide,
        Self::Remainder,
        Self::Equal,
        Self::NotEqual,
        Self::Less,
        Self::LessOrEqual,
        Self::Greater,
        Self::GreaterOrEqual,
    ];

    pub fn from_symbol(symbol: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|candidate| candidate.as_str() == symbol)
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Add => "+",
            Self::Subtract => "-",
            Self::Multiply => "*",
            Self::Divide => "/",
            Self::Remainder => "%",
            Self::Equal => "==",
            Self::NotEqual => "!=",
            Self::Less => "<",
            Self::LessOrEqual => "<=",
            Self::Greater => ">",
            Self::GreaterOrEqual => ">=",
        }
    }

    pub const fn is_comparison(self) -> bool {
        matches!(
            self,
            Self::Equal
                | Self::NotEqual
                | Self::Less
                | Self::LessOrEqual
                | Self::Greater
                | Self::GreaterOrEqual
        )
    }
}

impl fmt::Display for PlanInfixOp {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl Serialize for PlanInfixOp {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for PlanInfixOp {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let symbol = String::deserialize(deserializer)?;
        Self::from_symbol(&symbol).ok_or_else(|| {
            serde::de::Error::custom(format!("unknown plan infix operator `{symbol}`"))
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PlanRowBuiltinParameter {
    pub name: &'static str,
    pub required: bool,
    pub receiver: bool,
}

impl PlanRowBuiltinParameter {
    pub const fn required_receiver(name: &'static str) -> Self {
        Self {
            name,
            required: true,
            receiver: true,
        }
    }

    pub const fn required(name: &'static str) -> Self {
        Self {
            name,
            required: true,
            receiver: false,
        }
    }

    pub const fn optional(name: &'static str) -> Self {
        Self {
            name,
            required: false,
            receiver: false,
        }
    }

    pub const fn is_required(self) -> bool {
        self.required
    }

    pub const fn is_optional(self) -> bool {
        !self.required
    }

    pub const fn is_receiver(self) -> bool {
        self.receiver
    }
}

static BUILTIN_PARAMS_NONE: &[PlanRowBuiltinParameter] = &[];
static BUILTIN_PARAMS_VALUE: &[PlanRowBuiltinParameter] =
    &[PlanRowBuiltinParameter::required_receiver("value")];
static BUILTIN_PARAMS_BOOL_BINARY: &[PlanRowBuiltinParameter] = &[
    PlanRowBuiltinParameter::required_receiver("left"),
    PlanRowBuiltinParameter::required("right"),
];
static BUILTIN_PARAMS_BOOL_TOGGLE: &[PlanRowBuiltinParameter] = &[
    PlanRowBuiltinParameter::required_receiver("value"),
    PlanRowBuiltinParameter::required("when"),
];
static BUILTIN_PARAMS_TEXT_INPUT: &[PlanRowBuiltinParameter] =
    &[PlanRowBuiltinParameter::required_receiver("input")];
static BUILTIN_PARAMS_TEXT_CONTAINS: &[PlanRowBuiltinParameter] = &[
    PlanRowBuiltinParameter::required_receiver("input"),
    PlanRowBuiltinParameter::required("needle"),
];
static BUILTIN_PARAMS_TEXT_ALL_CHARS_IN: &[PlanRowBuiltinParameter] = &[
    PlanRowBuiltinParameter::required_receiver("input"),
    PlanRowBuiltinParameter::required("chars"),
];
static BUILTIN_PARAMS_TEXT_JOIN: &[PlanRowBuiltinParameter] = &[
    PlanRowBuiltinParameter::required_receiver("texts"),
    PlanRowBuiltinParameter::optional("separator"),
    PlanRowBuiltinParameter::optional("empty"),
];
static BUILTIN_PARAMS_TEXTS: &[PlanRowBuiltinParameter] =
    &[PlanRowBuiltinParameter::required_receiver("texts")];
static BUILTIN_PARAMS_TIME_RANGE_LABEL: &[PlanRowBuiltinParameter] = &[
    PlanRowBuiltinParameter::required_receiver("input"),
    PlanRowBuiltinParameter::required("end"),
    PlanRowBuiltinParameter::required("unit"),
];
static BUILTIN_PARAMS_NUMBER_TO_TEXT: &[PlanRowBuiltinParameter] = &[
    PlanRowBuiltinParameter::required_receiver("value"),
    PlanRowBuiltinParameter::optional("radix"),
    PlanRowBuiltinParameter::optional("min_width"),
    PlanRowBuiltinParameter::optional("signed_width"),
    PlanRowBuiltinParameter::optional("group_size"),
    PlanRowBuiltinParameter::optional("prefix"),
];
static BUILTIN_PARAMS_NUMBER_TO_ASCII_TEXT: &[PlanRowBuiltinParameter] = &[
    PlanRowBuiltinParameter::required_receiver("value"),
    PlanRowBuiltinParameter::optional("width"),
];
static BUILTIN_PARAMS_NUMBER_INTERPOLATE: &[PlanRowBuiltinParameter] = &[
    PlanRowBuiltinParameter::required("start"),
    PlanRowBuiltinParameter::required("end"),
    PlanRowBuiltinParameter::required("numerator"),
    PlanRowBuiltinParameter::required("denominator"),
    PlanRowBuiltinParameter::required("fallback"),
];
static BUILTIN_PARAMS_NUMBER_PROJECT_OFFSET: &[PlanRowBuiltinParameter] = &[
    PlanRowBuiltinParameter::required("time"),
    PlanRowBuiltinParameter::required("viewport_start"),
    PlanRowBuiltinParameter::required("viewport_end"),
    PlanRowBuiltinParameter::required("canvas_width"),
    PlanRowBuiltinParameter::required("fallback"),
    PlanRowBuiltinParameter::optional("zoom"),
];
static BUILTIN_PARAMS_NUMBER_PROJECT_TIME: &[PlanRowBuiltinParameter] = &[
    PlanRowBuiltinParameter::required("pointer_x"),
    PlanRowBuiltinParameter::required("pointer_width"),
    PlanRowBuiltinParameter::required("viewport_start"),
    PlanRowBuiltinParameter::required("viewport_end"),
    PlanRowBuiltinParameter::required("fallback"),
];
static BUILTIN_PARAMS_NUMBER_PROJECT_WIDTH: &[PlanRowBuiltinParameter] = &[
    PlanRowBuiltinParameter::required("start_time"),
    PlanRowBuiltinParameter::required("end_time"),
    PlanRowBuiltinParameter::required("viewport_start"),
    PlanRowBuiltinParameter::required("viewport_end"),
    PlanRowBuiltinParameter::required("canvas_width"),
    PlanRowBuiltinParameter::required("fallback"),
    PlanRowBuiltinParameter::optional("zoom"),
];
static BUILTIN_PARAMS_ERROR_NEW: &[PlanRowBuiltinParameter] =
    &[PlanRowBuiltinParameter::optional("code")];
static BUILTIN_PARAMS_LIST: &[PlanRowBuiltinParameter] =
    &[PlanRowBuiltinParameter::required_receiver("list")];
static BUILTIN_PARAMS_LIST_GET: &[PlanRowBuiltinParameter] = &[
    PlanRowBuiltinParameter::required_receiver("list"),
    PlanRowBuiltinParameter::required("index"),
];
static BUILTIN_PARAMS_LIST_TAKE: &[PlanRowBuiltinParameter] = &[
    PlanRowBuiltinParameter::required_receiver("list"),
    PlanRowBuiltinParameter::required("count"),
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PlanRowBuiltinSignature {
    pub parameters: &'static [PlanRowBuiltinParameter],
}

impl PlanRowBuiltinSignature {
    const fn new(parameters: &'static [PlanRowBuiltinParameter]) -> Self {
        Self { parameters }
    }

    pub fn receiver_parameter(self) -> Option<&'static PlanRowBuiltinParameter> {
        self.parameters.iter().find(|parameter| parameter.receiver)
    }

    pub fn parameter(self, name: &str) -> Option<&'static PlanRowBuiltinParameter> {
        self.parameters
            .iter()
            .find(|parameter| parameter.name == name)
    }

    fn validate_call_shape(
        self,
        function: PlanRowBuiltin,
        has_input: bool,
        args: &[PlanRowCallArg],
    ) -> Result<(), PlanError> {
        let receiver = self.receiver_parameter();
        if has_input && receiver.is_none() {
            return Err(PlanError::new(format!(
                "builtin `{}` does not accept an input",
                function.function_name()
            )));
        }

        let mut seen = BTreeSet::new();
        if let Some(receiver) = receiver.filter(|_| has_input) {
            seen.insert(receiver.name);
        }
        for argument in args {
            let Some(parameter) = self.parameter(&argument.name) else {
                return Err(PlanError::new(format!(
                    "builtin `{}` has unknown argument `{}`",
                    function.function_name(),
                    argument.name
                )));
            };
            if parameter.receiver {
                let detail = if has_input {
                    "duplicates its input"
                } else {
                    "must be stored as input"
                };
                return Err(PlanError::new(format!(
                    "builtin `{}` receiver argument `{}` {detail}",
                    function.function_name(),
                    argument.name
                )));
            }
            if !seen.insert(parameter.name) {
                return Err(PlanError::new(format!(
                    "builtin `{}` has duplicate argument `{}`",
                    function.function_name(),
                    argument.name
                )));
            }
        }

        if let Some(missing) = self
            .parameters
            .iter()
            .find(|parameter| parameter.required && !seen.contains(parameter.name))
        {
            return Err(PlanError::new(format!(
                "builtin `{}` is missing required {} `{}`",
                function.function_name(),
                if missing.receiver {
                    "input"
                } else {
                    "argument"
                },
                missing.name
            )));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum PlanRowBuiltin {
    BoolNot,
    BoolAnd,
    BoolOr,
    BoolToggle,
    TextEmpty,
    TextToLowercase,
    TextToUppercase,
    TextContains,
    TextIsNotEmpty,
    TextAllCharsIn,
    TextJoin,
    TextJoinLines,
    TextTimeRangeLabel,
    NumberToText,
    NumberToAsciiText,
    NumberBitWidth,
    NumberCeil,
    NumberFloor,
    NumberRound,
    NumberTruncate,
    NumberMin,
    NumberMax,
    NumberInterpolate,
    NumberProjectOffset,
    NumberProjectTime,
    NumberProjectWidth,
    ErrorNew,
    ErrorText,
    ListGet,
    ListLatest,
    ListCount,
    ListLength,
    ListIsNotEmpty,
    ListTake,
}

impl PlanRowBuiltin {
    pub const ALL: &'static [Self] = &[
        Self::BoolNot,
        Self::BoolAnd,
        Self::BoolOr,
        Self::BoolToggle,
        Self::TextEmpty,
        Self::TextToLowercase,
        Self::TextToUppercase,
        Self::TextContains,
        Self::TextIsNotEmpty,
        Self::TextAllCharsIn,
        Self::TextJoin,
        Self::TextJoinLines,
        Self::TextTimeRangeLabel,
        Self::NumberToText,
        Self::NumberToAsciiText,
        Self::NumberBitWidth,
        Self::NumberCeil,
        Self::NumberFloor,
        Self::NumberRound,
        Self::NumberTruncate,
        Self::NumberMin,
        Self::NumberMax,
        Self::NumberInterpolate,
        Self::NumberProjectOffset,
        Self::NumberProjectTime,
        Self::NumberProjectWidth,
        Self::ErrorNew,
        Self::ErrorText,
        Self::ListGet,
        Self::ListLatest,
        Self::ListCount,
        Self::ListLength,
        Self::ListIsNotEmpty,
        Self::ListTake,
    ];

    pub fn from_function_name(function: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|candidate| candidate.function_name() == function)
    }

    pub const fn function_name(self) -> &'static str {
        match self {
            Self::BoolNot => "Bool/not",
            Self::BoolAnd => "Bool/and",
            Self::BoolOr => "Bool/or",
            Self::BoolToggle => "Bool/toggle",
            Self::TextEmpty => "Text/empty",
            Self::TextToLowercase => "Text/to_lowercase",
            Self::TextToUppercase => "Text/to_uppercase",
            Self::TextContains => "Text/contains",
            Self::TextIsNotEmpty => "Text/is_not_empty",
            Self::TextAllCharsIn => "Text/all_chars_in",
            Self::TextJoin => "Text/join",
            Self::TextJoinLines => "Text/join_lines",
            Self::TextTimeRangeLabel => "Text/time_range_label",
            Self::NumberToText => "Number/to_text",
            Self::NumberToAsciiText => "Number/to_ascii_text",
            Self::NumberBitWidth => "Number/bit_width",
            Self::NumberCeil => "Number/ceil",
            Self::NumberFloor => "Number/floor",
            Self::NumberRound => "Number/round",
            Self::NumberTruncate => "Number/truncate",
            Self::NumberMin => "Number/min",
            Self::NumberMax => "Number/max",
            Self::NumberInterpolate => "Number/interpolate",
            Self::NumberProjectOffset => "Number/project_offset",
            Self::NumberProjectTime => "Number/project_time",
            Self::NumberProjectWidth => "Number/project_width",
            Self::ErrorNew => "Error/new",
            Self::ErrorText => "Error/text",
            Self::ListGet => "List/get",
            Self::ListLatest => "List/latest",
            Self::ListCount => "List/count",
            Self::ListLength => "List/length",
            Self::ListIsNotEmpty => "List/is_not_empty",
            Self::ListTake => "List/take",
        }
    }

    pub const fn fixed_result_type(self) -> Option<PlanValueType> {
        match self {
            Self::TextEmpty
            | Self::TextToLowercase
            | Self::TextToUppercase
            | Self::TextJoin
            | Self::TextJoinLines
            | Self::TextTimeRangeLabel
            | Self::NumberToText
            | Self::NumberToAsciiText
            | Self::ErrorText => Some(PlanValueType::Text),
            Self::NumberBitWidth
            | Self::NumberCeil
            | Self::NumberFloor
            | Self::NumberRound
            | Self::NumberTruncate
            | Self::NumberMin
            | Self::NumberMax
            | Self::NumberInterpolate
            | Self::NumberProjectOffset
            | Self::NumberProjectTime
            | Self::NumberProjectWidth
            | Self::ListCount
            | Self::ListLength => Some(PlanValueType::Number),
            Self::BoolNot
            | Self::BoolAnd
            | Self::BoolOr
            | Self::BoolToggle
            | Self::TextContains
            | Self::TextIsNotEmpty
            | Self::TextAllCharsIn
            | Self::ListIsNotEmpty => Some(PlanValueType::Bool),
            Self::ErrorNew => Some(PlanValueType::Data),
            Self::ListGet | Self::ListLatest | Self::ListTake => None,
        }
    }

    pub const fn signature(self) -> PlanRowBuiltinSignature {
        let parameters = match self {
            Self::BoolNot
            | Self::NumberBitWidth
            | Self::NumberCeil
            | Self::NumberFloor
            | Self::NumberRound
            | Self::NumberTruncate
            | Self::ErrorText => BUILTIN_PARAMS_VALUE,
            Self::BoolAnd | Self::BoolOr | Self::NumberMin | Self::NumberMax => {
                BUILTIN_PARAMS_BOOL_BINARY
            }
            Self::BoolToggle => BUILTIN_PARAMS_BOOL_TOGGLE,
            Self::TextEmpty => BUILTIN_PARAMS_NONE,
            Self::TextToLowercase | Self::TextToUppercase | Self::TextIsNotEmpty => {
                BUILTIN_PARAMS_TEXT_INPUT
            }
            Self::TextContains => BUILTIN_PARAMS_TEXT_CONTAINS,
            Self::TextAllCharsIn => BUILTIN_PARAMS_TEXT_ALL_CHARS_IN,
            Self::TextJoin => BUILTIN_PARAMS_TEXT_JOIN,
            Self::TextJoinLines => BUILTIN_PARAMS_TEXTS,
            Self::TextTimeRangeLabel => BUILTIN_PARAMS_TIME_RANGE_LABEL,
            Self::NumberToText => BUILTIN_PARAMS_NUMBER_TO_TEXT,
            Self::NumberToAsciiText => BUILTIN_PARAMS_NUMBER_TO_ASCII_TEXT,
            Self::NumberInterpolate => BUILTIN_PARAMS_NUMBER_INTERPOLATE,
            Self::NumberProjectOffset => BUILTIN_PARAMS_NUMBER_PROJECT_OFFSET,
            Self::NumberProjectTime => BUILTIN_PARAMS_NUMBER_PROJECT_TIME,
            Self::NumberProjectWidth => BUILTIN_PARAMS_NUMBER_PROJECT_WIDTH,
            Self::ErrorNew => BUILTIN_PARAMS_ERROR_NEW,
            Self::ListGet => BUILTIN_PARAMS_LIST_GET,
            Self::ListLatest | Self::ListCount | Self::ListLength | Self::ListIsNotEmpty => {
                BUILTIN_PARAMS_LIST
            }
            Self::ListTake => BUILTIN_PARAMS_LIST_TAKE,
        };
        PlanRowBuiltinSignature::new(parameters)
    }

    pub fn receiver_parameter(self) -> Option<&'static PlanRowBuiltinParameter> {
        self.signature().receiver_parameter()
    }

    pub fn parameter(self, name: &str) -> Option<&'static PlanRowBuiltinParameter> {
        self.signature().parameter(name)
    }

    pub fn validate_call(
        self,
        input: Option<PlanRowExpressionId>,
        args: &[PlanRowCallArg],
    ) -> Result<(), PlanError> {
        self.signature()
            .validate_call_shape(self, input.is_some(), args)
    }
}

impl fmt::Display for PlanRowBuiltin {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.function_name())
    }
}

impl Serialize for PlanRowBuiltin {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.function_name())
    }
}

impl<'de> Deserialize<'de> for PlanRowBuiltin {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let function = String::deserialize(deserializer)?;
        Self::from_function_name(&function).ok_or_else(|| {
            serde::de::Error::custom(format!("unknown plan row builtin `{function}`"))
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PlanRowExpressionNode {
    Intrinsic {
        intrinsic: PlanIntrinsic,
    },
    Field {
        input: ValueRef,
    },
    Constant {
        constant_id: PlanConstantId,
    },
    TextTrim {
        input: PlanRowExpressionId,
    },
    TextIsEmpty {
        input: PlanRowExpressionId,
    },
    TextStartsWith {
        input: PlanRowExpressionId,
        prefix: PlanRowExpressionId,
    },
    TextLength {
        input: PlanRowExpressionId,
    },
    TextToNumber {
        input: PlanRowExpressionId,
    },
    TextSubstring {
        input: PlanRowExpressionId,
        start: PlanRowExpressionId,
        length: PlanRowExpressionId,
    },
    TextToBytes {
        input: PlanRowExpressionId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        encoding: Option<PlanRowExpressionId>,
    },
    BytesToText {
        input: PlanRowExpressionId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        encoding: Option<PlanRowExpressionId>,
    },
    BytesToHex {
        input: PlanRowExpressionId,
    },
    BytesToBase64 {
        input: PlanRowExpressionId,
    },
    BytesFromHex {
        input: PlanRowExpressionId,
    },
    BytesFromBase64 {
        input: PlanRowExpressionId,
    },
    BytesIsEmpty {
        input: PlanRowExpressionId,
    },
    BytesLength {
        input: PlanRowExpressionId,
    },
    BytesGet {
        input: PlanRowExpressionId,
        index: PlanRowExpressionId,
    },
    BytesSlice {
        input: PlanRowExpressionId,
        offset: PlanRowExpressionId,
        byte_count: PlanRowExpressionId,
    },
    BytesTake {
        input: PlanRowExpressionId,
        byte_count: PlanRowExpressionId,
    },
    BytesDrop {
        input: PlanRowExpressionId,
        byte_count: PlanRowExpressionId,
    },
    BytesZeros {
        byte_count: PlanRowExpressionId,
    },
    BytesReadUnsigned {
        input: PlanRowExpressionId,
        offset: PlanRowExpressionId,
        byte_count: PlanRowExpressionId,
        endian: PlanRowExpressionId,
    },
    BytesReadSigned {
        input: PlanRowExpressionId,
        offset: PlanRowExpressionId,
        byte_count: PlanRowExpressionId,
        endian: PlanRowExpressionId,
    },
    BytesSet {
        input: PlanRowExpressionId,
        index: PlanRowExpressionId,
        value: PlanRowExpressionId,
    },
    BytesWriteUnsigned {
        input: PlanRowExpressionId,
        offset: PlanRowExpressionId,
        byte_count: PlanRowExpressionId,
        endian: PlanRowExpressionId,
        value: PlanRowExpressionId,
    },
    BytesWriteSigned {
        input: PlanRowExpressionId,
        offset: PlanRowExpressionId,
        byte_count: PlanRowExpressionId,
        endian: PlanRowExpressionId,
        value: PlanRowExpressionId,
    },
    BytesFind {
        input: PlanRowExpressionId,
        needle: PlanRowExpressionId,
    },
    BytesStartsWith {
        input: PlanRowExpressionId,
        prefix: PlanRowExpressionId,
    },
    BytesEndsWith {
        input: PlanRowExpressionId,
        suffix: PlanRowExpressionId,
    },
    BytesConcat {
        left: PlanRowExpressionId,
        right: PlanRowExpressionId,
    },
    BytesEqual {
        left: PlanRowExpressionId,
        right: PlanRowExpressionId,
    },
    NumberInfix {
        op: PlanInfixOp,
        left: PlanRowExpressionId,
        right: PlanRowExpressionId,
    },
    TextConcat {
        parts: Vec<PlanRowExpressionId>,
    },
    ListGetField {
        list_id: ListId,
        index: PlanRowExpressionId,
        field: FieldId,
    },
    ListRef {
        list_id: ListId,
    },
    /// Reads the keyed source rows owned by a list storage slot without
    /// recursively evaluating a derived view published under the same ListId.
    AuthorityListRef {
        list_id: ListId,
    },
    ListRange {
        from: PlanRowExpressionId,
        to: PlanRowExpressionId,
    },
    ListLiteral {
        items: Vec<PlanRowExpressionId>,
    },
    ContextualCollection {
        owner: PlanStaticOwnerId,
        operation: PlanContextualOperationKind,
        source: PlanRowExpressionId,
        row_local: PlanLocalId,
        body: PlanRowExpressionId,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        captures: Vec<PlanRowCapture>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        indexed_access: Option<Box<PlanContextualIndexedAccess>>,
    },
    ContextualOrder {
        owner: PlanStaticOwnerId,
        operation: PlanOrderOperationKind,
        source: PlanRowExpressionId,
        row_local: PlanLocalId,
        key: PlanRowExpressionId,
        direction: PlanRowExpressionId,
    },
    ListAccess {
        access: Box<PlanListAccess>,
    },
    ListPage {
        page: Box<PlanListPage>,
    },
    BoundedListPage {
        page: Box<PlanBoundedListPage>,
    },
    Local {
        owner: PlanStaticOwnerId,
        local: PlanLocalId,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        projection: Vec<String>,
    },
    LocalRow {
        owner: PlanStaticOwnerId,
        local: PlanLocalId,
    },
    EventRow {
        source: SourceId,
        list_id: ListId,
    },
    ListSum {
        input: PlanRowExpressionId,
    },
    Object {
        fields: Vec<PlanRowObjectField>,
    },
    TaggedObject {
        tag: String,
        fields: Vec<PlanRowObjectField>,
    },
    ObjectField {
        object: PlanRowExpressionId,
        field: String,
    },
    ListRowField {
        row: PlanRowExpressionId,
        list_id: ListId,
        field: FieldId,
    },
    BuiltinCall {
        function: PlanRowBuiltin,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        input: Option<PlanRowExpressionId>,
        args: Vec<PlanRowCallArg>,
    },
    Select {
        input: PlanRowExpressionId,
        arms: Vec<PlanRowSelectArm>,
    },
}

impl From<ValueRef> for PlanRowExpressionNode {
    fn from(input: ValueRef) -> Self {
        Self::Field { input }
    }
}

impl PlanRowExpressionNode {
    pub fn visit_children(&self, visitor: &mut impl FnMut(PlanRowExpressionId)) {
        match self {
            Self::Intrinsic { .. }
            | Self::Field { .. }
            | Self::Constant { .. }
            | Self::ListRef { .. }
            | Self::AuthorityListRef { .. }
            | Self::Local { .. }
            | Self::LocalRow { .. }
            | Self::EventRow { .. } => {}
            Self::TextTrim { input }
            | Self::TextIsEmpty { input }
            | Self::TextLength { input }
            | Self::TextToNumber { input }
            | Self::BytesToHex { input }
            | Self::BytesToBase64 { input }
            | Self::BytesFromHex { input }
            | Self::BytesFromBase64 { input }
            | Self::BytesIsEmpty { input }
            | Self::BytesLength { input }
            | Self::ListSum { input }
            | Self::ObjectField { object: input, .. }
            | Self::ListRowField { row: input, .. } => visitor(*input),
            Self::TextStartsWith { input, prefix }
            | Self::BytesStartsWith { input, prefix }
            | Self::BytesConcat {
                left: input,
                right: prefix,
            }
            | Self::BytesEqual {
                left: input,
                right: prefix,
            }
            | Self::NumberInfix {
                left: input,
                right: prefix,
                ..
            } => {
                visitor(*input);
                visitor(*prefix);
            }
            Self::BytesEndsWith { input, suffix }
            | Self::BytesFind {
                input,
                needle: suffix,
            }
            | Self::BytesGet {
                input,
                index: suffix,
            }
            | Self::BytesTake {
                input,
                byte_count: suffix,
            }
            | Self::BytesDrop {
                input,
                byte_count: suffix,
            } => {
                visitor(*input);
                visitor(*suffix);
            }
            Self::TextSubstring {
                input,
                start,
                length,
            }
            | Self::BytesSlice {
                input,
                offset: start,
                byte_count: length,
            }
            | Self::BytesSet {
                input,
                index: start,
                value: length,
            } => {
                visitor(*input);
                visitor(*start);
                visitor(*length);
            }
            Self::TextToBytes { input, encoding } | Self::BytesToText { input, encoding } => {
                visitor(*input);
                if let Some(encoding) = encoding {
                    visitor(*encoding);
                }
            }
            Self::BytesZeros { byte_count } => visitor(*byte_count),
            Self::BytesReadUnsigned {
                input,
                offset,
                byte_count,
                endian,
            }
            | Self::BytesReadSigned {
                input,
                offset,
                byte_count,
                endian,
            } => {
                visitor(*input);
                visitor(*offset);
                visitor(*byte_count);
                visitor(*endian);
            }
            Self::BytesWriteUnsigned {
                input,
                offset,
                byte_count,
                endian,
                value,
            }
            | Self::BytesWriteSigned {
                input,
                offset,
                byte_count,
                endian,
                value,
            } => {
                visitor(*input);
                visitor(*offset);
                visitor(*byte_count);
                visitor(*endian);
                visitor(*value);
            }
            Self::TextConcat { parts } | Self::ListLiteral { items: parts } => {
                parts.iter().copied().for_each(visitor);
            }
            Self::ListGetField { index, .. } => visitor(*index),
            Self::ListRange { from, to } => {
                visitor(*from);
                visitor(*to);
            }
            Self::ContextualCollection {
                source,
                body,
                captures,
                indexed_access,
                ..
            } => {
                visitor(*source);
                if let Some(indexed_access) = indexed_access {
                    indexed_access.selection.visit_expressions(visitor);
                }
                visitor(*body);
                captures
                    .iter()
                    .map(|capture| capture.value)
                    .for_each(visitor);
            }
            Self::ContextualOrder {
                source,
                key,
                direction,
                ..
            } => {
                visitor(*source);
                visitor(*key);
                visitor(*direction);
            }
            Self::ListAccess { access } => visit_list_access_children(access, visitor),
            Self::ListPage { page } => {
                visit_list_access_children(&page.access, visitor);
                if let Some(view_limit) = page.view_limit {
                    visitor(view_limit);
                }
                visitor(page.after);
            }
            Self::BoundedListPage { page } => {
                visitor(page.view);
                visitor(page.size);
                visitor(page.after);
            }
            Self::Object { fields } | Self::TaggedObject { fields, .. } => {
                fields.iter().map(|field| field.value).for_each(visitor);
            }
            Self::BuiltinCall { input, args, .. } => {
                if let Some(input) = input {
                    visitor(*input);
                }
                args.iter().map(|argument| argument.value).for_each(visitor);
            }
            Self::Select { input, arms } => {
                visitor(*input);
                arms.iter().map(|arm| arm.value).for_each(visitor);
            }
        }
    }

    pub fn child_ids(&self) -> Vec<PlanRowExpressionId> {
        let mut children = Vec::new();
        self.visit_children(&mut |child| children.push(child));
        children
    }

    fn visit_children_mut(&mut self, visitor: &mut impl FnMut(&mut PlanRowExpressionId)) {
        match self {
            Self::Intrinsic { .. }
            | Self::Field { .. }
            | Self::Constant { .. }
            | Self::ListRef { .. }
            | Self::AuthorityListRef { .. }
            | Self::Local { .. }
            | Self::LocalRow { .. }
            | Self::EventRow { .. } => {}
            Self::TextTrim { input }
            | Self::TextIsEmpty { input }
            | Self::TextLength { input }
            | Self::TextToNumber { input }
            | Self::BytesToHex { input }
            | Self::BytesToBase64 { input }
            | Self::BytesFromHex { input }
            | Self::BytesFromBase64 { input }
            | Self::BytesIsEmpty { input }
            | Self::BytesLength { input }
            | Self::ListSum { input }
            | Self::ObjectField { object: input, .. }
            | Self::ListRowField { row: input, .. } => visitor(input),
            Self::TextStartsWith { input, prefix }
            | Self::BytesStartsWith { input, prefix }
            | Self::BytesConcat {
                left: input,
                right: prefix,
            }
            | Self::BytesEqual {
                left: input,
                right: prefix,
            }
            | Self::NumberInfix {
                left: input,
                right: prefix,
                ..
            } => {
                visitor(input);
                visitor(prefix);
            }
            Self::BytesEndsWith { input, suffix }
            | Self::BytesFind {
                input,
                needle: suffix,
            }
            | Self::BytesGet {
                input,
                index: suffix,
            }
            | Self::BytesTake {
                input,
                byte_count: suffix,
            }
            | Self::BytesDrop {
                input,
                byte_count: suffix,
            } => {
                visitor(input);
                visitor(suffix);
            }
            Self::TextSubstring {
                input,
                start,
                length,
            }
            | Self::BytesSlice {
                input,
                offset: start,
                byte_count: length,
            }
            | Self::BytesSet {
                input,
                index: start,
                value: length,
            } => {
                visitor(input);
                visitor(start);
                visitor(length);
            }
            Self::TextToBytes { input, encoding } | Self::BytesToText { input, encoding } => {
                visitor(input);
                if let Some(encoding) = encoding {
                    visitor(encoding);
                }
            }
            Self::BytesZeros { byte_count } => visitor(byte_count),
            Self::BytesReadUnsigned {
                input,
                offset,
                byte_count,
                endian,
            }
            | Self::BytesReadSigned {
                input,
                offset,
                byte_count,
                endian,
            } => {
                visitor(input);
                visitor(offset);
                visitor(byte_count);
                visitor(endian);
            }
            Self::BytesWriteUnsigned {
                input,
                offset,
                byte_count,
                endian,
                value,
            }
            | Self::BytesWriteSigned {
                input,
                offset,
                byte_count,
                endian,
                value,
            } => {
                visitor(input);
                visitor(offset);
                visitor(byte_count);
                visitor(endian);
                visitor(value);
            }
            Self::TextConcat { parts } | Self::ListLiteral { items: parts } => {
                parts.iter_mut().for_each(visitor);
            }
            Self::ListGetField { index, .. } => visitor(index),
            Self::ListRange { from, to } => {
                visitor(from);
                visitor(to);
            }
            Self::ContextualCollection {
                source,
                body,
                captures,
                indexed_access,
                ..
            } => {
                visitor(source);
                if let Some(indexed_access) = indexed_access {
                    indexed_access.selection.visit_expressions_mut(visitor);
                }
                visitor(body);
                captures
                    .iter_mut()
                    .map(|capture| &mut capture.value)
                    .for_each(visitor);
            }
            Self::ContextualOrder {
                source,
                key,
                direction,
                ..
            } => {
                visitor(source);
                visitor(key);
                visitor(direction);
            }
            Self::ListAccess { access } => visit_list_access_children_mut(access, visitor),
            Self::ListPage { page } => {
                visit_list_access_children_mut(&mut page.access, visitor);
                if let Some(view_limit) = &mut page.view_limit {
                    visitor(view_limit);
                }
                visitor(&mut page.after);
            }
            Self::BoundedListPage { page } => {
                visitor(&mut page.view);
                visitor(&mut page.size);
                visitor(&mut page.after);
            }
            Self::Object { fields } | Self::TaggedObject { fields, .. } => {
                fields
                    .iter_mut()
                    .map(|field| &mut field.value)
                    .for_each(visitor);
            }
            Self::BuiltinCall { input, args, .. } => {
                if let Some(input) = input {
                    visitor(input);
                }
                args.iter_mut()
                    .map(|argument| &mut argument.value)
                    .for_each(visitor);
            }
            Self::Select { input, arms } => {
                visitor(input);
                arms.iter_mut().map(|arm| &mut arm.value).for_each(visitor);
            }
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PlanRowExpressionArena {
    nodes: Vec<PlanRowExpressionNode>,
    #[serde(skip)]
    structural_index: Option<BTreeMap<[u8; 32], Vec<PlanRowExpressionId>>>,
}

impl PartialEq for PlanRowExpressionArena {
    fn eq(&self, other: &Self) -> bool {
        self.nodes == other.nodes
    }
}

impl Eq for PlanRowExpressionArena {}

enum ContextualResolveStep {
    Expression(PlanRowExpressionId),
    Bind {
        owner: PlanStaticOwnerId,
        local: PlanLocalId,
        context: usize,
    },
    Unbind {
        owner: PlanStaticOwnerId,
        context: usize,
    },
}

impl PlanRowExpressionArena {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_nodes(nodes: Vec<PlanRowExpressionNode>) -> Result<Self, PlanError> {
        let arena = Self {
            nodes,
            structural_index: None,
        };
        arena.validate()?;
        Ok(arena)
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    pub fn get(&self, id: PlanRowExpressionId) -> Option<&PlanRowExpressionNode> {
        self.nodes.get(id.0)
    }

    pub fn node(&self, id: PlanRowExpressionId) -> Result<&PlanRowExpressionNode, PlanError> {
        self.get(id).ok_or_else(|| {
            PlanError::new(format!(
                "row expression id {} is invalid for arena length {}",
                id.0,
                self.len()
            ))
        })
    }

    pub fn iter(
        &self,
    ) -> impl ExactSizeIterator<Item = (PlanRowExpressionId, &PlanRowExpressionNode)> {
        self.nodes
            .iter()
            .enumerate()
            .map(|(index, node)| (PlanRowExpressionId(index), node))
    }

    pub fn into_nodes(self) -> Vec<PlanRowExpressionNode> {
        self.nodes
    }

    pub fn push(&mut self, node: PlanRowExpressionNode) -> Result<PlanRowExpressionId, PlanError> {
        self.validate_new_node(&node)?;
        let id = PlanRowExpressionId(self.nodes.len());
        let structural_key = self
            .structural_index
            .as_ref()
            .map(|_| canonical_sha256(&node))
            .transpose()?;
        self.nodes.push(node);
        if let (Some(index), Some(structural_key)) = (&mut self.structural_index, structural_key) {
            index.entry(structural_key).or_default().push(id);
        }
        Ok(id)
    }

    pub fn intern(
        &mut self,
        node: PlanRowExpressionNode,
    ) -> Result<PlanRowExpressionId, PlanError> {
        self.validate_new_node(&node)?;
        self.ensure_structural_index()?;
        let structural_key = canonical_sha256(&node)?;
        if let Some(id) = self.structural_index.as_ref().and_then(|index| {
            index.get(&structural_key).and_then(|candidates| {
                candidates
                    .iter()
                    .copied()
                    .find(|candidate| self.nodes[candidate.0] == node)
            })
        }) {
            return Ok(id);
        }
        self.push(node)
    }

    pub fn builder(&mut self) -> PlanRowExpressionBuilder<'_> {
        PlanRowExpressionBuilder { arena: self }
    }

    pub fn interner(&mut self) -> PlanRowExpressionInterner<'_> {
        PlanRowExpressionBuilder { arena: self }
    }

    pub fn validate(&self) -> Result<(), PlanError> {
        for (parent_index, node) in self.nodes.iter().enumerate() {
            let mut invalid_child = None;
            node.visit_children(&mut |child| {
                if invalid_child.is_none() && child.0 >= parent_index {
                    invalid_child = Some(child);
                }
            });
            if let Some(child) = invalid_child {
                return Err(PlanError::new(format!(
                    "row expression {} has invalid child {}; children must exist and precede parents",
                    parent_index, child.0
                )));
            }
        }
        Ok(())
    }

    pub fn walk_postorder(
        &self,
        root: PlanRowExpressionId,
    ) -> Result<Vec<PlanRowExpressionId>, PlanError> {
        self.walk_postorder_many([root])
    }

    fn walk_postorder_many(
        &self,
        roots: impl IntoIterator<Item = PlanRowExpressionId>,
    ) -> Result<Vec<PlanRowExpressionId>, PlanError> {
        self.walk_postorder_filtered(roots, |_| false)
    }

    fn walk_postorder_filtered(
        &self,
        roots: impl IntoIterator<Item = PlanRowExpressionId>,
        mut skip: impl FnMut(PlanRowExpressionId) -> bool,
    ) -> Result<Vec<PlanRowExpressionId>, PlanError> {
        let mut reachable = BTreeSet::new();
        let mut stack = roots.into_iter().collect::<Vec<_>>();
        while let Some(id) = stack.pop() {
            self.node(id)?;
            if skip(id) || !reachable.insert(id) {
                continue;
            }
            let mut invalid_child = None;
            self.nodes[id.0].visit_children(&mut |child| {
                if invalid_child.is_some() {
                    return;
                }
                if child >= id {
                    invalid_child = Some(child);
                } else {
                    stack.push(child);
                }
            });
            if let Some(child) = invalid_child {
                return Err(PlanError::new(format!(
                    "row expression {} has invalid child {}; children must exist and precede parents",
                    id.0, child.0
                )));
            }
        }
        Ok(reachable.into_iter().collect())
    }

    fn walk_canonical_postorder_filtered(
        &self,
        root: PlanRowExpressionId,
        mut skip: impl FnMut(PlanRowExpressionId) -> bool,
    ) -> Result<Vec<PlanRowExpressionId>, PlanError> {
        let mut visited = BTreeSet::new();
        let mut postorder = Vec::new();
        let mut stack = vec![(root, false)];
        while let Some((id, expanded)) = stack.pop() {
            if expanded {
                postorder.push(id);
                continue;
            }
            self.node(id)?;
            if skip(id) {
                continue;
            }
            if !visited.insert(id) {
                continue;
            }
            let children = self.nodes[id.0].child_ids();
            if let Some(child) = children.iter().copied().find(|child| *child >= id) {
                return Err(PlanError::new(format!(
                    "row expression {} has invalid child {}; children must exist and precede parents",
                    id.0, child.0
                )));
            }
            stack.push((id, true));
            stack.extend(children.into_iter().rev().map(|child| (child, false)));
        }
        Ok(postorder)
    }

    fn visit_roots(
        &self,
        roots: impl IntoIterator<Item = PlanRowExpressionId>,
        visitor: &mut impl FnMut(PlanRowExpressionId, &PlanRowExpressionNode),
    ) -> Result<(), PlanError> {
        for id in self.walk_postorder_many(roots)? {
            visitor(id, &self.nodes[id.0]);
        }
        Ok(())
    }

    pub fn visit(
        &self,
        root: PlanRowExpressionId,
        visitor: &mut impl FnMut(PlanRowExpressionId, &PlanRowExpressionNode),
    ) -> Result<(), PlanError> {
        for id in self.walk_postorder(root)? {
            visitor(id, &self.nodes[id.0]);
        }
        Ok(())
    }

    fn structurally_equivalent_with_local_remap(
        &self,
        expression: PlanRowExpressionId,
        from: (PlanStaticOwnerId, PlanLocalId),
        to: (PlanStaticOwnerId, PlanLocalId),
        expected: PlanRowExpressionId,
    ) -> Result<bool, PlanError> {
        let mut normalized = PlanRowExpressionArena::new();
        let remapped = self.clone_normalized_into(expression, Some((from, to)), &mut normalized)?;
        let expected = self.clone_normalized_into(expected, None, &mut normalized)?;
        Ok(remapped == expected)
    }

    fn clone_normalized_into(
        &self,
        root: PlanRowExpressionId,
        local_remap: Option<(
            (PlanStaticOwnerId, PlanLocalId),
            (PlanStaticOwnerId, PlanLocalId),
        )>,
        target: &mut PlanRowExpressionArena,
    ) -> Result<PlanRowExpressionId, PlanError> {
        let mut normalized_ids = BTreeMap::new();
        for original_id in self.walk_postorder(root)? {
            let mut node = self.node(original_id)?.clone();
            let mut missing_child = None;
            node.visit_children_mut(&mut |child| {
                if let Some(normalized) = normalized_ids.get(child).copied() {
                    *child = normalized;
                } else {
                    missing_child = Some(*child);
                }
            });
            if let Some(missing_child) = missing_child {
                return Err(PlanError::new(format!(
                    "row expression normalization reached parent {} before child {}",
                    original_id.0, missing_child.0
                )));
            }
            if let Some((from, to)) = local_remap {
                match &mut node {
                    PlanRowExpressionNode::Local { owner, local, .. }
                    | PlanRowExpressionNode::LocalRow { owner, local }
                        if (*owner, *local) == from =>
                    {
                        (*owner, *local) = to;
                    }
                    _ => {}
                }
            }
            normalized_ids.insert(original_id, target.intern(node)?);
        }
        normalized_ids.get(&root).copied().ok_or_else(|| {
            PlanError::new(format!(
                "row expression normalization did not produce root {}",
                root.0
            ))
        })
    }

    pub fn visit_value_refs(
        &self,
        root: PlanRowExpressionId,
        visitor: &mut impl FnMut(&ValueRef),
    ) -> Result<(), PlanError> {
        self.visit(root, &mut |_, node| {
            if let PlanRowExpressionNode::Field { input } = node {
                visitor(input);
            }
        })
    }

    pub fn visit_inputs(
        &self,
        root: PlanRowExpressionId,
        visitor: &mut impl FnMut(ValueRef),
    ) -> Result<(), PlanError> {
        self.visit(root, &mut |_, node| match node {
            PlanRowExpressionNode::Field { input } => visitor(input.clone()),
            PlanRowExpressionNode::Constant { constant_id } => {
                visitor(ValueRef::Constant(*constant_id));
            }
            PlanRowExpressionNode::ListGetField { list_id, .. }
            | PlanRowExpressionNode::ListRef { list_id }
            | PlanRowExpressionNode::AuthorityListRef { list_id }
            | PlanRowExpressionNode::ListRowField { list_id, .. } => {
                visitor(ValueRef::List(*list_id));
            }
            PlanRowExpressionNode::EventRow { source, .. } => visitor(ValueRef::Source(*source)),
            _ => {}
        })
    }

    pub fn visit_list_fields(
        &self,
        root: PlanRowExpressionId,
        visitor: &mut impl FnMut(ListId, FieldId),
    ) -> Result<(), PlanError> {
        self.visit(root, &mut |_, node| match node {
            PlanRowExpressionNode::ListGetField { list_id, field, .. }
            | PlanRowExpressionNode::ListRowField { list_id, field, .. } => {
                visitor(*list_id, *field);
            }
            _ => {}
        })
    }

    pub fn visit_intrinsics(
        &self,
        root: PlanRowExpressionId,
        visitor: &mut impl FnMut(PlanIntrinsic),
    ) -> Result<(), PlanError> {
        self.visit(root, &mut |_, node| {
            if let PlanRowExpressionNode::Intrinsic { intrinsic } = node {
                visitor(*intrinsic);
            }
        })
    }

    pub fn reads_authority_list(
        &self,
        root: PlanRowExpressionId,
        list_id: ListId,
    ) -> Result<bool, PlanError> {
        let mut found = false;
        self.visit(root, &mut |_, node| {
            found |= matches!(
                node,
                PlanRowExpressionNode::AuthorityListRef { list_id: candidate }
                    if *candidate == list_id
            );
        })?;
        Ok(found)
    }

    pub fn references_contextual_local(
        &self,
        root: PlanRowExpressionId,
        owner: PlanStaticOwnerId,
        local: PlanLocalId,
    ) -> Result<bool, PlanError> {
        let mut found = false;
        self.visit(root, &mut |_, node| {
            found |= matches!(
                node,
                PlanRowExpressionNode::Local {
                    owner: candidate_owner,
                    local: candidate_local,
                    ..
                } | PlanRowExpressionNode::LocalRow {
                    owner: candidate_owner,
                    local: candidate_local,
                } if *candidate_owner == owner && *candidate_local == local
            );
        })?;
        Ok(found)
    }

    pub fn contextual_locals_resolve(&self, root: PlanRowExpressionId) -> Result<bool, PlanError> {
        self.contextual_locals_resolve_with_bindings(root, [])
    }

    pub fn contextual_locals_resolve_with(
        &self,
        root: PlanRowExpressionId,
        owner: PlanStaticOwnerId,
        local: PlanLocalId,
    ) -> Result<bool, PlanError> {
        self.contextual_locals_resolve_with_bindings(root, [(owner, local)])
    }

    pub fn contextual_locals_resolve_with_bindings(
        &self,
        root: PlanRowExpressionId,
        bindings: impl IntoIterator<Item = (PlanStaticOwnerId, PlanLocalId)>,
    ) -> Result<bool, PlanError> {
        let mut active = BTreeMap::new();
        for (owner, local) in bindings {
            if active.insert(owner, local).is_some() {
                return Ok(false);
            }
        }
        self.node(root)?;

        let mut current_context = 0_usize;
        let mut next_context = 1_usize;
        let mut visited = BTreeSet::new();
        let mut stack = vec![ContextualResolveStep::Expression(root)];
        while let Some(step) = stack.pop() {
            match step {
                ContextualResolveStep::Bind {
                    owner,
                    local,
                    context,
                } => {
                    if active.insert(owner, local).is_some() {
                        return Ok(false);
                    }
                    current_context = context;
                }
                ContextualResolveStep::Unbind { owner, context } => {
                    active.remove(&owner);
                    current_context = context;
                }
                ContextualResolveStep::Expression(id) => {
                    if !visited.insert((current_context, id)) {
                        continue;
                    }
                    let node = self.node(id)?;
                    let mut invalid_child = None;
                    node.visit_children(&mut |child| {
                        if invalid_child.is_none() && child >= id {
                            invalid_child = Some(child);
                        }
                    });
                    if let Some(child) = invalid_child {
                        return Err(PlanError::new(format!(
                            "row expression {} has invalid child {}; children must exist and precede parents",
                            id.0, child.0
                        )));
                    }

                    let mut steps = Vec::new();
                    match node {
                        PlanRowExpressionNode::Local {
                            owner,
                            local,
                            projection,
                        } => {
                            if active.get(owner) != Some(local)
                                || projection.iter().any(String::is_empty)
                            {
                                return Ok(false);
                            }
                        }
                        PlanRowExpressionNode::LocalRow { owner, local } => {
                            if active.get(owner) != Some(local) {
                                return Ok(false);
                            }
                        }
                        PlanRowExpressionNode::ContextualCollection {
                            owner,
                            source,
                            row_local,
                            body,
                            captures,
                            indexed_access,
                            ..
                        } => {
                            steps.push(ContextualResolveStep::Expression(*source));
                            if let Some(indexed_access) = indexed_access {
                                indexed_access
                                    .selection
                                    .visit_expressions(&mut |expression| {
                                        steps.push(ContextualResolveStep::Expression(expression));
                                    });
                            }
                            let nested_context = next_context;
                            next_context += 1;
                            steps.push(ContextualResolveStep::Bind {
                                owner: *owner,
                                local: *row_local,
                                context: nested_context,
                            });
                            steps.push(ContextualResolveStep::Expression(*body));
                            steps.extend(
                                captures.iter().map(|capture| {
                                    ContextualResolveStep::Expression(capture.value)
                                }),
                            );
                            steps.push(ContextualResolveStep::Unbind {
                                owner: *owner,
                                context: current_context,
                            });
                        }
                        PlanRowExpressionNode::ContextualOrder {
                            owner,
                            source,
                            row_local,
                            key,
                            direction,
                            ..
                        } => {
                            steps.push(ContextualResolveStep::Expression(*source));
                            steps.push(ContextualResolveStep::Expression(*direction));
                            let nested_context = next_context;
                            next_context += 1;
                            steps.push(ContextualResolveStep::Bind {
                                owner: *owner,
                                local: *row_local,
                                context: nested_context,
                            });
                            steps.push(ContextualResolveStep::Expression(*key));
                            steps.push(ContextualResolveStep::Unbind {
                                owner: *owner,
                                context: current_context,
                            });
                        }
                        PlanRowExpressionNode::ListAccess { access } => {
                            append_list_access_contextual_steps(
                                access,
                                current_context,
                                &mut next_context,
                                &mut steps,
                            );
                        }
                        PlanRowExpressionNode::ListPage { page } => {
                            append_list_access_contextual_steps(
                                &page.access,
                                current_context,
                                &mut next_context,
                                &mut steps,
                            );
                            if let Some(view_limit) = page.view_limit {
                                steps.push(ContextualResolveStep::Expression(view_limit));
                            }
                            steps.push(ContextualResolveStep::Expression(page.after));
                        }
                        PlanRowExpressionNode::BoundedListPage { page } => {
                            steps.push(ContextualResolveStep::Expression(page.view));
                            steps.push(ContextualResolveStep::Expression(page.size));
                            steps.push(ContextualResolveStep::Expression(page.after));
                        }
                        node => node.visit_children(&mut |child| {
                            steps.push(ContextualResolveStep::Expression(child));
                        }),
                    }
                    stack.extend(steps.into_iter().rev());
                }
            }
        }
        Ok(true)
    }

    fn validate_new_node(&self, node: &PlanRowExpressionNode) -> Result<(), PlanError> {
        let parent = PlanRowExpressionId(self.len());
        for child in node.child_ids() {
            if child == parent {
                return Err(PlanError::new(format!(
                    "row expression {} cannot reference itself",
                    parent.0
                )));
            }
            if child.0 > parent.0 {
                return Err(PlanError::new(format!(
                    "row expression {} references invalid future id {}",
                    parent.0, child.0
                )));
            }
            if child >= parent {
                return Err(PlanError::new(format!(
                    "row expression {} has forward reference {}; children must precede parents",
                    parent.0, child.0
                )));
            }
        }
        Ok(())
    }

    fn ensure_structural_index(&mut self) -> Result<(), PlanError> {
        if self.structural_index.is_some() {
            return Ok(());
        }
        let mut index = BTreeMap::<[u8; 32], Vec<PlanRowExpressionId>>::new();
        for (id, node) in self.iter() {
            index.entry(canonical_sha256(node)?).or_default().push(id);
        }
        self.structural_index = Some(index);
        Ok(())
    }
}

fn append_list_access_contextual_steps(
    access: &PlanListAccess,
    parent_context: usize,
    next_context: &mut usize,
    steps: &mut Vec<ContextualResolveStep>,
) {
    if let Some(guard) = access.guard {
        steps.push(ContextualResolveStep::Expression(guard));
    }
    steps.push(ContextualResolveStep::Expression(access.limit));
    access.selection.visit_expressions(&mut |expression| {
        steps.push(ContextualResolveStep::Expression(expression));
    });
    for filter in &access.filters {
        let context = *next_context;
        *next_context += 1;
        steps.push(ContextualResolveStep::Bind {
            owner: filter.owner,
            local: filter.row_local,
            context,
        });
        steps.push(ContextualResolveStep::Expression(filter.predicate));
        steps.push(ContextualResolveStep::Unbind {
            owner: filter.owner,
            context: parent_context,
        });
    }
    for map in &access.maps {
        let context = *next_context;
        *next_context += 1;
        steps.push(ContextualResolveStep::Bind {
            owner: map.owner,
            local: map.row_local,
            context,
        });
        steps.push(ContextualResolveStep::Expression(map.body));
        steps.extend(
            map.captures
                .iter()
                .map(|capture| ContextualResolveStep::Expression(capture.value)),
        );
        steps.push(ContextualResolveStep::Unbind {
            owner: map.owner,
            context: parent_context,
        });
    }
}

pub struct PlanRowExpressionBuilder<'a> {
    arena: &'a mut PlanRowExpressionArena,
}

pub type PlanRowExpressionInterner<'a> = PlanRowExpressionBuilder<'a>;

impl<'a> PlanRowExpressionBuilder<'a> {
    pub fn push(&mut self, node: PlanRowExpressionNode) -> Result<PlanRowExpressionId, PlanError> {
        self.arena.push(node)
    }

    pub fn intern(
        &mut self,
        node: PlanRowExpressionNode,
    ) -> Result<PlanRowExpressionId, PlanError> {
        self.arena.intern(node)
    }

    pub fn value(&mut self, input: ValueRef) -> Result<PlanRowExpressionId, PlanError> {
        self.intern(PlanRowExpressionNode::Field { input })
    }

    pub fn constant(
        &mut self,
        constant_id: PlanConstantId,
    ) -> Result<PlanRowExpressionId, PlanError> {
        self.intern(PlanRowExpressionNode::Constant { constant_id })
    }

    pub fn arena(&self) -> &PlanRowExpressionArena {
        self.arena
    }
}

fn visit_list_access_children(
    access: &PlanListAccess,
    visitor: &mut impl FnMut(PlanRowExpressionId),
) {
    for key in &access.semantic_order {
        visitor(key.expression);
    }
    if let Some(guard) = access.guard {
        visitor(guard);
    }
    for filter in &access.filters {
        visitor(filter.predicate);
    }
    for map in &access.maps {
        visitor(map.body);
        for capture in &map.captures {
            visitor(capture.value);
        }
    }
    access.selection.visit_expressions(visitor);
    visitor(access.limit);
}

fn visit_list_access_children_mut(
    access: &mut PlanListAccess,
    visitor: &mut impl FnMut(&mut PlanRowExpressionId),
) {
    for key in &mut access.semantic_order {
        visitor(&mut key.expression);
    }
    if let Some(guard) = &mut access.guard {
        visitor(guard);
    }
    for filter in &mut access.filters {
        visitor(&mut filter.predicate);
    }
    for map in &mut access.maps {
        visitor(&mut map.body);
        for capture in &mut map.captures {
            visitor(&mut capture.value);
        }
    }
    access.selection.visit_expressions_mut(visitor);
    visitor(&mut access.limit);
}

fn visit_runtime_list_access_children(
    access: &PlanListAccess,
    visitor: &mut impl FnMut(PlanRowExpressionId),
) {
    if let Some(guard) = access.guard {
        visitor(guard);
    }
    for filter in &access.filters {
        visitor(filter.predicate);
    }
    for map in &access.maps {
        visitor(map.body);
        for capture in &map.captures {
            visitor(capture.value);
        }
    }
    access.selection.visit_expressions(visitor);
    visitor(access.limit);
}

fn visit_runtime_row_expression_children(
    node: &PlanRowExpressionNode,
    visitor: &mut impl FnMut(PlanRowExpressionId),
) {
    match node {
        PlanRowExpressionNode::ContextualCollection {
            source,
            body,
            captures,
            indexed_access,
            ..
        } => {
            visitor(*source);
            visitor(*body);
            for capture in captures {
                visitor(capture.value);
            }
            if let Some(indexed_access) = indexed_access {
                indexed_access.selection.visit_expressions(visitor);
            }
        }
        PlanRowExpressionNode::ListAccess { access } => {
            visit_runtime_list_access_children(access, visitor);
        }
        PlanRowExpressionNode::ListPage { page } => {
            visit_runtime_list_access_children(&page.access, visitor);
            if let Some(view_limit) = page.view_limit {
                visitor(view_limit);
            }
            visitor(page.after);
        }
        _ => node.visit_children(visitor),
    }
}

fn visit_cpu_row_expression_children(
    node: &PlanRowExpressionNode,
    visitor: &mut impl FnMut(PlanRowExpressionId),
) {
    if !matches!(node, PlanRowExpressionNode::ListRange { .. }) {
        visit_runtime_row_expression_children(node, visitor);
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PlanRowObjectField {
    pub name: String,
    pub value: PlanRowExpressionId,
    #[serde(default)]
    pub spread: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PlanRowCapture {
    pub field: FieldId,
    pub value: PlanRowExpressionId,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PlanRowCallArg {
    pub name: String,
    pub value: PlanRowExpressionId,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PlanRowSelectArm {
    pub pattern: PlanRowSelectPattern,
    pub value: PlanRowExpressionId,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PlanRowSelectPattern {
    Bool { value: bool },
    Text { value: String },
    Number { value: FiniteReal },
    NaN,
    Wildcard,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "plan", rename_all = "snake_case")]
pub enum PlanListMutation {
    Append(PlanListAppend),
    Remove(PlanListRemove),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PlanListAppend {
    pub site: usize,
    pub ordinal: u32,
    pub owner: PlanOwner,
    pub trigger: ValueRef,
    pub gate: PlanRowExpressionId,
    pub item: PlanRowExpressionId,
    pub fields: Vec<PlanListAppendField>,
    pub row_field_copies: Vec<PlanMaterializedRowFieldCopy>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PlanListAppendField {
    pub name: String,
    pub field_id: FieldId,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PlanListRemove {
    pub site: usize,
    pub ordinal: u32,
    pub owner: PlanOwner,
    pub trigger: ValueRef,
    pub gate: PlanRowExpressionId,
    pub local_owner: PlanStaticOwnerId,
    pub row_local: PlanLocalId,
    pub predicate: PlanRowExpressionId,
    pub remove_when: bool,
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
    StateProjection {
        state_id: StateId,
        field_path: Vec<String>,
    },
    Field(FieldId),
    List(ListId),
    Constant(PlanConstantId),
    DistributedImport(ImportId),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DirtyPlan {
    pub dependency_edges: usize,
    pub unresolved_dependency_edges: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CommitPlan {
    pub state_update_count: usize,
    pub unresolved_state_update_count: usize,
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

impl From<FiniteRealError> for PlanError {
    fn from(error: FiniteRealError) -> Self {
        Self::new(error.to_string())
    }
}

impl serde::ser::Error for PlanError {
    fn custom<T>(message: T) -> Self
    where
        T: fmt::Display,
    {
        Self::new(message.to_string())
    }
}

fn distributed_session_scope_failure(plan: &MachinePlan) -> Option<String> {
    fn inspect_row(
        arena: &PlanRowExpressionArena,
        expression: PlanRowExpressionId,
        found: &mut BTreeSet<PlanIntrinsic>,
    ) {
        let _ = arena.visit_intrinsics(expression, &mut |intrinsic| {
            found.insert(intrinsic);
        });
    }

    let initial_intrinsics = plan
        .storage_layout
        .scalar_slots
        .iter()
        .filter_map(|slot| match &slot.initializer {
            ScalarInitializerPlan::Expression { expression } => Some(expression),
            ScalarInitializerPlan::Constant { .. } => None,
        })
        .fold(BTreeSet::new(), |mut found, expression| {
            inspect_row(&plan.row_expressions, *expression, &mut found);
            found
        });

    if plan.program_role == ProgramRole::Server {
        if !initial_intrinsics.is_empty() {
            return Some(format!(
                "Server initializes SessionInfo intrinsic(s) outside an active Session scope: {:?}",
                initial_intrinsics
            ));
        }
        let endpoint = plan.distributed_endpoint.as_ref();
        let producer_scoped = plan.producer_function_instances.iter().filter(|instance| {
            endpoint.is_some_and(|endpoint| {
                endpoint.wire_schema.call_edges.iter().any(|edge| {
                    edge.call_site_id == instance.call_site_id
                        && edge.callee_role == ProgramRole::Server
                        && edge.scope == DistributedRouteScopePlan::OriginScoped
                })
            })
        });
        let mut scoped = endpoint
            .into_iter()
            .flat_map(|endpoint| endpoint.endpoint.value_imports.iter())
            .filter(|import| import.scope == DistributedRouteScopePlan::OriginScoped)
            .map(|import| ValueRef::DistributedImport(import.import_id))
            .chain(
                endpoint
                    .into_iter()
                    .flat_map(|endpoint| endpoint.endpoint.remote_call_sites.iter())
                    .filter(|call| call.scope == DistributedRouteScopePlan::OriginScoped)
                    .map(|call| call.result.value_ref()),
            )
            .chain(producer_scoped.flat_map(|instance| {
                std::iter::once(instance.result.clone()).chain(
                    instance
                        .arguments
                        .iter()
                        .map(|argument| ValueRef::DistributedImport(argument.import_id)),
                )
            }))
            .collect::<BTreeSet<_>>();
        let ops = plan
            .regions
            .iter()
            .flat_map(|region| &region.ops)
            .collect::<Vec<_>>();
        let mut server_intrinsics = BTreeSet::new();
        for op in &ops {
            let PlanOpKind::DerivedValue {
                expression: Some(expression),
                ..
            } = &op.kind
            else {
                continue;
            };
            let mut found = BTreeSet::new();
            let _ = expression.visit_intrinsics(&plan.row_expressions, &mut |intrinsic| {
                found.insert(intrinsic);
            });
            if found.is_empty() {
                continue;
            }
            server_intrinsics.extend(found);
            let Some(output) = &op.output else {
                return Some("Server SessionInfo expression has no scoped output".to_owned());
            };
            scoped.insert(output.clone());
        }
        if !server_intrinsics.is_empty() && endpoint.is_none() {
            return Some(format!(
                "Server contains SessionInfo intrinsic(s) without a distributed Session scope: {:?}",
                server_intrinsics
            ));
        }
        loop {
            let mut changed = false;
            for op in &ops {
                if !op.inputs.iter().any(|input| scoped.contains(input)) {
                    continue;
                }
                if !matches!(
                    op.kind,
                    PlanOpKind::DerivedValue { .. }
                        | PlanOpKind::ListProjection { .. }
                        | PlanOpKind::DependencyEdge
                ) {
                    return Some(format!(
                        "Server Session-scoped value crosses a global state/effect boundary at operation {}",
                        op.id.0
                    ));
                }
                if let Some(output) = &op.output {
                    changed |= scoped.insert(output.clone());
                }
            }
            if !changed {
                break;
            }
        }
        if let Some(endpoint) = endpoint {
            for output in &plan.outputs {
                if let OutputValueRef::RuntimeValue { value, .. } = &output.value
                    && scoped.contains(value)
                {
                    return Some(format!(
                        "Server host output `{}` depends on Session-scoped state; host outputs execute only in the global Server context",
                        output.name
                    ));
                }
            }

            let imported_event_sources = endpoint
                .endpoint
                .event_imports
                .iter()
                .filter(|import| import.scope == DistributedRouteScopePlan::OriginScoped)
                .map(|import| import.local_source_id)
                .collect::<BTreeSet<_>>();
            if let Some(route) = plan.source_routes.iter().find(|route| {
                imported_event_sources.contains(&route.source_id) && route.interval_ms.is_some()
            }) {
                return Some(format!(
                    "Server source {} is both a Session-origin event import and a global interval source",
                    route.source_id.0
                ));
            }
            if let Some(source) = plan
                .host_ports
                .iter()
                .flat_map(HostPortPlan::source_ids)
                .find(|source| imported_event_sources.contains(source))
            {
                return Some(format!(
                    "Server source {} is both a Session-origin event import and a global host-port source",
                    source.0
                ));
            }

            for export in &endpoint.endpoint.value_exports {
                let expected = scoped.contains(&export.value);
                if export.origin_scoped != expected {
                    return Some(format!(
                        "Server value export {} has origin_scoped={} but executable Session scope is {}",
                        export.export_id, export.origin_scoped, expected
                    ));
                }
            }
            for call in &endpoint.endpoint.remote_call_sites {
                let mut found = BTreeSet::new();
                let mut uses_scoped_value = false;
                for argument in &call.arguments {
                    inspect_row(&plan.row_expressions, argument.value, &mut found);
                    let _ = plan
                        .row_expressions
                        .visit_value_refs(argument.value, &mut |value| {
                            uses_scoped_value |= scoped.contains(value);
                        });
                }
                if (!found.is_empty() || uses_scoped_value)
                    && call.scope != DistributedRouteScopePlan::OriginScoped
                {
                    return Some(format!(
                        "Server remote call {} uses Session scope on a non-origin route",
                        call.call_site_id
                    ));
                }
            }
        }
        return None;
    }

    let mut forbidden = initial_intrinsics
        .into_iter()
        .filter(|intrinsic| !intrinsic.allowed_in_unscoped_role(plan.program_role))
        .collect::<BTreeSet<_>>();
    for expression in plan
        .regions
        .iter()
        .flat_map(|region| &region.ops)
        .filter_map(|op| match &op.kind {
            PlanOpKind::DerivedValue {
                expression: Some(expression),
                ..
            } => Some(expression),
            _ => None,
        })
    {
        let _ = expression.visit_intrinsics(&plan.row_expressions, &mut |intrinsic| {
            if !intrinsic.allowed_in_unscoped_role(plan.program_role) {
                forbidden.insert(intrinsic);
            }
        });
    }
    if let Some(document) = &plan.document {
        for expression in
            document
                .expressions
                .iter()
                .filter_map(|expression| match &expression.op {
                    DocumentExprOp::RuntimeExpression { expression, .. } => Some(*expression),
                    _ => None,
                })
        {
            let mut found = BTreeSet::new();
            inspect_row(&plan.row_expressions, expression, &mut found);
            forbidden.extend(
                found
                    .into_iter()
                    .filter(|intrinsic| !intrinsic.allowed_in_unscoped_role(plan.program_role)),
            );
        }
    }
    if let Some(endpoint) = &plan.distributed_endpoint {
        for argument in endpoint
            .endpoint
            .remote_call_sites
            .iter()
            .flat_map(|call| &call.arguments)
        {
            let mut found = BTreeSet::new();
            inspect_row(&plan.row_expressions, argument.value, &mut found);
            forbidden.extend(
                found
                    .into_iter()
                    .filter(|intrinsic| !intrinsic.allowed_in_unscoped_role(plan.program_role)),
            );
        }
    }
    (!forbidden.is_empty()).then(|| {
        format!(
            "{} role contains forbidden unscoped SessionInfo intrinsic(s): {:?}",
            plan.program_role.namespace(),
            forbidden
        )
    })
}

fn producer_function_ownership_ids_failure(
    plan: &MachinePlan,
    instance: &ProducerFunctionInstancePlan,
    plan_static_owners: &BTreeSet<PlanStaticOwnerId>,
) -> Option<String> {
    let ownership = &instance.ownership;
    if let Some(owner_id) = ownership
        .static_owners
        .iter()
        .find(|owner_id| !plan_static_owners.contains(owner_id))
    {
        return Some(format!(
            "producer function instance {} owns missing static owner {}",
            instance.call_site_id, owner_id.0
        ));
    }
    if let Some(source_id) = ownership.sources.iter().find(|source_id| {
        !plan
            .source_routes
            .iter()
            .any(|route| route.source_id == **source_id)
    }) {
        return Some(format!(
            "producer function instance {} owns missing source {}",
            instance.call_site_id, source_id.0
        ));
    }
    if let Some(state_id) = ownership.states.iter().find(|state_id| {
        !plan
            .storage_layout
            .scalar_slots
            .iter()
            .any(|slot| slot.state_id == **state_id)
    }) {
        return Some(format!(
            "producer function instance {} owns missing state {}",
            instance.call_site_id, state_id.0
        ));
    }
    if let Some(field_id) = ownership
        .fields
        .iter()
        .find(|field_id| !producer_function_field_exists(plan, **field_id))
    {
        return Some(format!(
            "producer function instance {} owns missing field {}",
            instance.call_site_id, field_id.0
        ));
    }
    if let Some(list_id) = ownership.lists.iter().find(|list_id| {
        !plan
            .storage_layout
            .list_slots
            .iter()
            .any(|slot| slot.list_id == **list_id)
    }) {
        return Some(format!(
            "producer function instance {} owns missing list {}",
            instance.call_site_id, list_id.0
        ));
    }
    if let Some(index_id) = ownership
        .indexes
        .iter()
        .find(|index_id| !plan.list_indexes.iter().any(|index| index.id == **index_id))
    {
        return Some(format!(
            "producer function instance {} owns missing list index {}",
            instance.call_site_id, index_id.0
        ));
    }
    if let Some(invocation_id) = ownership.effects.iter().find(|invocation_id| {
        !plan
            .regions
            .iter()
            .flat_map(|region| &region.ops)
            .any(|op| {
                matches!(
                    &op.kind,
                    PlanOpKind::StateUpdate {
                        effect: Some(invocation),
                        ..
                    } if invocation.invocation_id == **invocation_id
                )
            })
    }) {
        return Some(format!(
            "producer function instance {} owns missing effect invocation {}",
            instance.call_site_id, invocation_id
        ));
    }
    None
}

fn producer_function_plan_static_owners(plan: &MachinePlan) -> BTreeSet<PlanStaticOwnerId> {
    let mut owners = BTreeSet::new();
    if let Some(endpoint) = &plan.distributed_endpoint {
        for call in &endpoint.endpoint.remote_call_sites {
            collect_plan_owner_static_owners(&call.owner, &mut owners);
        }
    }
    for instance in &plan.producer_function_instances {
        collect_plan_owner_static_owners(&instance.owner, &mut owners);
    }
    for route in &plan.source_routes {
        collect_plan_owner_static_owners(&route.owner, &mut owners);
    }
    for slot in &plan.storage_layout.scalar_slots {
        collect_plan_owner_static_owners(&slot.owner, &mut owners);
    }
    for index in &plan.list_indexes {
        owners.extend(index.keys.iter().map(|key| key.owner));
    }
    for op in plan.regions.iter().flat_map(|region| &region.ops) {
        match &op.kind {
            PlanOpKind::DerivedValue {
                expression: Some(expression),
                ..
            } => collect_derived_expression_static_owners(expression, &mut owners),
            PlanOpKind::StateUpdate {
                effect: Some(effect),
                ..
            } => collect_plan_owner_static_owners(&effect.owner, &mut owners),
            PlanOpKind::ListMutation { mutation } => match mutation {
                PlanListMutation::Append(append) => {
                    collect_plan_owner_static_owners(&append.owner, &mut owners);
                }
                PlanListMutation::Remove(remove) => {
                    collect_plan_owner_static_owners(&remove.owner, &mut owners);
                    owners.insert(remove.local_owner);
                }
            },
            PlanOpKind::SourceRoute
            | PlanOpKind::DerivedValue {
                expression: None, ..
            }
            | PlanOpKind::StateUpdate { effect: None, .. }
            | PlanOpKind::ListProjection { .. }
            | PlanOpKind::DependencyEdge => {}
        }
    }
    let _ = visit_plan_row_expressions(plan, &mut |_, expression| match expression {
        PlanRowExpressionNode::ContextualCollection { owner, .. }
        | PlanRowExpressionNode::ContextualOrder { owner, .. } => {
            owners.insert(*owner);
        }
        PlanRowExpressionNode::ListAccess { access } => {
            for filter in &access.filters {
                owners.insert(filter.owner);
            }
        }
        PlanRowExpressionNode::ListPage { page } => {
            for filter in &page.access.filters {
                owners.insert(filter.owner);
            }
        }
        _ => {}
    });
    owners
}

fn collect_plan_owner_static_owners(owner: &PlanOwner, owners: &mut BTreeSet<PlanStaticOwnerId>) {
    owners.insert(owner.static_owner);
    owners.extend(owner.ancestors.iter().map(|ancestor| ancestor.static_owner));
}

fn collect_derived_expression_static_owners(
    expression: &PlanDerivedExpression,
    owners: &mut BTreeSet<PlanStaticOwnerId>,
) {
    match expression {
        PlanDerivedExpression::MaterializeList { expression, .. }
        | PlanDerivedExpression::BoolNotExpression { input: expression } => {
            collect_derived_expression_static_owners(expression, owners);
        }
        PlanDerivedExpression::BoolAnd { left, right } => {
            collect_derived_expression_static_owners(left, owners);
            collect_derived_expression_static_owners(right, owners);
        }
        PlanDerivedExpression::MaterializedRowField {
            local: Some(local), ..
        } => {
            owners.insert(local.owner);
        }
        PlanDerivedExpression::MaterializedRowField { local: None, .. }
        | PlanDerivedExpression::SourceEventTransform { .. }
        | PlanDerivedExpression::RowExpression { .. }
        | PlanDerivedExpression::SourceKeyTextTrimNonEmpty { .. }
        | PlanDerivedExpression::BoolNot { .. }
        | PlanDerivedExpression::NumberCompareConst { .. }
        | PlanDerivedExpression::ValueCompare { .. } => {}
    }
}

fn producer_function_field_exists(plan: &MachinePlan, field_id: FieldId) -> bool {
    plan.storage_layout
        .scalar_slots
        .iter()
        .any(|slot| slot.indexed_field_id == Some(field_id))
        || plan
            .storage_layout
            .list_slots
            .iter()
            .any(|slot| slot.contains_row_field(field_id))
        || plan
            .regions
            .iter()
            .flat_map(|region| &region.ops)
            .any(|op| op.output == Some(ValueRef::Field(field_id)))
}

fn first_producer_ownership_overlap<T: Copy + Ord>(
    claimed: &mut BTreeSet<T>,
    ids: &[T],
) -> Option<T> {
    for id in ids.iter().copied() {
        if !claimed.insert(id) {
            return Some(id);
        }
    }
    None
}

fn producer_function_instances_failure(plan: &MachinePlan) -> Option<String> {
    let Some(endpoint) = &plan.distributed_endpoint else {
        return (!plan.producer_function_instances.is_empty()).then(|| {
            "producer function instances require a linked distributed endpoint".to_owned()
        });
    };
    if !plan
        .producer_function_instances
        .windows(2)
        .all(|pair| pair[0].call_site_id < pair[1].call_site_id)
    {
        return Some(
            "producer function instances must have unique canonically ordered call-site IDs"
                .to_owned(),
        );
    }
    let inbound_call_sites = endpoint
        .wire_schema
        .call_edges
        .iter()
        .filter(|edge| edge.callee_role == plan.program_role)
        .map(|edge| edge.call_site_id)
        .collect::<Vec<_>>();
    if !plan
        .producer_function_instances
        .iter()
        .map(|instance| instance.call_site_id)
        .eq(inbound_call_sites)
    {
        return Some(
            "producer function instances must exactly cover the role's inbound wire call sites"
                .to_owned(),
        );
    }

    let mut import_ids = endpoint
        .endpoint
        .value_imports
        .iter()
        .map(|import| import.import_id)
        .chain(
            endpoint
                .endpoint
                .event_imports
                .iter()
                .map(|import| import.import_id),
        )
        .chain(
            endpoint
                .endpoint
                .remote_call_sites
                .iter()
                .filter_map(|call| call.result.current_import_id()),
        )
        .collect::<BTreeSet<_>>();
    let current_call_count = endpoint
        .endpoint
        .remote_call_sites
        .iter()
        .filter(|call| call.mode == DistributedCallMode::Current)
        .count();
    let local_import_count = endpoint.endpoint.value_imports.len()
        + endpoint.endpoint.event_imports.len()
        + current_call_count;
    if import_ids.len() != local_import_count {
        return Some("distributed machine import IDs are not unique".to_owned());
    }
    for call in &endpoint.endpoint.remote_call_sites {
        let DistributedCallResultPlan::Invocation {
            source_id,
            payload_field,
        } = &call.result
        else {
            continue;
        };
        let Some(route) = plan
            .source_routes
            .iter()
            .find(|route| route.source_id == *source_id)
        else {
            return Some(format!(
                "distributed invocation {} has no private result source route {}",
                call.call_site_id, source_id.0
            ));
        };
        if route
            .payload_schema
            .typed_fields
            .iter()
            .find(|field| field.field == *payload_field)
            .is_none_or(|field| field.data_type != call.result_type)
        {
            return Some(format!(
                "distributed invocation {} result source {} does not expose its exact result type",
                call.call_site_id, source_id.0
            ));
        }
    }

    let mut claimed_static_owners = BTreeSet::new();
    let mut claimed_sources = BTreeSet::new();
    let mut claimed_states = BTreeSet::new();
    let mut claimed_fields = BTreeSet::new();
    let mut claimed_lists = BTreeSet::new();
    let mut claimed_indexes = BTreeSet::new();
    let mut claimed_effects = BTreeSet::new();
    let plan_static_owners = producer_function_plan_static_owners(plan);
    for instance in &plan.producer_function_instances {
        let Some(function) = endpoint
            .endpoint
            .function_exports
            .iter()
            .find(|function| function.export_id == instance.function_export_id)
        else {
            return Some(format!(
                "producer function instance {} references a missing local function export",
                instance.call_site_id
            ));
        };
        if let Err(error) = validate_producer_function_instance_signature(instance, function) {
            return Some(error.to_string());
        }
        if let Some(failure) =
            producer_function_ownership_ids_failure(plan, instance, &plan_static_owners)
        {
            return Some(failure);
        }
        if let Some(owner_id) = first_producer_ownership_overlap(
            &mut claimed_static_owners,
            &instance.ownership.static_owners,
        ) {
            return Some(format!(
                "producer function ownership manifests overlap on static owner {}",
                owner_id.0
            ));
        }
        if let Some(source_id) =
            first_producer_ownership_overlap(&mut claimed_sources, &instance.ownership.sources)
        {
            return Some(format!(
                "producer function ownership manifests overlap on source {}",
                source_id.0
            ));
        }
        if let Some(state_id) =
            first_producer_ownership_overlap(&mut claimed_states, &instance.ownership.states)
        {
            return Some(format!(
                "producer function ownership manifests overlap on state {}",
                state_id.0
            ));
        }
        if let Some(field_id) =
            first_producer_ownership_overlap(&mut claimed_fields, &instance.ownership.fields)
        {
            return Some(format!(
                "producer function ownership manifests overlap on field {}",
                field_id.0
            ));
        }
        if let Some(list_id) =
            first_producer_ownership_overlap(&mut claimed_lists, &instance.ownership.lists)
        {
            return Some(format!(
                "producer function ownership manifests overlap on list {}",
                list_id.0
            ));
        }
        if let Some(index_id) =
            first_producer_ownership_overlap(&mut claimed_indexes, &instance.ownership.indexes)
        {
            return Some(format!(
                "producer function ownership manifests overlap on list index {}",
                index_id.0
            ));
        }
        if let Some(invocation_id) =
            first_producer_ownership_overlap(&mut claimed_effects, &instance.ownership.effects)
        {
            return Some(format!(
                "producer function ownership manifests overlap on effect invocation {}",
                invocation_id
            ));
        }
        if function.producer_role != plan.program_role
            || !plan_owner_resolves(plan, &instance.owner)
        {
            return Some(format!(
                "producer function instance {} has the wrong role or an invalid structural owner",
                instance.call_site_id
            ));
        }
        let Some(edge) = endpoint.wire_schema.call_edges.iter().find(|edge| {
            edge.call_site_id == instance.call_site_id
                && edge.callee_role == plan.program_role
                && edge.function_export_id == instance.function_export_id
        }) else {
            return Some(format!(
                "producer function instance {} has no matching inbound wire call",
                instance.call_site_id
            ));
        };
        if edge.parameters != function.parameters
            || edge.result_type != function.result_type
            || edge.result_type != instance.result_type
        {
            return Some(format!(
                "producer function instance {} disagrees with its inbound wire signature",
                instance.call_site_id
            ));
        }
        for argument in &instance.arguments {
            if !import_ids.insert(argument.import_id) {
                return Some(format!(
                    "producer function argument import {} is not unique in the machine",
                    argument.import_id
                ));
            }
        }
        if !runtime_output_ref_resolves(plan, &instance.result) {
            return Some(format!(
                "producer function instance {} result does not resolve to executable local state",
                instance.call_site_id
            ));
        }
        if !producer_result_type_is_compatible(plan, &instance.result, &instance.result_type) {
            return Some(format!(
                "producer function instance {} result is incompatible with its declared type",
                instance.call_site_id
            ));
        }
        if let ValueRef::DistributedImport(import_id) = &instance.result
            && plan
                .producer_function_instances
                .iter()
                .flat_map(|candidate| &candidate.arguments)
                .any(|argument| argument.import_id == *import_id)
            && !instance
                .arguments
                .iter()
                .any(|argument| argument.import_id == *import_id)
        {
            return Some(format!(
                "producer function instance {} result references another call site's argument import",
                instance.call_site_id
            ));
        }
    }
    for call in &endpoint.endpoint.remote_call_sites {
        if call.arguments.iter().any(|argument| {
            distributed_expression_import_ids(&plan.row_expressions, argument.value)
                .iter()
                .any(|import_id| !import_ids.contains(import_id))
        }) {
            return Some(format!(
                "distributed call {} references an undeclared machine import",
                call.call_site_id
            ));
        }
    }
    None
}

fn producer_result_type_is_compatible(
    plan: &MachinePlan,
    result: &ValueRef,
    expected: &DataTypePlan,
) -> bool {
    match result {
        ValueRef::State(state_id) => plan
            .storage_layout
            .scalar_slots
            .iter()
            .find(|slot| slot.state_id == *state_id)
            .is_none_or(|slot| plan_value_type_matches_data_type(slot.value_type, expected)),
        ValueRef::StateProjection { .. } | ValueRef::Field(_) => true,
        ValueRef::List(_) => matches!(expected, DataTypePlan::List { .. }),
        ValueRef::Constant(constant_id) => plan
            .constants
            .iter()
            .find(|constant| constant.id == *constant_id)
            .is_none_or(|constant| constant_value_matches_data_type(&constant.value, expected)),
        ValueRef::DistributedImport(import_id) => {
            distributed_import_data_type(plan, *import_id).is_none_or(|actual| actual == expected)
        }
        ValueRef::Source(_) | ValueRef::SourcePayload { .. } => false,
    }
}

fn distributed_import_data_type(plan: &MachinePlan, import_id: ImportId) -> Option<&DataTypePlan> {
    plan.distributed_endpoint
        .iter()
        .flat_map(|endpoint| endpoint.endpoint.value_imports.iter())
        .find(|import| import.import_id == import_id)
        .map(|import| &import.data_type)
        .or_else(|| {
            plan.distributed_endpoint
                .iter()
                .flat_map(|endpoint| endpoint.endpoint.remote_call_sites.iter())
                .find(|call| call.result.current_import_id() == Some(import_id))
                .map(|call| &call.result_type)
        })
        .or_else(|| {
            plan.producer_function_instances
                .iter()
                .flat_map(|instance| &instance.arguments)
                .find(|argument| argument.import_id == import_id)
                .map(|argument| &argument.data_type)
        })
}

fn plan_value_type_matches_data_type(actual: PlanValueType, expected: &DataTypePlan) -> bool {
    match actual {
        PlanValueType::Text => matches!(expected, DataTypePlan::Text),
        PlanValueType::Number => matches!(expected, DataTypePlan::Number),
        PlanValueType::Bool => matches!(expected, DataTypePlan::Bool),
        PlanValueType::Bytes { fixed_len } => {
            matches!(expected, DataTypePlan::Bytes { fixed_len: expected_len }
                if expected_len.is_none() || *expected_len == fixed_len)
        }
        PlanValueType::Enum | PlanValueType::Data | PlanValueType::Unknown => true,
    }
}

fn constant_value_matches_data_type(actual: &PlanConstantValue, expected: &DataTypePlan) -> bool {
    match actual {
        PlanConstantValue::Text { .. } => matches!(expected, DataTypePlan::Text),
        PlanConstantValue::Number { .. } => matches!(expected, DataTypePlan::Number),
        PlanConstantValue::Bool { .. } => matches!(expected, DataTypePlan::Bool),
        PlanConstantValue::Bytes { byte_len, .. } => {
            matches!(expected, DataTypePlan::Bytes { fixed_len }
                if fixed_len.is_none() || *fixed_len == Some(*byte_len))
        }
        PlanConstantValue::Enum { .. } | PlanConstantValue::Data { .. } => true,
    }
}

fn distributed_event_route_failure(plan: &MachinePlan) -> Option<String> {
    let endpoint = plan.distributed_endpoint.as_ref()?;
    let route_for = |source_id: SourceId| {
        plan.source_routes
            .iter()
            .find(|route| route.source_id == source_id)
    };
    let route_matches = |route: &SourceRoute,
                         payload_field: &Option<SourcePayloadField>,
                         payload_type: &DataTypePlan| {
        match payload_field {
            Some(field) => route.payload_schema.typed_fields.iter().any(|descriptor| {
                &descriptor.field == field && &descriptor.data_type == payload_type
            }),
            None => true,
        }
    };

    for export in &endpoint.endpoint.event_exports {
        let Some(route) = route_for(export.source_id) else {
            return Some(format!(
                "event export {} references missing source {}",
                export.export_id, export.source_id.0
            ));
        };
        if !route_matches(route, &export.payload_field, &export.payload_type) {
            return Some(format!(
                "event export {} payload does not match source {}",
                export.export_id, export.source_id.0
            ));
        }
    }
    for import in &endpoint.endpoint.event_imports {
        let Some(route) = route_for(import.local_source_id) else {
            return Some(format!(
                "event import {} references missing local source {}",
                import.import_id, import.local_source_id.0
            ));
        };
        if !route_matches(route, &import.payload_field, &import.payload_type) {
            return Some(format!(
                "event import {} payload does not match local source {}",
                import.import_id, import.local_source_id.0
            ));
        }
    }
    None
}

pub fn verify_plan(plan: &MachinePlan) -> Result<PlanVerification, PlanError> {
    plan.row_expressions.validate()?;
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
    let distributed_endpoint_failure = plan.distributed_endpoint.as_ref().and_then(|endpoint| {
        endpoint
            .graph
            .validate(&plan.application.identity)
            .and_then(|()| {
                if endpoint.endpoint.role != plan.program_role {
                    return Err(PlanError::new(
                        "distributed endpoint role does not match the MachinePlan role",
                    ));
                }
                validate_distributed_endpoint_contract(
                    &endpoint.graph,
                    &endpoint.endpoint,
                    Some(&plan.constants),
                    Some(&plan.row_expressions),
                )?;
                validate_distributed_wire_schema(&endpoint.graph, &endpoint.wire_schema)?;
                if endpoint.wire_schema_hash
                    != distributed_wire_schema_hash(&endpoint.wire_schema)?
                {
                    return Err(PlanError::new(
                        "distributed endpoint wire schema hash does not match its linked projection",
                    ));
                }
                validate_distributed_endpoint_wire_contract(
                    &endpoint.endpoint,
                    &endpoint.wire_schema,
                )?;
                distributed_event_route_failure(plan)
                    .map_or(Ok(()), |failure| Err(PlanError::new(failure)))
            })
            .err()
            .map(|error| error.to_string())
    });
    checks.push(PlanCheck {
        id: "distributed-endpoint-canonical-and-resolved".to_owned(),
        pass: distributed_endpoint_failure.is_none(),
        detail: distributed_endpoint_failure.unwrap_or_else(|| match &plan.distributed_endpoint {
            Some(endpoint) => format!(
                "{} distributed endpoint with {} value export(s), {} value import(s), {} event export(s), {} event import(s), {} function export(s), and {} remote call site(s)",
                endpoint.endpoint.role.as_str(),
                endpoint.endpoint.value_exports.len(),
                endpoint.endpoint.value_imports.len(),
                endpoint.endpoint.event_exports.len(),
                endpoint.endpoint.event_imports.len(),
                endpoint.endpoint.function_exports.len(),
                endpoint.endpoint.remote_call_sites.len()
            ),
            None => "standalone machine plan with no distributed endpoint".to_owned(),
        }),
    });
    let producer_function_instances_failure = producer_function_instances_failure(plan);
    checks.push(PlanCheck {
        id: "producer-function-instances-canonical-and-resolved".to_owned(),
        pass: producer_function_instances_failure.is_none(),
        detail: producer_function_instances_failure.unwrap_or_else(|| {
            format!(
                "{} role-local producer function instance(s)",
                plan.producer_function_instances.len()
            )
        }),
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
    let list_authority_failure = list_authority_fields_failure(plan);
    checks.push(PlanCheck {
        id: "list-authority-fields-have-stable-persistence-leaves".to_owned(),
        pass: list_authority_failure.is_none(),
        detail: list_authority_failure.unwrap_or_else(|| {
            "initial and appended row authority fields resolve to stable list memory leaves"
                .to_owned()
        }),
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
        id: "source-routes-have-structural-owners".to_owned(),
        pass: source_route_owners_resolve(plan),
        detail: format!(
            "{} structurally owned source routes",
            plan.source_routes.len()
        ),
    });
    checks.push(PlanCheck {
        id: "state-storage-has-structural-owners".to_owned(),
        pass: scalar_storage_owners_resolve(plan),
        detail: format!(
            "{} structurally owned scalar state slots",
            plan.storage_layout.scalar_slots.len()
        ),
    });
    checks.push(PlanCheck {
        id: "remote-calls-have-structural-owners".to_owned(),
        pass: remote_call_owners_resolve(plan),
        detail: format!(
            "{} structurally owned remote call sites",
            plan.distributed_endpoint
                .as_ref()
                .map_or(0, |endpoint| endpoint.endpoint.remote_call_sites.len())
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
    let role_valid = match plan.program_role {
        ProgramRole::Client => {
            plan.document.is_some()
                && plan.outputs.iter().any(|output| {
                    matches!(
                        output.contract,
                        OutputContractKind::Document | OutputContractKind::Scene
                    )
                })
        }
        ProgramRole::Session | ProgramRole::Server => {
            plan.document.is_none()
                && plan
                    .outputs
                    .iter()
                    .all(|output| matches!(output.contract, OutputContractKind::HostValue { .. }))
        }
    };
    checks.push(PlanCheck {
        id: "program-role-matches-output-boundary".to_owned(),
        pass: role_valid,
        detail: format!(
            "{} role, {} host output root(s), document plan {}",
            plan.program_role.as_str(),
            plan.outputs.len(),
            if plan.document.is_some() {
                "present"
            } else {
                "absent"
            }
        ),
    });
    let session_info_failure = distributed_session_scope_failure(plan);
    checks.push(PlanCheck {
        id: "session-info-intrinsics-match-role-and-scope".to_owned(),
        pass: session_info_failure.is_none(),
        detail: session_info_failure.unwrap_or_else(|| {
            format!(
                "{} role uses only permitted unscoped SessionInfo intrinsics",
                plan.program_role.namespace()
            )
        }),
    });
    let host_port_failure = host_ports_failure(plan);
    checks.push(PlanCheck {
        id: "host-ports-typed-and-resolved".to_owned(),
        pass: host_port_failure.is_none(),
        detail: host_port_failure
            .unwrap_or_else(|| format!("{} typed host port(s)", plan.host_ports.len())),
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
    let initial_expression_failure = initial_expressions_failure(plan);
    checks.push(PlanCheck {
        id: "initial-expressions-resolve".to_owned(),
        pass: initial_expression_failure.is_none(),
        detail: initial_expression_failure.unwrap_or_else(|| {
            "root-initial and row-initial scalar slots carry exact typed expressions".to_owned()
        }),
    });
    let row_ownership_failure = exact_row_field_ownership_failure(plan);
    checks.push(PlanCheck {
        id: "row-fields-have-exact-value-and-authority-ownership".to_owned(),
        pass: row_ownership_failure.is_none(),
        detail: row_ownership_failure.unwrap_or_else(|| {
            "list fields, indexed states, initial rows, and appends use exact structured ownership"
                .to_owned()
        }),
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
        id: "list-mutation-expressions-resolve".to_owned(),
        pass: list_mutation_expressions_resolve(plan),
        detail: "append and remove mutations carry exact typed trigger, gate, item, row owner, and predicate expressions"
            .to_owned(),
    });
    checks.push(PlanCheck {
        id: "list-projection-refs-resolve".to_owned(),
        pass: list_projection_refs_resolve(plan),
        detail: "list projections carry typed source list, value, and output refs".to_owned(),
    });
    let list_index_resource_failure = validate_typed_list_index_resources(plan).err();
    checks.push(PlanCheck {
        id: "typed-list-index-resources-within-target-profile".to_owned(),
        pass: list_index_resource_failure.is_none(),
        detail: list_index_resource_failure.map_or_else(
            || {
                format!(
                    "{} typed index plan(s) fit the {} target resource profile",
                    plan.list_indexes.len(),
                    plan.target_profile.as_str()
                )
            },
            |error| error.to_string(),
        ),
    });
    let list_access_failure = typed_list_access_failure(plan);
    checks.push(PlanCheck {
        id: "typed-list-access-plans-resolve".to_owned(),
        pass: list_access_failure.is_none(),
        detail: list_access_failure.unwrap_or_else(|| {
            format!(
                "{} typed index plan(s) and inline bounded access expressions use canonical row identities",
                plan.list_indexes.len()
            )
        }),
    });
    checks.push(PlanCheck {
        id: "derived-expression-refs-resolve".to_owned(),
        pass: derived_expression_refs_resolve(plan),
        detail: "derived expression operands are present as typed refs".to_owned(),
    });
    checks.push(PlanCheck {
        id: "row-expression-contextual-locals-resolve".to_owned(),
        pass: row_expression_contextual_locals_resolve(plan),
        detail: "contextual collection bodies read only owner-qualified typed locals".to_owned(),
    });
    checks.push(PlanCheck {
        id: "row-expression-list-fields-resolve".to_owned(),
        pass: row_expression_list_fields_resolve(plan),
        detail: "row list lookup expressions use field ids owned by their source list".to_owned(),
    });
    let builtin_call_failure = validate_plan_row_builtin_calls(plan).err();
    checks.push(PlanCheck {
        id: "row-builtin-calls-match-signatures".to_owned(),
        pass: builtin_call_failure.is_none(),
        detail: builtin_call_failure.map_or_else(
            || "typed row builtin calls use canonical inputs and named arguments".to_owned(),
            |error| error.to_string(),
        ),
    });
    let expected_capabilities = derive_capability_summary(plan);
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

pub fn validate_typed_list_index_resources(plan: &MachinePlan) -> Result<(), PlanError> {
    let limits = plan.target_profile.typed_list_index_limits();
    if plan.list_indexes.len() > limits.max_indexes {
        return Err(PlanError::new(format!(
            "target profile `{}` permits at most {} typed indexes, plan declares {}",
            plan.target_profile.as_str(),
            limits.max_indexes,
            plan.list_indexes.len()
        )));
    }

    let slots = plan
        .storage_layout
        .list_slots
        .iter()
        .map(|slot| (slot.list_id, slot))
        .collect::<BTreeMap<_, _>>();
    let mut indexes_by_list = BTreeMap::<ListId, usize>::new();
    let mut indexes_by_field = BTreeMap::<(ListId, FieldId), BTreeSet<PlanListIndexId>>::new();
    let mut startup_rebuild_entries = 0_u64;
    let mut estimated_startup_payload_bytes = 0_u64;

    for index in &plan.list_indexes {
        if index.keys.len() > limits.max_key_components {
            return Err(PlanError::new(format!(
                "typed index {} has {} key components; target profile `{}` permits {}",
                index.id.0,
                index.keys.len(),
                plan.target_profile.as_str(),
                limits.max_key_components
            )));
        }
        let list_count = indexes_by_list.entry(index.source_list).or_default();
        *list_count = list_count.saturating_add(1);
        if *list_count > limits.max_indexes_per_list {
            return Err(PlanError::new(format!(
                "list {} has {} typed indexes; target profile `{}` permits {}",
                index.source_list.0,
                *list_count,
                plan.target_profile.as_str(),
                limits.max_indexes_per_list
            )));
        }
        if *list_count > limits.max_affected_indexes_per_mutation {
            return Err(PlanError::new(format!(
                "a structural mutation of list {} affects {} typed indexes; target profile `{}` permits fanout {}",
                index.source_list.0,
                *list_count,
                plan.target_profile.as_str(),
                limits.max_affected_indexes_per_mutation
            )));
        }

        for key in &index.keys {
            plan.row_expressions
                .visit_list_fields(key.expression, &mut |list, field| {
                    if list == index.source_list {
                        indexes_by_field
                            .entry((list, field))
                            .or_default()
                            .insert(index.id);
                    }
                })?;
        }

        let Some(slot) = slots.get(&index.source_list).copied() else {
            continue;
        };
        let expansion = index
            .keys
            .iter()
            .find_map(|key| match key.multiplicity {
                PlanListIndexKeyMultiplicity::One => None,
                PlanListIndexKeyMultiplicity::ListItems { max_items } => Some(u64::from(max_items)),
            })
            .unwrap_or(1);
        if let Some(capacity) = slot.capacity {
            let capacity = usize_to_u64(capacity).saturating_mul(expansion);
            if capacity > limits.max_entries_per_index {
                return Err(PlanError::new(format!(
                    "typed index {} may retain {capacity} entries; target profile `{}` permits {}",
                    index.id.0,
                    plan.target_profile.as_str(),
                    limits.max_entries_per_index
                )));
            }
        }
        let startup_rows = list_startup_row_count(slot).saturating_mul(expansion);
        startup_rebuild_entries = startup_rebuild_entries.saturating_add(startup_rows);
        let estimated_entry_bytes = STATIC_INDEX_IDENTITY_BYTES_PER_ENTRY.saturating_add(
            usize_to_u64(index.keys.len()).saturating_mul(STATIC_ESTIMATED_KEY_BYTES_PER_COMPONENT),
        );
        estimated_startup_payload_bytes = estimated_startup_payload_bytes
            .saturating_add(startup_rows.saturating_mul(estimated_entry_bytes));
    }

    if let Some(((list, field), indexes)) = indexes_by_field
        .iter()
        .find(|(_, indexes)| indexes.len() > limits.max_affected_indexes_per_mutation)
    {
        return Err(PlanError::new(format!(
            "field {} on list {} affects {} typed indexes; target profile `{}` permits fanout {}",
            field.0,
            list.0,
            indexes.len(),
            plan.target_profile.as_str(),
            limits.max_affected_indexes_per_mutation
        )));
    }
    if startup_rebuild_entries > limits.max_startup_rebuild_entries {
        return Err(PlanError::new(format!(
            "typed index startup rebuild requires {startup_rebuild_entries} entries; target profile `{}` permits {}",
            plan.target_profile.as_str(),
            limits.max_startup_rebuild_entries
        )));
    }
    if estimated_startup_payload_bytes > limits.max_total_payload_bytes {
        return Err(PlanError::new(format!(
            "typed index startup payload estimate is {estimated_startup_payload_bytes} bytes; target profile `{}` permits {}",
            plan.target_profile.as_str(),
            limits.max_total_payload_bytes
        )));
    }
    Ok(())
}

fn list_startup_row_count(slot: &ListStorageSlot) -> u64 {
    if let Some(range) = &slot.range {
        if range.to < range.from {
            return 0;
        }
        return i128::from(range.to)
            .saturating_sub(i128::from(range.from))
            .saturating_add(1)
            .try_into()
            .unwrap_or(u64::MAX);
    }
    usize_to_u64(slot.initial_rows.len())
}

fn usize_to_u64(value: usize) -> u64 {
    value.try_into().unwrap_or(u64::MAX)
}

fn typed_list_access_failure(plan: &MachinePlan) -> Option<String> {
    let fingerprint_context = match TypedListViewFingerprintContext::new(plan) {
        Ok(context) => context,
        Err(error) => return Some(error.to_string()),
    };
    let list_exists = |list_id| {
        plan.storage_layout
            .list_slots
            .iter()
            .any(|slot| slot.list_id == list_id)
    };
    for (position, index) in plan.list_indexes.iter().enumerate() {
        if index.id.0 != position || !list_exists(index.source_list) {
            return Some(format!(
                "typed list index {} has noncanonical identity or missing source list",
                index.id.0
            ));
        }
        if index.keys.len() > MAX_TYPED_LIST_INDEX_KEYS {
            return Some(format!(
                "typed list index {} has {} keys; maximum is {}",
                index.id.0,
                index.keys.len(),
                MAX_TYPED_LIST_INDEX_KEYS
            ));
        }
        let expanded_keys = index
            .keys
            .iter()
            .filter(|key| {
                matches!(
                    key.multiplicity,
                    PlanListIndexKeyMultiplicity::ListItems { .. }
                )
            })
            .count();
        if expanded_keys > 1 {
            return Some(format!(
                "typed list index {} has more than one expanded key component",
                index.id.0
            ));
        }
        for key in &index.keys {
            if matches!(
                key.multiplicity,
                PlanListIndexKeyMultiplicity::ListItems { max_items }
                    if max_items == 0 || max_items > MAX_TYPED_LIST_EXPANDED_KEYS_PER_ROW
            ) {
                return Some(format!(
                    "typed list index {} has an invalid per-row key expansion limit",
                    index.id.0
                ));
            }
            if !plan
                .row_expressions
                .contextual_locals_resolve_with(key.expression, key.owner, key.row_local)
                .unwrap_or(false)
                || !row_expression_list_fields_resolve_inner(plan, key.expression)
                || !row_expression_cpu_evaluable(&plan.row_expressions, key.expression)
            {
                return Some(format!(
                    "typed list index {} has an unresolved or non-executable key",
                    index.id.0
                ));
            }
            let mut invalid_input = false;
            if plan
                .row_expressions
                .visit_inputs(key.expression, &mut |input| {
                    invalid_input |= !matches!(
                        input,
                        ValueRef::List(list) if list == index.source_list
                    ) && !matches!(input, ValueRef::Constant(_));
                })
                .is_err()
            {
                invalid_input = true;
            }
            if invalid_input {
                return Some(format!(
                    "typed list index {} key captures data outside its canonical source row",
                    index.id.0
                ));
            }
            if matches!(
                key.kind,
                PlanListIndexKeyKind::ClosedTag { type_id } if type_id == [0; 16]
            ) {
                return Some(format!(
                    "typed list index {} has an invalid closed-tag identity",
                    index.id.0
                ));
            }
            match key.kind {
                PlanListIndexKeyKind::ClosedTag { type_id }
                    if closed_tag_type_id(&key.closed_tags) != Some(type_id) =>
                {
                    return Some(format!(
                        "typed list index {} has a noncanonical closed-tag table",
                        index.id.0
                    ));
                }
                PlanListIndexKeyKind::Number
                | PlanListIndexKeyKind::Text
                | PlanListIndexKeyKind::Bool
                    if !key.closed_tags.is_empty() =>
                {
                    return Some(format!(
                        "typed list index {} attaches tags to a non-tag key",
                        index.id.0
                    ));
                }
                PlanListIndexKeyKind::ClosedTag { .. }
                | PlanListIndexKeyKind::Number
                | PlanListIndexKeyKind::Text
                | PlanListIndexKeyKind::Bool => {}
            }
        }
    }
    let mut access_failure = None;
    if let Err(error) = visit_plan_row_expressions(plan, &mut |_, expression| {
        if access_failure.is_some() {
            return;
        }
        match expression {
            PlanRowExpressionNode::ListAccess { access } => {
                access_failure = validate_typed_list_access(plan, access);
            }
            PlanRowExpressionNode::ListPage { page } => {
                access_failure = validate_typed_list_access(plan, &page.access);
                if access_failure.is_none()
                    && (!root_row_expression_cpu_evaluable(&plan.row_expressions, page.after)
                        || !plan
                            .row_expressions
                            .contextual_locals_resolve(page.after)
                            .unwrap_or(false)
                        || !row_expression_list_fields_resolve_inner(plan, page.after))
                {
                    access_failure = Some(format!(
                        "typed list page on index {} has an unresolved cursor",
                        page.access.index.0
                    ));
                }
                if access_failure.is_none()
                    && page.view_limit.as_ref().is_some_and(|limit| {
                        !root_row_expression_cpu_evaluable(&plan.row_expressions, *limit)
                            || !plan
                                .row_expressions
                                .contextual_locals_resolve(*limit)
                                .unwrap_or(false)
                            || !row_expression_list_fields_resolve_inner(plan, *limit)
                    })
                {
                    access_failure = Some(format!(
                        "typed list page on index {} has an unresolved semantic limit",
                        page.access.index.0
                    ));
                }
                if access_failure.is_none() {
                    let fingerprint =
                        plan.list_indexes
                            .get(page.access.index.0)
                            .and_then(|index| {
                                fingerprint_context
                                    .fingerprint(
                                        index.source_list,
                                        &page.access.semantic_order,
                                        &page.access.guard,
                                        &page.access.filters,
                                        &page.access.maps,
                                        &page.view_limit,
                                    )
                                    .ok()
                            });
                    if fingerprint != Some(page.view_fingerprint) {
                        access_failure = Some(format!(
                            "typed list page on index {} has a noncanonical view fingerprint",
                            page.access.index.0
                        ));
                    }
                }
            }
            PlanRowExpressionNode::BoundedListPage { page } => {
                if page.max_items == 0
                    || page.max_items > 10_000
                    || !root_row_expression_cpu_evaluable(&plan.row_expressions, page.view)
                    || !root_row_expression_cpu_evaluable(&plan.row_expressions, page.size)
                    || !root_row_expression_cpu_evaluable(&plan.row_expressions, page.after)
                    || !plan
                        .row_expressions
                        .contextual_locals_resolve(page.view)
                        .unwrap_or(false)
                    || !plan
                        .row_expressions
                        .contextual_locals_resolve(page.size)
                        .unwrap_or(false)
                    || !plan
                        .row_expressions
                        .contextual_locals_resolve(page.after)
                        .unwrap_or(false)
                    || !row_expression_list_fields_resolve_inner(plan, page.view)
                    || !row_expression_list_fields_resolve_inner(plan, page.size)
                    || !row_expression_list_fields_resolve_inner(plan, page.after)
                {
                    access_failure = Some(
                        "bounded typed list page has unresolved expressions or invalid limits"
                            .to_owned(),
                    );
                }
                if access_failure.is_none()
                    && fingerprint_context.bounded_fingerprint(page.view).ok()
                        != Some(page.view_fingerprint)
                {
                    access_failure = Some(
                        "bounded typed list page has a noncanonical view fingerprint".to_owned(),
                    );
                }
            }
            _ => {}
        }
    }) {
        return Some(error.to_string());
    }
    access_failure
}

fn validate_typed_list_access(plan: &MachinePlan, access: &PlanListAccess) -> Option<String> {
    let Some(index) = plan.list_indexes.get(access.index.0) else {
        return Some(format!(
            "typed list access references missing index {}",
            access.index.0
        ));
    };
    let semantic_offset = index.keys.len().saturating_sub(access.semantic_order.len());
    let semantic_order_matches = access.semantic_order.len() <= index.keys.len()
        && index.keys[semantic_offset..]
            .iter()
            .zip(&access.semantic_order)
            .all(|(physical, semantic)| {
                physical.kind == semantic.kind
                    && physical.closed_tags == semantic.closed_tags
                    && physical.direction == semantic.direction
                    && physical.multiplicity == semantic.multiplicity
                    && plan
                        .row_expressions
                        .structurally_equivalent_with_local_remap(
                            semantic.expression,
                            (semantic.owner, semantic.row_local),
                            (physical.owner, physical.row_local),
                            physical.expression,
                        )
                        .unwrap_or(false)
            });
    if !semantic_order_matches {
        return Some(format!(
            "typed list access on index {} has a physical order incompatible with its semantic order",
            access.index.0
        ));
    }
    if access.semantic_order.iter().any(|key| {
        matches!(
            key.multiplicity,
            PlanListIndexKeyMultiplicity::ListItems { .. }
        )
    }) {
        return Some(format!(
            "typed list access on index {} exposes an expanded physical key as semantic ordering",
            access.index.0
        ));
    }
    if let Some(expanded_position) = index.keys.iter().position(|key| {
        matches!(
            key.multiplicity,
            PlanListIndexKeyMultiplicity::ListItems { .. }
        )
    }) && !typed_list_selection_fixes_component(&access.selection, expanded_position)
    {
        return Some(format!(
            "typed list access on index {} does not fix its expanded key component before traversal",
            access.index.0
        ));
    }
    if access
        .exhaustive_candidate_limit
        .is_some_and(|limit| limit == 0 || limit > 10_000)
    {
        return Some(format!(
            "typed list access on index {} has an invalid exhaustive candidate limit",
            access.index.0
        ));
    }
    if !root_row_expression_cpu_evaluable(&plan.row_expressions, access.limit)
        || !plan
            .row_expressions
            .contextual_locals_resolve(access.limit)
            .unwrap_or(false)
        || !row_expression_list_fields_resolve_inner(plan, access.limit)
    {
        return Some(format!(
            "typed list access on index {} has an unresolved limit",
            access.index.0
        ));
    }
    if let Some(guard) = &access.guard {
        let cpu_evaluable = root_row_expression_cpu_evaluable(&plan.row_expressions, *guard);
        let contextual_locals_resolve = plan
            .row_expressions
            .contextual_locals_resolve(*guard)
            .unwrap_or(false);
        let list_fields_resolve = row_expression_list_fields_resolve_inner(plan, *guard);
        if !cpu_evaluable || !contextual_locals_resolve || !list_fields_resolve {
            return Some(format!(
                "typed list access on index {} has an unresolved guard: cpu_evaluable={cpu_evaluable}, contextual_locals_resolve={contextual_locals_resolve}, list_fields_resolve={list_fields_resolve}, expression={guard:?}",
                access.index.0
            ));
        }
    }
    for filter in &access.filters {
        if !plan
            .row_expressions
            .contextual_locals_resolve_with(filter.predicate, filter.owner, filter.row_local)
            .unwrap_or(false)
            || !row_expression_list_fields_resolve_inner(plan, filter.predicate)
            || !row_expression_cpu_evaluable(&plan.row_expressions, filter.predicate)
        {
            return Some(format!(
                "typed list access on index {} has an unresolved filter",
                access.index.0
            ));
        }
    }
    for map in &access.maps {
        if !plan
            .row_expressions
            .contextual_locals_resolve_with(map.body, map.owner, map.row_local)
            .unwrap_or(false)
            || !row_expression_list_fields_resolve_inner(plan, map.body)
            || !row_expression_cpu_evaluable(&plan.row_expressions, map.body)
            || map.captures.iter().any(|capture| {
                !plan
                    .row_expressions
                    .contextual_locals_resolve_with(capture.value, map.owner, map.row_local)
                    .unwrap_or(false)
                    || !row_expression_list_fields_resolve_inner(plan, capture.value)
                    || !row_expression_cpu_evaluable(&plan.row_expressions, capture.value)
            })
        {
            return Some(format!(
                "typed list access on index {} has an unresolved map",
                access.index.0
            ));
        }
    }
    if let Some(failure) = typed_list_selection_shape_failure(&access.selection) {
        return Some(format!(
            "typed list access on index {} has invalid selection: {failure}",
            access.index.0
        ));
    }
    validate_typed_list_selection(plan, index, &access.selection, !access.filters.is_empty()).map(
        |failure| {
            format!(
                "typed list access on index {} has invalid selection: {failure}",
                access.index.0
            )
        },
    )
}

fn typed_list_selection_fixes_component(
    selection: &PlanListAccessSelection,
    component: usize,
) -> bool {
    let mut stack = vec![selection];
    while let Some(selection) = stack.pop() {
        match selection {
            PlanListAccessSelection::OrderedStart => return false,
            PlanListAccessSelection::KeyPrefix { values } => {
                if values.len() <= component {
                    return false;
                }
            }
            PlanListAccessSelection::TextPrefix { leading, .. }
            | PlanListAccessSelection::ComponentRange { leading, .. } => {
                if leading.len() <= component {
                    return false;
                }
            }
            PlanListAccessSelection::Union { branches }
            | PlanListAccessSelection::Intersection { branches } => {
                stack.extend(branches);
            }
        }
    }
    true
}

fn typed_list_selection_shape_failure(selection: &PlanListAccessSelection) -> Option<String> {
    fn visit(
        selection: &PlanListAccessSelection,
        depth: usize,
        leaves: &mut usize,
    ) -> Option<String> {
        if depth > MAX_TYPED_LIST_SELECTION_DEPTH {
            return Some(format!(
                "selection depth {depth} exceeds {MAX_TYPED_LIST_SELECTION_DEPTH}"
            ));
        }
        match selection {
            PlanListAccessSelection::Union { branches }
            | PlanListAccessSelection::Intersection { branches } => {
                if !(2..=64).contains(&branches.len()) {
                    return Some(format!(
                        "composite selection has {} branches; expected 2..=64",
                        branches.len()
                    ));
                }
                for branch in branches {
                    if let Some(failure) = visit(branch, depth.saturating_add(1), leaves) {
                        return Some(failure);
                    }
                }
                None
            }
            PlanListAccessSelection::OrderedStart
            | PlanListAccessSelection::KeyPrefix { .. }
            | PlanListAccessSelection::TextPrefix { .. }
            | PlanListAccessSelection::ComponentRange { .. } => {
                *leaves = leaves.saturating_add(1);
                (*leaves > MAX_TYPED_LIST_SELECTION_LEAVES).then(|| {
                    format!("selection has more than {MAX_TYPED_LIST_SELECTION_LEAVES} leaves")
                })
            }
        }
    }

    visit(selection, 1, &mut 0)
}

fn validate_typed_list_selection(
    plan: &MachinePlan,
    index: &PlanListIndex,
    selection: &PlanListAccessSelection,
    residual_filter: bool,
) -> Option<String> {
    let expressions_resolve = |selection: &PlanListAccessSelection| {
        selection.all_expressions(&mut |expression| {
            root_row_expression_cpu_evaluable(&plan.row_expressions, expression)
                && plan
                    .row_expressions
                    .contextual_locals_resolve(expression)
                    .unwrap_or(false)
                && row_expression_list_fields_resolve_inner(plan, expression)
        })
    };
    match selection {
        PlanListAccessSelection::OrderedStart if residual_filter => {
            Some("residual filter would require an unseekable ordered scan".to_owned())
        }
        PlanListAccessSelection::OrderedStart => None,
        PlanListAccessSelection::KeyPrefix { values } => {
            if values.is_empty() || values.len() > index.keys.len() {
                Some(format!(
                    "structural prefix has {} values for {} key components",
                    values.len(),
                    index.keys.len()
                ))
            } else if !expressions_resolve(selection) {
                Some("structural prefix has unresolved values".to_owned())
            } else {
                None
            }
        }
        PlanListAccessSelection::TextPrefix { leading, .. } => {
            if leading.len() >= index.keys.len()
                || !matches!(
                    index.keys.get(leading.len()).map(|key| key.kind),
                    Some(PlanListIndexKeyKind::Text)
                )
            {
                Some("Text prefix does not target a Text key component".to_owned())
            } else if !expressions_resolve(selection) {
                Some("Text prefix has unresolved values".to_owned())
            } else {
                None
            }
        }
        PlanListAccessSelection::ComponentRange {
            leading,
            lower,
            upper,
        } => {
            if leading.len() >= index.keys.len() {
                Some("component range has no target key component".to_owned())
            } else if lower.is_none() && upper.is_none() {
                Some("component range has no finite bound".to_owned())
            } else if !expressions_resolve(selection) {
                Some("component range has unresolved values".to_owned())
            } else {
                None
            }
        }
        PlanListAccessSelection::Union { branches }
        | PlanListAccessSelection::Intersection { branches } => {
            if !(2..=64).contains(&branches.len()) {
                return Some(format!(
                    "composite selection has {} branches; expected 2..=64",
                    branches.len()
                ));
            }
            branches.iter().find_map(|branch| {
                validate_typed_list_selection(plan, index, branch, residual_filter)
            })
        }
    }
}

fn visit_plan_row_expressions(
    plan: &MachinePlan,
    visitor: &mut impl FnMut(PlanRowExpressionId, &PlanRowExpressionNode),
) -> Result<(), PlanError> {
    let mut roots = Vec::new();
    for index in &plan.list_indexes {
        for key in &index.keys {
            roots.push(key.expression);
        }
    }
    for slot in &plan.storage_layout.scalar_slots {
        if let ScalarInitializerPlan::Expression { expression } = &slot.initializer {
            roots.push(*expression);
        }
    }
    if let Some(document) = &plan.document {
        roots.extend(
            document
                .expressions
                .iter()
                .filter_map(|expression| match &expression.op {
                    DocumentExprOp::RuntimeExpression { expression, .. } => Some(*expression),
                    _ => None,
                }),
        );
    }
    if let Some(endpoint) = &plan.distributed_endpoint {
        for call in &endpoint.endpoint.remote_call_sites {
            for argument in &call.arguments {
                roots.push(argument.value);
            }
            for arm in &call.invocation_arms {
                roots.push(arm.gate);
            }
        }
    }
    for op in plan.regions.iter().flat_map(|region| &region.ops) {
        match &op.kind {
            PlanOpKind::DerivedValue {
                expression: Some(expression),
                ..
            } => collect_derived_row_expression_roots(expression, &mut roots),
            PlanOpKind::StateUpdate { value, effect, .. } => {
                if let Some(value) = value {
                    roots.push(*value);
                }
                if let Some(effect) = effect {
                    roots.push(effect.gate);
                    for field in &effect.intent_fields {
                        roots.push(field.expression);
                    }
                }
            }
            PlanOpKind::ListMutation { mutation } => match mutation {
                PlanListMutation::Append(append) => {
                    roots.push(append.gate);
                    roots.push(append.item);
                }
                PlanListMutation::Remove(remove) => {
                    roots.push(remove.gate);
                    roots.push(remove.predicate);
                }
            },
            PlanOpKind::SourceRoute
            | PlanOpKind::DerivedValue {
                expression: None, ..
            }
            | PlanOpKind::ListProjection { .. }
            | PlanOpKind::DependencyEdge => {}
        }
    }
    plan.row_expressions.visit_roots(roots, visitor)
}

pub fn validate_plan_row_builtin_calls(plan: &MachinePlan) -> Result<(), PlanError> {
    let mut failure = None;
    visit_plan_row_expressions(plan, &mut |_, expression| {
        if failure.is_some() {
            return;
        }
        if let PlanRowExpressionNode::BuiltinCall {
            function,
            input,
            args,
        } = expression
        {
            failure = function.validate_call(*input, args).err();
        }
    })?;
    failure.map_or(Ok(()), Err)
}

fn collect_derived_row_expression_roots(
    expression: &PlanDerivedExpression,
    roots: &mut Vec<PlanRowExpressionId>,
) {
    let mut stack = vec![expression];
    while let Some(expression) = stack.pop() {
        match expression {
            PlanDerivedExpression::MaterializeList { expression, .. } => {
                stack.push(expression);
            }
            PlanDerivedExpression::SourceEventTransform { default, arms, .. } => {
                roots.push(*default);
                roots.extend(arms.iter().map(|arm| arm.value));
            }
            PlanDerivedExpression::BoolAnd { left, right } => {
                stack.push(right);
                stack.push(left);
            }
            PlanDerivedExpression::BoolNotExpression { input } => {
                stack.push(input);
            }
            PlanDerivedExpression::RowExpression { expression }
            | PlanDerivedExpression::MaterializedRowField { expression, .. } => {
                roots.push(*expression);
            }
            PlanDerivedExpression::SourceKeyTextTrimNonEmpty { .. }
            | PlanDerivedExpression::BoolNot { .. }
            | PlanDerivedExpression::NumberCompareConst { .. }
            | PlanDerivedExpression::ValueCompare { .. } => {}
        }
    }
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

fn list_authority_fields_failure(plan: &MachinePlan) -> Option<String> {
    for memory in &plan.persistence.lists {
        let Some(slot) = plan
            .storage_layout
            .list_slots
            .iter()
            .find(|slot| slot.id == memory.runtime_slot)
        else {
            return Some(format!(
                "persistent list `{}` references missing runtime slot {}",
                memory.semantic_path, memory.runtime_slot.0
            ));
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
                let PlanOpKind::ListMutation {
                    mutation: PlanListMutation::Append(append),
                } = &op.kind
                else {
                    return None;
                };
                (op.output == Some(ValueRef::List(slot.list_id))).then_some(append)
            })
        {
            authoritative_fields.extend(append.fields.iter().map(|field| field.field_id));
        }
        let missing = authoritative_fields
            .difference(&stable_fields)
            .copied()
            .collect::<Vec<_>>();
        if !missing.is_empty() {
            return Some(format!(
                "persistent list `{}` slot {} has authoritative fields {missing:?} without stable persistence leaves; stable fields={stable_fields:?}",
                memory.semantic_path, slot.id.0
            ));
        }
    }
    None
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
    let mut invocations_by_id = BTreeMap::<EffectInvocationId, &EffectInvocationPlan>::new();
    for op in plan.regions.iter().flat_map(|region| &region.ops) {
        let PlanOpKind::StateUpdate { value, effect, .. } = &op.kind else {
            continue;
        };
        if value.is_some() == effect.is_some() {
            return Some(format!(
                "state update op {} must have exactly one value or effect",
                op.id.0
            ));
        }
        let Some(invocation) = effect else {
            continue;
        };
        if !plan_owner_resolves(plan, &invocation.owner) {
            return Some(format!(
                "effect invocation {} has an invalid structural owner",
                invocation.invocation_id
            ));
        }
        if let Some(previous) = invocations_by_id.insert(invocation.invocation_id, invocation)
            && (previous.effect_id != invocation.effect_id
                || previous.owner != invocation.owner
                || previous.idempotency_key != invocation.idempotency_key
                || previous.result != invocation.result
                || previous.barrier != invocation.barrier)
        {
            return Some(format!(
                "effect invocation {} is repeated with inconsistent lane metadata",
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
        let Some(effect_schema) = contract.schema.as_ref() else {
            return Some(format!(
                "effect invocation {} has no typed effect schema",
                invocation.invocation_id
            ));
        };
        let outbox = plan
            .persistence
            .effect_outbox
            .iter()
            .find(|schema| schema.effect_id == invocation.effect_id);
        match (&contract.replay, outbox) {
            (EffectReplay::Idempotent { .. }, Some(outbox))
                if outbox.invocation_ids.contains(&invocation.invocation_id) => {}
            (EffectReplay::Idempotent { .. }, _) => {
                return Some(format!(
                    "effect invocation {} is absent from its durable outbox schema",
                    invocation.invocation_id
                ));
            }
            (EffectReplay::ReadOnly, None) => {}
            (EffectReplay::ReadOnly, Some(_)) => {
                return Some(format!(
                    "read-only effect invocation {} must not use a durable outbox",
                    invocation.invocation_id
                ));
            }
            (EffectReplay::ProcessScoped, None) => {}
            (EffectReplay::ProcessScoped, Some(_)) => {
                return Some(format!(
                    "process-scoped effect invocation {} must not use a durable outbox",
                    invocation.invocation_id
                ));
            }
            (EffectReplay::NonReplayable, _) => {
                return Some(format!(
                    "non-replayable effect invocation {} cannot be executed safely",
                    invocation.invocation_id
                ));
            }
        }
        if !row_expression_refs_resolve(&plan.row_expressions, op, invocation.gate)
            || !plan
                .row_expressions
                .contextual_locals_resolve(invocation.gate)
                .unwrap_or(false)
            || !invocation.intent_fields.iter().all(|field| {
                row_expression_refs_resolve(&plan.row_expressions, op, field.expression)
                    && plan
                        .row_expressions
                        .contextual_locals_resolve(field.expression)
                        .unwrap_or(false)
            })
            || !effect_intent_fields_match_schema(
                &invocation.intent_fields,
                &effect_schema.intent_type,
            )
            || !effect_result_route_matches(
                plan,
                op,
                &invocation.result,
                &effect_schema.result_type,
            )
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
    let durable_invocation_ids = plan
        .regions
        .iter()
        .flat_map(|region| &region.ops)
        .filter_map(|op| match &op.kind {
            PlanOpKind::StateUpdate {
                effect: Some(invocation),
                ..
            } => plan
                .effects
                .iter()
                .find(|contract| contract.effect_id == invocation.effect_id)
                .is_some_and(|contract| matches!(contract.replay, EffectReplay::Idempotent { .. }))
                .then_some(invocation.invocation_id),
            _ => None,
        })
        .collect::<BTreeSet<_>>();
    if bound_invocation_ids != durable_invocation_ids {
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
    _plan: &MachinePlan,
    op: &PlanOp,
    route: &EffectResultRoute,
    _result_type: &DataTypePlan,
) -> bool {
    match route {
        EffectResultRoute::Target { target, policy } => {
            op.output.as_ref() == Some(target) && *policy == route.policy()
        }
    }
}

fn source_route_owners_resolve(plan: &MachinePlan) -> bool {
    plan.source_routes.iter().all(|route| {
        if route.scoped != route.scope_id.is_some() {
            return false;
        }
        if route.owner.static_owner.is_root() && !route.owner.ancestors.is_empty() {
            return false;
        }
        if route.scoped {
            let Some(scope) = route.scope_id else {
                return false;
            };
            if route.owner.static_owner.is_root()
                || route.owner.ancestors.last().map(|ancestor| ancestor.scope) != Some(scope)
            {
                return false;
            }
        } else if !route.owner.ancestors.is_empty() {
            return false;
        }
        route.owner.ancestors.iter().all(|ancestor| {
            plan.storage_layout
                .list_slots
                .iter()
                .any(|slot| slot.list_id == ancestor.list && slot.scope_id == Some(ancestor.scope))
        })
    })
}

fn plan_owner_resolves(plan: &MachinePlan, owner: &PlanOwner) -> bool {
    if owner.static_owner.is_root() && !owner.ancestors.is_empty() {
        return false;
    }
    owner.ancestors.iter().all(|ancestor| {
        plan.storage_layout
            .list_slots
            .iter()
            .any(|slot| slot.list_id == ancestor.list && slot.scope_id == Some(ancestor.scope))
    })
}

fn scalar_storage_owners_resolve(plan: &MachinePlan) -> bool {
    plan.storage_layout.scalar_slots.iter().all(|slot| {
        plan_owner_resolves(plan, &slot.owner)
            && match slot.scope_id {
                Some(scope) => {
                    slot.indexed
                        && !slot.owner.static_owner.is_root()
                        && slot.owner.ancestors.last().map(|ancestor| ancestor.scope) == Some(scope)
                }
                None => !slot.indexed && slot.owner.ancestors.is_empty(),
            }
    })
}

fn remote_call_owners_resolve(plan: &MachinePlan) -> bool {
    plan.distributed_endpoint.as_ref().is_none_or(|endpoint| {
        endpoint
            .endpoint
            .remote_call_sites
            .iter()
            .all(|call| plan_owner_resolves(plan, &call.owner))
    })
}

fn required_effect_operations(plan: &MachinePlan) -> BTreeSet<&'static str> {
    let mut operations = BTreeSet::new();
    for op in plan.regions.iter().flat_map(|region| &region.ops) {
        match &op.kind {
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
                OutputValueRef::RuntimeValue { value, list_fields },
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
                if !output_list_fields_resolve(plan, value, list_fields) {
                    return Some(format!(
                        "host output root `{}` has invalid exact list-field projections",
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
        ValueRef::StateProjection { state_id, .. } => plan
            .storage_layout
            .scalar_slots
            .iter()
            .any(|slot| slot.state_id == *state_id && !slot.indexed),
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
        ValueRef::DistributedImport(import_id) => {
            distributed_import_data_type(plan, *import_id).is_some()
        }
        ValueRef::Source(_) | ValueRef::SourcePayload { .. } => false,
    }
}

fn output_list_fields_resolve(
    plan: &MachinePlan,
    value: &ValueRef,
    fields: &[OutputListFieldRef],
) -> bool {
    let ValueRef::List(list_id) = value else {
        return fields.is_empty();
    };
    let Some(slot) = plan
        .storage_layout
        .list_slots
        .iter()
        .find(|slot| slot.list_id == *list_id)
    else {
        return false;
    };
    let mut names = BTreeSet::new();
    let mut ids = BTreeSet::new();
    fields.iter().all(|field| {
        field.list_id == *list_id
            && !field.name.is_empty()
            && names.insert(field.name.as_str())
            && ids.insert(field.field_id)
            && slot.row_fields.iter().any(|candidate| {
                candidate.field_id == field.field_id
                    && candidate.name == field.name
                    && candidate.role.is_value()
            })
    })
}

fn persistence_runtime_slots_consistent(plan: &MachinePlan) -> bool {
    let mut linked_slots = BTreeSet::new();
    let all_indexed_fields = plan
        .storage_layout
        .scalar_slots
        .iter()
        .filter_map(|slot| slot.indexed.then_some(slot.indexed_field_id).flatten())
        .collect::<BTreeSet<_>>();
    let mut durable_indexed_fields = BTreeSet::new();
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
        if memory.kind == MemoryKind::IndexedField {
            let Some(field) = slot.indexed_field_id else {
                return false;
            };
            durable_indexed_fields.insert(field);
        }
    }

    let mut linked_durable_indexed_fields = BTreeSet::new();
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
        let expected_fields = slot.row_field_ids().collect::<BTreeSet<_>>();
        if runtime_fields.len() != list.row_fields.len()
            || !runtime_fields.is_subset(&expected_fields)
            || runtime_fields
                .intersection(&all_indexed_fields)
                .any(|field| !durable_indexed_fields.contains(field))
            || !linked_slots.insert(list.runtime_slot)
        {
            return false;
        }
        linked_durable_indexed_fields.extend(
            runtime_fields
                .intersection(&durable_indexed_fields)
                .copied(),
        );
    }

    linked_slots.len() == plan.persistence.memory.len() + plan.persistence.lists.len()
        && linked_durable_indexed_fields == durable_indexed_fields
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

#[derive(Serialize)]
struct CanonicalDistributedWireSchema<'a> {
    namespace: &'static str,
    schema: &'a DistributedWireSchemaPlan,
}

fn distributed_wire_schema_hash(schema: &DistributedWireSchemaPlan) -> Result<[u8; 32], PlanError> {
    canonical_sha256(&CanonicalDistributedWireSchema {
        namespace: "boon.distributed-wire-schema.v1",
        schema,
    })
}

/// Hashes only the linked distributed wire projection. Endpoint-local source
/// IDs, value refs, argument expressions, and function bodies are omitted.
pub fn distributed_graph_schema_hash(graph: &DistributedGraphPlan) -> Result<[u8; 32], PlanError> {
    distributed_wire_schema_hash(&graph.wire_schema)
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

pub fn derive_capability_summary(plan: &MachinePlan) -> CapabilitySummary {
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
        .list_slots
        .iter()
        .filter(|slot| matches!(slot.initializer_kind, ListInitializerKind::Unknown))
        .count()
        + non_executable_constant_payload_count(&plan.constants);
    let unknown_plan_op_count = unknown_region_op_count + unknown_storage_op_count;
    let typed_lowering_executable =
        unresolved_executable_ref_count == 0 && unknown_plan_op_count == 0;
    let cpu_plan_executor_unsupported_op_count = cpu_plan_executor_unsupported_op_count(
        &plan.row_expressions,
        &plan.regions,
        &plan.storage_layout.scalar_slots,
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
    arena: &PlanRowExpressionArena,
    regions: &[OperationRegion],
    scalar_slots: &[ScalarStorageSlot],
) -> usize {
    cpu_plan_executor_unsupported_ops_for_parts(arena, regions, scalar_slots).len()
}

/// Returns the exact operations that the CPU executor cannot currently run.
///
/// This is the diagnostic counterpart to the capability-summary count and is
/// intentionally derived from the same support predicate.
pub fn cpu_plan_executor_unsupported_ops(plan: &MachinePlan) -> Vec<&PlanOp> {
    cpu_plan_executor_unsupported_ops_for_parts(
        &plan.row_expressions,
        &plan.regions,
        &plan.storage_layout.scalar_slots,
    )
}

fn cpu_plan_executor_unsupported_ops_for_parts<'a>(
    arena: &PlanRowExpressionArena,
    regions: &'a [OperationRegion],
    scalar_slots: &[ScalarStorageSlot],
) -> Vec<&'a PlanOp> {
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
    regions
        .iter()
        .flat_map(|region| &region.ops)
        .filter(|op| {
            !cpu_plan_executor_supports_whole_plan_op(
                arena,
                scalar_slots,
                op,
                &supported_list_projection_outputs,
            )
        })
        .collect()
}

pub fn cpu_plan_executor_supports_whole_plan_op(
    arena: &PlanRowExpressionArena,
    scalar_slots: &[ScalarStorageSlot],
    op: &PlanOp,
    supported_list_projection_outputs: &BTreeSet<FieldId>,
) -> bool {
    match &op.kind {
        PlanOpKind::SourceRoute | PlanOpKind::DependencyEdge => true,
        PlanOpKind::DerivedValue {
            expression: Some(PlanDerivedExpression::MaterializeList { expression, .. }),
            ..
        } if matches!(
            expression.as_ref(),
            PlanDerivedExpression::RowExpression { expression }
                if matches!(
                    arena.get(*expression),
                    Some(PlanRowExpressionNode::ListAccess { .. })
                )
        ) =>
        {
            op.unresolved_executable_ref_count == 0
        }
        PlanOpKind::StateUpdate {
            trigger: _,
            value,
            effect,
        } => {
            op.unresolved_executable_ref_count == 0
                && state_update_trigger_is_supported(op)
                && matches!(op.output, Some(ValueRef::State(_)))
                && match (value, effect) {
                    (Some(value), None) => {
                        row_expression_cpu_evaluable(arena, *value)
                            && row_expression_refs_resolve(arena, op, *value)
                            && arena.contextual_locals_resolve(*value).unwrap_or(false)
                    }
                    (None, Some(effect)) => {
                        row_expression_cpu_evaluable(arena, effect.gate)
                            && row_expression_refs_resolve(arena, op, effect.gate)
                            && arena
                                .contextual_locals_resolve(effect.gate)
                                .unwrap_or(false)
                            && effect.intent_fields.iter().all(|field| {
                                row_expression_cpu_evaluable(arena, field.expression)
                                    && row_expression_refs_resolve(arena, op, field.expression)
                                    && arena
                                        .contextual_locals_resolve(field.expression)
                                        .unwrap_or(false)
                            })
                    }
                    _ => false,
                }
        }
        PlanOpKind::DerivedValue { .. } => cpu_plan_executor_supports_derived_value_op(
            arena,
            scalar_slots,
            op,
            supported_list_projection_outputs,
        ),
        PlanOpKind::ListMutation { mutation } => {
            cpu_plan_executor_supports_list_mutation_op(arena, op, mutation)
        }
        PlanOpKind::ListProjection { projection } => {
            cpu_plan_executor_supports_list_projection_op(op, projection)
        }
    }
}

fn state_update_trigger_is_supported(op: &PlanOp) -> bool {
    let PlanOpKind::StateUpdate { trigger, .. } = &op.kind else {
        return false;
    };
    let source_inputs = op
        .inputs
        .iter()
        .filter_map(|input| match input {
            ValueRef::Source(source) => Some(*source),
            _ => None,
        })
        .collect::<BTreeSet<_>>();
    match trigger {
        ValueRef::Source(source) => {
            op.inputs.contains(trigger) && source_inputs == BTreeSet::from([*source])
        }
        ValueRef::State(_) => op.inputs.contains(trigger) && source_inputs.is_empty(),
        ValueRef::Field(_)
        | ValueRef::StateProjection { .. }
        | ValueRef::SourcePayload { .. }
        | ValueRef::List(_)
        | ValueRef::Constant(_)
        | ValueRef::DistributedImport(_) => false,
    }
}

pub fn cpu_plan_executor_supports_list_mutation_op(
    arena: &PlanRowExpressionArena,
    op: &PlanOp,
    mutation: &PlanListMutation,
) -> bool {
    if op.unresolved_executable_ref_count != 0 || !op.indexed {
        return false;
    }
    let Some(ValueRef::List(_)) = op.output else {
        return false;
    };
    match mutation {
        PlanListMutation::Append(append) => {
            op.inputs.contains(&append.trigger)
                && (!append.owner.static_owner.is_root() || append.owner.ancestors.is_empty())
                && row_expression_cpu_evaluable(arena, append.gate)
                && row_expression_refs_resolve(arena, op, append.gate)
                && arena
                    .contextual_locals_resolve(append.gate)
                    .unwrap_or(false)
                && row_expression_cpu_evaluable(arena, append.item)
                && row_expression_refs_resolve(arena, op, append.item)
                && arena
                    .contextual_locals_resolve(append.item)
                    .unwrap_or(false)
                && !append.fields.is_empty()
        }
        PlanListMutation::Remove(remove) => {
            op.inputs.contains(&remove.trigger)
                && (!remove.owner.static_owner.is_root() || remove.owner.ancestors.is_empty())
                && row_expression_cpu_evaluable(arena, remove.gate)
                && row_expression_refs_resolve(arena, op, remove.gate)
                && arena
                    .contextual_locals_resolve_with(
                        remove.gate,
                        remove.local_owner,
                        remove.row_local,
                    )
                    .unwrap_or(false)
                && row_expression_cpu_evaluable(arena, remove.predicate)
                && row_expression_refs_resolve(arena, op, remove.predicate)
                && arena
                    .contextual_locals_resolve_with(
                        remove.predicate,
                        remove.local_owner,
                        remove.row_local,
                    )
                    .unwrap_or(false)
        }
    }
}

fn cpu_plan_executor_supports_list_projection_op(
    op: &PlanOp,
    projection: &PlanListProjection,
) -> bool {
    if op.unresolved_executable_ref_count != 0 || !op.indexed {
        return false;
    }
    if !list_projection_output_matches(projection, op.output.as_ref()) {
        return false;
    }
    match projection {
        PlanListProjection::Chunk {
            source_list, size, ..
        } => *size > 0 && op.inputs.contains(&ValueRef::List(*source_list)),
        PlanListProjection::ChunkValue { source, size, .. } => {
            *size > 0 && op.inputs.contains(source)
        }
        PlanListProjection::Unknown { .. } => false,
    }
}

fn list_projection_output_matches(
    projection: &PlanListProjection,
    output: Option<&ValueRef>,
) -> bool {
    match projection {
        PlanListProjection::Chunk { .. } | PlanListProjection::ChunkValue { .. } => {
            matches!(output, Some(ValueRef::List(_)))
        }
        PlanListProjection::Unknown { .. } => false,
    }
}

fn cpu_plan_executor_supports_derived_value_op(
    arena: &PlanRowExpressionArena,
    scalar_slots: &[ScalarStorageSlot],
    op: &PlanOp,
    supported_list_projection_outputs: &BTreeSet<FieldId>,
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
        return supported_list_projection_outputs.contains(&field_id);
    }
    let Some(expression) = expression else {
        return false;
    };
    if let PlanDerivedExpression::MaterializeList { expression, .. } = expression {
        return match expression.as_ref() {
            PlanDerivedExpression::RowExpression { expression }
                if matches!(
                    arena.get(*expression),
                    Some(PlanRowExpressionNode::ListAccess { .. })
                ) =>
            {
                op.unresolved_executable_ref_count == 0
            }
            PlanDerivedExpression::RowExpression { expression } => {
                row_expression_refs_resolve(arena, op, *expression)
                    && root_row_expression_cpu_evaluable(arena, *expression)
            }
            _ => false,
        };
    }
    let expression = match expression {
        expression => expression,
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
            root_row_expression_cpu_evaluable(arena, *default)
                && row_expression_refs_resolve(arena, op, *default)
                && !arms.is_empty()
                && arms.iter().all(|arm| {
                    matches!(&arm.trigger, ValueRef::Source(_) | ValueRef::State(_))
                        && op.inputs.contains(&arm.trigger)
                        && root_row_expression_cpu_evaluable(arena, arm.value)
                        && row_expression_refs_resolve(arena, op, arm.value)
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
            row_expression_refs_resolve(arena, op, *expression)
                && row_expression_cpu_evaluable(arena, *expression)
        }
        (
            true,
            PlanDerivedKind::Pure,
            PlanDerivedExpression::MaterializedRowField { expression, .. },
        ) => {
            row_expression_refs_resolve(arena, op, *expression)
                && row_expression_cpu_evaluable(arena, *expression)
        }
        (false, PlanDerivedKind::Pure, PlanDerivedExpression::RowExpression { expression }) => {
            row_expression_refs_resolve(arena, op, *expression)
                && root_row_expression_cpu_evaluable(arena, *expression)
        }
        (
            _,
            PlanDerivedKind::ListView | PlanDerivedKind::Aggregate,
            PlanDerivedExpression::RowExpression { expression },
        ) => {
            row_expression_refs_resolve(arena, op, *expression)
                && root_row_expression_cpu_evaluable(arena, *expression)
        }
        (false, PlanDerivedKind::Pure, expression) => {
            root_bool_expression_cpu_supported(arena, op, expression)
        }
        _ => false,
    }
}

fn root_bool_expression_cpu_supported(
    arena: &PlanRowExpressionArena,
    op: &PlanOp,
    expression: &PlanDerivedExpression,
) -> bool {
    match expression {
        PlanDerivedExpression::NumberCompareConst {
            left, op: op_name, ..
        } => {
            op_name.is_comparison()
                && matches!(
                left,
                ValueRef::Field(_) if op.inputs.contains(left)
                )
        }
        PlanDerivedExpression::ValueCompare {
            left,
            op: op_name,
            right,
        } => op_name.is_comparison() && op.inputs.contains(left) && op.inputs.contains(right),
        PlanDerivedExpression::BoolAnd { left, right } => {
            root_bool_expression_cpu_supported(arena, op, left)
                && root_bool_expression_cpu_supported(arena, op, right)
        }
        PlanDerivedExpression::BoolNotExpression { input } => {
            root_bool_expression_cpu_supported(arena, op, input)
        }
        _ => false,
    }
}

pub fn is_unknown_op(op: &PlanOp) -> bool {
    matches!(
        &op.kind,
        PlanOpKind::DerivedValue {
            derived_kind: PlanDerivedKind::Unknown,
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
        | PlanConstantValue::Bool { .. }
        | PlanConstantValue::Enum { .. }
        | PlanConstantValue::Data { .. } => true,
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
        let ScalarInitializerPlan::Constant { constant_id } = slot.initializer else {
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
        let PlanOpKind::StateUpdate { value, effect, .. } = &op.kind else {
            continue;
        };
        if !matches!(op.output, Some(ValueRef::State(_))) || value.is_some() == effect.is_some() {
            return Some(format!(
                "state update op {} has an invalid output/value/effect shape",
                op.id.0
            ));
        }
        let expressions = value
            .iter()
            .chain(effect.iter().map(|effect| &effect.gate))
            .chain(
                effect
                    .iter()
                    .flat_map(|effect| effect.intent_fields.iter().map(|field| &field.expression)),
            );
        for expression in expressions {
            let mut missing_inputs = BTreeSet::new();
            if plan
                .row_expressions
                .visit_inputs(*expression, &mut |input| {
                    if !op.inputs.contains(&input) {
                        missing_inputs.insert(input);
                    }
                })
                .is_err()
            {
                return Some(format!(
                    "state update op {} references an invalid row expression {}",
                    op.id.0, expression.0
                ));
            }
            let refs_resolve = row_expression_refs_resolve(&plan.row_expressions, op, *expression);
            let contextual_locals_resolve = plan
                .row_expressions
                .contextual_locals_resolve(*expression)
                .unwrap_or(false);
            if !refs_resolve || !contextual_locals_resolve {
                return Some(format!(
                    "state update op {} has unresolved executable references: refs_resolve={refs_resolve}, contextual_locals_resolve={contextual_locals_resolve}, missing_inputs={missing_inputs:?}, expression={expression:?}",
                    op.id.0,
                ));
            }
        }
    }
    None
}

fn initial_expressions_failure(plan: &MachinePlan) -> Option<String> {
    for slot in &plan.storage_layout.scalar_slots {
        let ScalarInitializerPlan::Expression { expression } = &slot.initializer else {
            continue;
        };
        if !plan
            .row_expressions
            .contextual_locals_resolve(*expression)
            .unwrap_or(false)
        {
            return Some(format!(
                "state {} has an initializer with unbound contextual locals: {expression:?}",
                slot.state_id.0
            ));
        }
    }
    for slot in &plan.storage_layout.list_slots {
        for (row_index, row) in slot.initial_rows.iter().enumerate() {
            for field in &row.fields {
                let Some(expression) = field.initializer.expression() else {
                    continue;
                };
                if !plan
                    .row_expressions
                    .contextual_locals_resolve(expression)
                    .unwrap_or(false)
                {
                    return Some(format!(
                        "list {} initial row {row_index} field `{}` has unbound contextual locals: {expression:?}",
                        slot.list_id.0, field.name
                    ));
                }
                let mut invalid = BTreeSet::new();
                if plan
                    .row_expressions
                    .visit_inputs(expression, &mut |input| {
                        if !initial_expression_input_resolves(plan, &input) {
                            invalid.insert(input);
                        }
                    })
                    .is_err()
                {
                    return Some(format!(
                        "list {} initial row {row_index} field `{}` references invalid row expression {}",
                        slot.list_id.0, field.name, expression.0
                    ));
                }
                if !invalid.is_empty() {
                    return Some(format!(
                        "list {} initial row {row_index} field `{}` has invalid inputs {invalid:?}",
                        slot.list_id.0, field.name
                    ));
                }
            }
        }
    }
    None
}

fn initial_expression_input_resolves(plan: &MachinePlan, input: &ValueRef) -> bool {
    match input {
        ValueRef::State(state)
        | ValueRef::StateProjection {
            state_id: state, ..
        } => plan
            .storage_layout
            .scalar_slots
            .iter()
            .any(|slot| slot.state_id == *state),
        ValueRef::Field(field) => {
            plan.storage_layout
                .list_slots
                .iter()
                .any(|slot| slot.contains_row_field(*field))
                || plan
                    .regions
                    .iter()
                    .flat_map(|region| &region.ops)
                    .any(|op| op.output == Some(ValueRef::Field(*field)))
        }
        ValueRef::List(list) => plan
            .storage_layout
            .list_slots
            .iter()
            .any(|slot| slot.list_id == *list),
        ValueRef::Constant(constant) => plan.constants.iter().any(|item| item.id == *constant),
        ValueRef::DistributedImport(import) => {
            distributed_import_data_type(plan, *import).is_some()
        }
        ValueRef::Source(_) | ValueRef::SourcePayload { .. } => false,
    }
}

fn list_mutation_expressions_resolve(plan: &MachinePlan) -> bool {
    let mutations = plan
        .regions
        .iter()
        .flat_map(|region| &region.ops)
        .filter(|op| matches!(op.kind, PlanOpKind::ListMutation { .. }))
        .collect::<Vec<_>>();
    if mutations.iter().enumerate().any(|(ordinal, op)| {
        let PlanOpKind::ListMutation { mutation } = &op.kind else {
            return true;
        };
        let actual = match mutation {
            PlanListMutation::Append(append) => append.ordinal,
            PlanListMutation::Remove(remove) => remove.ordinal,
        };
        usize::try_from(actual).ok() != Some(ordinal)
    }) {
        return false;
    }
    mutations.into_iter().all(|op| {
        let PlanOpKind::ListMutation { mutation } = &op.kind else {
            return true;
        };
        if !matches!(op.output, Some(ValueRef::List(_))) {
            return false;
        }
        match mutation {
            PlanListMutation::Append(append) => {
                let fields = append
                    .fields
                    .iter()
                    .map(|field| (field.name.as_str(), field.field_id))
                    .collect::<BTreeSet<_>>();
                plan_owner_resolves(plan, &append.owner)
                    && op.inputs.contains(&append.trigger)
                    && row_expression_refs_resolve(&plan.row_expressions, op, append.gate)
                    && plan
                        .row_expressions
                        .contextual_locals_resolve(append.gate)
                        .unwrap_or(false)
                    && row_expression_refs_resolve(&plan.row_expressions, op, append.item)
                    && plan
                        .row_expressions
                        .contextual_locals_resolve(append.item)
                        .unwrap_or(false)
                    && !append.fields.is_empty()
                    && append.fields.iter().all(|field| !field.name.is_empty())
                    && fields.len() == append.fields.len()
                    && append.row_field_copies.iter().all(|copy| {
                        append
                            .fields
                            .iter()
                            .any(|field| field.field_id == copy.target_field)
                    })
            }
            PlanListMutation::Remove(remove) => {
                plan_owner_resolves(plan, &remove.owner)
                    && op.inputs.contains(&remove.trigger)
                    && row_expression_refs_resolve(&plan.row_expressions, op, remove.gate)
                    && plan
                        .row_expressions
                        .contextual_locals_resolve_with(
                            remove.gate,
                            remove.local_owner,
                            remove.row_local,
                        )
                        .unwrap_or(false)
                    && row_expression_refs_resolve(&plan.row_expressions, op, remove.predicate)
                    && plan
                        .row_expressions
                        .contextual_locals_resolve_with(
                            remove.predicate,
                            remove.local_owner,
                            remove.row_local,
                        )
                        .unwrap_or(false)
            }
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
            if !list_projection_output_matches(projection, op.output.as_ref()) {
                return false;
            }
            match projection {
                PlanListProjection::Chunk { source_list, size } => {
                    *size > 0 && op.inputs.contains(&ValueRef::List(*source_list))
                }
                PlanListProjection::ChunkValue { source, size } => {
                    *size > 0 && op.inputs.contains(source)
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

fn exact_row_field_ownership_failure(plan: &MachinePlan) -> Option<String> {
    let mut global_fields = BTreeMap::<FieldId, ListId>::new();
    for slot in &plan.storage_layout.list_slots {
        let mut identities = BTreeSet::new();
        let mut authority = BTreeSet::new();
        for field in &slot.row_fields {
            if field.name.is_empty()
                || !identities.insert((field.field_id, field.name.clone(), field.role))
            {
                return Some(format!(
                    "list {} has an empty or duplicate structured row field",
                    slot.list_id.0
                ));
            }
            if let Some(previous) = global_fields.insert(field.field_id, slot.list_id)
                && previous != slot.list_id
            {
                return Some(format!(
                    "field {} belongs to lists {} and {}",
                    field.field_id.0, previous.0, slot.list_id.0
                ));
            }
            if field.role.is_authority() {
                authority.insert(field.field_id);
            }
        }
        let unique_ids = slot.row_field_ids().collect::<BTreeSet<_>>();
        if unique_ids.len() != slot.row_fields.len() {
            return Some(format!(
                "list {} assigns one FieldId to multiple row fields",
                slot.list_id.0
            ));
        }
        for field in slot.initial_rows.iter().flat_map(|row| &row.fields) {
            let Some(field_id) = field.field_id else {
                return Some(format!(
                    "list {} initial field `{}` has no FieldId",
                    slot.list_id.0, field.name
                ));
            };
            if !authority.contains(&field_id) {
                return Some(format!(
                    "list {} initial field `{}` targets non-authority field {}",
                    slot.list_id.0, field.name, field_id.0
                ));
            }
        }
        if slot.initializer_kind == ListInitializerKind::Range {
            for name in ["index", "value"] {
                if !slot
                    .row_fields
                    .iter()
                    .any(|field| field.role.is_authority() && field.name == name)
                {
                    return Some(format!(
                        "range list {} has no authority field `{name}`",
                        slot.list_id.0
                    ));
                }
            }
        }
    }

    for slot in &plan.storage_layout.scalar_slots {
        match (slot.indexed, slot.indexed_field_id) {
            (false, None) => {}
            (false, Some(field)) => {
                return Some(format!(
                    "unscoped state {} carries indexed field {}",
                    slot.state_id.0, field.0
                ));
            }
            (true, None) => {
                return Some(format!(
                    "indexed state {} has no exact row field",
                    slot.state_id.0
                ));
            }
            (true, Some(field)) => {
                let Some(scope) = slot.scope_id else {
                    return Some(format!("indexed state {} has no scope", slot.state_id.0));
                };
                if !plan
                    .storage_layout
                    .list_slots
                    .iter()
                    .any(|list| list.scope_id == Some(scope) && list.contains_row_field(field))
                {
                    return Some(format!(
                        "indexed state {} field {} has no exact owner list",
                        slot.state_id.0, field.0
                    ));
                }
            }
        }
    }

    for op in plan.regions.iter().flat_map(|region| &region.ops) {
        let PlanOpKind::ListMutation {
            mutation: PlanListMutation::Append(append),
        } = &op.kind
        else {
            continue;
        };
        let Some(ValueRef::List(list)) = op.output else {
            return Some(format!("append op {} has no list output", op.id.0));
        };
        let Some(slot) = plan
            .storage_layout
            .list_slots
            .iter()
            .find(|slot| slot.list_id == list)
        else {
            return Some(format!(
                "append op {} targets missing list {}",
                op.id.0, list.0
            ));
        };
        for field in &append.fields {
            let field_id = field.field_id;
            if !slot
                .row_fields
                .iter()
                .any(|candidate| candidate.field_id == field_id && candidate.role.is_authority())
            {
                return Some(format!(
                    "append op {} field `{}` targets non-authority field {}",
                    op.id.0, field.name, field_id.0
                ));
            }
        }
        for copy in &append.row_field_copies {
            let source_resolves = plan.storage_layout.list_slots.iter().any(|source| {
                source.list_id == copy.source_list
                    && source
                        .row_fields
                        .iter()
                        .any(|field| field.field_id == copy.source_field && field.role.is_value())
            });
            if !source_resolves
                || !append
                    .fields
                    .iter()
                    .any(|field| field.field_id == copy.target_field)
            {
                return Some(format!(
                    "append op {} has invalid exact row-field copy {:?}",
                    op.id.0, copy
                ));
            }
        }
    }
    None
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
                PlanDerivedExpression::MaterializeList {
                    target_list,
                    fields,
                    row_field_copies,
                    expression,
                    ..
                } => {
                    plan.storage_layout
                        .list_slots
                        .iter()
                        .any(|slot| slot.list_id == *target_list)
                        && fields
                            .values()
                            .all(|field| list_has_row_field(plan, *target_list, *field))
                        && row_field_copies.iter().all(|copy| {
                            list_has_row_field(plan, copy.source_list, copy.source_field)
                                && list_has_row_field(plan, *target_list, copy.target_field)
                        })
                        && derived_expression_refs_resolve_for_op(
                            &plan.row_expressions,
                            op,
                            expression,
                        )
                }
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
                    row_expression_refs_resolve(&plan.row_expressions, op, *default)
                        && arms.iter().all(|arm| {
                            matches!(&arm.trigger, ValueRef::Source(_) | ValueRef::State(_))
                                && op.inputs.contains(&arm.trigger)
                                && row_expression_refs_resolve(&plan.row_expressions, op, arm.value)
                        })
                }
                PlanDerivedExpression::BoolNot { input } => op.inputs.contains(input),
                PlanDerivedExpression::NumberCompareConst { left, .. } => op.inputs.contains(left),
                PlanDerivedExpression::ValueCompare { left, right, .. } => {
                    op.inputs.contains(left) && op.inputs.contains(right)
                }
                PlanDerivedExpression::BoolAnd { left, right } => {
                    derived_expression_refs_resolve_for_op(&plan.row_expressions, op, left)
                        && derived_expression_refs_resolve_for_op(&plan.row_expressions, op, right)
                }
                PlanDerivedExpression::BoolNotExpression { input } => {
                    derived_expression_refs_resolve_for_op(&plan.row_expressions, op, input)
                }
                PlanDerivedExpression::RowExpression { expression } => {
                    row_expression_refs_resolve(&plan.row_expressions, op, *expression)
                }
                PlanDerivedExpression::MaterializedRowField { expression, .. } => {
                    row_expression_refs_resolve(&plan.row_expressions, op, *expression)
                }
            }
        })
}

fn derived_expression_refs_resolve_for_op(
    arena: &PlanRowExpressionArena,
    op: &PlanOp,
    expression: &PlanDerivedExpression,
) -> bool {
    match expression {
        PlanDerivedExpression::MaterializeList { expression, .. } => {
            derived_expression_refs_resolve_for_op(arena, op, expression)
        }
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
            row_expression_refs_resolve(arena, op, *default)
                && arms.iter().all(|arm| {
                    matches!(&arm.trigger, ValueRef::Source(_) | ValueRef::State(_))
                        && op.inputs.contains(&arm.trigger)
                        && row_expression_refs_resolve(arena, op, arm.value)
                })
        }
        PlanDerivedExpression::BoolNot { input } => op.inputs.contains(input),
        PlanDerivedExpression::NumberCompareConst { left, .. } => op.inputs.contains(left),
        PlanDerivedExpression::ValueCompare { left, right, .. } => {
            op.inputs.contains(left) && op.inputs.contains(right)
        }
        PlanDerivedExpression::BoolAnd { left, right } => {
            derived_expression_refs_resolve_for_op(arena, op, left)
                && derived_expression_refs_resolve_for_op(arena, op, right)
        }
        PlanDerivedExpression::BoolNotExpression { input } => {
            derived_expression_refs_resolve_for_op(arena, op, input)
        }
        PlanDerivedExpression::RowExpression { expression } => {
            row_expression_refs_resolve(arena, op, *expression)
        }
        PlanDerivedExpression::MaterializedRowField { expression, .. } => {
            row_expression_refs_resolve(arena, op, *expression)
        }
    }
}

fn row_expression_refs_resolve(
    arena: &PlanRowExpressionArena,
    op: &PlanOp,
    root: PlanRowExpressionId,
) -> bool {
    let Ok(order) = arena.walk_postorder(root) else {
        return false;
    };
    let mut valid = BTreeMap::new();
    for id in order {
        let Ok(node) = arena.node(id) else {
            return false;
        };
        let mut children_resolve = true;
        node.visit_children(&mut |child| {
            children_resolve &= valid.get(&child).copied().unwrap_or(false);
        });
        let node_resolves = children_resolve
            && match node {
                PlanRowExpressionNode::Field { input } => op.inputs.contains(input),
                PlanRowExpressionNode::Constant { constant_id } => {
                    op.inputs.contains(&ValueRef::Constant(*constant_id))
                }
                PlanRowExpressionNode::ListGetField { list_id, .. }
                | PlanRowExpressionNode::ListRef { list_id }
                | PlanRowExpressionNode::AuthorityListRef { list_id }
                | PlanRowExpressionNode::ListRowField { list_id, .. } => {
                    op.inputs.contains(&ValueRef::List(*list_id))
                }
                PlanRowExpressionNode::EventRow { source, .. } => {
                    op.inputs.contains(&ValueRef::Source(*source))
                }
                _ => true,
            };
        valid.insert(id, node_resolves);
    }
    valid.get(&root).copied().unwrap_or(false)
}

fn row_expression_contextual_locals_resolve(plan: &MachinePlan) -> bool {
    let initial_values_resolve = plan
        .storage_layout
        .scalar_slots
        .iter()
        .filter_map(|slot| match &slot.initializer {
            ScalarInitializerPlan::Expression { expression } => Some(expression),
            ScalarInitializerPlan::Constant { .. } => None,
        })
        .all(|expression| {
            plan.row_expressions
                .contextual_locals_resolve(*expression)
                .unwrap_or(false)
        });
    let derived_values_resolve = plan
        .regions
        .iter()
        .flat_map(|region| &region.ops)
        .filter_map(|op| match &op.kind {
            PlanOpKind::DerivedValue {
                expression: Some(expression),
                ..
            } => Some(expression),
            _ => None,
        })
        .all(|expression| {
            derived_expression_contextual_locals_resolve(&plan.row_expressions, expression)
        });
    let mutation_values_resolve = plan
        .regions
        .iter()
        .flat_map(|region| &region.ops)
        .filter_map(|op| match &op.kind {
            PlanOpKind::ListMutation { mutation } => Some(mutation),
            _ => None,
        })
        .all(|mutation| match mutation {
            PlanListMutation::Append(append) => {
                plan.row_expressions
                    .contextual_locals_resolve(append.gate)
                    .unwrap_or(false)
                    && plan
                        .row_expressions
                        .contextual_locals_resolve(append.item)
                        .unwrap_or(false)
            }
            PlanListMutation::Remove(remove) => {
                plan.row_expressions
                    .contextual_locals_resolve_with(
                        remove.gate,
                        remove.local_owner,
                        remove.row_local,
                    )
                    .unwrap_or(false)
                    && plan
                        .row_expressions
                        .contextual_locals_resolve_with(
                            remove.predicate,
                            remove.local_owner,
                            remove.row_local,
                        )
                        .unwrap_or(false)
            }
        });
    let distributed_values_resolve = plan.distributed_endpoint.as_ref().is_none_or(|endpoint| {
        endpoint.endpoint.remote_call_sites.iter().all(|call| {
            let Some(bindings) =
                distributed_call_contextual_bindings(call, Some(&plan.row_expressions))
            else {
                return false;
            };
            call.arguments.iter().all(|argument| {
                plan.row_expressions
                    .contextual_locals_resolve_with_bindings(
                        argument.value,
                        bindings.iter().map(|(owner, local)| (*owner, *local)),
                    )
                    .unwrap_or(false)
            })
        })
    });
    initial_values_resolve
        && derived_values_resolve
        && mutation_values_resolve
        && distributed_values_resolve
}

fn derived_expression_contextual_locals_resolve(
    arena: &PlanRowExpressionArena,
    expression: &PlanDerivedExpression,
) -> bool {
    match expression {
        PlanDerivedExpression::MaterializeList { expression, .. } => {
            derived_expression_contextual_locals_resolve(arena, expression)
        }
        PlanDerivedExpression::SourceEventTransform { default, arms, .. } => {
            arena.contextual_locals_resolve(*default).unwrap_or(false)
                && arms
                    .iter()
                    .all(|arm| arena.contextual_locals_resolve(arm.value).unwrap_or(false))
        }
        PlanDerivedExpression::BoolAnd { left, right } => {
            derived_expression_contextual_locals_resolve(arena, left)
                && derived_expression_contextual_locals_resolve(arena, right)
        }
        PlanDerivedExpression::BoolNotExpression { input } => {
            derived_expression_contextual_locals_resolve(arena, input)
        }
        PlanDerivedExpression::RowExpression { expression } => arena
            .contextual_locals_resolve(*expression)
            .unwrap_or(false),
        PlanDerivedExpression::MaterializedRowField { local, expression } => match local {
            Some(local) => arena
                .contextual_locals_resolve_with(*expression, local.owner, local.row_local)
                .unwrap_or(false),
            None => arena
                .contextual_locals_resolve(*expression)
                .unwrap_or(false),
        },
        PlanDerivedExpression::SourceKeyTextTrimNonEmpty { .. }
        | PlanDerivedExpression::BoolNot { .. }
        | PlanDerivedExpression::NumberCompareConst { .. }
        | PlanDerivedExpression::ValueCompare { .. } => true,
    }
}

pub(crate) struct RuntimeRowExpressionValidation {
    pub(crate) locals_resolve: bool,
    pub(crate) list_fields_resolve: bool,
    pub(crate) cpu_evaluable: bool,
    pub(crate) detail: Option<String>,
}

pub(crate) fn validate_runtime_row_expression(
    plan: &MachinePlan,
    root: PlanRowExpressionId,
    bindings: impl IntoIterator<Item = (PlanStaticOwnerId, PlanLocalId)>,
) -> RuntimeRowExpressionValidation {
    let order = match plan.row_expressions.walk_postorder(root) {
        Ok(order) => order,
        Err(error) => {
            return RuntimeRowExpressionValidation {
                locals_resolve: false,
                list_fields_resolve: false,
                cpu_evaluable: false,
                detail: Some(error.to_string()),
            };
        }
    };
    let locals = plan
        .row_expressions
        .contextual_locals_resolve_with_bindings(root, bindings);
    let locals_resolve = locals.as_ref().copied().unwrap_or(false);
    let list_fields_resolve = row_expression_list_fields_resolve_with_order(plan, root, &order);
    let (cpu_evaluable, cpu_detail) =
        row_expression_cpu_status_with_order(&plan.row_expressions, root, &order);
    let detail = locals
        .err()
        .map(|error| error.to_string())
        .or_else(|| {
            (!locals_resolve)
                .then(|| format!("row expression {} has unresolved contextual locals", root.0))
        })
        .or_else(|| {
            (!list_fields_resolve).then(|| {
                format!(
                    "row expression {} has unresolved list-field ownership",
                    root.0
                )
            })
        })
        .or(cpu_detail);
    RuntimeRowExpressionValidation {
        locals_resolve,
        list_fields_resolve,
        cpu_evaluable,
        detail,
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
            }
            | PlanOpKind::DerivedValue {
                expression: Some(PlanDerivedExpression::MaterializedRowField { expression, .. }),
                ..
            } => row_expression_list_fields_resolve_inner(plan, *expression),
            PlanOpKind::StateUpdate { value, effect, .. } => {
                value
                    .iter()
                    .chain(effect.iter().map(|effect| &effect.gate))
                    .chain(effect.iter().flat_map(|effect| {
                        effect.intent_fields.iter().map(|field| &field.expression)
                    }))
                    .all(|expression| row_expression_list_fields_resolve_inner(plan, *expression))
            }
            _ => true,
        })
}

fn row_expression_list_fields_resolve_inner(plan: &MachinePlan, root: PlanRowExpressionId) -> bool {
    let Ok(order) = plan.row_expressions.walk_postorder(root) else {
        return false;
    };
    row_expression_list_fields_resolve_with_order(plan, root, &order)
}

fn row_expression_list_fields_resolve_with_order(
    plan: &MachinePlan,
    root: PlanRowExpressionId,
    order: &[PlanRowExpressionId],
) -> bool {
    let mut valid = BTreeMap::new();
    for &id in order {
        let Ok(node) = plan.row_expressions.node(id) else {
            return false;
        };
        let mut children_resolve = true;
        visit_runtime_row_expression_children(node, &mut |child| {
            children_resolve &= valid.get(&child).copied().unwrap_or(false);
        });
        let node_resolves = children_resolve
            && match node {
                PlanRowExpressionNode::EventRow { source, list_id } => plan
                    .source_routes
                    .iter()
                    .find(|route| route.source_id == *source)
                    .is_some_and(|route| {
                        route.scoped
                            && route.scope_id.is_some()
                            && route.owner.ancestors.last().is_some_and(|ancestor| {
                                ancestor.list == *list_id && Some(ancestor.scope) == route.scope_id
                            })
                            && plan.storage_layout.list_slots.iter().any(|slot| {
                                slot.list_id == *list_id && slot.scope_id == route.scope_id
                            })
                    }),
                PlanRowExpressionNode::ListGetField { list_id, field, .. }
                | PlanRowExpressionNode::ListRowField { list_id, field, .. } => {
                    list_has_row_field(plan, *list_id, *field)
                }
                PlanRowExpressionNode::ContextualCollection {
                    owner,
                    operation,
                    source,
                    row_local,
                    body,
                    captures,
                    indexed_access,
                } => {
                    captures.iter().all(|capture| {
                        plan.storage_layout.list_slots.iter().any(|slot| {
                            slot.row_fields.iter().any(|field| {
                                field.field_id == capture.field
                                    && field.role == PlanListRowFieldRole::Capture
                            })
                        })
                    }) && indexed_access.as_ref().is_none_or(|indexed_access| {
                        contextual_indexed_access_matches(
                            plan,
                            *owner,
                            *operation,
                            *source,
                            *row_local,
                            *body,
                            indexed_access,
                            &valid,
                        )
                    })
                }
                PlanRowExpressionNode::ListAccess { access } => {
                    list_access_metadata_fields_resolve(plan, access)
                }
                PlanRowExpressionNode::ListPage { page } => {
                    list_access_metadata_fields_resolve(plan, &page.access)
                }
                _ => true,
            };
        valid.insert(id, node_resolves);
    }
    valid.get(&root).copied().unwrap_or(false)
}

fn list_access_metadata_fields_resolve(plan: &MachinePlan, access: &PlanListAccess) -> bool {
    plan.list_indexes.get(access.index.0).is_some()
        && access.maps.iter().all(|map| {
            map.captures.iter().all(|capture| {
                plan.storage_layout.list_slots.iter().any(|slot| {
                    slot.row_fields.iter().any(|field| {
                        field.field_id == capture.field
                            && field.role == PlanListRowFieldRole::Capture
                    })
                })
            })
        })
}

fn list_has_row_field(plan: &MachinePlan, list_id: ListId, field_id: FieldId) -> bool {
    plan.storage_layout
        .list_slots
        .iter()
        .find(|slot| slot.list_id == list_id)
        .is_some_and(|slot| slot.contains_row_field(field_id))
}

fn contextual_indexed_access_matches(
    plan: &MachinePlan,
    owner: PlanStaticOwnerId,
    operation: PlanContextualOperationKind,
    source: PlanRowExpressionId,
    row_local: PlanLocalId,
    body: PlanRowExpressionId,
    access: &PlanContextualIndexedAccess,
    list_fields_resolve: &BTreeMap<PlanRowExpressionId, bool>,
) -> bool {
    let Some(index) = plan
        .list_indexes
        .get(access.index.0)
        .filter(|index| index.id == access.index)
    else {
        return false;
    };
    let [key] = index.keys.as_slice() else {
        return false;
    };
    let PlanListAccessSelection::KeyPrefix { values } = &access.selection else {
        return false;
    };
    let [value] = values.as_slice() else {
        return false;
    };
    let Ok(source_node) = plan.row_expressions.node(source) else {
        return false;
    };
    if !matches!(
        operation,
        PlanContextualOperationKind::Filter
            | PlanContextualOperationKind::Retain
            | PlanContextualOperationKind::Any
            | PlanContextualOperationKind::Find
    ) || !matches!(
        source_node,
        PlanRowExpressionNode::ListRef { list_id } if *list_id == index.source_list
    ) || key.direction != PlanOrderDirection::Ascending
        || key.multiplicity != PlanListIndexKeyMultiplicity::One
        || !list_fields_resolve.get(value).copied().unwrap_or(false)
    {
        return false;
    }
    let Ok(PlanRowExpressionNode::NumberInfix { op, left, right }) =
        plan.row_expressions.node(body)
    else {
        return false;
    };
    if *op != PlanInfixOp::Equal {
        return false;
    }
    (|| {
        let mut normalized = PlanRowExpressionArena::new();
        let key = plan.row_expressions.clone_normalized_into(
            key.expression,
            Some(((key.owner, key.row_local), (owner, row_local))),
            &mut normalized,
        )?;
        let left = plan
            .row_expressions
            .clone_normalized_into(*left, None, &mut normalized)?;
        let right = plan
            .row_expressions
            .clone_normalized_into(*right, None, &mut normalized)?;
        let value = plan
            .row_expressions
            .clone_normalized_into(*value, None, &mut normalized)?;
        Ok::<_, PlanError>((key == left && right == value) || (key == right && left == value))
    })()
    .unwrap_or(false)
}

fn row_expression_cpu_evaluable(arena: &PlanRowExpressionArena, root: PlanRowExpressionId) -> bool {
    let Ok(order) = arena.walk_postorder(root) else {
        return false;
    };
    row_expression_cpu_status_with_order(arena, root, &order).0
}

fn row_expression_cpu_status_with_order(
    arena: &PlanRowExpressionArena,
    root: PlanRowExpressionId,
    order: &[PlanRowExpressionId],
) -> (bool, Option<String>) {
    let mut statuses = BTreeMap::<PlanRowExpressionId, (bool, Option<String>)>::new();
    for &id in order {
        let Ok(node) = arena.node(id) else {
            return (
                false,
                Some(format!("row expression id {} is invalid", id.0)),
            );
        };
        let mut children_evaluable = true;
        let mut child_failure = None;
        visit_cpu_row_expression_children(node, &mut |child| {
            let (evaluable, detail) = statuses.get(&child).cloned().unwrap_or_else(|| {
                (
                    false,
                    Some(format!("row expression {} has missing CPU status", child.0)),
                )
            });
            children_evaluable &= evaluable;
            if child_failure.is_none() && !evaluable {
                child_failure = detail
                    .or_else(|| Some(format!("row expression {} is not CPU-evaluable", child.0)));
            }
        });
        let call_failure = match node {
            PlanRowExpressionNode::BuiltinCall {
                function,
                input,
                args,
            } => function
                .validate_call(*input, args)
                .err()
                .map(|error| format!("row expression {}: {error}", id.0)),
            _ => None,
        };
        let detail = call_failure.or(child_failure);
        statuses.insert(id, (children_evaluable && detail.is_none(), detail));
    }
    statuses.get(&root).cloned().unwrap_or_else(|| {
        (
            false,
            Some(format!(
                "row expression {} did not produce a CPU validation status",
                root.0
            )),
        )
    })
}

fn root_row_expression_cpu_evaluable(
    arena: &PlanRowExpressionArena,
    root: PlanRowExpressionId,
) -> bool {
    arena.contextual_locals_resolve(root).unwrap_or(false)
        && row_expression_cpu_evaluable(arena, root)
}

pub fn plan_constant_by_id(
    constants: &[PlanConstant],
    id: PlanConstantId,
) -> Option<&PlanConstant> {
    constants.iter().find(|constant| constant.id == id)
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

fn constant_value_matches_plan_type(value: &PlanConstantValue, value_type: &PlanValueType) -> bool {
    match (value, value_type) {
        (PlanConstantValue::Text { .. }, PlanValueType::Text) => true,
        (PlanConstantValue::Number { .. }, PlanValueType::Number) => true,
        (PlanConstantValue::Bool { .. }, PlanValueType::Bool) => true,
        (PlanConstantValue::Enum { .. }, PlanValueType::Enum) => true,
        (PlanConstantValue::Data { .. }, PlanValueType::Data) => true,
        (
            PlanConstantValue::Bytes { byte_len, .. },
            PlanValueType::Bytes {
                fixed_len: Some(fixed_len),
            },
        ) => byte_len == fixed_len,
        (PlanConstantValue::Bytes { .. }, PlanValueType::Bytes { fixed_len: None }) => true,
        (_, PlanValueType::Unknown) => true,
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
mod contextual_collection_tests {
    use super::*;

    fn push(
        arena: &mut PlanRowExpressionArena,
        node: PlanRowExpressionNode,
    ) -> PlanRowExpressionId {
        arena.push(node).unwrap()
    }

    fn push_local(
        arena: &mut PlanRowExpressionArena,
        owner: usize,
        local: usize,
        projection: &[&str],
    ) -> PlanRowExpressionId {
        push(
            arena,
            PlanRowExpressionNode::Local {
                owner: PlanStaticOwnerId(owner),
                local: PlanLocalId(local),
                projection: projection.iter().map(|field| (*field).to_owned()).collect(),
            },
        )
    }

    fn contextual_expression(
        owner: usize,
        operation: PlanContextualOperationKind,
        row_local: usize,
        projection: &[&str],
    ) -> (PlanRowExpressionArena, PlanRowExpressionId) {
        let mut arena = PlanRowExpressionArena::new();
        let source = push(
            &mut arena,
            PlanRowExpressionNode::Field {
                input: ValueRef::List(ListId(2)),
            },
        );
        let value = push_local(&mut arena, owner, row_local, projection);
        let status = push(
            &mut arena,
            PlanRowExpressionNode::Intrinsic {
                intrinsic: PlanIntrinsic::SessionInfoStatus,
            },
        );
        let body = push(
            &mut arena,
            PlanRowExpressionNode::Object {
                fields: vec![
                    PlanRowObjectField {
                        name: "value".to_owned(),
                        value,
                        spread: false,
                    },
                    PlanRowObjectField {
                        name: "status".to_owned(),
                        value: status,
                        spread: false,
                    },
                ],
            },
        );
        let root = push(
            &mut arena,
            PlanRowExpressionNode::ContextualCollection {
                owner: PlanStaticOwnerId(owner),
                operation,
                source,
                row_local: PlanLocalId(row_local),
                body,
                captures: Vec::new(),
                indexed_access: None,
            },
        );
        (arena, root)
    }

    fn contextual_hash(
        owner: usize,
        operation: PlanContextualOperationKind,
        row_local: usize,
        projection: &[&str],
    ) -> [u8; 32] {
        let (arena, root) = contextual_expression(owner, operation, row_local, projection);
        canonical_sha256(&(arena, root)).unwrap()
    }

    fn bounded_fingerprint(arena: &PlanRowExpressionArena, root: PlanRowExpressionId) -> [u8; 32] {
        TypedListViewFingerprintContext {
            row_expressions: arena,
            constants: BTreeMap::new(),
            sources: BTreeMap::new(),
            states: BTreeMap::new(),
            fields: BTreeMap::new(),
            lists: BTreeMap::new(),
        }
        .bounded_fingerprint(root)
        .unwrap()
    }

    #[test]
    fn typed_contextual_locals_validate_visit_and_hash_structurally() {
        let (arena, root) =
            contextual_expression(7, PlanContextualOperationKind::Map, 3, &["name"]);
        assert!(arena.contextual_locals_resolve(root).unwrap());

        let mut refs = Vec::new();
        arena
            .visit_value_refs(root, &mut |value| refs.push(value.clone()))
            .unwrap();
        assert_eq!(refs, vec![ValueRef::List(ListId(2))]);
        let mut intrinsics = Vec::new();
        arena
            .visit_intrinsics(root, &mut |intrinsic| intrinsics.push(intrinsic))
            .unwrap();
        assert_eq!(intrinsics, vec![PlanIntrinsic::SessionInfoStatus]);

        let hash = contextual_hash(7, PlanContextualOperationKind::Map, 3, &["name"]);
        assert_eq!(
            hash,
            contextual_hash(7, PlanContextualOperationKind::Map, 3, &["name"])
        );
        for changed in [
            contextual_hash(8, PlanContextualOperationKind::Map, 3, &["name"]),
            contextual_hash(7, PlanContextualOperationKind::Filter, 3, &["name"]),
            contextual_hash(7, PlanContextualOperationKind::Map, 4, &["name"]),
            contextual_hash(7, PlanContextualOperationKind::Map, 3, &["title"]),
        ] {
            assert_ne!(hash, changed);
        }
    }

    #[test]
    fn contextual_local_validation_rejects_unbound_or_empty_projection_fields() {
        let mut arena = PlanRowExpressionArena::new();
        let unbound = push_local(&mut arena, 0, 0, &[]);
        assert!(!arena.contextual_locals_resolve(unbound).unwrap());

        let (arena, wrong_owner) =
            contextual_expression(0, PlanContextualOperationKind::Map, 0, &[]);
        let mut nodes = arena.into_nodes();
        let PlanRowExpressionNode::Object { fields } = &nodes[wrong_owner.0 - 1] else {
            panic!("expected contextual body object");
        };
        let local = fields[0].value;
        let PlanRowExpressionNode::Local { owner, .. } = &mut nodes[local.0] else {
            panic!("expected contextual local");
        };
        *owner = PlanStaticOwnerId(1);
        let arena = PlanRowExpressionArena::from_nodes(nodes).unwrap();
        assert!(!arena.contextual_locals_resolve(wrong_owner).unwrap());

        let (arena, empty_projection) =
            contextual_expression(0, PlanContextualOperationKind::Map, 0, &[""]);
        assert!(!arena.contextual_locals_resolve(empty_projection).unwrap());
    }

    #[test]
    fn deep_row_expression_dag_walks_without_recursion() {
        const DEPTH: usize = 50_000;

        let mut arena = PlanRowExpressionArena::new();
        let mut root = push(
            &mut arena,
            PlanRowExpressionNode::Intrinsic {
                intrinsic: PlanIntrinsic::SessionInfoStatus,
            },
        );
        for _ in 0..DEPTH {
            root = push(&mut arena, PlanRowExpressionNode::TextTrim { input: root });
        }

        assert_eq!(arena.walk_postorder(root).unwrap().len(), DEPTH + 1);
        assert!(arena.contextual_locals_resolve(root).unwrap());
        assert!(row_expression_cpu_evaluable(&arena, root));
        let _ = bounded_fingerprint(&arena, root);
    }

    #[test]
    fn canonical_fingerprint_ignores_source_arena_allocation_order() {
        let mut left_arena = PlanRowExpressionArena::new();
        let left_status = push(
            &mut left_arena,
            PlanRowExpressionNode::Intrinsic {
                intrinsic: PlanIntrinsic::SessionInfoStatus,
            },
        );
        let left_principal = push(
            &mut left_arena,
            PlanRowExpressionNode::Intrinsic {
                intrinsic: PlanIntrinsic::SessionInfoPrincipal,
            },
        );
        let left_root = push(
            &mut left_arena,
            PlanRowExpressionNode::TextConcat {
                parts: vec![left_status, left_principal],
            },
        );

        let mut right_arena = PlanRowExpressionArena::new();
        let right_principal = push(
            &mut right_arena,
            PlanRowExpressionNode::Intrinsic {
                intrinsic: PlanIntrinsic::SessionInfoPrincipal,
            },
        );
        let right_status = push(
            &mut right_arena,
            PlanRowExpressionNode::Intrinsic {
                intrinsic: PlanIntrinsic::SessionInfoStatus,
            },
        );
        let right_root = push(
            &mut right_arena,
            PlanRowExpressionNode::TextConcat {
                parts: vec![right_status, right_principal],
            },
        );

        assert_eq!(
            bounded_fingerprint(&left_arena, left_root),
            bounded_fingerprint(&right_arena, right_root)
        );

        let mut shared_arena = PlanRowExpressionArena::new();
        let shared_status = push(
            &mut shared_arena,
            PlanRowExpressionNode::Intrinsic {
                intrinsic: PlanIntrinsic::SessionInfoStatus,
            },
        );
        let shared_root = push(
            &mut shared_arena,
            PlanRowExpressionNode::TextConcat {
                parts: vec![shared_status, shared_status],
            },
        );
        let mut duplicate_arena = PlanRowExpressionArena::new();
        let first_status = push(
            &mut duplicate_arena,
            PlanRowExpressionNode::Intrinsic {
                intrinsic: PlanIntrinsic::SessionInfoStatus,
            },
        );
        let second_status = push(
            &mut duplicate_arena,
            PlanRowExpressionNode::Intrinsic {
                intrinsic: PlanIntrinsic::SessionInfoStatus,
            },
        );
        let duplicate_root = push(
            &mut duplicate_arena,
            PlanRowExpressionNode::TextConcat {
                parts: vec![first_status, second_status],
            },
        );
        assert_eq!(
            bounded_fingerprint(&shared_arena, shared_root),
            bounded_fingerprint(&duplicate_arena, duplicate_root)
        );
    }

    #[test]
    fn cpu_validation_uses_runtime_dependencies_not_metadata_edges() {
        let mut arena = PlanRowExpressionArena::new();
        let invalid_metadata = push(
            &mut arena,
            PlanRowExpressionNode::BuiltinCall {
                function: PlanRowBuiltin::TextContains,
                input: None,
                args: Vec::new(),
            },
        );
        let limit = push(
            &mut arena,
            PlanRowExpressionNode::Intrinsic {
                intrinsic: PlanIntrinsic::SessionInfoStatus,
            },
        );
        let access = push(
            &mut arena,
            PlanRowExpressionNode::ListAccess {
                access: Box::new(PlanListAccess {
                    index: PlanListIndexId(0),
                    semantic_order: vec![PlanListIndexKey {
                        owner: PlanStaticOwnerId(0),
                        row_local: PlanLocalId(0),
                        expression: invalid_metadata,
                        kind: PlanListIndexKeyKind::Text,
                        closed_tags: Vec::new(),
                        direction: PlanOrderDirection::Ascending,
                        multiplicity: PlanListIndexKeyMultiplicity::One,
                    }],
                    exhaustive_candidate_limit: None,
                    guard: None,
                    filters: Vec::new(),
                    maps: Vec::new(),
                    selection: PlanListAccessSelection::OrderedStart,
                    limit,
                }),
            },
        );
        let range = push(
            &mut arena,
            PlanRowExpressionNode::ListRange {
                from: invalid_metadata,
                to: invalid_metadata,
            },
        );

        assert!(!row_expression_cpu_evaluable(&arena, invalid_metadata));
        assert!(row_expression_cpu_evaluable(&arena, access));
        assert!(row_expression_cpu_evaluable(&arena, range));
    }

    #[test]
    fn root_walk_does_not_revalidate_unreachable_arena_nodes() {
        let arena = PlanRowExpressionArena {
            nodes: vec![
                PlanRowExpressionNode::Intrinsic {
                    intrinsic: PlanIntrinsic::SessionInfoStatus,
                },
                PlanRowExpressionNode::TextTrim {
                    input: PlanRowExpressionId(1),
                },
            ],
            structural_index: None,
        };

        assert!(arena.validate().is_err());
        assert_eq!(
            arena.walk_postorder(PlanRowExpressionId(0)).unwrap(),
            vec![PlanRowExpressionId(0)]
        );
        assert!(arena.walk_postorder(PlanRowExpressionId(1)).is_err());
        assert!(arena.walk_postorder(PlanRowExpressionId(2)).is_err());
    }

    #[test]
    fn visit_children_reports_each_direct_child_once() {
        let id = |value| PlanRowExpressionId(value);
        let node = PlanRowExpressionNode::ListPage {
            page: Box::new(PlanListPage {
                access: PlanListAccess {
                    index: PlanListIndexId(0),
                    semantic_order: vec![PlanListIndexKey {
                        owner: PlanStaticOwnerId(0),
                        row_local: PlanLocalId(0),
                        expression: id(0),
                        kind: PlanListIndexKeyKind::Number,
                        closed_tags: Vec::new(),
                        direction: PlanOrderDirection::Ascending,
                        multiplicity: PlanListIndexKeyMultiplicity::One,
                    }],
                    exhaustive_candidate_limit: None,
                    guard: Some(id(1)),
                    filters: vec![PlanListFilter {
                        owner: PlanStaticOwnerId(1),
                        row_local: PlanLocalId(1),
                        predicate: id(2),
                    }],
                    maps: vec![PlanListMap {
                        owner: PlanStaticOwnerId(2),
                        row_local: PlanLocalId(2),
                        body: id(3),
                        captures: vec![PlanRowCapture {
                            field: FieldId(0),
                            value: id(4),
                        }],
                    }],
                    selection: PlanListAccessSelection::Union {
                        branches: vec![
                            PlanListAccessSelection::TextPrefix {
                                leading: vec![id(5)],
                                prefix: id(6),
                            },
                            PlanListAccessSelection::ComponentRange {
                                leading: vec![id(7)],
                                lower: Some(PlanListAccessBound {
                                    value: id(8),
                                    inclusive: true,
                                }),
                                upper: Some(PlanListAccessBound {
                                    value: id(9),
                                    inclusive: false,
                                }),
                            },
                        ],
                    },
                    limit: id(10),
                },
                view_limit: Some(id(11)),
                after: id(12),
                view_fingerprint: [0; 32],
            }),
        };

        assert_eq!(
            node.child_ids(),
            (0..=12).map(PlanRowExpressionId).collect::<Vec<_>>()
        );
    }
}

#[cfg(test)]
mod tests;
