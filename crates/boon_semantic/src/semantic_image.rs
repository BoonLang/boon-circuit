//! Canonical checked/execution image ownership.
//!
//! Contextual expansion constructs the execution columns in `ExecutionPending`.
//! Resource elaboration is the only phase allowed to mutate those columns. The
//! builder then crosses a consuming post-resource validation boundary, hashes
//! every final row once, and seals the checked and execution receipts beside
//! the columns. Later semantic phases borrow the sealed columns; they never own
//! or materialize a second execution graph.

use crate::{
    DistributedCallOccurrenceRoot, OutCallInstanceId, ResolvedOutGraph,
    SemanticExecutionImageColumnsV1, SemanticExprId, SemanticFunction, SemanticStatementId,
    StaticOwnerId,
};
use boon_checked::{
    CHECKED_IMAGE_HANDOFF_SCHEMA_V1, CheckedImageHandoffV1, CheckedImageRowDomainV1,
    CheckedShardOwnerKeyV1, CheckedShardProjectionKeyV1, CheckedShardRegionV1, ProgramRole,
};
use boon_contract::SourceBundleDigestV1;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::marker::PhantomData;

pub const SEMANTIC_IMAGE_SCHEMA_V1: &str = "boon.semantic-image.v1";
pub const EXECUTION_IMAGE_HANDOFF_SCHEMA_V1: &str = "boon.execution-image-handoff.v1";

const EXECUTION_IMAGE_ROW_PAYLOAD_DOMAIN_V1: &[u8] = b"boon.execution-image-row-payload.v1\0";
const EXECUTION_IMAGE_SHARD_DOMAIN_V1: &[u8] = b"boon.execution-image-shard.v1\0";
const EXECUTION_IMAGE_HANDOFF_DOMAIN_V1: &[u8] = b"boon.execution-image-handoff.v1\0";
const SEMANTIC_IMAGE_SEAL_DOMAIN_V1: &[u8] = b"boon.semantic-image-seal.v1\0";

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionImageRowDomainV1 {
    Scope,
    Expression,
    ExpressionOrigin,
    Statement,
    Callable,
    Call,
    CallOccurrence,
    Source,
    State,
    Root,
    Function,
    Materialization,
    StaticOwner,
}

/// Stable execution projection. Definition rows reuse their checked shard;
/// concrete occurrences carry their full stable invocation path.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SemanticImageProjectionKeyV1 {
    Checked {
        projection: CheckedShardProjectionKeyV1,
    },
    Invocation {
        root: DistributedCallOccurrenceRoot,
        definition: CheckedShardOwnerKeyV1,
        call_path: Vec<CheckedShardProjectionKeyV1>,
    },
}

