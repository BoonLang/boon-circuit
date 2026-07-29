//! Semantic-owned durable memory and migration topology.
//!
//! This is the last semantic boundary before executable allocation. Durable
//! authority is keyed by semantic resource, reactive-binding, and total
//! storage-field identities. Checked expression IDs are retained only for the
//! exact marker-equivalence gate; they are never reinterpreted as executable
//! identity.

use crate::{
    SemanticBindingId, SemanticBindingTargetV1, SemanticContextualOperationKind,
    SemanticExecutionGraphV1, SemanticExprId, SemanticExpression, SemanticExpressionKind,
    SemanticListId, SemanticLoweringContractV1, SemanticMaterializationId, SemanticMigrationId,
    SemanticReactiveGraphV1, SemanticReadTargetV1, SemanticResourceGraphV1, SemanticRowBinding,
    SemanticScopeStorageGraphV1, SemanticSelectKind, SemanticSourceUnitId, SemanticStateId,
    SemanticStatementKind, SemanticStorageBindingTargetV1, SemanticStorageFieldId,
    SemanticValueOrigin, StaticOwnerId,
};
use boon_contract::SourceBundleDigestV1;
use boon_typecheck::{
    CheckedExpression, CheckedExpressionKind, CheckedPassedAccess, CheckedProgram, Type, Variant,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

pub const SEMANTIC_MEMORY_GRAPH_SCHEMA_V1: &str = "boon.semantic-memory-graph.v1";
const SEMANTIC_MEMORY_GRAPH_DIGEST_DOMAIN: &[u8] = b"boon.semantic-memory-graph.v1\0";

macro_rules! memory_id {
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

memory_id!(SemanticMemoryId, SemanticMigrationEdgeId);

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SemanticMemoryGraphDigestV1([u8; 32]);

impl SemanticMemoryGraphDigestV1 {
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    pub fn to_hex(self) -> String {
        self.0.iter().map(|byte| format!("{byte:02x}")).collect()
    }
}

impl fmt::Display for SemanticMemoryGraphDigestV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.to_hex())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticMemoryGraphV1 {
    pub schema: String,
    pub source_bundle_digest_v1: SourceBundleDigestV1,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub memories: Vec<SemanticMemoryV1>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub migration_edges: Vec<SemanticMigrationEdgeV1>,
    pub digest: SemanticMemoryGraphDigestV1,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticMemoryKindV1 {
    RootScalar,
    IndexedField,
    ListOwner,
    Map,
    Set,
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct SemanticMemoryIdentityV1 {
    pub source_unit: SemanticSourceUnitId,
    pub canonical_module: String,
    pub owner_path: String,
    pub semantic_path: String,
    pub kind: SemanticMemoryKindV1,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticMemoryV1 {
    pub id: SemanticMemoryId,
    pub identity: SemanticMemoryIdentityV1,
    pub backing: SemanticMemoryBackingV1,
    pub data_type: Type,
    pub leaves: Vec<SemanticMemoryLeafV1>,
    pub status: SemanticMemoryStatusV1,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SemanticMemoryBackingV1 {
    State {
        binding: SemanticBindingId,
        storage_field: SemanticStorageFieldId,
        state: SemanticStateId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        row: Option<SemanticRowBinding>,
    },
    List {
        binding: SemanticBindingId,
        storage_field: SemanticStorageFieldId,
        list: SemanticListId,
        row: SemanticRowBinding,
    },
    Collection {
        expression: SemanticExprId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        owner: Option<StaticOwnerId>,
    },
}

impl SemanticMemoryBackingV1 {
    pub const fn binding(self) -> Option<SemanticBindingId> {
        match self {
            Self::State { binding, .. } | Self::List { binding, .. } => Some(binding),
            Self::Collection { .. } => None,
        }
    }

    pub const fn storage_field(self) -> Option<SemanticStorageFieldId> {
        match self {
            Self::State { storage_field, .. } | Self::List { storage_field, .. } => {
                Some(storage_field)
            }
            Self::Collection { .. } => None,
        }
    }

    pub const fn row(self) -> Option<SemanticRowBinding> {
        match self {
            Self::State { row, .. } => row,
            Self::List { row, .. } => Some(row),
            Self::Collection { .. } => None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticMemoryLeafV1 {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub projection: Vec<String>,
    pub data_type: Type,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SemanticMemoryStatusV1 {
    Active,
    Draining { marker: SemanticMigrationId },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticMigrationEdgeV1 {
    pub id: SemanticMigrationEdgeId,
    pub inputs: Vec<SemanticMigrationDrainV1>,
    pub destination: SemanticMemoryRegionV1,
    pub initializer: SemanticMigrationInitializerV1,
    pub transfer: SemanticMigrationTransferV1,
    pub transform: SemanticMigrationTransformV1,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct SemanticMigrationDrainV1 {
    pub expression: SemanticExprId,
    pub source: SemanticMemoryRegionV1,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct SemanticMemoryRegionV1 {
    pub memory: SemanticMemoryId,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub projection: Vec<String>,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SemanticMigrationInitializerV1 {
    State {
        state: SemanticStateId,
        root: SemanticExprId,
    },
    ListMaterialization {
        list: SemanticListId,
        materialization: SemanticMaterializationId,
        source_root: SemanticExprId,
    },
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SemanticMigrationTransferV1 {
    Scalar,
    IndexedField {
        owner: SemanticRowBinding,
    },
    List {
        source: SemanticRowBinding,
        destination: SemanticRowBinding,
    },
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SemanticMigrationTransformV1 {
    Identity { input: SemanticExprId },
    PureExpression { root: SemanticExprId },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SemanticMemoryError {
    message: String,
}

impl SemanticMemoryError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for SemanticMemoryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for SemanticMemoryError {}

#[derive(Clone, Debug)]
struct ResolvedMemoryRegion {
    region: SemanticMemoryRegionV1,
    leaf_indexes: Vec<usize>,
    data_type: Type,
}

#[derive(Clone, Debug)]
struct DrainUse {
    drain: SemanticExprId,
    source: ResolvedMemoryRegion,
    destination: SemanticMemoryRegionV1,
    initializer: SemanticMigrationInitializerV1,
}

pub(crate) fn build_semantic_memory_graph(
    checked: &CheckedProgram,
    execution: &SemanticExecutionGraphV1,
    resources: &SemanticResourceGraphV1,
    reactive: &SemanticReactiveGraphV1,
    storage: &SemanticScopeStorageGraphV1,
    lowering: &SemanticLoweringContractV1,
) -> Result<SemanticMemoryGraphV1, SemanticMemoryError> {
    if checked.source_bundle_digest_v1 != storage.source_bundle_digest_v1
        || checked.source_bundle_digest_v1 != lowering.metadata.source_bundle_digest_v1
    {
        return Err(SemanticMemoryError::new(
            "semantic memory inputs disagree on source-bundle identity",
        ));
    }

    let reachable = reachable_semantic_expressions(execution)?;
    validate_checked_marker_equivalence(checked, execution, &reachable)?;
    let mut memories = build_memories(
        execution, &reachable, resources, reactive, storage, lowering,
    )?;
    attach_draining_markers(execution, reactive, storage, &reachable, &mut memories)?;
    let drains = collect_drains(
        execution, resources, reactive, storage, &reachable, &memories,
    )?;
    validate_source_coverage(&memories, &drains)?;
    validate_no_ordinary_draining_reads(execution, reactive, storage, &reachable, &memories)?;
    let migration_edges = lower_migration_edges(execution, resources, &memories, &drains)?;
    validate_migration_cycles(&memories, &migration_edges)?;

    let mut graph = SemanticMemoryGraphV1 {
        schema: SEMANTIC_MEMORY_GRAPH_SCHEMA_V1.to_owned(),
        source_bundle_digest_v1: checked.source_bundle_digest_v1,
        memories,
        migration_edges,
        digest: SemanticMemoryGraphDigestV1([0; 32]),
    };
    validate_memory_shape(&graph, execution, resources, reactive, storage)?;
    graph.digest = memory_graph_digest(&graph)?;
    Ok(graph)
}

impl SemanticMemoryGraphV1 {
    pub(crate) fn validate(
        &self,
        checked: &CheckedProgram,
        execution: &SemanticExecutionGraphV1,
        resources: &SemanticResourceGraphV1,
        reactive: &SemanticReactiveGraphV1,
        storage: &SemanticScopeStorageGraphV1,
        lowering: &SemanticLoweringContractV1,
    ) -> Result<(), SemanticMemoryError> {
        let expected = build_semantic_memory_graph(
            checked, execution, resources, reactive, storage, lowering,
        )?;
        if self != &expected {
            return Err(SemanticMemoryError::new(
                "semantic memory graph differs from its deterministic semantic derivation",
            ));
        }
        Ok(())
    }
}

fn build_memories(
    execution: &SemanticExecutionGraphV1,
    reachable: &BTreeSet<SemanticExprId>,
    resources: &SemanticResourceGraphV1,
    reactive: &SemanticReactiveGraphV1,
    storage: &SemanticScopeStorageGraphV1,
    lowering: &SemanticLoweringContractV1,
) -> Result<Vec<SemanticMemoryV1>, SemanticMemoryError> {
    let mut memories = Vec::with_capacity(
        resources.states.len() + resources.lists.len() + execution.expressions.len(),
    );
    let mut identities = BTreeMap::<SemanticMemoryIdentityV1, String>::new();

    for state in resources.states.iter().filter(|state| state.published) {
        let semantic_path = state.semantic_path.clone().ok_or_else(|| {
            SemanticMemoryError::new(format!(
                "published semantic state {} has no exact semantic path",
                state.id
            ))
        })?;
        let binding = exact_reactive_binding(
            reactive,
            |binding| binding.target == SemanticBindingTargetV1::State { state: state.id },
            &format!("published state {}", state.id),
        )?;
        let storage_binding = exact_storage_binding(storage, binding.id)?;
        let (storage_field, row) = match &storage_binding.target {
            SemanticStorageBindingTargetV1::State {
                state: target,
                published: true,
                field: Some(field),
                row,
            } if *target == state.id => (*field, *row),
            ref target => {
                return Err(SemanticMemoryError::new(format!(
                    "published state {} binding {} has non-authoritative storage target {target:?}",
                    state.id, binding.id
                )));
            }
        };
        let expected_row = state
            .target_list
            .zip(state.row_scope)
            .map(|(list, scope)| SemanticRowBinding { list, scope });
        if row != expected_row {
            return Err(SemanticMemoryError::new(format!(
                "published state {} resource row {expected_row:?} differs from storage row {row:?}",
                state.id
            )));
        }
        let field = require_storage_field(storage, storage_field)?;
        if field.flow_type != state.flow_type {
            return Err(SemanticMemoryError::new(format!(
                "published state {} type {:?} differs from storage field {} type {:?}",
                state.id, state.flow_type, storage_field, field.flow_type
            )));
        }
        ensure_closed_memory_type(
            &state.flow_type.ty,
            &format!("published state `{semantic_path}`"),
        )?;
        let (source_unit, canonical_module) =
            exact_source_unit(lowering, state.span.line, &semantic_path)?;
        let (kind, owner_path) = if let Some(row) = row {
            let list = require_list(resources, row.list)?;
            if list.row_scope != row.scope {
                return Err(SemanticMemoryError::new(format!(
                    "indexed state {} row scope {} differs from list {} row scope {}",
                    state.id, row.scope, list.id, list.row_scope
                )));
            }
            (
                SemanticMemoryKindV1::IndexedField,
                list.semantic_path.clone(),
            )
        } else {
            (
                SemanticMemoryKindV1::RootScalar,
                semantic_parent_path(&semantic_path)?,
            )
        };
        let identity = SemanticMemoryIdentityV1 {
            source_unit,
            canonical_module,
            owner_path,
            semantic_path,
            kind,
        };
        insert_memory_identity(&mut identities, &identity, format!("state {}", state.id))?;
        memories.push(SemanticMemoryV1 {
            id: SemanticMemoryId(memories.len()),
            leaves: semantic_memory_leaves(&state.flow_type.ty),
            identity,
            backing: SemanticMemoryBackingV1::State {
                binding: binding.id,
                storage_field,
                state: state.id,
                row,
            },
            data_type: state.flow_type.ty.clone(),
            status: SemanticMemoryStatusV1::Active,
        });
    }

    for list in &resources.lists {
        let binding = exact_reactive_binding(
            reactive,
            |binding| binding.target == SemanticBindingTargetV1::List { list: list.id },
            &format!("list {}", list.id),
        )?;
        let storage_binding = exact_storage_binding(storage, binding.id)?;
        let (storage_field, row) = match &storage_binding.target {
            SemanticStorageBindingTargetV1::List {
                list: target,
                field,
                row,
            } if *target == list.id => (*field, *row),
            ref target => {
                return Err(SemanticMemoryError::new(format!(
                    "list {} binding {} has non-list storage target {target:?}",
                    list.id, binding.id
                )));
            }
        };
        let expected_row = SemanticRowBinding {
            list: list.id,
            scope: list.row_scope,
        };
        if row != expected_row {
            return Err(SemanticMemoryError::new(format!(
                "list {} resource row {expected_row:?} differs from storage row {row:?}",
                list.id
            )));
        }
        let data_type = Type::List(Box::new(list.item_type.clone()));
        let field = require_storage_field(storage, storage_field)?;
        if field.flow_type.ty != data_type {
            return Err(SemanticMemoryError::new(format!(
                "list {} type differs from storage field {}",
                list.id, storage_field
            )));
        }
        ensure_closed_memory_type(&data_type, &format!("list `{}`", list.semantic_path))?;
        let (source_unit, canonical_module) =
            exact_source_unit(lowering, list.span.line, &list.semantic_path)?;
        let identity = SemanticMemoryIdentityV1 {
            source_unit,
            canonical_module,
            owner_path: semantic_parent_path(&list.semantic_path)?,
            semantic_path: list.semantic_path.clone(),
            kind: SemanticMemoryKindV1::ListOwner,
        };
        insert_memory_identity(&mut identities, &identity, format!("list {}", list.id))?;
        memories.push(SemanticMemoryV1 {
            id: SemanticMemoryId(memories.len()),
            leaves: vec![SemanticMemoryLeafV1 {
                projection: Vec::new(),
                data_type: data_type.clone(),
            }],
            identity,
            backing: SemanticMemoryBackingV1::List {
                binding: binding.id,
                storage_field,
                list: list.id,
                row,
            },
            data_type,
            status: SemanticMemoryStatusV1::Active,
        });
    }

    for expression in execution
        .expressions
        .iter()
        .filter(|expression| reachable.contains(&expression.id))
    {
        let kind = match expression.kind {
            SemanticExpressionKind::Map { .. } => SemanticMemoryKindV1::Map,
            SemanticExpressionKind::Set { .. } => SemanticMemoryKindV1::Set,
            _ => continue,
        };
        // An empty collection can remain intentionally unconstrained when it
        // is used only as a runtime authority. It does not become durable
        // schema until the compiler can prove its complete key/item/value
        // type.
        if !type_is_closed_memory_data(&expression.flow_type.ty) {
            continue;
        }
        let origin = execution
            .checked_expression_origins
            .get(expression.id.as_usize())
            .filter(|origin| origin.expression == expression.id)
            .ok_or_else(|| {
                SemanticMemoryError::new(format!(
                    "collection authority expression {} has no exact checked origin",
                    expression.id
                ))
            })?;
        let semantic_path = collection_statement_path(execution, expression.id)?.ok_or_else(|| {
            SemanticMemoryError::new(format!(
                "collection authority expression {} has no stable named semantic path; bind it beneath one named field",
                expression.id
            ))
        })?;
        let (source_unit, canonical_module) =
            exact_source_unit(lowering, origin.checked_span.line, &semantic_path)?;
        let identity = SemanticMemoryIdentityV1 {
            source_unit,
            canonical_module,
            owner_path: semantic_parent_path(&semantic_path)?,
            semantic_path,
            kind,
        };
        insert_memory_identity(
            &mut identities,
            &identity,
            format!("collection expression {}", expression.id),
        )?;
        memories.push(SemanticMemoryV1 {
            id: SemanticMemoryId(memories.len()),
            leaves: vec![SemanticMemoryLeafV1 {
                projection: Vec::new(),
                data_type: expression.flow_type.ty.clone(),
            }],
            identity,
            backing: SemanticMemoryBackingV1::Collection {
                expression: expression.id,
                owner: expression.owner,
            },
            data_type: expression.flow_type.ty.clone(),
            status: SemanticMemoryStatusV1::Active,
        });
    }

    Ok(memories)
}

fn collection_statement_path(
    execution: &SemanticExecutionGraphV1,
    expression: SemanticExprId,
) -> Result<Option<String>, SemanticMemoryError> {
    let mut candidates = Vec::<(String, SemanticExprId)>::new();
    for statement in &execution.statements {
        let Some(root) = statement.value else {
            continue;
        };
        if !expression_reaches(execution, root, expression)? {
            continue;
        }
        let path = match &statement.kind {
            SemanticStatementKind::Field { path, .. } => Some(path.clone()),
            SemanticStatementKind::Source { path, .. }
            | SemanticStatementKind::Hold { path, .. }
            | SemanticStatementKind::List { path, .. } => path.clone(),
            _ => None,
        };
        if let Some(path) = path {
            candidates.push((path, root));
        }
    }
    let max_depth = candidates
        .iter()
        .map(|(path, _)| path.split('.').count())
        .max();
    let Some(max_depth) = max_depth else {
        return Ok(None);
    };
    candidates.retain(|(path, _)| path.split('.').count() == max_depth);
    let mut resolved = candidates
        .into_iter()
        .map(|(path, root)| {
            let route = expression_structural_route(execution, root, expression)?;
            Ok(if route.is_empty() {
                path
            } else {
                format!("{path}.@authority:{}", route.join("/"))
            })
        })
        .collect::<Result<Vec<_>, SemanticMemoryError>>()?;
    resolved.sort();
    resolved.dedup();
    match resolved.as_slice() {
        [path] => Ok(Some(path.clone())),
        _ => Err(SemanticMemoryError::new(format!(
            "collection authority expression {expression} is contained by multiple equally specific named authority paths {resolved:?}; construct each authority beneath one unique parent"
        ))),
    }
}

fn expression_structural_route(
    execution: &SemanticExecutionGraphV1,
    root: SemanticExprId,
    target: SemanticExprId,
) -> Result<Vec<String>, SemanticMemoryError> {
    if root == target {
        return Ok(Vec::new());
    }
    let mut pending = vec![(root, Vec::<String>::new(), BTreeSet::new())];
    let mut routes = Vec::new();
    let mut visits = 0_usize;
    let visit_limit = execution.expressions.len().saturating_mul(8).max(64);
    while let Some((expression_id, route, mut active)) = pending.pop() {
        visits = visits.saturating_add(1);
        if visits > visit_limit {
            return Err(SemanticMemoryError::new(format!(
                "collection authority route from {root} to {target} exceeds the bounded semantic graph walk"
            )));
        }
        if !active.insert(expression_id) {
            return Err(SemanticMemoryError::new(format!(
                "collection authority route from {root} to {target} contains a semantic cycle at {expression_id}"
            )));
        }
        let expression = require_expression(execution, expression_id)?;
        for (index, child) in expression_children(execution, &expression.kind)?
            .into_iter()
            .enumerate()
            .rev()
        {
            let mut child_route = route.clone();
            child_route.push(expression_child_route_segment(expression, index));
            if child == target {
                routes.push(child_route);
                if routes.len() > 1 {
                    return Err(SemanticMemoryError::new(format!(
                        "collection authority expression {target} is reachable through multiple structural parents beneath {root}"
                    )));
                }
            } else {
                pending.push((child, child_route, active.clone()));
            }
        }
    }
    routes.pop().ok_or_else(|| {
        SemanticMemoryError::new(format!(
            "collection authority expression {target} is not reachable from named root {root}"
        ))
    })
}

fn expression_child_route_segment(expression: &SemanticExpression, index: usize) -> String {
    match &expression.kind {
        SemanticExpressionKind::TaggedObject { fields, .. }
        | SemanticExpressionKind::Object(fields) => fields
            .get(index)
            .map(|field| format!("field:{}", authority_path_segment(&field.name)))
            .unwrap_or_else(|| format!("field:{index}")),
        SemanticExpressionKind::Call { arguments, .. } => arguments
            .get(index)
            .map(|argument| {
                format!(
                    "arg:{}:{}",
                    argument.ordinal,
                    authority_path_segment(&argument.name)
                )
            })
            .unwrap_or_else(|| format!("arg:{index}")),
        SemanticExpressionKind::Map { .. } => format!("entry:{index}"),
        SemanticExpressionKind::MapEntry { .. } => match index {
            0 => "key".to_owned(),
            1 => "value".to_owned(),
            _ => format!("entry-child:{index}"),
        },
        SemanticExpressionKind::List { .. } => format!("list-item:{index}"),
        SemanticExpressionKind::Set { .. } => format!("set-item:{index}"),
        SemanticExpressionKind::Block { bindings, .. } if index < bindings.len() => {
            format!("binding:{index}")
        }
        SemanticExpressionKind::Block { .. } => "result".to_owned(),
        SemanticExpressionKind::Then { .. } => match index {
            0 => "input".to_owned(),
            1 => "output".to_owned(),
            _ => format!("then-child:{index}"),
        },
        SemanticExpressionKind::When { .. } => match index {
            0 => "input".to_owned(),
            _ => format!("arm:{}", index - 1),
        },
        _ => format!("child:{index}"),
    }
}

fn authority_path_segment(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-') {
            output.push(char::from(byte));
        } else {
            use std::fmt::Write as _;
            let _ = write!(output, "%{byte:02X}");
        }
    }
    output
}

fn exact_reactive_binding<'a>(
    reactive: &'a SemanticReactiveGraphV1,
    predicate: impl Fn(&crate::SemanticBindingV1) -> bool,
    context: &str,
) -> Result<&'a crate::SemanticBindingV1, SemanticMemoryError> {
    let matches = reactive
        .bindings
        .iter()
        .filter(|binding| predicate(binding))
        .collect::<Vec<_>>();
    let [binding] = matches.as_slice() else {
        return Err(SemanticMemoryError::new(format!(
            "{context} resolves to {} exact reactive bindings",
            matches.len()
        )));
    };
    Ok(*binding)
}

fn exact_storage_binding(
    storage: &SemanticScopeStorageGraphV1,
    binding: SemanticBindingId,
) -> Result<&crate::SemanticStorageBindingV1, SemanticMemoryError> {
    let matches = storage
        .bindings
        .iter()
        .filter(|candidate| candidate.binding == binding)
        .collect::<Vec<_>>();
    let [binding] = matches.as_slice() else {
        return Err(SemanticMemoryError::new(format!(
            "reactive binding {binding} resolves to {} exact storage bindings",
            matches.len()
        )));
    };
    Ok(*binding)
}

fn require_storage_field(
    storage: &SemanticScopeStorageGraphV1,
    field: SemanticStorageFieldId,
) -> Result<&crate::SemanticStorageFieldV1, SemanticMemoryError> {
    storage
        .fields
        .get(field.as_usize())
        .filter(|candidate| candidate.id == field)
        .ok_or_else(|| {
            SemanticMemoryError::new(format!(
                "semantic memory references missing storage field {field}"
            ))
        })
}

fn require_list(
    resources: &SemanticResourceGraphV1,
    list: SemanticListId,
) -> Result<&crate::SemanticListResourceV1, SemanticMemoryError> {
    resources
        .lists
        .get(list.as_usize())
        .filter(|candidate| candidate.id == list)
        .ok_or_else(|| SemanticMemoryError::new(format!("missing semantic list {list}")))
}

fn require_state(
    resources: &SemanticResourceGraphV1,
    state: SemanticStateId,
) -> Result<&crate::SemanticStateResourceV1, SemanticMemoryError> {
    resources
        .states
        .get(state.as_usize())
        .filter(|candidate| candidate.id == state)
        .ok_or_else(|| SemanticMemoryError::new(format!("missing semantic state {state}")))
}

fn insert_memory_identity(
    identities: &mut BTreeMap<SemanticMemoryIdentityV1, String>,
    identity: &SemanticMemoryIdentityV1,
    owner: String,
) -> Result<(), SemanticMemoryError> {
    if let Some(previous) = identities.insert(identity.clone(), owner.clone()) {
        return Err(SemanticMemoryError::new(format!(
            "semantic memory identity `{}` is shared by {previous} and {owner}",
            identity.semantic_path
        )));
    }
    Ok(())
}

fn exact_source_unit(
    lowering: &SemanticLoweringContractV1,
    line: usize,
    path: &str,
) -> Result<(SemanticSourceUnitId, String), SemanticMemoryError> {
    let matches = lowering
        .metadata
        .source_units
        .iter()
        .filter(|unit| {
            unit.start_line
                .checked_add(unit.line_count)
                .is_some_and(|end| line >= unit.start_line && line < end)
        })
        .collect::<Vec<_>>();
    let [unit] = matches.as_slice() else {
        return Err(SemanticMemoryError::new(format!(
            "semantic memory `{path}` at line {line} resolves to {} exact source units",
            matches.len()
        )));
    };
    let canonical_module = match &unit.module {
        Some(module) if module.trim().is_empty() => {
            return Err(SemanticMemoryError::new(format!(
                "semantic memory `{path}` belongs to source unit {} with an empty module identity",
                unit.id
            )));
        }
        Some(module) => module.clone(),
        None => "$root".to_owned(),
    };
    Ok((unit.id, canonical_module))
}

fn semantic_parent_path(path: &str) -> Result<String, SemanticMemoryError> {
    if path.trim().is_empty() {
        return Err(SemanticMemoryError::new(
            "semantic memory path must not be empty",
        ));
    }
    Ok(path
        .rsplit_once('.')
        .map(|(parent, _)| parent)
        .filter(|parent| !parent.is_empty())
        .unwrap_or("$root")
        .to_owned())
}

fn semantic_memory_leaves(data_type: &Type) -> Vec<SemanticMemoryLeafV1> {
    fn collect(
        data_type: &Type,
        projection: &mut Vec<String>,
        leaves: &mut Vec<SemanticMemoryLeafV1>,
    ) {
        let Type::Object(shape) = data_type else {
            leaves.push(SemanticMemoryLeafV1 {
                projection: projection.clone(),
                data_type: data_type.clone(),
            });
            return;
        };
        if shape.fields.is_empty() {
            leaves.push(SemanticMemoryLeafV1 {
                projection: projection.clone(),
                data_type: data_type.clone(),
            });
            return;
        }
        for (name, field) in &shape.fields {
            projection.push(name.clone());
            collect(field, projection, leaves);
            projection.pop();
        }
    }

    let mut leaves = Vec::new();
    collect(data_type, &mut Vec::new(), &mut leaves);
    leaves
}

fn ensure_closed_memory_type(data_type: &Type, context: &str) -> Result<(), SemanticMemoryError> {
    if type_is_closed_memory_data(data_type) {
        Ok(())
    } else {
        Err(SemanticMemoryError::new(format!(
            "{context} has an open, unresolved, or non-data memory type {data_type:?}"
        )))
    }
}

fn type_is_closed_memory_data(data_type: &Type) -> bool {
    match data_type {
        Type::Text | Type::Number | Type::Bytes(_) => true,
        Type::VariantSet(variants) => variants.iter().all(|variant| match variant {
            Variant::Tag(_) => true,
            Variant::Tagged { fields, .. } => {
                !fields.open && fields.fields.values().all(type_is_closed_memory_data)
            }
        }),
        Type::Object(shape) => !shape.open && shape.fields.values().all(type_is_closed_memory_data),
        Type::List(item) | Type::Set(item) => type_is_closed_memory_data(item),
        Type::Map { key, value } => {
            type_is_closed_memory_data(key) && type_is_closed_memory_data(value)
        }
        Type::Union(members) => {
            !members.is_empty() && members.iter().all(type_is_closed_memory_data)
        }
        Type::Absent
        | Type::Function { .. }
        | Type::RenderContract
        | Type::UnresolvedShape { .. }
        | Type::Var(_)
        | Type::Unknown => false,
    }
}

fn reachable_semantic_expressions(
    execution: &SemanticExecutionGraphV1,
) -> Result<BTreeSet<SemanticExprId>, SemanticMemoryError> {
    let child_statements = execution
        .statements
        .iter()
        .flat_map(|statement| statement.children.iter().copied())
        .collect::<BTreeSet<_>>();
    let mut pending = execution
        .statements
        .iter()
        .filter(|statement| !child_statements.contains(&statement.id))
        .filter_map(|statement| statement.value)
        .chain(execution.roots.iter().map(|root| root.expression))
        .chain(execution.functions.iter().map(|function| function.root))
        .chain(execution.sources.iter().map(|source| source.expression))
        .chain(execution.states.iter().map(|state| state.expression))
        .collect::<Vec<_>>();
    let mut reachable = BTreeSet::new();
    loop {
        while let Some(expression) = pending.pop() {
            if !reachable.insert(expression) {
                continue;
            }
            let expression = require_expression(execution, expression)?;
            pending.extend(expression_children(execution, &expression.kind)?);
        }
        let reverse_markers = execution
            .expressions
            .iter()
            .filter_map(|expression| match expression.kind {
                SemanticExpressionKind::Draining { input }
                    if reachable.contains(&input) && !reachable.contains(&expression.id) =>
                {
                    Some(expression.id)
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        if reverse_markers.is_empty() {
            break;
        }
        pending.extend(reverse_markers);
    }
    Ok(reachable)
}

fn validate_checked_marker_equivalence(
    checked: &CheckedProgram,
    execution: &SemanticExecutionGraphV1,
    reachable: &BTreeSet<SemanticExprId>,
) -> Result<(), SemanticMemoryError> {
    for expression in execution
        .expressions
        .iter()
        .filter(|expression| reachable.contains(&expression.id))
    {
        match &expression.kind {
            SemanticExpressionKind::Drain {
                target, projection, ..
            } => {
                let checked_expression =
                    require_checked_expression(checked, expression.checked_expr_id)?;
                validate_checked_drain_marker(
                    expression.id,
                    expression.checked_expr_id,
                    target,
                    projection,
                    &checked_expression.kind,
                )?;
            }
            SemanticExpressionKind::Draining { input } => {
                let checked_expression =
                    require_checked_expression(checked, expression.checked_expr_id)?;
                let CheckedExpressionKind::Draining {
                    input: checked_input,
                } = &checked_expression.kind
                else {
                    return Err(SemanticMemoryError::new(format!(
                        "semantic DRAINING expression {} does not match checked marker {}",
                        expression.id, expression.checked_expr_id.0
                    )));
                };
                let semantic_input = require_expression(execution, *input)?;
                if semantic_input.checked_expr_id != *checked_input {
                    return Err(SemanticMemoryError::new(format!(
                        "semantic DRAINING expression {} input {} has checked origin {}, expected {}",
                        expression.id, input, semantic_input.checked_expr_id.0, checked_input.0
                    )));
                }
            }
            _ => {}
        }
    }
    Ok(())
}

fn validate_checked_drain_marker(
    expression: SemanticExprId,
    checked_expression: boon_typecheck::CheckedExprId,
    target: &boon_typecheck::DeclId,
    projection: &[String],
    checked_kind: &CheckedExpressionKind,
) -> Result<(), SemanticMemoryError> {
    match checked_kind {
        CheckedExpressionKind::Drain {
            target: checked_target,
            projection: checked_projection,
        } if checked_target == target && checked_projection == projection => Ok(()),
        CheckedExpressionKind::Passed {
            formal,
            access: CheckedPassedAccess::Drain,
            ..
        } => Err(SemanticMemoryError::new(format!(
            "DRAIN expression {expression} originates from PASSED formal {formal:?}; migration rejects PASSED drains until ContextFormalId is explicit in semantic marker identity"
        ))),
        other => Err(SemanticMemoryError::new(format!(
            "semantic DRAIN expression {expression} does not exactly match checked marker {} ({other:?})",
            checked_expression.0
        ))),
    }
}

fn attach_draining_markers(
    execution: &SemanticExecutionGraphV1,
    reactive: &SemanticReactiveGraphV1,
    storage: &SemanticScopeStorageGraphV1,
    reachable: &BTreeSet<SemanticExprId>,
    memories: &mut [SemanticMemoryV1],
) -> Result<(), SemanticMemoryError> {
    let marker_records = reactive
        .migration_inputs
        .iter()
        .map(|marker| (marker.marker, marker))
        .collect::<BTreeMap<_, _>>();
    if marker_records.len() != reactive.migration_inputs.len() {
        return Err(SemanticMemoryError::new(
            "reactive migration inputs contain duplicate DRAINING marker identities",
        ));
    }

    for expression in execution
        .expressions
        .iter()
        .filter(|expression| reachable.contains(&expression.id))
    {
        let SemanticExpressionKind::Draining { input } = expression.kind else {
            continue;
        };
        let marker = marker_records.get(&expression.id).ok_or_else(|| {
            SemanticMemoryError::new(format!(
                "reachable DRAINING expression {} has no reactive migration identity",
                expression.id
            ))
        })?;
        if marker.input != input
            || marker.marker_value != expression.value_id
            || marker.input_value != require_expression(execution, input)?.value_id
            || marker.owner != expression.owner
        {
            return Err(SemanticMemoryError::new(format!(
                "DRAINING expression {} differs from reactive migration input {}",
                expression.id, marker.id
            )));
        }

        let candidate_bindings = reactive
            .bindings
            .iter()
            .filter(|binding| binding.producer == expression.id)
            .map(|binding| binding.id)
            .collect::<BTreeSet<_>>();
        let candidates = memories
            .iter()
            .filter(|memory| {
                memory
                    .backing
                    .binding()
                    .is_some_and(|binding| candidate_bindings.contains(&binding))
            })
            .map(|memory| memory.id)
            .collect::<Vec<_>>();
        let [memory_id] = candidates.as_slice() else {
            let targets = candidate_bindings
                .iter()
                .map(|binding| {
                    exact_storage_binding(storage, *binding)
                        .map(|storage| format!("{:?}", storage.target))
                })
                .collect::<Result<Vec<_>, _>>()?;
            return Err(SemanticMemoryError::new(format!(
                "DRAINING expression {} resolves to {} exact durable memories; binding targets={targets:?}",
                expression.id,
                candidates.len()
            )));
        };
        let memory = memories
            .get_mut(memory_id.as_usize())
            .filter(|memory| memory.id == *memory_id)
            .ok_or_else(|| {
                SemanticMemoryError::new(format!(
                    "DRAINING marker references missing memory {memory_id}"
                ))
            })?;
        if let SemanticMemoryStatusV1::Draining { marker: previous } = memory.status {
            return Err(SemanticMemoryError::new(format!(
                "semantic memory `{}` has multiple DRAINING markers {} and {}",
                memory.identity.semantic_path, previous, marker.id
            )));
        }
        memory.status = SemanticMemoryStatusV1::Draining { marker: marker.id };
    }

    for marker in &reactive.migration_inputs {
        if reachable.contains(&marker.marker)
            && !memories.iter().any(|memory| {
                memory.status == (SemanticMemoryStatusV1::Draining { marker: marker.id })
            })
        {
            return Err(SemanticMemoryError::new(format!(
                "reactive DRAINING marker {} is not attached to durable memory",
                marker.id
            )));
        }
    }
    Ok(())
}

fn require_expression(
    execution: &SemanticExecutionGraphV1,
    expression: SemanticExprId,
) -> Result<&SemanticExpression, SemanticMemoryError> {
    execution
        .expressions
        .get(expression.as_usize())
        .filter(|candidate| candidate.id == expression)
        .ok_or_else(|| {
            SemanticMemoryError::new(format!(
                "semantic memory topology references missing expression {expression}"
            ))
        })
}

fn require_checked_expression(
    checked: &CheckedProgram,
    expression: boon_typecheck::CheckedExprId,
) -> Result<&CheckedExpression, SemanticMemoryError> {
    checked
        .expressions
        .get(expression.0 as usize)
        .filter(|candidate| candidate.id == expression)
        .ok_or_else(|| {
            SemanticMemoryError::new(format!(
                "semantic migration references missing checked expression {}",
                expression.0
            ))
        })
}

fn expression_children(
    execution: &SemanticExecutionGraphV1,
    kind: &SemanticExpressionKind,
) -> Result<Vec<SemanticExprId>, SemanticMemoryError> {
    Ok(match kind {
        SemanticExpressionKind::CanonicalRead { .. }
        | SemanticExpressionKind::LocalRead { .. }
        | SemanticExpressionKind::ExternalRead { .. }
        | SemanticExpressionKind::ElementState { .. }
        | SemanticExpressionKind::Drain { .. }
        | SemanticExpressionKind::Text(_)
        | SemanticExpressionKind::Number(_)
        | SemanticExpressionKind::BytesByte(_)
        | SemanticExpressionKind::Absent
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
                SemanticMemoryError::new(format!(
                    "semantic expression references missing materialization {materialization}"
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
        SemanticExpressionKind::Then { input, output } => std::iter::once(*input)
            .chain(output.iter().copied())
            .collect(),
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
    })
}

fn expression_reaches(
    execution: &SemanticExecutionGraphV1,
    root: SemanticExprId,
    target: SemanticExprId,
) -> Result<bool, SemanticMemoryError> {
    let mut pending = vec![root];
    let mut visited = BTreeSet::new();
    while let Some(expression) = pending.pop() {
        if !visited.insert(expression) {
            continue;
        }
        if expression == target {
            return Ok(true);
        }
        let expression = require_expression(execution, expression)?;
        pending.extend(expression_children(execution, &expression.kind)?);
    }
    Ok(false)
}

fn expression_subtree(
    execution: &SemanticExecutionGraphV1,
    root: SemanticExprId,
) -> Result<BTreeSet<SemanticExprId>, SemanticMemoryError> {
    let mut pending = vec![root];
    let mut visited = BTreeSet::new();
    while let Some(expression) = pending.pop() {
        if !visited.insert(expression) {
            continue;
        }
        let expression = require_expression(execution, expression)?;
        pending.extend(expression_children(execution, &expression.kind)?);
    }
    Ok(visited)
}

fn collect_drains(
    execution: &SemanticExecutionGraphV1,
    resources: &SemanticResourceGraphV1,
    reactive: &SemanticReactiveGraphV1,
    storage: &SemanticScopeStorageGraphV1,
    reachable: &BTreeSet<SemanticExprId>,
    memories: &[SemanticMemoryV1],
) -> Result<Vec<DrainUse>, SemanticMemoryError> {
    let mut drains = Vec::new();
    for expression in execution
        .expressions
        .iter()
        .filter(|expression| reachable.contains(&expression.id))
    {
        let SemanticExpressionKind::Drain { projection, .. } = &expression.kind else {
            continue;
        };
        let binding = resolve_drain_binding(execution, reactive, expression)?;
        let storage_binding = exact_storage_binding(storage, binding.id)?;
        let source_memory = memory_for_storage_binding(memories, storage_binding)?;
        let source = resolve_memory_region(source_memory, projection)?;
        if expression.flow_type.ty != source.data_type {
            return Err(SemanticMemoryError::new(format!(
                "DRAIN expression {} type {:?} differs from source region `{}` type {:?}",
                expression.id,
                expression.flow_type.ty,
                memory_region_path(source_memory, projection),
                source.data_type
            )));
        }

        let destination_candidates =
            exact_drain_destination_candidates(execution, resources, memories, expression.id)?;
        let [destination] = destination_candidates.as_slice() else {
            return Err(SemanticMemoryError::new(format!(
                "DRAIN expression {} is contained by {} exact destination authorities",
                expression.id,
                destination_candidates.len()
            )));
        };
        let destination_memory = require_memory(memories, destination.memory)?;
        if !matches!(destination_memory.status, SemanticMemoryStatusV1::Active) {
            return Err(SemanticMemoryError::new(format!(
                "DRAIN expression {} targets draining memory `{}`",
                expression.id, destination_memory.identity.semantic_path
            )));
        }
        if source.region.memory == destination.memory {
            return Err(SemanticMemoryError::new(format!(
                "self drain is not allowed for semantic memory `{}`",
                source_memory.identity.semantic_path
            )));
        }
        drains.push(DrainUse {
            drain: expression.id,
            source,
            destination: SemanticMemoryRegionV1 {
                memory: destination.memory,
                projection: Vec::new(),
            },
            initializer: destination.initializer,
        });
    }
    Ok(drains)
}

fn resolve_drain_binding<'a>(
    execution: &SemanticExecutionGraphV1,
    reactive: &'a SemanticReactiveGraphV1,
    expression: &SemanticExpression,
) -> Result<&'a crate::SemanticBindingV1, SemanticMemoryError> {
    let SemanticExpressionKind::Drain { target, .. } = &expression.kind else {
        return Err(SemanticMemoryError::new(format!(
            "expression {} is not DRAIN",
            expression.id
        )));
    };
    let origin = execution
        .checked_expression_origins
        .get(expression.id.as_usize())
        .filter(|origin| {
            origin.expression == expression.id
                && origin.checked_expression == expression.checked_expr_id
        })
        .ok_or_else(|| {
            SemanticMemoryError::new(format!(
                "DRAIN expression {} has no exact semantic origin",
                expression.id
            ))
        })?;
    let state_origins = expression
        .provenance
        .members
        .iter()
        .filter_map(|member| match member.origin {
            SemanticValueOrigin::State { state, .. } => Some(state),
            _ => None,
        })
        .collect::<BTreeSet<_>>();
    if state_origins.len() > 1 {
        return Err(SemanticMemoryError::new(format!(
            "DRAIN expression {} has {} state authority origins",
            expression.id,
            state_origins.len()
        )));
    }
    let matches = reactive
        .bindings
        .iter()
        .filter(|binding| {
            binding.owner == expression.owner
                && binding.call_instance == origin.call_instance
                && if let Some(state) = state_origins.iter().next() {
                    binding.target == SemanticBindingTargetV1::State { state: *state }
                } else {
                    binding.declaration == *target
                }
        })
        .collect::<Vec<_>>();
    let [binding] = matches.as_slice() else {
        return Err(SemanticMemoryError::new(format!(
            "DRAIN expression {} resolves declaration {} to {} exact owner/frame authority bindings",
            expression.id,
            target.0,
            matches.len()
        )));
    };
    Ok(*binding)
}

fn memory_for_storage_binding<'a>(
    memories: &'a [SemanticMemoryV1],
    storage: &crate::SemanticStorageBindingV1,
) -> Result<&'a SemanticMemoryV1, SemanticMemoryError> {
    let matches = memories
        .iter()
        .filter(|memory| memory.backing.binding() == Some(storage.binding))
        .filter(|memory| match (memory.backing, &storage.target) {
            (
                SemanticMemoryBackingV1::State {
                    storage_field,
                    state,
                    row,
                    ..
                },
                SemanticStorageBindingTargetV1::State {
                    state: target,
                    published: true,
                    field: Some(field),
                    row: target_row,
                },
            ) => state == *target && storage_field == *field && row == *target_row,
            (
                SemanticMemoryBackingV1::List {
                    storage_field,
                    list,
                    row,
                    ..
                },
                SemanticStorageBindingTargetV1::List {
                    list: target,
                    field,
                    row: target_row,
                },
            ) => list == *target && storage_field == *field && row == *target_row,
            _ => false,
        })
        .collect::<Vec<_>>();
    let [memory] = matches.as_slice() else {
        return Err(SemanticMemoryError::new(format!(
            "storage binding {} target {:?} resolves to {} exact durable memories",
            storage.binding,
            storage.target,
            matches.len()
        )));
    };
    Ok(*memory)
}

fn resolve_memory_region(
    memory: &SemanticMemoryV1,
    projection: &[String],
) -> Result<ResolvedMemoryRegion, SemanticMemoryError> {
    if memory.identity.kind == SemanticMemoryKindV1::ListOwner && !projection.is_empty() {
        return Err(SemanticMemoryError::new(format!(
            "whole-list memory `{}` cannot be drained through projection `{}`",
            memory.identity.semantic_path,
            projection.join(".")
        )));
    }
    let data_type = type_at_projection(&memory.data_type, projection).ok_or_else(|| {
        SemanticMemoryError::new(format!(
            "projection `{}` is outside semantic memory `{}`",
            projection.join("."),
            memory.identity.semantic_path
        ))
    })?;
    let leaf_indexes = memory
        .leaves
        .iter()
        .enumerate()
        .filter_map(|(index, leaf)| leaf.projection.starts_with(projection).then_some(index))
        .collect::<Vec<_>>();
    if leaf_indexes.is_empty() {
        return Err(SemanticMemoryError::new(format!(
            "semantic memory region `{}` has no authoritative leaves",
            memory_region_path(memory, projection)
        )));
    }
    Ok(ResolvedMemoryRegion {
        region: SemanticMemoryRegionV1 {
            memory: memory.id,
            projection: projection.to_vec(),
        },
        leaf_indexes,
        data_type: data_type.clone(),
    })
}

fn type_at_projection<'a>(data_type: &'a Type, projection: &[String]) -> Option<&'a Type> {
    let Some((field, rest)) = projection.split_first() else {
        return Some(data_type);
    };
    let Type::Object(shape) = data_type else {
        return None;
    };
    type_at_projection(shape.fields.get(field)?, rest)
}

fn memory_region_path(memory: &SemanticMemoryV1, projection: &[String]) -> String {
    if projection.is_empty() {
        memory.identity.semantic_path.clone()
    } else {
        format!("{}.{}", memory.identity.semantic_path, projection.join("."))
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct DestinationCandidate {
    memory: SemanticMemoryId,
    initializer: SemanticMigrationInitializerV1,
}

fn exact_drain_destination_candidates(
    execution: &SemanticExecutionGraphV1,
    resources: &SemanticResourceGraphV1,
    memories: &[SemanticMemoryV1],
    drain: SemanticExprId,
) -> Result<Vec<DestinationCandidate>, SemanticMemoryError> {
    let mut candidates = BTreeSet::new();
    for memory in memories {
        let SemanticMemoryBackingV1::State { state, .. } = memory.backing else {
            continue;
        };
        let state_resource = require_state(resources, state)?;
        if expression_reaches(execution, state_resource.initial, drain)? {
            candidates.insert(DestinationCandidate {
                memory: memory.id,
                initializer: SemanticMigrationInitializerV1::State {
                    state,
                    root: state_resource.initial,
                },
            });
        }
    }

    for materialization in &execution.materializations {
        if !expression_reaches(execution, materialization.source, drain)? {
            continue;
        }
        let Some(list) = materialization.target_list_id else {
            continue;
        };
        let scope = materialization.target_scope_id.ok_or_else(|| {
            SemanticMemoryError::new(format!(
                "materialization {} targets list {} without row scope",
                materialization.id, list
            ))
        })?;
        let target_row = SemanticRowBinding { list, scope };
        let resource_bindings = resources
            .materialization_bindings
            .iter()
            .filter(|binding| binding.materialization == materialization.id)
            .collect::<Vec<_>>();
        let [resource_binding] = resource_bindings.as_slice() else {
            return Err(SemanticMemoryError::new(format!(
                "materialization {} resolves to {} exact resource bindings",
                materialization.id,
                resource_bindings.len()
            )));
        };
        if resource_binding.target != Some(target_row) {
            return Err(SemanticMemoryError::new(format!(
                "materialization {} target {:?} differs from resource target {:?}",
                materialization.id, target_row, resource_binding.target
            )));
        }
        if materialization.operation != SemanticContextualOperationKind::Map {
            return Err(SemanticMemoryError::new(format!(
                "whole-list DRAIN destination materialization {} uses {:?}; V1 requires an exact map transfer",
                materialization.id, materialization.operation
            )));
        }
        if materialization.source != drain {
            return Err(SemanticMemoryError::new(format!(
                "whole-list DRAIN {} is wrapped by materialization {} source {}; V1 requires the DRAIN itself as authority source",
                drain, materialization.id, materialization.source
            )));
        }
        let memory_matches = memories
            .iter()
            .filter(|memory| {
                matches!(
                    memory.backing,
                    SemanticMemoryBackingV1::List {
                        list: candidate,
                        row,
                        ..
                    } if candidate == list && row == target_row
                )
            })
            .map(|memory| memory.id)
            .collect::<Vec<_>>();
        let [memory] = memory_matches.as_slice() else {
            return Err(SemanticMemoryError::new(format!(
                "materialization {} target list {} resolves to {} exact list memories",
                materialization.id,
                list,
                memory_matches.len()
            )));
        };
        candidates.insert(DestinationCandidate {
            memory: *memory,
            initializer: SemanticMigrationInitializerV1::ListMaterialization {
                list,
                materialization: materialization.id,
                source_root: materialization.source,
            },
        });
    }
    Ok(candidates.into_iter().collect())
}

fn require_memory(
    memories: &[SemanticMemoryV1],
    memory: SemanticMemoryId,
) -> Result<&SemanticMemoryV1, SemanticMemoryError> {
    memories
        .get(memory.as_usize())
        .filter(|candidate| candidate.id == memory)
        .ok_or_else(|| SemanticMemoryError::new(format!("missing semantic memory {memory}")))
}

fn validate_source_coverage(
    memories: &[SemanticMemoryV1],
    drains: &[DrainUse],
) -> Result<(), SemanticMemoryError> {
    let mut coverage = BTreeMap::<SemanticMemoryId, BTreeSet<usize>>::new();
    let mut regions = BTreeMap::<SemanticMemoryId, Vec<Vec<String>>>::new();
    let mut drain_expressions = BTreeSet::new();
    for drain in drains {
        if !drain_expressions.insert(drain.drain) {
            return Err(SemanticMemoryError::new(format!(
                "DRAIN expression {} is assigned more than once",
                drain.drain
            )));
        }
        let source = require_memory(memories, drain.source.region.memory)?;
        if !matches!(source.status, SemanticMemoryStatusV1::Draining { .. }) {
            return Err(SemanticMemoryError::new(format!(
                "DRAIN source `{}` is not marked DRAINING",
                memory_region_path(source, &drain.source.region.projection)
            )));
        }
        let prior_regions = regions.entry(source.id).or_default();
        for prior in prior_regions.iter() {
            if prior == &drain.source.region.projection {
                return Err(SemanticMemoryError::new(format!(
                    "semantic memory region `{}` is drained more than once",
                    memory_region_path(source, prior)
                )));
            }
            if projection_contains(prior, &drain.source.region.projection)
                || projection_contains(&drain.source.region.projection, prior)
            {
                return Err(SemanticMemoryError::new(format!(
                    "overlapping ancestor/descendant drains `{}` and `{}` are not allowed",
                    memory_region_path(source, prior),
                    memory_region_path(source, &drain.source.region.projection)
                )));
            }
        }
        prior_regions.push(drain.source.region.projection.clone());
        let covered = coverage.entry(source.id).or_default();
        for leaf in &drain.source.leaf_indexes {
            if !covered.insert(*leaf) {
                return Err(SemanticMemoryError::new(format!(
                    "semantic memory leaf `{}` is drained more than once",
                    memory_region_path(source, &source.leaves[*leaf].projection)
                )));
            }
        }
    }

    for memory in memories
        .iter()
        .filter(|memory| matches!(memory.status, SemanticMemoryStatusV1::Draining { .. }))
    {
        let covered = coverage.get(&memory.id);
        if covered.is_none_or(BTreeSet::is_empty) {
            return Err(SemanticMemoryError::new(format!(
                "semantic memory `{}` is marked DRAINING but has no DRAIN destination",
                memory.identity.semantic_path
            )));
        }
        let missing = memory
            .leaves
            .iter()
            .enumerate()
            .filter(|(index, _)| !covered.is_some_and(|covered| covered.contains(index)))
            .map(|(_, leaf)| memory_region_path(memory, &leaf.projection))
            .collect::<Vec<_>>();
        if !missing.is_empty() {
            return Err(SemanticMemoryError::new(format!(
                "DRAINING memory `{}` has partial coverage; missing authoritative leaves: {}",
                memory.identity.semantic_path,
                missing.join(", ")
            )));
        }
    }
    Ok(())
}

fn projection_contains(ancestor: &[String], descendant: &[String]) -> bool {
    ancestor.len() < descendant.len() && descendant.starts_with(ancestor)
}

fn validate_no_ordinary_draining_reads(
    execution: &SemanticExecutionGraphV1,
    reactive: &SemanticReactiveGraphV1,
    storage: &SemanticScopeStorageGraphV1,
    reachable: &BTreeSet<SemanticExprId>,
    memories: &[SemanticMemoryV1],
) -> Result<(), SemanticMemoryError> {
    let draining = memories
        .iter()
        .filter(|memory| matches!(memory.status, SemanticMemoryStatusV1::Draining { .. }))
        .map(|memory| memory.id)
        .collect::<BTreeSet<_>>();
    if draining.is_empty() {
        return Ok(());
    }

    let mut definition_members = BTreeMap::new();
    for memory_id in &draining {
        let memory = require_memory(memories, *memory_id)?;
        let binding_id = memory.backing.binding().ok_or_else(|| {
            SemanticMemoryError::new(format!(
                "collection memory `{}` cannot be DRAINING",
                memory.identity.semantic_path
            ))
        })?;
        let binding = reactive
            .bindings
            .get(binding_id.as_usize())
            .filter(|binding| binding.id == binding_id)
            .ok_or_else(|| {
                SemanticMemoryError::new(format!(
                    "semantic memory `{}` references missing reactive binding {}",
                    memory.identity.semantic_path, binding_id
                ))
            })?;
        definition_members.insert(*memory_id, expression_subtree(execution, binding.producer)?);
    }

    for read in reactive
        .reads
        .iter()
        .filter(|read| reachable.contains(&read.expression))
    {
        let (binding, projection) = match &read.target {
            SemanticReadTargetV1::Binding {
                binding,
                projection,
            }
            | SemanticReadTargetV1::StateProjection {
                binding,
                projection,
                ..
            } => (*binding, projection.as_slice()),
            SemanticReadTargetV1::SourcePayload { .. }
            | SemanticReadTargetV1::Local { .. }
            | SemanticReadTargetV1::External { .. }
            | SemanticReadTargetV1::ElementState { .. }
            | SemanticReadTargetV1::MaterializationLocal { .. }
            | SemanticReadTargetV1::FunctionParameter { .. } => continue,
        };
        let storage_binding = exact_storage_binding(storage, binding)?;
        let candidates = memories
            .iter()
            .filter(|memory| memory.backing.binding() == Some(storage_binding.binding))
            .map(|memory| memory.id)
            .collect::<Vec<_>>();
        for memory_id in candidates {
            if !draining.contains(&memory_id)
                || definition_members
                    .get(&memory_id)
                    .is_some_and(|members| members.contains(&read.expression))
            {
                continue;
            }
            let memory = require_memory(memories, memory_id)?;
            return Err(SemanticMemoryError::new(format!(
                "ordinary read {} references DRAINING memory `{}`; use DRAIN at its migration destination",
                read.expression,
                memory_region_path(memory, projection)
            )));
        }
    }
    Ok(())
}

fn lower_migration_edges(
    execution: &SemanticExecutionGraphV1,
    resources: &SemanticResourceGraphV1,
    memories: &[SemanticMemoryV1],
    drains: &[DrainUse],
) -> Result<Vec<SemanticMigrationEdgeV1>, SemanticMemoryError> {
    let mut initializer_by_destination =
        BTreeMap::<SemanticMemoryId, SemanticMigrationInitializerV1>::new();
    let mut groups =
        BTreeMap::<(SemanticMemoryId, SemanticMigrationInitializerV1), Vec<&DrainUse>>::new();
    for drain in drains {
        if let Some(previous) =
            initializer_by_destination.insert(drain.destination.memory, drain.initializer)
            && previous != drain.initializer
        {
            let memory = require_memory(memories, drain.destination.memory)?;
            return Err(SemanticMemoryError::new(format!(
                "migration destination `{}` has conflicting initializers {previous:?} and {:?}",
                memory.identity.semantic_path, drain.initializer
            )));
        }
        groups
            .entry((drain.destination.memory, drain.initializer))
            .or_default()
            .push(drain);
    }

    let mut edges = Vec::with_capacity(groups.len());
    for ((destination_id, initializer), uses) in groups {
        let destination = require_memory(memories, destination_id)?;
        let mut inputs = uses
            .iter()
            .map(|drain| SemanticMigrationDrainV1 {
                expression: drain.drain,
                source: drain.source.region.clone(),
            })
            .collect::<Vec<_>>();
        inputs.sort();

        let (transfer, transform) = match destination.identity.kind {
            SemanticMemoryKindV1::RootScalar => {
                for input in &inputs {
                    let source = require_memory(memories, input.source.memory)?;
                    if source.identity.kind != SemanticMemoryKindV1::RootScalar {
                        return Err(SemanticMemoryError::new(format!(
                            "scalar migration destination `{}` consumes non-scalar memory `{}`",
                            destination.identity.semantic_path, source.identity.semantic_path
                        )));
                    }
                }
                (
                    SemanticMigrationTransferV1::Scalar,
                    state_transform(execution, destination, initializer, &inputs)?,
                )
            }
            SemanticMemoryKindV1::IndexedField => {
                let owner = destination.backing.row().ok_or_else(|| {
                    SemanticMemoryError::new(format!(
                        "indexed memory `{}` has no exact row owner",
                        destination.identity.semantic_path
                    ))
                })?;
                for input in &inputs {
                    let source = require_memory(memories, input.source.memory)?;
                    if source.identity.kind != SemanticMemoryKindV1::IndexedField
                        || source.backing.row() != Some(owner)
                    {
                        return Err(SemanticMemoryError::new(format!(
                            "indexed migration `{}` -> `{}` crosses stable row owner {:?}",
                            source.identity.semantic_path,
                            destination.identity.semantic_path,
                            owner
                        )));
                    }
                }
                (
                    SemanticMigrationTransferV1::IndexedField { owner },
                    state_transform(execution, destination, initializer, &inputs)?,
                )
            }
            SemanticMemoryKindV1::ListOwner => {
                let [input] = inputs.as_slice() else {
                    return Err(SemanticMemoryError::new(format!(
                        "whole-list migration destination `{}` requires one DRAIN input, found {}",
                        destination.identity.semantic_path,
                        inputs.len()
                    )));
                };
                if !input.source.projection.is_empty() {
                    return Err(SemanticMemoryError::new(
                        "whole-list migration cannot consume a projected source",
                    ));
                }
                let source = require_memory(memories, input.source.memory)?;
                let (
                    SemanticMemoryBackingV1::List {
                        list: source_list,
                        row: source_row,
                        ..
                    },
                    SemanticMemoryBackingV1::List {
                        list: destination_list,
                        row: destination_row,
                        ..
                    },
                ) = (source.backing, destination.backing)
                else {
                    return Err(SemanticMemoryError::new(format!(
                        "whole-list migration `{}` -> `{}` does not join two list backings",
                        source.identity.semantic_path, destination.identity.semantic_path
                    )));
                };
                if source.id == destination.id || source_list == destination_list {
                    return Err(SemanticMemoryError::new(
                        "whole-list migration must change list authority identity",
                    ));
                }
                let SemanticMigrationInitializerV1::ListMaterialization {
                    list, source_root, ..
                } = initializer
                else {
                    return Err(SemanticMemoryError::new(format!(
                        "list destination `{}` has non-list initializer {initializer:?}",
                        destination.identity.semantic_path
                    )));
                };
                if list != destination_list || source_root != input.expression {
                    return Err(SemanticMemoryError::new(format!(
                        "list destination `{}` initializer does not exactly consume DRAIN {}",
                        destination.identity.semantic_path, input.expression
                    )));
                }
                validate_list_owner_change(
                    resources,
                    memories,
                    source,
                    destination,
                    source_list,
                    destination_list,
                )?;
                (
                    SemanticMigrationTransferV1::List {
                        source: source_row,
                        destination: destination_row,
                    },
                    SemanticMigrationTransformV1::Identity {
                        input: input.expression,
                    },
                )
            }
            SemanticMemoryKindV1::Map | SemanticMemoryKindV1::Set => {
                return Err(SemanticMemoryError::new(format!(
                    "collection memory `{}` cannot be a DRAIN migration destination",
                    destination.identity.semantic_path
                )));
            }
        };

        edges.push(SemanticMigrationEdgeV1 {
            id: SemanticMigrationEdgeId(edges.len()),
            inputs,
            destination: SemanticMemoryRegionV1 {
                memory: destination.id,
                projection: Vec::new(),
            },
            initializer,
            transfer,
            transform,
        });
    }
    Ok(edges)
}

fn state_transform(
    execution: &SemanticExecutionGraphV1,
    destination: &SemanticMemoryV1,
    initializer: SemanticMigrationInitializerV1,
    inputs: &[SemanticMigrationDrainV1],
) -> Result<SemanticMigrationTransformV1, SemanticMemoryError> {
    let SemanticMigrationInitializerV1::State { state, root } = initializer else {
        return Err(SemanticMemoryError::new(format!(
            "state destination `{}` has non-state initializer {initializer:?}",
            destination.identity.semantic_path
        )));
    };
    let SemanticMemoryBackingV1::State {
        state: destination_state,
        ..
    } = destination.backing
    else {
        return Err(SemanticMemoryError::new(format!(
            "state initializer {state} targets non-state memory `{}`",
            destination.identity.semantic_path
        )));
    };
    if state != destination_state {
        return Err(SemanticMemoryError::new(format!(
            "state destination `{}` initializer state {} differs from backing state {}",
            destination.identity.semantic_path, state, destination_state
        )));
    }
    let root_expression = require_expression(execution, root)?;
    ensure_closed_memory_type(
        &root_expression.flow_type.ty,
        &format!(
            "migration initializer for `{}`",
            destination.identity.semantic_path
        ),
    )?;
    if !migration_type_assignable(&root_expression.flow_type.ty, &destination.data_type) {
        return Err(SemanticMemoryError::new(format!(
            "migration initializer {} type {:?} is not assignable to destination `{}` type {:?}",
            root,
            root_expression.flow_type.ty,
            destination.identity.semantic_path,
            destination.data_type
        )));
    }
    if inputs.len() == 1 && inputs[0].expression == root {
        return Ok(SemanticMigrationTransformV1::Identity { input: root });
    }
    let allowed = inputs
        .iter()
        .map(|input| input.expression)
        .collect::<BTreeSet<_>>();
    validate_pure_transform(execution, root, &allowed)?;
    Ok(SemanticMigrationTransformV1::PureExpression { root })
}

fn migration_type_assignable(source: &Type, target: &Type) -> bool {
    if type_uses_boolean_runtime_representation(source)
        || type_uses_boolean_runtime_representation(target)
    {
        return source == target;
    }
    match (source, target) {
        (Type::VariantSet(source), Type::VariantSet(target)) => source.iter().all(|source| {
            target
                .iter()
                .any(|target| migration_variant_assignable(source, target))
        }),
        (Type::Object(source), Type::Object(target)) => {
            (!source.open || target.open)
                && target.fields.iter().all(|(name, target)| {
                    source
                        .fields
                        .get(name)
                        .is_some_and(|source| migration_type_assignable(source, target))
                })
                && (target.open || source.fields.len() == target.fields.len())
        }
        (Type::List(source), Type::List(target)) => migration_type_assignable(source, target),
        _ => source == target,
    }
}

fn type_uses_boolean_runtime_representation(data_type: &Type) -> bool {
    match data_type {
        Type::VariantSet(variants) => {
            boon_typecheck::variants_use_boolean_runtime_representation(variants)
        }
        _ => false,
    }
}

fn migration_variant_assignable(source: &Variant, target: &Variant) -> bool {
    match (source, target) {
        (Variant::Tag(source), Variant::Tag(target)) => source == target,
        (
            Variant::Tagged {
                tag: source_tag,
                fields: source,
            },
            Variant::Tagged {
                tag: target_tag,
                fields: target,
            },
        ) => {
            source_tag == target_tag
                && source.open == target.open
                && source.fields.len() == target.fields.len()
                && source.fields.iter().all(|(name, source)| {
                    target
                        .fields
                        .get(name)
                        .is_some_and(|target| migration_type_assignable(source, target))
                })
        }
        _ => false,
    }
}

fn validate_list_owner_change(
    resources: &SemanticResourceGraphV1,
    memories: &[SemanticMemoryV1],
    source: &SemanticMemoryV1,
    destination: &SemanticMemoryV1,
    source_list: SemanticListId,
    destination_list: SemanticListId,
) -> Result<(), SemanticMemoryError> {
    if !migration_type_preserves_source_shape(&source.data_type, &destination.data_type) {
        return Err(SemanticMemoryError::new(format!(
            "whole-list migration `{}` -> `{}` changes authoritative source row shape",
            source.identity.semantic_path, destination.identity.semantic_path
        )));
    }
    let source_resource = require_list(resources, source_list)?;
    let destination_resource = require_list(resources, destination_list)?;
    if source_resource.key_policy != destination_resource.key_policy {
        return Err(SemanticMemoryError::new(format!(
            "whole-list migration `{}` -> `{}` changes hidden-key policy",
            source.identity.semantic_path, destination.identity.semantic_path
        )));
    }
    let source_indexed = indexed_memory_schema(memories, source_list, source)?;
    let destination_indexed = indexed_memory_schema(memories, destination_list, destination)?;
    if source_indexed != destination_indexed {
        return Err(SemanticMemoryError::new(format!(
            "whole-list migration `{}` -> `{}` also changes indexed row authority",
            source.identity.semantic_path, destination.identity.semantic_path
        )));
    }
    Ok(())
}

fn migration_type_preserves_source_shape(source: &Type, destination: &Type) -> bool {
    match (source, destination) {
        (Type::Object(source), Type::Object(destination)) => {
            source.fields.iter().all(|(name, source)| {
                destination.fields.get(name).is_some_and(|destination| {
                    migration_type_preserves_source_shape(source, destination)
                })
            })
        }
        (Type::VariantSet(source), Type::VariantSet(destination)) => source.iter().all(|source| {
            destination
                .iter()
                .any(|destination| migration_variant_preserves_source_shape(source, destination))
        }),
        (Type::List(source), Type::List(destination)) => {
            migration_type_preserves_source_shape(source, destination)
        }
        _ => source == destination,
    }
}

fn migration_variant_preserves_source_shape(source: &Variant, destination: &Variant) -> bool {
    match (source, destination) {
        (Variant::Tag(source), Variant::Tag(destination)) => source == destination,
        (
            Variant::Tagged {
                tag: source_tag,
                fields: source,
            },
            Variant::Tagged {
                tag: destination_tag,
                fields: destination,
            },
        ) => {
            source_tag == destination_tag
                && source.fields.iter().all(|(name, source)| {
                    destination.fields.get(name).is_some_and(|destination| {
                        migration_type_preserves_source_shape(source, destination)
                    })
                })
        }
        _ => false,
    }
}

fn indexed_memory_schema(
    memories: &[SemanticMemoryV1],
    list: SemanticListId,
    owner: &SemanticMemoryV1,
) -> Result<BTreeMap<String, Type>, SemanticMemoryError> {
    let mut schema = BTreeMap::new();
    for memory in memories.iter().filter(|memory| {
        memory.identity.kind == SemanticMemoryKindV1::IndexedField
            && matches!(
                memory.backing,
                SemanticMemoryBackingV1::State {
                    row: Some(row), ..
                } if row.list == list
            )
    }) {
        let prefix = format!("{}.", owner.identity.semantic_path);
        let relative = memory
            .identity
            .semantic_path
            .strip_prefix(&prefix)
            .filter(|relative| !relative.is_empty())
            .ok_or_else(|| {
                SemanticMemoryError::new(format!(
                    "indexed memory `{}` is not structurally owned by list `{}`",
                    memory.identity.semantic_path, owner.identity.semantic_path
                ))
            })?
            .to_owned();
        if schema
            .insert(relative.clone(), memory.data_type.clone())
            .is_some()
        {
            return Err(SemanticMemoryError::new(format!(
                "list `{}` has duplicate indexed memory path `{relative}`",
                owner.identity.semantic_path
            )));
        }
    }
    Ok(schema)
}

fn validate_pure_transform(
    execution: &SemanticExecutionGraphV1,
    root: SemanticExprId,
    allowed_drains: &BTreeSet<SemanticExprId>,
) -> Result<(), SemanticMemoryError> {
    let mut checker = SemanticMigrationPurityChecker {
        execution,
        allowed_drains,
        seen_drains: BTreeSet::new(),
        active_expressions: BTreeSet::new(),
        lexical_bindings: Vec::new(),
        active_bindings: BTreeSet::new(),
    };
    checker.check(root)?;
    if checker.seen_drains != *allowed_drains {
        let missing = allowed_drains
            .difference(&checker.seen_drains)
            .copied()
            .collect::<Vec<_>>();
        let extra = checker
            .seen_drains
            .difference(allowed_drains)
            .copied()
            .collect::<Vec<_>>();
        return Err(SemanticMemoryError::new(format!(
            "migration transform root {root} DRAIN set differs from edge inputs; missing={missing:?}, extra={extra:?}"
        )));
    }
    Ok(())
}

struct SemanticMigrationPurityChecker<'a> {
    execution: &'a SemanticExecutionGraphV1,
    allowed_drains: &'a BTreeSet<SemanticExprId>,
    seen_drains: BTreeSet<SemanticExprId>,
    active_expressions: BTreeSet<SemanticExprId>,
    lexical_bindings:
        Vec<BTreeMap<crate::SemanticLocalBindingId, (boon_typecheck::DeclId, SemanticExprId)>>,
    active_bindings: BTreeSet<crate::SemanticLocalBindingId>,
}

impl SemanticMigrationPurityChecker<'_> {
    fn check(&mut self, expression_id: SemanticExprId) -> Result<(), SemanticMemoryError> {
        if !self.active_expressions.insert(expression_id) {
            return Err(SemanticMemoryError::new(format!(
                "migration transform expression graph cycles at {expression_id}"
            )));
        }
        let result = self.check_inner(expression_id);
        self.active_expressions.remove(&expression_id);
        result
    }

    fn check_inner(&mut self, expression_id: SemanticExprId) -> Result<(), SemanticMemoryError> {
        let expression = require_expression(self.execution, expression_id)?;
        if expression.effect.reads_state
            || expression.effect.writes_state
            || expression.effect.emits_source
            || expression.effect.invokes_host
        {
            return Err(SemanticMemoryError::new(format!(
                "migration transform expression {expression_id} contains a state, source, or host effect"
            )));
        }
        match &expression.kind {
            SemanticExpressionKind::Drain { .. }
                if self.allowed_drains.contains(&expression_id) =>
            {
                self.seen_drains.insert(expression_id);
                Ok(())
            }
            SemanticExpressionKind::Drain { .. } => Err(SemanticMemoryError::new(format!(
                "migration transform expression {expression_id} consumes DRAIN owned by another destination"
            ))),
            SemanticExpressionKind::Absent => Err(SemanticMemoryError::new(format!(
                "private absence at semantic expression {expression_id} cannot be migration data"
            ))),
            SemanticExpressionKind::Flush { .. } | SemanticExpressionKind::FlushBoundary { .. } => {
                Err(SemanticMemoryError::new(format!(
                    "live FLUSH control at semantic expression {expression_id} cannot be migration data"
                )))
            }
            SemanticExpressionKind::Text(_)
            | SemanticExpressionKind::Number(_)
            | SemanticExpressionKind::BytesByte(_)
            | SemanticExpressionKind::Tag(_) => Ok(()),
            SemanticExpressionKind::TextTemplate { segments } => {
                for segment in segments {
                    if let crate::SemanticTextSegment::Dynamic { value } = segment {
                        self.check(*value)?;
                    }
                }
                Ok(())
            }
            SemanticExpressionKind::TaggedObject { fields, .. }
            | SemanticExpressionKind::Object(fields) => {
                if fields.iter().any(|field| field.spread) {
                    return Err(SemanticMemoryError::new(format!(
                        "migration transform expression {expression_id} uses record spread"
                    )));
                }
                for field in fields {
                    self.check(field.value)?;
                }
                Ok(())
            }
            SemanticExpressionKind::List { items, .. }
            | SemanticExpressionKind::Bytes { items, .. }
            | SemanticExpressionKind::Map { entries: items }
            | SemanticExpressionKind::Set { items } => {
                for item in items {
                    self.check(*item)?;
                }
                Ok(())
            }
            SemanticExpressionKind::MapEntry { key, value } => {
                self.check(*key)?;
                self.check(*value)
            }
            SemanticExpressionKind::Infix { left, right, .. } => {
                self.check(*left)?;
                self.check(*right)
            }
            SemanticExpressionKind::Project { input, .. } => self.check(*input),
            SemanticExpressionKind::When {
                select_kind: SemanticSelectKind::When,
                input,
                arms,
            } => {
                self.check(*input)?;
                for arm in arms {
                    self.check(arm.output)?;
                }
                Ok(())
            }
            SemanticExpressionKind::When {
                select_kind: SemanticSelectKind::While,
                ..
            } => Err(SemanticMemoryError::new(format!(
                "migration transform expression {expression_id} uses WHILE"
            ))),
            SemanticExpressionKind::Call {
                callable_kind: crate::SemanticCallableKind::Builtin,
                function,
                arguments,
                contexts,
                ..
            } => {
                if !contexts.is_empty() {
                    return Err(SemanticMemoryError::new(format!(
                        "migration transform call {expression_id} reads call-local context"
                    )));
                }
                if !semantic_migration_call_is_supported_v1(function) {
                    return Err(SemanticMemoryError::new(format!(
                        "pure migration call `{function}` is outside the target-neutral migration contract"
                    )));
                }
                for argument in arguments {
                    self.check(argument.value)?;
                }
                Ok(())
            }
            SemanticExpressionKind::Call { name, .. } => Err(SemanticMemoryError::new(format!(
                "migration transform expression {expression_id} invokes non-builtin `{name}`"
            ))),
            SemanticExpressionKind::Block { bindings, result } => {
                let scope = bindings
                    .iter()
                    .map(|binding| (binding.id, (binding.declaration, binding.value)))
                    .collect::<BTreeMap<_, _>>();
                if scope.len() != bindings.len() {
                    return Err(SemanticMemoryError::new(format!(
                        "migration block {expression_id} contains duplicate lexical bindings"
                    )));
                }
                self.lexical_bindings.push(scope);
                for binding in bindings {
                    self.check(binding.value)?;
                }
                let result = self.check(*result);
                self.lexical_bindings.pop();
                result
            }
            SemanticExpressionKind::LocalRead {
                binding,
                declaration,
                ..
            } => {
                let definition = self
                    .lexical_bindings
                    .iter()
                    .rev()
                    .find_map(|scope| scope.get(binding).copied())
                    .ok_or_else(|| {
                        SemanticMemoryError::new(format!(
                            "migration transform expression {expression_id} reads unbound local {binding}"
                        ))
                    })?;
                if definition.0 != *declaration {
                    return Err(SemanticMemoryError::new(format!(
                        "migration local read {expression_id} declaration {} differs from binding declaration {}",
                        declaration.0, definition.0.0
                    )));
                }
                if !self.active_bindings.insert(*binding) {
                    return Err(SemanticMemoryError::new(format!(
                        "migration lexical binding {binding} forms a value cycle"
                    )));
                }
                let result = self.check(definition.1);
                self.active_bindings.remove(binding);
                result
            }
            SemanticExpressionKind::CanonicalRead { path, .. } => {
                Err(SemanticMemoryError::new(format!(
                    "migration transform expression {expression_id} reads `{path}` outside DRAIN inputs"
                )))
            }
            SemanticExpressionKind::ExternalRead { canonical_path, .. } => {
                Err(SemanticMemoryError::new(format!(
                    "migration transform expression {expression_id} reads external value `{canonical_path}`"
                )))
            }
            SemanticExpressionKind::ElementState { .. }
            | SemanticExpressionKind::Source { .. }
            | SemanticExpressionKind::Materialize { .. }
            | SemanticExpressionKind::MaterializationLocal { .. }
            | SemanticExpressionKind::FunctionParameter { .. }
            | SemanticExpressionKind::Draining { .. }
            | SemanticExpressionKind::Hold { .. }
            | SemanticExpressionKind::Latest { .. }
            | SemanticExpressionKind::Then { .. }
            | SemanticExpressionKind::MatchArm { .. }
            | SemanticExpressionKind::Delimiter => Err(SemanticMemoryError::new(format!(
                "semantic expression {expression_id} is not legal in a target-neutral migration transform"
            ))),
        }
    }
}

/// The target-neutral V1 call surface accepted by semantic migration.
///
/// Backends must consume this authority rather than maintain a second
/// independently evolving whitelist.
pub fn semantic_migration_call_is_supported_v1(function: &str) -> bool {
    matches!(
        function,
        "Text/empty"
            | "Text/space"
            | "Text/trim"
            | "Text/to_uppercase"
            | "Text/concat"
            | "Text/slice"
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

fn validate_migration_cycles(
    memories: &[SemanticMemoryV1],
    edges: &[SemanticMigrationEdgeV1],
) -> Result<(), SemanticMemoryError> {
    let mut dependencies = BTreeMap::<SemanticMemoryId, BTreeSet<SemanticMemoryId>>::new();
    for edge in edges {
        require_memory(memories, edge.destination.memory)?;
        for input in &edge.inputs {
            require_memory(memories, input.source.memory)?;
            dependencies
                .entry(input.source.memory)
                .or_default()
                .insert(edge.destination.memory);
        }
    }

    let mut complete = BTreeSet::new();
    let mut active = Vec::new();
    for memory in memories {
        if let Some(cycle) =
            migration_cycle_from(memory.id, &dependencies, &mut complete, &mut active)
        {
            let paths = cycle
                .iter()
                .map(|memory| {
                    require_memory(memories, *memory)
                        .map(|memory| memory.identity.semantic_path.as_str())
                })
                .collect::<Result<Vec<_>, _>>()?;
            return Err(SemanticMemoryError::new(format!(
                "semantic migration graph cycle is not allowed: {}",
                paths.join(" -> ")
            )));
        }
    }
    Ok(())
}

fn migration_cycle_from(
    memory: SemanticMemoryId,
    dependencies: &BTreeMap<SemanticMemoryId, BTreeSet<SemanticMemoryId>>,
    complete: &mut BTreeSet<SemanticMemoryId>,
    active: &mut Vec<SemanticMemoryId>,
) -> Option<Vec<SemanticMemoryId>> {
    if let Some(start) = active.iter().position(|candidate| *candidate == memory) {
        let mut cycle = active[start..].to_vec();
        cycle.push(memory);
        return Some(cycle);
    }
    if complete.contains(&memory) {
        return None;
    }

    active.push(memory);
    for dependency in dependencies.get(&memory).into_iter().flatten().copied() {
        if let Some(cycle) = migration_cycle_from(dependency, dependencies, complete, active) {
            return Some(cycle);
        }
    }
    active.pop();
    complete.insert(memory);
    None
}

fn validate_memory_shape(
    graph: &SemanticMemoryGraphV1,
    execution: &SemanticExecutionGraphV1,
    resources: &SemanticResourceGraphV1,
    reactive: &SemanticReactiveGraphV1,
    storage: &SemanticScopeStorageGraphV1,
) -> Result<(), SemanticMemoryError> {
    if graph.schema != SEMANTIC_MEMORY_GRAPH_SCHEMA_V1 {
        return Err(SemanticMemoryError::new(format!(
            "semantic memory graph schema `{}` is not `{SEMANTIC_MEMORY_GRAPH_SCHEMA_V1}`",
            graph.schema
        )));
    }

    let mut identities = BTreeSet::new();
    let mut draining_markers = BTreeSet::new();
    for (index, memory) in graph.memories.iter().enumerate() {
        if memory.id != SemanticMemoryId(index) {
            return Err(SemanticMemoryError::new(format!(
                "semantic memory ID {} is not dense index {index}",
                memory.id
            )));
        }
        if !identities.insert(memory.identity.clone()) {
            return Err(SemanticMemoryError::new(format!(
                "duplicate semantic memory identity `{}`",
                memory.identity.semantic_path
            )));
        }
        ensure_closed_memory_type(
            &memory.data_type,
            &format!("semantic memory `{}`", memory.identity.semantic_path),
        )?;
        let expected_leaves = match memory.identity.kind {
            SemanticMemoryKindV1::ListOwner
            | SemanticMemoryKindV1::Map
            | SemanticMemoryKindV1::Set => vec![SemanticMemoryLeafV1 {
                projection: Vec::new(),
                data_type: memory.data_type.clone(),
            }],
            SemanticMemoryKindV1::RootScalar | SemanticMemoryKindV1::IndexedField => {
                semantic_memory_leaves(&memory.data_type)
            }
        };
        if memory.leaves != expected_leaves {
            return Err(SemanticMemoryError::new(format!(
                "semantic memory `{}` leaves are not the deterministic closed-type partition",
                memory.identity.semantic_path
            )));
        }

        match memory.backing {
            SemanticMemoryBackingV1::State {
                binding,
                storage_field,
                state,
                row,
            } => {
                let field = require_storage_field(storage, storage_field)?;
                if field.flow_type.ty != memory.data_type {
                    return Err(SemanticMemoryError::new(format!(
                        "semantic memory `{}` type differs from storage field {storage_field}",
                        memory.identity.semantic_path
                    )));
                }
                let reactive_binding = reactive
                    .bindings
                    .get(binding.as_usize())
                    .filter(|candidate| candidate.id == binding)
                    .ok_or_else(|| {
                        SemanticMemoryError::new(format!(
                            "semantic memory `{}` references missing reactive binding {binding}",
                            memory.identity.semantic_path
                        ))
                    })?;
                let storage_binding = exact_storage_binding(storage, binding)?;
                let resource = require_state(resources, state)?;
                if !resource.published
                    || resource.flow_type.ty != memory.data_type
                    || resource
                        .target_list
                        .zip(resource.row_scope)
                        .map(|(list, scope)| SemanticRowBinding { list, scope })
                        != row
                    || reactive_binding.target != (SemanticBindingTargetV1::State { state })
                    || storage_binding.target
                        != (SemanticStorageBindingTargetV1::State {
                            state,
                            published: true,
                            field: Some(storage_field),
                            row,
                        })
                {
                    return Err(SemanticMemoryError::new(format!(
                        "semantic state memory `{}` does not exactly join resource, reactive, and storage authority",
                        memory.identity.semantic_path
                    )));
                }
                let expected_kind = if row.is_some() {
                    SemanticMemoryKindV1::IndexedField
                } else {
                    SemanticMemoryKindV1::RootScalar
                };
                if memory.identity.kind != expected_kind {
                    return Err(SemanticMemoryError::new(format!(
                        "semantic state memory `{}` has kind {:?}, expected {expected_kind:?}",
                        memory.identity.semantic_path, memory.identity.kind
                    )));
                }
            }
            SemanticMemoryBackingV1::List {
                binding,
                storage_field,
                list,
                row,
            } => {
                let field = require_storage_field(storage, storage_field)?;
                if field.flow_type.ty != memory.data_type {
                    return Err(SemanticMemoryError::new(format!(
                        "semantic memory `{}` type differs from storage field {storage_field}",
                        memory.identity.semantic_path
                    )));
                }
                let reactive_binding = reactive
                    .bindings
                    .get(binding.as_usize())
                    .filter(|candidate| candidate.id == binding)
                    .ok_or_else(|| {
                        SemanticMemoryError::new(format!(
                            "semantic memory `{}` references missing reactive binding {binding}",
                            memory.identity.semantic_path
                        ))
                    })?;
                let storage_binding = exact_storage_binding(storage, binding)?;
                let resource = require_list(resources, list)?;
                if memory.identity.kind != SemanticMemoryKindV1::ListOwner
                    || memory.data_type != Type::List(Box::new(resource.item_type.clone()))
                    || row
                        != (SemanticRowBinding {
                            list,
                            scope: resource.row_scope,
                        })
                    || reactive_binding.target != (SemanticBindingTargetV1::List { list })
                    || storage_binding.target
                        != (SemanticStorageBindingTargetV1::List {
                            list,
                            field: storage_field,
                            row,
                        })
                {
                    return Err(SemanticMemoryError::new(format!(
                        "semantic list memory `{}` does not exactly join resource, reactive, and storage authority",
                        memory.identity.semantic_path
                    )));
                }
            }
            SemanticMemoryBackingV1::Collection { expression, owner } => {
                let expression = require_expression(execution, expression)?;
                let kind_matches = matches!(
                    (memory.identity.kind, &expression.kind, &memory.data_type),
                    (
                        SemanticMemoryKindV1::Map,
                        SemanticExpressionKind::Map { .. },
                        Type::Map { .. }
                    ) | (
                        SemanticMemoryKindV1::Set,
                        SemanticExpressionKind::Set { .. },
                        Type::Set(_)
                    )
                );
                if !kind_matches
                    || expression.owner != owner
                    || expression.flow_type.ty != memory.data_type
                    || !matches!(memory.status, SemanticMemoryStatusV1::Active)
                {
                    return Err(SemanticMemoryError::new(format!(
                        "semantic collection memory `{}` does not exactly join its executable authority, owner, type, and active status",
                        memory.identity.semantic_path
                    )));
                }
            }
        }

        if let SemanticMemoryStatusV1::Draining { marker } = memory.status {
            if !draining_markers.insert(marker) {
                return Err(SemanticMemoryError::new(format!(
                    "semantic DRAINING marker {marker} owns more than one durable memory"
                )));
            }
            let reactive_marker = reactive
                .migration_inputs
                .get(marker.as_usize())
                .filter(|candidate| candidate.id == marker)
                .ok_or_else(|| {
                    SemanticMemoryError::new(format!(
                        "semantic memory `{}` references missing DRAINING marker {marker}",
                        memory.identity.semantic_path
                    ))
                })?;
            require_expression(execution, reactive_marker.marker)?;
        }
    }

    let mut drain_ownership = BTreeSet::new();
    for (index, edge) in graph.migration_edges.iter().enumerate() {
        if edge.id != SemanticMigrationEdgeId(index) {
            return Err(SemanticMemoryError::new(format!(
                "semantic migration edge ID {} is not dense index {index}",
                edge.id
            )));
        }
        if edge.inputs.is_empty() {
            return Err(SemanticMemoryError::new(format!(
                "semantic migration edge {} has no DRAIN inputs",
                edge.id
            )));
        }
        if !edge.destination.projection.is_empty() {
            return Err(SemanticMemoryError::new(format!(
                "semantic migration edge {} targets a projected destination",
                edge.id
            )));
        }
        let destination = require_memory(&graph.memories, edge.destination.memory)?;
        if !matches!(destination.status, SemanticMemoryStatusV1::Active) {
            return Err(SemanticMemoryError::new(format!(
                "semantic migration edge {} targets DRAINING memory `{}`",
                edge.id, destination.identity.semantic_path
            )));
        }

        for input in &edge.inputs {
            if !drain_ownership.insert(input.expression) {
                return Err(SemanticMemoryError::new(format!(
                    "DRAIN expression {} belongs to more than one migration edge",
                    input.expression
                )));
            }
            let expression = require_expression(execution, input.expression)?;
            if !matches!(expression.kind, SemanticExpressionKind::Drain { .. }) {
                return Err(SemanticMemoryError::new(format!(
                    "semantic migration edge {} input {} is not DRAIN",
                    edge.id, input.expression
                )));
            }
            let source = require_memory(&graph.memories, input.source.memory)?;
            if !matches!(source.status, SemanticMemoryStatusV1::Draining { .. }) {
                return Err(SemanticMemoryError::new(format!(
                    "semantic migration edge {} consumes active memory `{}`",
                    edge.id, source.identity.semantic_path
                )));
            }
            resolve_memory_region(source, &input.source.projection)?;
        }

        match edge.initializer {
            SemanticMigrationInitializerV1::State { state, root } => {
                let SemanticMemoryBackingV1::State {
                    state: destination_state,
                    ..
                } = destination.backing
                else {
                    return Err(SemanticMemoryError::new(format!(
                        "semantic migration edge {} has a state initializer for non-state destination",
                        edge.id
                    )));
                };
                let state_resource = require_state(resources, state)?;
                if state != destination_state || state_resource.initial != root {
                    return Err(SemanticMemoryError::new(format!(
                        "semantic migration edge {} state initializer is not the exact destination initializer",
                        edge.id
                    )));
                }
                require_expression(execution, root)?;
            }
            SemanticMigrationInitializerV1::ListMaterialization {
                list,
                materialization,
                source_root,
            } => {
                let SemanticMemoryBackingV1::List {
                    list: destination_list,
                    ..
                } = destination.backing
                else {
                    return Err(SemanticMemoryError::new(format!(
                        "semantic migration edge {} has a list initializer for non-list destination",
                        edge.id
                    )));
                };
                let materialization = execution
                    .materializations
                    .get(materialization.as_usize())
                    .filter(|candidate| candidate.id == materialization)
                    .ok_or_else(|| {
                        SemanticMemoryError::new(format!(
                            "semantic migration edge {} references missing materialization {materialization}",
                            edge.id
                        ))
                    })?;
                if list != destination_list
                    || materialization.target_list_id != Some(list)
                    || materialization.source != source_root
                {
                    return Err(SemanticMemoryError::new(format!(
                        "semantic migration edge {} list initializer is not the exact destination materialization",
                        edge.id
                    )));
                }
            }
        }

        match edge.transform {
            SemanticMigrationTransformV1::Identity { input } => {
                if !edge
                    .inputs
                    .iter()
                    .any(|candidate| candidate.expression == input)
                {
                    return Err(SemanticMemoryError::new(format!(
                        "semantic migration edge {} identity transform references non-input DRAIN {input}",
                        edge.id
                    )));
                }
            }
            SemanticMigrationTransformV1::PureExpression { root } => {
                require_expression(execution, root)?;
                let allowed = edge
                    .inputs
                    .iter()
                    .map(|input| input.expression)
                    .collect::<BTreeSet<_>>();
                validate_pure_transform(execution, root, &allowed)?;
            }
        }

        match (destination.identity.kind, edge.transfer) {
            (SemanticMemoryKindV1::RootScalar, SemanticMigrationTransferV1::Scalar) => {}
            (
                SemanticMemoryKindV1::IndexedField,
                SemanticMigrationTransferV1::IndexedField { owner },
            ) if destination.backing.row() == Some(owner) => {}
            (
                SemanticMemoryKindV1::ListOwner,
                SemanticMigrationTransferV1::List {
                    destination: row, ..
                },
            ) if destination.backing.row() == Some(row) => {}
            _ => {
                return Err(SemanticMemoryError::new(format!(
                    "semantic migration edge {} transfer {:?} does not match destination kind {:?}",
                    edge.id, edge.transfer, destination.identity.kind
                )));
            }
        }
    }
    Ok(())
}

#[derive(Serialize)]
struct SemanticMemoryDigestPayload<'a> {
    schema: &'a str,
    source_bundle_digest_v1: SourceBundleDigestV1,
    memories: &'a [SemanticMemoryV1],
    migration_edges: &'a [SemanticMigrationEdgeV1],
}

fn memory_graph_digest(
    graph: &SemanticMemoryGraphV1,
) -> Result<SemanticMemoryGraphDigestV1, SemanticMemoryError> {
    let payload = SemanticMemoryDigestPayload {
        schema: &graph.schema,
        source_bundle_digest_v1: graph.source_bundle_digest_v1,
        memories: &graph.memories,
        migration_edges: &graph.migration_edges,
    };
    boon_contract::canonical_serde_hash_v1(SEMANTIC_MEMORY_GRAPH_DIGEST_DOMAIN, &payload)
        .map(SemanticMemoryGraphDigestV1)
        .map_err(|error| {
            SemanticMemoryError::new(format!("failed to hash semantic memory graph: {error}"))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn checked_source(name: &str, source: &str) -> CheckedProgram {
        let parsed = boon_parser::parse_source(name, source).expect("parse memory fixture");
        let output = boon_typecheck::check_program(&parsed);
        assert!(
            !output.report.has_errors(),
            "unexpected fixture diagnostics: {:#?}",
            output.report.diagnostics
        );
        output.program.expect("fixture has checked program")
    }

    fn elaborate_source(name: &str, source: &str) -> (CheckedProgram, crate::SemanticProgram) {
        let checked = checked_source(name, source);
        let semantic = crate::elaborate(checked.clone(), &[]).expect("memory fixture elaborates");
        (checked, semantic)
    }

    fn graph(checked: &CheckedProgram, semantic: &crate::SemanticProgram) -> SemanticMemoryGraphV1 {
        build_semantic_memory_graph(
            checked,
            semantic.execution_graph(),
            semantic.resource_graph(),
            semantic.reactive_graph(),
            semantic.scope_storage_graph(),
            semantic.lowering_contract(),
        )
        .expect("semantic memory graph")
    }

    fn elaboration_error(name: &str, source: &str) -> String {
        let checked = checked_source(name, source);
        crate::elaborate(checked, &[])
            .expect_err("invalid memory fixture must fail semantic elaboration")
            .to_string()
    }

    fn synthetic_memory(
        id: usize,
        semantic_path: &str,
        data_type: Type,
        status: SemanticMemoryStatusV1,
    ) -> SemanticMemoryV1 {
        SemanticMemoryV1 {
            id: SemanticMemoryId(id),
            identity: SemanticMemoryIdentityV1 {
                source_unit: SemanticSourceUnitId(0),
                canonical_module: "test".to_owned(),
                owner_path: "$root".to_owned(),
                semantic_path: semantic_path.to_owned(),
                kind: SemanticMemoryKindV1::RootScalar,
            },
            backing: SemanticMemoryBackingV1::State {
                binding: SemanticBindingId(id),
                storage_field: SemanticStorageFieldId(id),
                state: SemanticStateId(id),
                row: None,
            },
            leaves: semantic_memory_leaves(&data_type),
            data_type,
            status,
        }
    }

    fn record_coverage_fixture() -> (Vec<SemanticMemoryV1>, DrainUse, DrainUse) {
        let record_type = Type::Object(boon_typecheck::ObjectShape {
            fields: BTreeMap::from([
                ("density".to_owned(), Type::Number),
                ("theme".to_owned(), Type::Number),
            ]),
            field_order: vec!["theme".to_owned(), "density".to_owned()],
            open: false,
        });
        let memories = vec![
            synthetic_memory(
                0,
                "old_settings",
                record_type,
                SemanticMemoryStatusV1::Draining {
                    marker: SemanticMigrationId(0),
                },
            ),
            synthetic_memory(
                1,
                "new_settings",
                Type::Number,
                SemanticMemoryStatusV1::Active,
            ),
        ];
        let use_for = |expression, projection: &str| DrainUse {
            drain: SemanticExprId(expression),
            source: resolve_memory_region(&memories[0], &[projection.to_owned()])
                .expect("record projection resolves"),
            destination: SemanticMemoryRegionV1 {
                memory: SemanticMemoryId(1),
                projection: Vec::new(),
            },
            initializer: SemanticMigrationInitializerV1::State {
                state: SemanticStateId(1),
                root: SemanticExprId(expression),
            },
        };
        let theme = use_for(0, "theme");
        let density = use_for(1, "density");
        (memories, theme, density)
    }

    #[test]
    fn scalar_rename_has_one_total_identity_edge() {
        let (checked, semantic) = elaborate_source(
            "memory-scalar-rename.bn",
            r#"
old_count:
    0
    |> HOLD old_count { LATEST {} }
    |> DRAINING

click_count:
    DRAIN { old_count }
    |> HOLD click_count { LATEST {} }
"#,
        );
        let graph = graph(&checked, &semantic);
        assert_eq!(graph.memories.len(), 2);
        assert_eq!(graph.migration_edges.len(), 1);
        let edge = &graph.migration_edges[0];
        assert_eq!(edge.inputs.len(), 1);
        assert!(matches!(
            require_memory(&graph.memories, edge.inputs[0].source.memory)
                .expect("source memory")
                .status,
            SemanticMemoryStatusV1::Draining { .. }
        ));
        assert!(matches!(
            require_memory(&graph.memories, edge.destination.memory)
                .expect("destination memory")
                .status,
            SemanticMemoryStatusV1::Active
        ));
        assert!(matches!(
            edge.transform,
            SemanticMigrationTransformV1::Identity { .. }
        ));
        graph
            .validate(
                &checked,
                semantic.execution_graph(),
                semantic.resource_graph(),
                semantic.reactive_graph(),
                semantic.scope_storage_graph(),
                semantic.lowering_contract(),
            )
            .expect("fresh graph revalidates from semantic authority");
    }

    #[test]
    fn split_record_drain_covers_each_authoritative_leaf_once() {
        let (memories, theme, density) = record_coverage_fixture();
        validate_source_coverage(&memories, &[theme, density])
            .expect("sibling projections cover every authoritative leaf once");
    }

    #[test]
    fn partial_record_drain_is_rejected() {
        let (memories, theme, _) = record_coverage_fixture();
        let error = validate_source_coverage(&memories, &[theme])
            .expect_err("partial record migration must fail")
            .to_string();
        assert!(
            error.contains("partial coverage"),
            "unexpected semantic error: {error}"
        );
        assert!(
            error.contains("density"),
            "missing leaf is absent from error: {error}"
        );
    }

    #[test]
    fn ordinary_read_of_draining_memory_is_rejected() {
        let error = elaboration_error(
            "memory-ordinary-read.bn",
            r#"
old_count:
    0
    |> HOLD old_count { LATEST {} }
    |> DRAINING

ordinary_copy: old_count

click_count:
    DRAIN { old_count }
    |> HOLD click_count { LATEST {} }
"#,
        );
        assert!(
            error.contains("ordinary read") && error.contains("DRAINING"),
            "unexpected semantic error: {error}"
        );
    }

    #[test]
    fn passed_drain_is_rejected_until_context_formal_identity_exists() {
        let error = validate_checked_drain_marker(
            SemanticExprId(0),
            boon_typecheck::CheckedExprId(0),
            &boon_typecheck::DeclId(7),
            &[],
            &CheckedExpressionKind::Passed {
                formal: boon_typecheck::ContextFormalId(3),
                projection: vec!["old_count".to_owned()],
                access: CheckedPassedAccess::Drain,
            },
        )
        .expect_err("PASSED drain must fail until semantic marker owns ContextFormalId")
        .to_string();
        assert!(
            error.contains("PASSED")
                && error.contains("ContextFormalId")
                && error.contains("rejects PASSED drains"),
            "unexpected semantic error: {error}"
        );
    }

    #[test]
    fn graph_revalidation_rejects_identity_tampering() {
        let (checked, semantic) = elaborate_source(
            "memory-identity-tamper.bn",
            r#"
old_count:
    0
    |> HOLD old_count { LATEST {} }
    |> DRAINING

click_count:
    DRAIN { old_count }
    |> HOLD click_count { LATEST {} }
"#,
        );
        let mut graph = graph(&checked, &semantic);
        graph.memories[0].identity.semantic_path.push_str(".forged");
        let error = graph
            .validate(
                &checked,
                semantic.execution_graph(),
                semantic.resource_graph(),
                semantic.reactive_graph(),
                semantic.scope_storage_graph(),
                semantic.lowering_contract(),
            )
            .expect_err("tampered identity must fail deterministic validation");
        assert!(
            error
                .to_string()
                .contains("differs from its deterministic semantic derivation")
        );
    }
}
