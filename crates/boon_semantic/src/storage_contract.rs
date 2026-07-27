//! Semantic-owned scope and storage topology.
//!
//! Reactive fields are declaration/value anchors, not a total storage domain:
//! list authority columns, inline record projections, and detached row captures
//! do not all have a [`crate::SemanticFieldId`]. This module allocates the
//! complete pre-backend storage-field identity and retains exact joins back to
//! every reactive field and producer result.

use crate::{
    ResolvedOutGraph, SemanticBindingId, SemanticBindingTargetV1, SemanticCallId,
    SemanticContextualOperationKind, SemanticContextualRowPredecessor, SemanticExecutionGraphV1,
    SemanticExprId, SemanticExpressionKind, SemanticFieldId, SemanticListId,
    SemanticLoweringContractV1, SemanticMaterializationLocalId, SemanticNamedValueId,
    SemanticReactiveGraphV1, SemanticReadId, SemanticReadTargetV1, SemanticResourceGraphV1,
    SemanticRowBinding, SemanticSourceId, SemanticSourceOrigin, SemanticStateId,
    SemanticStatementId, SemanticValueId, SemanticValueListAuthorityId, SemanticValueOrigin,
    StaticOwnerId,
};
use boon_contract::SourceBundleDigestV1;
use boon_typecheck::{
    CheckedExternalDeclarationIdentityV1, CheckedProgram, DeclId, FlowMode, FlowType, Type,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

pub const SEMANTIC_SCOPE_STORAGE_GRAPH_SCHEMA_V1: &str = "boon.semantic-scope-storage-graph.v1";
const SEMANTIC_SCOPE_STORAGE_GRAPH_DIGEST_DOMAIN: &[u8] = b"boon.semantic-scope-storage-graph.v1\0";

macro_rules! storage_id {
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

storage_id!(
    SemanticStorageFieldId,
    SemanticStorageExternalReferenceId,
    SemanticStorageCaptureId,
    SemanticStorageProjectionId,
);

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SemanticScopeStorageGraphDigestV1([u8; 32]);

impl SemanticScopeStorageGraphDigestV1 {
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    pub fn to_hex(self) -> String {
        self.0.iter().map(|byte| format!("{byte:02x}")).collect()
    }
}

impl fmt::Display for SemanticScopeStorageGraphDigestV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.to_hex())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticScopeStorageGraphV1 {
    pub schema: String,
    pub source_bundle_digest_v1: SourceBundleDigestV1,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub owners: Vec<SemanticStorageOwnerV1>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub locals: Vec<SemanticStorageLocalV1>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fields: Vec<SemanticStorageFieldV1>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bindings: Vec<SemanticStorageBindingV1>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<SemanticStorageSourceV1>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub row_values: Vec<SemanticStorageRowValueV1>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub row_source_projections: Vec<SemanticStorageRowSourceProjectionV1>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub external_references: Vec<SemanticStorageExternalReferenceV1>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub producer_result_fields: Vec<SemanticProducerResultStorageV1>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub named_values: Vec<SemanticNamedValueStorageV1>,
    pub digest: SemanticScopeStorageGraphDigestV1,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticStorageOwnerV1 {
    pub id: StaticOwnerId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<StaticOwnerId>,
    pub child_ordinal: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_row: Option<SemanticRowBinding>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_row: Option<SemanticRowBinding>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authority_row: Option<SemanticRowBinding>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticStorageLocalV1 {
    pub owner: StaticOwnerId,
    pub local: SemanticMaterializationLocalId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub row: Option<SemanticRowBinding>,
    pub source: SemanticExprId,
    pub item_type: Type,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub members: Vec<SemanticStorageLocalMemberV1>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub captures: Vec<SemanticStorageLocalCaptureV1>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticStorageLocalMemberV1 {
    pub path: Vec<String>,
    pub target: SemanticStorageLocalMemberTargetV1,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub forwarded_from: Option<SemanticStorageLocalMemberForwardingV1>,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(tag = "kind", content = "id", rename_all = "snake_case")]
pub enum SemanticStorageLocalMemberTargetV1 {
    Field(SemanticStorageFieldId),
    Source(SemanticSourceId),
    State(SemanticStateId),
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SemanticStorageLocalMemberForwardingV1 {
    Local {
        owner: StaticOwnerId,
        local: SemanticMaterializationLocalId,
        path: Vec<String>,
    },
    Row {
        row: SemanticRowBinding,
        path: Vec<String>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticStorageLocalCaptureV1 {
    pub id: SemanticStorageCaptureId,
    pub source_owner: StaticOwnerId,
    pub source_local: SemanticMaterializationLocalId,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub projection: Vec<String>,
    pub field: SemanticStorageFieldId,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticStorageFieldRoleV1 {
    Value,
    ListAuthority,
    ValueAuthority,
    Capture,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticStorageFieldV1 {
    pub id: SemanticStorageFieldId,
    pub role: SemanticStorageFieldRoleV1,
    pub origin: SemanticStorageFieldOriginV1,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reactive_field: Option<SemanticFieldId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub producer_identity: Option<[u8; 32]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub declaration: Option<DeclId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<StaticOwnerId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<SemanticStorageFieldId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub row: Option<SemanticRowBinding>,
    pub name: String,
    /// Diagnostic only. Storage identity is `id`.
    pub diagnostic_path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub statement: Option<SemanticStatementId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub producer: Option<SemanticExprId>,
    pub resource_only: bool,
    pub flow_type: FlowType,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SemanticStorageFieldOriginV1 {
    Reactive {
        field: SemanticFieldId,
    },
    ListAuthority {
        list: SemanticListId,
        item_path: Vec<String>,
    },
    ValueListAuthority {
        authority: SemanticValueListAuthorityId,
        item_path: Vec<String>,
    },
    RecordProjection {
        parent: SemanticStorageFieldId,
        expression: SemanticExprId,
        projection: Vec<String>,
    },
    DetachedCapture {
        capture: SemanticStorageCaptureId,
        target_owner: StaticOwnerId,
        target_local: SemanticMaterializationLocalId,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticStorageBindingV1 {
    pub binding: SemanticBindingId,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub owner_ancestry: Vec<StaticOwnerId>,
    /// Diagnostic only. Storage identity is `binding` plus `target`.
    pub diagnostic_path: String,
    pub target: SemanticStorageBindingTargetV1,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SemanticStorageBindingTargetV1 {
    Value {
        field: SemanticStorageFieldId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        row: Option<SemanticRowBinding>,
    },
    Source {
        source: SemanticSourceId,
    },
    State {
        state: SemanticStateId,
        published: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        field: Option<SemanticStorageFieldId>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        row: Option<SemanticRowBinding>,
    },
    List {
        list: SemanticListId,
        field: SemanticStorageFieldId,
        row: SemanticRowBinding,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticStorageSourceV1 {
    pub source: SemanticSourceId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<StaticOwnerId>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub owner_ancestry: Vec<StaticOwnerId>,
    pub origin: SemanticSourceOrigin,
    pub binding: SemanticBindingId,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct SemanticStorageRowValueV1 {
    pub expression: SemanticExprId,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub projection: Vec<String>,
    pub row: SemanticRowBinding,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct SemanticStorageRowSourceProjectionV1 {
    pub row: SemanticRowBinding,
    pub path: Vec<String>,
    pub source: SemanticSourceId,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticStorageExternalReferenceV1 {
    pub id: SemanticStorageExternalReferenceId,
    pub kind: SemanticStorageExternalReferenceKindV1,
    pub canonical_path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub external_identity: Option<CheckedExternalDeclarationIdentityV1>,
    pub bundle_ready: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SemanticStorageExternalReferenceKindV1 {
    Read {
        read: SemanticReadId,
        expression: SemanticExprId,
    },
    Call {
        call: SemanticCallId,
        expression: SemanticExprId,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticProducerResultStorageV1 {
    pub identity: [u8; 32],
    pub binding: SemanticBindingId,
    pub reactive_field: SemanticFieldId,
    pub storage_field: SemanticStorageFieldId,
}

/// Total semantic target for one checked named-value origin.
///
/// `named_value` and `origin_ordinal` select the type metadata entry without
/// consulting its diagnostic path. Multiple concrete contextual targets are
/// represented by separate, deterministically ordered rows.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticNamedValueStorageV1 {
    pub named_value: SemanticNamedValueId,
    pub origin_ordinal: usize,
    pub target_ordinal: usize,
    pub target: SemanticNamedValueStorageTargetV1,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub projection: Vec<SemanticStorageProjectionStepV1>,
    pub representation: SemanticStorageRepresentationV1,
    pub flow_type: FlowType,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SemanticNamedValueStorageTargetV1 {
    Field {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        binding: Option<SemanticBindingId>,
        field: SemanticStorageFieldId,
    },
    Source {
        binding: SemanticBindingId,
        source: SemanticSourceId,
    },
    State {
        binding: SemanticBindingId,
        state: SemanticStateId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        field: Option<SemanticStorageFieldId>,
    },
    List {
        binding: SemanticBindingId,
        list: SemanticListId,
        field: SemanticStorageFieldId,
        row: SemanticRowBinding,
    },
    Value {
        expression: SemanticExprId,
        value: SemanticValueId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        field: Option<SemanticStorageFieldId>,
    },
    DiagnosticOnly {
        reason: SemanticNamedValueDiagnosticOnlyReasonV1,
    },
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticNamedValueDiagnosticOnlyReasonV1 {
    NonExecutableStructuralContainer,
}

/// One checked and type-resolved object projection rooted at an exact semantic
/// target. `selector` is a structural object-field selector, not a canonical
/// value path; consumers must never resolve the root from it.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticStorageProjectionStepV1 {
    pub id: SemanticStorageProjectionId,
    pub ordinal: usize,
    pub selector: String,
    pub field_ordinal: usize,
    pub input_type: Type,
    pub output_type: Type,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub storage_field: Option<SemanticStorageFieldId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expression: Option<SemanticExprId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<SemanticValueId>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SemanticStorageRepresentationV1 {
    Exact,
    CheckedFixedBytes {
        refinements: Vec<SemanticStorageFixedBytesRefinementV1>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticStorageFixedBytesRefinementV1 {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub path: Vec<SemanticStorageTypePathSegmentV1>,
    pub fixed_len: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SemanticStorageTypePathSegmentV1 {
    ObjectField {
        selector: String,
        field_ordinal: usize,
    },
    ListItem,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SemanticScopeStorageError {
    message: String,
}

impl SemanticScopeStorageError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for SemanticScopeStorageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for SemanticScopeStorageError {}

pub(crate) fn build_semantic_scope_storage_graph(
    checked: &CheckedProgram,
    execution: &SemanticExecutionGraphV1,
    resources: &SemanticResourceGraphV1,
    reactive: &SemanticReactiveGraphV1,
    lowering: &SemanticLoweringContractV1,
    out_net: &ResolvedOutGraph,
) -> Result<SemanticScopeStorageGraphV1, SemanticScopeStorageError> {
    execution
        .validate(out_net)
        .map_err(SemanticScopeStorageError::new)?;
    resources
        .validate(execution, out_net)
        .map_err(SemanticScopeStorageError::new)?;
    reactive
        .validate(execution, resources, out_net)
        .map_err(|error| SemanticScopeStorageError::new(error.to_string()))?;

    let owners = build_owners(execution, resources)?;
    let mut fields = build_storage_fields(checked, execution, resources, reactive)?;
    let mut locals = build_storage_locals(execution, resources, &fields)?;
    resolve_local_forwarding(execution, resources, &mut locals)?;
    let captures = discover_detached_captures(execution, resources, reactive, &owners, &locals)?;
    append_capture_fields(&captures, &mut fields, &mut locals)?;
    classify_resource_only_fields(execution, resources, &locals, &mut fields)?;
    let bindings = build_storage_bindings(execution, resources, reactive, &fields, &owners)?;
    let sources = build_storage_sources(resources, reactive, &owners)?;
    let row_values = build_row_values(execution, &locals)?;
    let row_source_projections = build_row_source_projections(execution, resources, &locals)?;
    let external_references = build_external_references(execution, reactive)?;
    let producer_result_fields = build_producer_result_fields(reactive, &fields, &bindings)?;
    let named_values = build_named_value_storage(
        checked, execution, resources, reactive, lowering, &fields, &bindings,
    )?;

    let mut graph = SemanticScopeStorageGraphV1 {
        schema: SEMANTIC_SCOPE_STORAGE_GRAPH_SCHEMA_V1.to_owned(),
        source_bundle_digest_v1: checked.source_bundle_digest_v1,
        owners,
        locals,
        fields,
        bindings,
        sources,
        row_values,
        row_source_projections,
        external_references,
        producer_result_fields,
        named_values,
        digest: SemanticScopeStorageGraphDigestV1([0; 32]),
    };
    validate_storage_shape(&graph, checked, execution, resources, reactive, lowering)?;
    graph.digest = scope_storage_digest(&graph)?;
    Ok(graph)
}

impl SemanticScopeStorageGraphV1 {
    pub(crate) fn validate(
        &self,
        checked: &CheckedProgram,
        execution: &SemanticExecutionGraphV1,
        resources: &SemanticResourceGraphV1,
        reactive: &SemanticReactiveGraphV1,
        lowering: &SemanticLoweringContractV1,
        out_net: &ResolvedOutGraph,
    ) -> Result<(), SemanticScopeStorageError> {
        let expected = build_semantic_scope_storage_graph(
            checked, execution, resources, reactive, lowering, out_net,
        )?;
        if self != &expected {
            return Err(SemanticScopeStorageError::new(
                "semantic scope-storage graph differs from its deterministic semantic derivation",
            ));
        }
        Ok(())
    }
}

fn build_owners(
    execution: &SemanticExecutionGraphV1,
    resources: &SemanticResourceGraphV1,
) -> Result<Vec<SemanticStorageOwnerV1>, SemanticScopeStorageError> {
    execution
        .static_owners
        .iter()
        .map(|owner| {
            let materializations = resources
                .materialization_bindings
                .iter()
                .filter(|binding| binding.owner == owner.id)
                .collect::<Vec<_>>();
            if materializations.len() > 1 {
                return Err(SemanticScopeStorageError::new(format!(
                    "static owner {} has {} materialization storage bindings",
                    owner.id,
                    materializations.len()
                )));
            }
            let source_row = materializations.first().and_then(|binding| binding.source);
            let target_row = materializations.first().and_then(|binding| binding.target);
            Ok(SemanticStorageOwnerV1 {
                id: owner.id,
                parent: owner.parent,
                child_ordinal: owner.child_ordinal,
                source_row,
                target_row,
                authority_row: target_row.or(source_row),
            })
        })
        .collect()
}

fn build_storage_fields(
    checked: &CheckedProgram,
    execution: &SemanticExecutionGraphV1,
    resources: &SemanticResourceGraphV1,
    reactive: &SemanticReactiveGraphV1,
) -> Result<Vec<SemanticStorageFieldV1>, SemanticScopeStorageError> {
    let reactive_by_statement = reactive
        .fields
        .iter()
        .map(|field| (field.statement, field))
        .collect::<BTreeMap<_, _>>();
    if reactive_by_statement.len() != reactive.fields.len() {
        return Err(SemanticScopeStorageError::new(
            "reactive graph has multiple fields for one semantic statement",
        ));
    }
    let producer_identity_by_statement = reactive
        .producer_instances
        .iter()
        .map(|producer| (producer.result_statement, producer.identity))
        .collect::<BTreeMap<_, _>>();
    let mut fields = Vec::new();
    let mut reactive_storage = BTreeMap::new();
    for reactive_field in &reactive.fields {
        if !checked
            .declarations
            .iter()
            .any(|declaration| declaration.id == reactive_field.declaration)
        {
            return Err(SemanticScopeStorageError::new(format!(
                "reactive field {} references missing checked declaration {}",
                reactive_field.id, reactive_field.declaration.0
            )));
        }
        let parent = nearest_parent_storage_field(
            execution,
            reactive_field.statement,
            &reactive_by_statement,
            &reactive_storage,
        )?;
        let id = SemanticStorageFieldId(fields.len());
        reactive_storage.insert(reactive_field.id, id);
        fields.push(SemanticStorageFieldV1 {
            id,
            role: SemanticStorageFieldRoleV1::Value,
            origin: SemanticStorageFieldOriginV1::Reactive {
                field: reactive_field.id,
            },
            reactive_field: Some(reactive_field.id),
            producer_identity: producer_identity_by_statement
                .get(&reactive_field.statement)
                .copied(),
            declaration: Some(reactive_field.declaration),
            owner: reactive_field.owner,
            parent,
            row: reactive_field.row,
            name: reactive_field.name.clone(),
            diagnostic_path: reactive_field.path.clone(),
            statement: Some(reactive_field.statement),
            producer: Some(reactive_field.producer),
            resource_only: false,
            flow_type: reactive_field.flow_type.clone(),
        });
    }

    append_list_authority_fields(
        resources,
        &reactive_by_statement,
        &reactive_storage,
        &mut fields,
    )?;
    append_record_projection_fields(execution, &mut fields)?;
    Ok(fields)
}

fn nearest_parent_storage_field(
    execution: &SemanticExecutionGraphV1,
    statement: SemanticStatementId,
    reactive_by_statement: &BTreeMap<SemanticStatementId, &crate::SemanticFieldV1>,
    reactive_storage: &BTreeMap<SemanticFieldId, SemanticStorageFieldId>,
) -> Result<Option<SemanticStorageFieldId>, SemanticScopeStorageError> {
    let mut parent = require_statement(execution, statement)?.parent;
    let mut visited = BTreeSet::new();
    while let Some(statement) = parent {
        if !visited.insert(statement) {
            return Err(SemanticScopeStorageError::new(
                "semantic statement parent graph contains a cycle",
            ));
        }
        if let Some(field) = reactive_by_statement.get(&statement) {
            return reactive_storage
                .get(&field.id)
                .copied()
                .map(Some)
                .ok_or_else(|| {
                    SemanticScopeStorageError::new(format!(
                        "reactive parent field {} is not ordered before child statement {statement}",
                        field.id
                    ))
                });
        }
        parent = require_statement(execution, statement)?.parent;
    }
    Ok(None)
}

fn append_list_authority_fields(
    resources: &SemanticResourceGraphV1,
    reactive_by_statement: &BTreeMap<SemanticStatementId, &crate::SemanticFieldV1>,
    reactive_storage: &BTreeMap<SemanticFieldId, SemanticStorageFieldId>,
    fields: &mut Vec<SemanticStorageFieldV1>,
) -> Result<(), SemanticScopeStorageError> {
    for list in &resources.lists {
        let row = SemanticRowBinding {
            list: list.id,
            scope: list.row_scope,
        };
        let parent = reactive_by_statement
            .get(&list.statement)
            .and_then(|field| reactive_storage.get(&field.id))
            .copied()
            .ok_or_else(|| {
                SemanticScopeStorageError::new(format!(
                    "list {} statement {} has no reactive storage field",
                    list.id, list.statement
                ))
            })?;
        for (path, data_type) in
            list_authority_fields(&list.item_type, &list.initializer, &list.item_fields)
        {
            let name = path.last().cloned().ok_or_else(|| {
                SemanticScopeStorageError::new(format!(
                    "list {} produced an empty authority field path",
                    list.id
                ))
            })?;
            let parent_field = if path.len() == 1 {
                parent
            } else {
                let parent_path = &path[..path.len() - 1];
                ensure_authority_parent(
                    list.id,
                    row,
                    parent,
                    parent_path,
                    &list.semantic_path,
                    fields,
                )?
            };
            if fields.iter().any(|field| {
                field.row == Some(row)
                    && field.parent == Some(parent_field)
                    && field.name == name
                    && matches!(
                        field.origin,
                        SemanticStorageFieldOriginV1::ListAuthority { list: candidate, .. }
                            if candidate == list.id
                    )
            }) {
                continue;
            }
            fields.push(SemanticStorageFieldV1 {
                id: SemanticStorageFieldId(fields.len()),
                role: SemanticStorageFieldRoleV1::ListAuthority,
                origin: SemanticStorageFieldOriginV1::ListAuthority {
                    list: list.id,
                    item_path: path.clone(),
                },
                reactive_field: None,
                producer_identity: None,
                declaration: None,
                owner: None,
                parent: Some(parent_field),
                row: Some(row),
                name,
                diagnostic_path: format!("{}.{}", list.semantic_path, path.join(".")),
                statement: Some(list.statement),
                producer: None,
                resource_only: false,
                flow_type: FlowType {
                    mode: FlowMode::Continuous,
                    ty: data_type,
                },
            });
        }
    }
    for authority in &resources.value_list_authorities {
        let parent = reactive_by_statement
            .get(&authority.statement)
            .and_then(|field| reactive_storage.get(&field.id))
            .copied()
            .ok_or_else(|| {
                SemanticScopeStorageError::new(format!(
                    "value-list authority {} statement {} has no reactive storage field",
                    authority.id, authority.statement
                ))
            })?;
        for (path, data_type) in
            list_authority_fields(&authority.item_type, &authority.initializer, &[])
        {
            let name = path.last().cloned().ok_or_else(|| {
                SemanticScopeStorageError::new(format!(
                    "value-list authority {} produced an empty authority field path",
                    authority.id
                ))
            })?;
            let parent_field = if path.len() == 1 {
                parent
            } else {
                ensure_value_authority_parent(
                    authority.id,
                    parent,
                    &path[..path.len() - 1],
                    &authority.semantic_path,
                    fields,
                )?
            };
            fields.push(SemanticStorageFieldV1 {
                id: SemanticStorageFieldId(fields.len()),
                role: SemanticStorageFieldRoleV1::ListAuthority,
                origin: SemanticStorageFieldOriginV1::ValueListAuthority {
                    authority: authority.id,
                    item_path: path.clone(),
                },
                reactive_field: None,
                producer_identity: None,
                declaration: None,
                owner: None,
                parent: Some(parent_field),
                row: None,
                name,
                diagnostic_path: format!("{}.{}", authority.semantic_path, path.join(".")),
                statement: Some(authority.statement),
                producer: None,
                resource_only: false,
                flow_type: FlowType {
                    mode: FlowMode::Continuous,
                    ty: data_type,
                },
            });
        }
    }
    Ok(())
}

fn list_authority_fields(
    item_type: &Type,
    initializer: &crate::SemanticListInitializerV1,
    item_fields: &[String],
) -> Vec<(Vec<String>, Type)> {
    let mut fields = flattened_type_fields(item_type);
    if matches!(initializer, crate::SemanticListInitializerV1::Range { .. }) {
        fields.push((vec!["index".to_owned()], Type::Number));
        fields.push((vec!["value".to_owned()], Type::Number));
    } else if fields.is_empty() {
        fields.push((vec!["value".to_owned()], item_type.clone()));
    }
    for name in item_fields {
        if fields
            .iter()
            .any(|(path, _)| path.len() == 1 && path[0] == *name)
        {
            continue;
        }
        let data_type = match item_type {
            Type::Object(shape) => shape.fields.get(name).cloned().unwrap_or(Type::Unknown),
            item => item.clone(),
        };
        fields.push((vec![name.clone()], data_type));
    }
    fields.sort_by(|left, right| left.0.cmp(&right.0));
    fields.dedup_by(|left, right| left.0 == right.0);
    fields
}

fn ensure_value_authority_parent(
    authority: SemanticValueListAuthorityId,
    root: SemanticStorageFieldId,
    path: &[String],
    authority_path: &str,
    fields: &mut Vec<SemanticStorageFieldV1>,
) -> Result<SemanticStorageFieldId, SemanticScopeStorageError> {
    let mut parent = root;
    let mut prefix = Vec::new();
    for name in path {
        prefix.push(name.clone());
        let matches = fields
            .iter()
            .filter(|field| {
                field.parent == Some(parent)
                    && field.row.is_none()
                    && field.name == *name
                    && matches!(
                        field.origin,
                        SemanticStorageFieldOriginV1::ValueListAuthority {
                            authority: candidate,
                            ..
                        } if candidate == authority
                    )
            })
            .map(|field| field.id)
            .collect::<Vec<_>>();
        parent = match matches.as_slice() {
            [field] => *field,
            [] => {
                let id = SemanticStorageFieldId(fields.len());
                fields.push(SemanticStorageFieldV1 {
                    id,
                    role: SemanticStorageFieldRoleV1::ListAuthority,
                    origin: SemanticStorageFieldOriginV1::ValueListAuthority {
                        authority,
                        item_path: prefix.clone(),
                    },
                    reactive_field: None,
                    producer_identity: None,
                    declaration: None,
                    owner: None,
                    parent: Some(parent),
                    row: None,
                    name: name.clone(),
                    diagnostic_path: format!("{authority_path}.{}", prefix.join(".")),
                    statement: None,
                    producer: None,
                    resource_only: false,
                    flow_type: FlowType {
                        mode: FlowMode::Continuous,
                        ty: Type::Object(boon_typecheck::ObjectShape {
                            fields: BTreeMap::new(),
                            field_order: Vec::new(),
                            open: false,
                        }),
                    },
                });
                id
            }
            _ => {
                return Err(SemanticScopeStorageError::new(format!(
                    "value-list authority {authority} parent `{}` resolves to {} storage fields",
                    prefix.join("."),
                    matches.len()
                )));
            }
        };
    }
    Ok(parent)
}

fn ensure_authority_parent(
    list: SemanticListId,
    row: SemanticRowBinding,
    root: SemanticStorageFieldId,
    path: &[String],
    list_path: &str,
    fields: &mut Vec<SemanticStorageFieldV1>,
) -> Result<SemanticStorageFieldId, SemanticScopeStorageError> {
    let mut parent = root;
    let mut prefix = Vec::new();
    for name in path {
        prefix.push(name.clone());
        let matches = fields
            .iter()
            .filter(|field| {
                field.parent == Some(parent)
                    && field.row == Some(row)
                    && field.name == *name
                    && matches!(
                        field.origin,
                        SemanticStorageFieldOriginV1::ListAuthority {
                            list: candidate,
                            ..
                        } if candidate == list
                    )
            })
            .map(|field| field.id)
            .collect::<Vec<_>>();
        parent = match matches.as_slice() {
            [field] => *field,
            [] => {
                let id = SemanticStorageFieldId(fields.len());
                fields.push(SemanticStorageFieldV1 {
                    id,
                    role: SemanticStorageFieldRoleV1::ListAuthority,
                    origin: SemanticStorageFieldOriginV1::ListAuthority {
                        list,
                        item_path: prefix.clone(),
                    },
                    reactive_field: None,
                    producer_identity: None,
                    declaration: None,
                    owner: None,
                    parent: Some(parent),
                    row: Some(row),
                    name: name.clone(),
                    diagnostic_path: format!("{list_path}.{}", prefix.join(".")),
                    statement: None,
                    producer: None,
                    resource_only: false,
                    flow_type: FlowType {
                        mode: FlowMode::Continuous,
                        ty: Type::Object(boon_typecheck::ObjectShape {
                            fields: BTreeMap::new(),
                            field_order: Vec::new(),
                            open: false,
                        }),
                    },
                });
                id
            }
            _ => {
                return Err(SemanticScopeStorageError::new(format!(
                    "list {list} authority parent `{}` resolves to {} storage fields",
                    prefix.join("."),
                    matches.len()
                )));
            }
        };
    }
    Ok(parent)
}

fn flattened_type_fields(data_type: &Type) -> Vec<(Vec<String>, Type)> {
    fn visit(ty: &Type, prefix: &mut Vec<String>, out: &mut Vec<(Vec<String>, Type)>) {
        let Type::Object(shape) = ty else {
            return;
        };
        let mut seen = BTreeSet::new();
        for name in shape.field_order.iter().chain(shape.fields.keys()) {
            if !seen.insert(name.clone()) {
                continue;
            }
            let Some(field_type) = shape.fields.get(name) else {
                continue;
            };
            prefix.push(name.clone());
            out.push((prefix.clone(), field_type.clone()));
            visit(field_type, prefix, out);
            prefix.pop();
        }
    }
    let mut fields = Vec::new();
    visit(data_type, &mut Vec::new(), &mut fields);
    fields
}

fn append_record_projection_fields(
    execution: &SemanticExecutionGraphV1,
    fields: &mut Vec<SemanticStorageFieldV1>,
) -> Result<(), SemanticScopeStorageError> {
    let mut parent_index = 0;
    while parent_index < fields.len() {
        let parent = fields[parent_index].clone();
        parent_index += 1;
        let Some(producer) = parent.producer else {
            continue;
        };
        let expression = require_expression(execution, producer)?;
        let record_fields = match &expression.kind {
            SemanticExpressionKind::Object(fields)
            | SemanticExpressionKind::Record(fields)
            | SemanticExpressionKind::TaggedObject { fields, .. } => fields,
            _ => continue,
        };
        for record_field in record_fields.iter().filter(|field| !field.spread) {
            if fields.iter().any(|field| {
                field.parent == Some(parent.id)
                    && field.name == record_field.name
                    && field.producer == Some(record_field.value)
            }) {
                continue;
            }
            let value = require_expression(execution, record_field.value)?;
            fields.push(SemanticStorageFieldV1 {
                id: SemanticStorageFieldId(fields.len()),
                role: SemanticStorageFieldRoleV1::Value,
                origin: SemanticStorageFieldOriginV1::RecordProjection {
                    parent: parent.id,
                    expression: record_field.value,
                    projection: vec![record_field.name.clone()],
                },
                reactive_field: None,
                producer_identity: None,
                declaration: record_field.declaration,
                owner: value.owner.or(parent.owner),
                parent: Some(parent.id),
                row: parent.row,
                name: record_field.name.clone(),
                diagnostic_path: format!("{}.{}", parent.diagnostic_path, record_field.name),
                statement: None,
                producer: Some(record_field.value),
                resource_only: false,
                flow_type: value.flow_type.clone(),
            });
        }
    }
    Ok(())
}

fn build_storage_locals(
    execution: &SemanticExecutionGraphV1,
    resources: &SemanticResourceGraphV1,
    fields: &[SemanticStorageFieldV1],
) -> Result<Vec<SemanticStorageLocalV1>, SemanticScopeStorageError> {
    execution
        .materializations
        .iter()
        .map(|materialization| {
            let binding = resources
                .materialization_bindings
                .iter()
                .find(|binding| binding.materialization == materialization.id)
                .ok_or_else(|| {
                    SemanticScopeStorageError::new(format!(
                        "materialization {} has no exact storage binding",
                        materialization.id
                    ))
                })?;
            let members = binding
                .source
                .map(|row| {
                    local_members_for_row(resources, fields, row, &materialization.item_type)
                })
                .transpose()?
                .unwrap_or_default();
            Ok(SemanticStorageLocalV1 {
                owner: materialization.owner,
                local: materialization.row_local,
                row: binding.source,
                source: materialization.source,
                item_type: materialization.item_type.clone(),
                members,
                captures: Vec::new(),
            })
        })
        .collect()
}

fn local_members_for_row(
    resources: &SemanticResourceGraphV1,
    fields: &[SemanticStorageFieldV1],
    row: SemanticRowBinding,
    item_type: &Type,
) -> Result<Vec<SemanticStorageLocalMemberV1>, SemanticScopeStorageError> {
    let list = resources
        .lists
        .get(row.list.as_usize())
        .filter(|list| list.id == row.list && list.row_scope == row.scope)
        .ok_or_else(|| {
            SemanticScopeStorageError::new(format!(
                "local row {}/{} references missing list storage",
                row.list, row.scope
            ))
        })?;
    let mut members = BTreeMap::<Vec<String>, SemanticStorageLocalMemberV1>::new();
    for source in resources.sources.iter().filter(|source| {
        source.target_list == Some(row.list) && source.row_scope == Some(row.scope)
    }) {
        let path = relative_resource_path(&list.semantic_path, &source.semantic_path)?;
        insert_local_member(
            &mut members,
            SemanticStorageLocalMemberV1 {
                path,
                target: SemanticStorageLocalMemberTargetV1::Source(source.id),
                forwarded_from: None,
            },
            "row source",
        )?;
    }
    for state in resources.states.iter().filter(|state| {
        state.published && state.target_list == Some(row.list) && state.row_scope == Some(row.scope)
    }) {
        let path = relative_resource_path(&list.semantic_path, &state.path)?;
        insert_local_member(
            &mut members,
            SemanticStorageLocalMemberV1 {
                path,
                target: SemanticStorageLocalMemberTargetV1::State(state.id),
                forwarded_from: None,
            },
            "row state",
        )?;
    }
    for (path, _) in flattened_type_fields(item_type) {
        if members.contains_key(&path) {
            continue;
        }
        let candidates = fields
            .iter()
            .filter(|field| {
                field.row == Some(row)
                    && storage_field_item_path(field).as_deref() == Some(path.as_slice())
            })
            .collect::<Vec<_>>();
        let preferred = candidates
            .iter()
            .copied()
            .filter(|field| {
                matches!(
                    field.role,
                    SemanticStorageFieldRoleV1::ValueAuthority
                        | SemanticStorageFieldRoleV1::ListAuthority
                )
            })
            .collect::<Vec<_>>();
        let selected = if preferred.is_empty() {
            candidates
        } else {
            preferred
        };
        let [field] = selected.as_slice() else {
            return Err(SemanticScopeStorageError::new(format!(
                "row {}/{} member `{}` resolves to {} exact storage fields",
                row.list,
                row.scope,
                path.join("."),
                selected.len()
            )));
        };
        insert_local_member(
            &mut members,
            SemanticStorageLocalMemberV1 {
                path,
                target: SemanticStorageLocalMemberTargetV1::Field(field.id),
                forwarded_from: Some(SemanticStorageLocalMemberForwardingV1::Row {
                    row,
                    path: storage_field_item_path(field).unwrap_or_default(),
                }),
            },
            "row field",
        )?;
    }
    Ok(members.into_values().collect())
}

fn relative_resource_path(
    list_path: &str,
    resource_path: &str,
) -> Result<Vec<String>, SemanticScopeStorageError> {
    resource_path
        .strip_prefix(list_path)
        .and_then(|suffix| suffix.strip_prefix('.'))
        .filter(|suffix| !suffix.is_empty())
        .map(|suffix| suffix.split('.').map(str::to_owned).collect())
        .ok_or_else(|| {
            SemanticScopeStorageError::new(format!(
                "row resource `{resource_path}` is not structurally owned by list `{list_path}`"
            ))
        })
}

fn storage_field_item_path(field: &SemanticStorageFieldV1) -> Option<Vec<String>> {
    match &field.origin {
        SemanticStorageFieldOriginV1::ListAuthority { item_path, .. } => Some(item_path.clone()),
        SemanticStorageFieldOriginV1::Reactive { .. }
        | SemanticStorageFieldOriginV1::ValueListAuthority { .. }
        | SemanticStorageFieldOriginV1::RecordProjection { .. }
        | SemanticStorageFieldOriginV1::DetachedCapture { .. } => {
            field.row.map(|_| vec![field.name.clone()])
        }
    }
}

fn insert_local_member(
    members: &mut BTreeMap<Vec<String>, SemanticStorageLocalMemberV1>,
    incoming: SemanticStorageLocalMemberV1,
    context: &str,
) -> Result<(), SemanticScopeStorageError> {
    match members.get(&incoming.path) {
        None => {
            members.insert(incoming.path.clone(), incoming);
            Ok(())
        }
        Some(existing) if existing.target == incoming.target => Ok(()),
        Some(existing)
            if matches!(
                existing.target,
                SemanticStorageLocalMemberTargetV1::Field(_)
            ) && matches!(
                incoming.target,
                SemanticStorageLocalMemberTargetV1::Source(_)
            ) =>
        {
            members.insert(incoming.path.clone(), incoming);
            Ok(())
        }
        Some(existing) => Err(SemanticScopeStorageError::new(format!(
            "{context} member `{}` resolves to both {:?} and {:?}",
            incoming.path.join("."),
            existing.target,
            incoming.target
        ))),
    }
}

fn resolve_local_forwarding(
    execution: &SemanticExecutionGraphV1,
    resources: &SemanticResourceGraphV1,
    locals: &mut [SemanticStorageLocalV1],
) -> Result<(), SemanticScopeStorageError> {
    let local_index = locals
        .iter()
        .enumerate()
        .map(|(index, local)| ((local.owner, local.local), index))
        .collect::<BTreeMap<_, _>>();
    for materialization in &execution.materializations {
        let target_index = *local_index
            .get(&(materialization.owner, materialization.row_local))
            .ok_or_else(|| {
                SemanticScopeStorageError::new(format!(
                    "materialization {} has no exact semantic storage local",
                    materialization.id
                ))
            })?;
        let binding = resources
            .materialization_bindings
            .iter()
            .find(|binding| binding.materialization == materialization.id)
            .ok_or_else(|| {
                SemanticScopeStorageError::new(format!(
                    "materialization {} has no exact resource binding",
                    materialization.id
                ))
            })?;
        for predecessor in &binding.predecessors {
            let (SemanticContextualRowPredecessor::Materialized {
                materialization: input,
            }
            | SemanticContextualRowPredecessor::Provenance {
                materialization: input,
            }) = predecessor
            else {
                continue;
            };
            let input_materialization = execution
                .materializations
                .get(input.as_usize())
                .filter(|candidate| candidate.id == *input)
                .ok_or_else(|| {
                    SemanticScopeStorageError::new(format!(
                        "materialization {} references missing predecessor {input}",
                        materialization.id
                    ))
                })?;
            let source_index = *local_index
                .get(&(input_materialization.owner, input_materialization.row_local))
                .ok_or_else(|| {
                    SemanticScopeStorageError::new(format!(
                        "predecessor materialization {input} has no storage local"
                    ))
                })?;
            let source_members = locals[source_index].members.clone();
            for source_member in source_members.into_iter().filter(|member| {
                matches!(member.target, SemanticStorageLocalMemberTargetV1::Source(_))
            }) {
                let mut forwarded = source_member;
                forwarded.forwarded_from = Some(SemanticStorageLocalMemberForwardingV1::Local {
                    owner: input_materialization.owner,
                    local: input_materialization.row_local,
                    path: forwarded.path.clone(),
                });
                let target_members = &mut locals[target_index].members;
                let mut map = target_members
                    .drain(..)
                    .map(|member| (member.path.clone(), member))
                    .collect::<BTreeMap<_, _>>();
                insert_local_member(&mut map, forwarded, "materialization forwarding")?;
                *target_members = map.into_values().collect();
            }
        }
    }
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct CaptureRequest {
    target_owner: StaticOwnerId,
    target_local: SemanticMaterializationLocalId,
    target_row: SemanticRowBinding,
    source_owner: StaticOwnerId,
    source_local: SemanticMaterializationLocalId,
    projection: Vec<String>,
    data_type: Type,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct CaptureRequestKey {
    target_owner: StaticOwnerId,
    target_local: SemanticMaterializationLocalId,
    target_row: SemanticRowBinding,
    source_owner: StaticOwnerId,
    source_local: SemanticMaterializationLocalId,
    projection: Vec<String>,
}

fn discover_detached_captures(
    execution: &SemanticExecutionGraphV1,
    resources: &SemanticResourceGraphV1,
    reactive: &SemanticReactiveGraphV1,
    owners: &[SemanticStorageOwnerV1],
    locals: &[SemanticStorageLocalV1],
) -> Result<Vec<CaptureRequest>, SemanticScopeStorageError> {
    let mut requests = BTreeMap::new();
    for state in resources
        .states
        .iter()
        .filter(|state| state.row_scope.is_some())
    {
        let state_owner = state.owner.ok_or_else(|| {
            SemanticScopeStorageError::new(format!(
                "row-scoped state {} has no static owner",
                state.id
            ))
        })?;
        let target_row = SemanticRowBinding {
            list: state.target_list.ok_or_else(|| {
                SemanticScopeStorageError::new(format!(
                    "row-scoped state {} has no target list",
                    state.id
                ))
            })?,
            scope: state.row_scope.ok_or_else(|| {
                SemanticScopeStorageError::new(format!(
                    "row-scoped state {} has no row scope",
                    state.id
                ))
            })?,
        };
        let (target_owner, target_local) =
            nearest_target_materialization(execution, owners, state_owner, target_row)?;
        let mut roots = vec![state.initial];
        for update in reactive
            .state_update_arms
            .iter()
            .filter(|update| update.state == state.id)
        {
            let trigger = reactive
                .trigger_arms
                .get(update.trigger.as_usize())
                .filter(|trigger| trigger.id == update.trigger)
                .ok_or_else(|| {
                    SemanticScopeStorageError::new(format!(
                        "state update {} references missing trigger {}",
                        update.id, update.trigger
                    ))
                })?;
            roots.push(trigger.output_expression);
        }
        for root in roots {
            collect_capture_requests(
                execution,
                owners,
                locals,
                root,
                target_owner,
                target_local,
                target_row,
                &mut BTreeSet::new(),
                &mut requests,
            )?;
        }
    }
    Ok(requests
        .into_iter()
        .map(|(key, data_type)| CaptureRequest {
            target_owner: key.target_owner,
            target_local: key.target_local,
            target_row: key.target_row,
            source_owner: key.source_owner,
            source_local: key.source_local,
            projection: key.projection,
            data_type,
        })
        .collect())
}

fn nearest_target_materialization(
    execution: &SemanticExecutionGraphV1,
    owners: &[SemanticStorageOwnerV1],
    mut owner: StaticOwnerId,
    row: SemanticRowBinding,
) -> Result<(StaticOwnerId, SemanticMaterializationLocalId), SemanticScopeStorageError> {
    loop {
        let matches = execution
            .materializations
            .iter()
            .filter(|materialization| {
                materialization.owner == owner
                    && materialization.target_list_id == Some(row.list)
                    && materialization.target_scope_id == Some(row.scope)
            })
            .collect::<Vec<_>>();
        match matches.as_slice() {
            [materialization]
                if materialization.operation == SemanticContextualOperationKind::Map =>
            {
                return Ok((owner, materialization.row_local));
            }
            [materialization] => {
                return Err(SemanticScopeStorageError::new(format!(
                    "row {}/{} state owner {owner} is bound by non-map operation {:?}",
                    row.list, row.scope, materialization.operation
                )));
            }
            [] => {}
            _ => {
                return Err(SemanticScopeStorageError::new(format!(
                    "row {}/{} state owner {owner} has {} target materializations",
                    row.list,
                    row.scope,
                    matches.len()
                )));
            }
        }
        owner = owners
            .get(owner.as_usize())
            .filter(|candidate| candidate.id == owner)
            .and_then(|owner| owner.parent)
            .ok_or_else(|| {
                SemanticScopeStorageError::new(format!(
                    "row {}/{} state has no owning map materialization",
                    row.list, row.scope
                ))
            })?;
    }
}

#[allow(clippy::too_many_arguments)]
fn collect_capture_requests(
    execution: &SemanticExecutionGraphV1,
    owners: &[SemanticStorageOwnerV1],
    locals: &[SemanticStorageLocalV1],
    expression: SemanticExprId,
    target_owner: StaticOwnerId,
    target_local: SemanticMaterializationLocalId,
    target_row: SemanticRowBinding,
    visited: &mut BTreeSet<SemanticExprId>,
    requests: &mut BTreeMap<CaptureRequestKey, Type>,
) -> Result<(), SemanticScopeStorageError> {
    if !visited.insert(expression) {
        return Ok(());
    }
    let value = require_expression(execution, expression)?;
    if let SemanticExpressionKind::MaterializationLocal {
        owner,
        local,
        projection,
    } = &value.kind
    {
        let source = locals
            .iter()
            .find(|candidate| candidate.owner == *owner && candidate.local == *local)
            .ok_or_else(|| {
                SemanticScopeStorageError::new(format!(
                    "capture expression {expression} references missing local {owner}:{local}"
                ))
            })?;
        if source.row != Some(target_row) {
            if !owner_descends_from(target_owner, *owner, owners)? {
                return Err(SemanticScopeStorageError::new(format!(
                    "capture target owner {target_owner} is unrelated to source local {owner}:{local}"
                )));
            }
            let key = CaptureRequestKey {
                target_owner,
                target_local,
                target_row,
                source_owner: *owner,
                source_local: *local,
                projection: projection.clone(),
            };
            if let Some(existing) = requests.insert(key, value.flow_type.ty.clone())
                && existing != value.flow_type.ty
            {
                return Err(SemanticScopeStorageError::new(format!(
                    "detached capture expression {expression} has conflicting semantic types"
                )));
            }
        }
        return Ok(());
    }
    for child in expression_children(execution, &value.kind)? {
        collect_capture_requests(
            execution,
            owners,
            locals,
            child,
            target_owner,
            target_local,
            target_row,
            visited,
            requests,
        )?;
    }
    Ok(())
}

fn append_capture_fields(
    requests: &[CaptureRequest],
    fields: &mut Vec<SemanticStorageFieldV1>,
    locals: &mut [SemanticStorageLocalV1],
) -> Result<(), SemanticScopeStorageError> {
    let next_capture = locals
        .iter()
        .map(|local| local.captures.len())
        .sum::<usize>();
    for (offset, request) in requests.iter().enumerate() {
        let target = locals
            .iter_mut()
            .find(|local| {
                local.owner == request.target_owner && local.local == request.target_local
            })
            .ok_or_else(|| {
                SemanticScopeStorageError::new(format!(
                    "capture target local {}:{} is missing",
                    request.target_owner, request.target_local
                ))
            })?;
        let capture = SemanticStorageCaptureId(next_capture + offset);
        let field = SemanticStorageFieldId(fields.len());
        fields.push(SemanticStorageFieldV1 {
            id: field,
            role: SemanticStorageFieldRoleV1::Capture,
            origin: SemanticStorageFieldOriginV1::DetachedCapture {
                capture,
                target_owner: request.target_owner,
                target_local: request.target_local,
            },
            reactive_field: None,
            producer_identity: None,
            declaration: None,
            owner: None,
            parent: None,
            row: Some(request.target_row),
            name: format!(
                "@capture/{}/{}/{}",
                request.target_owner.as_usize(),
                request.target_local.as_usize(),
                target.captures.len()
            ),
            diagnostic_path: format!(
                "@capture/{}/{}/from/{}/{}{}",
                request.target_owner.as_usize(),
                request.target_local.as_usize(),
                request.source_owner.as_usize(),
                request.source_local.as_usize(),
                request
                    .projection
                    .iter()
                    .map(|part| format!("/{part}"))
                    .collect::<String>()
            ),
            statement: None,
            producer: None,
            resource_only: false,
            flow_type: FlowType {
                mode: FlowMode::Continuous,
                ty: request.data_type.clone(),
            },
        });
        target.captures.push(SemanticStorageLocalCaptureV1 {
            id: capture,
            source_owner: request.source_owner,
            source_local: request.source_local,
            projection: request.projection.clone(),
            field,
        });
    }
    Ok(())
}

fn classify_resource_only_fields(
    execution: &SemanticExecutionGraphV1,
    resources: &SemanticResourceGraphV1,
    locals: &[SemanticStorageLocalV1],
    fields: &mut [SemanticStorageFieldV1],
) -> Result<(), SemanticScopeStorageError> {
    let snapshot = fields.to_vec();
    let mut resolver = StorageProvenanceResolver {
        execution,
        resources,
        locals,
        fields: &snapshot,
        visiting: BTreeSet::new(),
        cache: BTreeMap::new(),
    };
    for field in fields {
        field.resource_only = resolver.field_is_source_only(field.id)?;
    }
    Ok(())
}

struct StorageProvenanceResolver<'a> {
    execution: &'a SemanticExecutionGraphV1,
    resources: &'a SemanticResourceGraphV1,
    locals: &'a [SemanticStorageLocalV1],
    fields: &'a [SemanticStorageFieldV1],
    visiting: BTreeSet<SemanticStorageFieldId>,
    cache: BTreeMap<SemanticStorageFieldId, bool>,
}

impl StorageProvenanceResolver<'_> {
    fn field_is_source_only(
        &mut self,
        field: SemanticStorageFieldId,
    ) -> Result<bool, SemanticScopeStorageError> {
        if let Some(cached) = self.cache.get(&field) {
            return Ok(*cached);
        }
        if !self.visiting.insert(field) {
            return Ok(false);
        }
        let definition = self
            .fields
            .get(field.as_usize())
            .filter(|candidate| candidate.id == field)
            .ok_or_else(|| {
                SemanticScopeStorageError::new(format!(
                    "provenance references missing storage field {field}"
                ))
            })?;
        let result = definition
            .producer
            .map(|producer| self.expression_is_source_only(producer))
            .transpose()?
            .unwrap_or(false);
        self.visiting.remove(&field);
        self.cache.insert(field, result);
        Ok(result)
    }

    fn expression_is_source_only(
        &mut self,
        expression: SemanticExprId,
    ) -> Result<bool, SemanticScopeStorageError> {
        let expression = require_expression(self.execution, expression)?;
        if expression.provenance.members.is_empty() {
            return Ok(false);
        }
        let mut has_source = false;
        for member in &expression.provenance.members {
            match &member.origin {
                SemanticValueOrigin::Source { source, .. } => {
                    require_source(self.resources, *source)?;
                    has_source = true;
                }
                SemanticValueOrigin::ProducerSource {
                    function,
                    producer,
                    identity,
                    owner,
                } => {
                    let matches = self
                        .resources
                        .sources
                        .iter()
                        .filter(|source| {
                            source.owner == Some(*owner)
                                && matches!(
                                    source.origin,
                                    SemanticSourceOrigin::ProducerInvocation {
                                        function: candidate_function,
                                        producer: candidate_producer,
                                        identity: candidate_identity,
                                    } if candidate_function == *function
                                        && candidate_producer == *producer
                                        && candidate_identity == *identity
                                )
                        })
                        .count();
                    if matches != 1 {
                        return Err(SemanticScopeStorageError::new(format!(
                            "producer source provenance resolves to {matches} semantic sources"
                        )));
                    }
                    has_source = true;
                }
                SemanticValueOrigin::MaterializationLocal {
                    owner,
                    local,
                    projection,
                } => {
                    let local = self
                        .locals
                        .iter()
                        .find(|candidate| candidate.owner == *owner && candidate.local == *local)
                        .ok_or_else(|| {
                            SemanticScopeStorageError::new(format!(
                                "provenance references missing storage local {owner}:{local}"
                            ))
                        })?;
                    let matches = local
                        .members
                        .iter()
                        .filter(|member| {
                            member.path.starts_with(projection)
                                || projection.starts_with(&member.path)
                        })
                        .collect::<Vec<_>>();
                    if matches.is_empty() {
                        return Ok(false);
                    }
                    for member in matches {
                        match member.target {
                            SemanticStorageLocalMemberTargetV1::Source(source) => {
                                require_source(self.resources, source)?;
                                has_source = true;
                            }
                            SemanticStorageLocalMemberTargetV1::Field(field) => {
                                if !self.field_is_source_only(field)? {
                                    return Ok(false);
                                }
                                has_source = true;
                            }
                            SemanticStorageLocalMemberTargetV1::State(_) => return Ok(false),
                        }
                    }
                }
                SemanticValueOrigin::Runtime | SemanticValueOrigin::State { .. } => {
                    return Ok(false);
                }
            }
        }
        Ok(has_source)
    }
}

fn build_storage_bindings(
    execution: &SemanticExecutionGraphV1,
    resources: &SemanticResourceGraphV1,
    reactive: &SemanticReactiveGraphV1,
    fields: &[SemanticStorageFieldV1],
    owners: &[SemanticStorageOwnerV1],
) -> Result<Vec<SemanticStorageBindingV1>, SemanticScopeStorageError> {
    reactive
        .bindings
        .iter()
        .map(|binding| {
            let owner_ancestry = owner_ancestry(binding.owner, owners)?;
            let target = match binding.target {
                SemanticBindingTargetV1::Field { field } => {
                    let storage = storage_field_for_reactive(fields, field)?;
                    SemanticStorageBindingTargetV1::Value {
                        field: storage.id,
                        row: storage.row,
                    }
                }
                SemanticBindingTargetV1::Source { source } => {
                    require_source(resources, source)?;
                    SemanticStorageBindingTargetV1::Source { source }
                }
                SemanticBindingTargetV1::State { state } => {
                    let state = resources
                        .states
                        .get(state.as_usize())
                        .filter(|candidate| candidate.id == state)
                        .ok_or_else(|| {
                            SemanticScopeStorageError::new(format!(
                                "binding {} references missing state {state}",
                                binding.id
                            ))
                        })?;
                    let field = reactive
                        .fields
                        .iter()
                        .find(|field| field.statement == state.statement)
                        .map(|field| storage_field_for_reactive(fields, field.id))
                        .transpose()?
                        .map(|field| field.id);
                    let row = state
                        .target_list
                        .zip(state.row_scope)
                        .map(|(list, scope)| SemanticRowBinding { list, scope });
                    SemanticStorageBindingTargetV1::State {
                        state: state.id,
                        published: state.published,
                        field,
                        row,
                    }
                }
                SemanticBindingTargetV1::List { list } => {
                    let list = resources
                        .lists
                        .get(list.as_usize())
                        .filter(|candidate| candidate.id == list)
                        .ok_or_else(|| {
                            SemanticScopeStorageError::new(format!(
                                "binding {} references missing list {list}",
                                binding.id
                            ))
                        })?;
                    let reactive_field = reactive
                        .fields
                        .iter()
                        .find(|field| field.statement == list.statement)
                        .ok_or_else(|| {
                            SemanticScopeStorageError::new(format!(
                                "list {list:?} has no reactive field"
                            ))
                        })?;
                    SemanticStorageBindingTargetV1::List {
                        list: list.id,
                        field: storage_field_for_reactive(fields, reactive_field.id)?.id,
                        row: SemanticRowBinding {
                            list: list.id,
                            scope: list.row_scope,
                        },
                    }
                }
            };
            let diagnostic_path = match target {
                SemanticStorageBindingTargetV1::Value { field, .. }
                | SemanticStorageBindingTargetV1::List { field, .. } => fields
                    .get(field.as_usize())
                    .filter(|candidate| candidate.id == field)
                    .map(|field| field.diagnostic_path.clone())
                    .ok_or_else(|| {
                        SemanticScopeStorageError::new(format!(
                            "binding {} references missing storage field {field}",
                            binding.id
                        ))
                    })?,
                SemanticStorageBindingTargetV1::Source { source } => {
                    require_source(resources, source)?.semantic_path.clone()
                }
                SemanticStorageBindingTargetV1::State { state, .. } => {
                    require_state(resources, state)?.path.clone()
                }
            };
            require_expression(execution, binding.producer)?;
            Ok(SemanticStorageBindingV1 {
                binding: binding.id,
                owner_ancestry,
                diagnostic_path,
                target,
            })
        })
        .collect()
}

fn build_storage_sources(
    resources: &SemanticResourceGraphV1,
    reactive: &SemanticReactiveGraphV1,
    owners: &[SemanticStorageOwnerV1],
) -> Result<Vec<SemanticStorageSourceV1>, SemanticScopeStorageError> {
    resources
        .sources
        .iter()
        .map(|source| {
            let matches = reactive
                .bindings
                .iter()
                .filter(|binding| {
                    binding.target == SemanticBindingTargetV1::Source { source: source.id }
                })
                .collect::<Vec<_>>();
            let [binding] = matches.as_slice() else {
                return Err(SemanticScopeStorageError::new(format!(
                    "semantic source {} resolves to {} exact reactive bindings",
                    source.id,
                    matches.len()
                )));
            };
            Ok(SemanticStorageSourceV1 {
                source: source.id,
                owner: source.owner,
                owner_ancestry: owner_ancestry(source.owner, owners)?,
                origin: source.origin,
                binding: binding.id,
            })
        })
        .collect()
}

fn build_row_values(
    execution: &SemanticExecutionGraphV1,
    locals: &[SemanticStorageLocalV1],
) -> Result<Vec<SemanticStorageRowValueV1>, SemanticScopeStorageError> {
    let local_rows = locals
        .iter()
        .map(|local| ((local.owner, local.local), local.row))
        .collect::<BTreeMap<_, _>>();
    let mut values = BTreeSet::new();
    for expression in &execution.expressions {
        for member in &expression.provenance.members {
            let SemanticValueOrigin::MaterializationLocal {
                owner,
                local,
                projection,
            } = &member.origin
            else {
                continue;
            };
            let Some(row) = local_rows.get(&(*owner, *local)).copied().flatten() else {
                continue;
            };
            let mut path = member.path.clone();
            path.extend(projection.iter().cloned());
            values.insert(SemanticStorageRowValueV1 {
                expression: expression.id,
                projection: path,
                row,
            });
        }
    }
    Ok(values.into_iter().collect())
}

fn build_row_source_projections(
    execution: &SemanticExecutionGraphV1,
    resources: &SemanticResourceGraphV1,
    locals: &[SemanticStorageLocalV1],
) -> Result<Vec<SemanticStorageRowSourceProjectionV1>, SemanticScopeStorageError> {
    let mut projections = BTreeMap::<(SemanticRowBinding, Vec<String>), SemanticSourceId>::new();
    for local in locals {
        let Some(row) = local.row else {
            continue;
        };
        for member in &local.members {
            let SemanticStorageLocalMemberTargetV1::Source(source) = member.target else {
                continue;
            };
            insert_row_source_projection(
                &mut projections,
                row,
                &member.path,
                source,
                "storage local",
            )?;
        }
    }
    for materialization in &execution.materializations {
        if materialization.operation != SemanticContextualOperationKind::Map {
            continue;
        }
        let Some(row) = materialization
            .target_list_id
            .zip(materialization.target_scope_id)
            .map(|(list, scope)| SemanticRowBinding { list, scope })
        else {
            continue;
        };
        let body = require_expression(execution, materialization.body)?;
        for member in &body.provenance.members {
            match member.origin {
                SemanticValueOrigin::Source { source, .. } => {
                    require_source(resources, source)?;
                    if !member.path.is_empty() {
                        insert_row_source_projection(
                            &mut projections,
                            row,
                            &member.path,
                            source,
                            "map body",
                        )?;
                    }
                }
                SemanticValueOrigin::Runtime
                | SemanticValueOrigin::ProducerSource { .. }
                | SemanticValueOrigin::State { .. }
                | SemanticValueOrigin::MaterializationLocal { .. } => {}
            }
        }
    }
    Ok(projections
        .into_iter()
        .map(|((row, path), source)| SemanticStorageRowSourceProjectionV1 { row, path, source })
        .collect())
}

fn insert_row_source_projection(
    projections: &mut BTreeMap<(SemanticRowBinding, Vec<String>), SemanticSourceId>,
    row: SemanticRowBinding,
    path: &[String],
    source: SemanticSourceId,
    context: &str,
) -> Result<(), SemanticScopeStorageError> {
    if path.is_empty() {
        return Err(SemanticScopeStorageError::new(format!(
            "{context} has an empty row source projection"
        )));
    }
    if let Some(existing) = projections.insert((row, path.to_vec()), source)
        && existing != source
    {
        return Err(SemanticScopeStorageError::new(format!(
            "{context} row {}/{} projection `{}` resolves to both source {existing} and {source}",
            row.list,
            row.scope,
            path.join(".")
        )));
    }
    Ok(())
}

fn build_external_references(
    execution: &SemanticExecutionGraphV1,
    reactive: &SemanticReactiveGraphV1,
) -> Result<Vec<SemanticStorageExternalReferenceV1>, SemanticScopeStorageError> {
    let mut raw = Vec::new();
    for read in &reactive.reads {
        let SemanticReadTargetV1::External {
            canonical_path,
            external_identity,
        } = &read.target
        else {
            continue;
        };
        raw.push((
            read.expression,
            SemanticStorageExternalReferenceKindV1::Read {
                read: read.id,
                expression: read.expression,
            },
            canonical_path.clone(),
            *external_identity,
        ));
    }
    for use_site in &reactive.dependency_uses {
        let crate::SemanticDependencyTargetV1::ExternalCall { call, expression } = use_site.target
        else {
            continue;
        };
        let call = execution
            .calls
            .get(call.as_usize())
            .filter(|candidate| candidate.id == call)
            .ok_or_else(|| {
                SemanticScopeStorageError::new(format!(
                    "external dependency references missing call {call}"
                ))
            })?;
        raw.push((
            expression,
            SemanticStorageExternalReferenceKindV1::Call {
                call: call.id,
                expression,
            },
            call.function.clone(),
            call.external_identity,
        ));
    }
    raw.sort_by_key(|(expression, kind, path, identity)| {
        (
            *expression,
            match kind {
                SemanticStorageExternalReferenceKindV1::Read { .. } => 0_u8,
                SemanticStorageExternalReferenceKindV1::Call { .. } => 1,
            },
            path.clone(),
            *identity,
        )
    });
    raw.dedup();
    Ok(raw
        .into_iter()
        .enumerate()
        .map(|(index, (_, kind, canonical_path, external_identity))| {
            SemanticStorageExternalReferenceV1 {
                id: SemanticStorageExternalReferenceId(index),
                kind,
                canonical_path,
                external_identity,
                bundle_ready: external_identity.is_some(),
            }
        })
        .collect())
}

fn build_producer_result_fields(
    reactive: &SemanticReactiveGraphV1,
    fields: &[SemanticStorageFieldV1],
    bindings: &[SemanticStorageBindingV1],
) -> Result<Vec<SemanticProducerResultStorageV1>, SemanticScopeStorageError> {
    reactive
        .producer_instances
        .iter()
        .map(|producer| {
            let reactive_matches = reactive
                .fields
                .iter()
                .filter(|field| {
                    field.statement == producer.result_statement
                        && field.producer == producer.root_expression
                })
                .collect::<Vec<_>>();
            let [reactive_field] = reactive_matches.as_slice() else {
                return Err(SemanticScopeStorageError::new(format!(
                    "producer identity {} has {} exact reactive result fields",
                    hex_identity(&producer.identity),
                    reactive_matches.len()
                )));
            };
            let storage_field = storage_field_for_reactive(fields, reactive_field.id)?;
            let binding_matches = reactive
                .bindings
                .iter()
                .filter(|binding| {
                    binding.statement == producer.result_statement
                        && binding.producer == producer.root_expression
                        && binding.value == producer.root_value
                })
                .collect::<Vec<_>>();
            let [binding] = binding_matches.as_slice() else {
                return Err(SemanticScopeStorageError::new(format!(
                    "producer identity {} has {} exact reactive result bindings",
                    hex_identity(&producer.identity),
                    binding_matches.len()
                )));
            };
            if !bindings.iter().any(|storage| storage.binding == binding.id) {
                return Err(SemanticScopeStorageError::new(format!(
                    "producer identity {} result binding {} is absent from storage topology",
                    hex_identity(&producer.identity),
                    binding.id
                )));
            }
            Ok(SemanticProducerResultStorageV1 {
                identity: producer.identity,
                binding: binding.id,
                reactive_field: reactive_field.id,
                storage_field: storage_field.id,
            })
        })
        .collect()
}

fn build_named_value_storage(
    checked: &CheckedProgram,
    execution: &SemanticExecutionGraphV1,
    resources: &SemanticResourceGraphV1,
    reactive: &SemanticReactiveGraphV1,
    lowering: &SemanticLoweringContractV1,
    fields: &[SemanticStorageFieldV1],
    bindings: &[SemanticStorageBindingV1],
) -> Result<Vec<SemanticNamedValueStorageV1>, SemanticScopeStorageError> {
    let mut rows = Vec::new();
    let mut next_projection_id = 0;
    for named_value in &lowering.metadata.named_value_types {
        for (origin_ordinal, origin) in named_value.origins.iter().enumerate() {
            let targets =
                named_value_targets(execution, resources, reactive, fields, bindings, origin)?;
            if targets.is_empty() {
                return Err(SemanticScopeStorageError::new(format!(
                    "named value {} origin {origin_ordinal} has no exact semantic storage target",
                    named_value.id
                )));
            }
            for (target_ordinal, target) in targets.into_iter().enumerate() {
                let root_flow =
                    named_value_target_flow_type(execution, resources, reactive, fields, &target)?;
                let projection = build_named_value_projection(
                    execution,
                    fields,
                    &target,
                    &root_flow.ty,
                    &origin.checked.projection,
                    &mut next_projection_id,
                )?;
                let projected_storage_type = projection
                    .last()
                    .map(|step| &step.output_type)
                    .unwrap_or(&root_flow.ty);
                let contract_flow = named_value_origin_contract_flow(
                    checked,
                    origin,
                    &target,
                    &root_flow,
                    projected_storage_type,
                )
                .map_err(|error| {
                    SemanticScopeStorageError::new(format!(
                        "named value {} `{}` origin {origin_ordinal} {:?} semantic statements {:?} expressions {:?} bindings {:?} sources {:?} states {:?} lists {:?} target {target:?}: {error}",
                        named_value.id,
                        named_value.diagnostic_path,
                        origin.checked,
                        origin.statements,
                        origin.expressions,
                        origin.bindings,
                        origin.sources,
                        origin.states,
                        origin.lists,
                    ))
                })?;
                let representation = derive_storage_representation(
                    projected_storage_type,
                    &contract_flow.ty,
                )
                .map_err(|error| {
                    SemanticScopeStorageError::new(format!(
                        "named value {} `{}` origin {origin_ordinal} {:?} semantic statements {:?} expressions {:?} bindings {:?} sources {:?} states {:?} lists {:?} target {target:?}: {error}",
                        named_value.id,
                        named_value.diagnostic_path,
                        origin.checked,
                        origin.statements,
                        origin.expressions,
                        origin.bindings,
                        origin.sources,
                        origin.states,
                        origin.lists,
                    ))
                })?;
                rows.push(SemanticNamedValueStorageV1 {
                    named_value: named_value.id,
                    origin_ordinal,
                    target_ordinal,
                    target,
                    projection,
                    representation,
                    flow_type: contract_flow,
                });
            }
        }
    }
    Ok(rows)
}

fn named_value_targets(
    execution: &SemanticExecutionGraphV1,
    resources: &SemanticResourceGraphV1,
    reactive: &SemanticReactiveGraphV1,
    fields: &[SemanticStorageFieldV1],
    bindings: &[SemanticStorageBindingV1],
    origin: &crate::SemanticNamedValueTypeOriginV1,
) -> Result<Vec<SemanticNamedValueStorageTargetV1>, SemanticScopeStorageError> {
    let mut targets = BTreeSet::new();
    for binding in &origin.bindings {
        let storage = bindings
            .iter()
            .find(|candidate| candidate.binding == *binding)
            .ok_or_else(|| {
                SemanticScopeStorageError::new(format!(
                    "named-value origin references missing storage binding {binding}"
                ))
            })?;
        targets.insert(named_value_target_from_binding(storage));
    }

    if targets.is_empty() {
        for source in &origin.sources {
            let matches = bindings
                .iter()
                .filter_map(|binding| match binding.target {
                    SemanticStorageBindingTargetV1::Source { source: candidate }
                        if candidate == *source =>
                    {
                        Some(SemanticNamedValueStorageTargetV1::Source {
                            binding: binding.binding,
                            source: *source,
                        })
                    }
                    _ => None,
                })
                .collect::<Vec<_>>();
            if matches.is_empty() {
                return Err(SemanticScopeStorageError::new(format!(
                    "named-value source {source} has no exact storage binding"
                )));
            }
            targets.extend(matches);
        }
        for state in &origin.states {
            let matches = bindings
                .iter()
                .filter_map(|binding| match binding.target {
                    SemanticStorageBindingTargetV1::State {
                        state: candidate,
                        field,
                        ..
                    } if candidate == *state => Some(SemanticNamedValueStorageTargetV1::State {
                        binding: binding.binding,
                        state: *state,
                        field,
                    }),
                    _ => None,
                })
                .collect::<Vec<_>>();
            if matches.is_empty() {
                return Err(SemanticScopeStorageError::new(format!(
                    "named-value state {state} has no exact storage binding"
                )));
            }
            targets.extend(matches);
        }
        for list in &origin.lists {
            let matches = bindings
                .iter()
                .filter_map(|binding| match binding.target {
                    SemanticStorageBindingTargetV1::List {
                        list: candidate,
                        field,
                        row,
                    } if candidate == *list => Some(SemanticNamedValueStorageTargetV1::List {
                        binding: binding.binding,
                        list: *list,
                        field,
                        row,
                    }),
                    _ => None,
                })
                .collect::<Vec<_>>();
            if matches.is_empty() {
                return Err(SemanticScopeStorageError::new(format!(
                    "named-value list {list} has no exact storage binding"
                )));
            }
            targets.extend(matches);
        }
    }

    if targets.is_empty() {
        let statement_set = origin.statements.iter().copied().collect::<BTreeSet<_>>();
        let expression_set = origin.expressions.iter().copied().collect::<BTreeSet<_>>();
        for field in fields.iter().filter(|field| {
            (field.reactive_field.is_some()
                && field
                    .statement
                    .is_some_and(|statement| statement_set.contains(&statement)))
                || field
                    .producer
                    .is_some_and(|producer| expression_set.contains(&producer))
        }) {
            targets.insert(SemanticNamedValueStorageTargetV1::Field {
                binding: None,
                field: field.id,
            });
        }
    }

    if targets.is_empty() {
        for expression in &origin.expressions {
            let semantic = require_expression(execution, *expression)?;
            let matching_fields = fields
                .iter()
                .filter(|field| field.producer == Some(*expression))
                .map(|field| field.id)
                .collect::<Vec<_>>();
            let field = match matching_fields.as_slice() {
                [] => None,
                [field] => Some(*field),
                _ => {
                    return Err(SemanticScopeStorageError::new(format!(
                        "named-value expression {expression} resolves to {} storage fields without an exact binding",
                        matching_fields.len()
                    )));
                }
            };
            targets.insert(SemanticNamedValueStorageTargetV1::Value {
                expression: *expression,
                value: semantic.value_id,
                field,
            });
        }
    }

    if targets.is_empty()
        && !origin.statements.is_empty()
        && origin.expressions.is_empty()
        && origin.bindings.is_empty()
        && origin.sources.is_empty()
        && origin.states.is_empty()
        && origin.lists.is_empty()
    {
        targets.insert(SemanticNamedValueStorageTargetV1::DiagnosticOnly {
            reason: SemanticNamedValueDiagnosticOnlyReasonV1::NonExecutableStructuralContainer,
        });
    }

    // Exact resource identities above are validated even when an executable
    // binding was already present; this prevents stale lowering metadata from
    // silently selecting a different resource instance.
    for source in &origin.sources {
        require_source(resources, *source)?;
    }
    for state in &origin.states {
        require_state(resources, *state)?;
    }
    for list in &origin.lists {
        resources
            .lists
            .get(list.as_usize())
            .filter(|candidate| candidate.id == *list)
            .ok_or_else(|| {
                SemanticScopeStorageError::new(format!(
                    "named-value origin references missing list {list}"
                ))
            })?;
    }
    for binding in &origin.bindings {
        reactive
            .bindings
            .get(binding.as_usize())
            .filter(|candidate| candidate.id == *binding)
            .ok_or_else(|| {
                SemanticScopeStorageError::new(format!(
                    "named-value origin references missing reactive binding {binding}"
                ))
            })?;
    }
    Ok(targets.into_iter().collect())
}

fn named_value_target_from_binding(
    binding: &SemanticStorageBindingV1,
) -> SemanticNamedValueStorageTargetV1 {
    match binding.target {
        SemanticStorageBindingTargetV1::Value { field, .. } => {
            SemanticNamedValueStorageTargetV1::Field {
                binding: Some(binding.binding),
                field,
            }
        }
        SemanticStorageBindingTargetV1::Source { source } => {
            SemanticNamedValueStorageTargetV1::Source {
                binding: binding.binding,
                source,
            }
        }
        SemanticStorageBindingTargetV1::State { state, field, .. } => {
            SemanticNamedValueStorageTargetV1::State {
                binding: binding.binding,
                state,
                field,
            }
        }
        SemanticStorageBindingTargetV1::List {
            list, field, row, ..
        } => SemanticNamedValueStorageTargetV1::List {
            binding: binding.binding,
            list,
            field,
            row,
        },
    }
}

fn named_value_target_flow_type(
    execution: &SemanticExecutionGraphV1,
    resources: &SemanticResourceGraphV1,
    reactive: &SemanticReactiveGraphV1,
    fields: &[SemanticStorageFieldV1],
    target: &SemanticNamedValueStorageTargetV1,
) -> Result<FlowType, SemanticScopeStorageError> {
    match target {
        SemanticNamedValueStorageTargetV1::Field { field, .. } => {
            Ok(require_storage_field(fields, *field)?.flow_type.clone())
        }
        SemanticNamedValueStorageTargetV1::Source { binding, source } => {
            require_source(resources, *source)?;
            let binding = require_reactive_binding(reactive, *binding)?;
            if binding.target != (SemanticBindingTargetV1::Source { source: *source }) {
                return Err(SemanticScopeStorageError::new(format!(
                    "named-value source {source} does not match reactive binding {}",
                    binding.id
                )));
            }
            Ok(binding.flow_type.clone())
        }
        SemanticNamedValueStorageTargetV1::State { binding, state, .. } => {
            require_reactive_binding(reactive, *binding)?;
            Ok(require_state(resources, *state)?.flow_type.clone())
        }
        SemanticNamedValueStorageTargetV1::List { binding, .. } => {
            Ok(require_reactive_binding(reactive, *binding)?
                .flow_type
                .clone())
        }
        SemanticNamedValueStorageTargetV1::Value {
            expression, value, ..
        } => {
            let expression = require_expression(execution, *expression)?;
            if expression.value_id != *value {
                return Err(SemanticScopeStorageError::new(format!(
                    "named-value target expression {} does not own value {value}",
                    expression.id
                )));
            }
            Ok(expression.flow_type.clone())
        }
        SemanticNamedValueStorageTargetV1::DiagnosticOnly { .. } => Ok(FlowType {
            mode: FlowMode::Continuous,
            ty: Type::Unknown,
        }),
    }
}

fn build_named_value_projection(
    execution: &SemanticExecutionGraphV1,
    fields: &[SemanticStorageFieldV1],
    target: &SemanticNamedValueStorageTargetV1,
    root_type: &Type,
    selectors: &[String],
    next_projection_id: &mut usize,
) -> Result<Vec<SemanticStorageProjectionStepV1>, SemanticScopeStorageError> {
    if matches!(
        target,
        SemanticNamedValueStorageTargetV1::DiagnosticOnly { .. }
    ) {
        if !selectors.is_empty() {
            return Err(SemanticScopeStorageError::new(
                "diagnostic-only named value cannot carry an executable projection",
            ));
        }
        return Ok(Vec::new());
    }
    let mut input_type = root_type.clone();
    let mut parent_field = named_value_target_storage_field(target);
    let mut steps = Vec::with_capacity(selectors.len());
    for (ordinal, selector) in selectors.iter().enumerate() {
        let Type::Object(shape) = &input_type else {
            return Err(SemanticScopeStorageError::new(format!(
                "named-value projection `{selector}` requires an object input, got {input_type:?}"
            )));
        };
        let output_type = shape.fields.get(selector).cloned().ok_or_else(|| {
            SemanticScopeStorageError::new(format!(
                "named-value projection references missing object field `{selector}`"
            ))
        })?;
        let field_order = canonical_object_field_order(shape);
        let field_ordinal = field_order
            .iter()
            .position(|field| field == selector)
            .ok_or_else(|| {
                SemanticScopeStorageError::new(format!(
                    "named-value projection field `{selector}` has no structural ordinal"
                ))
            })?;
        let storage_field = parent_field
            .map(|parent| {
                let matches = fields
                    .iter()
                    .filter(|field| field.parent == Some(parent) && field.name == *selector)
                    .map(|field| field.id)
                    .collect::<Vec<_>>();
                match matches.as_slice() {
                    [] => Ok(None),
                    [field] => Ok(Some(*field)),
                    _ => Err(SemanticScopeStorageError::new(format!(
                        "storage field {parent} projection `{selector}` resolves to {} children",
                        matches.len()
                    ))),
                }
            })
            .transpose()?
            .flatten();
        let (expression, value) = match storage_field
            .map(|field| require_storage_field(fields, field))
            .transpose()?
            .and_then(|field| field.producer)
        {
            Some(expression) => (
                Some(expression),
                Some(require_expression(execution, expression)?.value_id),
            ),
            None => (None, None),
        };
        steps.push(SemanticStorageProjectionStepV1 {
            id: SemanticStorageProjectionId(*next_projection_id),
            ordinal,
            selector: selector.clone(),
            field_ordinal,
            input_type: input_type.clone(),
            output_type: output_type.clone(),
            storage_field,
            expression,
            value,
        });
        *next_projection_id += 1;
        input_type = output_type;
        parent_field = storage_field;
    }
    Ok(steps)
}

fn named_value_origin_contract_flow(
    checked: &CheckedProgram,
    origin: &crate::SemanticNamedValueTypeOriginV1,
    target: &SemanticNamedValueStorageTargetV1,
    root_flow: &FlowType,
    projected_storage_type: &Type,
) -> Result<FlowType, SemanticScopeStorageError> {
    if matches!(target, SemanticNamedValueStorageTargetV1::Source { .. }) {
        // Source payload shape/type authority is the exact CheckedSourceId ->
        // SemanticSourceId table in A-C. A named source denotes that authority,
        // not the pre-host-refinement syntax placeholder on its declaration.
        return Ok(FlowType {
            mode: root_flow.mode,
            ty: projected_storage_type.clone(),
        });
    }
    if named_value_origin_is_structural_container_placeholder(checked, origin)? {
        // A checked empty record attached to a statement with structural
        // children is the parser/typechecker anchor for the assembled
        // container, not a competing zero-field runtime representation. The
        // exact semantic statement/target join above owns its concrete shape.
        return Ok(FlowType {
            mode: root_flow.mode,
            ty: projected_storage_type.clone(),
        });
    }
    let checked_flow = if let Some(value) = origin.checked.value {
        Some(
            checked
                .expressions
                .iter()
                .find(|expression| expression.id == value)
                .ok_or_else(|| {
                    SemanticScopeStorageError::new(format!(
                        "named-value origin references missing checked expression {}",
                        value.0
                    ))
                })?
                .flow_type
                .clone(),
        )
    } else if let Some(declaration) = origin.checked.declaration {
        Some(
            checked
                .declarations
                .iter()
                .find(|candidate| candidate.id == declaration)
                .ok_or_else(|| {
                    SemanticScopeStorageError::new(format!(
                        "named-value origin references missing checked declaration {}",
                        declaration.0
                    ))
                })?
                .flow_type
                .clone(),
        )
    } else {
        None
    };

    let Some(checked_flow) = checked_flow else {
        if matches!(
            target,
            SemanticNamedValueStorageTargetV1::DiagnosticOnly { .. }
        ) {
            return Ok(root_flow.clone());
        }
        return Err(SemanticScopeStorageError::new(
            "executable named-value origin has no exact checked flow contract",
        ));
    };
    let projected_checked_type =
        project_named_value_contract_type(&checked_flow.ty, &origin.checked.projection)?;
    // This call is the exact admissibility gate. The only non-identical
    // representation admitted is the explicit fixed-BYTES refinement encoded
    // into the resulting D row.
    derive_storage_representation(projected_storage_type, &projected_checked_type)?;
    Ok(FlowType {
        mode: checked_flow.mode,
        ty: projected_checked_type,
    })
}

fn named_value_origin_is_structural_container_placeholder(
    checked: &CheckedProgram,
    origin: &crate::SemanticNamedValueTypeOriginV1,
) -> Result<bool, SemanticScopeStorageError> {
    let (Some(statement_id), Some(value_id)) = (origin.checked.statement, origin.checked.value)
    else {
        return Ok(false);
    };
    let statement = checked
        .statements
        .iter()
        .find(|statement| statement.id == statement_id)
        .ok_or_else(|| {
            SemanticScopeStorageError::new(format!(
                "named-value origin references missing checked statement {}",
                statement_id.0
            ))
        })?;
    if statement.value != Some(value_id) || statement.children.is_empty() {
        return Ok(false);
    }
    let expression = checked
        .expressions
        .iter()
        .find(|expression| expression.id == value_id)
        .ok_or_else(|| {
            SemanticScopeStorageError::new(format!(
                "named-value structural container references missing checked expression {}",
                value_id.0
            ))
        })?;
    Ok(matches!(
        &expression.kind,
        boon_typecheck::CheckedExpressionKind::Object { fields }
            | boon_typecheck::CheckedExpressionKind::Record { fields }
            if fields.is_empty()
    ) || matches!(
        &expression.kind,
        boon_typecheck::CheckedExpressionKind::List { items, .. } if items.is_empty()
    ))
}

fn project_named_value_contract_type(
    ty: &Type,
    projection: &[String],
) -> Result<Type, SemanticScopeStorageError> {
    fn project_one(ty: &Type, selector: &str) -> Result<Type, SemanticScopeStorageError> {
        match ty {
            Type::Object(shape) => shape.fields.get(selector).cloned().ok_or_else(|| {
                SemanticScopeStorageError::new(format!(
                    "named-value checked projection references missing field `{selector}`"
                ))
            }),
            Type::List(item) => {
                project_one(item, selector).map(|field| Type::List(Box::new(field)))
            }
            _ => Err(SemanticScopeStorageError::new(format!(
                "named-value checked projection `{selector}` requires an object or list item, got {ty:?}"
            ))),
        }
    }

    projection.iter().try_fold(ty.clone(), |current, selector| {
        project_one(&current, selector)
    })
}

fn derive_storage_representation(
    storage: &Type,
    contract: &Type,
) -> Result<SemanticStorageRepresentationV1, SemanticScopeStorageError> {
    fn visit(
        storage: &Type,
        contract: &Type,
        path: &mut Vec<SemanticStorageTypePathSegmentV1>,
        refinements: &mut Vec<SemanticStorageFixedBytesRefinementV1>,
    ) -> Result<(), SemanticScopeStorageError> {
        if storage == contract {
            return Ok(());
        }
        match (storage, contract) {
            (
                Type::Bytes(boon_typecheck::BytesType::Dynamic),
                Type::Bytes(boon_typecheck::BytesType::Fixed(fixed_len)),
            ) => {
                refinements.push(SemanticStorageFixedBytesRefinementV1 {
                    path: path.clone(),
                    fixed_len: *fixed_len,
                });
                Ok(())
            }
            (Type::List(storage), Type::List(contract)) => {
                path.push(SemanticStorageTypePathSegmentV1::ListItem);
                let result = visit(storage, contract, path, refinements);
                path.pop();
                result
            }
            (Type::Object(storage), Type::Object(contract))
                if storage.open == contract.open
                    && storage.fields.len() == contract.fields.len()
                    && storage.fields.keys().eq(contract.fields.keys()) =>
            {
                let field_order = canonical_object_field_order(storage);
                for (field_ordinal, selector) in field_order.into_iter().enumerate() {
                    let storage_field = storage.fields.get(&selector).ok_or_else(|| {
                        SemanticScopeStorageError::new(format!(
                            "storage representation lost object field `{selector}`"
                        ))
                    })?;
                    let contract_field = contract.fields.get(&selector).ok_or_else(|| {
                        SemanticScopeStorageError::new(format!(
                            "storage representation contract lost object field `{selector}`"
                        ))
                    })?;
                    path.push(SemanticStorageTypePathSegmentV1::ObjectField {
                        selector,
                        field_ordinal,
                    });
                    visit(storage_field, contract_field, path, refinements)?;
                    path.pop();
                }
                Ok(())
            }
            _ => Err(SemanticScopeStorageError::new(format!(
                "named-value storage representation {storage:?} does not exactly preserve contract {contract:?}"
            ))),
        }
    }

    let mut refinements = Vec::new();
    visit(storage, contract, &mut Vec::new(), &mut refinements)?;
    if refinements.is_empty() {
        Ok(SemanticStorageRepresentationV1::Exact)
    } else {
        Ok(SemanticStorageRepresentationV1::CheckedFixedBytes { refinements })
    }
}

fn canonical_object_field_order(shape: &boon_typecheck::ObjectShape) -> Vec<String> {
    let mut order = Vec::new();
    let mut seen = BTreeSet::new();
    for field in shape.field_order.iter().chain(shape.fields.keys()) {
        if shape.fields.contains_key(field) && seen.insert(field.clone()) {
            order.push(field.clone());
        }
    }
    order
}

fn named_value_target_storage_field(
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

fn require_reactive_binding(
    reactive: &SemanticReactiveGraphV1,
    id: SemanticBindingId,
) -> Result<&crate::SemanticBindingV1, SemanticScopeStorageError> {
    reactive
        .bindings
        .get(id.as_usize())
        .filter(|binding| binding.id == id)
        .ok_or_else(|| {
            SemanticScopeStorageError::new(format!(
                "storage topology references missing reactive binding {id}"
            ))
        })
}

fn validate_storage_shape(
    graph: &SemanticScopeStorageGraphV1,
    checked: &CheckedProgram,
    execution: &SemanticExecutionGraphV1,
    resources: &SemanticResourceGraphV1,
    reactive: &SemanticReactiveGraphV1,
    lowering: &SemanticLoweringContractV1,
) -> Result<(), SemanticScopeStorageError> {
    if graph.schema != SEMANTIC_SCOPE_STORAGE_GRAPH_SCHEMA_V1
        || graph.source_bundle_digest_v1 != checked.source_bundle_digest_v1
    {
        return Err(SemanticScopeStorageError::new(
            "semantic scope-storage schema or source digest differs",
        ));
    }
    for (index, field) in graph.fields.iter().enumerate() {
        if field.id != SemanticStorageFieldId(index) {
            return Err(SemanticScopeStorageError::new(
                "semantic storage field IDs are not dense",
            ));
        }
        if let Some(parent) = field.parent {
            require_storage_field(&graph.fields, parent)?;
        }
        if let Some(reactive_field) = field.reactive_field {
            let source = reactive
                .fields
                .get(reactive_field.as_usize())
                .filter(|candidate| candidate.id == reactive_field)
                .ok_or_else(|| {
                    SemanticScopeStorageError::new(format!(
                        "storage field {} references missing reactive field {reactive_field}",
                        field.id
                    ))
                })?;
            if field.statement != Some(source.statement)
                || field.producer != Some(source.producer)
                || field.flow_type != source.flow_type
            {
                return Err(SemanticScopeStorageError::new(format!(
                    "storage field {} differs from reactive field {reactive_field}",
                    field.id
                )));
            }
        }
    }
    for reactive_field in &reactive.fields {
        storage_field_for_reactive(&graph.fields, reactive_field.id)?;
    }
    for binding in &reactive.bindings {
        let matches = graph
            .bindings
            .iter()
            .filter(|candidate| candidate.binding == binding.id)
            .collect::<Vec<_>>();
        let [storage] = matches.as_slice() else {
            return Err(SemanticScopeStorageError::new(format!(
                "reactive binding {} resolves to {} storage bindings",
                binding.id,
                matches.len()
            )));
        };
        let expected_ancestry = owner_ancestry(binding.owner, &graph.owners)?;
        if storage.owner_ancestry != expected_ancestry {
            return Err(SemanticScopeStorageError::new(format!(
                "storage binding {} owner ancestry is not canonical root-to-leaf",
                binding.id
            )));
        }
    }
    for source in &resources.sources {
        let matches = graph
            .sources
            .iter()
            .filter(|candidate| candidate.source == source.id)
            .collect::<Vec<_>>();
        let [storage] = matches.as_slice() else {
            return Err(SemanticScopeStorageError::new(format!(
                "resource source {} resolves to {} storage sources",
                source.id,
                matches.len()
            )));
        };
        let expected_ancestry = owner_ancestry(source.owner, &graph.owners)?;
        if storage.owner_ancestry != expected_ancestry {
            return Err(SemanticScopeStorageError::new(format!(
                "storage source {} owner ancestry is not canonical root-to-leaf",
                source.id
            )));
        }
    }
    for owner in &graph.owners {
        if owner.authority_row != owner.target_row.or(owner.source_row) {
            return Err(SemanticScopeStorageError::new(format!(
                "storage owner {} has inconsistent authority row",
                owner.id
            )));
        }
    }
    for local in &graph.locals {
        require_expression(execution, local.source)?;
        for capture in &local.captures {
            let field = require_storage_field(&graph.fields, capture.field)?;
            if field.role != SemanticStorageFieldRoleV1::Capture {
                return Err(SemanticScopeStorageError::new(format!(
                    "local capture {} targets non-capture field {}",
                    capture.id, capture.field
                )));
            }
        }
    }
    validate_named_value_storage_shape(graph, execution, resources, reactive, lowering)?;
    Ok(())
}

fn validate_named_value_storage_shape(
    graph: &SemanticScopeStorageGraphV1,
    execution: &SemanticExecutionGraphV1,
    resources: &SemanticResourceGraphV1,
    reactive: &SemanticReactiveGraphV1,
    lowering: &SemanticLoweringContractV1,
) -> Result<(), SemanticScopeStorageError> {
    let mut row_cursor = 0;
    let mut projection_cursor = 0;
    for named_value in &lowering.metadata.named_value_types {
        for (origin_ordinal, origin) in named_value.origins.iter().enumerate() {
            let start = row_cursor;
            while let Some(row) = graph.named_values.get(row_cursor)
                && row.named_value == named_value.id
                && row.origin_ordinal == origin_ordinal
            {
                if row.target_ordinal != row_cursor - start {
                    return Err(SemanticScopeStorageError::new(format!(
                        "named value {} origin {origin_ordinal} target ordinals are not dense",
                        named_value.id
                    )));
                }
                let root_flow = named_value_target_flow_type(
                    execution,
                    resources,
                    reactive,
                    &graph.fields,
                    &row.target,
                )?;
                if matches!(
                    row.target,
                    SemanticNamedValueStorageTargetV1::DiagnosticOnly { .. }
                ) {
                    if !row.projection.is_empty()
                        || row.representation != SemanticStorageRepresentationV1::Exact
                        || !origin.bindings.is_empty()
                        || !origin.expressions.is_empty()
                        || !origin.sources.is_empty()
                        || !origin.states.is_empty()
                        || !origin.lists.is_empty()
                    {
                        return Err(SemanticScopeStorageError::new(format!(
                            "named value {} origin {origin_ordinal} is incorrectly classified diagnostic-only",
                            named_value.id
                        )));
                    }
                } else {
                    let final_type = row
                        .projection
                        .last()
                        .map(|step| &step.output_type)
                        .unwrap_or(&root_flow.ty);
                    let expected_representation =
                        derive_storage_representation(final_type, &row.flow_type.ty)?;
                    if row.representation != expected_representation {
                        return Err(SemanticScopeStorageError::new(format!(
                            "named value {} origin {origin_ordinal} storage representation differs from its exact checked contract",
                            named_value.id
                        )));
                    }
                }
                let mut previous_output = &root_flow.ty;
                for (ordinal, step) in row.projection.iter().enumerate() {
                    if step.id != SemanticStorageProjectionId(projection_cursor)
                        || step.ordinal != ordinal
                        || &step.input_type != previous_output
                    {
                        return Err(SemanticScopeStorageError::new(format!(
                            "named value {} origin {origin_ordinal} projection steps are not dense or structurally linked",
                            named_value.id
                        )));
                    }
                    if let Some(field) = step.storage_field {
                        let field = require_storage_field(&graph.fields, field)?;
                        if field.name != step.selector {
                            return Err(SemanticScopeStorageError::new(format!(
                                "named value {} origin {origin_ordinal} projection field {} does not match selector `{}`",
                                named_value.id, field.id, step.selector
                            )));
                        }
                    }
                    match (step.expression, step.value) {
                        (Some(expression), Some(value)) => {
                            if require_expression(execution, expression)?.value_id != value {
                                return Err(SemanticScopeStorageError::new(format!(
                                    "named value {} origin {origin_ordinal} projection expression/value join differs",
                                    named_value.id
                                )));
                            }
                        }
                        (None, None) => {}
                        _ => {
                            return Err(SemanticScopeStorageError::new(format!(
                                "named value {} origin {origin_ordinal} projection has a partial expression/value join",
                                named_value.id
                            )));
                        }
                    }
                    previous_output = &step.output_type;
                    projection_cursor += 1;
                }
                row_cursor += 1;
            }
            if row_cursor == start {
                return Err(SemanticScopeStorageError::new(format!(
                    "named value {} origin {origin_ordinal} has no exact semantic storage target",
                    named_value.id
                )));
            }
        }
    }
    if row_cursor != graph.named_values.len() {
        return Err(SemanticScopeStorageError::new(
            "semantic named-value storage rows are not ordered by lowering identity",
        ));
    }
    Ok(())
}

fn storage_field_for_reactive(
    fields: &[SemanticStorageFieldV1],
    reactive: SemanticFieldId,
) -> Result<&SemanticStorageFieldV1, SemanticScopeStorageError> {
    let matches = fields
        .iter()
        .filter(|field| field.reactive_field == Some(reactive))
        .collect::<Vec<_>>();
    let [field] = matches.as_slice() else {
        return Err(SemanticScopeStorageError::new(format!(
            "reactive field {reactive} resolves to {} semantic storage fields",
            matches.len()
        )));
    };
    Ok(*field)
}

fn require_storage_field(
    fields: &[SemanticStorageFieldV1],
    id: SemanticStorageFieldId,
) -> Result<&SemanticStorageFieldV1, SemanticScopeStorageError> {
    fields
        .get(id.as_usize())
        .filter(|field| field.id == id)
        .ok_or_else(|| {
            SemanticScopeStorageError::new(format!(
                "storage topology references missing field {id}"
            ))
        })
}

fn require_statement(
    execution: &SemanticExecutionGraphV1,
    id: SemanticStatementId,
) -> Result<&crate::SemanticStatement, SemanticScopeStorageError> {
    execution
        .statements
        .get(id.as_usize())
        .filter(|statement| statement.id == id)
        .ok_or_else(|| {
            SemanticScopeStorageError::new(format!(
                "storage topology references missing statement {id}"
            ))
        })
}

fn require_expression(
    execution: &SemanticExecutionGraphV1,
    id: SemanticExprId,
) -> Result<&crate::SemanticExpression, SemanticScopeStorageError> {
    execution
        .expressions
        .get(id.as_usize())
        .filter(|expression| expression.id == id)
        .ok_or_else(|| {
            SemanticScopeStorageError::new(format!(
                "storage topology references missing expression {id}"
            ))
        })
}

fn require_source(
    resources: &SemanticResourceGraphV1,
    id: SemanticSourceId,
) -> Result<&crate::SemanticSourceResourceV1, SemanticScopeStorageError> {
    resources
        .sources
        .get(id.as_usize())
        .filter(|source| source.id == id)
        .ok_or_else(|| {
            SemanticScopeStorageError::new(format!(
                "storage topology references missing source {id}"
            ))
        })
}

fn require_state(
    resources: &SemanticResourceGraphV1,
    id: SemanticStateId,
) -> Result<&crate::SemanticStateResourceV1, SemanticScopeStorageError> {
    resources
        .states
        .get(id.as_usize())
        .filter(|state| state.id == id)
        .ok_or_else(|| {
            SemanticScopeStorageError::new(format!(
                "storage topology references missing state {id}"
            ))
        })
}

fn owner_ancestry(
    owner: Option<StaticOwnerId>,
    owners: &[SemanticStorageOwnerV1],
) -> Result<Vec<StaticOwnerId>, SemanticScopeStorageError> {
    let mut ancestry = Vec::new();
    let mut current = owner;
    let mut visited = BTreeSet::new();
    while let Some(id) = current {
        if !visited.insert(id) {
            return Err(SemanticScopeStorageError::new(
                "semantic storage owner ancestry contains a cycle",
            ));
        }
        ancestry.push(id);
        current = owners
            .get(id.as_usize())
            .filter(|owner| owner.id == id)
            .ok_or_else(|| {
                SemanticScopeStorageError::new(format!(
                    "semantic storage references missing owner {id}"
                ))
            })?
            .parent;
    }
    // This is the direct final-erasure contract: ancestry is canonical
    // root-to-leaf, with the concrete owner in the final slot.
    ancestry.reverse();
    Ok(ancestry)
}

fn owner_descends_from(
    candidate: StaticOwnerId,
    ancestor: StaticOwnerId,
    owners: &[SemanticStorageOwnerV1],
) -> Result<bool, SemanticScopeStorageError> {
    Ok(owner_ancestry(Some(candidate), owners)?
        .into_iter()
        .any(|owner| owner == ancestor))
}

fn expression_children(
    execution: &SemanticExecutionGraphV1,
    kind: &SemanticExpressionKind,
) -> Result<Vec<SemanticExprId>, SemanticScopeStorageError> {
    Ok(match kind {
        SemanticExpressionKind::CanonicalRead { .. }
        | SemanticExpressionKind::LocalRead { .. }
        | SemanticExpressionKind::ExternalRead { .. }
        | SemanticExpressionKind::ElementState { .. }
        | SemanticExpressionKind::Drain { .. }
        | SemanticExpressionKind::Text(_)
        | SemanticExpressionKind::Number(_)
        | SemanticExpressionKind::BytesByte(_)
        | SemanticExpressionKind::Bool(_)
        | SemanticExpressionKind::Tag(_)
        | SemanticExpressionKind::Source { .. }
        | SemanticExpressionKind::Delimiter
        | SemanticExpressionKind::MaterializationLocal { .. }
        | SemanticExpressionKind::FunctionParameter { .. } => Vec::new(),
        SemanticExpressionKind::Materialize { materialization } => execution
            .materializations
            .get(materialization.as_usize())
            .filter(|candidate| candidate.id == *materialization)
            .ok_or_else(|| {
                SemanticScopeStorageError::new(format!(
                    "expression references missing materialization {materialization}"
                ))
            })?
            .expression_roots(),
        SemanticExpressionKind::TextTemplate { segments } => segments
            .iter()
            .filter_map(|segment| match segment {
                crate::SemanticTextSegment::Static { .. } => None,
                crate::SemanticTextSegment::Dynamic { value } => Some(*value),
            })
            .collect(),
        SemanticExpressionKind::TaggedObject { fields, .. }
        | SemanticExpressionKind::Object(fields)
        | SemanticExpressionKind::Record(fields) => {
            fields.iter().map(|field| field.value).collect()
        }
        SemanticExpressionKind::Call { arguments, .. } => {
            arguments.iter().map(|argument| argument.value).collect()
        }
        SemanticExpressionKind::Draining { input }
        | SemanticExpressionKind::Project { input, .. } => vec![*input],
        SemanticExpressionKind::Hold {
            initial, updates, ..
        } => std::iter::once(*initial)
            .chain(updates.iter().copied())
            .collect(),
        SemanticExpressionKind::Latest { branches } => branches.clone(),
        SemanticExpressionKind::When { input, arms, .. } => std::iter::once(*input)
            .chain(arms.iter().map(|arm| arm.output))
            .collect(),
        SemanticExpressionKind::Then { input, output } => std::iter::once(*input)
            .chain(output.iter().copied())
            .collect(),
        SemanticExpressionKind::Infix { left, right, .. } => vec![*left, *right],
        SemanticExpressionKind::MatchArm { output, .. } => output.iter().copied().collect(),
        SemanticExpressionKind::Block { bindings, result } => bindings
            .iter()
            .map(|binding| binding.value)
            .chain(std::iter::once(*result))
            .collect(),
        SemanticExpressionKind::List { items, .. }
        | SemanticExpressionKind::Bytes { items, .. } => items.clone(),
    })
}

fn hex_identity(identity: &[u8; 32]) -> String {
    identity.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[derive(Serialize)]
struct StorageDigestPayload<'a> {
    schema: &'a str,
    source_bundle_digest_v1: SourceBundleDigestV1,
    owners: &'a [SemanticStorageOwnerV1],
    locals: &'a [SemanticStorageLocalV1],
    fields: &'a [SemanticStorageFieldV1],
    bindings: &'a [SemanticStorageBindingV1],
    sources: &'a [SemanticStorageSourceV1],
    row_values: &'a [SemanticStorageRowValueV1],
    row_source_projections: &'a [SemanticStorageRowSourceProjectionV1],
    external_references: &'a [SemanticStorageExternalReferenceV1],
    producer_result_fields: &'a [SemanticProducerResultStorageV1],
    named_values: &'a [SemanticNamedValueStorageV1],
}

fn scope_storage_digest(
    graph: &SemanticScopeStorageGraphV1,
) -> Result<SemanticScopeStorageGraphDigestV1, SemanticScopeStorageError> {
    let payload = StorageDigestPayload {
        schema: &graph.schema,
        source_bundle_digest_v1: graph.source_bundle_digest_v1,
        owners: &graph.owners,
        locals: &graph.locals,
        fields: &graph.fields,
        bindings: &graph.bindings,
        sources: &graph.sources,
        row_values: &graph.row_values,
        row_source_projections: &graph.row_source_projections,
        external_references: &graph.external_references,
        producer_result_fields: &graph.producer_result_fields,
        named_values: &graph.named_values,
    };
    boon_contract::canonical_serde_hash_v1(SEMANTIC_SCOPE_STORAGE_GRAPH_DIGEST_DOMAIN, &payload)
        .map(SemanticScopeStorageGraphDigestV1)
        .map_err(|error| {
            SemanticScopeStorageError::new(format!(
                "failed to hash semantic scope-storage graph: {error}"
            ))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (CheckedProgram, crate::SemanticProgram) {
        let parsed = boon_parser::parse_source(
            "server-outputs.bn",
            include_str!("../../../examples/server_outputs.bn"),
        )
        .expect("parse");
        let output = boon_typecheck::check_program(&parsed);
        assert!(
            !output.report.has_errors(),
            "unexpected diagnostics: {:#?}",
            output.report.diagnostics
        );
        let checked = output.program.expect("checked");
        let semantic = crate::elaborate(checked.clone(), &[]).expect("semantic");
        (checked, semantic)
    }

    fn graph(
        checked: &CheckedProgram,
        semantic: &crate::SemanticProgram,
    ) -> SemanticScopeStorageGraphV1 {
        build_semantic_scope_storage_graph(
            checked,
            semantic.execution_graph(),
            semantic.resource_graph(),
            semantic.reactive_graph(),
            semantic.lowering_contract(),
            semantic.resolved_out_graph(),
        )
        .expect("storage graph")
    }

    #[test]
    fn every_reactive_field_binding_and_source_has_exact_storage_identity() {
        let (checked, semantic) = fixture();
        let graph = graph(&checked, &semantic);
        assert_eq!(
            graph
                .fields
                .iter()
                .filter(|field| field.reactive_field.is_some())
                .count(),
            semantic.reactive_graph().fields.len()
        );
        assert_eq!(
            graph.bindings.len(),
            semantic.reactive_graph().bindings.len()
        );
        assert_eq!(graph.sources.len(), semantic.resource_graph().sources.len());
        assert!(
            graph
                .fields
                .iter()
                .enumerate()
                .all(|(index, field)| { field.id == SemanticStorageFieldId(index) })
        );
        graph
            .validate(
                &checked,
                semantic.execution_graph(),
                semantic.resource_graph(),
                semantic.reactive_graph(),
                semantic.lowering_contract(),
                semantic.resolved_out_graph(),
            )
            .expect("fresh graph validates");
    }

    #[test]
    fn list_item_authority_fields_are_semantic_before_backend_allocation() {
        let (checked, semantic) = fixture();
        let graph = graph(&checked, &semantic);
        let pending = semantic
            .resource_graph()
            .value_list_authorities
            .iter()
            .find(|authority| authority.local_name == "pending_priorities")
            .expect("pending priorities value-list authority");
        assert!(graph.fields.iter().any(|field| {
            matches!(
                field.origin,
                SemanticStorageFieldOriginV1::ValueListAuthority {
                    authority,
                    ref item_path,
                } if authority == pending.id && !item_path.is_empty()
            )
        }));
    }

    #[test]
    fn validation_rejects_storage_field_and_source_identity_mutations() {
        let (checked, semantic) = fixture();
        let fresh = graph(&checked, &semantic);

        let mut field_mutation = fresh.clone();
        field_mutation.fields[0].id = SemanticStorageFieldId(usize::MAX);
        assert!(
            field_mutation
                .validate(
                    &checked,
                    semantic.execution_graph(),
                    semantic.resource_graph(),
                    semantic.reactive_graph(),
                    semantic.lowering_contract(),
                    semantic.resolved_out_graph(),
                )
                .is_err()
        );

        let mut source_mutation = fresh;
        source_mutation.sources[0].binding = SemanticBindingId(usize::MAX);
        assert!(
            source_mutation
                .validate(
                    &checked,
                    semantic.execution_graph(),
                    semantic.resource_graph(),
                    semantic.reactive_graph(),
                    semantic.lowering_contract(),
                    semantic.resolved_out_graph(),
                )
                .is_err()
        );
    }

    #[test]
    fn owner_ancestry_is_root_to_leaf_in_the_stored_contract() {
        let owners = vec![
            SemanticStorageOwnerV1 {
                id: StaticOwnerId(0),
                parent: None,
                child_ordinal: 0,
                source_row: None,
                target_row: None,
                authority_row: None,
            },
            SemanticStorageOwnerV1 {
                id: StaticOwnerId(1),
                parent: Some(StaticOwnerId(0)),
                child_ordinal: 0,
                source_row: None,
                target_row: None,
                authority_row: None,
            },
        ];
        assert_eq!(
            owner_ancestry(Some(StaticOwnerId(1)), &owners).unwrap(),
            vec![StaticOwnerId(0), StaticOwnerId(1)]
        );
        let stored_binding = SemanticStorageBindingV1 {
            binding: SemanticBindingId(0),
            owner_ancestry: owner_ancestry(Some(StaticOwnerId(1)), &owners).unwrap(),
            diagnostic_path: "nested.value".to_owned(),
            target: SemanticStorageBindingTargetV1::Value {
                field: SemanticStorageFieldId(0),
                row: None,
            },
        };
        assert_eq!(
            stored_binding.owner_ancestry,
            vec![StaticOwnerId(0), StaticOwnerId(1)]
        );
        assert_eq!(
            stored_binding.owner_ancestry.last(),
            Some(&StaticOwnerId(1))
        );
    }

    #[test]
    fn every_named_value_origin_has_an_exact_non_path_target() {
        let (_, semantic) = fixture();
        let graph = semantic.scope_storage_graph();
        for named_value in &semantic.lowering_contract().metadata.named_value_types {
            for origin_ordinal in 0..named_value.origins.len() {
                let rows = graph
                    .named_values
                    .iter()
                    .filter(|row| {
                        row.named_value == named_value.id && row.origin_ordinal == origin_ordinal
                    })
                    .collect::<Vec<_>>();
                assert!(
                    !rows.is_empty(),
                    "named value {} origin {origin_ordinal} lacks an exact target",
                    named_value.id
                );
                assert!(rows.iter().all(|row| !matches!(
                    row.target,
                    SemanticNamedValueStorageTargetV1::DiagnosticOnly { .. }
                ) || row.projection.is_empty()));
            }
        }
    }

    #[test]
    fn fixed_bytes_representation_rejects_length_path_kind_and_digest_mutations() {
        let (checked, semantic) = fixture();
        let object = |fields: Vec<(&str, Type)>| {
            let field_order = fields
                .iter()
                .map(|(name, _)| (*name).to_owned())
                .collect::<Vec<_>>();
            Type::Object(boon_typecheck::ObjectShape {
                fields: fields
                    .into_iter()
                    .map(|(name, ty)| (name.to_owned(), ty))
                    .collect(),
                field_order,
                open: false,
            })
        };
        let dynamic_body = object(vec![(
            "body",
            Type::Bytes(boon_typecheck::BytesType::Dynamic),
        )]);
        let fixed_body = object(vec![(
            "body",
            Type::Bytes(boon_typecheck::BytesType::Fixed(8)),
        )]);
        let dynamic = object(vec![("envelope", Type::List(Box::new(dynamic_body)))]);
        let fixed = object(vec![("envelope", Type::List(Box::new(fixed_body)))]);
        let expected =
            derive_storage_representation(&dynamic, &fixed).expect("exact fixed-BYTES refinement");
        assert!(matches!(
            &expected,
            SemanticStorageRepresentationV1::CheckedFixedBytes { .. }
        ));

        let mut fresh = graph(&checked, &semantic);
        let row_index = 0;
        fresh.named_values[row_index].representation = expected.clone();
        fresh.digest = scope_storage_digest(&fresh).unwrap();
        let rejects = |representation: &SemanticStorageRepresentationV1| {
            representation != &derive_storage_representation(&dynamic, &fixed).unwrap()
        };

        let mut length = fresh.clone();
        let SemanticStorageRepresentationV1::CheckedFixedBytes { refinements } =
            &mut length.named_values[row_index].representation
        else {
            unreachable!()
        };
        refinements[0].fixed_len += 1;
        assert_ne!(scope_storage_digest(&length).unwrap(), fresh.digest);
        assert!(rejects(&length.named_values[row_index].representation));

        let mut selector = fresh.clone();
        let SemanticStorageRepresentationV1::CheckedFixedBytes { refinements } =
            &mut selector.named_values[row_index].representation
        else {
            unreachable!()
        };
        let selector_name = refinements[0]
            .path
            .iter_mut()
            .find_map(|segment| match segment {
                SemanticStorageTypePathSegmentV1::ObjectField { selector, .. } => Some(selector),
                SemanticStorageTypePathSegmentV1::ListItem => None,
            })
            .expect("fixed-BYTES refinement has a structural object path");
        selector_name.push_str("_stale");
        assert_ne!(scope_storage_digest(&selector).unwrap(), fresh.digest);
        assert!(rejects(&selector.named_values[row_index].representation));

        let mut ordinal = fresh.clone();
        let SemanticStorageRepresentationV1::CheckedFixedBytes { refinements } =
            &mut ordinal.named_values[row_index].representation
        else {
            unreachable!()
        };
        let field_ordinal = refinements[0]
            .path
            .iter_mut()
            .find_map(|segment| match segment {
                SemanticStorageTypePathSegmentV1::ObjectField { field_ordinal, .. } => {
                    Some(field_ordinal)
                }
                SemanticStorageTypePathSegmentV1::ListItem => None,
            })
            .expect("fixed-BYTES refinement has a structural object ordinal");
        *field_ordinal += 1;
        assert_ne!(scope_storage_digest(&ordinal).unwrap(), fresh.digest);
        assert!(rejects(&ordinal.named_values[row_index].representation));

        let mut kind = fresh;
        kind.named_values[row_index].representation = SemanticStorageRepresentationV1::Exact;
        assert_ne!(scope_storage_digest(&kind).unwrap(), kind.digest);
        assert!(rejects(&kind.named_values[row_index].representation));
    }
}