impl SemanticImageProjectionKeyV1 {
    pub fn checked(projection: CheckedShardProjectionKeyV1) -> Self {
        Self::Checked { projection }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ExecutionImageShardReceiptV1 {
    pub projection: SemanticImageProjectionKeyV1,
    pub local_content_digest: [u8; 32],
    pub row_count: u32,
    pub dependency_row_count: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub relocations: Vec<SemanticImageProjectionKeyV1>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ExecutionImageEntityRouteV1 {
    pub domain: ExecutionImageRowDomainV1,
    pub dense_index: u32,
    pub projection: SemanticImageProjectionKeyV1,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ExecutionImageHandoffV1 {
    pub schema: String,
    pub source_bundle_digest_v1: SourceBundleDigestV1,
    pub role: ProgramRole,
    pub shards: Vec<ExecutionImageShardReceiptV1>,
    pub entity_routes: Vec<ExecutionImageEntityRouteV1>,
    pub local_image_digest: [u8; 32],
}

impl ExecutionImageHandoffV1 {
    pub fn entity_projection(
        &self,
        domain: ExecutionImageRowDomainV1,
        dense_index: usize,
    ) -> Option<&SemanticImageProjectionKeyV1> {
        let dense_index = u32::try_from(dense_index).ok()?;
        self.entity_routes
            .binary_search_by_key(&(domain, dense_index), |route| {
                (route.domain, route.dense_index)
            })
            .ok()
            .map(|index| &self.entity_routes[index].projection)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SealedSemanticImageV1 {
    schema: String,
    checked_handoff: CheckedImageHandoffV1,
    execution_handoff: ExecutionImageHandoffV1,
    execution: SemanticExecutionImageColumnsV1,
    seal_digest: [u8; 32],
}

impl SealedSemanticImageV1 {
    pub const fn checked_handoff(&self) -> &CheckedImageHandoffV1 {
        &self.checked_handoff
    }

    pub const fn execution_handoff(&self) -> &ExecutionImageHandoffV1 {
        &self.execution_handoff
    }

    pub const fn execution(&self) -> &SemanticExecutionImageColumnsV1 {
        &self.execution
    }

    pub const fn seal_digest(&self) -> [u8; 32] {
        self.seal_digest
    }

    pub(crate) fn validate_identity(
        &self,
        source_bundle_digest_v1: SourceBundleDigestV1,
        role: ProgramRole,
    ) -> Result<(), String> {
        if self.schema != SEMANTIC_IMAGE_SCHEMA_V1
            || self.checked_handoff.schema != CHECKED_IMAGE_HANDOFF_SCHEMA_V1
            || self.execution_handoff.schema != EXECUTION_IMAGE_HANDOFF_SCHEMA_V1
        {
            return Err("semantic image contains an unsupported schema".to_owned());
        }
        if self.checked_handoff.source_bundle_digest_v1 != source_bundle_digest_v1
            || self.execution_handoff.source_bundle_digest_v1 != source_bundle_digest_v1
            || self.checked_handoff.role != role
            || self.execution_handoff.role != role
        {
            return Err(
                "semantic image receipts disagree on source-bundle or role identity".to_owned(),
            );
        }
        let expected = semantic_image_seal_digest(
            &self.schema,
            &self.checked_handoff,
            &self.execution_handoff,
        )?;
        if self.seal_digest != expected {
            return Err("semantic image seal digest is stale".to_owned());
        }
        Ok(())
    }
}

pub(crate) struct ExecutionPending;
pub(crate) struct ExecutionFinalized;

/// This witness is deliberately private. It can only be created by consuming
/// a pending builder through the post-resource validator below.
struct PostResourceValidatedV1;

pub(crate) struct SemanticImageBuilder<State> {
    checked_handoff: CheckedImageHandoffV1,
    execution: SemanticExecutionImageColumnsV1,
    execution_handoff: Option<ExecutionImageHandoffV1>,
    state: PhantomData<State>,
}

impl SemanticImageBuilder<ExecutionPending> {
    pub(crate) fn execution_pending(
        checked_handoff: CheckedImageHandoffV1,
        execution: SemanticExecutionImageColumnsV1,
    ) -> Result<Self, String> {
        if checked_handoff.schema != CHECKED_IMAGE_HANDOFF_SCHEMA_V1 {
            return Err(format!(
                "unsupported checked image handoff schema `{}`",
                checked_handoff.schema
            ));
        }
        Ok(Self {
            checked_handoff,
            execution,
            execution_handoff: None,
            state: PhantomData,
        })
    }

    pub(crate) const fn execution(&self) -> &SemanticExecutionImageColumnsV1 {
        &self.execution
    }

    /// The resource builder is the sole production mutation window. This is
    /// crate-private so no downstream phase can acquire mutable columns.
    pub(super) fn execution_for_resource(&mut self) -> &mut SemanticExecutionImageColumnsV1 {
        &mut self.execution
    }

    pub(crate) fn finalize_execution(
        self,
        out: &ResolvedOutGraph,
    ) -> Result<SemanticImageBuilder<ExecutionFinalized>, String> {
        self.execution.validate(out)?;
        let witness = PostResourceValidatedV1;
        self.finish_execution(witness, out)
    }

    fn finish_execution(
        self,
        _witness: PostResourceValidatedV1,
        out: &ResolvedOutGraph,
    ) -> Result<SemanticImageBuilder<ExecutionFinalized>, String> {
        let execution_handoff =
            execution_image_handoff(&self.checked_handoff, out, &self.execution)?;
        Ok(SemanticImageBuilder {
            checked_handoff: self.checked_handoff,
            execution: self.execution,
            execution_handoff: Some(execution_handoff),
            state: PhantomData,
        })
    }
}

impl SemanticImageBuilder<ExecutionFinalized> {
    pub(crate) const fn execution(&self) -> &SemanticExecutionImageColumnsV1 {
        &self.execution
    }

    pub(crate) const fn checked_handoff(&self) -> &CheckedImageHandoffV1 {
        &self.checked_handoff
    }

    pub(crate) fn execution_handoff(&self) -> &ExecutionImageHandoffV1 {
        self.execution_handoff
            .as_ref()
            .expect("execution-finalized typestate always carries a handoff")
    }

    pub(crate) fn seal(self) -> Result<SealedSemanticImageV1, String> {
        let execution_handoff = self
            .execution_handoff
            .ok_or_else(|| "finalized execution builder has no handoff".to_owned())?;
        let schema = SEMANTIC_IMAGE_SCHEMA_V1.to_owned();
        let seal_digest =
            semantic_image_seal_digest(&schema, &self.checked_handoff, &execution_handoff)?;
        Ok(SealedSemanticImageV1 {
            schema,
            checked_handoff: self.checked_handoff,
            execution_handoff,
            execution: self.execution,
            seal_digest,
        })
    }
}

#[derive(Serialize)]
struct ExecutionImageRowReceipt<'a, T> {
    domain: ExecutionImageRowDomainV1,
    owner_local_ordinal: u32,
    payload: &'a T,
    relocations: &'a [SemanticImageProjectionKeyV1],
}

struct ExecutionImageHandoffBuilderV1 {
    rows: BTreeMap<SemanticImageProjectionKeyV1, Vec<[u8; 32]>>,
    dependency_rows: BTreeMap<SemanticImageProjectionKeyV1, u32>,
    relocations: BTreeMap<SemanticImageProjectionKeyV1, BTreeSet<SemanticImageProjectionKeyV1>>,
    next_ordinals: BTreeMap<(SemanticImageProjectionKeyV1, ExecutionImageRowDomainV1), u32>,
    entity_routes: Vec<ExecutionImageEntityRouteV1>,
}

impl ExecutionImageHandoffBuilderV1 {
    fn new() -> Self {
        Self {
            rows: BTreeMap::new(),
            dependency_rows: BTreeMap::new(),
            relocations: BTreeMap::new(),
            next_ordinals: BTreeMap::new(),
            entity_routes: Vec::new(),
        }
    }

    fn push<T: Serialize>(
        &mut self,
        projection: SemanticImageProjectionKeyV1,
        domain: ExecutionImageRowDomainV1,
        payload: &T,
        mut relocations: Vec<SemanticImageProjectionKeyV1>,
    ) -> Result<(), String> {
        relocations.sort();
        relocations.dedup();
        relocations.retain(|target| target != &projection);
        let ordinal = self
            .next_ordinals
            .entry((projection.clone(), domain))
            .or_default();
        let owner_local_ordinal = *ordinal;
        *ordinal = ordinal
            .checked_add(1)
            .ok_or_else(|| "execution image row ordinal overflow".to_owned())?;
        let digest = boon_contract::canonical_serde_hash_v1(
            EXECUTION_IMAGE_ROW_PAYLOAD_DOMAIN_V1,
            &ExecutionImageRowReceipt {
                domain,
                owner_local_ordinal,
                payload,
                relocations: &relocations,
            },
        )
        .map_err(|error| format!("failed to hash execution image row: {error}"))?;
        self.rows
            .entry(projection.clone())
            .or_default()
            .push(digest);
        if !relocations.is_empty() {
            let count = self.dependency_rows.entry(projection.clone()).or_default();
            *count = count
                .checked_add(1)
                .ok_or_else(|| "execution image dependency row count overflow".to_owned())?;
        }
        self.relocations
            .entry(projection)
            .or_default()
            .extend(relocations);
        Ok(())
    }

    fn route(
        &mut self,
        domain: ExecutionImageRowDomainV1,
        dense_index: usize,
        projection: SemanticImageProjectionKeyV1,
    ) -> Result<(), String> {
        self.entity_routes.push(ExecutionImageEntityRouteV1 {
            domain,
            dense_index: u32::try_from(dense_index)
                .map_err(|_| "execution image entity index exceeds u32".to_owned())?,
            projection,
        });
        Ok(())
    }

    fn finish(
        mut self,
        source_bundle_digest_v1: SourceBundleDigestV1,
        role: ProgramRole,
    ) -> Result<ExecutionImageHandoffV1, String> {
        let mut shards = Vec::with_capacity(self.rows.len());
        for (projection, rows) in self.rows {
            let row_count = u32::try_from(rows.len())
                .map_err(|_| "execution image shard row count exceeds u32".to_owned())?;
            let relocations = self
                .relocations
                .remove(&projection)
                .unwrap_or_default()
                .into_iter()
                .collect::<Vec<_>>();
            let dependency_row_count = self.dependency_rows.remove(&projection).unwrap_or(0);
            let local_content_digest = boon_contract::canonical_serde_hash_v1(
                EXECUTION_IMAGE_SHARD_DOMAIN_V1,
                &(projection.clone(), &rows, &relocations),
            )
            .map_err(|error| format!("failed to hash execution image shard: {error}"))?;
            shards.push(ExecutionImageShardReceiptV1 {
                projection,
                local_content_digest,
                row_count,
                dependency_row_count,
                relocations,
            });
        }
        if !self.relocations.is_empty() {
            return Err("execution image has relocations without local rows".to_owned());
        }
        if !self.dependency_rows.is_empty() {
            return Err("execution image has dependency counts without local rows".to_owned());
        }
        self.entity_routes
            .sort_by_key(|route| (route.domain, route.dense_index, route.projection.clone()));
        if self.entity_routes.windows(2).any(|pair| {
            (pair[0].domain, pair[0].dense_index) == (pair[1].domain, pair[1].dense_index)
        }) {
            return Err("execution image entity routes more than once".to_owned());
        }
        let local_image_digest = boon_contract::canonical_serde_hash_v1(
            EXECUTION_IMAGE_HANDOFF_DOMAIN_V1,
            &(
                EXECUTION_IMAGE_HANDOFF_SCHEMA_V1,
                source_bundle_digest_v1,
                role,
                &shards,
                &self.entity_routes,
            ),
        )
        .map_err(|error| format!("failed to hash execution image handoff: {error}"))?;
        Ok(ExecutionImageHandoffV1 {
            schema: EXECUTION_IMAGE_HANDOFF_SCHEMA_V1.to_owned(),
            source_bundle_digest_v1,
            role,
            shards,
            entity_routes: self.entity_routes,
            local_image_digest,
        })
    }
}

fn checked_projection(
    checked: &CheckedImageHandoffV1,
    domain: CheckedImageRowDomainV1,
    dense_index: usize,
) -> Result<SemanticImageProjectionKeyV1, String> {
    let dense_index = u32::try_from(dense_index)
        .map_err(|_| "checked image lookup index exceeds u32".to_owned())?;
    checked
        .entity_routes
        .binary_search_by_key(&(domain, dense_index), |route| {
            (route.domain, route.dense_index)
        })
        .ok()
        .map(|index| {
            SemanticImageProjectionKeyV1::checked(checked.entity_routes[index].projection.clone())
        })
        .ok_or_else(|| {
            format!("checked image has no {domain:?} route for dense index {dense_index}")
        })
}

fn call_instance_projections(
    checked: &CheckedImageHandoffV1,
    out: &ResolvedOutGraph,
) -> Result<Vec<SemanticImageProjectionKeyV1>, String> {
    let producer_roots = out
        .producer_roots()
        .iter()
        .map(|root| (root.call, root.spec.identity))
        .collect::<BTreeMap<_, _>>();
    let mut projections = Vec::with_capacity(out.call_instances.len());
    for instance in &out.call_instances {
        if instance.id.as_usize() != projections.len() {
            return Err(format!(
                "OUT call instance {} is not dense while sealing execution image",
                instance.id
            ));
        }
        let (root, mut call_path) = match instance.parent {
            Some(parent) => {
                let parent_projection =
                    projections.get(parent.as_usize()).cloned().ok_or_else(|| {
                        format!("OUT call {} has missing parent {parent}", instance.id)
                    })?;
                let SemanticImageProjectionKeyV1::Invocation {
                    root,
                    definition: _,
                    call_path,
                } = parent_projection
                else {
                    return Err(format!(
                        "OUT call {} parent does not have an invocation projection",
                        instance.id
                    ));
                };
                (root, call_path)
            }
            None => (
                producer_roots
                    .get(&instance.id)
                    .copied()
                    .map(DistributedCallOccurrenceRoot::Producer)
                    .unwrap_or(DistributedCallOccurrenceRoot::Program),
                Vec::new(),
            ),
        };
        if !producer_roots.contains_key(&instance.id) {
            let checked_call = instance.provenance.call_id.ok_or_else(|| {
                format!(
                    "non-producer OUT call {} has no checked call identity",
                    instance.id
                )
            })?;
            let projection = checked_projection(
                checked,
                CheckedImageRowDomainV1::Call,
                checked_call.0 as usize,
            )?;
            let SemanticImageProjectionKeyV1::Checked { projection } = projection else {
                unreachable!("checked route always yields a checked projection")
            };
            if !matches!(projection.region, CheckedShardRegionV1::Invocation { .. }) {
                return Err(format!(
                    "checked call {} does not route to an invocation shard",
                    checked_call.0
                ));
            }
            call_path.push(projection);
        }
        let callable_projection = checked_projection(
            checked,
            CheckedImageRowDomainV1::Callable,
            instance.provenance.callable.0 as usize,
        )?;
        let SemanticImageProjectionKeyV1::Checked {
            projection: callable_projection,
        } = callable_projection
        else {
            unreachable!("checked route always yields a checked projection")
        };
        projections.push(SemanticImageProjectionKeyV1::Invocation {
            root,
            definition: callable_projection.owner,
            call_path,
        });
    }
    Ok(projections)
}

fn route_for_frame(
    frame: Option<OutCallInstanceId>,
    fallback: SemanticImageProjectionKeyV1,
    invocation_projections: &[SemanticImageProjectionKeyV1],
) -> Result<SemanticImageProjectionKeyV1, String> {
    frame.map_or(Ok(fallback), |frame| {
        invocation_projections
            .get(frame.as_usize())
            .cloned()
            .ok_or_else(|| format!("execution row references missing invocation frame {frame}"))
    })
}

fn owner_projections(
    out: &ResolvedOutGraph,
    invocations: &[SemanticImageProjectionKeyV1],
) -> Result<BTreeMap<StaticOwnerId, SemanticImageProjectionKeyV1>, String> {
    let mut projections = BTreeMap::new();
    let mut attach =
        |owner: StaticOwnerId, projection: SemanticImageProjectionKeyV1, context: &str| {
            if let Some(previous) = projections.insert(owner, projection.clone())
                && previous != projection
            {
                return Err(format!(
                    "static owner {owner} has conflicting invocation projections at {context}"
                ));
            }
            Ok(())
        };
    for call in &out.call_instances {
        if let Some(owner) = call.owner {
            let projection = invocations
                .get(call.id.as_usize())
                .cloned()
                .ok_or_else(|| format!("static owner {owner} has missing call {}", call.id))?;
            attach(owner, projection, &format!("OUT call {}", call.id))?;
        }
    }
    for net in &out.nets {
        let Some(owner) = net.owner else {
            continue;
        };
        let anchor = net.owner_anchor.ok_or_else(|| {
            format!(
                "OUT net {} has static owner {owner} without an exact port anchor",
                net.id
            )
        })?;
        let port = out
            .ports
            .get(anchor.as_usize())
            .filter(|port| port.id == anchor)
            .ok_or_else(|| {
                format!(
                    "OUT net {} owner anchor references missing port {anchor}",
                    net.id
                )
            })?;
        let projection = invocations
            .get(port.call.as_usize())
            .cloned()
            .ok_or_else(|| format!("static owner {owner} has missing call {}", port.call))?;
        attach(owner, projection, &format!("OUT net {}", net.id))?;
    }
    drop(attach);
    for owner in &out.static_owners {
        if !projections.contains_key(&owner.id) {
            return Err(format!(
                "static owner {} has no exact call/net invocation projection",
                owner.id
            ));
        }
    }
    Ok(projections)
}

fn route_for_expression(
    checked: &CheckedImageHandoffV1,
    execution: &SemanticExecutionImageColumnsV1,
    invocations: &[SemanticImageProjectionKeyV1],
    owner_projections: &BTreeMap<StaticOwnerId, SemanticImageProjectionKeyV1>,
    expression: SemanticExprId,
) -> Result<SemanticImageProjectionKeyV1, String> {
    let definition = execution
        .expressions
        .get(expression.as_usize())
        .filter(|candidate| candidate.id == expression)
        .ok_or_else(|| format!("execution image references missing expression {expression}"))?;
    let origin = execution
        .checked_expression_origins
        .get(expression.as_usize())
        .filter(|candidate| candidate.expression == expression)
        .ok_or_else(|| format!("execution expression {expression} has no exact origin"))?;
    let fallback = checked_projection(
        checked,
        CheckedImageRowDomainV1::Expression,
        definition.checked_expr_id.0 as usize,
    )?;
    let static_projection = definition
        .owner
        .map(|owner| {
            owner_projections.get(&owner).cloned().ok_or_else(|| {
                format!("execution expression {expression} has unanchored static owner {owner}")
            })
        })
        .transpose()?;
    let frame_projection = origin
        .call_instance
        .map(|frame| {
            invocations.get(frame.as_usize()).cloned().ok_or_else(|| {
                format!("execution expression {expression} has missing invocation frame {frame}")
            })
        })
        .transpose()?;
    match (static_projection, frame_projection) {
        (
            Some(
                static_projection @ SemanticImageProjectionKeyV1::Invocation {
                    root: static_root,
                    ..
                },
            ),
            Some(SemanticImageProjectionKeyV1::Invocation {
                root: frame_root, ..
            }),
        ) => {
            if static_root != frame_root {
                return Err(format!(
                    "execution expression {expression} crosses static {static_root:?} and invocation {frame_root:?} roots"
                ));
            }
            // Static ownership is the exact retained-runtime occurrence. A
            // checked expression origin may sit in an ancestor invocation of
            // the same program/producer root, so the static projection owns
            // the row while the frame remains provenance.
            Ok(static_projection)
        }
        (Some(projection), None) | (None, Some(projection)) => Ok(projection),
        (None, None) => Ok(fallback),
        (Some(_), Some(_)) => Err(format!(
            "execution expression {expression} has non-invocation concrete ownership"
        )),
    }
}

fn function_projection(
    checked: &CheckedImageHandoffV1,
    execution: &SemanticExecutionImageColumnsV1,
    function: &SemanticFunction,
) -> Result<SemanticImageProjectionKeyV1, String> {
    let callable = execution
        .callables
        .get(function.callable.as_usize())
        .filter(|candidate| candidate.id == function.callable)
        .ok_or_else(|| {
            format!(
                "producer function `{}` references missing callable {}",
                function.name, function.callable
            )
        })?;
    let callable_projection = checked_projection(
        checked,
        CheckedImageRowDomainV1::Callable,
        callable.checked_callable.0 as usize,
    )?;
    let SemanticImageProjectionKeyV1::Checked {
        projection: callable_projection,
    } = callable_projection
    else {
        unreachable!("checked route always yields a checked projection")
    };
    Ok(SemanticImageProjectionKeyV1::Invocation {
        root: DistributedCallOccurrenceRoot::Producer(function.identity),
        definition: callable_projection.owner,
        call_path: Vec::new(),
    })
}

fn execution_image_handoff(
    checked: &CheckedImageHandoffV1,
    out: &ResolvedOutGraph,
    execution: &SemanticExecutionImageColumnsV1,
) -> Result<ExecutionImageHandoffV1, String> {
    let invocations = call_instance_projections(checked, out)?;
    let owner_projections = owner_projections(out, &invocations)?;
    let mut builder = ExecutionImageHandoffBuilderV1::new();

    let expression_routes = execution
        .expressions
        .iter()
        .map(|expression| {
            route_for_expression(
                checked,
                execution,
                &invocations,
                &owner_projections,
                expression.id,
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    let expression_projection = |id: SemanticExprId| {
        expression_routes
            .get(id.as_usize())
            .cloned()
            .ok_or_else(|| format!("execution image references missing expression {id}"))
    };
    let statement_routes = execution
        .statements
        .iter()
        .map(|statement| {
            let checked_statement = match statement.origin {
                crate::SemanticStatementOrigin::Checked { statement } => statement,
                crate::SemanticStatementOrigin::ProducerResult {
                    checked_statement, ..
                } => checked_statement,
            };
            let fallback = checked_projection(
                checked,
                CheckedImageRowDomainV1::Statement,
                checked_statement.0 as usize,
            )?;
            route_for_frame(statement.call_instance, fallback, &invocations)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let statement_projection = |id: SemanticStatementId| {
        execution
            .statements
            .get(id.as_usize())
            .filter(|statement| statement.id == id)
            .and_then(|_| statement_routes.get(id.as_usize()))
            .cloned()
            .ok_or_else(|| format!("execution image references missing statement {id}"))
    };

    for scope in &execution.scopes {
        let projection = checked_projection(
            checked,
            CheckedImageRowDomainV1::Scope,
            scope.checked_scope.0 as usize,
        )?;
        builder.push(
            projection.clone(),
            ExecutionImageRowDomainV1::Scope,
            scope,
            Vec::new(),
        )?;
        builder.route(
            ExecutionImageRowDomainV1::Scope,
            scope.id.as_usize(),
            projection,
        )?;
    }
    for expression in &execution.expressions {
        let projection = expression_projection(expression.id)?;
        let mut relocations = execution
            .expression_children(&expression.kind)
            .ok_or_else(|| {
                format!(
                    "execution expression {} has a missing materialization child",
                    expression.id
                )
            })?
            .into_iter()
            .map(expression_projection)
            .collect::<Result<Vec<_>, _>>()?;
        if let crate::SemanticExpressionKind::Call { callable, .. } = expression.kind {
            let callable = execution
                .callables
                .get(callable.as_usize())
                .filter(|candidate| candidate.id == callable)
                .ok_or_else(|| {
                    format!(
                        "expression {} has missing callable {callable}",
                        expression.id
                    )
                })?;
            relocations.push(checked_projection(
                checked,
                CheckedImageRowDomainV1::Callable,
                callable.checked_callable.0 as usize,
            )?);
        }
        builder.push(
            projection.clone(),
            ExecutionImageRowDomainV1::Expression,
            expression,
            relocations,
        )?;
        builder.route(
            ExecutionImageRowDomainV1::Expression,
            expression.id.as_usize(),
            projection.clone(),
        )?;
        let origin = execution
            .checked_expression_origins
            .get(expression.id.as_usize())
            .filter(|origin| origin.expression == expression.id)
            .ok_or_else(|| format!("expression {} has no exact origin", expression.id))?;
        let mut relocations = origin
            .owning_statement
            .map(statement_projection)
            .transpose()?
            .into_iter()
            .collect::<Vec<_>>();
        if let Some(frame) = origin.call_instance {
            relocations.push(invocations.get(frame.as_usize()).cloned().ok_or_else(|| {
                format!(
                    "expression origin {} references missing invocation frame {frame}",
                    origin.expression
                )
            })?);
        }
        builder.push(
            projection.clone(),
            ExecutionImageRowDomainV1::ExpressionOrigin,
            origin,
            relocations,
        )?;
        builder.route(
            ExecutionImageRowDomainV1::ExpressionOrigin,
            origin.expression.as_usize(),
            projection,
        )?;
    }
    for statement in &execution.statements {
        let projection = statement_projection(statement.id)?;
        let mut relocations = statement
            .value
            .into_iter()
            .chain(statement.children.iter().filter_map(|child| {
                execution
                    .statements
                    .get(child.as_usize())
                    .and_then(|statement| statement.value)
            }))
            .map(expression_projection)
            .collect::<Result<Vec<_>, _>>()?;
        if let Some(parent) = statement.parent
            && let Some(parent) = execution.statements.get(parent.as_usize())
            && let Some(value) = parent.value
        {
            relocations.push(expression_projection(value)?);
        }
        builder.push(
            projection.clone(),
            ExecutionImageRowDomainV1::Statement,
            statement,
            relocations,
        )?;
        builder.route(
            ExecutionImageRowDomainV1::Statement,
            statement.id.as_usize(),
            projection,
        )?;
    }
    for callable in &execution.callables {
        let projection = checked_projection(
            checked,
            CheckedImageRowDomainV1::Callable,
            callable.checked_callable.0 as usize,
        )?;
        let relocations = callable
            .semantic_root
            .map(expression_projection)
            .transpose()?
            .into_iter()
            .collect();
        builder.push(
            projection.clone(),
            ExecutionImageRowDomainV1::Callable,
            callable,
            relocations,
        )?;
        builder.route(
            ExecutionImageRowDomainV1::Callable,
            callable.id.as_usize(),
            projection,
        )?;
    }
    for call in &execution.calls {
        let projection = checked_projection(
            checked,
            CheckedImageRowDomainV1::Call,
            call.checked_call.0 as usize,
        )?;
        let callable = execution
            .callables
            .get(call.callable.as_usize())
            .filter(|candidate| candidate.id == call.callable)
            .ok_or_else(|| {
                format!(
                    "execution call {} has missing callable {}",
                    call.id, call.callable
                )
            })?;
        let relocations = vec![checked_projection(
            checked,
            CheckedImageRowDomainV1::Callable,
            callable.checked_callable.0 as usize,
        )?];
        builder.push(
            projection.clone(),
            ExecutionImageRowDomainV1::Call,
            call,
            relocations,
        )?;
        builder.route(
            ExecutionImageRowDomainV1::Call,
            call.id.as_usize(),
            projection,
        )?;
    }
    for occurrence in &execution.call_occurrences {
        let projection = invocations
            .get(occurrence.id.as_usize())
            .cloned()
            .ok_or_else(|| {
                format!(
                    "call occurrence {} has no invocation projection",
                    occurrence.id
                )
            })?;
        let mut relocations = occurrence
            .parent
            .and_then(|parent| invocations.get(parent.as_usize()).cloned())
            .into_iter()
            .collect::<Vec<_>>();
        if let Some(call) = occurrence.call {
            let call = execution
                .calls
                .get(call.as_usize())
                .filter(|candidate| candidate.id == call)
                .ok_or_else(|| {
                    format!("call occurrence {} has missing call {call}", occurrence.id)
                })?;
            relocations.push(checked_projection(
                checked,
                CheckedImageRowDomainV1::Call,
                call.checked_call.0 as usize,
            )?);
        }
        builder.push(
            projection.clone(),
            ExecutionImageRowDomainV1::CallOccurrence,
            occurrence,
            relocations,
        )?;
        builder.route(
            ExecutionImageRowDomainV1::CallOccurrence,
            occurrence.id.as_usize(),
            projection,
        )?;
    }
    for source in &execution.sources {
        let projection = route_for_frame(
            source.call_instance,
            expression_projection(source.expression)?,
            &invocations,
        )?;
        let relocations = vec![expression_projection(source.expression)?];
        builder.push(
            projection.clone(),
            ExecutionImageRowDomainV1::Source,
            source,
            relocations,
        )?;
        builder.route(
            ExecutionImageRowDomainV1::Source,
            source.id.as_usize(),
            projection,
        )?;
    }
    for state in &execution.states {
        let projection = route_for_frame(
            state.call_instance,
            expression_projection(state.expression)?,
            &invocations,
        )?;
        let mut relocations = vec![
            expression_projection(state.expression)?,
            expression_projection(state.initial)?,
        ];
        if let crate::SemanticStateLifetimeV1::ActivationLocal { then_expression } = state.lifetime
        {
            relocations.push(expression_projection(then_expression)?);
        }
        builder.push(
            projection.clone(),
            ExecutionImageRowDomainV1::State,
            state,
            relocations,
        )?;
        builder.route(
            ExecutionImageRowDomainV1::State,
            state.id.as_usize(),
            projection,
        )?;
    }
    for root in &execution.roots {
        let projection = expression_projection(root.expression)?;
        builder.push(
            projection.clone(),
            ExecutionImageRowDomainV1::Root,
            root,
            vec![expression_projection(root.expression)?],
        )?;
        builder.route(ExecutionImageRowDomainV1::Root, root.ordinal, projection)?;
    }
    for (index, function) in execution.functions.iter().enumerate() {
        let projection = function_projection(checked, execution, function)?;
        let mut relocations = vec![expression_projection(function.root)?];
        if let Some(source) = function.invocation_source {
            relocations.push(expression_projection(source)?);
        }
        builder.push(
            projection.clone(),
            ExecutionImageRowDomainV1::Function,
            function,
            relocations,
        )?;
        builder.route(ExecutionImageRowDomainV1::Function, index, projection)?;
    }
    for materialization in &execution.materializations {
        let projection = owner_projections
            .get(&materialization.owner)
            .cloned()
            .unwrap_or(expression_projection(materialization.source)?);
        let relocations = materialization
            .expression_roots()
            .into_iter()
            .map(expression_projection)
            .collect::<Result<Vec<_>, _>>()?;
        builder.push(
            projection.clone(),
            ExecutionImageRowDomainV1::Materialization,
            materialization,
            relocations,
        )?;
        builder.route(
            ExecutionImageRowDomainV1::Materialization,
            materialization.id.as_usize(),
            projection,
        )?;
    }
    for owner in &execution.static_owners {
        let projection = owner_projections
            .get(&owner.id)
            .cloned()
            .ok_or_else(|| format!("static owner {} has no invocation projection", owner.id))?;
        let relocations = owner
            .parent
            .and_then(|parent| owner_projections.get(&parent).cloned())
            .into_iter()
            .collect();
        builder.push(
            projection.clone(),
            ExecutionImageRowDomainV1::StaticOwner,
            owner,
            relocations,
        )?;
        builder.route(
            ExecutionImageRowDomainV1::StaticOwner,
            owner.id.as_usize(),
            projection,
        )?;
    }

    builder.finish(checked.source_bundle_digest_v1, checked.role)
}

fn semantic_image_seal_digest(
    schema: &str,
    checked: &CheckedImageHandoffV1,
    execution: &ExecutionImageHandoffV1,
) -> Result<[u8; 32], String> {
    boon_contract::canonical_serde_hash_v1(
        SEMANTIC_IMAGE_SEAL_DOMAIN_V1,
        &(
            schema,
            checked.local_image_digest,
            execution.local_image_digest,
        ),
    )
    .map_err(|error| format!("failed to hash semantic image seal: {error}"))
}
