//! Exact semantic retained-view roots, constructor arguments, and bindings.
//!
//! This boundary consumes C's retained output contracts and the reactive
//! capture table. It never rediscovers roots from backend types and never uses
//! diagnostic node, attribute, or path strings as binding identity.

use crate::{
    SemanticBindingId, SemanticCallId, SemanticCallableId, SemanticCallableKind, SemanticCaptureId,
    SemanticExecutionImageColumnsV1, SemanticExprId, SemanticExpression, SemanticExpressionKind,
    SemanticLoweringContractV2, SemanticOutputContractId, SemanticOutputContractKindV1,
    SemanticReactiveGraphV1, SemanticReadBindingV1, SemanticReadId, SemanticReadTargetV1,
    SemanticResourceGraphV2, SemanticRowBinding, SemanticScopeId, SemanticScopeStorageGraphV1,
    SemanticSourceId, SemanticValueId, SemanticValueOrigin, SemanticViewCaptureTargetV1,
};
use boon_checked::{CheckedCallableKind, DeclId, Type};
use boon_contract::SourceBundleDigestV1;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt;

pub const SEMANTIC_VIEW_BINDING_GRAPH_SCHEMA_V1: &str = "boon.semantic-view-binding-graph.v1";
const SEMANTIC_VIEW_BINDING_GRAPH_DIGEST_DOMAIN: &[u8] = b"boon.semantic-view-binding-graph.v1\0";

