//! Semantic resource ownership and storage graph.
//!
//! This module is deliberately pre-backend. It assigns only `Semantic*Id`
//! identities and records every storage/resource choice needed by verification.
//! Executable lowering may map these identities, but must not rediscover them.

use crate::{
    OutCallInstanceId, ProducerFunctionId, ProducerMaterializationMode, ResolvedOutGraph,
    SemanticBlockBinding, SemanticCallableId, SemanticContextualOperationKind,
    SemanticContextualRowPredecessor, SemanticExecutionGraphV1, SemanticExprId,
    SemanticExpressionKind, SemanticListId, SemanticLocalBindingId, SemanticMaterializationId,
    SemanticMaterializationResultKind, SemanticRowBinding, SemanticRowScopeId, SemanticSourceId,
    SemanticSourceOrigin, SemanticStateId, SemanticStatement, SemanticStatementId,
    SemanticStatementKind, SemanticStatementOrigin, SemanticValueId, SemanticValueListAuthorityId,
    StaticOwnerId,
};
use boon_typecheck::{
    CheckedListId, CheckedListKeyPolicy, CheckedProgram, CheckedResourceBinding, CheckedSourceId,
    CheckedSpan, CheckedStateId, CheckedStateKind, CheckedStatementId, DeclId, FlowType, Type,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

pub const SEMANTIC_RESOURCE_GRAPH_SCHEMA_V1: &str = "boon.semantic-resource-graph.v1";
const SEMANTIC_RESOURCE_GRAPH_DIGEST_DOMAIN: &[u8] = b"boon.semantic-resource-graph.v1\0";

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SemanticResourceGraphDigestV1([u8; 32]);

impl SemanticResourceGraphDigestV1 {
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    pub fn to_hex(self) -> String {
        self.0.iter().map(|byte| format!("{byte:02x}")).collect()
    }
}

impl fmt::Display for SemanticResourceGraphDigestV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.to_hex())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticResourceGraphV1 {
    pub schema: String,
    pub row_scopes: Vec<SemanticRowScopeV1>,
    pub lists: Vec<SemanticListResourceV1>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub value_list_authorities: Vec<SemanticValueListAuthorityV1>,
    pub sources: Vec<SemanticSourceResourceV1>,
    pub states: Vec<SemanticStateResourceV1>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub aliases: Vec<SemanticResourceAliasV1>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub materialization_bindings: Vec<SemanticMaterializationResourceBindingV1>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub list_projections: Vec<SemanticListProjectionV1>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub producer_resources: Vec<SemanticProducerResourceV1>,
    pub digest: SemanticResourceGraphDigestV1,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticRowScopeV1 {
    pub id: SemanticRowScopeId,
    pub list: SemanticListId,
    pub semantic_path: String,
    pub stable_name: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticListResourceV1 {
    pub id: SemanticListId,
    pub declaration: DeclId,
    pub statement: SemanticStatementId,
    pub producer: SemanticExprId,
    pub origin: SemanticListResourceOriginV1,
    pub semantic_path: String,
    pub local_name: String,
    pub row_scope: SemanticRowScopeId,
    pub item_type: Type,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub item_fields: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capacity: Option<usize>,
    pub key_policy: SemanticListKeyPolicyV1,
    pub initializer: SemanticListInitializerV1,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub row_predecessors: Vec<SemanticContextualRowPredecessor>,
    pub span: SemanticResourceSpanV1,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SemanticListResourceOriginV1 {
    CheckedLiteral {
        checked_list: CheckedListId,
    },
    Derived {
        statement: SemanticStatementId,
        producer: SemanticExprId,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticValueListAuthorityV1 {
    pub id: SemanticValueListAuthorityId,
    pub declaration: DeclId,
    pub statement: SemanticStatementId,
    pub producer: SemanticExprId,
    pub origin: SemanticListResourceOriginV1,
    pub semantic_path: String,
    pub local_name: String,
    pub role: SemanticValueListRoleV1,
    pub item_type: Type,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capacity: Option<usize>,
    pub initializer: SemanticListInitializerV1,
    pub span: SemanticResourceSpanV1,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SemanticValueListRoleV1 {
    ScalarAuthority,
    InlineValue,
    Alias { target: DeclId },
    Absent,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SemanticListKeyPolicyV1 {
    GeneratedOccurrenceU64 { has_generation: bool },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SemanticListInitializerV1 {
    Empty,
    RecordLiteral {
        authority_root: SemanticExprId,
        rows: Vec<SemanticListInitialRowV1>,
    },
    ValueLiteral {
        authority_root: SemanticExprId,
        values: Vec<SemanticListInitialValueV1>,
    },
    Range {
        authority_root: SemanticExprId,
        from_expression: SemanticExprId,
        to_expression: SemanticExprId,
        from: i64,
        to: i64,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticListInitialValueV1 {
    pub expression: SemanticExprId,
    pub value: SemanticInitialValueV1,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticListInitialRowV1 {
    pub expression: SemanticExprId,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fields: Vec<SemanticListInitialFieldV1>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticListInitialFieldV1 {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub declaration: Option<DeclId>,
    pub name: String,
    pub value: SemanticInitialValueV1,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expression: Option<SemanticExprId>,
    pub spread: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spread_origin: Option<SemanticExprId>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SemanticInitialValueV1 {
    Text {
        value: String,
    },
    Number {
        value: boon_data::ExactNumber,
    },
    Bytes {
        bytes: Vec<u8>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        fixed_len: Option<usize>,
    },
    Tag {
        name: String,
    },
    Data {
        value: boon_data::Value,
    },
    RootInitialField {
        path: String,
    },
    RowInitialField {
        path: String,
    },
    Unknown {
        summary: String,
    },
    /// The exact executable expression is retained on the enclosing initial
    /// field. This marker distinguishes a checked collection authority from
    /// an unresolved initializer while keeping authority identity out of the
    /// public value payload.
    ExpressionAuthority,
    /// A row-local source facade is runtime routing metadata, not an
    /// application value or a serializable list initializer.
    ResourceOnly,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticSourceResourceV1 {
    pub id: SemanticSourceId,
    pub declaration: DeclId,
    pub statement: SemanticStatementId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checked_statement: Option<CheckedStatementId>,
    pub expression: SemanticExprId,
    pub origin: SemanticSourceOrigin,
    pub semantic_path: String,
    /// Exact declared/lexical binding before canonical contextual ownership.
    pub declared_binding_path: String,
    /// Final runtime binding path. This is deliberately identical to
    /// `semantic_path`; lowering must copy it without re-canonicalizing.
    pub binding_path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<StaticOwnerId>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub owner_ancestry: Vec<StaticOwnerId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_list: Option<SemanticListId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub row_scope: Option<SemanticRowScopeId>,
    pub scoped: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interval_ms: Option<u64>,
    pub payload_type: Type,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub payload_fields: Vec<SemanticPayloadFieldV1>,
    pub span: SemanticResourceSpanV1,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticPayloadFieldV1 {
    pub name: String,
    pub data_type: Type,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticStateResourceV1 {
    pub id: SemanticStateId,
    pub checked_state: CheckedStateId,
    pub declaration: DeclId,
    pub statement: SemanticStatementId,
    pub checked_statement: CheckedStatementId,
    pub expression: SemanticExprId,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub expression_members: Vec<SemanticExprId>,
    pub initial: SemanticExprId,
    pub flow_type: FlowType,
    pub kind: CheckedStateKind,
    pub binding_path: String,
    pub declared_path: String,
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub semantic_path: Option<String>,
    pub published: bool,
    pub hold_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<StaticOwnerId>,
    pub lifetime: crate::SemanticStateLifetimeV1,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub owner_ancestry: Vec<StaticOwnerId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_list: Option<SemanticListId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub row_scope: Option<SemanticRowScopeId>,
    pub scoped: bool,
    pub checked_span: SemanticResourceSpanV1,
    pub span: SemanticResourceSpanV1,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticResourceSpanV1 {
    pub line: usize,
    pub start: usize,
    pub end: usize,
}

impl From<CheckedSpan> for SemanticResourceSpanV1 {
    fn from(span: CheckedSpan) -> Self {
        Self {
            line: span.line,
            start: span.start,
            end: span.end,
        }
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct SemanticResourceAliasV1 {
    /// Lexical/provenance owner, not a runtime lookup namespace. Reused
    /// contextual call frames may contribute the same alias and owner for
    /// multiple occurrence-specific source or state targets.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<StaticOwnerId>,
    pub alias: String,
    pub target: SemanticResourceAliasTargetV1,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(tag = "kind", content = "id", rename_all = "snake_case")]
pub enum SemanticResourceAliasTargetV1 {
    Source(SemanticSourceId),
    State(SemanticStateId),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticMaterializationResourceBindingV1 {
    pub materialization: SemanticMaterializationId,
    pub owner: StaticOwnerId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<SemanticRowBinding>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<SemanticRowBinding>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub predecessors: Vec<SemanticContextualRowPredecessor>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticListProjectionV1 {
    pub target: SemanticListId,
    pub source: SemanticListId,
    pub kind: SemanticListProjectionKindV1,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SemanticListProjectionKindV1 {
    Chunk {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        size_expression: Option<SemanticExprId>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        resolved_size: Option<usize>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticProducerResourceV1 {
    pub identity: [u8; 32],
    pub mode: ProducerMaterializationMode,
    pub function: ProducerFunctionId,
    pub callable: SemanticCallableId,
    pub root_call: OutCallInstanceId,
    pub result_statement: SemanticStatementId,
    pub result_declaration: DeclId,
    pub result_path: String,
    pub owner: StaticOwnerId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invocation_source: Option<SemanticSourceId>,
}

impl SemanticResourceGraphV1 {
    pub fn validate(
        &self,
        execution: &SemanticExecutionGraphV1,
        out_net: &ResolvedOutGraph,
    ) -> Result<(), String> {
        if self.schema != SEMANTIC_RESOURCE_GRAPH_SCHEMA_V1 {
            return Err(format!(
                "unsupported semantic resource graph schema `{}`",
                self.schema
            ));
        }
        validate_dense_resource_ids(self, execution)?;
        validate_materialization_bindings(self, execution)?;
        validate_list_lineage(self, execution)?;
        validate_list_projections(self, execution)?;
        validate_resource_owners(self, execution)?;
        validate_resource_aliases(self)?;
        validate_producer_resources(self, execution, out_net)?;
        let expected = resource_graph_digest(self)?;
        if self.digest != expected {
            return Err(
                "semantic resource graph digest does not match its canonical payload".to_owned(),
            );
        }
        Ok(())
    }
}

pub(crate) fn build_semantic_resource_graph(
    checked: &CheckedProgram,
    out_net: &ResolvedOutGraph,
    execution: &mut SemanticExecutionGraphV1,
) -> Result<SemanticResourceGraphV1, String> {
    let (row_scopes, mut lists, value_list_authorities) =
        discover_list_resources(checked, execution)?;
    let target_lists = materialization_target_lists(execution, &lists)?;
    bind_materialization_targets(execution, &lists, &target_lists)?;
    bind_materialization_sources(execution, &lists)?;
    bind_materialization_lineage(execution, &lists)?;
    bind_list_lineage(execution, &mut lists)?;

    let mut aliases = Vec::new();
    let sources = build_source_resources(checked, execution, &lists, &target_lists, &mut aliases)?;
    let states = build_state_resources(checked, execution, &lists, &target_lists, &mut aliases)?;
    aliases.sort();
    aliases.dedup();
    let materialization_bindings = execution
        .materializations
        .iter()
        .map(|materialization| {
            Ok(SemanticMaterializationResourceBindingV1 {
                materialization: materialization.id,
                owner: materialization.owner,
                source: paired_row_binding(
                    materialization.source_list_id,
                    materialization.source_scope_id,
                    "source",
                    materialization.owner,
                )?,
                target: paired_row_binding(
                    materialization.target_list_id,
                    materialization.target_scope_id,
                    "target",
                    materialization.owner,
                )?,
                predecessors: materialization.source_row_predecessors.clone(),
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let list_projections = discover_list_projections(execution, &lists)?;
    let producer_resources = build_producer_resources(out_net, execution)?;
    let mut graph = SemanticResourceGraphV1 {
        schema: SEMANTIC_RESOURCE_GRAPH_SCHEMA_V1.to_owned(),
        row_scopes,
        lists,
        value_list_authorities,
        sources,
        states,
        aliases,
        materialization_bindings,
        list_projections,
        producer_resources,
        digest: SemanticResourceGraphDigestV1([0; 32]),
    };
    graph.digest = resource_graph_digest(&graph)?;
    graph.validate(execution, out_net)?;
    validate_checked_list_classification(checked, execution, &graph)?;
    validate_checked_resource_provenance(checked, execution, &graph)?;
    Ok(graph)
}

#[derive(Clone)]
struct ListTarget {
    statement: SemanticStatementId,
    declaration: DeclId,
    producer: SemanticExprId,
    path: String,
    local_name: String,
    capacity: Option<usize>,
    item_type: Type,
    item_fields: Vec<String>,
    span: SemanticResourceSpanV1,
    alias: Option<DeclId>,
    absent: bool,
    value_only: bool,
    origin: SemanticListResourceOriginV1,
    key_policy: SemanticListKeyPolicyV1,
}

type DiscoveredListResources = (
    Vec<SemanticRowScopeV1>,
    Vec<SemanticListResourceV1>,
    Vec<SemanticValueListAuthorityV1>,
);

fn discover_list_resources(
    checked: &CheckedProgram,
    execution: &mut SemanticExecutionGraphV1,
) -> Result<DiscoveredListResources, String> {
    let targets = typed_list_targets(checked, execution)?;
    let mut row_scopes = Vec::with_capacity(targets.len());
    let mut lists = Vec::with_capacity(targets.len());
    let mut value_list_authorities = Vec::new();
    for target in targets {
        let initializer = if target.value_only {
            SemanticListInitializerV1::Empty
        } else {
            semantic_list_initializer(execution, target.producer, &target.path)?
        };
        if target.value_only
            || target.absent
            || target.alias.is_some()
            || !matches!(target.item_type, Type::Object(_))
        {
            let role = if target.value_only {
                SemanticValueListRoleV1::InlineValue
            } else if target.absent {
                SemanticValueListRoleV1::Absent
            } else if let Some(target) = target.alias {
                SemanticValueListRoleV1::Alias { target }
            } else {
                SemanticValueListRoleV1::ScalarAuthority
            };
            value_list_authorities.push(SemanticValueListAuthorityV1 {
                id: SemanticValueListAuthorityId(value_list_authorities.len()),
                declaration: target.declaration,
                statement: target.statement,
                producer: target.producer,
                origin: target.origin,
                semantic_path: target.path,
                local_name: target.local_name,
                role,
                item_type: target.item_type,
                capacity: target.capacity,
                initializer,
                span: target.span,
            });
            continue;
        }
        let id = SemanticListId(lists.len());
        let row_scope = SemanticRowScopeId(row_scopes.len());
        row_scopes.push(SemanticRowScopeV1 {
            id: row_scope,
            list: id,
            semantic_path: target.path.clone(),
            stable_name: format!("list_{}_row", id.as_usize()),
        });
        lists.push(SemanticListResourceV1 {
            id,
            declaration: target.declaration,
            statement: target.statement,
            producer: target.producer,
            origin: target.origin,
            semantic_path: target.path,
            local_name: target.local_name,
            row_scope,
            item_type: target.item_type,
            item_fields: target.item_fields,
            capacity: target.capacity,
            key_policy: target.key_policy,
            initializer,
            row_predecessors: Vec::new(),
            span: target.span,
        });
    }
    Ok((row_scopes, lists, value_list_authorities))
}

fn typed_list_targets(
    checked: &CheckedProgram,
    execution: &mut SemanticExecutionGraphV1,
) -> Result<Vec<ListTarget>, String> {
    let direct = direct_storage_statements(execution);
    let mut targets = Vec::new();
    for statement in &execution.statements {
        if !direct.contains(&statement.id) {
            continue;
        }
        let Some((local_name, path)) = statement_name_path(&statement.kind) else {
            continue;
        };
        let Some(producer) = statement.value else {
            continue;
        };
        let expression = expression(execution, producer)?;
        let Type::List(item_type) = &expression.flow_type.ty else {
            continue;
        };
        let declaration = statement.declaration.ok_or_else(|| {
            format!(
                "typed list-valued semantic statement {} has no declaration",
                statement.id
            )
        })?;
        let mut item_fields = ordered_object_fields(item_type);
        for field in expression_record_field_names(execution, producer)? {
            if !item_fields.contains(&field) {
                item_fields.push(field);
            }
        }
        let span = checked
            .declarations
            .iter()
            .find(|candidate| candidate.id == declaration)
            .map(|declaration| declaration.span.into())
            .ok_or_else(|| {
                format!(
                    "typed list-valued semantic statement {} references missing declaration {}",
                    statement.id, declaration.0
                )
            })?;
        let capacity = match &statement.kind {
            SemanticStatementKind::List { capacity, .. } => *capacity,
            _ => match expression.kind {
                SemanticExpressionKind::List { capacity, .. } => capacity,
                _ => None,
            },
        };
        targets.push(ListTarget {
            statement: statement.id,
            declaration,
            producer,
            path: path.to_owned(),
            local_name: local_name.to_owned(),
            capacity,
            item_type: (**item_type).clone(),
            item_fields,
            span,
            alias: direct_list_alias_target(execution, statement),
            absent: expression.flow_type.mode == boon_typecheck::FlowMode::Absent,
            value_only: false,
            origin: SemanticListResourceOriginV1::Derived {
                statement: statement.id,
                producer,
            },
            key_policy: SemanticListKeyPolicyV1::GeneratedOccurrenceU64 {
                has_generation: true,
            },
        });
    }
    targets.sort_by_key(|target| target.statement);
    let mut classified = BTreeSet::new();
    for checked_list in &checked.lists {
        let path = checked.semantic_path(&checked_list.path).ok_or_else(|| {
            format!(
                "checked list {} declaration {} has no canonical semantic path from anchor {} projection {:?}",
                checked_list.id.0,
                checked_list.declaration.0,
                checked_list.path.anchor.0,
                checked_list.path.projection,
            )
        })?;
        let mut matches = Vec::new();
        for (index, target) in targets.iter().enumerate() {
            if target.declaration != checked_list.declaration || target.path != path {
                continue;
            }
            let Some(authority) = inline_list_authority_root(execution, target.producer)? else {
                continue;
            };
            if expression(execution, authority)?.checked_expr_id == checked_list.producer {
                matches.push(index);
            }
        }
        if matches.is_empty()
            && let Some(target) =
                synthesize_inline_checked_list_target(checked, execution, checked_list, &path)?
        {
            matches.push(targets.len());
            targets.push(target);
        }
        let [index] = matches.as_slice() else {
            return Err(format!(
                "checked list {} declaration {} path `{path}` maps to {} exact semantic list producers",
                checked_list.id.0,
                checked_list.declaration.0,
                matches.len()
            ));
        };
        if !classified.insert(*index) {
            return Err(format!(
                "semantic list target {} is claimed by more than one checked list",
                targets[*index].statement
            ));
        }
        let target = &mut targets[*index];
        if target.item_type != checked_list.item_type
            || target.capacity != checked_list.capacity
            || target.alias.is_some()
        {
            return Err(format!(
                "checked list {} differs from semantic list target {}: item type {:?} vs {:?}, capacity {:?} vs {:?}, alias {:?}",
                checked_list.id.0,
                target.statement,
                checked_list.item_type,
                target.item_type,
                checked_list.capacity,
                target.capacity,
                target.alias,
            ));
        }
        target.origin = SemanticListResourceOriginV1::CheckedLiteral {
            checked_list: checked_list.id,
        };
        target.key_policy = match checked_list.key_policy {
            CheckedListKeyPolicy::GeneratedOccurrenceU64 { has_generation } => {
                SemanticListKeyPolicyV1::GeneratedOccurrenceU64 { has_generation }
            }
        };
        target.span = checked_list.span.into();
    }
    Ok(targets)
}

fn contextual_list_authority_statements(
    execution: &SemanticExecutionGraphV1,
    owner: Option<StaticOwnerId>,
) -> Result<BTreeSet<SemanticStatementId>, String> {
    for owner in owner_ancestry(owner, &execution.static_owners)? {
        let materializations = execution
            .materializations
            .iter()
            .filter(|materialization| materialization.owner == owner)
            .map(|materialization| materialization.id)
            .collect::<BTreeSet<_>>();
        if materializations.is_empty() {
            continue;
        }

        let mut statements = BTreeSet::new();
        for expression in execution.expressions.iter().filter(|expression| {
            matches!(
                expression.kind,
                SemanticExpressionKind::Materialize { materialization }
                    if materializations.contains(&materialization)
            )
        }) {
            let origin = execution
                .checked_expression_origins
                .get(expression.id.as_usize())
                .filter(|origin| origin.expression == expression.id)
                .ok_or_else(|| {
                    format!(
                        "contextual materialization expression {} has no exact origin",
                        expression.id
                    )
                })?;
            let Some(statement) = origin.owning_statement else {
                continue;
            };
            execution
                .statements
                .get(statement.as_usize())
                .filter(|candidate| candidate.id == statement)
                .ok_or_else(|| {
                    format!(
                        "contextual materialization expression {} references missing statement {statement}",
                        expression.id
                    )
                })?;
            statements.insert(statement);
        }
        if statements.len() > 1 {
            let declaration_statements = statements
                .iter()
                .copied()
                .filter(|statement| {
                    execution
                        .statements
                        .get(statement.as_usize())
                        .filter(|candidate| candidate.id == *statement)
                        .is_some_and(|statement| statement.declaration.is_some())
                })
                .collect::<BTreeSet<_>>();
            if !declaration_statements.is_empty() {
                statements = declaration_statements;
            }
        }
        return Ok(statements);
    }
    Ok(BTreeSet::new())
}

fn synthesize_inline_checked_list_target(
    checked: &CheckedProgram,
    execution: &mut SemanticExecutionGraphV1,
    checked_list: &boon_typecheck::CheckedList,
    path: &str,
) -> Result<Option<ListTarget>, String> {
    let binding = CheckedResourceBinding::ListAuthority {
        list: checked_list.id,
    };
    let binding_statements = execution
        .statements
        .iter()
        .filter(|statement| statement.checked_resources.contains(&binding))
        .map(|statement| statement.id)
        .collect::<Vec<_>>();
    let mut candidates = Vec::new();
    for candidate in execution.expressions.iter().filter(|expression| {
        expression.checked_expr_id == checked_list.producer
            && matches!(expression.kind, SemanticExpressionKind::List { .. })
    }) {
        let origin = execution
            .checked_expression_origins
            .get(candidate.id.as_usize())
            .filter(|origin| origin.expression == candidate.id)
            .ok_or_else(|| {
                format!(
                    "checked inline list {} expression {} has no exact origin",
                    checked_list.id.0, candidate.id
                )
            })?;
        let mut owners = BTreeSet::new();
        if let Some(statement) = origin.owning_statement
            && execution
                .statements
                .get(statement.as_usize())
                .filter(|candidate| candidate.id == statement)
                .is_some_and(|statement| {
                    statement.checked_resources.contains(&binding)
                        || origin.call_instance.is_some()
                            && checked
                                .declarations
                                .iter()
                                .find(|declaration| declaration.id == checked_list.path.anchor)
                                .is_some_and(|declaration| {
                                    declaration.kind
                                        == boon_typecheck::CheckedDeclarationKind::Function
                                })
                })
        {
            owners.insert(statement);
        }
        for statement in &binding_statements {
            let statement = execution
                .statements
                .get(statement.as_usize())
                .filter(|candidate| candidate.id == *statement)
                .ok_or_else(|| {
                    format!(
                        "checked inline list {} references missing binding statement {statement}",
                        checked_list.id.0
                    )
                })?;
            if let Some(root) = statement.value
                && expression_reaches(execution, root, candidate.id)?
            {
                owners.insert(statement.id);
            }
        }
        if owners.is_empty() {
            owners.extend(contextual_list_authority_statements(
                execution,
                candidate.owner,
            )?);
        }
        match owners.len() {
            0 => {}
            1 => candidates.push((
                candidate.id,
                owners.iter().next().copied().expect("one exact list owner"),
            )),
            count => {
                return Err(format!(
                    "checked inline list {} expression {} resolves to {count} semantic authority statements",
                    checked_list.id.0, candidate.id
                ));
            }
        }
    }
    let concrete_candidates = candidates
        .iter()
        .copied()
        .filter(|(expression, statement)| {
            execution
                .statements
                .get(statement.as_usize())
                .filter(|candidate| candidate.id == *statement)
                .is_some_and(|statement| {
                    statement.declaration.is_none()
                        && statement.value == Some(*expression)
                        && matches!(
                            &statement.kind,
                            SemanticStatementKind::List {
                                path: Some(candidate_path),
                                ..
                            } if candidate_path == path
                        )
                })
        })
        .collect::<Vec<_>>();
    let (candidate, existing_statement, authority_statement) = match concrete_candidates.as_slice()
    {
        [(expression, statement)] => (*expression, Some(*statement), *statement),
        [] => match candidates.as_slice() {
            [(expression, statement)] => (*expression, None, *statement),
            [] => return Ok(None),
            _ => {
                return Err(format!(
                    "checked inline list {} resolves to {} exact semantic authority expressions",
                    checked_list.id.0,
                    candidates.len()
                ));
            }
        },
        _ => {
            return Err(format!(
                "checked inline list {} resolves to {} concrete semantic authority statements",
                checked_list.id.0,
                concrete_candidates.len()
            ));
        }
    };
    let definition = expression(execution, candidate)?.clone();
    let origin = execution
        .checked_expression_origins
        .get(candidate.as_usize())
        .filter(|origin| origin.expression == candidate)
        .cloned()
        .ok_or_else(|| {
            format!(
                "checked inline list {} expression {candidate} has no exact origin",
                checked_list.id.0
            )
        })?;
    if origin.checked_scope != checked_list.owner_scope {
        return Err(format!(
            "checked inline list {} expression {candidate} has checked scope {}, expected {}",
            checked_list.id.0, origin.checked_scope.0, checked_list.owner_scope.0
        ));
    }
    let local_name = checked
        .declarations
        .iter()
        .find(|declaration| declaration.id == checked_list.declaration)
        .filter(|declaration| declaration.kind != boon_typecheck::CheckedDeclarationKind::Function)
        .map(|declaration| declaration.name.clone())
        .or_else(|| path.rsplit('.').next().map(str::to_owned))
        .ok_or_else(|| {
            format!(
                "checked inline list {} has no semantic local name",
                checked_list.id.0
            )
        })?;
    let (producer, statement) = if let Some(statement) = existing_statement {
        (candidate, statement)
    } else {
        execution
            .statements
            .get(authority_statement.as_usize())
            .filter(|statement| statement.id == authority_statement)
            .ok_or_else(|| {
                format!(
                    "checked inline list {} references missing authority statement {authority_statement}",
                    checked_list.id.0
                )
            })?;
        let parent = Some(authority_statement);
        let statement = SemanticStatementId(execution.statements.len());
        let producer = SemanticExprId(execution.expressions.len());
        let mut concrete = definition.clone();
        concrete.id = producer;
        concrete.value_id = SemanticValueId(producer.as_usize());
        concrete.flow_type.ty = Type::List(Box::new(checked_list.item_type.clone()));
        let concrete_flow_type = concrete.flow_type.clone();
        execution.expressions.push(concrete);
        let mut concrete_origin = origin.clone();
        concrete_origin.expression = producer;
        concrete_origin.owning_statement = Some(statement);
        execution.checked_expression_origins.push(concrete_origin);
        let scope = execution
            .scopes
            .iter()
            .find(|scope| scope.checked_scope == origin.checked_scope)
            .map(|scope| scope.id)
            .ok_or_else(|| {
                format!(
                    "checked inline list {} expression {candidate} references missing semantic scope {}",
                    checked_list.id.0, origin.checked_scope.0
                )
            })?;
        execution.statements.push(SemanticStatement {
            id: statement,
            origin: SemanticStatementOrigin::Checked {
                statement: checked_list.statement,
            },
            scope,
            parent,
            call_instance: origin.call_instance,
            span: checked_list.span,
            checked_resources: vec![binding],
            declaration: None,
            flow_type: Some(concrete_flow_type),
            kind: SemanticStatementKind::List {
                name: Some(local_name.clone()),
                path: Some(path.to_owned()),
                capacity: checked_list.capacity,
            },
            value: Some(producer),
            value_use: SemanticMaterializationResultKind::RuntimeValue,
            children: Vec::new(),
        });
        if let Some(parent_id) = parent {
            let parent = execution
                .statements
                .get_mut(parent_id.as_usize())
                .filter(|candidate| candidate.id == parent_id)
                .ok_or_else(|| {
                    format!(
                        "checked inline list {} references missing parent semantic statement {parent_id}",
                        checked_list.id.0
                    )
                })?;
            if !parent.children.contains(&statement) {
                parent.children.push(statement);
            }
        }
        (producer, statement)
    };
    let Type::List(_) = &definition.flow_type.ty else {
        return Err(format!(
            "checked inline list {} semantic authority is not list-typed",
            checked_list.id.0
        ));
    };
    // The literal's local type can intentionally be open (most notably for an
    // empty fallback arm).  The checked list owns the contextual item type
    // inferred from the complete declaration, so that is the exact authority
    // type that must survive into semantic storage.
    let mut item_fields = ordered_object_fields(&checked_list.item_type);
    for field in expression_record_field_names(execution, producer)? {
        if !item_fields.contains(&field) {
            item_fields.push(field);
        }
    }
    Ok(Some(ListTarget {
        statement,
        declaration: checked_list.declaration,
        producer,
        path: path.to_owned(),
        local_name,
        capacity: checked_list.capacity,
        item_type: checked_list.item_type.clone(),
        item_fields,
        span: checked_list.span.into(),
        alias: None,
        absent: definition.flow_type.mode == boon_typecheck::FlowMode::Absent,
        value_only: true,
        origin: SemanticListResourceOriginV1::CheckedLiteral {
            checked_list: checked_list.id,
        },
        key_policy: match checked_list.key_policy {
            CheckedListKeyPolicy::GeneratedOccurrenceU64 { has_generation } => {
                SemanticListKeyPolicyV1::GeneratedOccurrenceU64 { has_generation }
            }
        },
    }))
}

fn direct_storage_statements(
    execution: &SemanticExecutionGraphV1,
) -> BTreeSet<SemanticStatementId> {
    let parents = execution
        .statements
        .iter()
        .flat_map(|parent| parent.children.iter().map(move |child| (*child, parent.id)))
        .collect::<BTreeMap<_, _>>();
    execution
        .statements
        .iter()
        .filter(|statement| {
            if statement.declaration.is_none()
                && matches!(statement.kind, SemanticStatementKind::List { .. })
                && statement
                    .checked_resources
                    .iter()
                    .any(|binding| matches!(binding, CheckedResourceBinding::ListAuthority { .. }))
            {
                return false;
            }
            let Some(parent) = parents.get(&statement.id) else {
                return true;
            };
            execution
                .statements
                .get(parent.as_usize())
                .filter(|candidate| candidate.id == *parent)
                .is_some_and(|parent| {
                    parent.declaration.is_some()
                        && matches!(parent.kind, SemanticStatementKind::Field { .. })
                })
        })
        .map(|statement| statement.id)
        .collect()
}

fn statement_name_path(kind: &SemanticStatementKind) -> Option<(&str, &str)> {
    match kind {
        SemanticStatementKind::Field { name, path } => Some((name, path)),
        SemanticStatementKind::List {
            name: Some(name),
            path: Some(path),
            ..
        } => Some((name, path)),
        _ => None,
    }
}

fn direct_list_alias_target(
    execution: &SemanticExecutionGraphV1,
    statement: &crate::SemanticStatement,
) -> Option<DeclId> {
    let value = statement.value?;
    let expression = execution
        .expressions
        .get(value.as_usize())
        .filter(|expression| expression.id == value)?;
    if !matches!(expression.flow_type.ty, Type::List(_)) {
        return None;
    }
    match &expression.kind {
        SemanticExpressionKind::CanonicalRead {
            target, projection, ..
        } if projection.is_empty() => Some(*target),
        _ => None,
    }
}

fn ordered_object_fields(item_type: &Type) -> Vec<String> {
    let Type::Object(shape) = item_type else {
        return Vec::new();
    };
    let mut seen = BTreeSet::new();
    shape
        .field_order
        .iter()
        .chain(shape.fields.keys())
        .filter(|field| seen.insert((*field).clone()))
        .cloned()
        .collect()
}

fn expression_record_field_names(
    execution: &SemanticExecutionGraphV1,
    root: SemanticExprId,
) -> Result<Vec<String>, String> {
    let mut fields = Vec::new();
    let mut pending = vec![root];
    let mut visited = BTreeSet::new();
    while let Some(id) = pending.pop() {
        if !visited.insert(id) {
            continue;
        }
        let value = expression(execution, id)?;
        match &value.kind {
            SemanticExpressionKind::Object(record_fields)
            | SemanticExpressionKind::TaggedObject {
                fields: record_fields,
                ..
            } => {
                for field in record_fields {
                    if !field.spread && !fields.contains(&field.name) {
                        fields.push(field.name.clone());
                    }
                }
            }
            SemanticExpressionKind::Then { output, .. }
            | SemanticExpressionKind::MatchArm { output, .. } => {
                pending.extend(output.iter().copied());
            }
            SemanticExpressionKind::When { arms, .. } => {
                pending.extend(arms.iter().map(|arm| arm.output));
            }
            SemanticExpressionKind::Latest { branches } => {
                pending.extend(branches.iter().copied());
            }
            SemanticExpressionKind::Hold {
                initial, updates, ..
            } => {
                pending.push(*initial);
                pending.extend(updates.iter().copied());
            }
            SemanticExpressionKind::List { items, .. } => {
                pending.extend(items.iter().copied());
            }
            SemanticExpressionKind::Flush { payload: input }
            | SemanticExpressionKind::FlushBoundary { input }
            | SemanticExpressionKind::Draining { input }
            | SemanticExpressionKind::Project { input, .. } => pending.push(*input),
            SemanticExpressionKind::Block { bindings, result } => {
                pending.extend(bindings.iter().map(|binding| binding.value));
                pending.push(*result);
            }
            SemanticExpressionKind::CanonicalRead { .. }
            | SemanticExpressionKind::LocalRead { .. }
            | SemanticExpressionKind::ExternalRead { .. }
            | SemanticExpressionKind::ElementState { .. }
            | SemanticExpressionKind::Drain { .. }
            | SemanticExpressionKind::Text(_)
            | SemanticExpressionKind::TextTemplate { .. }
            | SemanticExpressionKind::Number(_)
            | SemanticExpressionKind::Bits(_)
            | SemanticExpressionKind::BytesByte(_)
            | SemanticExpressionKind::Absent
            | SemanticExpressionKind::Tag(_)
            | SemanticExpressionKind::Source { .. }
            | SemanticExpressionKind::Call { .. }
            | SemanticExpressionKind::Materialize { .. }
            | SemanticExpressionKind::Infix { .. }
            | SemanticExpressionKind::MapEntry { .. }
            | SemanticExpressionKind::Map { .. }
            | SemanticExpressionKind::Set { .. }
            | SemanticExpressionKind::Bytes { .. }
            | SemanticExpressionKind::Delimiter
            | SemanticExpressionKind::MaterializationLocal { .. }
            | SemanticExpressionKind::FunctionParameter { .. } => {}
        }
    }
    Ok(fields)
}

fn semantic_list_initializer(
    execution: &SemanticExecutionGraphV1,
    producer: SemanticExprId,
    path: &str,
) -> Result<SemanticListInitializerV1, String> {
    let Some(root) = inline_list_authority_root(execution, producer)? else {
        return Ok(SemanticListInitializerV1::Empty);
    };
    let value = expression(execution, root)?;
    match &value.kind {
        SemanticExpressionKind::List { items, .. } => {
            if items.is_empty() {
                return Ok(SemanticListInitializerV1::Empty);
            }
            let Type::List(item_type) = &value.flow_type.ty else {
                return Err(format!(
                    "list `{path}` authority root {root} is not typed as List"
                ));
            };
            if !matches!(item_type.as_ref(), Type::Object(_)) {
                let values = items
                    .iter()
                    .map(|item| {
                        Ok(SemanticListInitialValueV1 {
                            expression: *item,
                            value: semantic_initial_value(execution, *item)?,
                        })
                    })
                    .collect::<Result<Vec<_>, String>>()?;
                return Ok(SemanticListInitializerV1::ValueLiteral {
                    authority_root: root,
                    values,
                });
            }
            let mut rows = Vec::with_capacity(items.len());
            for item in items {
                rows.push(semantic_initial_record(execution, *item).map_err(|error| {
                    format!("list `{path}` authority item {item} is invalid: {error}")
                })?);
            }
            Ok(SemanticListInitializerV1::RecordLiteral {
                authority_root: root,
                rows,
            })
        }
        SemanticExpressionKind::Call {
            name, arguments, ..
        } if name == "List/range" => {
            let from_expression = call_argument(arguments, "from")
                .ok_or_else(|| "List/range authority has no `from` argument".to_owned())?;
            let to_expression = call_argument(arguments, "to")
                .ok_or_else(|| "List/range authority has no `to` argument".to_owned())?;
            let exact_bound = |name: &str, expression: SemanticExprId| -> Result<i64, String> {
                let value = semantic_static_data(execution, expression)?;
                let boon_data::Value::Number(value) = value else {
                    return Err(format!("List/range `{name}` is not a Number"));
                };
                value.to_i64_exact().map_err(|error| {
                    format!("List/range `{name}` is not an exact integer: {error}")
                })
            };
            Ok(SemanticListInitializerV1::Range {
                authority_root: root,
                from_expression,
                to_expression,
                from: exact_bound("from", from_expression)?,
                to: exact_bound("to", to_expression)?,
            })
        }
        other => Err(format!(
            "list `{path}` authority root {root} has unsupported semantic shape {other:?}"
        )),
    }
}

fn semantic_initial_record(
    execution: &SemanticExecutionGraphV1,
    expression_id: SemanticExprId,
) -> Result<SemanticListInitialRowV1, String> {
    let value = expression(execution, expression_id)?;
    let fields = match &value.kind {
        SemanticExpressionKind::Object(fields) => {
            let mut result = Vec::new();
            for field in fields {
                if field.spread {
                    let value = semantic_static_data(execution, field.value)?;
                    let boon_data::Value::Object(fields) = value else {
                        return Err(format!(
                            "spread field `{}` is not a static object",
                            field.name
                        ));
                    };
                    result.extend(fields.into_iter().map(|(name, value)| {
                        SemanticListInitialFieldV1 {
                            declaration: None,
                            name,
                            value: semantic_initial_value_from_data(value),
                            expression: None,
                            spread: true,
                            spread_origin: Some(field.value),
                        }
                    }));
                    continue;
                }
                result.push(SemanticListInitialFieldV1 {
                    declaration: field.declaration,
                    name: field.name.clone(),
                    value: semantic_initial_value(execution, field.value)?,
                    expression: Some(field.value),
                    spread: false,
                    spread_origin: None,
                });
            }
            result
        }
        _ => {
            let value = semantic_static_data(execution, expression_id)?;
            let boon_data::Value::Object(fields) = value else {
                return Err(format!("expression {expression_id} is not a static object"));
            };
            fields
                .into_iter()
                .map(|(name, value)| SemanticListInitialFieldV1 {
                    declaration: None,
                    name,
                    value: semantic_initial_value_from_data(value),
                    expression: None,
                    spread: false,
                    spread_origin: None,
                })
                .collect()
        }
    };
    Ok(SemanticListInitialRowV1 {
        expression: expression_id,
        fields,
    })
}

fn semantic_initial_value(
    execution: &SemanticExecutionGraphV1,
    expression_id: SemanticExprId,
) -> Result<SemanticInitialValueV1, String> {
    if let Some(path) = semantic_root_initial_path(execution, expression_id) {
        return Ok(SemanticInitialValueV1::RootInitialField { path });
    }
    if semantic_initializer_is_resource_only(execution, expression_id, &mut BTreeSet::new())? {
        return Ok(SemanticInitialValueV1::ResourceOnly);
    }
    match semantic_static_data(execution, expression_id) {
        Ok(value) => Ok(semantic_initial_value_from_data(value)),
        Err(_error)
            if semantic_type_contains_collection_authority(
                &expression(execution, expression_id)?.flow_type.ty,
            ) =>
        {
            Ok(SemanticInitialValueV1::ExpressionAuthority)
        }
        Err(error) => Err(format!(
            "semantic initial expression {expression_id}: {error}"
        )),
    }
}

fn semantic_initializer_is_resource_only(
    execution: &SemanticExecutionGraphV1,
    expression_id: SemanticExprId,
    visiting: &mut BTreeSet<SemanticExprId>,
) -> Result<bool, String> {
    if !visiting.insert(expression_id) {
        return Ok(false);
    }
    let expression = expression(execution, expression_id)?;
    let result = match &expression.kind {
        SemanticExpressionKind::Source { .. } => true,
        SemanticExpressionKind::Object(fields)
        | SemanticExpressionKind::TaggedObject { fields, .. } => {
            let mut all_resource_only = !fields.is_empty();
            for field in fields {
                if !semantic_initializer_is_resource_only(execution, field.value, visiting)? {
                    all_resource_only = false;
                    break;
                }
            }
            all_resource_only
        }
        SemanticExpressionKind::Block { result, .. } => {
            semantic_initializer_is_resource_only(execution, *result, visiting)?
        }
        SemanticExpressionKind::FlushBoundary { input }
        | SemanticExpressionKind::Draining { input } => {
            semantic_initializer_is_resource_only(execution, *input, visiting)?
        }
        _ => false,
    };
    visiting.remove(&expression_id);
    Ok(result)
}

fn semantic_type_contains_collection_authority(ty: &Type) -> bool {
    match ty {
        Type::List(_) | Type::Map { .. } | Type::Set(_) => true,
        Type::Object(shape) => shape
            .fields
            .values()
            .any(semantic_type_contains_collection_authority),
        Type::VariantSet(variants) => variants.iter().any(|variant| match variant {
            boon_typecheck::Variant::Tag(_) => false,
            boon_typecheck::Variant::Tagged { fields, .. } => fields
                .fields
                .values()
                .any(semantic_type_contains_collection_authority),
        }),
        Type::Union(members) => members
            .iter()
            .any(semantic_type_contains_collection_authority),
        Type::Text
        | Type::Number
        | Type::Bytes(_)
        | Type::Bits { .. }
        | Type::Absent
        | Type::RenderContract
        | Type::Function { .. }
        | Type::UnresolvedShape { .. }
        | Type::Var(_)
        | Type::Unknown => false,
    }
}

fn semantic_initial_value_from_data(value: boon_data::Value) -> SemanticInitialValueV1 {
    match value {
        boon_data::Value::Number(value) => SemanticInitialValueV1::Number { value },
        boon_data::Value::Text(value) => SemanticInitialValueV1::Text { value },
        boon_data::Value::Bytes(bytes) => SemanticInitialValueV1::Bytes {
            fixed_len: Some(bytes.len()),
            bytes: bytes.to_vec(),
        },
        boon_data::Value::Tag { tag, fields } if fields.is_empty() => {
            SemanticInitialValueV1::Tag { name: tag }
        }
        value => SemanticInitialValueV1::Data { value },
    }
}

fn semantic_root_initial_path(
    execution: &SemanticExecutionGraphV1,
    expression_id: SemanticExprId,
) -> Option<String> {
    let mut current = expression_id;
    let mut suffix = Vec::<String>::new();
    let mut visited = BTreeSet::new();
    loop {
        if !visited.insert(current) {
            return None;
        }
        let value = execution
            .expressions
            .get(current.as_usize())
            .filter(|value| value.id == current)?;
        match &value.kind {
            SemanticExpressionKind::CanonicalRead {
                path, projection, ..
            } => {
                let mut parts = path
                    .split('.')
                    .filter(|part| !part.is_empty())
                    .map(ToOwned::to_owned)
                    .collect::<Vec<_>>();
                parts.extend(projection.iter().cloned());
                parts.extend(suffix);
                return Some(parts.join("."));
            }
            SemanticExpressionKind::Project { input, fields } => {
                let mut next = fields.clone();
                next.extend(suffix);
                suffix = next;
                current = *input;
            }
            SemanticExpressionKind::Draining { input } => current = *input,
            SemanticExpressionKind::MatchArm {
                output: Some(output),
                ..
            }
            | SemanticExpressionKind::Then {
                output: Some(output),
                ..
            } => current = *output,
            _ => return None,
        }
    }
}

#[derive(Clone)]
enum StaticTextPart {
    Literal(String),
    Dynamic,
}

#[derive(Clone)]
struct StaticRecordPart {
    name: String,
    spread: bool,
}

enum StaticTask {
    Eval {
        expression: SemanticExprId,
        locals: BTreeMap<SemanticLocalBindingId, boon_data::Value>,
    },
    Exit(SemanticExprId),
    Project(Vec<String>),
    FinishText(Vec<StaticTextPart>),
    FinishRecord(Vec<StaticRecordPart>),
    FinishTagged {
        tag: String,
        fields: Vec<StaticRecordPart>,
    },
    FinishList(usize),
    FinishBytes {
        fixed_size: Option<usize>,
        item_count: usize,
    },
    BlockNext {
        bindings: Vec<SemanticBlockBinding>,
        index: usize,
        locals: BTreeMap<SemanticLocalBindingId, boon_data::Value>,
        result: SemanticExprId,
    },
    BindLocal {
        bindings: Vec<SemanticBlockBinding>,
        index: usize,
        locals: BTreeMap<SemanticLocalBindingId, boon_data::Value>,
        result: SemanticExprId,
    },
}

fn semantic_static_data(
    execution: &SemanticExecutionGraphV1,
    root: SemanticExprId,
) -> Result<boon_data::Value, String> {
    let statement_values = execution
        .statements
        .iter()
        .filter_map(|statement| Some((statement.declaration?, statement.value?)))
        .collect::<BTreeMap<_, _>>();
    let mut tasks = vec![StaticTask::Eval {
        expression: root,
        locals: BTreeMap::new(),
    }];
    let mut active = BTreeSet::new();
    let mut values = Vec::<boon_data::Value>::new();
    while let Some(task) = tasks.pop() {
        match task {
            StaticTask::Exit(expression) => {
                active.remove(&expression);
            }
            StaticTask::Project(projection) => {
                let value = values
                    .pop()
                    .ok_or_else(|| "static projection has no input value".to_owned())?;
                values.push(static_projection(value, &projection)?);
            }
            StaticTask::FinishText(parts) => {
                let dynamic_count = parts
                    .iter()
                    .filter(|part| matches!(part, StaticTextPart::Dynamic))
                    .count();
                let mut dynamic = take_static_values(&mut values, dynamic_count)?.into_iter();
                let mut text = String::new();
                for part in parts {
                    match part {
                        StaticTextPart::Literal(value) => text.push_str(&value),
                        StaticTextPart::Dynamic => {
                            let Some(boon_data::Value::Text(value)) = dynamic.next() else {
                                return Err("text template dynamic expression is not static Text"
                                    .to_owned());
                            };
                            text.push_str(&value);
                        }
                    }
                }
                values.push(boon_data::Value::Text(text));
            }
            StaticTask::FinishRecord(parts) => {
                let field_values = take_static_values(&mut values, parts.len())?;
                values.push(boon_data::Value::Object(finish_static_record(
                    parts,
                    field_values,
                )?));
            }
            StaticTask::FinishTagged { tag, fields } => {
                let field_values = take_static_values(&mut values, fields.len())?;
                values.push(boon_data::Value::Tag {
                    tag,
                    fields: finish_static_record(fields, field_values)?,
                });
            }
            StaticTask::FinishList(count) => {
                let items = take_static_values(&mut values, count)?;
                values.push(boon_data::Value::List(items));
            }
            StaticTask::FinishBytes {
                fixed_size,
                item_count,
            } => {
                let items = take_static_values(&mut values, item_count)?;
                let mut bytes = Vec::new();
                for value in items {
                    let boon_data::Value::Bytes(value) = value else {
                        return Err("BYTES item is not static BYTES".to_owned());
                    };
                    bytes.extend_from_slice(&value);
                }
                if let Some(expected) = fixed_size {
                    if item_count == 0 {
                        bytes.resize(expected, 0);
                    } else if expected != bytes.len() {
                        return Err(format!(
                            "BYTES literal has {} bytes, expected {expected}",
                            bytes.len()
                        ));
                    }
                }
                values.push(boon_data::Value::Bytes(bytes.into()));
            }
            StaticTask::BlockNext {
                bindings,
                index,
                locals,
                result,
            } => {
                if let Some(binding) = bindings.get(index) {
                    tasks.push(StaticTask::BindLocal {
                        bindings: bindings.clone(),
                        index,
                        locals: locals.clone(),
                        result,
                    });
                    tasks.push(StaticTask::Eval {
                        expression: binding.value,
                        locals,
                    });
                } else {
                    tasks.push(StaticTask::Eval {
                        expression: result,
                        locals,
                    });
                }
            }
            StaticTask::BindLocal {
                bindings,
                index,
                mut locals,
                result,
            } => {
                let value = values
                    .pop()
                    .ok_or_else(|| "static BLOCK binding has no value".to_owned())?;
                let binding = bindings
                    .get(index)
                    .ok_or_else(|| "static BLOCK binding index is missing".to_owned())?;
                locals.insert(binding.id, value);
                tasks.push(StaticTask::BlockNext {
                    bindings,
                    index: index + 1,
                    locals,
                    result,
                });
            }
            StaticTask::Eval { expression, locals } => {
                if !active.insert(expression) {
                    return Err(format!("static semantic expression cycle at {expression}"));
                }
                let value = expression_at(execution, expression)?;
                tasks.push(StaticTask::Exit(expression));
                match &value.kind {
                    SemanticExpressionKind::Text(value) => {
                        values.push(boon_data::Value::Text(value.clone()));
                    }
                    SemanticExpressionKind::Number(value) => {
                        values.push(boon_data::Value::Number(value.clone()));
                    }
                    SemanticExpressionKind::Bits(value) => {
                        values.push(boon_data::Value::Bits(value.clone()));
                    }
                    SemanticExpressionKind::BytesByte(value) => {
                        values.push(boon_data::Value::Bytes(boon_data::Bytes::copy_from_slice(
                            &[*value],
                        )));
                    }
                    SemanticExpressionKind::Tag(tag) => {
                        values.push(boon_data::Value::Tag {
                            tag: tag.clone(),
                            fields: BTreeMap::new(),
                        });
                    }
                    SemanticExpressionKind::TextTemplate { segments } => {
                        let parts = segments
                            .iter()
                            .map(|segment| match segment {
                                crate::SemanticTextSegment::Static { value } => {
                                    StaticTextPart::Literal(value.clone())
                                }
                                crate::SemanticTextSegment::Dynamic { .. } => {
                                    StaticTextPart::Dynamic
                                }
                            })
                            .collect::<Vec<_>>();
                        tasks.push(StaticTask::FinishText(parts));
                        for dynamic in segments.iter().rev().filter_map(|segment| match segment {
                            crate::SemanticTextSegment::Static { .. } => None,
                            crate::SemanticTextSegment::Dynamic { value } => Some(*value),
                        }) {
                            tasks.push(StaticTask::Eval {
                                expression: dynamic,
                                locals: locals.clone(),
                            });
                        }
                    }
                    SemanticExpressionKind::Object(fields) => {
                        let parts = fields
                            .iter()
                            .map(|field| StaticRecordPart {
                                name: field.name.clone(),
                                spread: field.spread,
                            })
                            .collect();
                        tasks.push(StaticTask::FinishRecord(parts));
                        for field in fields.iter().rev() {
                            tasks.push(StaticTask::Eval {
                                expression: field.value,
                                locals: locals.clone(),
                            });
                        }
                    }
                    SemanticExpressionKind::TaggedObject { tag, fields } => {
                        let parts = fields
                            .iter()
                            .map(|field| StaticRecordPart {
                                name: field.name.clone(),
                                spread: field.spread,
                            })
                            .collect();
                        tasks.push(StaticTask::FinishTagged {
                            tag: tag.clone(),
                            fields: parts,
                        });
                        for field in fields.iter().rev() {
                            tasks.push(StaticTask::Eval {
                                expression: field.value,
                                locals: locals.clone(),
                            });
                        }
                    }
                    SemanticExpressionKind::List { items, .. } => {
                        tasks.push(StaticTask::FinishList(items.len()));
                        for item in items.iter().rev() {
                            tasks.push(StaticTask::Eval {
                                expression: *item,
                                locals: locals.clone(),
                            });
                        }
                    }
                    SemanticExpressionKind::Bytes { fixed_size, items } => {
                        tasks.push(StaticTask::FinishBytes {
                            fixed_size: *fixed_size,
                            item_count: items.len(),
                        });
                        for item in items.iter().rev() {
                            tasks.push(StaticTask::Eval {
                                expression: *item,
                                locals: locals.clone(),
                            });
                        }
                    }
                    SemanticExpressionKind::Block { bindings, result } => {
                        tasks.push(StaticTask::BlockNext {
                            bindings: bindings.clone(),
                            index: 0,
                            locals,
                            result: *result,
                        });
                    }
                    SemanticExpressionKind::LocalRead {
                        binding,
                        declaration,
                        projection,
                    } => {
                        let value = locals.get(binding).cloned().ok_or_else(|| {
                            format!(
                                "local declaration {} binding {binding} has no static value",
                                declaration.0
                            )
                        })?;
                        values.push(static_projection(value, projection)?);
                    }
                    SemanticExpressionKind::CanonicalRead {
                        target, projection, ..
                    } => {
                        let target_value =
                            statement_values.get(target).copied().ok_or_else(|| {
                                format!("declaration {} has no static semantic value", target.0)
                            })?;
                        tasks.push(StaticTask::Project(projection.clone()));
                        tasks.push(StaticTask::Eval {
                            expression: target_value,
                            locals,
                        });
                    }
                    SemanticExpressionKind::Project { input, fields } => {
                        tasks.push(StaticTask::Project(fields.clone()));
                        tasks.push(StaticTask::Eval {
                            expression: *input,
                            locals,
                        });
                    }
                    SemanticExpressionKind::MatchArm {
                        output: Some(output),
                        ..
                    }
                    | SemanticExpressionKind::Then {
                        output: Some(output),
                        ..
                    } => {
                        tasks.push(StaticTask::Eval {
                            expression: *output,
                            locals,
                        });
                    }
                    other => {
                        return Err(format!(
                            "expression {expression} has non-static semantic shape {other:?}"
                        ));
                    }
                }
            }
        }
    }
    match values.as_slice() {
        [value] => Ok(value.clone()),
        _ => Err(format!(
            "static semantic evaluation produced {} values",
            values.len()
        )),
    }
}

fn expression_at(
    execution: &SemanticExecutionGraphV1,
    id: SemanticExprId,
) -> Result<&crate::SemanticExpression, String> {
    expression(execution, id)
}

fn take_static_values(
    values: &mut Vec<boon_data::Value>,
    count: usize,
) -> Result<Vec<boon_data::Value>, String> {
    let start = values
        .len()
        .checked_sub(count)
        .ok_or_else(|| "static semantic evaluator value stack underflow".to_owned())?;
    Ok(values.drain(start..).collect())
}

fn finish_static_record(
    parts: Vec<StaticRecordPart>,
    values: Vec<boon_data::Value>,
) -> Result<BTreeMap<String, boon_data::Value>, String> {
    let mut result = BTreeMap::new();
    for (part, value) in parts.into_iter().zip(values) {
        if part.spread {
            let boon_data::Value::Object(fields) = value else {
                return Err(format!(
                    "spread field `{}` is not a static object",
                    part.name
                ));
            };
            result.extend(fields);
        } else {
            result.insert(part.name, value);
        }
    }
    Ok(result)
}

fn static_projection(
    mut value: boon_data::Value,
    projection: &[String],
) -> Result<boon_data::Value, String> {
    for field in projection {
        value = match value {
            boon_data::Value::Object(mut fields) | boon_data::Value::Tag { mut fields, .. } => {
                fields
                    .remove(field)
                    .ok_or_else(|| format!("static value has no field `{field}`"))?
            }
            _ => {
                return Err(format!(
                    "cannot project `{field}` from non-object static value"
                ));
            }
        };
    }
    Ok(value)
}

fn inline_list_authority_root(
    execution: &SemanticExecutionGraphV1,
    root: SemanticExprId,
) -> Result<Option<SemanticExprId>, String> {
    let mut next = Some(root);
    let mut visited = BTreeSet::new();
    while let Some(id) = next.take() {
        if !visited.insert(id) {
            return Err(format!(
                "list authority expression {id} contains a semantic cycle"
            ));
        }
        let value = expression(execution, id)?;
        next = match &value.kind {
            SemanticExpressionKind::List { .. } => return Ok(Some(id)),
            SemanticExpressionKind::Call { name, .. } if name == "List/range" => {
                return Ok(Some(id));
            }
            SemanticExpressionKind::Call { arguments, .. }
                if matches!(value.flow_type.ty, Type::List(_)) =>
            {
                let mut inputs = Vec::new();
                for argument in arguments {
                    let input = expression(execution, argument.value)?;
                    if matches!(input.flow_type.ty, Type::List(_)) {
                        inputs.push(argument.value);
                    }
                }
                match inputs.as_slice() {
                    [input] => Some(*input),
                    _ => return Ok(None),
                }
            }
            SemanticExpressionKind::Materialize { materialization } => execution
                .materializations
                .get(materialization.as_usize())
                .filter(|candidate| candidate.id == *materialization)
                .map(|materialization| materialization.source),
            SemanticExpressionKind::Draining { input }
            | SemanticExpressionKind::Project { input, .. } => Some(*input),
            SemanticExpressionKind::Block { result, .. } => Some(*result),
            SemanticExpressionKind::Then {
                output: Some(output),
                ..
            }
            | SemanticExpressionKind::MatchArm {
                output: Some(output),
                ..
            } => Some(*output),
            _ => return Ok(None),
        };
    }
    Ok(None)
}

fn materialization_target_lists(
    execution: &SemanticExecutionGraphV1,
    lists: &[SemanticListResourceV1],
) -> Result<BTreeMap<StaticOwnerId, SemanticListId>, String> {
    let mut targets = BTreeMap::new();
    for list in lists {
        let mut pending = vec![list.producer];
        let mut visited = BTreeSet::new();
        while let Some(id) = pending.pop() {
            if !visited.insert(id) {
                continue;
            }
            let value = expression(execution, id)?;
            if let SemanticExpressionKind::Materialize { materialization } = value.kind {
                let materialization = execution
                    .materializations
                    .get(materialization.as_usize())
                    .filter(|candidate| candidate.id == materialization)
                    .ok_or_else(|| {
                        format!("list target reaches missing materialization {materialization}")
                    })?;
                if let Some(previous) = targets.insert(materialization.owner, list.id)
                    && previous != list.id
                {
                    return Err(format!(
                        "static owner {} ambiguously targets semantic lists {previous} and {}",
                        materialization.owner, list.id
                    ));
                }
                pending.push(materialization.source);
            } else {
                pending.extend(semantic_expression_children(&value.kind));
            }
        }
    }
    let materialization_owners = execution
        .materializations
        .iter()
        .map(|materialization| materialization.owner)
        .collect::<BTreeSet<_>>();
    let mut changed = true;
    let mut remaining = execution.static_owners.len().saturating_add(1);
    while changed {
        if remaining == 0 {
            return Err("semantic owner target inheritance failed to converge".to_owned());
        }
        remaining -= 1;
        changed = false;
        for owner in &execution.static_owners {
            if targets.contains_key(&owner.id) || materialization_owners.contains(&owner.id) {
                continue;
            }
            let Some(parent) = owner.parent else {
                continue;
            };
            if let Some(inherited) = targets.get(&parent).copied() {
                targets.insert(owner.id, inherited);
                changed = true;
            }
        }
    }
    Ok(targets)
}

fn bind_materialization_targets(
    execution: &mut SemanticExecutionGraphV1,
    lists: &[SemanticListResourceV1],
    targets: &BTreeMap<StaticOwnerId, SemanticListId>,
) -> Result<(), String> {
    for materialization in &mut execution.materializations {
        let Some(list_id) = targets.get(&materialization.owner).copied() else {
            continue;
        };
        let list = list_resource(lists, list_id)?;
        materialization.target_list_id = Some(list_id);
        materialization.target_scope_id = Some(list.row_scope);
    }
    Ok(())
}

fn bind_materialization_sources(
    execution: &mut SemanticExecutionGraphV1,
    lists: &[SemanticListResourceV1],
) -> Result<(), String> {
    let storage_scopes = semantic_storage_scopes(execution, lists)?;
    let mut scope_lists = BTreeMap::new();
    for list in lists {
        if let Some(previous) = scope_lists.insert(list.row_scope, list.id)
            && previous != list.id
        {
            return Err(format!(
                "semantic row scope {} belongs to both lists {previous} and {}",
                list.row_scope, list.id
            ));
        }
    }
    let snapshot = execution.materializations.clone();
    let mut bindings = Vec::with_capacity(snapshot.len());
    for materialization in &snapshot {
        let candidates = storage_scopes
            .get(materialization.source.as_usize())
            .filter(|candidate| {
                execution
                    .expressions
                    .get(materialization.source.as_usize())
                    .is_some_and(|expression| expression.id == materialization.source)
                    && !candidate.is_empty()
            })
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .collect::<Vec<_>>();
        let mut list = match candidates.as_slice() {
            [] => None,
            [scope] => Some(*scope_lists.get(scope).ok_or_else(|| {
                format!(
                    "contextual materialization {} source reaches unowned semantic row scope {scope}",
                    materialization.id
                )
            })?),
            _ => {
                return Err(format!(
                    "contextual materialization {} source reaches ambiguous semantic row scopes {candidates:?}",
                    materialization.id
                ));
            }
        };
        if list.is_none()
            && (inline_list_authority_root(execution, materialization.source)?.is_some()
                || matches!(
                    materialization.operation,
                    SemanticContextualOperationKind::Filter
                        | SemanticContextualOperationKind::Retain
                        | SemanticContextualOperationKind::Remove
                        | SemanticContextualOperationKind::SortBy
                        | SemanticContextualOperationKind::ThenBy
                ))
        {
            list = materialization.target_list_id;
        }
        let scope = list
            .map(|list| list_resource(lists, list).map(|list| list.row_scope))
            .transpose()?;
        bindings.push((list, scope));
    }
    for (materialization, (list, scope)) in execution.materializations.iter_mut().zip(bindings) {
        materialization.source_list_id = list;
        materialization.source_scope_id = scope;
    }
    Ok(())
}

/// Resolve the runtime row-storage scope of every semantic expression without
/// recursion. This is the semantic equivalent of the legacy
/// `ContextualStorageScopeResolver`: canonical reads and drains prefer exact
/// projected paths, materialization locals resolve through their typed
/// materialization, and only the special `.items` projection crosses a
/// `List/chunk` row boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct StatementValueOccurrence {
    statement: SemanticStatementId,
    value: SemanticExprId,
    owner: Option<StaticOwnerId>,
    call_instance: Option<OutCallInstanceId>,
    producer_result: bool,
}

fn statement_value_occurrences(
    execution: &SemanticExecutionGraphV1,
) -> Result<BTreeMap<DeclId, Vec<StatementValueOccurrence>>, String> {
    let mut values = BTreeMap::<DeclId, Vec<StatementValueOccurrence>>::new();
    for statement in &execution.statements {
        let (Some(declaration), Some(value)) = (statement.declaration, statement.value) else {
            continue;
        };
        let expression = expression(execution, value)?;
        let origin = execution
            .checked_expression_origins
            .get(value.as_usize())
            .filter(|origin| origin.expression == value)
            .ok_or_else(|| {
                format!(
                    "semantic statement {} value {value} has no exact expression origin",
                    statement.id
                )
            })?;
        if statement.call_instance.is_some()
            && origin.call_instance.is_some()
            && statement.call_instance != origin.call_instance
        {
            return Err(format!(
                "semantic statement {} and value {value} disagree on call occurrence",
                statement.id
            ));
        }
        values
            .entry(declaration)
            .or_default()
            .push(StatementValueOccurrence {
                statement: statement.id,
                value,
                owner: expression.owner,
                call_instance: statement.call_instance.or(origin.call_instance),
                producer_result: matches!(
                    statement.origin,
                    SemanticStatementOrigin::ProducerResult { .. }
                ),
            });
    }
    for occurrences in values.values_mut() {
        occurrences.sort_by_key(|occurrence| {
            (
                occurrence.owner,
                occurrence.call_instance,
                occurrence.producer_result,
                occurrence.statement,
            )
        });
    }
    Ok(values)
}

fn statement_value_for_expression(
    execution: &SemanticExecutionGraphV1,
    values: &BTreeMap<DeclId, Vec<StatementValueOccurrence>>,
    declaration: DeclId,
    consumer: SemanticExprId,
) -> Result<Option<SemanticExprId>, String> {
    let consumer_expression = expression(execution, consumer)?;
    let origin = execution
        .checked_expression_origins
        .get(consumer.as_usize())
        .filter(|origin| origin.expression == consumer)
        .ok_or_else(|| format!("semantic expression {consumer} has no exact occurrence origin"))?;
    let Some(occurrences) = values.get(&declaration) else {
        return Ok(None);
    };
    let mut exact = occurrences
        .iter()
        .filter(|occurrence| {
            occurrence.owner == consumer_expression.owner
                && occurrence.call_instance == origin.call_instance
        })
        .collect::<Vec<_>>();
    if exact.is_empty() {
        exact.extend(
            occurrences.iter().filter(|occurrence| {
                occurrence.owner.is_none() && occurrence.call_instance.is_none()
            }),
        );
    }
    exact.sort_by_key(|occurrence| (occurrence.producer_result, occurrence.statement));
    let Some(preferred) = exact.first().copied() else {
        return Ok(None);
    };
    let same_rank = exact
        .iter()
        .take_while(|candidate| candidate.producer_result == preferred.producer_result)
        .map(|candidate| candidate.value)
        .collect::<BTreeSet<_>>();
    if same_rank.len() != 1 {
        return Err(format!(
            "semantic declaration {} expression {consumer} resolves to {} exact occurrence values {same_rank:?}",
            declaration.0,
            same_rank.len()
        ));
    }
    Ok(Some(preferred.value))
}

fn semantic_storage_scopes(
    execution: &SemanticExecutionGraphV1,
    lists: &[SemanticListResourceV1],
) -> Result<Vec<BTreeSet<SemanticRowScopeId>>, String> {
    let mut scopes_by_declaration = BTreeMap::new();
    let mut scopes_by_path = BTreeMap::new();
    let mut declarations_by_scope = BTreeMap::<SemanticRowScopeId, Vec<DeclId>>::new();
    for list in lists {
        if let Some(previous) = scopes_by_declaration.insert(list.declaration, list.row_scope)
            && previous != list.row_scope
        {
            return Err(format!(
                "semantic list declaration {} resolves to both row scopes {previous} and {}",
                list.declaration.0, list.row_scope
            ));
        }
        if let Some(previous) = scopes_by_path.insert(list.semantic_path.clone(), list.row_scope)
            && previous != list.row_scope
        {
            return Err(format!(
                "semantic list path `{}` resolves to both row scopes {previous} and {}",
                list.semantic_path, list.row_scope
            ));
        }
        declarations_by_scope
            .entry(list.row_scope)
            .or_default()
            .push(list.declaration);
    }

    let statement_values = statement_value_occurrences(execution)?;

    let mut materializations_by_local = BTreeMap::new();
    for materialization in &execution.materializations {
        let key = (materialization.owner, materialization.row_local);
        if let Some(previous) = materializations_by_local.insert(key, materialization.id)
            && previous != materialization.id
        {
            return Err(format!(
                "semantic contextual local {}:{} belongs to materializations {previous} and {}",
                materialization.owner, materialization.row_local, materialization.id
            ));
        }
    }

    let mut scopes = vec![BTreeSet::new(); execution.expressions.len()];
    let maximum_insertions = execution
        .expressions
        .len()
        .saturating_mul(lists.len().max(1));
    let mut remaining_insertions = maximum_insertions;
    loop {
        let snapshot = scopes.clone();
        let mut changed = false;
        for expression in &execution.expressions {
            let resolved = semantic_expression_storage_scopes(
                execution,
                expression.id,
                &snapshot,
                &statement_values,
                &scopes_by_declaration,
                &scopes_by_path,
                &declarations_by_scope,
                &materializations_by_local,
            )?;
            let current = scopes
                .get_mut(expression.id.as_usize())
                .ok_or_else(|| format!("missing semantic expression {}", expression.id))?;
            for scope in resolved {
                if current.insert(scope) {
                    if remaining_insertions == 0 {
                        return Err(
                            "semantic storage-scope resolution exceeded its bounded fixed-point budget"
                                .to_owned(),
                        );
                    }
                    remaining_insertions -= 1;
                    changed = true;
                }
            }
        }
        if !changed {
            break;
        }
    }
    Ok(scopes)
}

#[allow(clippy::too_many_arguments)]
fn semantic_expression_storage_scopes(
    execution: &SemanticExecutionGraphV1,
    id: SemanticExprId,
    scopes: &[BTreeSet<SemanticRowScopeId>],
    statement_values: &BTreeMap<DeclId, Vec<StatementValueOccurrence>>,
    scopes_by_declaration: &BTreeMap<DeclId, SemanticRowScopeId>,
    scopes_by_path: &BTreeMap<String, SemanticRowScopeId>,
    declarations_by_scope: &BTreeMap<SemanticRowScopeId, Vec<DeclId>>,
    materializations_by_local: &BTreeMap<
        (StaticOwnerId, crate::SemanticMaterializationLocalId),
        SemanticMaterializationId,
    >,
) -> Result<BTreeSet<SemanticRowScopeId>, String> {
    let value = expression(execution, id)?;
    let child_scopes = |child: SemanticExprId| -> Result<BTreeSet<SemanticRowScopeId>, String> {
        scopes
            .get(child.as_usize())
            .filter(|_| {
                execution
                    .expressions
                    .get(child.as_usize())
                    .is_some_and(|candidate| candidate.id == child)
            })
            .cloned()
            .ok_or_else(|| format!("storage-scope expression {id} reaches missing child {child}"))
    };
    let resolve_canonical = |target: DeclId,
                             path: &str,
                             projection: &[String]|
     -> Result<BTreeSet<SemanticRowScopeId>, String> {
        let projected_path = semantic_canonical_read_path(path, projection);
        if let Some(scope) = scopes_by_path.get(&projected_path) {
            return Ok(BTreeSet::from([*scope]));
        }
        if let Some(scope) = scopes_by_declaration.get(&target) {
            return Ok(BTreeSet::from([*scope]));
        }
        statement_value_for_expression(execution, statement_values, target, id)?
            .map(&child_scopes)
            .transpose()
            .map(Option::unwrap_or_default)
    };
    let resolve_chunk_items =
        |chunk_scopes: BTreeSet<SemanticRowScopeId>|
         -> Result<BTreeSet<SemanticRowScopeId>, String> {
            if chunk_scopes.len() > 1 {
                return Err(format!(
                    "semantic expression {id} reaches ambiguous chunk row scopes {chunk_scopes:?}"
                ));
            }
            let Some(chunk_scope) = chunk_scopes.into_iter().next() else {
                return Ok(BTreeSet::new());
            };
            let declarations = declarations_by_scope
                .get(&chunk_scope)
                .cloned()
                .unwrap_or_default();
            let [declaration] = declarations.as_slice() else {
                return if declarations.is_empty() {
                    Ok(BTreeSet::new())
                } else {
                    Err(format!(
                        "semantic row scope {chunk_scope} belongs to multiple typed list declarations {declarations:?}"
                    ))
                };
            };
            let Some(producer) =
                statement_value_for_expression(execution, statement_values, *declaration, id)?
            else {
                return Ok(BTreeSet::new());
            };
            let SemanticExpressionKind::Call {
                name, arguments, ..
            } = &expression(execution, producer)?.kind
            else {
                return Ok(BTreeSet::new());
            };
            if name != "List/chunk" {
                return Ok(BTreeSet::new());
            }
            let source = call_argument(arguments, "list").ok_or_else(|| {
                format!(
                    "typed List/chunk producer {producer} has no canonical `list` argument"
                )
            })?;
            child_scopes(source)
        };
    let union_children =
        |children: Vec<SemanticExprId>| -> Result<BTreeSet<SemanticRowScopeId>, String> {
            let mut resolved = BTreeSet::new();
            for child in children {
                resolved.extend(child_scopes(child)?);
            }
            Ok(resolved)
        };

    match &value.kind {
        SemanticExpressionKind::CanonicalRead {
            target,
            path,
            projection,
            ..
        }
        | SemanticExpressionKind::Drain {
            target,
            path,
            projection,
        } => resolve_canonical(*target, path, projection),
        SemanticExpressionKind::Materialize { materialization } => {
            let source = execution
                .materializations
                .get(materialization.as_usize())
                .filter(|candidate| candidate.id == *materialization)
                .map(|definition| definition.source)
                .ok_or_else(|| {
                    format!(
                        "storage-scope expression {id} references missing materialization {materialization}"
                    )
                })?;
            child_scopes(source)
        }
        SemanticExpressionKind::MaterializationLocal {
            owner,
            local,
            projection,
        } => {
            let materialization = materializations_by_local
                .get(&(*owner, *local))
                .copied()
                .ok_or_else(|| {
                    format!("contextual owner {owner} local {local} has no typed materialization")
                })?;
            let source = execution
                .materializations
                .get(materialization.as_usize())
                .filter(|candidate| candidate.id == materialization)
                .map(|definition| definition.source)
                .ok_or_else(|| {
                    format!(
                        "contextual owner {owner} local {local} references missing materialization {materialization}"
                    )
                })?;
            match projection.first().map(String::as_str) {
                None => child_scopes(source),
                Some("items") => resolve_chunk_items(child_scopes(source)?),
                Some(_) => Ok(BTreeSet::new()),
            }
        }
        SemanticExpressionKind::Project { input, fields } => {
            match fields.first().map(String::as_str) {
                None => child_scopes(*input),
                Some("items") => resolve_chunk_items(child_scopes(*input)?),
                Some(_) => Ok(BTreeSet::new()),
            }
        }
        SemanticExpressionKind::Draining { input } => child_scopes(*input),
        SemanticExpressionKind::Call { arguments, .. } => {
            union_children(arguments.iter().map(|argument| argument.value).collect())
        }
        SemanticExpressionKind::Then { input, output } => {
            union_children(std::iter::once(*input).chain(*output).collect())
        }
        SemanticExpressionKind::Latest { branches } => union_children(branches.clone()),
        _ => Ok(BTreeSet::new()),
    }
}

fn semantic_canonical_read_path(path: &str, projection: &[String]) -> String {
    if projection.is_empty() {
        path.to_owned()
    } else {
        format!("{path}.{}", projection.join("."))
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum LineageLeaf {
    Value,
    Stored(SemanticRowBinding),
    Materialized(SemanticMaterializationId),
    Provenance(SemanticMaterializationId),
}

fn bind_materialization_lineage(
    execution: &mut SemanticExecutionGraphV1,
    lists: &[SemanticListResourceV1],
) -> Result<(), String> {
    let statement_values = execution
        .statements
        .iter()
        .filter_map(|statement| Some((statement.declaration?, statement.value?)))
        .collect::<BTreeMap<_, _>>();
    let statement_rows = lists
        .iter()
        .map(|list| {
            (
                list.declaration,
                SemanticRowBinding {
                    list: list.id,
                    scope: list.row_scope,
                },
            )
        })
        .collect::<BTreeMap<_, _>>();
    let locals = semantic_local_values(execution)?;
    let rows = execution
        .materializations
        .iter()
        .map(|materialization| {
            Ok((
                paired_row_binding(
                    materialization.source_list_id,
                    materialization.source_scope_id,
                    "source",
                    materialization.owner,
                )?,
                paired_row_binding(
                    materialization.target_list_id,
                    materialization.target_scope_id,
                    "target",
                    materialization.owner,
                )?,
            ))
        })
        .collect::<Result<Vec<_>, String>>()?;
    let operations = execution
        .materializations
        .iter()
        .map(|materialization| materialization.operation)
        .collect::<Vec<_>>();
    let snapshot = execution.materializations.clone();
    let mut resolved = Vec::with_capacity(snapshot.len());
    for materialization in &snapshot {
        let leaves = collect_lineage_leaves(
            execution,
            materialization.source,
            &statement_values,
            &statement_rows,
            &locals,
        )?;
        let source_row = rows
            .get(materialization.id.as_usize())
            .map(|(source, _)| *source)
            .ok_or_else(|| {
                format!(
                    "materialization {} has no semantic row contract",
                    materialization.id
                )
            })?;
        let mut predecessors = Vec::new();
        for leaf in leaves {
            match leaf {
                LineageLeaf::Value => predecessors.push(match source_row {
                    Some(row) => SemanticContextualRowPredecessor::Stored { row },
                    None => SemanticContextualRowPredecessor::Value,
                }),
                LineageLeaf::Stored(row) => {
                    if source_row != Some(row) {
                        return Err(format!(
                            "materialization {} source row {source_row:?} differs from stored predecessor {row:?}",
                            materialization.id
                        ));
                    }
                    predecessors.push(SemanticContextualRowPredecessor::Stored { row });
                }
                LineageLeaf::Materialized(input) => {
                    if input == materialization.id {
                        return Err(format!(
                            "materialization {} directly consumes its own output",
                            materialization.id
                        ));
                    }
                    let input_index = input.as_usize();
                    let input_operation =
                        operations.get(input_index).copied().ok_or_else(|| {
                            format!(
                                "materialization {} references missing input {input}",
                                materialization.id
                            )
                        })?;
                    let (input_source, input_target) =
                        rows.get(input_index).copied().ok_or_else(|| {
                            format!("input materialization {input} has no semantic row contract")
                        })?;
                    let compatible = match input_operation {
                        SemanticContextualOperationKind::Map => match source_row {
                            Some(row) => input_target.or(input_source) == Some(row),
                            None => input_source.is_none() && input_target.is_none(),
                        },
                        SemanticContextualOperationKind::Filter
                        | SemanticContextualOperationKind::Retain
                        | SemanticContextualOperationKind::Remove
                        | SemanticContextualOperationKind::SortBy
                        | SemanticContextualOperationKind::ThenBy => match source_row {
                            Some(row) => input_source == Some(row) || input_target == Some(row),
                            None => input_source.is_none() && input_target.is_none(),
                        },
                        SemanticContextualOperationKind::Every
                        | SemanticContextualOperationKind::Any
                        | SemanticContextualOperationKind::Find => false,
                    };
                    if !compatible {
                        return Err(format!(
                            "materialization {} source row {source_row:?} is incompatible with input {input} {input_operation:?} rows {input_source:?}/{input_target:?}",
                            materialization.id
                        ));
                    }
                    predecessors.push(SemanticContextualRowPredecessor::Materialized {
                        materialization: input,
                    });
                }
                LineageLeaf::Provenance(input) => {
                    if input.as_usize() >= snapshot.len() {
                        return Err(format!(
                            "materialization {} references missing provenance input {input}",
                            materialization.id
                        ));
                    }
                    predecessors.push(SemanticContextualRowPredecessor::Provenance {
                        materialization: input,
                    });
                }
            }
        }
        if predecessors.is_empty() {
            return Err(format!(
                "materialization {} has no exact source-row predecessor",
                materialization.id
            ));
        }
        predecessors.sort();
        predecessors.dedup();
        resolved.push(predecessors);
    }
    for (materialization, predecessors) in execution.materializations.iter_mut().zip(resolved) {
        materialization.source_row_predecessors = predecessors;
    }
    verify_semantic_materialization_lineage(&execution.materializations)
}

fn bind_list_lineage(
    execution: &SemanticExecutionGraphV1,
    lists: &mut [SemanticListResourceV1],
) -> Result<(), String> {
    let statement_values = execution
        .statements
        .iter()
        .filter_map(|statement| Some((statement.declaration?, statement.value?)))
        .collect::<BTreeMap<_, _>>();
    let statement_rows = lists
        .iter()
        .map(|list| {
            (
                list.declaration,
                SemanticRowBinding {
                    list: list.id,
                    scope: list.row_scope,
                },
            )
        })
        .collect::<BTreeMap<_, _>>();
    let locals = semantic_local_values(execution)?;
    let mut resolved = Vec::with_capacity(lists.len());
    for list in lists.iter() {
        let predecessors = collect_lineage_leaves(
            execution,
            list.producer,
            &statement_values,
            &statement_rows,
            &locals,
        )?
        .into_iter()
        .map(|leaf| match leaf {
            LineageLeaf::Value => SemanticContextualRowPredecessor::Value,
            LineageLeaf::Stored(row) => SemanticContextualRowPredecessor::Stored { row },
            LineageLeaf::Materialized(materialization) => {
                SemanticContextualRowPredecessor::Materialized { materialization }
            }
            LineageLeaf::Provenance(materialization) => {
                SemanticContextualRowPredecessor::Provenance { materialization }
            }
        })
        .collect::<Vec<_>>();
        if predecessors.is_empty() {
            return Err(format!(
                "list {} `{}` has no exact row predecessor",
                list.id, list.semantic_path
            ));
        }
        resolved.push(predecessors);
    }
    for (list, predecessors) in lists.iter_mut().zip(resolved) {
        list.row_predecessors = predecessors;
    }
    Ok(())
}

fn validate_list_lineage(
    graph: &SemanticResourceGraphV1,
    execution: &SemanticExecutionGraphV1,
) -> Result<(), String> {
    let mut expected = graph.lists.clone();
    bind_list_lineage(execution, &mut expected)?;
    for (list, expected) in graph.lists.iter().zip(expected) {
        if list.row_predecessors != expected.row_predecessors {
            return Err(format!(
                "list {} row lineage differs from its exact semantic producer",
                list.id
            ));
        }
    }
    Ok(())
}

fn collect_lineage_leaves(
    execution: &SemanticExecutionGraphV1,
    root: SemanticExprId,
    statement_values: &BTreeMap<DeclId, SemanticExprId>,
    statement_rows: &BTreeMap<DeclId, SemanticRowBinding>,
    locals: &BTreeMap<SemanticLocalBindingId, (DeclId, SemanticExprId)>,
) -> Result<BTreeSet<LineageLeaf>, String> {
    let mut leaves = BTreeSet::new();
    let mut pending = vec![(root, Vec::<String>::new(), true)];
    let mut visited = BTreeSet::new();
    while let Some((id, projection, row_identity)) = pending.pop() {
        if !visited.insert((id, projection.clone(), row_identity)) {
            continue;
        }
        let value = expression(execution, id)?;
        if !projection.is_empty() {
            match &value.kind {
                SemanticExpressionKind::CanonicalRead {
                    target,
                    projection: read_projection,
                    ..
                } => {
                    let mut combined = read_projection.clone();
                    combined.extend(projection);
                    if let Some(value) = statement_values.get(target) {
                        pending.push((*value, combined, row_identity));
                    } else if row_identity {
                        leaves.insert(LineageLeaf::Value);
                    }
                }
                SemanticExpressionKind::LocalRead {
                    binding,
                    declaration,
                    projection: read_projection,
                } => {
                    let (local_declaration, local_value) =
                        locals.get(binding).ok_or_else(|| {
                            format!("lineage references missing local binding {binding}")
                        })?;
                    if local_declaration != declaration {
                        return Err(format!(
                            "lineage local {binding} declaration {} differs from {}",
                            local_declaration.0, declaration.0
                        ));
                    }
                    let mut combined = read_projection.clone();
                    combined.extend(projection);
                    pending.push((*local_value, combined, row_identity));
                }
                SemanticExpressionKind::Project { input, fields } => {
                    let mut combined = fields.clone();
                    combined.extend(projection);
                    pending.push((*input, combined, row_identity));
                }
                SemanticExpressionKind::Object(_) | SemanticExpressionKind::TaggedObject { .. } => {
                    let (projected, remaining) =
                        exact_semantic_record_projection(execution, id, &projection)?;
                    if projected == id {
                        return Err(format!(
                            "materialization lineage expression {id} has no exact field `{}`",
                            projection[0]
                        ));
                    }
                    pending.push((projected, remaining, row_identity));
                }
                SemanticExpressionKind::Flush { payload: input }
                | SemanticExpressionKind::FlushBoundary { input }
                | SemanticExpressionKind::Draining { input } => {
                    pending.push((*input, projection, row_identity));
                }
                SemanticExpressionKind::Block { result, .. } => {
                    pending.push((*result, projection, row_identity));
                }
                SemanticExpressionKind::Latest { branches } => {
                    pending.extend(
                        branches
                            .iter()
                            .map(|branch| (*branch, projection.clone(), row_identity)),
                    );
                }
                SemanticExpressionKind::When { arms, .. } => {
                    pending.extend(
                        arms.iter()
                            .map(|arm| (arm.output, projection.clone(), row_identity)),
                    );
                }
                SemanticExpressionKind::Then { output, .. }
                | SemanticExpressionKind::MatchArm { output, .. } => {
                    let output = output.ok_or_else(|| {
                        format!("materialization lineage expression {id} has no projected output")
                    })?;
                    pending.push((output, projection, row_identity));
                }
                SemanticExpressionKind::Source { .. }
                | SemanticExpressionKind::Call { .. }
                | SemanticExpressionKind::Hold { .. }
                | SemanticExpressionKind::ExternalRead { .. }
                | SemanticExpressionKind::Drain { .. }
                | SemanticExpressionKind::FunctionParameter { .. }
                | SemanticExpressionKind::MaterializationLocal { .. }
                | SemanticExpressionKind::ElementState { .. } => {
                    if row_identity {
                        leaves.insert(LineageLeaf::Value);
                    }
                }
                SemanticExpressionKind::Materialize { materialization } => {
                    if matches!(value.flow_type.ty, Type::List(_)) {
                        return Err(format!(
                            "materialization lineage cannot project `{}` from list-valued materialization {materialization}",
                            projection.join(".")
                        ));
                    }
                    if row_identity {
                        leaves.insert(LineageLeaf::Value);
                    }
                }
                SemanticExpressionKind::Text(_)
                | SemanticExpressionKind::TextTemplate { .. }
                | SemanticExpressionKind::Number(_)
                | SemanticExpressionKind::Bits(_)
                | SemanticExpressionKind::BytesByte(_)
                | SemanticExpressionKind::Absent
                | SemanticExpressionKind::Tag(_)
                | SemanticExpressionKind::Infix { .. }
                | SemanticExpressionKind::List { .. }
                | SemanticExpressionKind::MapEntry { .. }
                | SemanticExpressionKind::Map { .. }
                | SemanticExpressionKind::Set { .. }
                | SemanticExpressionKind::Bytes { .. }
                | SemanticExpressionKind::Delimiter => {
                    return Err(format!(
                        "materialization lineage cannot project `{}` from expression {id}: {:?}",
                        projection.join("."),
                        value.kind
                    ));
                }
            }
            continue;
        }
        match &value.kind {
            SemanticExpressionKind::Absent => {}
            SemanticExpressionKind::Materialize { materialization } => {
                leaves.insert(if row_identity {
                    LineageLeaf::Materialized(*materialization)
                } else {
                    LineageLeaf::Provenance(*materialization)
                });
            }
            SemanticExpressionKind::CanonicalRead {
                target, projection, ..
            } => {
                if projection.is_empty()
                    && let Some(row) = statement_rows.get(target)
                {
                    if row_identity {
                        leaves.insert(LineageLeaf::Stored(*row));
                    }
                    if let Some(value) = statement_values.get(target) {
                        pending.push((*value, Vec::new(), false));
                    }
                } else if let Some(value) = statement_values.get(target) {
                    pending.push((*value, projection.clone(), row_identity));
                } else if row_identity {
                    leaves.insert(LineageLeaf::Value);
                }
            }
            SemanticExpressionKind::LocalRead {
                binding,
                declaration,
                ..
            } => {
                let (local_declaration, value) = locals
                    .get(binding)
                    .ok_or_else(|| format!("lineage references missing local binding {binding}"))?;
                if local_declaration != declaration {
                    return Err(format!(
                        "lineage local {binding} declaration {} differs from {}",
                        local_declaration.0, declaration.0
                    ));
                }
                let read_projection = match &expression(execution, id)?.kind {
                    SemanticExpressionKind::LocalRead { projection, .. } => projection.clone(),
                    _ => unreachable!("matched local read"),
                };
                pending.push((*value, read_projection, row_identity));
            }
            SemanticExpressionKind::Project { input, fields } => {
                pending.push((*input, fields.clone(), row_identity));
            }
            SemanticExpressionKind::Flush { payload: input }
            | SemanticExpressionKind::FlushBoundary { input }
            | SemanticExpressionKind::Draining { input } => {
                pending.push((*input, Vec::new(), row_identity));
            }
            SemanticExpressionKind::Block { result, .. } => {
                pending.push((*result, Vec::new(), row_identity));
            }
            SemanticExpressionKind::Then {
                output: Some(output),
                ..
            }
            | SemanticExpressionKind::MatchArm {
                output: Some(output),
                ..
            } => pending.push((*output, Vec::new(), row_identity)),
            SemanticExpressionKind::Then { output: None, .. }
            | SemanticExpressionKind::MatchArm { output: None, .. } => {
                return Err(format!(
                    "materialization lineage expression {id} has no list-valued output"
                ));
            }
            SemanticExpressionKind::Latest { branches } => {
                pending.extend(
                    branches
                        .iter()
                        .map(|branch| (*branch, Vec::new(), row_identity)),
                );
            }
            SemanticExpressionKind::When { arms, .. } => {
                pending.extend(
                    arms.iter()
                        .map(|arm| (arm.output, Vec::new(), row_identity)),
                );
            }
            SemanticExpressionKind::Call { .. } if matches!(value.flow_type.ty, Type::List(_)) => {
                if row_identity {
                    leaves.insert(LineageLeaf::Value);
                }
            }
            SemanticExpressionKind::ExternalRead { .. }
            | SemanticExpressionKind::Drain { .. }
            | SemanticExpressionKind::List { .. }
            | SemanticExpressionKind::Hold { .. }
            | SemanticExpressionKind::FunctionParameter { .. }
            | SemanticExpressionKind::MaterializationLocal { .. } => {
                if row_identity {
                    leaves.insert(LineageLeaf::Value);
                }
            }
            SemanticExpressionKind::ElementState { .. }
            | SemanticExpressionKind::Text(_)
            | SemanticExpressionKind::TextTemplate { .. }
            | SemanticExpressionKind::Number(_)
            | SemanticExpressionKind::Bits(_)
            | SemanticExpressionKind::BytesByte(_)
            | SemanticExpressionKind::Tag(_)
            | SemanticExpressionKind::TaggedObject { .. }
            | SemanticExpressionKind::Source { .. }
            | SemanticExpressionKind::Call { .. }
            | SemanticExpressionKind::Infix { .. }
            | SemanticExpressionKind::Object(_)
            | SemanticExpressionKind::MapEntry { .. }
            | SemanticExpressionKind::Map { .. }
            | SemanticExpressionKind::Set { .. }
            | SemanticExpressionKind::Bytes { .. }
            | SemanticExpressionKind::Delimiter => {
                return Err(format!(
                    "materialization lineage reaches non-list expression {id}: {:?}",
                    value.kind
                ));
            }
        }
    }
    Ok(leaves)
}

fn verify_semantic_materialization_lineage(
    materializations: &[crate::SemanticContextualMaterialization],
) -> Result<(), String> {
    let mut colors = vec![0_u8; materializations.len()];
    for start in 0..materializations.len() {
        if colors[start] == 2 {
            continue;
        }
        let mut pending = vec![(SemanticMaterializationId(start), false)];
        while let Some((id, exiting)) = pending.pop() {
            let index = id.as_usize();
            let definition = materializations
                .get(index)
                .filter(|candidate| candidate.id == id)
                .ok_or_else(|| format!("missing semantic materialization {id}"))?;
            if exiting {
                colors[index] = 2;
                continue;
            }
            match colors[index] {
                2 => continue,
                1 => {
                    return Err(format!(
                        "semantic materialization lineage contains a cycle through {id}"
                    ));
                }
                _ => {}
            }
            colors[index] = 1;
            pending.push((id, true));
            for predecessor in definition.source_row_predecessors.iter().rev() {
                let input = match predecessor {
                    SemanticContextualRowPredecessor::Materialized { materialization }
                    | SemanticContextualRowPredecessor::Provenance { materialization } => {
                        *materialization
                    }
                    SemanticContextualRowPredecessor::Value
                    | SemanticContextualRowPredecessor::Stored { .. } => continue,
                };
                let input_index = input.as_usize();
                if materializations
                    .get(input_index)
                    .is_none_or(|candidate| candidate.id != input)
                {
                    return Err(format!(
                        "semantic materialization {id} references missing lineage input {input}"
                    ));
                }
                if colors[input_index] == 1 {
                    return Err(format!(
                        "semantic materialization lineage contains a cycle through {input}"
                    ));
                }
                if colors[input_index] != 2 {
                    pending.push((input, false));
                }
            }
        }
    }
    Ok(())
}

fn exact_semantic_record_projection(
    execution: &SemanticExecutionGraphV1,
    mut expression_id: SemanticExprId,
    projection: &[String],
) -> Result<(SemanticExprId, Vec<String>), String> {
    let mut consumed = 0usize;
    while let Some(field_name) = projection.get(consumed) {
        let value = expression(execution, expression_id)?;
        let fields = match &value.kind {
            SemanticExpressionKind::Object(fields)
            | SemanticExpressionKind::TaggedObject { fields, .. } => fields,
            _ => break,
        };
        let matches = fields
            .iter()
            .filter(|field| !field.spread && field.name == *field_name)
            .collect::<Vec<_>>();
        let field = match matches.as_slice() {
            [] => break,
            [field] => *field,
            _ => {
                return Err(format!(
                    "semantic expression {expression_id} projection `{field_name}` resolves to {} fields",
                    matches.len()
                ));
            }
        };
        expression_id = field.value;
        consumed += 1;
    }
    Ok((expression_id, projection[consumed..].to_vec()))
}

fn validate_checked_source_statement(
    checked: &CheckedProgram,
    id: CheckedSourceId,
    source: &boon_typecheck::CheckedSource,
) -> Result<(), String> {
    let statement = checked
        .statements
        .iter()
        .find(|candidate| candidate.id == source.statement)
        .ok_or_else(|| {
            format!(
                "checked source {} references missing checked statement {}",
                id.0, source.statement.0
            )
        })?;
    let bindings = statement
        .resources
        .iter()
        .filter(
            |binding| matches!(binding, CheckedResourceBinding::Source { source } if *source == id),
        )
        .count();
    if bindings != 1 {
        return Err(format!(
            "checked source {} statement {} contains {bindings} exact source bindings",
            id.0, source.statement.0
        ));
    }
    Ok(())
}

fn validate_checked_state_statement(
    checked: &CheckedProgram,
    id: CheckedStateId,
    state: &boon_typecheck::CheckedState,
) -> Result<(), String> {
    let statement = checked
        .statements
        .iter()
        .find(|candidate| candidate.id == state.statement)
        .ok_or_else(|| {
            format!(
                "checked state {} references missing checked statement {}",
                id.0, state.statement.0
            )
        })?;
    let bindings = statement
        .resources
        .iter()
        .filter(
            |binding| matches!(binding, CheckedResourceBinding::State { state } if *state == id),
        )
        .count();
    if bindings != 1 {
        return Err(format!(
            "checked state {} statement {} contains {bindings} exact state bindings",
            id.0, state.statement.0
        ));
    }
    Ok(())
}

fn build_source_resources(
    checked: &CheckedProgram,
    execution: &SemanticExecutionGraphV1,
    lists: &[SemanticListResourceV1],
    target_lists: &BTreeMap<StaticOwnerId, SemanticListId>,
    aliases: &mut Vec<SemanticResourceAliasV1>,
) -> Result<Vec<SemanticSourceResourceV1>, String> {
    let mut resources = Vec::with_capacity(execution.sources.len());
    for source in &execution.sources {
        let value = expression(execution, source.expression)?;
        let target_list = source
            .owner
            .and_then(|owner| target_lists.get(&owner).copied());
        let target_list_resource = target_list
            .map(|list| list_resource(lists, list))
            .transpose()?;
        let row_scope = target_list_resource.map(|list| list.row_scope);
        let (declared_path, checked_statement, interval_ms, payload_type, span, alias_paths) =
            match source.origin {
                SemanticSourceOrigin::Checked { source: checked_id } => {
                    let checked_source = checked
                        .sources
                        .iter()
                        .find(|candidate| candidate.id == checked_id)
                        .ok_or_else(|| {
                            format!(
                                "semantic source {} references missing checked source {}",
                                source.id, checked_id.0
                            )
                        })?;
                    if checked_source.declaration != source.declaration
                        || value.checked_expr_id != checked_source.expression
                    {
                        return Err(format!(
                            "semantic source {} differs from checked source {} declaration/expression origin",
                            source.id, checked_id.0
                        ));
                    }
                    validate_checked_source_statement(checked, checked_id, checked_source)?;
                    let path = checked.semantic_path(&checked_source.path).ok_or_else(|| {
                        format!(
                            "checked source {} has no canonical semantic path",
                            checked_id.0
                        )
                    })?;
                    (
                        path.clone(),
                        Some(checked_source.statement),
                        checked_source.interval_ms,
                        checked_source.payload_type.clone(),
                        checked_source.span.into(),
                        vec![path, source.binding_path.clone()],
                    )
                }
                SemanticSourceOrigin::ProducerInvocation { .. } => {
                    if value.flow_type
                        != (boon_typecheck::FlowType {
                            mode: boon_typecheck::FlowMode::PresentOrAbsent,
                            ty: Type::Absent,
                        })
                    {
                        return Err(format!(
                            "producer invocation source {} has non-trigger flow type {:?}",
                            source.id, value.flow_type
                        ));
                    }
                    let span = checked
                        .declarations
                        .iter()
                        .find(|declaration| declaration.id == source.declaration)
                        .map(|declaration| declaration.span.into())
                        .ok_or_else(|| {
                            format!(
                                "producer invocation source {} references missing declaration {}",
                                source.id, source.declaration.0
                            )
                        })?;
                    (
                        source.binding_path.clone(),
                        None,
                        None,
                        Type::Absent,
                        span,
                        vec![source.binding_path.clone()],
                    )
                }
            };
        let canonical = canonical_resource_path(
            target_list_resource,
            value.resource_binding_path.as_deref(),
            &source.binding_path,
        );
        for alias in alias_paths {
            insert_alias(
                aliases,
                source.owner,
                alias,
                SemanticResourceAliasTargetV1::Source(source.id),
            );
        }
        resources.push(SemanticSourceResourceV1 {
            id: source.id,
            declaration: source.declaration,
            statement: source.statement,
            checked_statement,
            expression: source.expression,
            origin: source.origin,
            semantic_path: canonical.clone(),
            declared_binding_path: declared_path,
            binding_path: canonical,
            owner: source.owner,
            owner_ancestry: owner_ancestry(source.owner, &execution.static_owners)?,
            target_list,
            row_scope,
            scoped: row_scope.is_some(),
            interval_ms,
            payload_fields: payload_fields(&payload_type),
            payload_type,
            span,
        });
    }
    Ok(resources)
}

fn build_state_resources(
    checked: &CheckedProgram,
    execution: &SemanticExecutionGraphV1,
    lists: &[SemanticListResourceV1],
    target_lists: &BTreeMap<StaticOwnerId, SemanticListId>,
    aliases: &mut Vec<SemanticResourceAliasV1>,
) -> Result<Vec<SemanticStateResourceV1>, String> {
    let mut resources = Vec::with_capacity(execution.states.len());
    let mut published = BTreeSet::new();
    for state in &execution.states {
        let checked_state = checked
            .states
            .get(state.checked_state.0 as usize)
            .filter(|candidate| candidate.id == state.checked_state)
            .ok_or_else(|| {
                format!(
                    "semantic state {} references missing checked state {}",
                    state.id, state.checked_state.0
                )
            })?;
        let declaration = checked
            .declarations
            .iter()
            .find(|candidate| candidate.id == state.declaration)
            .ok_or_else(|| {
                format!(
                    "semantic state {} references missing checked declaration {}",
                    state.id, state.declaration.0
                )
            })?;
        let declared_path = checked.semantic_path(&checked_state.path).ok_or_else(|| {
            format!(
                "checked state {} has no canonical semantic path",
                state.checked_state.0
            )
        })?;
        let value = expression(execution, state.expression)?;
        expression(execution, state.initial)?;
        let concrete_initial = semantic_state_initial_expression(execution, state.expression)?;
        if checked_state.declaration != state.declaration
            || value.checked_expr_id != checked_state.expression
            || concrete_initial != state.initial
        {
            return Err(format!(
                "semantic state {} differs from checked state {} declaration/expression origins: declaration {} vs {}, expression {} checked {} vs {}, initial {} vs concrete {}",
                state.id,
                state.checked_state.0,
                state.declaration.0,
                checked_state.declaration.0,
                state.expression,
                value.checked_expr_id.0,
                checked_state.expression.0,
                state.initial,
                concrete_initial,
            ));
        }
        validate_checked_state_statement(checked, state.checked_state, checked_state)?;
        let statement = state.statement;
        let expression_members = reachable_expression_members(execution, state.expression)?;
        let mut is_published = true;
        for candidate in &execution.states {
            if candidate.id != state.id
                && candidate.declaration == state.declaration
                && candidate.owner == state.owner
                && expression_reaches(execution, candidate.expression, state.expression)?
            {
                is_published = false;
                break;
            }
        }
        if is_published && !published.insert((state.declaration, state.owner)) {
            return Err(format!(
                "declaration {} owner {:?} publishes more than one semantic state",
                state.declaration.0, state.owner
            ));
        }
        let target_list = state
            .owner
            .and_then(|owner| target_lists.get(&owner).copied());
        let target_list_resource = target_list
            .map(|list| list_resource(lists, list))
            .transpose()?;
        let row_scope = target_list_resource.map(|list| list.row_scope);
        let semantic_path = is_published.then(|| {
            canonical_resource_path(
                target_list_resource,
                value.resource_binding_path.as_deref(),
                &state.binding_path,
            )
        });
        if is_published {
            insert_alias(
                aliases,
                state.owner,
                declared_path.clone(),
                SemanticResourceAliasTargetV1::State(state.id),
            );
        }
        let path = semantic_path
            .clone()
            .unwrap_or_else(|| format!("$state.s{}", state.id.as_usize()));
        let hold_name = semantic_state_hold_name(execution, state, statement, is_published)?;
        resources.push(SemanticStateResourceV1 {
            id: state.id,
            checked_state: state.checked_state,
            declaration: state.declaration,
            statement,
            checked_statement: checked_state.statement,
            expression: state.expression,
            expression_members,
            initial: state.initial,
            // Checked state definitions inside generic helpers intentionally
            // retain their lexical type variables. A semantic state is an
            // occurrence in a concrete call frame, so persistence and runtime
            // storage must use the contextualized expression type.
            flow_type: value.flow_type.clone(),
            kind: checked_state.kind,
            binding_path: state.binding_path.clone(),
            declared_path,
            path,
            semantic_path,
            published: is_published,
            hold_name,
            owner: state.owner,
            lifetime: state.lifetime,
            owner_ancestry: owner_ancestry(state.owner, &execution.static_owners)?,
            target_list,
            row_scope,
            scoped: row_scope.is_some(),
            checked_span: checked_state.span.into(),
            span: declaration.span.into(),
        });
    }
    Ok(resources)
}

fn semantic_state_initial_expression(
    execution: &SemanticExecutionGraphV1,
    root: SemanticExprId,
) -> Result<SemanticExprId, String> {
    let mut current = root;
    let mut visited = BTreeSet::new();
    while visited.insert(current) {
        let value = expression(execution, current)?;
        current = match &value.kind {
            SemanticExpressionKind::Hold { initial, .. } => *initial,
            SemanticExpressionKind::Latest { branches } => *branches.first().ok_or_else(|| {
                format!("semantic state expression {current} has no initial branch")
            })?,
            SemanticExpressionKind::Call { arguments, .. } if value.effect.writes_state => {
                arguments
                    .iter()
                    .min_by_key(|argument| argument.ordinal)
                    .map(|argument| argument.value)
                    .ok_or_else(|| {
                        format!("semantic stateful call {current} has no initializer argument")
                    })?
            }
            _ => return Ok(current),
        };
    }
    Err(format!(
        "semantic state initializer rooted at {root} contains a cycle"
    ))
}

fn semantic_state_hold_name(
    execution: &SemanticExecutionGraphV1,
    state: &crate::SemanticStateDef,
    statement: SemanticStatementId,
    published: bool,
) -> Result<String, String> {
    if !published {
        return Ok(format!("{}#internal", state.binding_path));
    }
    let expression = expression(execution, state.expression)?;
    if let SemanticExpressionKind::Hold { name, .. } = &expression.kind
        && !name.is_empty()
    {
        return Ok(name.clone());
    }
    let statement = execution
        .statements
        .get(statement.as_usize())
        .filter(|candidate| candidate.id == statement)
        .ok_or_else(|| {
            format!(
                "semantic state {} references missing owning statement {statement}",
                state.id
            )
        })?;
    if let SemanticStatementKind::Hold {
        hold_name: Some(name),
        ..
    } = &statement.kind
        && !name.is_empty()
    {
        return Ok(name.clone());
    }
    state
        .binding_path
        .rsplit('.')
        .find(|part| !part.is_empty())
        .map(ToOwned::to_owned)
        .ok_or_else(|| format!("semantic state {} has no canonical HOLD name", state.id))
}

fn build_producer_resources(
    out_net: &ResolvedOutGraph,
    execution: &SemanticExecutionGraphV1,
) -> Result<Vec<SemanticProducerResourceV1>, String> {
    let mut resources = Vec::with_capacity(out_net.producer_roots().len());
    for root in out_net.producer_roots() {
        let owner = out_net.owner_for_call(root.call).ok_or_else(|| {
            format!(
                "producer {} has no semantic static owner",
                root.spec.function_name
            )
        })?;
        let function = execution
            .functions
            .iter()
            .find(|function| function.producer == root.spec.function)
            .ok_or_else(|| {
                format!(
                    "producer {} has no semantic callable for producer identity {}",
                    root.spec.function_name, root.spec.function
                )
            })?;
        let callable = function.callable;
        let result_statement = exact_statement_origin(execution, function.root, "producer result")?;
        let statement = execution
            .statements
            .get(result_statement.as_usize())
            .filter(|statement| statement.id == result_statement)
            .ok_or_else(|| {
                format!(
                    "producer {} resolves to missing semantic result statement {result_statement}",
                    root.spec.function_name
                )
            })?;
        if statement.declaration != Some(root.spec.result_declaration) {
            return Err(format!(
                "producer {} result statement declaration differs from {}",
                root.spec.function_name, root.spec.result_declaration.0
            ));
        }
        let invocation_source = function
            .invocation_source
            .map(|expression| {
                execution
                    .sources
                    .iter()
                    .find(|source| {
                        source.expression == expression
                            && matches!(
                                source.origin,
                                SemanticSourceOrigin::ProducerInvocation { identity, .. }
                                    if identity == root.spec.identity
                            )
                    })
                    .map(|source| source.id)
                    .ok_or_else(|| {
                        format!(
                            "producer {} invocation expression has no semantic source",
                            root.spec.function_name
                        )
                    })
            })
            .transpose()?;
        resources.push(SemanticProducerResourceV1 {
            identity: root.spec.identity,
            mode: root.spec.mode,
            function: root.spec.function,
            callable,
            root_call: root.call,
            result_statement,
            result_declaration: root.spec.result_declaration,
            result_path: root.spec.result_path.clone(),
            owner,
            invocation_source,
        });
    }
    resources.sort_by_key(|resource| resource.identity);
    Ok(resources)
}

fn discover_list_projections(
    execution: &SemanticExecutionGraphV1,
    lists: &[SemanticListResourceV1],
) -> Result<Vec<SemanticListProjectionV1>, String> {
    let declaration_lists = lists
        .iter()
        .map(|list| (list.declaration, list.id))
        .collect::<BTreeMap<_, _>>();
    let locals = semantic_local_values(execution)?;
    let mut projections = Vec::new();
    let mut targets = BTreeSet::new();
    for target in lists {
        let Some(chunk) = terminal_chunk_expression(execution, target.producer)? else {
            continue;
        };
        let value = expression(execution, chunk)?;
        let SemanticExpressionKind::Call { arguments, .. } = &value.kind else {
            unreachable!("terminal chunk is a call");
        };
        let list_arguments = arguments
            .iter()
            .filter(|argument| argument.name == "list")
            .collect::<Vec<_>>();
        let [list_argument] = list_arguments.as_slice() else {
            return Err(format!(
                "List/chunk expression {chunk} must have exactly one checked `list` argument"
            ));
        };
        let source = semantic_list_id(execution, &declaration_lists, &locals, list_argument.value)?
            .ok_or_else(|| {
                format!("List/chunk expression {chunk} has no exact semantic list provenance")
            })?;
        list_resource(lists, source)?;
        let size_arguments = arguments
            .iter()
            .filter(|argument| argument.name == "size")
            .collect::<Vec<_>>();
        let [size_argument] = size_arguments.as_slice() else {
            return Err(format!(
                "List/chunk expression {chunk} must have exactly one checked `size` argument"
            ));
        };
        let resolved_size = match semantic_static_data(execution, size_argument.value) {
            Ok(boon_data::Value::Number(value)) => value.to_usize_exact().ok(),
            Ok(_) | Err(_) => None,
        };
        if !targets.insert(target.semantic_path.clone()) {
            return Err(format!(
                "semantic list projection target `{}` was allocated more than once",
                target.semantic_path
            ));
        }
        projections.push(SemanticListProjectionV1 {
            target: target.id,
            source,
            kind: SemanticListProjectionKindV1::Chunk {
                size_expression: Some(size_argument.value),
                resolved_size,
            },
        });
    }
    Ok(projections)
}

fn terminal_chunk_expression(
    execution: &SemanticExecutionGraphV1,
    root: SemanticExprId,
) -> Result<Option<SemanticExprId>, String> {
    let mut colors = vec![0_u8; execution.expressions.len()];
    let mut results = BTreeMap::<SemanticExprId, Option<SemanticExprId>>::new();
    let mut pending = vec![(root, false)];
    while let Some((id, exiting)) = pending.pop() {
        let index = id.as_usize();
        let value = expression(execution, id)?;
        if exiting {
            let result = match &value.kind {
                SemanticExpressionKind::Call { name, .. } if name == "List/chunk" => Some(id),
                SemanticExpressionKind::Block { result, .. } => results
                    .get(result)
                    .copied()
                    .ok_or_else(|| format!("terminal chunk child {result} was not evaluated"))?,
                SemanticExpressionKind::Project { input, .. }
                | SemanticExpressionKind::Draining { input } => results
                    .get(input)
                    .copied()
                    .ok_or_else(|| format!("terminal chunk child {input} was not evaluated"))?,
                SemanticExpressionKind::Then {
                    output: Some(output),
                    ..
                }
                | SemanticExpressionKind::MatchArm {
                    output: Some(output),
                    ..
                } => results
                    .get(output)
                    .copied()
                    .ok_or_else(|| format!("terminal chunk child {output} was not evaluated"))?,
                SemanticExpressionKind::When { arms, .. } => {
                    exact_terminal_chunk_branches(id, arms.iter().map(|arm| arm.output), &results)?
                }
                SemanticExpressionKind::Latest { branches } => {
                    exact_terminal_chunk_branches(id, branches.iter().copied(), &results)?
                }
                _ => None,
            };
            results.insert(id, result);
            colors[index] = 2;
            continue;
        }
        match colors[index] {
            2 => continue,
            1 => {
                return Err(format!(
                    "list projection expression {id} contains a semantic cycle"
                ));
            }
            _ => {}
        }
        colors[index] = 1;
        pending.push((id, true));
        let children = match &value.kind {
            SemanticExpressionKind::Block { result, .. } => vec![*result],
            SemanticExpressionKind::Project { input, .. }
            | SemanticExpressionKind::Draining { input } => vec![*input],
            SemanticExpressionKind::Then {
                output: Some(output),
                ..
            }
            | SemanticExpressionKind::MatchArm {
                output: Some(output),
                ..
            } => vec![*output],
            SemanticExpressionKind::When { arms, .. } => {
                arms.iter().map(|arm| arm.output).collect()
            }
            SemanticExpressionKind::Latest { branches } => branches.clone(),
            _ => Vec::new(),
        };
        for child in children.into_iter().rev() {
            let child_index = child.as_usize();
            expression(execution, child)?;
            if colors[child_index] == 1 {
                return Err(format!(
                    "list projection expression {id} contains a semantic cycle through {child}"
                ));
            }
            if colors[child_index] != 2 {
                pending.push((child, false));
            }
        }
    }
    results
        .remove(&root)
        .ok_or_else(|| format!("list projection root {root} was not evaluated"))
}

fn exact_terminal_chunk_branches(
    parent: SemanticExprId,
    branches: impl IntoIterator<Item = SemanticExprId>,
    results: &BTreeMap<SemanticExprId, Option<SemanticExprId>>,
) -> Result<Option<SemanticExprId>, String> {
    let chunks = branches
        .into_iter()
        .map(|branch| {
            results.get(&branch).copied().ok_or_else(|| {
                format!("terminal chunk branch {branch} of {parent} was not evaluated")
            })
        })
        .collect::<Result<BTreeSet<_>, _>>()?;
    match chunks.len() {
        0 => Ok(None),
        1 => Ok(chunks.into_iter().next().flatten()),
        _ => Err(format!(
            "conditional semantic list projection {parent} has inconsistent terminal chunk operations"
        )),
    }
}

fn semantic_list_id(
    execution: &SemanticExecutionGraphV1,
    lists_by_declaration: &BTreeMap<DeclId, SemanticListId>,
    local_bindings: &BTreeMap<SemanticLocalBindingId, (DeclId, SemanticExprId)>,
    root: SemanticExprId,
) -> Result<Option<SemanticListId>, String> {
    let mut current = root;
    let mut visiting = BTreeSet::new();
    loop {
        if !visiting.insert(current) {
            return Err(format!(
                "list provenance expression {current} contains a semantic cycle"
            ));
        }
        let candidate = expression(execution, current)?;
        match &candidate.kind {
            SemanticExpressionKind::CanonicalRead {
                target, projection, ..
            } if projection.is_empty() => return Ok(lists_by_declaration.get(target).copied()),
            SemanticExpressionKind::LocalRead {
                binding,
                declaration,
                projection,
            } if projection.is_empty() => {
                let (local_declaration, value) = local_bindings.get(binding).ok_or_else(|| {
                    format!(
                        "list provenance expression {current} references missing local binding {binding}"
                    )
                })?;
                if local_declaration != declaration {
                    return Err(format!(
                        "list provenance local {binding} declaration {} differs from read declaration {}",
                        local_declaration.0, declaration.0
                    ));
                }
                current = *value;
            }
            SemanticExpressionKind::Materialize { materialization } => {
                let definition = execution
                    .materializations
                    .get(materialization.as_usize())
                    .filter(|candidate| candidate.id == *materialization)
                    .ok_or_else(|| {
                        format!(
                            "list provenance expression {current} references missing materialization {materialization}"
                        )
                    })?;
                return Ok(definition.target_list_id.or(definition.source_list_id));
            }
            SemanticExpressionKind::Block { result, .. } => current = *result,
            SemanticExpressionKind::Project { input, fields } if fields.is_empty() => {
                current = *input;
            }
            SemanticExpressionKind::Draining { input } => current = *input,
            SemanticExpressionKind::Then {
                output: Some(output),
                ..
            }
            | SemanticExpressionKind::MatchArm {
                output: Some(output),
                ..
            } => current = *output,
            SemanticExpressionKind::Call { arguments, .. }
                if matches!(candidate.flow_type.ty, Type::List(_)) =>
            {
                let mut list_inputs = Vec::new();
                for argument in arguments {
                    let input = expression(execution, argument.value)?;
                    if matches!(input.flow_type.ty, Type::List(_)) {
                        list_inputs.push(argument.value);
                    }
                }
                let [input] = list_inputs.as_slice() else {
                    return Ok(None);
                };
                current = *input;
            }
            _ => return Ok(None),
        }
    }
}

fn validate_dense_resource_ids(
    graph: &SemanticResourceGraphV1,
    execution: &SemanticExecutionGraphV1,
) -> Result<(), String> {
    if graph.row_scopes.len() != graph.lists.len() {
        return Err(format!(
            "semantic row scopes do not bijectively cover lists: {} scopes for {} lists",
            graph.row_scopes.len(),
            graph.lists.len()
        ));
    }
    for (index, scope) in graph.row_scopes.iter().enumerate() {
        if scope.id != SemanticRowScopeId(index) {
            return Err(format!(
                "semantic row scope at index {index} has non-dense ID {}",
                scope.id
            ));
        }
        let list = graph
            .lists
            .get(scope.list.as_usize())
            .filter(|list| list.id == scope.list)
            .ok_or_else(|| {
                format!(
                    "semantic row scope {} references missing list {}",
                    scope.id, scope.list
                )
            })?;
        if list.row_scope != scope.id {
            return Err(format!(
                "semantic list {} row scope {} differs from scope {}",
                list.id, list.row_scope, scope.id
            ));
        }
    }
    for (index, list) in graph.lists.iter().enumerate() {
        if list.id != SemanticListId(index) {
            return Err(format!(
                "semantic list at index {index} has non-dense ID {}",
                list.id
            ));
        }
        let statement = execution
            .statements
            .get(list.statement.as_usize())
            .filter(|statement| statement.id == list.statement)
            .ok_or_else(|| {
                format!(
                    "semantic list {} references missing statement {}",
                    list.id, list.statement
                )
            })?;
        if statement.declaration != Some(list.declaration) || statement.value != Some(list.producer)
        {
            return Err(format!(
                "semantic list {} does not exactly match its storage statement",
                list.id
            ));
        }
        let producer = expression(execution, list.producer)?;
        let Type::List(item_type) = &producer.flow_type.ty else {
            return Err(format!(
                "semantic list {} producer {} is not typed as List",
                list.id, list.producer
            ));
        };
        if item_type.as_ref() != &list.item_type {
            return Err(format!(
                "semantic list {} item type differs from its producer",
                list.id
            ));
        }
        let scope = graph
            .row_scopes
            .get(list.row_scope.as_usize())
            .filter(|scope| scope.id == list.row_scope)
            .ok_or_else(|| {
                format!(
                    "semantic list {} references missing row scope {}",
                    list.id, list.row_scope
                )
            })?;
        if scope.list != list.id {
            return Err(format!(
                "semantic list {} row scope {} points to list {}",
                list.id, scope.id, scope.list
            ));
        }
        if list.initializer
            != semantic_list_initializer(execution, list.producer, &list.semantic_path)?
        {
            return Err(format!(
                "semantic list {} has stale initializer value or provenance",
                list.id
            ));
        }
    }
    let mut classified_list_producers = graph
        .lists
        .iter()
        .map(|list| (list.statement, list.producer))
        .collect::<BTreeSet<_>>();
    for (index, authority) in graph.value_list_authorities.iter().enumerate() {
        if authority.id != SemanticValueListAuthorityId(index) {
            return Err(format!(
                "semantic value-list authority at index {index} has non-dense ID {}",
                authority.id
            ));
        }
        if !classified_list_producers.insert((authority.statement, authority.producer)) {
            return Err(format!(
                "semantic list statement {} producer {} is classified more than once",
                authority.statement, authority.producer
            ));
        }
        let statement = execution
            .statements
            .get(authority.statement.as_usize())
            .filter(|statement| statement.id == authority.statement)
            .ok_or_else(|| {
                format!(
                    "semantic value-list authority {} references missing statement {}",
                    authority.id, authority.statement
                )
            })?;
        let declaration_matches = if authority.role == SemanticValueListRoleV1::InlineValue {
            statement.declaration.is_none()
        } else {
            statement.declaration == Some(authority.declaration)
        };
        if !declaration_matches || statement.value != Some(authority.producer) {
            return Err(format!(
                "semantic value-list authority {} does not exactly match its statement",
                authority.id
            ));
        }
        let producer = expression(execution, authority.producer)?;
        let Type::List(item_type) = &producer.flow_type.ty else {
            return Err(format!(
                "semantic value-list authority {} producer is not typed as List",
                authority.id
            ));
        };
        if item_type.as_ref() != &authority.item_type {
            return Err(format!(
                "semantic value-list authority {} item type differs from its producer",
                authority.id
            ));
        }
        match authority.role {
            SemanticValueListRoleV1::ScalarAuthority
                if matches!(authority.item_type, Type::Object(_)) =>
            {
                return Err(format!(
                    "semantic value-list authority {} misclassifies object rows as scalar",
                    authority.id
                ));
            }
            SemanticValueListRoleV1::Alias { target }
                if direct_list_alias_target(execution, statement) != Some(target) =>
            {
                return Err(format!(
                    "semantic value-list authority {} has stale alias target",
                    authority.id
                ));
            }
            SemanticValueListRoleV1::Absent
                if producer.flow_type.mode != boon_typecheck::FlowMode::Absent =>
            {
                return Err(format!(
                    "semantic value-list authority {} has stale absent classification",
                    authority.id
                ));
            }
            SemanticValueListRoleV1::ScalarAuthority
            | SemanticValueListRoleV1::InlineValue
            | SemanticValueListRoleV1::Alias { .. }
            | SemanticValueListRoleV1::Absent => {}
        }
        let expected_initializer = if authority.role == SemanticValueListRoleV1::InlineValue {
            SemanticListInitializerV1::Empty
        } else {
            semantic_list_initializer(execution, authority.producer, &authority.semantic_path)?
        };
        if authority.initializer != expected_initializer {
            return Err(format!(
                "semantic value-list authority {} has stale initializer value or provenance",
                authority.id
            ));
        }
    }
    if graph.sources.len() != execution.sources.len() {
        return Err("semantic resource sources do not exactly cover execution sources".to_owned());
    }
    for (index, source) in graph.sources.iter().enumerate() {
        let id = SemanticSourceId(index);
        if source.id != id {
            return Err(format!(
                "semantic source resource at index {index} has non-dense ID {}",
                source.id
            ));
        }
        let definition = execution
            .sources
            .get(index)
            .filter(|definition| definition.id == id)
            .ok_or_else(|| format!("semantic source resource {id} has no execution definition"))?;
        if source.declaration != definition.declaration
            || source.statement != definition.statement
            || source.expression != definition.expression
            || source.owner != definition.owner
            || source.origin != definition.origin
        {
            return Err(format!(
                "semantic source resource {id} differs from its execution definition"
            ));
        }
        if source.binding_path != source.semantic_path
            || source.declared_binding_path.trim().is_empty()
        {
            return Err(format!(
                "semantic source resource {id} does not own its final canonical runtime binding"
            ));
        }
        let statement = execution
            .statements
            .get(source.statement.as_usize())
            .filter(|candidate| candidate.id == source.statement)
            .ok_or_else(|| {
                format!(
                    "semantic source resource {id} references missing statement {}",
                    source.statement
                )
            })?;
        let statement_value = statement.value.ok_or_else(|| {
            format!(
                "semantic source statement {} has no value",
                source.statement
            )
        })?;
        let expression_is_statement_value =
            expression_reaches(execution, statement_value, source.expression)?;
        let expression_is_exact_invocation_source = match source.origin {
            SemanticSourceOrigin::Checked { .. } => false,
            SemanticSourceOrigin::ProducerInvocation {
                function,
                producer,
                identity,
            } => {
                let matches = graph
                    .producer_resources
                    .iter()
                    .filter(|resource| {
                        resource.identity == identity
                            && resource.function == producer
                            && resource.callable == function
                            && resource.result_statement == source.statement
                            && resource.invocation_source == Some(source.id)
                    })
                    .count();
                if matches > 1 {
                    return Err(format!(
                        "semantic producer invocation source {id} resolves to {matches} producer resources"
                    ));
                }
                matches == 1
            }
        };
        if statement.declaration != Some(source.declaration)
            || (!expression_is_statement_value && !expression_is_exact_invocation_source)
        {
            let statement_expression = expression(execution, statement_value)?;
            let source_expression = expression(execution, source.expression)?;
            let source_origin = execution
                .checked_expression_origins
                .get(source.expression.as_usize());
            return Err(format!(
                "semantic source resource {id} has stale statement provenance: origin={:?}, statement={}, statement_declaration={:?}, source_declaration={}, statement_value={statement_value} ({:?}, checked={}), source_expression={} ({:?}, checked={}, semantic_origin={source_origin:?}), value_reaches_source={expression_is_statement_value}, exact_invocation_source={expression_is_exact_invocation_source}",
                source.origin,
                source.statement,
                statement.declaration,
                source.declaration.0,
                statement_expression.kind,
                statement_expression.checked_expr_id.0,
                source.expression,
                source_expression.kind,
                source_expression.checked_expr_id.0,
            ));
        }
    }
    if graph.states.len() != execution.states.len() {
        return Err("semantic resource states do not exactly cover execution states".to_owned());
    }
    for (index, state) in graph.states.iter().enumerate() {
        let id = SemanticStateId(index);
        if state.id != id {
            return Err(format!(
                "semantic state resource at index {index} has non-dense ID {}",
                state.id
            ));
        }
        let definition = execution
            .states
            .get(index)
            .filter(|definition| definition.id == id)
            .ok_or_else(|| format!("semantic state resource {id} has no execution definition"))?;
        if state.checked_state != definition.checked_state
            || state.declaration != definition.declaration
            || state.statement != definition.statement
            || state.expression != definition.expression
            || state.initial != definition.initial
            || state.owner != definition.owner
        {
            return Err(format!(
                "semantic state resource {id} differs from its execution definition"
            ));
        }
        let statement = execution
            .statements
            .get(state.statement.as_usize())
            .filter(|candidate| candidate.id == state.statement)
            .ok_or_else(|| {
                format!(
                    "semantic state resource {id} references missing statement {}",
                    state.statement
                )
            })?;
        if statement.declaration != Some(state.declaration)
            || !expression_reaches(
                execution,
                statement.value.ok_or_else(|| {
                    format!("semantic state statement {} has no value", state.statement)
                })?,
                state.expression,
            )?
        {
            return Err(format!(
                "semantic state resource {id} has stale statement provenance"
            ));
        }
        if state.expression_members != reachable_expression_members(execution, state.expression)? {
            return Err(format!(
                "semantic state resource {id} has stale expression-member coverage"
            ));
        }
        let expected_hold_name =
            semantic_state_hold_name(execution, definition, state.statement, state.published)?;
        if state.hold_name != expected_hold_name {
            return Err(format!(
                "semantic state resource {id} has stale final HOLD name"
            ));
        }
    }
    Ok(())
}

pub(crate) fn validate_checked_list_classification(
    checked: &CheckedProgram,
    execution: &SemanticExecutionGraphV1,
    graph: &SemanticResourceGraphV1,
) -> Result<(), String> {
    let mut snapshot = execution.clone();
    let (row_scopes, mut lists, value_list_authorities) =
        discover_list_resources(checked, &mut snapshot)?;
    bind_list_lineage(&snapshot, &mut lists)?;
    if graph.row_scopes != row_scopes
        || graph.lists != lists
        || graph.value_list_authorities != value_list_authorities
    {
        return Err(
            "semantic list resources do not exactly classify checked and typed semantic list producers"
                .to_owned(),
        );
    }
    Ok(())
}

pub(crate) fn validate_checked_resource_provenance(
    checked: &CheckedProgram,
    execution: &SemanticExecutionGraphV1,
    graph: &SemanticResourceGraphV1,
) -> Result<(), String> {
    let target_lists = materialization_target_lists(execution, &graph.lists)?;
    let mut expected_aliases = Vec::new();
    let expected_sources = build_source_resources(
        checked,
        execution,
        &graph.lists,
        &target_lists,
        &mut expected_aliases,
    )?;
    let expected_states = build_state_resources(
        checked,
        execution,
        &graph.lists,
        &target_lists,
        &mut expected_aliases,
    )?;
    expected_aliases.sort();
    expected_aliases.dedup();
    if graph.sources != expected_sources {
        return Err(
            "semantic source resources differ from exact checked and execution provenance"
                .to_owned(),
        );
    }
    if graph.states != expected_states {
        return Err(
            "semantic state resources differ from exact checked and execution provenance"
                .to_owned(),
        );
    }
    if graph.aliases != expected_aliases {
        return Err(
            "semantic resource aliases differ from exact checked source/state paths".to_owned(),
        );
    }
    Ok(())
}

fn validate_materialization_bindings(
    graph: &SemanticResourceGraphV1,
    execution: &SemanticExecutionGraphV1,
) -> Result<(), String> {
    verify_semantic_materialization_lineage(&execution.materializations)?;
    if graph.materialization_bindings.len() != execution.materializations.len() {
        return Err("semantic resource bindings do not exactly cover materializations".to_owned());
    }
    for (index, binding) in graph.materialization_bindings.iter().enumerate() {
        let id = SemanticMaterializationId(index);
        if binding.materialization != id {
            return Err(format!(
                "semantic materialization binding at index {index} has ID {}",
                binding.materialization
            ));
        }
        let materialization = execution
            .materializations
            .get(index)
            .filter(|materialization| materialization.id == id)
            .ok_or_else(|| format!("missing semantic materialization {id}"))?;
        let source = paired_row_binding(
            materialization.source_list_id,
            materialization.source_scope_id,
            "source",
            materialization.owner,
        )?;
        let target = paired_row_binding(
            materialization.target_list_id,
            materialization.target_scope_id,
            "target",
            materialization.owner,
        )?;
        if binding.owner != materialization.owner
            || binding.source != source
            || binding.target != target
            || binding.predecessors != materialization.source_row_predecessors
        {
            return Err(format!(
                "semantic materialization binding {id} differs from execution graph"
            ));
        }
        for row in binding.source.into_iter().chain(binding.target) {
            validate_row_binding(graph, row, &format!("materialization {id}"))?;
        }
        for predecessor in &binding.predecessors {
            match predecessor {
                SemanticContextualRowPredecessor::Stored { row } => {
                    validate_row_binding(
                        graph,
                        *row,
                        &format!("materialization {id} predecessor"),
                    )?;
                }
                SemanticContextualRowPredecessor::Materialized { materialization }
                | SemanticContextualRowPredecessor::Provenance { materialization } => {
                    if execution
                        .materializations
                        .get(materialization.as_usize())
                        .is_none_or(|candidate| candidate.id != *materialization)
                    {
                        return Err(format!(
                            "materialization {id} references missing predecessor {materialization}"
                        ));
                    }
                }
                SemanticContextualRowPredecessor::Value => {}
            }
        }
    }
    Ok(())
}

fn validate_row_binding(
    graph: &SemanticResourceGraphV1,
    row: SemanticRowBinding,
    context: &str,
) -> Result<(), String> {
    let list = list_resource(&graph.lists, row.list)?;
    if list.row_scope != row.scope {
        return Err(format!(
            "{context} row {row:?} does not match semantic list scope"
        ));
    }
    let scope = graph
        .row_scopes
        .get(row.scope.as_usize())
        .filter(|scope| scope.id == row.scope)
        .ok_or_else(|| format!("{context} references missing row scope {}", row.scope))?;
    if scope.list != row.list {
        return Err(format!(
            "{context} row scope {} points to list {}, not {}",
            row.scope, scope.list, row.list
        ));
    }
    Ok(())
}

fn validate_list_projections(
    graph: &SemanticResourceGraphV1,
    execution: &SemanticExecutionGraphV1,
) -> Result<(), String> {
    let expected = discover_list_projections(execution, &graph.lists)?;
    if graph.list_projections != expected {
        return Err(
            "semantic list projections do not exactly match executable list authority".to_owned(),
        );
    }
    for projection in &graph.list_projections {
        list_resource(&graph.lists, projection.target)?;
        list_resource(&graph.lists, projection.source)?;
        if let SemanticListProjectionKindV1::Chunk {
            size_expression: Some(expression_id),
            ..
        } = projection.kind
        {
            expression(execution, expression_id)?;
        }
    }
    Ok(())
}

fn validate_resource_owners(
    graph: &SemanticResourceGraphV1,
    execution: &SemanticExecutionGraphV1,
) -> Result<(), String> {
    for (owner, ancestry, label) in graph
        .sources
        .iter()
        .map(|source| {
            (
                source.owner,
                &source.owner_ancestry,
                format!("source {}", source.id),
            )
        })
        .chain(graph.states.iter().map(|state| {
            (
                state.owner,
                &state.owner_ancestry,
                format!("state {}", state.id),
            )
        }))
    {
        let expected = owner_ancestry(owner, &execution.static_owners)?;
        if *ancestry != expected {
            return Err(format!("{label} has stale semantic owner ancestry"));
        }
    }
    for source in &graph.sources {
        validate_scoped_resource_binding(
            graph,
            source.owner,
            source.target_list,
            source.row_scope,
            source.scoped,
            &format!("source {}", source.id),
        )?;
    }
    for state in &graph.states {
        validate_scoped_resource_binding(
            graph,
            state.owner,
            state.target_list,
            state.row_scope,
            state.scoped,
            &format!("state {}", state.id),
        )?;
    }
    Ok(())
}

fn validate_scoped_resource_binding(
    graph: &SemanticResourceGraphV1,
    owner: Option<StaticOwnerId>,
    target_list: Option<SemanticListId>,
    row_scope: Option<SemanticRowScopeId>,
    scoped: bool,
    context: &str,
) -> Result<(), String> {
    match (target_list, row_scope) {
        (None, None) if !scoped => Ok(()),
        (Some(list), Some(scope)) if scoped => {
            if owner.is_none() {
                return Err(format!("{context} is row-scoped without a static owner"));
            }
            validate_row_binding(graph, SemanticRowBinding { list, scope }, context)
        }
        _ => Err(format!(
            "{context} has inconsistent target-list, row-scope, and scoped fields"
        )),
    }
}

fn validate_resource_aliases(graph: &SemanticResourceGraphV1) -> Result<(), String> {
    if graph.aliases.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err("semantic resource aliases are not strictly sorted and unique".to_owned());
    }
    for alias in &graph.aliases {
        if alias.alias.trim().is_empty() {
            return Err("semantic resource alias is empty".to_owned());
        }
        match alias.target {
            SemanticResourceAliasTargetV1::Source(id) => {
                let source = graph
                    .sources
                    .get(id.as_usize())
                    .filter(|source| source.id == id)
                    .ok_or_else(|| {
                        format!("semantic resource alias references missing source {id}")
                    })?;
                if source.owner != alias.owner {
                    return Err(format!(
                        "semantic resource alias `{}` owner differs from source {id}",
                        alias.alias
                    ));
                }
            }
            SemanticResourceAliasTargetV1::State(id) => {
                let state = graph
                    .states
                    .get(id.as_usize())
                    .filter(|state| state.id == id)
                    .ok_or_else(|| {
                        format!("semantic resource alias references missing state {id}")
                    })?;
                if state.owner != alias.owner {
                    return Err(format!(
                        "semantic resource alias `{}` owner differs from state {id}",
                        alias.alias
                    ));
                }
            }
        }
    }
    Ok(())
}

fn validate_producer_resources(
    graph: &SemanticResourceGraphV1,
    execution: &SemanticExecutionGraphV1,
    out_net: &ResolvedOutGraph,
) -> Result<(), String> {
    if graph.producer_resources.len() != out_net.producer_roots().len() {
        return Err("semantic producer resources do not exactly cover producer roots".to_owned());
    }
    let roots = out_net
        .producer_roots()
        .iter()
        .map(|root| (root.spec.identity, root))
        .collect::<BTreeMap<_, _>>();
    let resources = graph
        .producer_resources
        .iter()
        .map(|resource| resource.identity)
        .collect::<BTreeSet<_>>();
    if roots.keys().copied().collect::<BTreeSet<_>>() != resources
        || resources.len() != graph.producer_resources.len()
    {
        return Err("semantic producer resource identities differ from OUT roots".to_owned());
    }
    for resource in &graph.producer_resources {
        let root = roots
            .get(&resource.identity)
            .copied()
            .ok_or_else(|| "semantic producer resource has no exact OUT root".to_owned())?;
        let expected_owner = out_net.owner_for_call(root.call).ok_or_else(|| {
            format!(
                "producer {} has no semantic static owner",
                root.spec.function_name
            )
        })?;
        let function = execution
            .functions
            .iter()
            .find(|function| function.producer == root.spec.function)
            .ok_or_else(|| {
                format!(
                    "semantic producer resource references missing producer function {}",
                    root.spec.function
                )
            })?;
        let expected_statement =
            exact_statement_origin(execution, function.root, "producer result")?;
        let statement = execution
            .statements
            .get(expected_statement.as_usize())
            .filter(|statement| statement.id == expected_statement)
            .ok_or_else(|| {
                format!(
                    "semantic producer resource references missing result statement {expected_statement}"
                )
            })?;
        if statement.declaration != Some(root.spec.result_declaration) {
            return Err(format!(
                "producer {} exact result statement has stale declaration",
                root.spec.function_name
            ));
        }
        let expected_invocation_source = function
            .invocation_source
            .map(|expression| {
                graph
                    .sources
                    .iter()
                    .find(|source| {
                        source.expression == expression
                            && matches!(
                                source.origin,
                                SemanticSourceOrigin::ProducerInvocation {
                                    function: source_function,
                                    producer,
                                    identity,
                            } if source_function == function.callable
                                    && producer == root.spec.function
                                    && identity == root.spec.identity
                            )
                    })
                    .map(|source| source.id)
                    .ok_or_else(|| {
                        format!(
                            "producer {} invocation expression has no exact semantic source",
                            root.spec.function_name
                        )
                    })
            })
            .transpose()?;
        if resource.mode != root.spec.mode
            || resource.function != root.spec.function
            || resource.callable != function.callable
            || function.identity != root.spec.identity
            || resource.root_call != root.call
            || resource.result_statement != expected_statement
            || resource.result_declaration != root.spec.result_declaration
            || resource.result_path != root.spec.result_path
            || resource.owner != expected_owner
            || resource.invocation_source != expected_invocation_source
        {
            return Err(format!(
                "semantic producer resource for {} differs from its exact OUT/function ownership contract",
                root.spec.function_name
            ));
        }
    }
    Ok(())
}

fn resource_graph_digest(
    graph: &SemanticResourceGraphV1,
) -> Result<SemanticResourceGraphDigestV1, String> {
    #[derive(Serialize)]
    struct Payload<'a> {
        schema: &'a str,
        row_scopes: &'a [SemanticRowScopeV1],
        lists: &'a [SemanticListResourceV1],
        value_list_authorities: &'a [SemanticValueListAuthorityV1],
        sources: &'a [SemanticSourceResourceV1],
        states: &'a [SemanticStateResourceV1],
        aliases: &'a [SemanticResourceAliasV1],
        materialization_bindings: &'a [SemanticMaterializationResourceBindingV1],
        list_projections: &'a [SemanticListProjectionV1],
        producer_resources: &'a [SemanticProducerResourceV1],
    }
    boon_contract::canonical_serde_hash_v1(
        SEMANTIC_RESOURCE_GRAPH_DIGEST_DOMAIN,
        &Payload {
            schema: &graph.schema,
            row_scopes: &graph.row_scopes,
            lists: &graph.lists,
            value_list_authorities: &graph.value_list_authorities,
            sources: &graph.sources,
            states: &graph.states,
            aliases: &graph.aliases,
            materialization_bindings: &graph.materialization_bindings,
            list_projections: &graph.list_projections,
            producer_resources: &graph.producer_resources,
        },
    )
    .map(SemanticResourceGraphDigestV1)
    .map_err(|error| format!("canonical semantic resource encoding failed: {error}"))
}

fn semantic_local_values(
    execution: &SemanticExecutionGraphV1,
) -> Result<BTreeMap<SemanticLocalBindingId, (DeclId, SemanticExprId)>, String> {
    let mut locals = BTreeMap::new();
    for expression in &execution.expressions {
        let SemanticExpressionKind::Block { bindings, .. } = &expression.kind else {
            continue;
        };
        for SemanticBlockBinding {
            id,
            declaration,
            value,
        } in bindings
        {
            if let Some(previous) = locals.insert(*id, (*declaration, *value))
                && previous != (*declaration, *value)
            {
                return Err(format!(
                    "semantic local binding {id} has conflicting definitions"
                ));
            }
        }
    }
    Ok(locals)
}

fn semantic_expression_children(kind: &SemanticExpressionKind) -> Vec<SemanticExprId> {
    match kind {
        SemanticExpressionKind::CanonicalRead { .. }
        | SemanticExpressionKind::LocalRead { .. }
        | SemanticExpressionKind::ExternalRead { .. }
        | SemanticExpressionKind::ElementState { .. }
        | SemanticExpressionKind::Drain { .. }
        | SemanticExpressionKind::Text(_)
        | SemanticExpressionKind::Number(_)
        | SemanticExpressionKind::Bits(_)
        | SemanticExpressionKind::BytesByte(_)
        | SemanticExpressionKind::Absent
        | SemanticExpressionKind::Tag(_)
        | SemanticExpressionKind::Source { .. }
        | SemanticExpressionKind::Materialize { .. }
        | SemanticExpressionKind::Delimiter
        | SemanticExpressionKind::MaterializationLocal { .. }
        | SemanticExpressionKind::FunctionParameter { .. } => Vec::new(),
        SemanticExpressionKind::TextTemplate { segments } => segments
            .iter()
            .filter_map(|segment| match segment {
                crate::SemanticTextSegment::Static { .. } => None,
                crate::SemanticTextSegment::Dynamic { value } => Some(*value),
            })
            .collect(),
        SemanticExpressionKind::TaggedObject { fields, .. }
        | SemanticExpressionKind::Object(fields) => {
            fields.iter().map(|field| field.value).collect()
        }
        SemanticExpressionKind::Call { arguments, .. } => {
            arguments.iter().map(|argument| argument.value).collect()
        }
        SemanticExpressionKind::Flush { payload: input }
        | SemanticExpressionKind::FlushBoundary { input }
        | SemanticExpressionKind::Draining { input }
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
        SemanticExpressionKind::Then { input, output } => {
            std::iter::once(*input).chain(*output).collect()
        }
        SemanticExpressionKind::Infix { left, right, .. } => vec![*left, *right],
        SemanticExpressionKind::MapEntry { key, value } => vec![*key, *value],
        SemanticExpressionKind::MatchArm { output, .. } => output.iter().copied().collect(),
        SemanticExpressionKind::Block { bindings, result } => bindings
            .iter()
            .map(|binding| binding.value)
            .chain(std::iter::once(*result))
            .collect(),
        SemanticExpressionKind::List { items, .. }
        | SemanticExpressionKind::Bytes { items, .. }
        | SemanticExpressionKind::Map { entries: items }
        | SemanticExpressionKind::Set { items } => items.clone(),
    }
}

fn expression(
    execution: &SemanticExecutionGraphV1,
    id: SemanticExprId,
) -> Result<&crate::SemanticExpression, String> {
    execution
        .expressions
        .get(id.as_usize())
        .filter(|expression| expression.id == id)
        .ok_or_else(|| format!("semantic resource graph reaches missing expression {id}"))
}

fn list_resource(
    lists: &[SemanticListResourceV1],
    id: SemanticListId,
) -> Result<&SemanticListResourceV1, String> {
    lists
        .get(id.as_usize())
        .filter(|list| list.id == id)
        .ok_or_else(|| format!("missing semantic list {id}"))
}

fn call_argument(arguments: &[crate::SemanticCallArgument], name: &str) -> Option<SemanticExprId> {
    arguments
        .iter()
        .find(|argument| argument.name == name)
        .map(|argument| argument.value)
}

fn paired_row_binding(
    list: Option<SemanticListId>,
    scope: Option<SemanticRowScopeId>,
    role: &str,
    owner: StaticOwnerId,
) -> Result<Option<SemanticRowBinding>, String> {
    match (list, scope) {
        (None, None) => Ok(None),
        (Some(list), Some(scope)) => Ok(Some(SemanticRowBinding { list, scope })),
        _ => Err(format!(
            "semantic owner {owner} has incomplete {role} row binding: list={list:?}, scope={scope:?}"
        )),
    }
}

fn owner_ancestry(
    owner: Option<StaticOwnerId>,
    owners: &[crate::SemanticStaticOwner],
) -> Result<Vec<StaticOwnerId>, String> {
    let mut ancestry = Vec::new();
    let mut current = owner;
    let mut visited = BTreeSet::new();
    while let Some(id) = current {
        if !visited.insert(id) {
            return Err(format!("semantic static owner ancestry cycles at {id}"));
        }
        ancestry.push(id);
        current = owners
            .get(id.as_usize())
            .filter(|owner| owner.id == id)
            .ok_or_else(|| format!("missing semantic static owner {id}"))?
            .parent;
    }
    Ok(ancestry)
}

fn canonical_resource_path(
    target: Option<&SemanticListResourceV1>,
    expanded: Option<&str>,
    binding: &str,
) -> String {
    match (target, expanded) {
        (Some(target), Some(path))
            if path == target.semantic_path
                || path.starts_with(&format!("{}.", target.semantic_path)) =>
        {
            path.to_owned()
        }
        (Some(target), _) => format!("{}.{}", target.semantic_path, binding),
        (None, Some(path)) => path.to_owned(),
        (None, None) => binding.to_owned(),
    }
}

fn insert_alias(
    aliases: &mut Vec<SemanticResourceAliasV1>,
    owner: Option<StaticOwnerId>,
    alias: String,
    target: SemanticResourceAliasTargetV1,
) {
    if aliases.iter().any(|candidate| {
        candidate.owner == owner && candidate.alias == alias && candidate.target == target
    }) {
        return;
    }
    aliases.push(SemanticResourceAliasV1 {
        owner,
        alias,
        target,
    });
}

fn payload_fields(data_type: &Type) -> Vec<SemanticPayloadFieldV1> {
    let Type::Object(shape) = data_type else {
        return Vec::new();
    };
    let mut fields = Vec::new();
    let mut seen = BTreeSet::new();
    for name in shape.field_order.iter().chain(shape.fields.keys()) {
        if !seen.insert(name.clone()) {
            continue;
        }
        if let Some(data_type) = shape.fields.get(name) {
            fields.push(SemanticPayloadFieldV1 {
                name: name.clone(),
                data_type: data_type.clone(),
            });
        }
    }
    fields
}

fn exact_statement_origin(
    execution: &SemanticExecutionGraphV1,
    target: SemanticExprId,
    kind: &str,
) -> Result<SemanticStatementId, String> {
    expression(execution, target)?;
    let origin = execution
        .checked_expression_origins
        .get(target.as_usize())
        .filter(|origin| origin.expression == target)
        .ok_or_else(|| format!("semantic {kind} expression {target} has no exact origin"))?;
    let statement = origin.owning_statement.ok_or_else(|| {
        format!("semantic {kind} expression {target} has no exact owning statement")
    })?;
    execution
        .statements
        .get(statement.as_usize())
        .filter(|candidate| candidate.id == statement)
        .ok_or_else(|| {
            format!(
                "semantic {kind} expression {target} references missing owning statement {statement}"
            )
        })?;
    Ok(statement)
}

fn reachable_expression_members(
    execution: &SemanticExecutionGraphV1,
    root: SemanticExprId,
) -> Result<Vec<SemanticExprId>, String> {
    let mut pending = vec![root];
    let mut visited = BTreeSet::new();
    while let Some(id) = pending.pop() {
        if !visited.insert(id) {
            continue;
        }
        let value = expression(execution, id)?;
        if let SemanticExpressionKind::Materialize { materialization } = value.kind {
            let materialization = execution
                .materializations
                .get(materialization.as_usize())
                .filter(|candidate| candidate.id == materialization)
                .ok_or_else(|| {
                    format!(
                        "semantic expression member traversal references missing materialization {materialization}"
                    )
                })?;
            pending.extend(materialization.expression_roots());
        }
        pending.extend(semantic_expression_children(&value.kind));
    }
    Ok(visited.into_iter().collect())
}

fn expression_reaches(
    execution: &SemanticExecutionGraphV1,
    root: SemanticExprId,
    target: SemanticExprId,
) -> Result<bool, String> {
    let mut pending = vec![root];
    let mut visited = BTreeSet::new();
    while let Some(id) = pending.pop() {
        if !visited.insert(id) {
            continue;
        }
        if id == target {
            return Ok(true);
        }
        let value = expression(execution, id)?;
        if let SemanticExpressionKind::Materialize { materialization } = value.kind {
            let materialization = execution
                .materializations
                .get(materialization.as_usize())
                .filter(|candidate| candidate.id == materialization)
                .ok_or_else(|| {
                    format!("reachability references missing materialization {materialization}")
                })?;
            pending.extend(materialization.expression_roots());
        }
        pending.extend(semantic_expression_children(&value.kind));
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn elaborate_source(source: &str) -> crate::SemanticProgram {
        let parsed = boon_parser::parse_source("semantic-resource.bn", source).unwrap();
        let output = boon_typecheck::check_program(&parsed);
        assert!(
            !output.report.has_errors(),
            "diagnostics: {:#?}",
            output.report.diagnostics
        );
        let checked = output.program.expect("resource fixture typechecks");
        crate::elaborate(checked, &[]).expect("resource fixture elaborates")
    }

    fn synthetic_expression(
        id: usize,
        ty: Type,
        kind: SemanticExpressionKind,
    ) -> crate::SemanticExpression {
        crate::SemanticExpression {
            id: SemanticExprId(id),
            value_id: crate::SemanticValueId(id),
            checked_expr_id: boon_typecheck::CheckedExprId(id as u32),
            flow_type: FlowType {
                mode: boon_typecheck::FlowMode::Continuous,
                ty,
            },
            effect: boon_typecheck::CheckedEffectSummary::default(),
            owner: None,
            provenance: crate::SemanticValueProvenance::default(),
            resource_binding_path: None,
            kind,
        }
    }

    fn synthetic_materialization(
        id: usize,
        source: SemanticExprId,
    ) -> crate::SemanticContextualMaterialization {
        crate::SemanticContextualMaterialization {
            id: SemanticMaterializationId(id),
            operation: SemanticContextualOperationKind::Map,
            source,
            source_row_predecessors: Vec::new(),
            body: source,
            direction: None,
            inherited_order: Vec::new(),
            result_kind: crate::SemanticMaterializationResultKind::RuntimeValue,
            row_local: crate::SemanticMaterializationLocalId(id),
            owner: StaticOwnerId(0),
            source_list_id: None,
            source_scope_id: None,
            target_list_id: None,
            target_scope_id: None,
            item_type: Type::Unknown,
            result_type: Type::Unknown,
        }
    }

    #[test]
    fn direct_and_multiple_wrapper_layers_have_the_same_storage_contract() {
        let direct = elaborate_source(
            r#"
rows: LIST { [value: 1] }
result: rows |> List/map(item, new: [value: item.value + 1])
"#,
        );
        let wrapped = elaborate_source(
            r#"
FUNCTION wrapped(list, row: OUT, new) {
    list |> List/map(item: row, new: new)
}
rows: LIST { [value: 1] }
result: rows |> wrapped(row, new: [value: row.value + 1])
"#,
        );
        let multiply_wrapped = elaborate_source(
            r#"
FUNCTION wrapped(list, row: OUT, new) {
    list |> List/map(item: row, new: new)
}
FUNCTION wrapped_twice(list, row: OUT, new) {
    list |> wrapped(row: row, new: new)
}
rows: LIST { [value: 1] }
result: rows |> wrapped_twice(row, new: [value: row.value + 1])
"#,
        );
        assert_eq!(direct.resource_graph().lists.len(), 2);
        assert_eq!(wrapped.resource_graph().lists.len(), 2);
        assert_eq!(multiply_wrapped.resource_graph().lists.len(), 2);
        let direct_binding = &direct.resource_graph().materialization_bindings[0];
        let wrapped_binding = &wrapped.resource_graph().materialization_bindings[0];
        let multiply_wrapped_binding =
            &multiply_wrapped.resource_graph().materialization_bindings[0];
        assert_eq!(direct_binding.source, wrapped_binding.source);
        assert_eq!(direct_binding.target, wrapped_binding.target);
        assert_eq!(direct_binding.predecessors, wrapped_binding.predecessors);
        assert_eq!(direct_binding.source, multiply_wrapped_binding.source);
        assert_eq!(direct_binding.target, multiply_wrapped_binding.target);
        assert_eq!(
            direct_binding.predecessors,
            multiply_wrapped_binding.predecessors
        );
    }

    #[test]
    fn nested_materializations_bind_exact_source_and_target_rows() {
        let semantic = elaborate_source(
            r#"
rows: LIST {
    [value: 1]
    [value: 2]
}
mapped: rows |> List/map(item, new: [value: item.value + 1])
kept: mapped |> List/retain(item, if: True)
"#,
        );
        assert_eq!(semantic.resource_graph().lists.len(), 3);
        assert_eq!(semantic.resource_graph().materialization_bindings.len(), 2);
        for binding in &semantic.resource_graph().materialization_bindings {
            assert!(binding.source.is_some());
            assert!(binding.target.is_some());
            assert!(!binding.predecessors.is_empty());
        }
    }

    #[test]
    fn lineage_projection_matches_record_scalar_and_new_list_value_semantics() {
        let list_type = Type::List(Box::new(Type::Unknown));
        let object_type = Type::Object(boon_typecheck::ObjectShape {
            fields: BTreeMap::new(),
            field_order: Vec::new(),
            open: false,
        });
        let execution = SemanticExecutionGraphV1 {
            expressions: vec![
                synthetic_expression(
                    0,
                    list_type.clone(),
                    SemanticExpressionKind::Materialize {
                        materialization: SemanticMaterializationId(0),
                    },
                ),
                synthetic_expression(
                    1,
                    object_type.clone(),
                    SemanticExpressionKind::Object(vec![crate::SemanticRecordField {
                        declaration: None,
                        name: "rows".to_owned(),
                        value: SemanticExprId(0),
                        spread: false,
                    }]),
                ),
                synthetic_expression(
                    2,
                    list_type.clone(),
                    SemanticExpressionKind::Project {
                        input: SemanticExprId(1),
                        fields: vec!["rows".to_owned()],
                    },
                ),
                synthetic_expression(
                    3,
                    object_type,
                    SemanticExpressionKind::Materialize {
                        materialization: SemanticMaterializationId(1),
                    },
                ),
                synthetic_expression(
                    4,
                    list_type.clone(),
                    SemanticExpressionKind::Project {
                        input: SemanticExprId(3),
                        fields: vec!["nested".to_owned()],
                    },
                ),
                synthetic_expression(
                    5,
                    list_type,
                    SemanticExpressionKind::Call {
                        call: crate::SemanticCallId(0),
                        callable: crate::SemanticCallableId(0),
                        callable_kind: crate::SemanticCallableKind::Builtin,
                        name: "List/chunk".to_owned(),
                        function: "List/chunk".to_owned(),
                        intrinsic: None,
                        role: boon_typecheck::ProgramRole::Client,
                        effect: boon_typecheck::CheckedEffectSummary::default(),
                        result: FlowType {
                            mode: boon_typecheck::FlowMode::Continuous,
                            ty: Type::Unknown,
                        },
                        instance: OutCallInstanceId(0),
                        arguments: Vec::new(),
                        parameter_bindings: Vec::new(),
                        contexts: Vec::new(),
                    },
                ),
            ],
            ..SemanticExecutionGraphV1::default()
        };
        let statement_values = BTreeMap::new();
        let statement_rows = BTreeMap::new();
        let locals = BTreeMap::new();

        assert_eq!(
            collect_lineage_leaves(
                &execution,
                SemanticExprId(2),
                &statement_values,
                &statement_rows,
                &locals,
            )
            .unwrap(),
            BTreeSet::from([LineageLeaf::Materialized(SemanticMaterializationId(0))])
        );
        assert_eq!(
            collect_lineage_leaves(
                &execution,
                SemanticExprId(4),
                &statement_values,
                &statement_rows,
                &locals,
            )
            .unwrap(),
            BTreeSet::from([LineageLeaf::Value])
        );
        assert_eq!(
            collect_lineage_leaves(
                &execution,
                SemanticExprId(5),
                &statement_values,
                &statement_rows,
                &locals,
            )
            .unwrap(),
            BTreeSet::from([LineageLeaf::Value])
        );

        let mut missing_projection = execution.clone();
        let SemanticExpressionKind::Project { fields, .. } =
            &mut missing_projection.expressions[2].kind
        else {
            unreachable!("projection fixture");
        };
        fields[0] = "missing".to_owned();
        let error = collect_lineage_leaves(
            &missing_projection,
            SemanticExprId(2),
            &statement_values,
            &statement_rows,
            &locals,
        )
        .expect_err("missing exact record projection must fail closed");
        assert!(error.contains("no exact field"), "{error}");
    }

    #[test]
    fn materialization_lineage_fails_closed_on_empty_branches_and_cycles() {
        for kind in [
            SemanticExpressionKind::Latest {
                branches: Vec::new(),
            },
            SemanticExpressionKind::When {
                select_kind: crate::SemanticSelectKind::When,
                input: SemanticExprId(0),
                arms: Vec::new(),
            },
        ] {
            let mut execution = SemanticExecutionGraphV1::default();
            execution.expressions.push(synthetic_expression(
                0,
                Type::List(Box::new(Type::Unknown)),
                kind,
            ));
            execution
                .materializations
                .push(synthetic_materialization(0, SemanticExprId(0)));
            let error = bind_materialization_lineage(&mut execution, &[])
                .expect_err("empty lineage branch must not fabricate a predecessor");
            assert!(error.contains("no exact source-row predecessor"), "{error}");
        }

        let mut cycle = vec![
            synthetic_materialization(0, SemanticExprId(0)),
            synthetic_materialization(1, SemanticExprId(0)),
        ];
        cycle[0].source_row_predecessors = vec![SemanticContextualRowPredecessor::Materialized {
            materialization: SemanticMaterializationId(1),
        }];
        cycle[1].source_row_predecessors = vec![SemanticContextualRowPredecessor::Provenance {
            materialization: SemanticMaterializationId(0),
        }];
        let error = verify_semantic_materialization_lineage(&cycle)
            .expect_err("lineage cycle must fail closed");
        assert!(error.contains("cycle"), "{error}");
    }

    #[test]
    fn iterative_lineage_validation_handles_8192_materializations() {
        const DEPTH: usize = 8_192;
        let mut materializations = Vec::with_capacity(DEPTH);
        for index in 0..DEPTH {
            let mut materialization = synthetic_materialization(index, SemanticExprId(0));
            materialization.source_row_predecessors = if index == 0 {
                vec![SemanticContextualRowPredecessor::Value]
            } else {
                vec![SemanticContextualRowPredecessor::Materialized {
                    materialization: SemanticMaterializationId(index - 1),
                }]
            };
            materializations.push(materialization);
        }
        verify_semantic_materialization_lineage(&materializations).unwrap();
    }

    #[test]
    fn storage_scope_resolution_handles_drain_locals_and_only_chunk_items_projection() {
        let row_type = Type::Object(boon_typecheck::ObjectShape {
            fields: BTreeMap::new(),
            field_order: Vec::new(),
            open: false,
        });
        let list_type = Type::List(Box::new(row_type.clone()));
        let rows = DeclId(10);
        let chunks = DeclId(20);
        let mut execution = SemanticExecutionGraphV1::default();
        execution.expressions = vec![
            synthetic_expression(
                0,
                list_type.clone(),
                SemanticExpressionKind::List {
                    capacity: None,
                    items: Vec::new(),
                },
            ),
            synthetic_expression(
                1,
                list_type.clone(),
                SemanticExpressionKind::CanonicalRead {
                    target: rows,
                    path: "rows".to_owned(),
                    projection: Vec::new(),
                    source: None,
                },
            ),
            synthetic_expression(
                2,
                Type::Number,
                SemanticExpressionKind::Number(boon_data::ExactNumber::from_i64(2)),
            ),
            synthetic_expression(
                3,
                Type::List(Box::new(row_type.clone())),
                SemanticExpressionKind::Call {
                    call: crate::SemanticCallId(0),
                    callable: crate::SemanticCallableId(0),
                    callable_kind: crate::SemanticCallableKind::Builtin,
                    name: "List/chunk".to_owned(),
                    function: "List/chunk".to_owned(),
                    intrinsic: None,
                    role: boon_typecheck::ProgramRole::Client,
                    effect: boon_typecheck::CheckedEffectSummary::default(),
                    result: FlowType {
                        mode: boon_typecheck::FlowMode::Continuous,
                        ty: Type::Unknown,
                    },
                    instance: OutCallInstanceId(0),
                    arguments: vec![
                        crate::SemanticCallArgument {
                            formal: DeclId(30),
                            ordinal: 0,
                            name: "list".to_owned(),
                            checked_value: boon_typecheck::CheckedExprId(1),
                            value: SemanticExprId(1),
                            from_pipe: true,
                        },
                        crate::SemanticCallArgument {
                            formal: DeclId(31),
                            ordinal: 1,
                            name: "size".to_owned(),
                            checked_value: boon_typecheck::CheckedExprId(2),
                            value: SemanticExprId(2),
                            from_pipe: false,
                        },
                    ],
                    parameter_bindings: Vec::new(),
                    contexts: Vec::new(),
                },
            ),
            synthetic_expression(
                4,
                Type::List(Box::new(row_type.clone())),
                SemanticExpressionKind::CanonicalRead {
                    target: chunks,
                    path: "chunks".to_owned(),
                    projection: Vec::new(),
                    source: None,
                },
            ),
            synthetic_expression(
                5,
                list_type.clone(),
                SemanticExpressionKind::Project {
                    input: SemanticExprId(4),
                    fields: vec!["items".to_owned()],
                },
            ),
            synthetic_expression(
                6,
                Type::Unknown,
                SemanticExpressionKind::Project {
                    input: SemanticExprId(4),
                    fields: vec!["other".to_owned()],
                },
            ),
            synthetic_expression(
                7,
                list_type.clone(),
                SemanticExpressionKind::Drain {
                    target: rows,
                    path: "rows".to_owned(),
                    projection: Vec::new(),
                },
            ),
            synthetic_expression(
                8,
                list_type,
                SemanticExpressionKind::MaterializationLocal {
                    owner: StaticOwnerId(0),
                    local: crate::SemanticMaterializationLocalId(0),
                    projection: vec!["items".to_owned()],
                },
            ),
        ];
        execution.statements = vec![
            crate::SemanticStatement {
                id: SemanticStatementId(0),
                origin: crate::SemanticStatementOrigin::Checked {
                    statement: boon_typecheck::CheckedStatementId(0),
                },
                scope: crate::SemanticScopeId(0),
                parent: None,
                call_instance: None,
                span: boon_typecheck::CheckedSpan::default(),
                checked_resources: Vec::new(),
                declaration: Some(rows),
                flow_type: Some(execution.expressions[0].flow_type.clone()),
                kind: SemanticStatementKind::List {
                    name: Some("rows".to_owned()),
                    path: Some("rows".to_owned()),
                    capacity: None,
                },
                value: Some(SemanticExprId(0)),
                value_use: crate::SemanticMaterializationResultKind::RuntimeValue,
                children: Vec::new(),
            },
            crate::SemanticStatement {
                id: SemanticStatementId(1),
                origin: crate::SemanticStatementOrigin::Checked {
                    statement: boon_typecheck::CheckedStatementId(1),
                },
                scope: crate::SemanticScopeId(0),
                parent: None,
                call_instance: None,
                span: boon_typecheck::CheckedSpan::default(),
                checked_resources: Vec::new(),
                declaration: Some(chunks),
                flow_type: Some(execution.expressions[3].flow_type.clone()),
                kind: SemanticStatementKind::Field {
                    name: "chunks".to_owned(),
                    path: "chunks".to_owned(),
                },
                value: Some(SemanticExprId(3)),
                value_use: crate::SemanticMaterializationResultKind::RuntimeValue,
                children: Vec::new(),
            },
        ];
        execution.checked_expression_origins = execution
            .expressions
            .iter()
            .map(|expression| crate::SemanticExpressionOrigin {
                expression: expression.id,
                checked_expression: expression.checked_expr_id,
                checked_scope: boon_typecheck::LexicalScopeId(0),
                checked_span: boon_typecheck::CheckedSpan::default(),
                owning_statement: match expression.id {
                    SemanticExprId(0) => Some(SemanticStatementId(0)),
                    SemanticExprId(3) => Some(SemanticStatementId(1)),
                    _ => None,
                },
                call_instance: None,
            })
            .collect();
        execution
            .materializations
            .push(synthetic_materialization(0, SemanticExprId(4)));

        let list = |id: usize,
                    declaration: DeclId,
                    statement: usize,
                    producer: usize,
                    path: &str|
         -> SemanticListResourceV1 {
            SemanticListResourceV1 {
                id: SemanticListId(id),
                declaration,
                statement: SemanticStatementId(statement),
                producer: SemanticExprId(producer),
                origin: SemanticListResourceOriginV1::Derived {
                    statement: SemanticStatementId(statement),
                    producer: SemanticExprId(producer),
                },
                semantic_path: path.to_owned(),
                local_name: path.to_owned(),
                row_scope: SemanticRowScopeId(id),
                item_type: row_type.clone(),
                item_fields: Vec::new(),
                capacity: None,
                key_policy: SemanticListKeyPolicyV1::GeneratedOccurrenceU64 {
                    has_generation: true,
                },
                initializer: SemanticListInitializerV1::Empty,
                row_predecessors: Vec::new(),
                span: SemanticResourceSpanV1 {
                    line: 0,
                    start: 0,
                    end: 0,
                },
            }
        };
        let lists = vec![list(0, rows, 0, 0, "rows"), list(1, chunks, 1, 3, "chunks")];
        let scopes = semantic_storage_scopes(&execution, &lists).unwrap();
        assert_eq!(scopes[1], BTreeSet::from([SemanticRowScopeId(0)]));
        assert_eq!(scopes[4], BTreeSet::from([SemanticRowScopeId(1)]));
        assert_eq!(scopes[5], BTreeSet::from([SemanticRowScopeId(0)]));
        assert!(scopes[6].is_empty());
        assert_eq!(scopes[7], BTreeSet::from([SemanticRowScopeId(0)]));
        assert_eq!(scopes[8], BTreeSet::from([SemanticRowScopeId(0)]));
    }

    #[test]
    fn source_and_state_resources_bind_exact_semantic_statement_provenance() {
        let semantic = elaborate_source(
            r#"
store: [
    pulse: SOURCE
    selected:
        False |> HOLD selected {
            pulse |> THEN { True }
        }
]
"#,
        );
        let [source] = semantic.resource_graph().sources.as_slice() else {
            panic!(
                "expected one semantic source: {:#?}",
                semantic.resource_graph().sources
            );
        };
        let [state] = semantic.resource_graph().states.as_slice() else {
            panic!(
                "expected one semantic state: {:#?}",
                semantic.resource_graph().states
            );
        };
        let source_statement = &semantic.execution_graph().statements[source.statement.as_usize()];
        assert_eq!(source_statement.id, source.statement);
        assert_eq!(source_statement.declaration, Some(source.declaration));
        let SemanticSourceOrigin::Checked { source: checked_id } = source.origin else {
            panic!("fixture source is not checked-backed");
        };
        let checked_source = semantic
            .checked_program
            .sources
            .iter()
            .find(|candidate| candidate.id == checked_id)
            .expect("checked source origin");
        assert_eq!(source.checked_statement, Some(checked_source.statement));
        assert_eq!(source.binding_path, source.semantic_path);
        assert_eq!(source.declared_binding_path, "store.pulse");
        let state_statement = &semantic.execution_graph().statements[state.statement.as_usize()];
        assert_eq!(state_statement.id, state.statement);
        assert_eq!(state_statement.declaration, Some(state.declaration));
        let checked_state = semantic
            .checked_program
            .states
            .iter()
            .find(|candidate| candidate.id == state.checked_state)
            .expect("checked state origin");
        let declaration = semantic
            .checked_program
            .declarations
            .iter()
            .find(|candidate| candidate.id == state.declaration)
            .expect("checked state declaration");
        assert_eq!(state.checked_statement, checked_state.statement);
        assert_eq!(state.kind, checked_state.kind);
        assert_eq!(state.declared_path, "store.selected");
        assert_eq!(state.checked_span, checked_state.span.into());
        assert_eq!(state.span, declaration.span.into());
        assert!(state.expression_members.contains(&state.expression));
        assert!(state.expression_members.contains(&state.initial));
        let definition = &semantic.execution_graph().states[state.id.as_usize()];
        assert_eq!(
            semantic_state_hold_name(
                semantic.execution_graph(),
                definition,
                state.statement,
                false
            )
            .unwrap(),
            format!("{}#internal", state.binding_path)
        );
    }

    #[test]
    fn checked_resource_provenance_rejects_every_checked_backed_field_mutation() {
        let semantic = elaborate_source(
            r#"
store: [
    pulse: SOURCE
    selected:
        False |> HOLD selected {
            pulse |> THEN { True }
        }
]
"#,
        );
        macro_rules! reject_mutation {
            ($mutation:expr) => {{
                let mut resources = semantic.resource_graph().clone();
                ($mutation)(&mut resources);
                validate_checked_resource_provenance(
                    &semantic.checked_program,
                    semantic.execution_graph(),
                    &resources,
                )
                .expect_err("checked-backed resource mutation must be rejected");
            }};
        }

        reject_mutation!(|graph: &mut SemanticResourceGraphV1| {
            graph.sources[0].origin = SemanticSourceOrigin::Checked {
                source: CheckedSourceId(u32::MAX),
            };
        });
        reject_mutation!(|graph: &mut SemanticResourceGraphV1| {
            graph.sources[0].checked_statement = Some(CheckedStatementId(u32::MAX));
        });
        reject_mutation!(|graph: &mut SemanticResourceGraphV1| {
            graph.sources[0].statement = SemanticStatementId(usize::MAX);
        });
        reject_mutation!(|graph: &mut SemanticResourceGraphV1| {
            graph.sources[0].declaration = DeclId(u32::MAX);
        });
        reject_mutation!(|graph: &mut SemanticResourceGraphV1| {
            graph.sources[0].expression = SemanticExprId(usize::MAX);
        });
        reject_mutation!(|graph: &mut SemanticResourceGraphV1| {
            graph.sources[0].declared_binding_path.push_str(".mutated");
        });
        reject_mutation!(|graph: &mut SemanticResourceGraphV1| {
            graph.sources[0].semantic_path.push_str(".mutated");
        });
        reject_mutation!(|graph: &mut SemanticResourceGraphV1| {
            graph.sources[0].binding_path.push_str(".mutated");
        });
        reject_mutation!(|graph: &mut SemanticResourceGraphV1| {
            graph.sources[0].interval_ms = Some(999);
        });
        reject_mutation!(|graph: &mut SemanticResourceGraphV1| {
            graph.sources[0].payload_type = Type::Unknown;
        });
        reject_mutation!(|graph: &mut SemanticResourceGraphV1| {
            graph.sources[0]
                .payload_fields
                .push(SemanticPayloadFieldV1 {
                    name: "mutated".to_owned(),
                    data_type: Type::Unknown,
                });
        });
        reject_mutation!(|graph: &mut SemanticResourceGraphV1| {
            graph.sources[0].span.line = usize::MAX;
        });

        reject_mutation!(|graph: &mut SemanticResourceGraphV1| {
            graph.states[0].checked_state = CheckedStateId(u32::MAX);
        });
        reject_mutation!(|graph: &mut SemanticResourceGraphV1| {
            graph.states[0].checked_statement = CheckedStatementId(u32::MAX);
        });
        reject_mutation!(|graph: &mut SemanticResourceGraphV1| {
            graph.states[0].statement = SemanticStatementId(usize::MAX);
        });
        reject_mutation!(|graph: &mut SemanticResourceGraphV1| {
            graph.states[0].declaration = DeclId(u32::MAX);
        });
        reject_mutation!(|graph: &mut SemanticResourceGraphV1| {
            graph.states[0].expression = SemanticExprId(usize::MAX);
        });
        reject_mutation!(|graph: &mut SemanticResourceGraphV1| {
            graph.states[0].initial = SemanticExprId(usize::MAX);
        });
        reject_mutation!(|graph: &mut SemanticResourceGraphV1| {
            graph.states[0].expression_members.clear();
        });
        reject_mutation!(|graph: &mut SemanticResourceGraphV1| {
            graph.states[0].flow_type.ty = Type::Unknown;
        });
        reject_mutation!(|graph: &mut SemanticResourceGraphV1| {
            graph.states[0].kind = CheckedStateKind::StatefulCall;
        });
        reject_mutation!(|graph: &mut SemanticResourceGraphV1| {
            graph.states[0].binding_path.push_str(".mutated");
        });
        reject_mutation!(|graph: &mut SemanticResourceGraphV1| {
            graph.states[0].declared_path.push_str(".mutated");
        });
        reject_mutation!(|graph: &mut SemanticResourceGraphV1| {
            graph.states[0].path.push_str(".mutated");
        });
        reject_mutation!(|graph: &mut SemanticResourceGraphV1| {
            graph.states[0].semantic_path = Some("mutated".to_owned());
        });
        reject_mutation!(|graph: &mut SemanticResourceGraphV1| {
            graph.states[0].published = !graph.states[0].published;
        });
        reject_mutation!(|graph: &mut SemanticResourceGraphV1| {
            graph.states[0].hold_name.push_str(".mutated");
        });
        reject_mutation!(|graph: &mut SemanticResourceGraphV1| {
            graph.states[0].checked_span.line = usize::MAX;
        });
        reject_mutation!(|graph: &mut SemanticResourceGraphV1| {
            graph.states[0].span.line = usize::MAX;
        });
    }

    #[test]
    fn sparse_checked_declaration_identity_resolves_state_span_without_vector_indexing() {
        let semantic = elaborate_source(
            r#"
pulse: SOURCE
selected:
    False |> HOLD selected {
        pulse |> THEN { True }
    }
"#,
        );
        let state = &semantic.resource_graph().states[0];
        let declaration = semantic
            .checked_program
            .declarations
            .iter()
            .find(|candidate| candidate.id == state.declaration)
            .expect("sparse checked declaration identity");
        assert_eq!(state.span, declaration.span.into());
        validate_checked_resource_provenance(
            &semantic.checked_program,
            semantic.execution_graph(),
            semantic.resource_graph(),
        )
        .unwrap();
    }

    #[test]
    fn final_hold_name_is_digest_bound_and_recomputed() {
        let semantic = elaborate_source(
            r#"
pulse: SOURCE
selected:
    False |> HOLD selected {
        pulse |> THEN { True }
    }
"#,
        );
        let mut resources = semantic.resource_graph().clone();
        resources.states[0].hold_name.push_str("#mutated");
        let error = resources
            .validate(semantic.execution_graph(), semantic.resolved_out_graph())
            .expect_err("mutated final HOLD name must be rejected");
        assert!(error.contains("final HOLD name"), "{error}");
    }

    #[test]
    fn initializers_retain_exact_values_and_range_expression_provenance() {
        let semantic = elaborate_source(
            r#"
rows: LIST {
    [label: TEXT { ready }, enabled: True, count: 2]
}
numbers: List/range(from: -2, to: 3)
"#,
        );
        let rows = semantic
            .resource_graph()
            .lists
            .iter()
            .find(|list| list.local_name == "rows")
            .expect("record list resource");
        assert!(matches!(
            rows.origin,
            SemanticListResourceOriginV1::CheckedLiteral { .. }
        ));
        let SemanticListInitializerV1::RecordLiteral {
            rows: initial_rows, ..
        } = &rows.initializer
        else {
            panic!("record list has wrong initializer: {:#?}", rows.initializer);
        };
        assert_eq!(initial_rows.len(), 1);
        assert!(initial_rows[0].fields.iter().any(|field| {
            field.name == "label"
                && field.expression.is_some()
                && field.value
                    == (SemanticInitialValueV1::Text {
                        value: "ready".to_owned(),
                    })
        }));
        let numbers = semantic
            .resource_graph()
            .value_list_authorities
            .iter()
            .find(|authority| authority.local_name == "numbers")
            .expect("range value-list authority");
        let SemanticListInitializerV1::Range {
            from_expression,
            to_expression,
            from,
            to,
            ..
        } = &numbers.initializer
        else {
            panic!(
                "range list has wrong initializer: {:#?}",
                numbers.initializer
            );
        };
        assert_eq!((*from, *to), (-2, 3));
        assert_ne!(from_expression, to_expression);
        assert!(matches!(numbers.item_type, Type::Number));
        assert!(matches!(
            numbers.origin,
            SemanticListResourceOriginV1::Derived { .. }
        ));
        assert_eq!(numbers.role, SemanticValueListRoleV1::ScalarAuthority);
        assert_eq!(semantic.resource_graph().lists.len(), 1);
    }

    #[test]
    fn checked_and_derived_list_classification_is_bijective_and_fail_closed() {
        let semantic = elaborate_source(
            r#"
rows: LIST {
    [value: 1]
}
mapped: rows |> List/map(item, new: [value: item.value + 1])
numbers: List/range(from: 0, to: 3)
"#,
        );
        let rows = semantic
            .resource_graph()
            .lists
            .iter()
            .find(|list| list.local_name == "rows")
            .expect("checked literal runtime list");
        let mapped = semantic
            .resource_graph()
            .lists
            .iter()
            .find(|list| list.local_name == "mapped")
            .expect("derived runtime list");
        assert!(matches!(
            rows.origin,
            SemanticListResourceOriginV1::CheckedLiteral { .. }
        ));
        assert!(matches!(
            mapped.origin,
            SemanticListResourceOriginV1::Derived { .. }
        ));
        assert!(
            semantic
                .resource_graph()
                .value_list_authorities
                .iter()
                .any(|authority| {
                    authority.local_name == "numbers"
                        && authority.role == SemanticValueListRoleV1::ScalarAuthority
                })
        );

        let mut resources = semantic.resource_graph().clone();
        resources.lists[rows.id.as_usize()].origin = SemanticListResourceOriginV1::Derived {
            statement: rows.statement,
            producer: rows.producer,
        };
        let error = validate_checked_list_classification(
            &semantic.checked_program,
            semantic.execution_graph(),
            &resources,
        )
        .expect_err("checked list origin mutation must fail closed");
        assert!(error.contains("exactly classify"), "{error}");

        let mut resources = semantic.resource_graph().clone();
        resources.value_list_authorities.clear();
        validate_checked_list_classification(
            &semantic.checked_program,
            semantic.execution_graph(),
            &resources,
        )
        .expect_err("missing value-list authority must fail closed");
    }

    #[test]
    fn chunk_projection_is_exact_constant_sized_and_wrapper_invariant() {
        let direct = elaborate_source(
            r#"
chunk_size: 2
rows: LIST {
    [value: 1]
    [value: 2]
}
chunks: rows |> List/chunk(size: chunk_size)
"#,
        );
        let wrapped = elaborate_source(
            r#"
FUNCTION chunked(list, size) {
    list |> List/chunk(size: size)
}
chunk_size: 2
rows: LIST {
    [value: 1]
    [value: 2]
}
chunks: rows |> chunked(size: chunk_size)
"#,
        );
        let direct_chunks = direct
            .resource_graph()
            .lists
            .iter()
            .find(|list| list.local_name == "chunks")
            .expect("direct chunk storage");
        let wrapped_chunks = wrapped
            .resource_graph()
            .lists
            .iter()
            .find(|list| list.local_name == "chunks")
            .expect("wrapped chunk storage");
        assert_eq!(direct_chunks.item_fields, vec!["label", "items"]);
        assert_eq!(wrapped_chunks.item_fields, vec!["label", "items"]);
        assert!(!direct_chunks.item_fields.contains(&"value".to_owned()));
        assert!(!wrapped_chunks.item_fields.contains(&"value".to_owned()));
        let direct_projection = direct
            .resource_graph()
            .list_projections
            .iter()
            .find(|projection| projection.target == direct_chunks.id)
            .expect("direct chunk projection");
        let wrapped_projection = wrapped
            .resource_graph()
            .list_projections
            .iter()
            .find(|projection| projection.target == wrapped_chunks.id)
            .expect("wrapped chunk projection");
        assert_eq!(direct_projection.source, SemanticListId(0));
        assert_eq!(wrapped_projection.source, SemanticListId(0));
        let SemanticListProjectionKindV1::Chunk {
            resolved_size: direct_size,
            ..
        } = direct_projection.kind;
        let SemanticListProjectionKindV1::Chunk {
            resolved_size: wrapped_size,
            ..
        } = wrapped_projection.kind;
        assert_eq!(direct_size, Some(2));
        assert_eq!(wrapped_size, Some(2));
    }

    #[test]
    fn chunk_projection_rejects_duplicate_arguments_and_nested_list_projections() {
        let semantic = elaborate_source(
            r#"
rows: LIST {
    [value: 1]
    [value: 2]
}
chunks: rows |> List/chunk(size: 2)
"#,
        );
        let chunk = semantic
            .execution_graph()
            .expressions
            .iter()
            .find_map(|expression| match &expression.kind {
                SemanticExpressionKind::Call {
                    name, arguments, ..
                } if name == "List/chunk" => Some((expression.id, arguments.clone())),
                _ => None,
            })
            .expect("semantic List/chunk call");

        for duplicate_name in ["list", "size"] {
            let mut execution = semantic.execution_graph().clone();
            let argument = chunk
                .1
                .iter()
                .find(|argument| argument.name == duplicate_name)
                .cloned()
                .expect("canonical chunk argument");
            let SemanticExpressionKind::Call { arguments, .. } =
                &mut execution.expressions[chunk.0.as_usize()].kind
            else {
                unreachable!("located chunk call");
            };
            arguments.push(argument);
            let error = discover_list_projections(&execution, &semantic.resource_graph().lists)
                .expect_err("duplicate chunk argument must fail closed");
            assert!(error.contains("exactly one checked"), "{error}");
        }

        let mut execution = semantic.execution_graph().clone();
        let list_argument = chunk
            .1
            .iter()
            .find(|argument| argument.name == "list")
            .cloned()
            .expect("canonical list argument");
        let input = execution.expressions[list_argument.value.as_usize()].clone();
        let projected_id = SemanticExprId(execution.expressions.len());
        execution.expressions.push(crate::SemanticExpression {
            id: projected_id,
            value_id: crate::SemanticValueId(projected_id.as_usize()),
            checked_expr_id: input.checked_expr_id,
            flow_type: input.flow_type,
            effect: input.effect,
            owner: input.owner,
            provenance: input.provenance,
            resource_binding_path: input.resource_binding_path,
            kind: SemanticExpressionKind::Project {
                input: list_argument.value,
                fields: vec!["nested".to_owned()],
            },
        });
        let SemanticExpressionKind::Call { arguments, .. } =
            &mut execution.expressions[chunk.0.as_usize()].kind
        else {
            unreachable!("located chunk call");
        };
        arguments
            .iter_mut()
            .find(|argument| argument.name == "list")
            .expect("list argument")
            .value = projected_id;
        let error = discover_list_projections(&execution, &semantic.resource_graph().lists)
            .expect_err("non-empty list projection must not inherit list identity");
        assert!(
            error.contains("no exact semantic list provenance"),
            "{error}"
        );
    }

    #[test]
    fn iterative_authority_walk_handles_deep_default_stack_graph() {
        const DEPTH: usize = 8_192;
        let mut graph = SemanticExecutionGraphV1::default();
        graph.expressions.push(crate::SemanticExpression {
            id: SemanticExprId(0),
            value_id: crate::SemanticValueId(0),
            checked_expr_id: boon_typecheck::CheckedExprId(0),
            flow_type: FlowType {
                mode: boon_typecheck::FlowMode::Continuous,
                ty: Type::List(Box::new(Type::Object(boon_typecheck::ObjectShape {
                    fields: BTreeMap::new(),
                    field_order: Vec::new(),
                    open: false,
                }))),
            },
            effect: boon_typecheck::CheckedEffectSummary::default(),
            owner: None,
            provenance: crate::SemanticValueProvenance::default(),
            resource_binding_path: None,
            kind: SemanticExpressionKind::List {
                capacity: None,
                items: Vec::new(),
            },
        });
        for index in 1..=DEPTH {
            graph.expressions.push(crate::SemanticExpression {
                id: SemanticExprId(index),
                value_id: crate::SemanticValueId(index),
                checked_expr_id: boon_typecheck::CheckedExprId(index as u32),
                flow_type: graph.expressions[0].flow_type.clone(),
                effect: boon_typecheck::CheckedEffectSummary::default(),
                owner: None,
                provenance: crate::SemanticValueProvenance::default(),
                resource_binding_path: None,
                kind: SemanticExpressionKind::Project {
                    input: SemanticExprId(index - 1),
                    fields: Vec::new(),
                },
            });
        }
        assert_eq!(
            inline_list_authority_root(&graph, SemanticExprId(DEPTH)).unwrap(),
            Some(SemanticExprId(0))
        );
    }

    #[test]
    fn resource_digest_rejects_mutated_storage_identity() {
        let semantic = elaborate_source("rows: LIST { [value: 1] }");
        let mut resources = semantic.resource_graph().clone();
        resources.lists[0].semantic_path.push_str(".mutated");
        let error = resources
            .validate(semantic.execution_graph(), semantic.resolved_out_graph())
            .expect_err("mutated resource graph must be rejected");
        assert!(error.contains("digest"), "{error}");
    }
}