macro_rules! view_id {
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

view_id!(
    SemanticViewRootId,
    SemanticViewNodeId,
    SemanticViewArgumentId,
    SemanticViewBindingId,
);

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SemanticViewBindingGraphDigestV1([u8; 32]);

impl SemanticViewBindingGraphDigestV1 {
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    pub fn to_hex(self) -> String {
        self.0.iter().map(|byte| format!("{byte:02x}")).collect()
    }
}

impl fmt::Display for SemanticViewBindingGraphDigestV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.to_hex())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticViewBindingGraphV1 {
    pub schema: String,
    pub source_bundle_digest_v1: SourceBundleDigestV1,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub roots: Vec<SemanticViewRootV1>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub nodes: Vec<SemanticViewNodeV1>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub arguments: Vec<SemanticViewArgumentV1>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bindings: Vec<SemanticViewBindingV1>,
    pub digest: SemanticViewBindingGraphDigestV1,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticViewRootV1 {
    pub id: SemanticViewRootId,
    pub output: SemanticOutputContractId,
    pub reactive_output_ordinal: usize,
    pub statement: crate::SemanticStatementId,
    pub expression: SemanticExprId,
    pub value: SemanticValueId,
    pub binding: SemanticBindingId,
    pub route_scope: SemanticScopeId,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticViewNodeV1 {
    pub id: SemanticViewNodeId,
    pub root: SemanticViewRootId,
    pub expression: SemanticExprId,
    pub value: SemanticValueId,
    pub call: SemanticCallId,
    pub callable: SemanticCallableId,
    /// Presentation only. Constructor identity is `callable` + `call`.
    pub diagnostic_kind: String,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticViewArgumentKindV1 {
    RenderTree,
    BindingInput,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticViewArgumentV1 {
    pub id: SemanticViewArgumentId,
    pub root: SemanticViewRootId,
    pub node: SemanticViewNodeId,
    pub call: SemanticCallId,
    pub callable: SemanticCallableId,
    pub formal: DeclId,
    pub ordinal: usize,
    pub expression: SemanticExprId,
    pub value: SemanticValueId,
    pub kind: SemanticViewArgumentKindV1,
    /// Presentation only. Formal identity is `callable` + `formal` + `ordinal`.
    pub diagnostic_attribute: String,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SemanticViewBindingTargetV1 {
    Data { read: SemanticReadId },
    Event { source: SemanticSourceId },
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticViewBindingKindV1 {
    Data,
    Source,
    Target,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticViewBindingV1 {
    pub id: SemanticViewBindingId,
    pub root: SemanticViewRootId,
    pub node: SemanticViewNodeId,
    pub argument: SemanticViewArgumentId,
    pub capture: SemanticCaptureId,
    pub expression: SemanticExprId,
    pub value: SemanticValueId,
    pub target: SemanticViewBindingTargetV1,
    pub kind: SemanticViewBindingKindV1,
    /// Exact retained-view attribute selected at the semantic leaf.
    pub canonical_attribute: String,
    /// Projection applied after the exact semantic read captured by `target`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub additional_projection: Vec<String>,
    pub route_scope: SemanticScopeId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub row: Option<SemanticRowBinding>,
    /// Presentation only; target selection is the typed `target` above.
    pub diagnostic_node: String,
    /// Presentation only; argument selection is the typed `argument` above.
    pub diagnostic_attribute: String,
    /// Presentation only; read/source identity is the typed `target` above.
    pub diagnostic_path: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SemanticViewBindingError {
    message: String,
}

impl SemanticViewBindingError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for SemanticViewBindingError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for SemanticViewBindingError {}

fn merge_view_capture_row_authorities(
    capture: SemanticCaptureId,
    capture_row: Option<SemanticRowBinding>,
    resource_row: Option<SemanticRowBinding>,
) -> Result<Option<SemanticRowBinding>, SemanticViewBindingError> {
    if let (Some(resource_row), Some(capture_row)) = (resource_row, capture_row)
        && resource_row != capture_row
    {
        return Err(SemanticViewBindingError::new(format!(
            "view capture {capture} reactive row authority {}/{} differs from exact storage route row authority {}/{}",
            capture_row.list, capture_row.scope, resource_row.list, resource_row.scope,
        )));
    }
    Ok(resource_row.or(capture_row))
}

pub fn build_semantic_view_binding_graph(
    execution: &SemanticExecutionImageColumnsV1,
    resources: &SemanticResourceGraphV2,
    reactive: &SemanticReactiveGraphV1,
    storage: &SemanticScopeStorageGraphV1,
    lowering: &SemanticLoweringContractV2,
) -> Result<SemanticViewBindingGraphV1, SemanticViewBindingError> {
    let mut graph =
        derive_semantic_view_binding_graph(execution, resources, reactive, storage, lowering)?;
    graph.digest = semantic_view_binding_graph_digest(&graph)?;
    Ok(graph)
}

impl SemanticViewBindingGraphV1 {
    pub fn validate(
        &self,
        execution: &SemanticExecutionImageColumnsV1,
        resources: &SemanticResourceGraphV2,
        reactive: &SemanticReactiveGraphV1,
        storage: &SemanticScopeStorageGraphV1,
        lowering: &SemanticLoweringContractV2,
    ) -> Result<(), SemanticViewBindingError> {
        if self.schema != SEMANTIC_VIEW_BINDING_GRAPH_SCHEMA_V1
            || self.source_bundle_digest_v1 != lowering.metadata.source_bundle_digest_v1
        {
            return Err(SemanticViewBindingError::new(
                "semantic view-binding schema or source digest differs",
            ));
        }
        if self.digest != semantic_view_binding_graph_digest(self)? {
            return Err(SemanticViewBindingError::new(
                "semantic view-binding digest does not match its canonical payload",
            ));
        }
        let expected =
            build_semantic_view_binding_graph(execution, resources, reactive, storage, lowering)?;
        if self != &expected {
            return Err(SemanticViewBindingError::new(
                "semantic view-binding graph differs from exact semantic roots and captures",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct ExactEventSourceLeaf {
    source: SemanticSourceId,
    relative_path: Vec<String>,
    /// Exact row in which this source leaf is exposed to the retained view.
    /// This can differ from the source's declaration row when an
    /// identity-preserving contextual operation forwards the source into a
    /// new list authority.
    resource_row: Option<SemanticRowBinding>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ResolvedViewCaptureRoute {
    target: SemanticViewBindingTargetV1,
    leaf_capture_target: SemanticViewCaptureTargetV1,
    diagnostic_path: String,
    resource_row: Option<SemanticRowBinding>,
    source_fallback_attribute: String,
}

fn exact_event_source_leaves(
    execution: &SemanticExecutionImageColumnsV1,
    resources: &SemanticResourceGraphV2,
    storage: &SemanticScopeStorageGraphV1,
    read: &SemanticReadBindingV1,
) -> Result<Vec<ExactEventSourceLeaf>, SemanticViewBindingError> {
    let expression = require_expression(execution, read.expression)?;
    if expression.value_id != read.value {
        return Err(SemanticViewBindingError::new(format!(
            "view read {} expression/value identity differs",
            read.id
        )));
    }

    // Storage row projections are the authority for event leaves exposed by
    // a row-bearing value. They deliberately apply map replacement priority
    // and preserve the output row for identity-forwarded sources. Generic
    // expression provenance still contains upstream scheduling dependencies,
    // which must not be mistaken for retained-view placement or leaf identity.
    if let Some(leaves) = exact_row_event_source_leaves(resources, storage, read)? {
        return Ok(leaves);
    }

    let mut leaves = BTreeSet::new();
    for member in &expression.provenance.members {
        match &member.origin {
            SemanticValueOrigin::Source { source, .. } => {
                resources
                    .sources
                    .get(source.as_usize())
                    .filter(|candidate| candidate.id == *source)
                    .ok_or_else(|| {
                        SemanticViewBindingError::new(format!(
                            "view read {} provenance references missing source {source}",
                            read.id
                        ))
                    })?;
                leaves.insert(ExactEventSourceLeaf {
                    source: *source,
                    relative_path: member.path.clone(),
                    resource_row: None,
                });
            }
            SemanticValueOrigin::ProducerSource {
                function,
                producer,
                identity,
                owner,
            } => {
                let sources = resources
                    .sources
                    .iter()
                    .filter(|source| {
                        source.owner == Some(*owner)
                            && matches!(
                                source.origin,
                                crate::SemanticSourceOrigin::ProducerInvocation {
                                    function: candidate_function,
                                    producer: candidate_producer,
                                    identity: candidate_identity,
                                } if candidate_function == *function
                                    && candidate_producer == *producer
                                    && candidate_identity == *identity
                            )
                    })
                    .map(|source| source.id)
                    .collect::<Vec<_>>();
                let [source] = sources.as_slice() else {
                    return Err(SemanticViewBindingError::new(format!(
                        "view read {} producer provenance resolves to {} exact sources",
                        read.id,
                        sources.len()
                    )));
                };
                resources
                    .sources
                    .get(source.as_usize())
                    .filter(|candidate| candidate.id == *source)
                    .ok_or_else(|| {
                        SemanticViewBindingError::new(format!(
                            "view read {} producer provenance references missing source {source}",
                            read.id
                        ))
                    })?;
                leaves.insert(ExactEventSourceLeaf {
                    source: *source,
                    relative_path: member.path.clone(),
                    resource_row: None,
                });
            }
            SemanticValueOrigin::MaterializationLocal {
                owner,
                local,
                projection,
            } => {
                let locals = storage
                    .locals
                    .iter()
                    .filter(|candidate| candidate.owner == *owner && candidate.local == *local)
                    .collect::<Vec<_>>();
                let [local_definition] = locals.as_slice() else {
                    return Err(SemanticViewBindingError::new(format!(
                        "view read {} materialization provenance {owner}:{local} resolves to {} exact storage locals",
                        read.id,
                        locals.len()
                    )));
                };
                let Some(row) = local_definition.row else {
                    continue;
                };
                for source_projection in storage
                    .row_source_projections
                    .iter()
                    .filter(|candidate| candidate.row == row)
                {
                    let Some(suffix) = source_projection.path.strip_prefix(projection.as_slice())
                    else {
                        continue;
                    };
                    resources
                        .sources
                        .get(source_projection.source.as_usize())
                        .filter(|candidate| candidate.id == source_projection.source)
                        .ok_or_else(|| {
                            SemanticViewBindingError::new(format!(
                                "view read {} row projection references missing source {}",
                                read.id, source_projection.source
                            ))
                        })?;
                    let mut relative_path = member.path.clone();
                    relative_path.extend_from_slice(suffix);
                    leaves.insert(ExactEventSourceLeaf {
                        source: source_projection.source,
                        relative_path,
                        resource_row: Some(row),
                    });
                }
            }
            SemanticValueOrigin::Runtime | SemanticValueOrigin::State { .. } => {}
        }
    }
    Ok(leaves.into_iter().collect())
}

/// Resolve the exact storage row represented by a read and enumerate only the
/// source projections published in that row. `Some(empty)` is authoritative:
/// a pure row value must not fall back to unrelated upstream provenance.
fn exact_row_event_source_leaves(
    resources: &SemanticResourceGraphV2,
    storage: &SemanticScopeStorageGraphV1,
    read: &SemanticReadBindingV1,
) -> Result<Option<Vec<ExactEventSourceLeaf>>, SemanticViewBindingError> {
    let (row, row_path) = match &read.target {
        SemanticReadTargetV1::MaterializationLocal {
            owner,
            local,
            projection,
        } => {
            let candidates = storage
                .locals
                .iter()
                .filter(|candidate| candidate.owner == *owner && candidate.local == *local)
                .collect::<Vec<_>>();
            let [local] = candidates.as_slice() else {
                return Err(SemanticViewBindingError::new(format!(
                    "view read {} materialization local {owner}:{local} resolves to {} exact storage locals",
                    read.id,
                    candidates.len()
                )));
            };
            let Some(row) = local.row else {
                return Ok(None);
            };
            (row, projection.clone())
        }
        SemanticReadTargetV1::Binding {
            binding,
            projection,
        } => {
            let candidates = storage
                .bindings
                .iter()
                .filter(|candidate| candidate.binding == *binding)
                .collect::<Vec<_>>();
            let [binding] = candidates.as_slice() else {
                return Err(SemanticViewBindingError::new(format!(
                    "view read {} binding {binding} resolves to {} exact storage bindings",
                    read.id,
                    candidates.len()
                )));
            };
            match binding.target {
                crate::SemanticStorageBindingTargetV1::List { row, .. } => {
                    if !projection.is_empty() {
                        return Ok(None);
                    }
                    (row, Vec::new())
                }
                crate::SemanticStorageBindingTargetV1::Value { row: Some(row), .. }
                | crate::SemanticStorageBindingTargetV1::State { row: Some(row), .. } => {
                    (row, projection.clone())
                }
                crate::SemanticStorageBindingTargetV1::Value { field, row: None } => {
                    let producer = storage
                        .fields
                        .get(field.as_usize())
                        .filter(|candidate| candidate.id == field)
                        .ok_or_else(|| {
                            SemanticViewBindingError::new(format!(
                                "view read {} binding {} references missing storage field {field}",
                                read.id, binding.binding
                            ))
                        })?
                        .producer;
                    let Some(producer) = producer else {
                        return Ok(None);
                    };
                    let matches = storage
                        .row_values
                        .iter()
                        .filter(|row_value| {
                            row_value.expression == producer
                                && projection.starts_with(&row_value.projection)
                        })
                        .collect::<Vec<_>>();
                    let consumed = matches
                        .iter()
                        .map(|row_value| row_value.projection.len())
                        .max();
                    let Some(consumed) = consumed else {
                        return Ok(None);
                    };
                    let rows = matches
                        .into_iter()
                        .filter(|row_value| row_value.projection.len() == consumed)
                        .map(|row_value| row_value.row)
                        .collect::<BTreeSet<_>>();
                    if rows.len() != 1 {
                        return Err(SemanticViewBindingError::new(format!(
                            "view read {} binding {} projection `{}` resolves to {} longest exact rows",
                            read.id,
                            binding.binding,
                            projection.join("."),
                            rows.len()
                        )));
                    }
                    (
                        *rows.iter().next().expect("one exact row was checked"),
                        projection[consumed..].to_vec(),
                    )
                }
                crate::SemanticStorageBindingTargetV1::Source { .. }
                | crate::SemanticStorageBindingTargetV1::State { row: None, .. } => {
                    return Ok(None);
                }
            }
        }
        SemanticReadTargetV1::SourcePayload { .. }
        | SemanticReadTargetV1::StateProjection { .. }
        | SemanticReadTargetV1::Local { .. }
        | SemanticReadTargetV1::External { .. }
        | SemanticReadTargetV1::ElementState { .. }
        | SemanticReadTargetV1::FunctionParameter { .. } => return Ok(None),
    };

    let mut leaves = BTreeSet::new();
    for source_projection in storage
        .row_source_projections
        .iter()
        .filter(|candidate| candidate.row == row)
    {
        let Some(relative_path) = source_projection.path.strip_prefix(row_path.as_slice()) else {
            continue;
        };
        resources
            .sources
            .get(source_projection.source.as_usize())
            .filter(|candidate| candidate.id == source_projection.source)
            .ok_or_else(|| {
                SemanticViewBindingError::new(format!(
                    "view read {} row value references missing source {}",
                    read.id, source_projection.source
                ))
            })?;
        leaves.insert(ExactEventSourceLeaf {
            source: source_projection.source,
            relative_path: relative_path.to_vec(),
            resource_row: Some(row),
        });
    }
    Ok(Some(leaves.into_iter().collect()))
}

fn exact_source_capture_route(
    resources: &SemanticResourceGraphV2,
    capture: SemanticCaptureId,
    leaf: ExactEventSourceLeaf,
) -> Result<ResolvedViewCaptureRoute, SemanticViewBindingError> {
    let source = resources
        .sources
        .get(leaf.source.as_usize())
        .filter(|candidate| candidate.id == leaf.source)
        .ok_or_else(|| {
            SemanticViewBindingError::new(format!(
                "view capture {capture} route references missing semantic source {}",
                leaf.source
            ))
        })?;
    let resource_row = leaf.resource_row.or_else(|| {
        source
            .target_list
            .zip(source.row_scope)
            .map(|(list, scope)| SemanticRowBinding { list, scope })
    });
    let source_fallback_attribute = leaf
        .relative_path
        .last()
        .cloned()
        .or_else(|| {
            source
                .semantic_path
                .rsplit('.')
                .next()
                .filter(|attribute| !attribute.is_empty())
                .map(str::to_owned)
        })
        .unwrap_or_else(|| "event".to_owned());
    Ok(ResolvedViewCaptureRoute {
        target: SemanticViewBindingTargetV1::Event { source: source.id },
        leaf_capture_target: SemanticViewCaptureTargetV1::Source { source: source.id },
        diagnostic_path: source.semantic_path.clone(),
        resource_row,
        source_fallback_attribute,
    })
}

fn semantic_read_diagnostic_path(
    resources: &SemanticResourceGraphV2,
    storage: &SemanticScopeStorageGraphV1,
    read: &SemanticReadBindingV1,
) -> Result<Option<String>, SemanticViewBindingError> {
    let append = |base: &str, projection: &[String]| {
        if projection.is_empty() {
            base.to_owned()
        } else {
            format!("{base}.{}", projection.join("."))
        }
    };
    match &read.target {
        SemanticReadTargetV1::Binding {
            binding,
            projection,
        } => {
            let candidates = storage
                .bindings
                .iter()
                .filter(|candidate| candidate.binding == *binding)
                .collect::<Vec<_>>();
            let [binding] = candidates.as_slice() else {
                return Err(SemanticViewBindingError::new(format!(
                    "view read {} binding {binding} resolves to {} exact storage bindings",
                    read.id,
                    candidates.len()
                )));
            };
            Ok(Some(append(&binding.diagnostic_path, projection)))
        }
        SemanticReadTargetV1::SourcePayload {
            source,
            payload_projection,
            projection,
            ..
        } => {
            let source = resources
                .sources
                .get(source.as_usize())
                .filter(|candidate| candidate.id == *source)
                .ok_or_else(|| {
                    SemanticViewBindingError::new(format!(
                        "view read {} references missing source {source}",
                        read.id
                    ))
                })?;
            let mut full_projection = payload_projection.clone();
            full_projection.extend(projection.clone());
            Ok(Some(append(&source.semantic_path, &full_projection)))
        }
        SemanticReadTargetV1::StateProjection {
            state, projection, ..
        } => {
            let state = resources
                .states
                .get(state.as_usize())
                .filter(|candidate| candidate.id == *state)
                .ok_or_else(|| {
                    SemanticViewBindingError::new(format!(
                        "view read {} references missing state {state}",
                        read.id
                    ))
                })?;
            Ok(Some(append(&state.path, projection)))
        }
        SemanticReadTargetV1::MaterializationLocal {
            owner,
            local,
            projection,
        } => {
            let candidates = storage
                .locals
                .iter()
                .filter(|candidate| candidate.owner == *owner && candidate.local == *local)
                .collect::<Vec<_>>();
            let [local] = candidates.as_slice() else {
                return Err(SemanticViewBindingError::new(format!(
                    "view read {} materialization local {owner}:{local} resolves to {} exact storage locals",
                    read.id,
                    candidates.len()
                )));
            };
            let Some(row) = local.row else {
                return Ok(None);
            };
            let list = resources
                .lists
                .get(row.list.as_usize())
                .filter(|candidate| candidate.id == row.list && candidate.row_scope == row.scope)
                .ok_or_else(|| {
                    SemanticViewBindingError::new(format!(
                        "view read {} local row {}/{} references missing list storage",
                        read.id, row.list, row.scope
                    ))
                })?;
            Ok(Some(append(&list.semantic_path, projection)))
        }
        SemanticReadTargetV1::External { canonical_path, .. } => Ok(Some(canonical_path.clone())),
        SemanticReadTargetV1::Local { .. }
        | SemanticReadTargetV1::ElementState { .. }
        | SemanticReadTargetV1::FunctionParameter { .. } => Ok(None),
    }
}

fn derive_semantic_view_binding_graph(
    execution: &SemanticExecutionImageColumnsV1,
    resources: &SemanticResourceGraphV2,
    reactive: &SemanticReactiveGraphV1,
    storage: &SemanticScopeStorageGraphV1,
    lowering: &SemanticLoweringContractV2,
) -> Result<SemanticViewBindingGraphV1, SemanticViewBindingError> {
    let mut roots = Vec::new();
    let mut nodes = Vec::new();
    let mut arguments = Vec::new();
    let mut bindings = Vec::new();

    for output in lowering.output_contracts.iter().filter(|output| {
        matches!(
            output.contract,
            SemanticOutputContractKindV1::RetainedVisualDocument
                | SemanticOutputContractKindV1::RetainedVisualScene
        )
    }) {
        let output_expression = require_expression(execution, output.expression)?;
        let reactive_matches = reactive
            .output_values
            .iter()
            .filter(|candidate| {
                candidate.checked_expression == output_expression.checked_expr_id
                    && candidate.expression == output.expression
                    && candidate.value == output.value
                    && candidate.statement == output.statement
            })
            .collect::<Vec<_>>();
        let [reactive_output] = reactive_matches.as_slice() else {
            return Err(SemanticViewBindingError::new(format!(
                "retained output {} resolves to {} exact reactive output roots",
                output.id,
                reactive_matches.len()
            )));
        };

        let root_id = SemanticViewRootId(roots.len());
        roots.push(SemanticViewRootV1 {
            id: root_id,
            output: output.id,
            reactive_output_ordinal: reactive_output.ordinal,
            statement: output.statement,
            expression: output.expression,
            value: output.value,
            binding: output.binding,
            route_scope: reactive_output.route_scope,
        });

        let mut reachable = reachable_expressions(execution, output.expression)?;
        let record_node = reachable.iter().find_map(|expression| {
            let expression = require_expression(execution, *expression).ok()?;
            record_style_render_node(expression).then_some(expression.id)
        });
        if let Some(expression) = record_node {
            return Err(SemanticViewBindingError::new(format!(
                "retained output {} contains unsupported record-style retained node {expression}; V1 requires exact registered constructor calls",
                output.id
            )));
        }
        if !output.typed_contract_known {
            return Err(SemanticViewBindingError::new(format!(
                "retained output {} has no exact typed constructor contract",
                output.id
            )));
        }

        let mut constructor_expressions = reachable
            .iter()
            .copied()
            .filter_map(|expression| {
                let definition = require_expression(execution, expression).ok()?;
                let SemanticExpressionKind::Call { function, .. } = &definition.kind else {
                    return None;
                };
                exact_view_constructor(output.contract, function).then_some(expression)
            })
            .collect::<Vec<_>>();
        constructor_expressions.sort();
        if constructor_expressions.is_empty() {
            return Err(SemanticViewBindingError::new(format!(
                "retained output {} has no exact registered constructor node",
                output.id
            )));
        }

        for constructor_expression in constructor_expressions {
            let expression = require_expression(execution, constructor_expression)?;
            let SemanticExpressionKind::Call {
                call,
                callable,
                callable_kind,
                name,
                function,
                arguments: call_arguments,
                parameter_bindings,
                ..
            } = &expression.kind
            else {
                unreachable!("constructor expressions are exact calls")
            };
            validate_constructor_call(
                execution,
                ViewConstructorCallIdentity {
                    expression,
                    call: *call,
                    callable: *callable,
                    callable_kind: *callable_kind,
                    diagnostic_name: name,
                    function,
                    parameter_bindings,
                },
            )?;
            let diagnostic_kind =
                canonical_view_kind(&expression.flow_type.ty).unwrap_or_else(|| function.clone());
            let node_id = SemanticViewNodeId(nodes.len());
            nodes.push(SemanticViewNodeV1 {
                id: node_id,
                root: root_id,
                expression: expression.id,
                value: expression.value_id,
                call: *call,
                callable: *callable,
                diagnostic_kind: diagnostic_kind.clone(),
            });

            let callable_definition = require_callable(execution, *callable)?;
            let mut explicit_arguments = call_arguments.iter().collect::<Vec<_>>();
            explicit_arguments.sort_by_key(|argument| argument.ordinal);
            for call_argument in explicit_arguments {
                let parameter_matches = callable_definition
                    .parameters
                    .iter()
                    .filter(|parameter| {
                        parameter.formal == call_argument.formal
                            && parameter.ordinal == call_argument.ordinal
                            && parameter.id.callable == *callable
                    })
                    .collect::<Vec<_>>();
                let [parameter] = parameter_matches.as_slice() else {
                    return Err(SemanticViewBindingError::new(format!(
                        "view constructor node {node_id} argument {} resolves to {} exact callable formals",
                        call_argument.ordinal,
                        parameter_matches.len()
                    )));
                };
                if parameter.name != call_argument.name {
                    return Err(SemanticViewBindingError::new(format!(
                        "view constructor node {node_id} argument {} diagnostic name differs from exact formal",
                        call_argument.ordinal
                    )));
                }
                let kind = if render_tree_formal(&parameter.flow_type.ty) {
                    SemanticViewArgumentKindV1::RenderTree
                } else {
                    SemanticViewArgumentKindV1::BindingInput
                };
                let argument_expression = require_expression(execution, call_argument.value)?;
                let argument_id = SemanticViewArgumentId(arguments.len());
                arguments.push(SemanticViewArgumentV1 {
                    id: argument_id,
                    root: root_id,
                    node: node_id,
                    call: *call,
                    callable: *callable,
                    formal: call_argument.formal,
                    ordinal: call_argument.ordinal,
                    expression: call_argument.value,
                    value: argument_expression.value_id,
                    kind,
                    diagnostic_attribute: parameter.name.clone(),
                });
                if kind == SemanticViewArgumentKindV1::RenderTree {
                    continue;
                }

                let argument_reachable =
                    binding_input_reachability(execution, call_argument.value, output.contract)?;
                let mut captures = reactive
                    .view_captures
                    .iter()
                    .filter(|capture| {
                        capture.output_ordinal == reactive_output.ordinal
                            && argument_reachable.expressions.contains(&capture.expression)
                    })
                    .collect::<Vec<_>>();
                captures.sort_by_key(|capture| capture.id);
                for capture in captures {
                    let capture_ancestors = argument_reachable.ancestors_of(capture.expression);
                    if !capture_ancestors.contains(&call_argument.value) {
                        return Err(SemanticViewBindingError::new(format!(
                            "view capture {} has no exact ancestor path from argument expression {}",
                            capture.id, call_argument.value
                        )));
                    }
                    let routes = match capture.target {
                        SemanticViewCaptureTargetV1::Read { read } => {
                            let read_definition = reactive
                                .reads
                                .get(read.as_usize())
                                .filter(|candidate| candidate.id == read)
                                .ok_or_else(|| {
                                    SemanticViewBindingError::new(format!(
                                        "view capture {} references missing semantic read {read}",
                                        capture.id
                                    ))
                                })?;
                            if read_definition.expression != capture.expression
                                || read_definition.value != capture.value
                            {
                                return Err(SemanticViewBindingError::new(format!(
                                    "view capture {} differs from exact semantic read {read}",
                                    capture.id
                                )));
                            }
                            let event_leaves = exact_event_source_leaves(
                                execution,
                                resources,
                                storage,
                                read_definition,
                            )?;
                            if event_leaves.is_empty() {
                                let diagnostic_path = semantic_read_diagnostic_path(
                                    resources,
                                    storage,
                                    read_definition,
                                )?
                                .unwrap_or_else(|| format!("read:{read}"));
                                let source_fallback_attribute = diagnostic_path
                                    .rsplit('.')
                                    .next()
                                    .filter(|attribute| !attribute.is_empty())
                                    .unwrap_or("event")
                                    .to_owned();
                                vec![ResolvedViewCaptureRoute {
                                    target: SemanticViewBindingTargetV1::Data { read },
                                    leaf_capture_target: capture.target,
                                    diagnostic_path,
                                    resource_row: None,
                                    source_fallback_attribute,
                                }]
                            } else {
                                event_leaves
                                    .into_iter()
                                    .map(|leaf| {
                                        exact_source_capture_route(resources, capture.id, leaf)
                                    })
                                    .collect::<Result<Vec<_>, _>>()?
                            }
                        }
                        SemanticViewCaptureTargetV1::Source { source } => {
                            vec![exact_source_capture_route(
                                resources,
                                capture.id,
                                ExactEventSourceLeaf {
                                    source,
                                    relative_path: Vec::new(),
                                    resource_row: None,
                                },
                            )?]
                        }
                        SemanticViewCaptureTargetV1::Field { .. } => continue,
                    };
                    let capture_expression = require_expression(execution, capture.expression)?;
                    if capture_expression.value_id != capture.value {
                        return Err(SemanticViewBindingError::new(format!(
                            "view capture {} expression/value identity differs",
                            capture.id
                        )));
                    }
                    let route_scope = expression_route_scope(execution, capture.expression)?;
                    let capture_row = capture
                        .row_scope
                        .map(|scope| {
                            resources
                                .row_scopes
                                .get(scope.as_usize())
                                .filter(|candidate| candidate.id == scope)
                                .map(|row| SemanticRowBinding {
                                    list: row.list,
                                    scope,
                                })
                                .ok_or_else(|| {
                                    SemanticViewBindingError::new(format!(
                                        "view capture {} references missing row scope {scope}",
                                        capture.id
                                    ))
                                })
                        })
                        .transpose()?;
                    for route in routes {
                        let row = merge_view_capture_row_authorities(
                            capture.id,
                            capture_row,
                            route.resource_row,
                        )?;
                        let leaf_bindings = binding_leaf_metadata(
                            execution,
                            call_argument.value,
                            capture.expression,
                            &capture_ancestors,
                            route.leaf_capture_target,
                            function,
                            &parameter.name,
                            &route.source_fallback_attribute,
                            output.contract,
                        )?;
                        if leaf_bindings.is_empty() {
                            return Err(SemanticViewBindingError::new(format!(
                                "view capture {} expression {} ({:?}) has no exact retained-view leaf under {} argument {} expression {} ({:?})",
                                capture.id,
                                capture.expression,
                                capture_expression.kind,
                                function,
                                parameter.name,
                                call_argument.value,
                                argument_expression.kind,
                            )));
                        }
                        for leaf in leaf_bindings {
                            bindings.push(SemanticViewBindingV1 {
                                id: SemanticViewBindingId(bindings.len()),
                                root: root_id,
                                node: node_id,
                                argument: argument_id,
                                capture: capture.id,
                                expression: capture.expression,
                                value: capture.value,
                                target: route.target,
                                kind: leaf.kind,
                                canonical_attribute: leaf.canonical_attribute,
                                additional_projection: leaf.additional_projection,
                                route_scope,
                                row,
                                diagnostic_node: diagnostic_kind.clone(),
                                diagnostic_attribute: parameter.name.clone(),
                                diagnostic_path: route.diagnostic_path.clone(),
                            });
                        }
                    }
                }
            }
        }
        reachable.clear();
    }

    Ok(SemanticViewBindingGraphV1 {
        schema: SEMANTIC_VIEW_BINDING_GRAPH_SCHEMA_V1.to_owned(),
        source_bundle_digest_v1: lowering.metadata.source_bundle_digest_v1,
        roots,
        nodes,
        arguments,
        bindings,
        digest: SemanticViewBindingGraphDigestV1([0; 32]),
    })
}

fn canonical_view_kind(ty: &Type) -> Option<String> {
    let Type::Object(shape) = ty else {
        return None;
    };
    let Some(Type::VariantSet(variants)) = shape.fields.get("kind") else {
        return None;
    };
    match variants.as_slice() {
        [boon_checked::Variant::Tag(tag)] => Some(tag.clone()),
        _ => None,
    }
}

struct ViewConstructorCallIdentity<'a> {
    expression: &'a SemanticExpression,
    call: SemanticCallId,
    callable: SemanticCallableId,
    callable_kind: SemanticCallableKind,
    diagnostic_name: &'a str,
    function: &'a str,
    parameter_bindings: &'a [crate::SemanticCallParameterBinding],
}

fn validate_constructor_call(
    execution: &SemanticExecutionImageColumnsV1,
    identity: ViewConstructorCallIdentity<'_>,
) -> Result<(), SemanticViewBindingError> {
    let ViewConstructorCallIdentity {
        expression,
        call,
        callable,
        callable_kind,
        diagnostic_name,
        function,
        parameter_bindings,
    } = identity;
    let callable_definition = require_callable(execution, callable)?;
    let call_definition = execution
        .calls
        .get(call.as_usize())
        .filter(|candidate| candidate.id == call)
        .ok_or_else(|| {
            SemanticViewBindingError::new(format!(
                "view constructor expression {} references missing semantic call {call}",
                expression.id
            ))
        })?;
    let value_parameters = callable_definition
        .parameters
        .iter()
        .filter(|parameter| parameter.kind == boon_checked::CheckedParameterKind::Value)
        .collect::<Vec<_>>();
    if callable_kind != SemanticCallableKind::Builtin
        || callable_definition.kind != CheckedCallableKind::Builtin
        || call_definition.callable != callable
        || call_definition.checked_expression != expression.checked_expr_id
        || call_definition.function != function
        || callable_definition.name != diagnostic_name
        || parameter_bindings.len() != value_parameters.len()
        || parameter_bindings
            .iter()
            .zip(value_parameters)
            .any(|(binding, parameter)| {
                binding.formal != parameter.formal
                    || binding.ordinal != parameter.ordinal
                    || binding.name != parameter.name
            })
    {
        return Err(SemanticViewBindingError::new(format!(
            "view constructor expression {} lacks exact builtin callable/formal identity",
            expression.id
        )));
    }
    Ok(())
}

fn reachable_expressions(
    execution: &SemanticExecutionImageColumnsV1,
    root: SemanticExprId,
) -> Result<BTreeSet<SemanticExprId>, SemanticViewBindingError> {
    let mut reachable = BTreeSet::new();
    let mut pending = vec![root];
    while let Some(expression) = pending.pop() {
        if !reachable.insert(expression) {
            continue;
        }
        let expression = require_expression(execution, expression)?;
        pending.extend(expression_children(execution, &expression.kind)?);
    }
    Ok(reachable)
}

struct BindingInputReachability {
    expressions: BTreeSet<SemanticExprId>,
    parents: BTreeMap<SemanticExprId, BTreeSet<SemanticExprId>>,
}

impl BindingInputReachability {
    fn ancestors_of(&self, expression: SemanticExprId) -> BTreeSet<SemanticExprId> {
        if !self.expressions.contains(&expression) {
            return BTreeSet::new();
        }
        let mut ancestors = BTreeSet::new();
        let mut pending = vec![expression];
        while let Some(expression) = pending.pop() {
            if !ancestors.insert(expression) {
                continue;
            }
            pending.extend(self.parents.get(&expression).into_iter().flatten().copied());
        }
        ancestors
    }
}

fn binding_input_reachability(
    execution: &SemanticExecutionImageColumnsV1,
    root: SemanticExprId,
    contract: SemanticOutputContractKindV1,
) -> Result<BindingInputReachability, SemanticViewBindingError> {
    let mut reachable = BTreeSet::new();
    let mut parents = BTreeMap::<SemanticExprId, BTreeSet<SemanticExprId>>::new();
    let mut pending = vec![root];
    while let Some(expression) = pending.pop() {
        if !reachable.insert(expression) {
            continue;
        }
        let expression = require_expression(execution, expression)?;
        if expression.id != root
            && matches!(
                &expression.kind,
                SemanticExpressionKind::Call { function, .. }
                    if exact_view_constructor(contract, function)
            )
        {
            continue;
        }
        for child in expression_children(execution, &expression.kind)? {
            parents.entry(child).or_default().insert(expression.id);
            pending.push(child);
        }
    }
    Ok(BindingInputReachability {
        expressions: reachable,
        parents,
    })
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct SemanticViewBindingLeaf {
    kind: SemanticViewBindingKindV1,
    canonical_attribute: String,
    additional_projection: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum BindingLeafMode {
    Data {
        attribute: String,
        kind: SemanticViewBindingKindV1,
    },
    Element,
    Style {
        attribute: String,
    },
    Events {
        attribute: Option<String>,
    },
}

struct BindingLeafTraversal<'a> {
    execution: &'a SemanticExecutionImageColumnsV1,
    capture: SemanticExprId,
    capture_ancestors: &'a BTreeSet<SemanticExprId>,
    capture_target: SemanticViewCaptureTargetV1,
    constructor: &'a str,
    source_fallback_attribute: &'a str,
    contract: SemanticOutputContractKindV1,
    result: BTreeSet<SemanticViewBindingLeaf>,
    visiting: BTreeSet<SemanticExprId>,
}

fn binding_leaf_metadata(
    execution: &SemanticExecutionImageColumnsV1,
    root: SemanticExprId,
    capture: SemanticExprId,
    capture_ancestors: &BTreeSet<SemanticExprId>,
    capture_target: SemanticViewCaptureTargetV1,
    constructor: &str,
    argument: &str,
    source_fallback_attribute: &str,
    contract: SemanticOutputContractKindV1,
) -> Result<Vec<SemanticViewBindingLeaf>, SemanticViewBindingError> {
    let mode = match argument {
        "element" => BindingLeafMode::Element,
        "style" => BindingLeafMode::Style {
            attribute: argument.to_owned(),
        },
        "target" => BindingLeafMode::Data {
            attribute: argument.to_owned(),
            kind: SemanticViewBindingKindV1::Target,
        },
        _ => BindingLeafMode::Data {
            attribute: argument.to_owned(),
            kind: SemanticViewBindingKindV1::Data,
        },
    };
    let mut traversal = BindingLeafTraversal {
        execution,
        capture,
        capture_ancestors,
        capture_target,
        constructor,
        source_fallback_attribute,
        contract,
        result: BTreeSet::new(),
        visiting: BTreeSet::new(),
    };
    traversal.visit(root, mode, Vec::new(), true)?;
    Ok(traversal.result.into_iter().collect())
}

impl BindingLeafTraversal<'_> {
    fn visit(
        &mut self,
        id: SemanticExprId,
        mode: BindingLeafMode,
        additional_projection: Vec<String>,
        is_root: bool,
    ) -> Result<(), SemanticViewBindingError> {
        if !self.capture_ancestors.contains(&id) {
            return Ok(());
        }
        if !self.visiting.insert(id) {
            return Ok(());
        }
        let expression = require_expression(self.execution, id)?;
        if !is_root
            && matches!(
                &expression.kind,
                SemanticExpressionKind::Call { function, .. }
                    if exact_view_constructor(self.contract, function)
            )
        {
            self.visiting.remove(&id);
            return Ok(());
        }
        if let SemanticExpressionKind::Project { input, fields } = &expression.kind {
            let mut projection = fields.clone();
            projection.extend(additional_projection);
            self.visit(*input, mode, projection, false)?;
            self.visiting.remove(&id);
            return Ok(());
        }
        if let SemanticExpressionKind::Call {
            function,
            arguments,
            ..
        } = &expression.kind
            && let Some(field_path) = function.strip_prefix("Field/")
        {
            let inputs = arguments
                .iter()
                .filter(|argument| argument.name == "input")
                .collect::<Vec<_>>();
            let [input] = inputs.as_slice() else {
                return Err(SemanticViewBindingError::new(format!(
                    "semantic field projection {id} resolves to {} exact input arguments",
                    inputs.len()
                )));
            };
            let mut projection = field_path
                .split('/')
                .filter(|field| !field.is_empty())
                .map(str::to_owned)
                .collect::<Vec<_>>();
            if projection.is_empty() {
                return Err(SemanticViewBindingError::new(format!(
                    "semantic field projection {id} has no canonical fields"
                )));
            }
            projection.extend(additional_projection);
            self.visit(input.value, mode, projection, false)?;
            self.visiting.remove(&id);
            return Ok(());
        }
        if id == self.capture {
            if matches!(mode, BindingLeafMode::Events { .. })
                && !matches!(
                    self.capture_target,
                    SemanticViewCaptureTargetV1::Source { .. }
                )
            {
                return Err(SemanticViewBindingError::new(format!(
                    "Element constructor `{}` argument `element.events` has no concrete SOURCE leaves; event expression {id} resolves to data",
                    self.constructor
                )));
            }
            let (attribute, read_kind) = match &mode {
                BindingLeafMode::Data { attribute, kind } => (attribute.as_str(), *kind),
                BindingLeafMode::Element => ("element", SemanticViewBindingKindV1::Data),
                BindingLeafMode::Style { attribute } => {
                    (attribute.as_str(), SemanticViewBindingKindV1::Data)
                }
                BindingLeafMode::Events { attribute } => (
                    attribute
                        .as_deref()
                        .unwrap_or(self.source_fallback_attribute),
                    SemanticViewBindingKindV1::Data,
                ),
            };
            let (kind, canonical_attribute) = match self.capture_target {
                SemanticViewCaptureTargetV1::Source { .. } => (
                    SemanticViewBindingKindV1::Source,
                    canonical_event_attribute(attribute).to_owned(),
                ),
                SemanticViewCaptureTargetV1::Read { .. } => (read_kind, attribute.to_owned()),
                SemanticViewCaptureTargetV1::Field { .. } => {
                    self.visiting.remove(&id);
                    return Ok(());
                }
            };
            self.result.insert(SemanticViewBindingLeaf {
                kind,
                canonical_attribute,
                additional_projection,
            });
            self.visiting.remove(&id);
            return Ok(());
        }

        match (&mode, &expression.kind) {
            (
                BindingLeafMode::Element,
                SemanticExpressionKind::Object(fields)
                | SemanticExpressionKind::TaggedObject { fields, .. },
            ) => {
                for field in fields {
                    let child_mode = if field.spread {
                        Some(BindingLeafMode::Element)
                    } else if matches!(field.name.as_str(), "event" | "events") {
                        Some(BindingLeafMode::Events { attribute: None })
                    } else if field.name == "target" {
                        Some(BindingLeafMode::Data {
                            attribute: field.name.clone(),
                            kind: SemanticViewBindingKindV1::Target,
                        })
                    } else {
                        Some(BindingLeafMode::Data {
                            attribute: field.name.clone(),
                            kind: SemanticViewBindingKindV1::Data,
                        })
                    };
                    if let Some(child_mode) = child_mode {
                        self.visit(
                            field.value,
                            child_mode,
                            additional_projection.clone(),
                            false,
                        )?;
                    }
                }
            }
            (
                BindingLeafMode::Style { .. },
                SemanticExpressionKind::Object(fields)
                | SemanticExpressionKind::TaggedObject { fields, .. },
            ) => {
                for field in fields {
                    let child_mode = if field.spread {
                        mode.clone()
                    } else {
                        BindingLeafMode::Style {
                            attribute: field.name.clone(),
                        }
                    };
                    self.visit(
                        field.value,
                        child_mode,
                        additional_projection.clone(),
                        false,
                    )?;
                }
            }
            (
                BindingLeafMode::Events { attribute },
                SemanticExpressionKind::Object(fields)
                | SemanticExpressionKind::TaggedObject { fields, .. },
            ) => {
                for field in fields {
                    self.visit(
                        field.value,
                        BindingLeafMode::Events {
                            attribute: if field.spread {
                                attribute.clone()
                            } else {
                                Some(field.name.clone())
                            },
                        },
                        additional_projection.clone(),
                        false,
                    )?;
                }
            }
            _ => {
                for child in expression_children(self.execution, &expression.kind)? {
                    self.visit(child, mode.clone(), additional_projection.clone(), false)?;
                }
            }
        }
        self.visiting.remove(&id);
        Ok(())
    }
}

fn canonical_event_attribute(attribute: &str) -> &str {
    if attribute == "key_down" {
        "submit"
    } else {
        attribute
    }
}

fn expression_children(
    execution: &SemanticExecutionImageColumnsV1,
    kind: &SemanticExpressionKind,
) -> Result<Vec<SemanticExprId>, SemanticViewBindingError> {
    execution.expression_children(kind).ok_or_else(|| {
        let SemanticExpressionKind::Materialize { materialization } = kind else {
            unreachable!("only invalid materialization references lack expression children");
        };
        SemanticViewBindingError::new(format!(
            "view traversal references missing semantic materialization {materialization}"
        ))
    })
}

fn record_style_render_node(expression: &SemanticExpression) -> bool {
    if !matches!(
        expression.kind,
        SemanticExpressionKind::Object(_) | SemanticExpressionKind::TaggedObject { .. }
    ) {
        return false;
    }
    let Type::Object(shape) = &expression.flow_type.ty else {
        return false;
    };
    let Some(Type::VariantSet(variants)) = shape.fields.get("kind") else {
        return false;
    };
    variants.iter().any(|variant| {
        matches!(
            variant,
            boon_checked::Variant::Tag(tag) if matches!(
                tag.as_str(),
                "Document"
                    | "Scene"
                    | "Button"
                    | "Checkbox"
                    | "Row"
                    | "Stack"
                    | "Block"
                    | "Text"
                    | "TextInput"
                    | "Label"
                    | "Paragraph"
                    | "Link"
                    | "EmbeddedProgram"
                    | "EmbeddedMedia"
                    | "MapViewport"
            )
        )
    })
}

fn render_tree_formal(ty: &Type) -> bool {
    match ty {
        Type::RenderContract => true,
        Type::List(item) => render_tree_formal(item),
        Type::Union(members) => !members.is_empty() && members.iter().all(render_tree_formal),
        Type::Text
        | Type::Number
        | Type::Bytes(_)
        | Type::Bits { .. }
        | Type::Object(_)
        | Type::VariantSet(_)
        | Type::Map { .. }
        | Type::Set(_)
        | Type::Absent
        | Type::Function { .. }
        | Type::UnresolvedShape { .. }
        | Type::Var(_)
        | Type::Unknown => false,
    }
}

fn exact_view_constructor(contract: SemanticOutputContractKindV1, function: &str) -> bool {
    const DOCUMENT: &[&str] = &[
        "Document/new",
        "Element/container",
        "Element/stripe",
        "Element/text",
        "Element/label",
        "Element/paragraph",
        "Element/link",
        "Element/button",
        "Element/checkbox",
        "Element/text_input",
        "Element/program",
        "Element/embedded_media",
        "Element/map",
    ];
    const SCENE: &[&str] = &[
        "Scene/new",
        "Scene/Element/stripe",
        "Scene/Element/block",
        "Scene/Element/text",
        "Scene/Element/text_input",
        "Scene/Element/program",
        "Scene/Element/checkbox",
        "Scene/Element/label",
        "Scene/Element/button",
        "Scene/Element/paragraph",
        "Scene/Element/link",
        "Scene/Element/embedded_media",
        "Scene/Element/map",
    ];
    match contract {
        SemanticOutputContractKindV1::RetainedVisualDocument => DOCUMENT.contains(&function),
        SemanticOutputContractKindV1::RetainedVisualScene => SCENE.contains(&function),
        SemanticOutputContractKindV1::HostValue => false,
    }
}

fn expression_route_scope(
    execution: &SemanticExecutionImageColumnsV1,
    expression: SemanticExprId,
) -> Result<SemanticScopeId, SemanticViewBindingError> {
    execution
        .route_scope(expression)
        .map_err(SemanticViewBindingError::new)
}

fn require_expression(
    execution: &SemanticExecutionImageColumnsV1,
    id: SemanticExprId,
) -> Result<&SemanticExpression, SemanticViewBindingError> {
    execution
        .expression(id)
        .map_err(SemanticViewBindingError::new)
}

fn require_callable(
    execution: &SemanticExecutionImageColumnsV1,
    id: SemanticCallableId,
) -> Result<&crate::SemanticCallable, SemanticViewBindingError> {
    execution
        .callable(id)
        .map_err(SemanticViewBindingError::new)
}

#[derive(Serialize)]
struct ViewBindingDigestPayload<'a> {
    schema: &'a str,
    source_bundle_digest_v1: SourceBundleDigestV1,
    roots: &'a [SemanticViewRootV1],
    nodes: &'a [SemanticViewNodeV1],
    arguments: &'a [SemanticViewArgumentV1],
    bindings: &'a [SemanticViewBindingV1],
}

fn semantic_view_binding_graph_digest(
    graph: &SemanticViewBindingGraphV1,
) -> Result<SemanticViewBindingGraphDigestV1, SemanticViewBindingError> {
    boon_contract::canonical_serde_hash_v1(
        SEMANTIC_VIEW_BINDING_GRAPH_DIGEST_DOMAIN,
        &ViewBindingDigestPayload {
            schema: &graph.schema,
            source_bundle_digest_v1: graph.source_bundle_digest_v1,
            roots: &graph.roots,
            nodes: &graph.nodes,
            arguments: &graph.arguments,
            bindings: &graph.bindings,
        },
    )
    .map(SemanticViewBindingGraphDigestV1)
    .map_err(|error| {
        SemanticViewBindingError::new(format!(
            "failed to hash semantic view-binding graph: {error}"
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn checked(source: &str) -> boon_checked::CheckedProgram {
        let parsed = boon_parser::parse_source("view-contract.bn", source).expect("parse");
        let output = boon_typecheck::check_program(&parsed);
        assert!(
            !output.report.has_errors(),
            "unexpected type diagnostics: {:#?}",
            output.report.diagnostics
        );
        output.program.expect("checked program")
    }

    fn document_fixture() -> crate::SemanticProgram {
        crate::elaborate(
            checked(
                r#"
store: [
    value: 1
]
document: Document/new(
    root: Element/label(
        element: []
        style: []
        label: store.value
    )
)
"#,
            ),
            &[],
        )
        .expect("semantic document")
    }

    #[test]
    fn retained_root_constructor_formals_and_data_captures_are_exact() {
        let semantic = document_fixture();
        let graph = semantic.view_binding_graph();
        assert_eq!(graph.roots.len(), 1);
        assert_eq!(graph.nodes.len(), 2);
        assert!(graph.arguments.iter().any(|argument| {
            argument.kind == SemanticViewArgumentKindV1::RenderTree
                && argument.diagnostic_attribute == "root"
        }));
        let data = graph
            .bindings
            .iter()
            .find(|binding| matches!(binding.target, SemanticViewBindingTargetV1::Data { .. }))
            .expect("label read is an exact data binding");
        let argument = &graph.arguments[data.argument.as_usize()];
        assert_eq!(data.kind, SemanticViewBindingKindV1::Data);
        assert_eq!(data.canonical_attribute, "label");
        assert!(data.additional_projection.is_empty());
        assert_eq!(
            argument.formal,
            semantic.execution_graph().callables[argument.callable.as_usize()].parameters
                [argument.ordinal]
                .formal
        );
        assert_eq!(
            data.capture,
            semantic.reactive_graph().view_captures[data.capture.as_usize()].id
        );
        graph
            .validate(
                semantic.execution_graph(),
                semantic.resource_graph(),
                semantic.reactive_graph(),
                semantic.scope_storage_graph(),
                semantic.lowering_contract(),
            )
            .expect("fresh view graph validates");
    }

    #[test]
    fn event_capture_targets_exact_semantic_source() {
        let semantic = crate::elaborate(
            checked(
                r#"
store: [
    clicked: SOURCE
]
document: Document/new(
    root: Element/button(
        element: [
            events: [
                click: store.clicked
            ]
        ]
        style: []
        label: TEXT { Click }
    )
)
"#,
            ),
            &[],
        )
        .expect("semantic event document");
        let graph = semantic.view_binding_graph();
        let event = graph
            .bindings
            .iter()
            .find_map(|binding| match binding.target {
                SemanticViewBindingTargetV1::Event { source } => Some((binding, source)),
                SemanticViewBindingTargetV1::Data { .. } => None,
            })
            .expect("button event is an exact source binding");
        assert_eq!(
            semantic.resource_graph().sources[event.1.as_usize()].id,
            event.1
        );
        assert_eq!(
            semantic.reactive_graph().view_captures[event.0.capture.as_usize()].target,
            SemanticViewCaptureTargetV1::Source { source: event.1 }
        );
        assert_eq!(event.0.kind, SemanticViewBindingKindV1::Source);
        assert_eq!(event.0.canonical_attribute, "click");
        assert!(event.0.additional_projection.is_empty());
    }

    #[test]
    fn mapped_list_capture_uses_output_row_sources_not_upstream_dependencies() {
        let semantic = crate::elaborate(
            checked(
                r#"
store: [
    rows:
        LIST { [name: TEXT { one }] }
        |> List/map(item, new: base_row(row: item))
    visible_rows:
        rows
        |> List/filter(item, if: item.name == TEXT { one })
        |> List/map(item, new: visible_row(row: item))
]

scene: main_scene(PASS: [store: store])

FUNCTION base_row(row) {
    [name: row.name, event: SOURCE]
}

FUNCTION visible_row(row) {
    [name: row.name, event: SOURCE]
}

FUNCTION main_scene() {
    Scene/new(
        root: Scene/Element/stripe(
            element: []
            direction: Column
            gap: 0
            style: []
            items:
                PASSED.store.visible_rows
                |> List/map(item, new: row_element(row: item))
        )
        lights: []
        geometry: []
    )
}

FUNCTION row_element(row) {
    Scene/Element/text(
        element: [events: [press: row.event]]
        style: []
        text: row.name
    )
}
"#,
            ),
            &[],
        )
        .expect("semantic mapped-row scene");
        let resources = semantic.resource_graph();
        let rows = resources
            .lists
            .iter()
            .find(|list| list.local_name == "rows")
            .expect("base rows list");
        let visible = resources
            .lists
            .iter()
            .find(|list| list.local_name == "visible_rows")
            .expect("visible rows list");
        assert_ne!(rows.row_scope, visible.row_scope);
        let visible_row = SemanticRowBinding {
            list: visible.id,
            scope: visible.row_scope,
        };

        let projected_store_capture = semantic
            .reactive_graph()
            .view_captures
            .iter()
            .find(|capture| {
                let SemanticViewCaptureTargetV1::Read { read } = capture.target else {
                    return false;
                };
                matches!(
                    semantic
                        .reactive_graph()
                        .reads
                        .get(read.as_usize())
                        .filter(|candidate| candidate.id == read)
                        .map(|read| &read.target),
                    Some(SemanticReadTargetV1::Binding { projection, .. })
                        if projection == &["visible_rows".to_owned()]
                )
            })
            .expect("projected whole-store visible-list capture");
        assert_eq!(projected_store_capture.row_scope, Some(visible.row_scope));

        let visible_captures = semantic
            .reactive_graph()
            .view_captures
            .iter()
            .filter(|capture| capture.row_scope == Some(visible.row_scope))
            .collect::<Vec<_>>();
        assert!(
            !visible_captures.is_empty(),
            "the whole visible-list read must retain its output-row authority"
        );
        let event_leaves = semantic
            .reactive_graph()
            .view_captures
            .iter()
            .filter_map(|capture| match capture.target {
                SemanticViewCaptureTargetV1::Read { read } => semantic
                    .reactive_graph()
                    .reads
                    .get(read.as_usize())
                    .filter(|candidate| candidate.id == read),
                SemanticViewCaptureTargetV1::Source { .. }
                | SemanticViewCaptureTargetV1::Field { .. } => None,
            })
            .map(|read| {
                exact_event_source_leaves(
                    semantic.execution_graph(),
                    resources,
                    semantic.scope_storage_graph(),
                    read,
                )
            })
            .collect::<Result<Vec<_>, _>>()
            .expect("exact mapped-row event leaves")
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
        assert!(
            !event_leaves.is_empty(),
            "mapped row event leaf; captures={:#?}; row projections={:#?}",
            semantic.reactive_graph().view_captures,
            semantic.scope_storage_graph().row_source_projections,
        );
        assert!(
            event_leaves
                .iter()
                .all(|leaf| { leaf.resource_row == Some(visible_row) }),
            "view event leaves must belong only to the mapped output row: {event_leaves:#?}"
        );
        assert!(
            event_leaves
                .iter()
                .all(|leaf| leaf.resource_row.map(|row| row.scope) != Some(rows.row_scope)),
            "upstream row dependencies must not become retained-view placement"
        );

        assert_eq!(
            merge_view_capture_row_authorities(
                projected_store_capture.id,
                Some(visible_row),
                Some(visible_row),
            )
            .expect("matching reactive and storage rows"),
            Some(visible_row)
        );
        let error = merge_view_capture_row_authorities(
            projected_store_capture.id,
            Some(SemanticRowBinding {
                list: rows.id,
                scope: rows.row_scope,
            }),
            Some(visible_row),
        )
        .expect_err("conflicting reactive and storage row authorities must fail closed");
        assert!(
            error.to_string().contains("reactive row authority"),
            "{error}"
        );
    }

    #[test]
    fn filtered_list_capture_rebinds_forwarded_sources_to_the_output_row() {
        let semantic = crate::elaborate(
            checked(
                r#"
store: [
    rows:
        LIST { [name: TEXT { one }] }
        |> List/map(item, new: event_row(row: item))
    filtered_rows:
        rows
        |> List/filter(item, if: item.name == TEXT { one })
]

scene: main_scene(PASS: [store: store])

FUNCTION event_row(row) {
    [name: row.name, event: SOURCE]
}

FUNCTION main_scene() {
    Scene/new(
        root: Scene/Element/stripe(
            element: []
            direction: Column
            gap: 0
            style: []
            items:
                PASSED.store.filtered_rows
                |> List/map(item, new: row_element(row: item))
        )
        lights: []
        geometry: []
    )
}

FUNCTION row_element(row) {
    Scene/Element/text(
        element: [events: [press: row.event]]
        style: []
        text: row.name
    )
}
"#,
            ),
            &[],
        )
        .expect("semantic filtered-row scene");
        let resources = semantic.resource_graph();
        let rows = resources
            .lists
            .iter()
            .find(|list| list.local_name == "rows")
            .expect("input rows list");
        let filtered = resources
            .lists
            .iter()
            .find(|list| list.local_name == "filtered_rows")
            .expect("filtered rows list");
        assert_ne!(rows.row_scope, filtered.row_scope);
        let source = resources.sources.first().expect("forwarded row source");
        assert_eq!(source.target_list, Some(rows.id));
        assert_eq!(source.row_scope, Some(rows.row_scope));
        assert!(
            semantic
                .scope_storage_graph()
                .row_source_projections
                .iter()
                .any(|projection| {
                    projection.source == source.id
                        && projection.row
                            == SemanticRowBinding {
                                list: filtered.id,
                                scope: filtered.row_scope,
                            }
                })
        );

        let leaves = semantic
            .reactive_graph()
            .view_captures
            .iter()
            .filter_map(|capture| match capture.target {
                SemanticViewCaptureTargetV1::Read { read } => semantic
                    .reactive_graph()
                    .reads
                    .get(read.as_usize())
                    .filter(|candidate| candidate.id == read),
                SemanticViewCaptureTargetV1::Source { .. }
                | SemanticViewCaptureTargetV1::Field { .. } => None,
            })
            .map(|read| {
                exact_event_source_leaves(
                    semantic.execution_graph(),
                    resources,
                    semantic.scope_storage_graph(),
                    read,
                )
            })
            .collect::<Result<Vec<_>, _>>()
            .expect("exact filtered-row event leaves")
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
        assert!(
            leaves.iter().any(|leaf| {
                leaf.source == source.id
                    && leaf.resource_row
                        == Some(SemanticRowBinding {
                            list: filtered.id,
                            scope: filtered.row_scope,
                        })
            }),
            "the forwarded source ID must retain the filtered output-row placement: {leaves:#?}"
        );
        assert!(
            leaves
                .iter()
                .all(|leaf| leaf.resource_row.map(|row| row.scope) != Some(rows.row_scope)),
            "the source declaration row must not override the filtered capture row"
        );
    }

    #[test]
    fn nested_target_and_style_bindings_preserve_exact_leaf_kind_and_attribute() {
        let semantic = crate::elaborate(
            checked(
                r#"
store: [
    target: TEXT { target }
    size: 14
]
document: Document/new(
    root: Element/label(
        element: [
            target: store.target
        ]
        style: [
            font: [
                size: store.size
            ]
        ]
        label: TEXT { Label }
    )
)
"#,
            ),
            &[],
        )
        .expect("semantic target/style document");
        let graph = semantic.view_binding_graph();
        let target = graph
            .bindings
            .iter()
            .find(|binding| binding.canonical_attribute == "target")
            .expect("nested element target binding");
        assert_eq!(target.kind, SemanticViewBindingKindV1::Target);
        let size = graph
            .bindings
            .iter()
            .find(|binding| binding.canonical_attribute == "size")
            .expect("nested style leaf binding");
        assert_eq!(size.kind, SemanticViewBindingKindV1::Data);
    }

    #[test]
    fn projected_binding_preserves_projection_after_the_exact_semantic_read() {
        let semantic = crate::elaborate(
            checked(
                r#"
FUNCTION wrap(value) {
    [leaf: value]
}
store: [
    value: TEXT { projected }
]
document: Document/new(
    root: Element/label(
        element: []
        style: []
        label: wrap(value: store.value).leaf
    )
)
"#,
            ),
            &[],
        )
        .expect("semantic projected document");
        let binding = semantic
            .view_binding_graph()
            .bindings
            .iter()
            .find(|binding| binding.canonical_attribute == "label")
            .expect("projected label binding");
        assert_eq!(binding.kind, SemanticViewBindingKindV1::Data);
        assert_eq!(binding.additional_projection, ["leaf"]);
    }

    #[test]
    fn validation_rejects_formal_target_scope_and_diagnostic_mutations() {
        let semantic = document_fixture();
        let fresh = semantic.view_binding_graph().clone();
        let rejects = |graph: &SemanticViewBindingGraphV1| {
            graph
                .validate(
                    semantic.execution_graph(),
                    semantic.resource_graph(),
                    semantic.reactive_graph(),
                    semantic.scope_storage_graph(),
                    semantic.lowering_contract(),
                )
                .is_err()
        };

        let mut formal = fresh.clone();
        formal.arguments[0].formal = DeclId(usize::MAX as u32);
        formal.digest = semantic_view_binding_graph_digest(&formal).unwrap();
        assert!(rejects(&formal));

        let mut target = fresh.clone();
        let binding = target.bindings.first_mut().expect("data binding");
        binding.target = SemanticViewBindingTargetV1::Data {
            read: SemanticReadId(usize::MAX),
        };
        target.digest = semantic_view_binding_graph_digest(&target).unwrap();
        assert!(rejects(&target));

        let mut scope = fresh.clone();
        scope.bindings[0].route_scope = SemanticScopeId(usize::MAX);
        scope.digest = semantic_view_binding_graph_digest(&scope).unwrap();
        assert!(rejects(&scope));

        let mut diagnostic = fresh;
        diagnostic.bindings[0].diagnostic_path = "misleading.presentation.path".to_owned();
        diagnostic.digest = semantic_view_binding_graph_digest(&diagnostic).unwrap();
        assert!(rejects(&diagnostic));
    }

    #[test]
    fn record_style_retained_nodes_are_explicitly_rejected_in_v1() {
        let error = crate::elaborate(
            checked(
                r#"
document: [kind: Document, root: [kind: Text, text: TEXT { unsupported record node }]]
"#,
            ),
            &[],
        )
        .expect_err("record-style retained node must fail closed");
        assert!(
            error
                .to_string()
                .contains("unsupported record-style retained node"),
            "{error}"
        );
    }
}
