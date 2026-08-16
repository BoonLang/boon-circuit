//! Canonical checked/execution image ownership.
//!
//! Contextual expansion constructs the execution columns in `ExecutionPending`.
//! Before publication, the builder normalizes any checked inline-list
//! authorities that must become concrete execution rows. Resource elaboration
//! then receives immutable columns and owns row/list bindings separately. The
//! builder crosses one consuming validation boundary and seals the checked and
//! execution receipts beside the columns. Final executable payloads are hashed
//! by core lowering while each typed row is still construction-owned; this
//! linker hashes only semantic-owned rows and receipt/relocation envelopes.
//! Later semantic phases borrow the sealed columns; they never own or
//! materialize a second execution graph.

use crate::{
    DistributedCallOccurrenceRoot, OutCallInstanceId, ResolvedOutGraph,
    SemanticExecutionImageColumnsV1, SemanticExprId, SemanticFunction, SemanticStatementId,
    StaticOwnerId,
};
use boon_checked::{
    CHECKED_IMAGE_HANDOFF_SCHEMA_V4, CheckedImageHandoffV4, CheckedImageProjectionIdV2,
    CheckedImageRowDomainV2, CheckedShardRegionV2, ProgramRole,
};
use boon_contract::SourceBundleDigestV1;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::marker::PhantomData;

pub const SEMANTIC_IMAGE_SCHEMA_V3: &str = "boon.semantic-image.v3";
#[cfg(test)]
pub const EXECUTION_IMAGE_HANDOFF_SCHEMA_V2: &str = "boon.execution-image-handoff.v2";
pub const EXECUTION_IMAGE_HANDOFF_SCHEMA_V3: &str = "boon.execution-image-handoff.v3";
pub const EXECUTION_CONSTRUCTION_ROUTES_SCHEMA_V3: &str = "boon.execution-construction-routes.v3";

// The parent/call-site path is a stable logical identity shared by the V2
// oracle and V3 overlays; changing the container must not re-key that identity.
const EXECUTION_INVOCATION_PATH_DOMAIN_V2: &[u8] = b"boon.execution-invocation-path.v2\0";
#[cfg(test)]
const EXECUTION_IMAGE_PROJECTION_KEY_DOMAIN_V2: &[u8] = b"boon.execution-image-projection-key.v2\0";
#[cfg(test)]
const EXECUTION_IMAGE_ROW_PAYLOAD_DOMAIN_V2: &[u8] = b"boon.execution-image-row-payload.v2\0";
#[cfg(test)]
const EXECUTION_IMAGE_ROW_DOMAIN_V2: &[u8] = b"boon.execution-image-row.v2\0";
#[cfg(test)]
const EXECUTION_IMAGE_SHARD_DOMAIN_V2: &[u8] = b"boon.execution-image-shard.v2\0";
#[cfg(test)]
const EXECUTION_IMAGE_HANDOFF_DOMAIN_V2: &[u8] = b"boon.execution-image-handoff.v2\0";
const EXECUTION_IMAGE_PROJECTION_KEY_DOMAIN_V3: &[u8] = b"boon.execution-image-projection-key.v3\0";
const EXECUTION_IMAGE_ROW_PAYLOAD_DOMAIN_V3: &[u8] = b"boon.execution-image-row-payload.v3\0";
const EXECUTION_IMAGE_ROW_DOMAIN_V3: &[u8] = b"boon.execution-image-row.v3\0";
const EXECUTION_IMAGE_SHARD_DOMAIN_V3: &[u8] = b"boon.execution-image-shard.v3\0";
const EXECUTION_IMAGE_HANDOFF_DOMAIN_V3: &[u8] = b"boon.execution-image-handoff.v3\0";
const SEMANTIC_IMAGE_SEAL_DOMAIN_V3: &[u8] = b"boon.semantic-image-seal.v3\0";
const EXECUTION_INVOCATION_OVERLAY_DOMAIN_V3: &[u8] = b"boon.execution-invocation-overlay.v3\0";
#[cfg(test)]
const EXECUTION_CONSTRUCTION_ROUTES_DOMAIN_V3: &[u8] = b"boon.execution-construction-routes.v3\0";

/// Test-only payload-seal surface for the independent post-hoc handoff oracle.
#[cfg(test)]
#[derive(Debug, Eq, PartialEq)]
pub(crate) struct ExecutionRowPayloadSealsV3 {
    pub(crate) expressions: Box<[[u8; 32]]>,
    pub(crate) statements: Box<[[u8; 32]]>,
    pub(crate) call_occurrences: Box<[[u8; 32]]>,
    pub(crate) sources: Box<[[u8; 32]]>,
    pub(crate) states: Box<[[u8; 32]]>,
    pub(crate) roots: Box<[[u8; 32]]>,
    pub(crate) functions: Box<[[u8; 32]]>,
    pub(crate) materializations: Box<[[u8; 32]]>,
    pub(crate) static_owners: Box<[[u8; 32]]>,
}

pub(crate) fn seal_execution_row_payload_v3<T: Serialize + ?Sized>(
    payload: &T,
    scratch: &mut Vec<u8>,
) -> Result<[u8; 32], String> {
    boon_contract::canonical_serde_hash_v1_with_buffer(
        EXECUTION_IMAGE_ROW_PAYLOAD_DOMAIN_V3,
        payload,
        scratch,
    )
    .map_err(|error| format!("failed to seal execution V3 row payload: {error}"))
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionImageRowDomainV2 {
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

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionImageRowDomainV3 {
    Scope,
    Expression,
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

#[cfg(test)]
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct ExecutionInvocationPathIdV2(pub u32);

#[cfg(test)]
impl ExecutionInvocationPathIdV2 {
    pub const fn as_usize(self) -> usize {
        self.0 as usize
    }
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct ExecutionImageProjectionIdV2(pub u32);

#[cfg(test)]
impl ExecutionImageProjectionIdV2 {
    pub const fn as_usize(self) -> usize {
        self.0 as usize
    }
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ExecutionImageRelocationSpanV2 {
    pub start: u32,
    pub len: u32,
}

#[cfg(test)]
impl ExecutionImageRelocationSpanV2 {
    pub fn checked_range(self) -> Option<std::ops::Range<usize>> {
        let end = self.start.checked_add(self.len)?;
        Some(self.start as usize..end as usize)
    }
}

/// Collision-checked parent-pointer invocation path. Cumulative logical depth
/// never becomes an owned vector in a row or projection key.
#[cfg(test)]
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ExecutionInvocationPathNodeV2 {
    pub parent: Option<ExecutionInvocationPathIdV2>,
    pub call_site: CheckedImageProjectionIdV2,
    pub stable_path_digest: [u8; 32],
}

/// Snapshot-local projection identity. Its separate stable-key digest commits
/// checked stable identities and invocation-path digests, never these dense IDs.
#[cfg(test)]
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SemanticImageProjectionIdentityV2 {
    Checked {
        projection: CheckedImageProjectionIdV2,
    },
    Invocation {
        root: DistributedCallOccurrenceRoot,
        definition: CheckedImageProjectionIdV2,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        call_path: Option<ExecutionInvocationPathIdV2>,
    },
}

#[cfg(test)]
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ExecutionImageProjectionV2 {
    pub identity: SemanticImageProjectionIdentityV2,
    pub stable_key_digest: [u8; 32],
    pub local_content_digest: [u8; 32],
    pub row_count: u32,
    pub dependency_row_count: u32,
    pub relocation_span: ExecutionImageRelocationSpanV2,
}

#[cfg(test)]
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ExecutionImageEntityRouteV2 {
    pub domain: ExecutionImageRowDomainV2,
    pub dense_index: u32,
    pub projection: ExecutionImageProjectionIdV2,
}

/// One concrete invocation overlay prepared before semantic row construction.
/// Stable checked definitions remain owned by the checked image; this record
/// carries only occurrence ancestry and the exact definition route needed by
/// execution-owned rows.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ExecutionInvocationOverlayV3 {
    pub occurrence: OutCallInstanceId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<OutCallInstanceId>,
    pub root: DistributedCallOccurrenceRoot,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authored_call_site: Option<CheckedImageProjectionIdV2>,
    pub definition: CheckedImageProjectionIdV2,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stable_path_digest: Option<[u8; 32]>,
    pub stable_key_digest: [u8; 32],
}

/// Snapshot-local construction owner. Stable identity is always obtained from
/// the referenced checked projection or invocation overlay receipt; these
/// dense IDs never cross revisions on their own.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ExecutionConstructionProjectionV3 {
    Checked {
        projection: CheckedImageProjectionIdV2,
    },
    Invocation {
        occurrence: OutCallInstanceId,
    },
    Producer {
        identity: [u8; 32],
        definition: CheckedImageProjectionIdV2,
    },
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct ExecutionImageProjectionIdV3(pub u32);

impl ExecutionImageProjectionIdV3 {
    pub const fn as_usize(self) -> usize {
        self.0 as usize
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ExecutionImageRelocationSpanV3 {
    pub start: u32,
    pub len: u32,
}

impl ExecutionImageRelocationSpanV3 {
    pub fn checked_range(self) -> Option<std::ops::Range<usize>> {
        let end = self.start.checked_add(self.len)?;
        Some(self.start as usize..end as usize)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ExecutionImageProjectionV3 {
    pub identity: ExecutionConstructionProjectionV3,
    pub stable_key_digest: [u8; 32],
    pub local_content_digest: [u8; 32],
    pub row_count: u32,
    pub dependency_row_count: u32,
    pub relocation_span: ExecutionImageRelocationSpanV3,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ExecutionImageEntityRouteV3 {
    pub domain: ExecutionImageRowDomainV3,
    pub dense_index: u32,
    pub projection: ExecutionImageProjectionIdV3,
}

/// Compact executable receipt set. Invocation ancestry is owned by the V3
/// parent-linked overlays; it is never expanded into a second path arena.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ExecutionImageHandoffV3 {
    pub schema: String,
    pub source_bundle_digest_v1: SourceBundleDigestV1,
    pub role: ProgramRole,
    pub invocation_overlays: Vec<ExecutionInvocationOverlayV3>,
    pub projections: Vec<ExecutionImageProjectionV3>,
    pub relocations: Vec<ExecutionImageProjectionIdV3>,
    pub entity_routes: Vec<ExecutionImageEntityRouteV3>,
    pub local_image_digest: [u8; 32],
}

impl ExecutionImageHandoffV3 {
    pub fn projection(
        &self,
        id: ExecutionImageProjectionIdV3,
    ) -> Option<&ExecutionImageProjectionV3> {
        self.projections.get(id.as_usize())
    }

    pub fn projection_relocations(
        &self,
        id: ExecutionImageProjectionIdV3,
    ) -> Option<&[ExecutionImageProjectionIdV3]> {
        let projection = self.projection(id)?;
        self.relocations
            .get(projection.relocation_span.checked_range()?)
    }

    pub fn entity_projection(
        &self,
        domain: ExecutionImageRowDomainV3,
        dense_index: usize,
    ) -> Option<ExecutionImageProjectionIdV3> {
        let dense_index = u32::try_from(dense_index).ok()?;
        self.entity_routes
            .binary_search_by_key(&(domain, dense_index), |route| {
                (route.domain, route.dense_index)
            })
            .ok()
            .map(|index| self.entity_routes[index].projection)
    }

    pub fn invocation(
        &self,
        occurrence: OutCallInstanceId,
    ) -> Option<&ExecutionInvocationOverlayV3> {
        self.invocation_overlays
            .get(occurrence.as_usize())
            .filter(|overlay| overlay.occurrence == occurrence)
    }
}

#[derive(Serialize)]
struct ExecutionInvocationOverlayFingerprintV3 {
    root: DistributedCallOccurrenceRoot,
    definition_digest: [u8; 32],
    path_digest: Option<[u8; 32]>,
}

/// Construction-time definition and invocation routes. This table is built
/// before execution rows and is deliberately discarded after the transitional
/// V2 handoff seals. It becomes the route spine for direct V3 row publication;
/// it is not retained beside the final image as a duplicate owner.
pub(crate) struct ExecutionConstructionRoutesV3 {
    definition_by_checked_projection: Vec<CheckedImageProjectionIdV2>,
    invocations: Vec<ExecutionInvocationOverlayV3>,
    owner_occurrences: Vec<OutCallInstanceId>,
    #[cfg(test)]
    local_digest: [u8; 32],
}

pub(crate) struct ExecutionConstructionImageV3 {
    routes: ExecutionConstructionRoutesV3,
    expression_routes: Vec<ExecutionConstructionProjectionV3>,
    statement_routes: Vec<ExecutionConstructionProjectionV3>,
}

impl ExecutionConstructionRoutesV3 {
    fn definition_projection(
        &self,
        projection: CheckedImageProjectionIdV2,
    ) -> Result<CheckedImageProjectionIdV2, String> {
        self.definition_by_checked_projection
            .get(projection.as_usize())
            .copied()
            .ok_or_else(|| {
                format!(
                    "execution construction routes have no definition for checked projection {}",
                    projection.0
                )
            })
    }

    fn invocation(
        &self,
        occurrence: OutCallInstanceId,
    ) -> Result<&ExecutionInvocationOverlayV3, String> {
        self.invocations
            .get(occurrence.as_usize())
            .filter(|overlay| overlay.occurrence == occurrence)
            .ok_or_else(|| {
                format!("execution construction routes have no dense invocation {occurrence}")
            })
    }
}

impl ExecutionConstructionImageV3 {
    fn definition_projection(
        &self,
        projection: CheckedImageProjectionIdV2,
    ) -> Result<CheckedImageProjectionIdV2, String> {
        self.routes.definition_projection(projection)
    }

    fn invocation(
        &self,
        occurrence: OutCallInstanceId,
    ) -> Result<&ExecutionInvocationOverlayV3, String> {
        self.routes.invocation(occurrence)
    }

    fn owner_occurrence(&self, owner: StaticOwnerId) -> Result<OutCallInstanceId, String> {
        self.routes
            .owner_occurrences
            .get(owner.as_usize())
            .copied()
            .ok_or_else(|| format!("execution construction image has no static owner {owner}"))
    }

    fn expression_route(
        &self,
        expression: SemanticExprId,
    ) -> Result<ExecutionConstructionProjectionV3, String> {
        self.expression_routes
            .get(expression.as_usize())
            .copied()
            .ok_or_else(|| format!("execution construction image has no expression {expression}"))
    }

    fn statement_route(
        &self,
        statement: SemanticStatementId,
    ) -> Result<ExecutionConstructionProjectionV3, String> {
        self.statement_routes
            .get(statement.as_usize())
            .copied()
            .ok_or_else(|| format!("execution construction image has no statement {statement}"))
    }
}

#[cfg(test)]
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ExecutionImageHandoffV2 {
    pub schema: String,
    pub source_bundle_digest_v1: SourceBundleDigestV1,
    pub role: ProgramRole,
    pub invocation_paths: Vec<ExecutionInvocationPathNodeV2>,
    pub projections: Vec<ExecutionImageProjectionV2>,
    pub relocations: Vec<ExecutionImageProjectionIdV2>,
    pub entity_routes: Vec<ExecutionImageEntityRouteV2>,
    pub local_image_digest: [u8; 32],
}

#[cfg(test)]
impl ExecutionImageHandoffV2 {
    pub fn projection(
        &self,
        id: ExecutionImageProjectionIdV2,
    ) -> Option<&ExecutionImageProjectionV2> {
        self.projections.get(id.as_usize())
    }

    pub fn projection_relocations(
        &self,
        id: ExecutionImageProjectionIdV2,
    ) -> Option<&[ExecutionImageProjectionIdV2]> {
        let projection = self.projection(id)?;
        self.relocations
            .get(projection.relocation_span.checked_range()?)
    }

    pub fn entity_projection(
        &self,
        domain: ExecutionImageRowDomainV2,
        dense_index: usize,
    ) -> Option<ExecutionImageProjectionIdV2> {
        let dense_index = u32::try_from(dense_index).ok()?;
        self.entity_routes
            .binary_search_by_key(&(domain, dense_index), |route| {
                (route.domain, route.dense_index)
            })
            .ok()
            .map(|index| self.entity_routes[index].projection)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SealedSemanticImageV3 {
    schema: String,
    checked_handoff: CheckedImageHandoffV4,
    execution_handoff: ExecutionImageHandoffV3,
    execution: SemanticExecutionImageColumnsV1,
    seal_digest: [u8; 32],
}

impl SealedSemanticImageV3 {
    pub const fn checked_handoff(&self) -> &CheckedImageHandoffV4 {
        &self.checked_handoff
    }

    pub const fn execution_handoff(&self) -> &ExecutionImageHandoffV3 {
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
        if self.schema != SEMANTIC_IMAGE_SCHEMA_V3
            || self.checked_handoff.schema != CHECKED_IMAGE_HANDOFF_SCHEMA_V4
            || self.execution_handoff.schema != EXECUTION_IMAGE_HANDOFF_SCHEMA_V3
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
struct PostResourceValidatedV2;

pub(crate) struct SemanticImageBuilder<State> {
    checked_handoff: CheckedImageHandoffV4,
    execution_routes_v3: Option<ExecutionConstructionRoutesV3>,
    execution_image_v3: Option<ExecutionConstructionImageV3>,
    execution: SemanticExecutionImageColumnsV1,
    execution_handoff: Option<ExecutionImageHandoffV3>,
    #[cfg(test)]
    execution_handoff_v2_oracle: Option<ExecutionImageHandoffV2>,
    state: PhantomData<State>,
}

impl SemanticImageBuilder<ExecutionPending> {
    pub(crate) fn execution_pending(
        checked_handoff: CheckedImageHandoffV4,
        execution_routes_v3: ExecutionConstructionRoutesV3,
        execution: SemanticExecutionImageColumnsV1,
    ) -> Result<Self, String> {
        if checked_handoff.schema != CHECKED_IMAGE_HANDOFF_SCHEMA_V4 {
            return Err(format!(
                "unsupported checked image handoff schema `{}`",
                checked_handoff.schema
            ));
        }
        Ok(Self {
            checked_handoff,
            execution_routes_v3: Some(execution_routes_v3),
            execution_image_v3: None,
            execution,
            execution_handoff: None,
            #[cfg(test)]
            execution_handoff_v2_oracle: None,
            state: PhantomData,
        })
    }

    pub(crate) const fn execution(&self) -> &SemanticExecutionImageColumnsV1 {
        &self.execution
    }

    /// Normalize execution-owned list authority rows before the immutable
    /// execution/resource boundary. The returned targets are the exact input
    /// to pure resource-table construction; no later phase receives mutable
    /// execution columns.
    pub(crate) fn normalize_resource_authorities(
        &mut self,
        checked: &boon_checked::CheckedProgramFields,
    ) -> Result<crate::resource::PreparedSemanticResourceInputs, String> {
        let prepared =
            crate::resource::prepare_semantic_resource_inputs(checked, &mut self.execution)?;
        let routes = self
            .execution_routes_v3
            .take()
            .ok_or_else(|| "execution V3 routes were already consumed".to_owned())?;
        self.execution_image_v3 = Some(execution_construction_image_v3(
            &self.checked_handoff,
            routes,
            &self.execution,
        )?);
        Ok(prepared)
    }

    pub(crate) fn finalize_execution(
        self,
        out: &ResolvedOutGraph,
    ) -> Result<SemanticImageBuilder<ExecutionFinalized>, String> {
        self.execution.validate(out)?;
        let witness = PostResourceValidatedV2;
        self.finish_execution(witness, out)
    }

    fn finish_execution(
        self,
        _witness: PostResourceValidatedV2,
        _out: &ResolvedOutGraph,
    ) -> Result<SemanticImageBuilder<ExecutionFinalized>, String> {
        if self.execution_routes_v3.is_some() {
            return Err("execution V3 routes were not bound after normalization".to_owned());
        }
        let execution_image_v3 = self
            .execution_image_v3
            .ok_or_else(|| "execution V3 image was not constructed".to_owned())?;
        #[cfg(test)]
        let execution_handoff_v2_oracle = Some(execution_image_handoff_v2_oracle(
            &self.checked_handoff,
            &execution_image_v3,
            _out,
            &self.execution,
        )?);
        Ok(SemanticImageBuilder {
            checked_handoff: self.checked_handoff,
            execution_routes_v3: None,
            execution_image_v3: Some(execution_image_v3),
            execution: self.execution,
            execution_handoff: None,
            #[cfg(test)]
            execution_handoff_v2_oracle,
            state: PhantomData,
        })
    }
}

impl SemanticImageBuilder<ExecutionFinalized> {
    pub(crate) const fn execution(&self) -> &SemanticExecutionImageColumnsV1 {
        &self.execution
    }

    pub(crate) const fn checked_handoff(&self) -> &CheckedImageHandoffV4 {
        &self.checked_handoff
    }

    pub(crate) fn execution_handoff(&self) -> &ExecutionImageHandoffV3 {
        self.execution_handoff
            .as_ref()
            .expect("executable receipts are finalized before handoff access")
    }

    pub(crate) fn execution_receipt_publisher(
        &self,
    ) -> Result<ExecutionReceiptPublisherV3<'_>, String> {
        let construction_image = self
            .execution_image_v3
            .as_ref()
            .ok_or_else(|| "finalized execution builder has no V3 construction image".to_owned())?;
        ExecutionReceiptPublisherV3::new(&self.checked_handoff, construction_image, &self.execution)
    }

    pub(crate) fn install_execution_handoff(
        &mut self,
        handoff: ExecutionImageHandoffV3,
    ) -> Result<(), String> {
        if self.execution_handoff.replace(handoff).is_some() {
            return Err("execution V3 handoff was already finalized".to_owned());
        }
        #[cfg(test)]
        if let Some(oracle) = &self.execution_handoff_v2_oracle {
            validate_v3_routes_against_v2_oracle(
                self.execution_handoff
                    .as_ref()
                    .expect("installed execution handoff exists"),
                oracle,
                &self.checked_handoff,
            )?;
        }
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn validate_direct_execution_handoff(
        &self,
        core: &crate::program_core::CanonicalProgramCoreV2,
        direct: &ExecutionImageHandoffV3,
    ) -> Result<(), String> {
        let construction_image = self
            .execution_image_v3
            .as_ref()
            .ok_or_else(|| "finalized execution builder has no V3 construction image".to_owned())?;
        let payload_seals = execution_row_payload_seals_v3_oracle(core)?;
        let oracle = execution_image_handoff_v3(
            &self.checked_handoff,
            construction_image,
            &self.execution,
            core,
            &payload_seals,
        )?;
        if direct != &oracle {
            return Err(
                "construction-published execution V3 handoff differs from the post-hoc oracle"
                    .to_owned(),
            );
        }
        Ok(())
    }

    pub(crate) fn seal(self) -> Result<SealedSemanticImageV3, String> {
        let execution_handoff = self
            .execution_handoff
            .ok_or_else(|| "finalized execution builder has no V3 handoff".to_owned())?;
        let schema = SEMANTIC_IMAGE_SCHEMA_V3.to_owned();
        let seal_digest =
            semantic_image_seal_digest(&schema, &self.checked_handoff, &execution_handoff)?;
        let _execution_image_v3 = self
            .execution_image_v3
            .ok_or_else(|| "finalized execution builder has no V3 image".to_owned())?;
        Ok(SealedSemanticImageV3 {
            schema,
            checked_handoff: self.checked_handoff,
            execution_handoff,
            execution: self.execution,
            seal_digest,
        })
    }
}

#[cfg(test)]
fn execution_row_payload_seals_v3_oracle(
    core: &crate::program_core::CanonicalProgramCoreV2,
) -> Result<ExecutionRowPayloadSealsV3, String> {
    fn seal<T: Serialize>(rows: &[T], scratch: &mut Vec<u8>) -> Result<Box<[[u8; 32]]>, String> {
        rows.iter()
            .map(|row| seal_execution_row_payload_v3(row, scratch))
            .collect::<Result<Vec<_>, _>>()
            .map(Vec::into_boxed_slice)
    }

    let mut scratch = Vec::new();
    Ok(ExecutionRowPayloadSealsV3 {
        expressions: seal(&core.executable.expressions, &mut scratch)?,
        statements: seal(&core.executable.statements, &mut scratch)?,
        call_occurrences: seal(&core.executable.call_occurrences, &mut scratch)?,
        sources: seal(&core.executable.sources, &mut scratch)?,
        states: seal(&core.executable.states, &mut scratch)?,
        roots: seal(&core.executable.roots, &mut scratch)?,
        functions: seal(&core.executable.functions, &mut scratch)?,
        materializations: seal(&core.materializations, &mut scratch)?,
        static_owners: seal(&core.scope_index.owners, &mut scratch)?,
    })
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum ExecutionProjectionStableFingerprintV3 {
    Checked {
        definition_digest: [u8; 32],
    },
    Invocation {
        overlay_digest: [u8; 32],
    },
    Producer {
        identity: [u8; 32],
        definition_digest: [u8; 32],
    },
}

#[derive(Serialize)]
struct ExecutionImageRowFingerprintV3<'a> {
    projection_stable_key_digest: [u8; 32],
    domain: ExecutionImageRowDomainV3,
    payload_digest: [u8; 32],
    relocation_stable_key_digests: &'a [[u8; 32]],
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
struct PendingExecutionProjectionIdV3(u32);

impl PendingExecutionProjectionIdV3 {
    const fn as_usize(self) -> usize {
        self.0 as usize
    }
}

struct PendingExecutionProjectionV3 {
    identity: ExecutionConstructionProjectionV3,
    stable_key_digest: [u8; 32],
    row_digests: Vec<[u8; 32]>,
    dependency_row_count: u32,
    relocations: Vec<PendingExecutionProjectionIdV3>,
}

struct ExecutionImageHandoffBuilderV3<'a> {
    checked: &'a CheckedImageHandoffV4,
    construction_image: &'a ExecutionConstructionImageV3,
    ids: BTreeMap<ExecutionConstructionProjectionV3, PendingExecutionProjectionIdV3>,
    stable_digest_ids: BTreeMap<[u8; 32], PendingExecutionProjectionIdV3>,
    projections: Vec<PendingExecutionProjectionV3>,
    entity_routes: Vec<(
        ExecutionImageRowDomainV3,
        u32,
        PendingExecutionProjectionIdV3,
    )>,
    /// Canonical encoding is the hot part of executable receipt sealing. Keep
    /// one pass-owned byte arena instead of allocating a fresh `Vec` for every
    /// projection key, row payload, row fingerprint, and shard.
    hash_scratch: Vec<u8>,
    /// Row fingerprints commit relocation stable keys rather than dense IDs.
    /// Most rows have only a few relocations, so retaining one arena removes a
    /// second allocation from every dependency-bearing row.
    relocation_digest_scratch: Vec<[u8; 32]>,
}

impl<'a> ExecutionImageHandoffBuilderV3<'a> {
    fn new(
        checked: &'a CheckedImageHandoffV4,
        construction_image: &'a ExecutionConstructionImageV3,
    ) -> Result<Self, String> {
        if construction_image
            .routes
            .definition_by_checked_projection
            .len()
            != checked.projections.len()
        {
            return Err(
                "execution V3 definition routes do not cover checked projections".to_owned(),
            );
        }
        Ok(Self {
            checked,
            construction_image,
            ids: BTreeMap::new(),
            stable_digest_ids: BTreeMap::new(),
            projections: Vec::new(),
            entity_routes: Vec::new(),
            hash_scratch: Vec::new(),
            relocation_digest_scratch: Vec::new(),
        })
    }

    fn checked_projection(
        &self,
        id: CheckedImageProjectionIdV2,
    ) -> Result<&boon_checked::CheckedImageProjectionV2, String> {
        self.checked.projection(id).ok_or_else(|| {
            format!(
                "execution V3 references missing checked projection {}",
                id.0
            )
        })
    }

    fn normalized_identity(
        &self,
        identity: ExecutionConstructionProjectionV3,
    ) -> Result<ExecutionConstructionProjectionV3, String> {
        Ok(match identity {
            ExecutionConstructionProjectionV3::Checked { projection } => {
                ExecutionConstructionProjectionV3::Checked {
                    projection: self.construction_image.definition_projection(projection)?,
                }
            }
            other => other,
        })
    }

    fn stable_fingerprint(
        &self,
        identity: ExecutionConstructionProjectionV3,
    ) -> Result<ExecutionProjectionStableFingerprintV3, String> {
        Ok(match identity {
            ExecutionConstructionProjectionV3::Checked { projection } => {
                ExecutionProjectionStableFingerprintV3::Checked {
                    definition_digest: self.checked_projection(projection)?.stable_key_digest,
                }
            }
            ExecutionConstructionProjectionV3::Invocation { occurrence } => {
                ExecutionProjectionStableFingerprintV3::Invocation {
                    overlay_digest: self
                        .construction_image
                        .invocation(occurrence)?
                        .stable_key_digest,
                }
            }
            ExecutionConstructionProjectionV3::Producer {
                identity,
                definition,
            } => ExecutionProjectionStableFingerprintV3::Producer {
                identity,
                definition_digest: self.checked_projection(definition)?.stable_key_digest,
            },
        })
    }

    fn intern(
        &mut self,
        identity: ExecutionConstructionProjectionV3,
    ) -> Result<PendingExecutionProjectionIdV3, String> {
        let identity = self.normalized_identity(identity)?;
        if let Some(id) = self.ids.get(&identity) {
            return Ok(*id);
        }
        let fingerprint = self.stable_fingerprint(identity)?;
        let stable_key_digest = boon_contract::canonical_serde_hash_v1_with_buffer(
            EXECUTION_IMAGE_PROJECTION_KEY_DOMAIN_V3,
            &fingerprint,
            &mut self.hash_scratch,
        )
        .map_err(|error| format!("failed to hash execution V3 projection key: {error}"))?;
        if let Some(previous) = self.stable_digest_ids.get(&stable_key_digest).copied() {
            let previous_identity = self
                .projections
                .get(previous.as_usize())
                .map(|projection| projection.identity)
                .ok_or_else(|| "execution V3 projection digest registry is stale".to_owned())?;
            if previous_identity != identity {
                return Err(format!(
                    "execution V3 projection digest collision between {previous_identity:?} and {identity:?}"
                ));
            }
            return Ok(previous);
        }
        let id = PendingExecutionProjectionIdV3(
            u32::try_from(self.projections.len())
                .map_err(|_| "execution V3 projection registry exceeds u32".to_owned())?,
        );
        self.ids.insert(identity, id);
        self.stable_digest_ids.insert(stable_key_digest, id);
        self.projections.push(PendingExecutionProjectionV3 {
            identity,
            stable_key_digest,
            row_digests: Vec::new(),
            dependency_row_count: 0,
            relocations: Vec::new(),
        });
        Ok(id)
    }

    fn push<T: Serialize>(
        &mut self,
        projection: PendingExecutionProjectionIdV3,
        domain: ExecutionImageRowDomainV3,
        payload: &T,
        relocations: Vec<PendingExecutionProjectionIdV3>,
    ) -> Result<(), String> {
        let payload_digest = seal_execution_row_payload_v3(payload, &mut self.hash_scratch)?;
        self.push_presealed(projection, domain, payload_digest, relocations)
    }

    fn push_presealed(
        &mut self,
        projection: PendingExecutionProjectionIdV3,
        domain: ExecutionImageRowDomainV3,
        payload_digest: [u8; 32],
        mut relocations: Vec<PendingExecutionProjectionIdV3>,
    ) -> Result<(), String> {
        let stable_key_digests = &self.projections;
        relocations
            .sort_unstable_by_key(|target| stable_key_digests[target.as_usize()].stable_key_digest);
        relocations.dedup_by_key(|target| stable_key_digests[target.as_usize()].stable_key_digest);
        relocations.retain(|target| *target != projection);
        self.relocation_digest_scratch.clear();
        self.relocation_digest_scratch.extend(
            relocations
                .iter()
                .map(|target| self.projections[target.as_usize()].stable_key_digest),
        );
        let projection_stable_key_digest =
            self.projections[projection.as_usize()].stable_key_digest;
        let digest = boon_contract::canonical_serde_hash_v1_with_buffer(
            EXECUTION_IMAGE_ROW_DOMAIN_V3,
            &ExecutionImageRowFingerprintV3 {
                projection_stable_key_digest,
                domain,
                payload_digest,
                relocation_stable_key_digests: &self.relocation_digest_scratch,
            },
            &mut self.hash_scratch,
        )
        .map_err(|error| format!("failed to hash execution V3 row: {error}"))?;
        let has_relocations = !relocations.is_empty();
        let pending = &mut self.projections[projection.as_usize()];
        pending.row_digests.push(digest);
        if has_relocations {
            pending.dependency_row_count = pending
                .dependency_row_count
                .checked_add(1)
                .ok_or_else(|| "execution V3 dependency row count overflow".to_owned())?;
        }
        pending.relocations.extend(relocations);
        Ok(())
    }

    fn route(
        &mut self,
        domain: ExecutionImageRowDomainV3,
        dense_index: usize,
        projection: PendingExecutionProjectionIdV3,
    ) -> Result<(), String> {
        self.entity_routes.push((
            domain,
            u32::try_from(dense_index)
                .map_err(|_| "execution V3 entity index exceeds u32".to_owned())?,
            projection,
        ));
        Ok(())
    }

    fn finish(
        self,
        source_bundle_digest_v1: SourceBundleDigestV1,
        role: ProgramRole,
        mut manifest_prefix: crate::dependency_manifest::ManifestCheckedExecutionPrefixBuilderV7,
    ) -> Result<
        (
            ExecutionImageHandoffV3,
            crate::dependency_manifest::ManifestCheckedExecutionPrefixV7,
        ),
        String,
    > {
        let Self {
            checked,
            construction_image,
            ids: _,
            stable_digest_ids,
            mut projections,
            mut entity_routes,
            mut hash_scratch,
            relocation_digest_scratch: _,
        } = self;
        let mut canonical_projection_by_pending =
            vec![ExecutionImageProjectionIdV3(u32::MAX); projections.len()];
        for (canonical_index, pending_id) in stable_digest_ids.values().copied().enumerate() {
            canonical_projection_by_pending[pending_id.as_usize()] = ExecutionImageProjectionIdV3(
                u32::try_from(canonical_index)
                    .map_err(|_| "execution V3 canonical projection index exceeds u32")?,
            );
        }
        let stable_key_digests = projections
            .iter()
            .map(|projection| projection.stable_key_digest)
            .collect::<Vec<_>>();
        let invocation_overlays = construction_image.routes.invocations.clone();
        for pending_id in stable_digest_ids.values().copied() {
            let pending = &projections[pending_id.as_usize()];
            manifest_prefix
                .register_execution_identity(
                    checked,
                    &invocation_overlays,
                    pending.identity,
                    pending.stable_key_digest,
                )
                .map_err(|error| error.to_string())?;
        }
        let mut sealed_projections = Vec::with_capacity(projections.len());
        let mut relocation_arena = Vec::new();
        let mut manifest_relocation_targets = Vec::new();
        for (canonical_index, pending_id) in stable_digest_ids.values().copied().enumerate() {
            let pending = &mut projections[pending_id.as_usize()];
            if pending.row_digests.is_empty() {
                return Err(format!(
                    "execution V3 projection {:?} has no final rows",
                    pending.identity
                ));
            }
            pending.relocations.sort_unstable_by(|left, right| {
                stable_key_digests[left.as_usize()].cmp(&stable_key_digests[right.as_usize()])
            });
            pending.relocations.dedup();
            let relocation_start = u32::try_from(relocation_arena.len())
                .map_err(|_| "execution V3 relocation arena exceeds u32")?;
            let relocation_len = u32::try_from(pending.relocations.len())
                .map_err(|_| "execution V3 relocation span exceeds u32")?;
            relocation_start
                .checked_add(relocation_len)
                .ok_or_else(|| "execution V3 relocation arena exceeds u32".to_owned())?;
            manifest_relocation_targets.clear();
            for target in &pending.relocations {
                let target = canonical_projection_by_pending[target.as_usize()];
                manifest_relocation_targets.push(target.as_usize());
                relocation_arena.push(target);
            }
            let local_content_digest = boon_contract::canonical_serde_hash_v1_with_buffer(
                EXECUTION_IMAGE_SHARD_DOMAIN_V3,
                &(pending.stable_key_digest, &pending.row_digests),
                &mut hash_scratch,
            )
            .map_err(|error| format!("failed to hash execution V3 shard: {error}"))?;
            let row_count = u32::try_from(pending.row_digests.len())
                .map_err(|_| "execution V3 shard row count exceeds u32")?;
            manifest_prefix
                .publish_execution_projection(
                    &invocation_overlays,
                    canonical_index,
                    pending.identity,
                    local_content_digest,
                    row_count,
                    pending.dependency_row_count,
                    &manifest_relocation_targets,
                )
                .map_err(|error| error.to_string())?;
            sealed_projections.push(ExecutionImageProjectionV3 {
                identity: pending.identity,
                stable_key_digest: pending.stable_key_digest,
                local_content_digest,
                row_count,
                dependency_row_count: pending.dependency_row_count,
                relocation_span: ExecutionImageRelocationSpanV3 {
                    start: relocation_start,
                    len: relocation_len,
                },
            });
        }
        entity_routes.sort_unstable_by(|left, right| {
            (left.0, left.1, stable_key_digests[left.2.as_usize()]).cmp(&(
                right.0,
                right.1,
                stable_key_digests[right.2.as_usize()],
            ))
        });
        if entity_routes
            .windows(2)
            .any(|pair| (pair[0].0, pair[0].1) == (pair[1].0, pair[1].1))
        {
            return Err("execution V3 routes an entity more than once".to_owned());
        }
        let mut sealed_entity_routes = Vec::with_capacity(entity_routes.len());
        for (domain, dense_index, projection) in entity_routes {
            let projection = canonical_projection_by_pending[projection.as_usize()];
            manifest_prefix
                .route_execution_entity(domain, dense_index, projection.as_usize())
                .map_err(|error| error.to_string())?;
            sealed_entity_routes.push(ExecutionImageEntityRouteV3 {
                domain,
                dense_index,
                projection,
            });
        }
        let local_image_digest = boon_contract::canonical_serde_hash_v1_with_buffer(
            EXECUTION_IMAGE_HANDOFF_DOMAIN_V3,
            &(
                EXECUTION_IMAGE_HANDOFF_SCHEMA_V3,
                source_bundle_digest_v1,
                role,
                &invocation_overlays,
                &sealed_projections,
                &relocation_arena,
                &sealed_entity_routes,
            ),
            &mut hash_scratch,
        )
        .map_err(|error| format!("failed to hash execution V3 handoff: {error}"))?;
        if std::env::var_os("BOON_SEMANTIC_TRACE").is_some() {
            let row_count = sealed_projections
                .iter()
                .map(|projection| projection.row_count as usize)
                .sum::<usize>();
            eprintln!(
                "boon_semantic execution_handoff_v3 overlays={} projections={} rows={} entity_routes={} relocations={}",
                invocation_overlays.len(),
                sealed_projections.len(),
                row_count,
                sealed_entity_routes.len(),
                relocation_arena.len(),
            );
        }
        Ok((
            ExecutionImageHandoffV3 {
                schema: EXECUTION_IMAGE_HANDOFF_SCHEMA_V3.to_owned(),
                source_bundle_digest_v1,
                role,
                invocation_overlays,
                projections: sealed_projections,
                relocations: relocation_arena,
                entity_routes: sealed_entity_routes,
                local_image_digest,
            },
            manifest_prefix
                .finish(source_bundle_digest_v1, role, local_image_digest)
                .map_err(|error| error.to_string())?,
        ))
    }
}

#[cfg(test)]
#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum ExecutionProjectionStableFingerprintV2 {
    Checked {
        checked_projection_digest: [u8; 32],
    },
    Invocation {
        root: DistributedCallOccurrenceRoot,
        definition_digest: [u8; 32],
        call_path_digest: Option<[u8; 32]>,
    },
}

#[cfg(test)]
#[derive(Serialize)]
struct ExecutionImageRowFingerprintV2<'a> {
    projection_stable_key_digest: [u8; 32],
    domain: ExecutionImageRowDomainV2,
    payload_digest: [u8; 32],
    relocation_stable_key_digests: &'a [[u8; 32]],
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
struct PendingExecutionProjectionIdV2(u32);

#[cfg(test)]
impl PendingExecutionProjectionIdV2 {
    const fn as_usize(self) -> usize {
        self.0 as usize
    }
}

#[cfg(test)]
struct PendingInvocationPathV2 {
    parent: Option<ExecutionInvocationPathIdV2>,
    call_site: CheckedImageProjectionIdV2,
    stable_path_digest: [u8; 32],
}

#[cfg(test)]
struct PendingExecutionProjectionV2 {
    identity: SemanticImageProjectionIdentityV2,
    stable_key_digest: [u8; 32],
    row_digests: Vec<[u8; 32]>,
    dependency_row_count: u32,
    relocations: Vec<PendingExecutionProjectionIdV2>,
}

#[cfg(test)]
struct ExecutionImageHandoffBuilderV2<'a> {
    checked: &'a CheckedImageHandoffV4,
    construction_image: &'a ExecutionConstructionImageV3,
    path_ids: BTreeMap<
        (
            Option<ExecutionInvocationPathIdV2>,
            CheckedImageProjectionIdV2,
        ),
        ExecutionInvocationPathIdV2,
    >,
    path_digest_ids: BTreeMap<[u8; 32], ExecutionInvocationPathIdV2>,
    paths: Vec<PendingInvocationPathV2>,
    ids: BTreeMap<SemanticImageProjectionIdentityV2, PendingExecutionProjectionIdV2>,
    stable_digest_ids: BTreeMap<[u8; 32], PendingExecutionProjectionIdV2>,
    projections: Vec<PendingExecutionProjectionV2>,
    entity_routes: Vec<(
        ExecutionImageRowDomainV2,
        u32,
        PendingExecutionProjectionIdV2,
    )>,
}

#[cfg(test)]
impl<'a> ExecutionImageHandoffBuilderV2<'a> {
    fn new(
        checked: &'a CheckedImageHandoffV4,
        construction_image: &'a ExecutionConstructionImageV3,
    ) -> Result<Self, String> {
        if construction_image
            .routes
            .definition_by_checked_projection
            .len()
            != checked.projections.len()
        {
            return Err(
                "execution construction definition routes do not cover checked projections"
                    .to_owned(),
            );
        }
        Ok(Self {
            checked,
            construction_image,
            path_ids: BTreeMap::new(),
            path_digest_ids: BTreeMap::new(),
            paths: Vec::new(),
            ids: BTreeMap::new(),
            stable_digest_ids: BTreeMap::new(),
            projections: Vec::new(),
            entity_routes: Vec::new(),
        })
    }

    fn checked_projection(
        &self,
        id: CheckedImageProjectionIdV2,
    ) -> Result<&boon_checked::CheckedImageProjectionV2, String> {
        self.checked.projection(id).ok_or_else(|| {
            format!(
                "execution image references missing checked projection {}",
                id.0
            )
        })
    }

    fn definition_projection(
        &self,
        projection: CheckedImageProjectionIdV2,
    ) -> Result<CheckedImageProjectionIdV2, String> {
        self.construction_image.definition_projection(projection)
    }

    fn append_path(
        &mut self,
        parent: Option<ExecutionInvocationPathIdV2>,
        call_site: CheckedImageProjectionIdV2,
    ) -> Result<ExecutionInvocationPathIdV2, String> {
        let key = (parent, call_site);
        if let Some(id) = self.path_ids.get(&key) {
            return Ok(*id);
        }
        let call_site_projection = self.checked_projection(call_site)?;
        if !matches!(
            call_site_projection.stable_key.region,
            CheckedShardRegionV2::Invocation { .. }
        ) {
            return Err(format!(
                "checked projection {} is not an authored invocation site",
                call_site.0
            ));
        }
        let parent_digest = parent
            .map(|parent| {
                self.paths
                    .get(parent.as_usize())
                    .map(|path| path.stable_path_digest)
                    .ok_or_else(|| format!("invocation path has missing parent {}", parent.0))
            })
            .transpose()?;
        let stable_path_digest = boon_contract::canonical_serde_hash_v1(
            EXECUTION_INVOCATION_PATH_DOMAIN_V2,
            &(parent_digest, call_site_projection.stable_key_digest),
        )
        .map_err(|error| format!("failed to hash execution invocation path: {error}"))?;
        if let Some(previous) = self.path_digest_ids.get(&stable_path_digest).copied() {
            let previous_path = self
                .paths
                .get(previous.as_usize())
                .ok_or_else(|| "invocation path digest registry is stale".to_owned())?;
            if (previous_path.parent, previous_path.call_site) != key {
                return Err(format!(
                    "execution invocation-path digest collision at checked call {}",
                    call_site.0
                ));
            }
            return Ok(previous);
        }
        let id = ExecutionInvocationPathIdV2(
            u32::try_from(self.paths.len())
                .map_err(|_| "execution invocation-path registry exceeds u32")?,
        );
        self.path_ids.insert(key, id);
        self.path_digest_ids.insert(stable_path_digest, id);
        self.paths.push(PendingInvocationPathV2 {
            parent,
            call_site,
            stable_path_digest,
        });
        Ok(id)
    }

    fn projection_stable_fingerprint(
        &self,
        identity: SemanticImageProjectionIdentityV2,
    ) -> Result<ExecutionProjectionStableFingerprintV2, String> {
        match identity {
            SemanticImageProjectionIdentityV2::Checked { projection } => {
                Ok(ExecutionProjectionStableFingerprintV2::Checked {
                    checked_projection_digest: self
                        .checked_projection(projection)?
                        .stable_key_digest,
                })
            }
            SemanticImageProjectionIdentityV2::Invocation {
                root,
                definition,
                call_path,
            } => Ok(ExecutionProjectionStableFingerprintV2::Invocation {
                root,
                definition_digest: self.checked_projection(definition)?.stable_key_digest,
                call_path_digest: call_path
                    .map(|path| {
                        self.paths
                            .get(path.as_usize())
                            .map(|path| path.stable_path_digest)
                            .ok_or_else(|| {
                                format!("execution projection has missing path {}", path.0)
                            })
                    })
                    .transpose()?,
            }),
        }
    }

    fn intern(
        &mut self,
        identity: SemanticImageProjectionIdentityV2,
    ) -> Result<PendingExecutionProjectionIdV2, String> {
        if let Some(id) = self.ids.get(&identity) {
            return Ok(*id);
        }
        let stable_key_digest = boon_contract::canonical_serde_hash_v1(
            EXECUTION_IMAGE_PROJECTION_KEY_DOMAIN_V2,
            &self.projection_stable_fingerprint(identity)?,
        )
        .map_err(|error| format!("failed to hash execution projection key: {error}"))?;
        if let Some(previous) = self.stable_digest_ids.get(&stable_key_digest).copied() {
            let previous_identity = self
                .projections
                .get(previous.as_usize())
                .map(|projection| projection.identity)
                .ok_or_else(|| "execution projection digest registry is stale".to_owned())?;
            if previous_identity != identity {
                return Err(format!(
                    "execution projection stable-key digest collision between {previous_identity:?} and {identity:?}"
                ));
            }
            return Ok(previous);
        }
        let id = PendingExecutionProjectionIdV2(
            u32::try_from(self.projections.len())
                .map_err(|_| "execution image projection registry exceeds u32".to_owned())?,
        );
        self.ids.insert(identity, id);
        self.stable_digest_ids.insert(stable_key_digest, id);
        self.projections.push(PendingExecutionProjectionV2 {
            identity,
            stable_key_digest,
            row_digests: Vec::new(),
            dependency_row_count: 0,
            relocations: Vec::new(),
        });
        Ok(id)
    }

    fn checked(
        &mut self,
        projection: CheckedImageProjectionIdV2,
    ) -> Result<PendingExecutionProjectionIdV2, String> {
        self.intern(SemanticImageProjectionIdentityV2::Checked { projection })
    }

    fn invocation(
        &mut self,
        root: DistributedCallOccurrenceRoot,
        definition: CheckedImageProjectionIdV2,
        call_path: Option<ExecutionInvocationPathIdV2>,
    ) -> Result<PendingExecutionProjectionIdV2, String> {
        self.intern(SemanticImageProjectionIdentityV2::Invocation {
            root,
            definition,
            call_path,
        })
    }

    fn identity(
        &self,
        projection: PendingExecutionProjectionIdV2,
    ) -> Result<SemanticImageProjectionIdentityV2, String> {
        self.projections
            .get(projection.as_usize())
            .map(|projection| projection.identity)
            .ok_or_else(|| {
                format!(
                    "execution image references missing pending projection {}",
                    projection.0
                )
            })
    }

    fn push<T: Serialize>(
        &mut self,
        projection: PendingExecutionProjectionIdV2,
        domain: ExecutionImageRowDomainV2,
        payload: &T,
        mut relocations: Vec<PendingExecutionProjectionIdV2>,
    ) -> Result<(), String> {
        let stable_key_digests = &self.projections;
        relocations
            .sort_unstable_by_key(|target| stable_key_digests[target.as_usize()].stable_key_digest);
        relocations.dedup_by_key(|target| stable_key_digests[target.as_usize()].stable_key_digest);
        relocations.retain(|target| target != &projection);
        let payload_digest =
            boon_contract::canonical_serde_hash_v1(EXECUTION_IMAGE_ROW_PAYLOAD_DOMAIN_V2, payload)
                .map_err(|error| format!("failed to hash execution image row payload: {error}"))?;
        let relocation_stable_key_digests = relocations
            .iter()
            .map(|target| self.projections[target.as_usize()].stable_key_digest)
            .collect::<Vec<_>>();
        let projection_stable_key_digest =
            self.projections[projection.as_usize()].stable_key_digest;
        let digest = boon_contract::canonical_serde_hash_v1(
            EXECUTION_IMAGE_ROW_DOMAIN_V2,
            &ExecutionImageRowFingerprintV2 {
                projection_stable_key_digest,
                domain,
                payload_digest,
                relocation_stable_key_digests: &relocation_stable_key_digests,
            },
        )
        .map_err(|error| format!("failed to hash execution image row fingerprint: {error}"))?;
        let has_relocations = !relocations.is_empty();
        let pending = &mut self.projections[projection.as_usize()];
        pending.row_digests.push(digest);
        if has_relocations {
            pending.dependency_row_count = pending
                .dependency_row_count
                .checked_add(1)
                .ok_or_else(|| "execution image dependency row count overflow".to_owned())?;
        }
        pending.relocations.extend(relocations);
        Ok(())
    }

    fn route(
        &mut self,
        domain: ExecutionImageRowDomainV2,
        dense_index: usize,
        projection: PendingExecutionProjectionIdV2,
    ) -> Result<(), String> {
        self.entity_routes.push((
            domain,
            u32::try_from(dense_index)
                .map_err(|_| "execution image entity index exceeds u32".to_owned())?,
            projection,
        ));
        Ok(())
    }

    fn finish(
        self,
        source_bundle_digest_v1: SourceBundleDigestV1,
        role: ProgramRole,
    ) -> Result<ExecutionImageHandoffV2, String> {
        if std::env::var_os("BOON_SEMANTIC_TRACE").is_some() {
            let checked_projections = self
                .projections
                .iter()
                .filter(|projection| {
                    matches!(
                        projection.identity,
                        SemanticImageProjectionIdentityV2::Checked { .. }
                    )
                })
                .count();
            let row_count = self
                .projections
                .iter()
                .map(|projection| projection.row_digests.len())
                .sum::<usize>();
            let raw_relocation_count = self
                .projections
                .iter()
                .map(|projection| projection.relocations.len())
                .sum::<usize>();
            let maximum_projection_rows = self
                .projections
                .iter()
                .map(|projection| projection.row_digests.len())
                .max()
                .unwrap_or_default();
            eprintln!(
                "boon_semantic execution_handoff pending paths={} projections={} checked_projections={} invocation_projections={} rows={} entity_routes={} raw_relocations={} maximum_projection_rows={}",
                self.paths.len(),
                self.projections.len(),
                checked_projections,
                self.projections.len() - checked_projections,
                row_count,
                self.entity_routes.len(),
                raw_relocation_count,
                maximum_projection_rows,
            );
        }
        let Self {
            checked: _,
            construction_image: _,
            path_ids: _,
            path_digest_ids,
            paths,
            ids: _,
            stable_digest_ids,
            mut projections,
            mut entity_routes,
        } = self;
        let mut canonical_path_by_pending =
            vec![ExecutionInvocationPathIdV2(u32::MAX); paths.len()];
        let mut invocation_paths = Vec::with_capacity(paths.len());
        for (canonical_index, pending_id) in path_digest_ids.values().copied().enumerate() {
            canonical_path_by_pending[pending_id.as_usize()] = ExecutionInvocationPathIdV2(
                u32::try_from(canonical_index)
                    .map_err(|_| "execution canonical invocation-path index exceeds u32")?,
            );
        }
        for pending_id in path_digest_ids.values().copied() {
            let path = &paths[pending_id.as_usize()];
            invocation_paths.push(ExecutionInvocationPathNodeV2 {
                parent: path
                    .parent
                    .map(|parent| canonical_path_by_pending[parent.as_usize()]),
                call_site: path.call_site,
                stable_path_digest: path.stable_path_digest,
            });
        }

        let mut canonical_projection_by_pending =
            vec![ExecutionImageProjectionIdV2(u32::MAX); projections.len()];
        for (canonical_index, pending_id) in stable_digest_ids.values().copied().enumerate() {
            canonical_projection_by_pending[pending_id.as_usize()] = ExecutionImageProjectionIdV2(
                u32::try_from(canonical_index)
                    .map_err(|_| "execution canonical projection index exceeds u32")?,
            );
        }
        let stable_key_digests = projections
            .iter()
            .map(|projection| projection.stable_key_digest)
            .collect::<Vec<_>>();
        let mut sealed_projections = Vec::with_capacity(projections.len());
        let mut relocation_arena = Vec::new();
        for pending_id in stable_digest_ids.values().copied() {
            let pending = &mut projections[pending_id.as_usize()];
            if pending.row_digests.is_empty() {
                return Err(format!(
                    "execution image projection {:?} has no local rows",
                    pending.identity
                ));
            }
            let row_count = u32::try_from(pending.row_digests.len())
                .map_err(|_| "execution image shard row count exceeds u32".to_owned())?;
            pending.relocations.sort_unstable_by(|left, right| {
                stable_key_digests[left.as_usize()].cmp(&stable_key_digests[right.as_usize()])
            });
            pending.relocations.dedup();
            let relocation_start = u32::try_from(relocation_arena.len())
                .map_err(|_| "execution image relocation arena exceeds u32")?;
            let relocation_len = u32::try_from(pending.relocations.len())
                .map_err(|_| "execution image projection relocation span exceeds u32")?;
            relocation_start
                .checked_add(relocation_len)
                .ok_or_else(|| "execution image relocation arena exceeds u32".to_owned())?;
            relocation_arena.extend(
                pending
                    .relocations
                    .iter()
                    .map(|target| canonical_projection_by_pending[target.as_usize()]),
            );
            let local_content_digest = boon_contract::canonical_serde_hash_v1(
                EXECUTION_IMAGE_SHARD_DOMAIN_V2,
                &(pending.stable_key_digest, &pending.row_digests),
            )
            .map_err(|error| format!("failed to hash execution image shard: {error}"))?;
            let identity = match pending.identity {
                SemanticImageProjectionIdentityV2::Invocation {
                    root,
                    definition,
                    call_path,
                } => SemanticImageProjectionIdentityV2::Invocation {
                    root,
                    definition,
                    call_path: call_path.map(|path| canonical_path_by_pending[path.as_usize()]),
                },
                checked => checked,
            };
            sealed_projections.push(ExecutionImageProjectionV2 {
                identity,
                stable_key_digest: pending.stable_key_digest,
                local_content_digest,
                row_count,
                dependency_row_count: pending.dependency_row_count,
                relocation_span: ExecutionImageRelocationSpanV2 {
                    start: relocation_start,
                    len: relocation_len,
                },
            });
        }
        entity_routes.sort_unstable_by(|left, right| {
            (left.0, left.1, stable_key_digests[left.2.as_usize()]).cmp(&(
                right.0,
                right.1,
                stable_key_digests[right.2.as_usize()],
            ))
        });
        if entity_routes
            .windows(2)
            .any(|pair| (pair[0].0, pair[0].1) == (pair[1].0, pair[1].1))
        {
            return Err("execution image entity routes more than once".to_owned());
        }
        let entity_routes = entity_routes
            .into_iter()
            .map(|(domain, dense_index, projection)| {
                Ok(ExecutionImageEntityRouteV2 {
                    domain,
                    dense_index,
                    projection: canonical_projection_by_pending[projection.as_usize()],
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        let local_image_digest = boon_contract::canonical_serde_hash_v1(
            EXECUTION_IMAGE_HANDOFF_DOMAIN_V2,
            &(
                EXECUTION_IMAGE_HANDOFF_SCHEMA_V2,
                source_bundle_digest_v1,
                role,
                &invocation_paths,
                &sealed_projections,
                &relocation_arena,
                &entity_routes,
            ),
        )
        .map_err(|error| format!("failed to hash execution image handoff: {error}"))?;
        Ok(ExecutionImageHandoffV2 {
            schema: EXECUTION_IMAGE_HANDOFF_SCHEMA_V2.to_owned(),
            source_bundle_digest_v1,
            role,
            invocation_paths,
            projections: sealed_projections,
            relocations: relocation_arena,
            entity_routes,
            local_image_digest,
        })
    }
}

fn checked_projection(
    checked: &CheckedImageHandoffV4,
    domain: CheckedImageRowDomainV2,
    dense_index: usize,
) -> Result<CheckedImageProjectionIdV2, String> {
    checked
        .entity_projection(domain, dense_index)
        .ok_or_else(|| {
            format!("checked image has no {domain:?} route for dense index {dense_index}")
        })
}

fn checked_definition_routes(
    checked: &CheckedImageHandoffV4,
) -> Result<Vec<CheckedImageProjectionIdV2>, String> {
    let mut by_owner = BTreeMap::<
        &boon_checked::CheckedShardOwnerKeyV2,
        (
            Option<CheckedImageProjectionIdV2>,
            Option<CheckedImageProjectionIdV2>,
        ),
    >::new();
    for (index, projection) in checked.projections.iter().enumerate() {
        let id = CheckedImageProjectionIdV2(
            u32::try_from(index).map_err(|_| "checked image projection index exceeds u32")?,
        );
        let entry = by_owner.entry(&projection.stable_key.owner).or_default();
        let slot = match projection.stable_key.region {
            CheckedShardRegionV2::Definition => &mut entry.0,
            CheckedShardRegionV2::Interface => &mut entry.1,
            _ => continue,
        };
        if slot.replace(id).is_some() {
            return Err(format!(
                "checked owner {:?} has duplicate interface/definition projections",
                projection.stable_key.owner
            ));
        }
    }
    Ok(checked
        .projections
        .iter()
        .enumerate()
        .map(|(index, projection)| {
            by_owner
                .get(&projection.stable_key.owner)
                .and_then(|(definition, interface)| (*definition).or(*interface))
                .unwrap_or(CheckedImageProjectionIdV2(index as u32))
        })
        .collect())
}

pub(crate) fn execution_construction_routes_v3(
    checked: &CheckedImageHandoffV4,
    out: &ResolvedOutGraph,
) -> Result<ExecutionConstructionRoutesV3, String> {
    let trace_started = std::env::var_os("BOON_SEMANTIC_TRACE")
        .is_some()
        .then(std::time::Instant::now);
    let definition_by_checked_projection = checked_definition_routes(checked)?;
    let owner_occurrences = static_owner_occurrences_v3(out)?;
    let producer_roots = out
        .producer_roots()
        .iter()
        .map(|root| (root.call, root.spec.identity))
        .collect::<BTreeMap<_, _>>();
    let mut invocations =
        Vec::<ExecutionInvocationOverlayV3>::with_capacity(out.call_instances.len());
    let mut stable_digest_owner = BTreeMap::<[u8; 32], OutCallInstanceId>::new();
    let mut hash_scratch = Vec::new();
    for instance in &out.call_instances {
        if instance.id.as_usize() != invocations.len() {
            return Err(format!(
                "OUT call instance {} is not dense while preparing execution routes",
                instance.id
            ));
        }
        let (root, parent_path_digest) = match instance.parent {
            Some(parent) => {
                let parent_overlay = invocations.get(parent.as_usize()).ok_or_else(|| {
                    format!("OUT call {} has missing parent {parent}", instance.id)
                })?;
                (parent_overlay.root, parent_overlay.stable_path_digest)
            }
            None => (
                producer_roots
                    .get(&instance.id)
                    .copied()
                    .map(DistributedCallOccurrenceRoot::Producer)
                    .unwrap_or(DistributedCallOccurrenceRoot::Program),
                None,
            ),
        };
        let (authored_call_site, stable_path_digest) = if producer_roots.contains_key(&instance.id)
        {
            if instance.parent.is_some() {
                return Err(format!(
                    "producer-root OUT call {} unexpectedly has a parent",
                    instance.id
                ));
            }
            (None, None)
        } else {
            let checked_call = instance.provenance.call_id.ok_or_else(|| {
                format!(
                    "non-producer OUT call {} has no checked call identity",
                    instance.id
                )
            })?;
            let projection = checked_projection(
                checked,
                CheckedImageRowDomainV2::Call,
                checked_call.0 as usize,
            )?;
            let projection_record = checked.projection(projection).ok_or_else(|| {
                format!(
                    "execution route references missing checked call projection {}",
                    projection.0
                )
            })?;
            if !matches!(
                projection_record.stable_key.region,
                CheckedShardRegionV2::Invocation { .. }
            ) {
                return Err(format!(
                    "checked call {} does not route to an invocation shard",
                    checked_call.0
                ));
            }
            let digest = boon_contract::canonical_serde_hash_v1_with_buffer(
                EXECUTION_INVOCATION_PATH_DOMAIN_V2,
                &(parent_path_digest, projection_record.stable_key_digest),
                &mut hash_scratch,
            )
            .map_err(|error| format!("failed to hash execution V3 invocation path: {error}"))?;
            (Some(projection), Some(digest))
        };
        let callable_projection = checked_projection(
            checked,
            CheckedImageRowDomainV2::Callable,
            instance.provenance.callable.0 as usize,
        )?;
        let definition = definition_by_checked_projection
            .get(callable_projection.as_usize())
            .copied()
            .ok_or_else(|| {
                format!(
                    "execution route has no definition for checked callable projection {}",
                    callable_projection.0
                )
            })?;
        let definition_digest = checked
            .projection(definition)
            .ok_or_else(|| {
                format!(
                    "execution route references missing checked definition projection {}",
                    definition.0
                )
            })?
            .stable_key_digest;
        let stable_key_digest = boon_contract::canonical_serde_hash_v1_with_buffer(
            EXECUTION_INVOCATION_OVERLAY_DOMAIN_V3,
            &ExecutionInvocationOverlayFingerprintV3 {
                root,
                definition_digest,
                path_digest: stable_path_digest,
            },
            &mut hash_scratch,
        )
        .map_err(|error| format!("failed to hash execution V3 invocation overlay: {error}"))?;
        if let Some(previous) = stable_digest_owner.insert(stable_key_digest, instance.id) {
            return Err(format!(
                "execution V3 invocation overlay digest collides between {previous} and {}",
                instance.id
            ));
        }
        invocations.push(ExecutionInvocationOverlayV3 {
            occurrence: instance.id,
            parent: instance.parent,
            root,
            authored_call_site,
            definition,
            stable_path_digest,
            stable_key_digest,
        });
    }
    #[cfg(test)]
    let local_digest = boon_contract::canonical_serde_hash_v1_with_buffer(
        EXECUTION_CONSTRUCTION_ROUTES_DOMAIN_V3,
        &(
            EXECUTION_CONSTRUCTION_ROUTES_SCHEMA_V3,
            checked.source_bundle_digest_v1,
            checked.role,
            checked.local_image_digest,
            &definition_by_checked_projection,
            &invocations,
            &owner_occurrences,
        ),
        &mut hash_scratch,
    )
    .map_err(|error| format!("failed to hash execution V3 construction routes: {error}"))?;
    let routes = ExecutionConstructionRoutesV3 {
        definition_by_checked_projection,
        invocations,
        owner_occurrences,
        #[cfg(test)]
        local_digest,
    };
    if let Some(started) = trace_started {
        eprintln!(
            "boon_semantic execution_routes_v3 definitions={} invocations={} owners={} elapsed_ms={:.3}",
            routes.definition_by_checked_projection.len(),
            routes.invocations.len(),
            routes.owner_occurrences.len(),
            started.elapsed().as_secs_f64() * 1_000.0,
        );
    }
    Ok(routes)
}

fn static_owner_occurrences_v3(out: &ResolvedOutGraph) -> Result<Vec<OutCallInstanceId>, String> {
    let mut occurrences = vec![None; out.static_owners.len()];
    let mut attach = |owner: StaticOwnerId, occurrence: OutCallInstanceId, context: &str| {
        let slot = occurrences.get_mut(owner.as_usize()).ok_or_else(|| {
            format!("static owner {owner} is outside the dense owner table at {context}")
        })?;
        if let Some(previous) = slot.replace(occurrence)
            && previous != occurrence
        {
            return Err(format!(
                "static owner {owner} has conflicting occurrences {previous} and {occurrence} at {context}"
            ));
        }
        Ok(())
    };
    for call in &out.call_instances {
        if let Some(owner) = call.owner {
            attach(owner, call.id, &format!("OUT call {}", call.id))?;
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
        attach(owner, port.call, &format!("OUT net {}", net.id))?;
    }
    drop(attach);
    out.static_owners
        .iter()
        .enumerate()
        .map(|(index, owner)| {
            if owner.id.as_usize() != index {
                return Err(format!(
                    "static owner {} is noncanonical at dense index {index}",
                    owner.id
                ));
            }
            occurrences[index]
                .ok_or_else(|| format!("static owner {} has no exact OUT occurrence", owner.id))
        })
        .collect()
}

pub(crate) fn execution_construction_image_v3(
    checked: &CheckedImageHandoffV4,
    routes: ExecutionConstructionRoutesV3,
    execution: &SemanticExecutionImageColumnsV1,
) -> Result<ExecutionConstructionImageV3, String> {
    let trace_started = std::env::var_os("BOON_SEMANTIC_TRACE")
        .is_some()
        .then(std::time::Instant::now);
    let mut expression_routes = Vec::with_capacity(execution.expressions.len());
    for expression in &execution.expressions {
        if expression.id.as_usize() != expression_routes.len() {
            return Err(format!(
                "execution expression {} is noncanonical while binding V3 routes",
                expression.id
            ));
        }
        let origin = execution
            .checked_expression_origins
            .get(expression.id.as_usize())
            .filter(|origin| origin.expression == expression.id)
            .ok_or_else(|| format!("execution expression {} has no exact origin", expression.id))?;
        let checked_projection = checked_projection(
            checked,
            CheckedImageRowDomainV2::Expression,
            expression.checked_expr_id.0 as usize,
        )?;
        let checked_route = ExecutionConstructionProjectionV3::Checked {
            projection: checked_projection,
        };
        let static_occurrence = expression
            .owner
            .map(|owner| {
                routes
                    .owner_occurrences
                    .get(owner.as_usize())
                    .copied()
                    .ok_or_else(|| {
                        format!(
                            "execution expression {} has unanchored static owner {owner}",
                            expression.id
                        )
                    })
            })
            .transpose()?;
        let frame_occurrence = origin
            .call_instance
            .map(|frame| routes.invocation(frame).map(|_| frame))
            .transpose()?;
        let route = match (static_occurrence, frame_occurrence) {
            (Some(static_occurrence), Some(frame_occurrence)) => {
                let static_root = routes.invocation(static_occurrence)?.root;
                let frame_root = routes.invocation(frame_occurrence)?.root;
                if static_root != frame_root {
                    return Err(format!(
                        "execution expression {} crosses static {static_root:?} and invocation {frame_root:?} roots",
                        expression.id
                    ));
                }
                ExecutionConstructionProjectionV3::Invocation {
                    occurrence: static_occurrence,
                }
            }
            (Some(occurrence), None) | (None, Some(occurrence)) => {
                ExecutionConstructionProjectionV3::Invocation { occurrence }
            }
            (None, None) => checked_route,
        };
        expression_routes.push(route);
    }

    let mut statement_routes = Vec::with_capacity(execution.statements.len());
    for statement in &execution.statements {
        if statement.id.as_usize() != statement_routes.len() {
            return Err(format!(
                "execution statement {} is noncanonical while binding V3 routes",
                statement.id
            ));
        }
        let checked_statement = match &statement.origin {
            crate::SemanticStatementOrigin::Checked { statement } => *statement,
            crate::SemanticStatementOrigin::ProducerResult {
                checked_statement, ..
            } => *checked_statement,
        };
        let fallback = ExecutionConstructionProjectionV3::Checked {
            projection: checked_projection(
                checked,
                CheckedImageRowDomainV2::Statement,
                checked_statement.0 as usize,
            )?,
        };
        statement_routes.push(statement.call_instance.map_or(fallback, |occurrence| {
            ExecutionConstructionProjectionV3::Invocation { occurrence }
        }));
    }

    let image = ExecutionConstructionImageV3 {
        routes,
        expression_routes,
        statement_routes,
    };
    if let Some(started) = trace_started {
        eprintln!(
            "boon_semantic execution_image_v3 owners={} expression_routes={} statement_routes={} elapsed_ms={:.3}",
            image.routes.owner_occurrences.len(),
            image.expression_routes.len(),
            image.statement_routes.len(),
            started.elapsed().as_secs_f64() * 1_000.0,
        );
    }
    Ok(image)
}

#[cfg(test)]
fn checked_execution_projection(
    builder: &mut ExecutionImageHandoffBuilderV2<'_>,
    domain: CheckedImageRowDomainV2,
    dense_index: usize,
) -> Result<PendingExecutionProjectionIdV2, String> {
    let checked = checked_projection(builder.checked, domain, dense_index)?;
    builder.checked(checked)
}

#[cfg(test)]
fn call_instance_projections(
    builder: &mut ExecutionImageHandoffBuilderV2<'_>,
) -> Result<Vec<PendingExecutionProjectionIdV2>, String> {
    let invocation_count = builder.construction_image.routes.invocations.len();
    let mut projections = Vec::<PendingExecutionProjectionIdV2>::with_capacity(invocation_count);
    let mut paths = Vec::<Option<ExecutionInvocationPathIdV2>>::with_capacity(invocation_count);
    for index in 0..invocation_count {
        let occurrence = OutCallInstanceId(index);
        let (overlay_occurrence, parent, root, authored_call_site, definition) = {
            let overlay = builder.construction_image.invocation(occurrence)?;
            (
                overlay.occurrence,
                overlay.parent,
                overlay.root,
                overlay.authored_call_site,
                overlay.definition,
            )
        };
        if overlay_occurrence != occurrence {
            return Err(format!(
                "execution construction invocation {overlay_occurrence} is noncanonical at {index}"
            ));
        }
        let parent_path = parent
            .map(|parent| {
                paths.get(parent.as_usize()).copied().ok_or_else(|| {
                    format!("execution invocation {occurrence} has missing parent {parent}")
                })
            })
            .transpose()?
            .flatten();
        let call_path = authored_call_site
            .map(|call_site| builder.append_path(parent_path, call_site))
            .transpose()?;
        if authored_call_site.is_none() && parent.is_some() {
            return Err(format!(
                "execution invocation {occurrence} has a parent but no authored call site"
            ));
        }
        paths.push(call_path);
        projections.push(builder.invocation(root, definition, call_path)?);
    }
    Ok(projections)
}

#[cfg(test)]
fn v2_projection_for_construction_route(
    builder: &mut ExecutionImageHandoffBuilderV2<'_>,
    invocations: &[PendingExecutionProjectionIdV2],
    route: ExecutionConstructionProjectionV3,
) -> Result<PendingExecutionProjectionIdV2, String> {
    match route {
        ExecutionConstructionProjectionV3::Checked { projection } => builder.checked(projection),
        ExecutionConstructionProjectionV3::Invocation { occurrence } => invocations
            .get(occurrence.as_usize())
            .copied()
            .ok_or_else(|| {
                format!("execution construction route has missing invocation {occurrence}")
            }),
        ExecutionConstructionProjectionV3::Producer {
            identity,
            definition,
        } => builder.invocation(
            DistributedCallOccurrenceRoot::Producer(identity),
            definition,
            None,
        ),
    }
}

#[cfg(test)]
fn route_for_frame(
    frame: Option<OutCallInstanceId>,
    fallback: PendingExecutionProjectionIdV2,
    invocation_projections: &[PendingExecutionProjectionIdV2],
) -> Result<PendingExecutionProjectionIdV2, String> {
    frame.map_or(Ok(fallback), |frame| {
        invocation_projections
            .get(frame.as_usize())
            .copied()
            .ok_or_else(|| format!("execution row references missing invocation frame {frame}"))
    })
}

#[cfg(test)]
fn owner_projections(
    out: &ResolvedOutGraph,
    invocations: &[PendingExecutionProjectionIdV2],
) -> Result<BTreeMap<StaticOwnerId, PendingExecutionProjectionIdV2>, String> {
    let mut projections = BTreeMap::new();
    let mut attach =
        |owner: StaticOwnerId, projection: PendingExecutionProjectionIdV2, context: &str| {
            if let Some(previous) = projections.insert(owner, projection)
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
                .copied()
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
            .copied()
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

#[cfg(test)]
fn route_for_expression(
    builder: &mut ExecutionImageHandoffBuilderV2<'_>,
    execution: &SemanticExecutionImageColumnsV1,
    invocations: &[PendingExecutionProjectionIdV2],
    owner_projections: &BTreeMap<StaticOwnerId, PendingExecutionProjectionIdV2>,
    expression: SemanticExprId,
) -> Result<PendingExecutionProjectionIdV2, String> {
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
    let fallback = checked_execution_projection(
        builder,
        CheckedImageRowDomainV2::Expression,
        definition.checked_expr_id.0 as usize,
    )?;
    let static_projection = definition
        .owner
        .map(|owner| {
            owner_projections.get(&owner).copied().ok_or_else(|| {
                format!("execution expression {expression} has unanchored static owner {owner}")
            })
        })
        .transpose()?;
    let frame_projection = origin
        .call_instance
        .map(|frame| {
            invocations.get(frame.as_usize()).copied().ok_or_else(|| {
                format!("execution expression {expression} has missing invocation frame {frame}")
            })
        })
        .transpose()?;
    match (static_projection, frame_projection) {
        (Some(static_projection), Some(frame_projection)) => {
            match (
                builder.identity(static_projection)?,
                builder.identity(frame_projection)?,
            ) {
                (
                    SemanticImageProjectionIdentityV2::Invocation {
                        root: static_root, ..
                    },
                    SemanticImageProjectionIdentityV2::Invocation {
                        root: frame_root, ..
                    },
                ) => {
                    if static_root != frame_root {
                        return Err(format!(
                            "execution expression {expression} crosses static {static_root:?} and invocation {frame_root:?} roots"
                        ));
                    }
                    // Static ownership is the exact retained-runtime
                    // occurrence. A checked expression origin may sit in an
                    // ancestor invocation of the same program/producer root,
                    // so the static projection owns the row while the frame
                    // remains provenance.
                    Ok(static_projection)
                }
                _ => Err(format!(
                    "execution expression {expression} has non-invocation concrete ownership"
                )),
            }
        }
        (Some(projection), None) | (None, Some(projection)) => Ok(projection),
        (None, None) => Ok(fallback),
    }
}

#[cfg(test)]
fn function_projection(
    builder: &mut ExecutionImageHandoffBuilderV2<'_>,
    execution: &SemanticExecutionImageColumnsV1,
    function: &SemanticFunction,
) -> Result<PendingExecutionProjectionIdV2, String> {
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
        builder.checked,
        CheckedImageRowDomainV2::Callable,
        callable.checked_callable.0 as usize,
    )?;
    let definition_projection = builder.definition_projection(callable_projection)?;
    builder.invocation(
        DistributedCallOccurrenceRoot::Producer(function.identity),
        definition_projection,
        None,
    )
}

#[cfg(test)]
fn checked_execution_projection_v3(
    builder: &mut ExecutionImageHandoffBuilderV3<'_>,
    domain: CheckedImageRowDomainV2,
    dense_index: usize,
) -> Result<PendingExecutionProjectionIdV3, String> {
    let projection = checked_projection(builder.checked, domain, dense_index)?;
    builder.intern(ExecutionConstructionProjectionV3::Checked { projection })
}

#[cfg(test)]
fn projection_for_construction_route_v3(
    builder: &mut ExecutionImageHandoffBuilderV3<'_>,
    route: ExecutionConstructionProjectionV3,
) -> Result<PendingExecutionProjectionIdV3, String> {
    builder.intern(route)
}

fn function_projection_v3(
    builder: &mut ExecutionImageHandoffBuilderV3<'_>,
    execution: &SemanticExecutionImageColumnsV1,
    function: &SemanticFunction,
) -> Result<PendingExecutionProjectionIdV3, String> {
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
        builder.checked,
        CheckedImageRowDomainV2::Callable,
        callable.checked_callable.0 as usize,
    )?;
    let definition = builder
        .construction_image
        .definition_projection(callable_projection)?;
    builder.intern(ExecutionConstructionProjectionV3::Producer {
        identity: function.identity,
        definition,
    })
}

/// Construction transaction for executable image receipts.
///
/// Canonical lowering owns the last mutable form of every executable row.  It
/// publishes the row into this transaction at that point, so sealing does not
/// need to walk the completed semantic and executable images again.  The
/// transaction retains only dense projection routes and compact receipt
/// accumulators; it never owns a second executable graph.
pub(crate) struct ExecutionReceiptPublisherV3<'a> {
    builder: ExecutionImageHandoffBuilderV3<'a>,
    manifest_prefix: crate::dependency_manifest::ManifestCheckedExecutionPrefixBuilderV7,
    checked_projections: Vec<Option<PendingExecutionProjectionIdV3>>,
    invocation_projections: Vec<PendingExecutionProjectionIdV3>,
    expression_routes: Vec<PendingExecutionProjectionIdV3>,
    statement_routes: Vec<PendingExecutionProjectionIdV3>,
    owner_routes: Vec<PendingExecutionProjectionIdV3>,
}

impl<'a> ExecutionReceiptPublisherV3<'a> {
    fn new(
        checked: &'a CheckedImageHandoffV4,
        construction_image: &'a ExecutionConstructionImageV3,
        execution: &SemanticExecutionImageColumnsV1,
    ) -> Result<Self, String> {
        let mut builder = ExecutionImageHandoffBuilderV3::new(checked, construction_image)?;
        let manifest_prefix =
            crate::dependency_manifest::ManifestCheckedExecutionPrefixBuilderV7::new(
                checked, execution,
            )
            .map_err(|error| error.to_string())?;
        let invocation_projections = construction_image
            .routes
            .invocations
            .iter()
            .map(|overlay| {
                builder.intern(ExecutionConstructionProjectionV3::Invocation {
                    occurrence: overlay.occurrence,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let checked_projections = vec![None; checked.projections.len()];
        let mut publisher = Self {
            builder,
            manifest_prefix,
            checked_projections,
            invocation_projections,
            expression_routes: Vec::with_capacity(execution.expressions.len()),
            statement_routes: Vec::with_capacity(execution.statements.len()),
            owner_routes: Vec::with_capacity(execution.static_owners.len()),
        };
        for expression in &execution.expressions {
            let route = construction_image.expression_route(expression.id)?;
            let projection = publisher.projection_for_construction_route(route)?;
            publisher.expression_routes.push(projection);
        }
        for statement in &execution.statements {
            let route = construction_image.statement_route(statement.id)?;
            let projection = publisher.projection_for_construction_route(route)?;
            publisher.statement_routes.push(projection);
        }
        for owner in &execution.static_owners {
            let occurrence = construction_image.owner_occurrence(owner.id)?;
            let projection = publisher
                .invocation_projections
                .get(occurrence.as_usize())
                .copied()
                .ok_or_else(|| format!("static owner route has missing invocation {occurrence}"))?;
            publisher.owner_routes.push(projection);
        }
        Ok(publisher)
    }

    fn checked_projection(
        &mut self,
        projection: CheckedImageProjectionIdV2,
    ) -> Result<PendingExecutionProjectionIdV3, String> {
        let definition = self
            .builder
            .construction_image
            .definition_projection(projection)?;
        if let Some(id) = self
            .checked_projections
            .get(definition.as_usize())
            .copied()
            .flatten()
        {
            return Ok(id);
        }
        let id = self
            .builder
            .intern(ExecutionConstructionProjectionV3::Checked {
                projection: definition,
            })?;
        let slot = self
            .checked_projections
            .get_mut(definition.as_usize())
            .ok_or_else(|| {
                format!(
                    "execution V3 checked projection cache has no definition {}",
                    definition.0
                )
            })?;
        *slot = Some(id);
        Ok(id)
    }

    fn checked_execution_projection(
        &mut self,
        domain: CheckedImageRowDomainV2,
        dense_index: usize,
    ) -> Result<PendingExecutionProjectionIdV3, String> {
        let projection = checked_projection(self.builder.checked, domain, dense_index)?;
        self.checked_projection(projection)
    }

    fn projection_for_construction_route(
        &mut self,
        route: ExecutionConstructionProjectionV3,
    ) -> Result<PendingExecutionProjectionIdV3, String> {
        match route {
            ExecutionConstructionProjectionV3::Checked { projection } => {
                self.checked_projection(projection)
            }
            ExecutionConstructionProjectionV3::Invocation { occurrence } => self
                .invocation_projections
                .get(occurrence.as_usize())
                .copied()
                .ok_or_else(|| {
                    format!("execution construction route has missing invocation {occurrence}")
                }),
            producer @ ExecutionConstructionProjectionV3::Producer { .. } => {
                self.builder.intern(producer)
            }
        }
    }

    fn expression_projection(
        &self,
        expression: SemanticExprId,
    ) -> Result<PendingExecutionProjectionIdV3, String> {
        self.expression_routes
            .get(expression.as_usize())
            .copied()
            .ok_or_else(|| format!("execution V3 references missing expression {expression}"))
    }

    fn statement_projection(
        &self,
        statement: SemanticStatementId,
    ) -> Result<PendingExecutionProjectionIdV3, String> {
        self.statement_routes
            .get(statement.as_usize())
            .copied()
            .ok_or_else(|| format!("execution V3 references missing statement {statement}"))
    }

    pub(crate) fn publish_scopes(
        &mut self,
        execution: &SemanticExecutionImageColumnsV1,
    ) -> Result<(), String> {
        for scope in &execution.scopes {
            let projection = self.checked_execution_projection(
                CheckedImageRowDomainV2::Scope,
                scope.checked_scope.0 as usize,
            )?;
            self.builder.push(
                projection,
                ExecutionImageRowDomainV3::Scope,
                scope,
                Vec::new(),
            )?;
            self.builder.route(
                ExecutionImageRowDomainV3::Scope,
                scope.id.as_usize(),
                projection,
            )?;
        }
        Ok(())
    }

    pub(crate) fn publish_expression<T: Serialize>(
        &mut self,
        execution: &SemanticExecutionImageColumnsV1,
        semantic: &crate::SemanticExpression,
        executable: &T,
    ) -> Result<(), String> {
        let projection = self.expression_projection(semantic.id)?;
        let mut relocations = execution
            .expression_children(&semantic.kind)
            .ok_or_else(|| {
                format!(
                    "execution expression {} has a missing materialization child",
                    semantic.id
                )
            })?
            .into_iter()
            .map(|expression| self.expression_projection(expression))
            .collect::<Result<Vec<_>, _>>()?;
        if let crate::SemanticExpressionKind::Call { callable, .. } = semantic.kind {
            let callable = execution
                .callables
                .get(callable.as_usize())
                .filter(|candidate| candidate.id == callable)
                .ok_or_else(|| {
                    format!("expression {} has missing callable {callable}", semantic.id)
                })?;
            relocations.push(self.checked_execution_projection(
                CheckedImageRowDomainV2::Callable,
                callable.checked_callable.0 as usize,
            )?);
        }
        self.builder.push(
            projection,
            ExecutionImageRowDomainV3::Expression,
            executable,
            relocations,
        )?;
        self.builder.route(
            ExecutionImageRowDomainV3::Expression,
            semantic.id.as_usize(),
            projection,
        )
    }

    pub(crate) fn publish_statement<T: Serialize>(
        &mut self,
        execution: &SemanticExecutionImageColumnsV1,
        semantic: &crate::SemanticStatement,
        executable: &T,
    ) -> Result<(), String> {
        let projection = self.statement_projection(semantic.id)?;
        let mut relocations = semantic
            .value
            .into_iter()
            .chain(semantic.children.iter().filter_map(|child| {
                execution
                    .statements
                    .get(child.as_usize())
                    .and_then(|statement| statement.value)
            }))
            .map(|expression| self.expression_projection(expression))
            .collect::<Result<Vec<_>, _>>()?;
        if let Some(parent) = semantic.parent
            && let Some(parent) = execution.statements.get(parent.as_usize())
            && let Some(value) = parent.value
        {
            relocations.push(self.expression_projection(value)?);
        }
        self.builder.push(
            projection,
            ExecutionImageRowDomainV3::Statement,
            executable,
            relocations,
        )?;
        self.builder.route(
            ExecutionImageRowDomainV3::Statement,
            semantic.id.as_usize(),
            projection,
        )
    }

    pub(crate) fn publish_callables_and_calls(
        &mut self,
        execution: &SemanticExecutionImageColumnsV1,
    ) -> Result<(), String> {
        for callable in &execution.callables {
            let projection = self.checked_execution_projection(
                CheckedImageRowDomainV2::Callable,
                callable.checked_callable.0 as usize,
            )?;
            let relocations = callable
                .semantic_root
                .map(|expression| self.expression_projection(expression))
                .transpose()?
                .into_iter()
                .collect();
            self.builder.push(
                projection,
                ExecutionImageRowDomainV3::Callable,
                callable,
                relocations,
            )?;
            self.builder.route(
                ExecutionImageRowDomainV3::Callable,
                callable.id.as_usize(),
                projection,
            )?;
        }
        for call in &execution.calls {
            let projection = self.checked_execution_projection(
                CheckedImageRowDomainV2::Call,
                call.checked_call.0 as usize,
            )?;
            let callable = execution
                .callables
                .get(call.callable.as_usize())
                .filter(|candidate| candidate.id == call.callable)
                .ok_or_else(|| format!("execution call {} has missing callable", call.id))?;
            let relocations = vec![self.checked_execution_projection(
                CheckedImageRowDomainV2::Callable,
                callable.checked_callable.0 as usize,
            )?];
            self.builder.push(
                projection,
                ExecutionImageRowDomainV3::Call,
                call,
                relocations,
            )?;
            self.builder.route(
                ExecutionImageRowDomainV3::Call,
                call.id.as_usize(),
                projection,
            )?;
        }
        Ok(())
    }

    pub(crate) fn publish_call_occurrence<T: Serialize>(
        &mut self,
        execution: &SemanticExecutionImageColumnsV1,
        semantic: &crate::SemanticCallOccurrence,
        executable: &T,
    ) -> Result<(), String> {
        let projection = self
            .invocation_projections
            .get(semantic.id.as_usize())
            .copied()
            .ok_or_else(|| format!("call occurrence {} has no V3 overlay", semantic.id))?;
        let mut relocations = semantic
            .parent
            .and_then(|parent| self.invocation_projections.get(parent.as_usize()).copied())
            .into_iter()
            .collect::<Vec<_>>();
        if let Some(call) = semantic.call {
            let call = execution
                .calls
                .get(call.as_usize())
                .filter(|candidate| candidate.id == call)
                .ok_or_else(|| format!("call occurrence {} has missing call", semantic.id))?;
            relocations.push(self.checked_execution_projection(
                CheckedImageRowDomainV2::Call,
                call.checked_call.0 as usize,
            )?);
        }
        self.builder.push(
            projection,
            ExecutionImageRowDomainV3::CallOccurrence,
            executable,
            relocations,
        )?;
        self.builder.route(
            ExecutionImageRowDomainV3::CallOccurrence,
            semantic.id.as_usize(),
            projection,
        )
    }

    pub(crate) fn publish_source<T: Serialize>(
        &mut self,
        semantic: &crate::SemanticSourceDef,
        executable: &T,
    ) -> Result<(), String> {
        let fallback = self.expression_projection(semantic.expression)?;
        let projection = semantic.call_instance.map_or(Ok(fallback), |occurrence| {
            self.invocation_projections
                .get(occurrence.as_usize())
                .copied()
                .ok_or_else(|| format!("source {} has missing invocation", semantic.id))
        })?;
        self.builder.push(
            projection,
            ExecutionImageRowDomainV3::Source,
            executable,
            vec![fallback],
        )?;
        self.builder.route(
            ExecutionImageRowDomainV3::Source,
            semantic.id.as_usize(),
            projection,
        )
    }

    pub(crate) fn publish_state<T: Serialize>(
        &mut self,
        semantic: &crate::SemanticStateDef,
        executable: &T,
    ) -> Result<(), String> {
        let fallback = self.expression_projection(semantic.expression)?;
        let projection = semantic.call_instance.map_or(Ok(fallback), |occurrence| {
            self.invocation_projections
                .get(occurrence.as_usize())
                .copied()
                .ok_or_else(|| format!("state {} has missing invocation", semantic.id))
        })?;
        let mut relocations = vec![fallback, self.expression_projection(semantic.initial)?];
        if let crate::SemanticStateLifetimeV1::ActivationLocal { then_expression } =
            semantic.lifetime
        {
            relocations.push(self.expression_projection(then_expression)?);
        }
        self.builder.push(
            projection,
            ExecutionImageRowDomainV3::State,
            executable,
            relocations,
        )?;
        self.builder.route(
            ExecutionImageRowDomainV3::State,
            semantic.id.as_usize(),
            projection,
        )
    }

    pub(crate) fn publish_root<T: Serialize>(
        &mut self,
        semantic: &crate::SemanticRoot,
        executable: &T,
    ) -> Result<(), String> {
        let projection = self.expression_projection(semantic.expression)?;
        self.builder.push(
            projection,
            ExecutionImageRowDomainV3::Root,
            executable,
            vec![projection],
        )?;
        self.builder.route(
            ExecutionImageRowDomainV3::Root,
            semantic.ordinal,
            projection,
        )
    }

    pub(crate) fn publish_function<T: Serialize>(
        &mut self,
        execution: &SemanticExecutionImageColumnsV1,
        semantic: &SemanticFunction,
        executable: &T,
        dense_index: usize,
    ) -> Result<(), String> {
        let projection = function_projection_v3(&mut self.builder, execution, semantic)?;
        let mut relocations = vec![self.expression_projection(semantic.root)?];
        if let Some(source) = semantic.invocation_source {
            relocations.push(self.expression_projection(source)?);
        }
        self.builder.push(
            projection,
            ExecutionImageRowDomainV3::Function,
            executable,
            relocations,
        )?;
        self.builder
            .route(ExecutionImageRowDomainV3::Function, dense_index, projection)
    }

    pub(crate) fn publish_materialization<T: Serialize>(
        &mut self,
        semantic: &crate::SemanticContextualMaterialization,
        executable: &T,
    ) -> Result<(), String> {
        let projection = self
            .owner_routes
            .get(semantic.owner.as_usize())
            .copied()
            .unwrap_or(self.expression_projection(semantic.source)?);
        let relocations = semantic
            .expression_roots()
            .into_iter()
            .map(|expression| self.expression_projection(expression))
            .collect::<Result<Vec<_>, _>>()?;
        self.builder.push(
            projection,
            ExecutionImageRowDomainV3::Materialization,
            executable,
            relocations,
        )?;
        self.builder.route(
            ExecutionImageRowDomainV3::Materialization,
            semantic.id.as_usize(),
            projection,
        )
    }

    pub(crate) fn publish_static_owner<T: Serialize>(
        &mut self,
        semantic: &crate::SemanticStaticOwner,
        executable: &T,
    ) -> Result<(), String> {
        let projection = self
            .owner_routes
            .get(semantic.id.as_usize())
            .copied()
            .ok_or_else(|| format!("static owner {} has no V3 route", semantic.id))?;
        let relocations = semantic
            .parent
            .and_then(|parent| self.owner_routes.get(parent.as_usize()).copied())
            .into_iter()
            .collect();
        self.builder.push(
            projection,
            ExecutionImageRowDomainV3::StaticOwner,
            executable,
            relocations,
        )?;
        self.builder.route(
            ExecutionImageRowDomainV3::StaticOwner,
            semantic.id.as_usize(),
            projection,
        )
    }

    pub(crate) fn finish(
        self,
    ) -> Result<
        (
            ExecutionImageHandoffV3,
            crate::dependency_manifest::ManifestCheckedExecutionPrefixV7,
        ),
        String,
    > {
        let source_bundle_digest_v1 = self.builder.checked.source_bundle_digest_v1;
        let role = self.builder.checked.role;
        self.builder
            .finish(source_bundle_digest_v1, role, self.manifest_prefix)
    }
}

#[cfg(test)]
fn execution_image_handoff_v3(
    checked: &CheckedImageHandoffV4,
    construction_image: &ExecutionConstructionImageV3,
    execution: &SemanticExecutionImageColumnsV1,
    core: &crate::program_core::CanonicalProgramCoreV2,
    payload_seals: &ExecutionRowPayloadSealsV3,
) -> Result<ExecutionImageHandoffV3, String> {
    let trace = std::env::var_os("BOON_SEMANTIC_TRACE").is_some();
    let mut trace_started = std::time::Instant::now();
    let executable = &core.executable;
    if executable.expressions.len() != execution.expressions.len()
        || executable.statements.len() != execution.statements.len()
        || executable.sources.len() != execution.sources.len()
        || executable.states.len() != execution.states.len()
        || executable.roots.len() != execution.roots.len()
        || executable.functions.len() != execution.functions.len()
        || executable.call_occurrences.len() != execution.call_occurrences.len()
        || core.materializations.len() != execution.materializations.len()
        || core.scope_index.owners.len() != execution.static_owners.len()
        || payload_seals.expressions.len() != executable.expressions.len()
        || payload_seals.statements.len() != executable.statements.len()
        || payload_seals.call_occurrences.len() != executable.call_occurrences.len()
        || payload_seals.sources.len() != executable.sources.len()
        || payload_seals.states.len() != executable.states.len()
        || payload_seals.roots.len() != executable.roots.len()
        || payload_seals.functions.len() != executable.functions.len()
        || payload_seals.materializations.len() != core.materializations.len()
        || payload_seals.static_owners.len() != core.scope_index.owners.len()
    {
        return Err("execution V3 receipts do not exactly cover final executable rows".to_owned());
    }

    let mut builder = ExecutionImageHandoffBuilderV3::new(checked, construction_image)?;
    let invocation_projections = construction_image
        .routes
        .invocations
        .iter()
        .map(|overlay| {
            builder.intern(ExecutionConstructionProjectionV3::Invocation {
                occurrence: overlay.occurrence,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let expression_routes = execution
        .expressions
        .iter()
        .map(|expression| construction_image.expression_route(expression.id))
        .map(|route| projection_for_construction_route_v3(&mut builder, route?))
        .collect::<Result<Vec<_>, _>>()?;
    let statement_routes = execution
        .statements
        .iter()
        .map(|statement| construction_image.statement_route(statement.id))
        .map(|route| projection_for_construction_route_v3(&mut builder, route?))
        .collect::<Result<Vec<_>, _>>()?;
    let owner_routes = execution
        .static_owners
        .iter()
        .map(|owner| {
            let occurrence = construction_image.owner_occurrence(owner.id)?;
            invocation_projections
                .get(occurrence.as_usize())
                .copied()
                .ok_or_else(|| format!("static owner route has missing invocation {occurrence}"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let expression_projection = |id: SemanticExprId| {
        expression_routes
            .get(id.as_usize())
            .copied()
            .ok_or_else(|| format!("execution V3 references missing expression {id}"))
    };
    let statement_projection = |id: SemanticStatementId| {
        statement_routes
            .get(id.as_usize())
            .copied()
            .ok_or_else(|| format!("execution V3 references missing statement {id}"))
    };
    trace_execution_handoff_phase(trace, "v3_projection_setup", &mut trace_started);

    for scope in &execution.scopes {
        let projection = checked_execution_projection_v3(
            &mut builder,
            CheckedImageRowDomainV2::Scope,
            scope.checked_scope.0 as usize,
        )?;
        builder.push(
            projection,
            ExecutionImageRowDomainV3::Scope,
            scope,
            Vec::new(),
        )?;
        builder.route(
            ExecutionImageRowDomainV3::Scope,
            scope.id.as_usize(),
            projection,
        )?;
    }
    trace_execution_handoff_phase(trace, "v3_scope_rows", &mut trace_started);

    for (semantic, executable) in execution.expressions.iter().zip(&executable.expressions) {
        if executable.id.as_usize() != semantic.id.as_usize() {
            return Err(format!(
                "execution expression {} maps to non-dense executable {}",
                semantic.id, executable.id
            ));
        }
        let projection = expression_projection(semantic.id)?;
        let mut relocations = execution
            .expression_children(&semantic.kind)
            .ok_or_else(|| {
                format!(
                    "execution expression {} has a missing materialization child",
                    semantic.id
                )
            })?
            .into_iter()
            .map(expression_projection)
            .collect::<Result<Vec<_>, _>>()?;
        if let crate::SemanticExpressionKind::Call { callable, .. } = semantic.kind {
            let callable = execution
                .callables
                .get(callable.as_usize())
                .filter(|candidate| candidate.id == callable)
                .ok_or_else(|| {
                    format!("expression {} has missing callable {callable}", semantic.id)
                })?;
            relocations.push(checked_execution_projection_v3(
                &mut builder,
                CheckedImageRowDomainV2::Callable,
                callable.checked_callable.0 as usize,
            )?);
        }
        let payload_digest = execution_payload_seal_v3(
            &payload_seals.expressions,
            ExecutionImageRowDomainV3::Expression,
            semantic.id.as_usize(),
        )?;
        builder.push_presealed(
            projection,
            ExecutionImageRowDomainV3::Expression,
            payload_digest,
            relocations,
        )?;
        builder.route(
            ExecutionImageRowDomainV3::Expression,
            semantic.id.as_usize(),
            projection,
        )?;
    }
    trace_execution_handoff_phase(trace, "v3_expression_rows", &mut trace_started);

    for (semantic, executable) in execution.statements.iter().zip(&executable.statements) {
        if executable.id.as_usize() != semantic.id.as_usize() {
            return Err(format!(
                "execution statement {} maps to non-dense executable {}",
                semantic.id, executable.id
            ));
        }
        let projection = statement_projection(semantic.id)?;
        let mut relocations = semantic
            .value
            .into_iter()
            .chain(semantic.children.iter().filter_map(|child| {
                execution
                    .statements
                    .get(child.as_usize())
                    .and_then(|statement| statement.value)
            }))
            .map(expression_projection)
            .collect::<Result<Vec<_>, _>>()?;
        if let Some(parent) = semantic.parent
            && let Some(parent) = execution.statements.get(parent.as_usize())
            && let Some(value) = parent.value
        {
            relocations.push(expression_projection(value)?);
        }
        let payload_digest = execution_payload_seal_v3(
            &payload_seals.statements,
            ExecutionImageRowDomainV3::Statement,
            semantic.id.as_usize(),
        )?;
        builder.push_presealed(
            projection,
            ExecutionImageRowDomainV3::Statement,
            payload_digest,
            relocations,
        )?;
        builder.route(
            ExecutionImageRowDomainV3::Statement,
            semantic.id.as_usize(),
            projection,
        )?;
    }
    trace_execution_handoff_phase(trace, "v3_statement_rows", &mut trace_started);

    for callable in &execution.callables {
        let projection = checked_execution_projection_v3(
            &mut builder,
            CheckedImageRowDomainV2::Callable,
            callable.checked_callable.0 as usize,
        )?;
        let relocations = callable
            .semantic_root
            .map(expression_projection)
            .transpose()?
            .into_iter()
            .collect();
        builder.push(
            projection,
            ExecutionImageRowDomainV3::Callable,
            callable,
            relocations,
        )?;
        builder.route(
            ExecutionImageRowDomainV3::Callable,
            callable.id.as_usize(),
            projection,
        )?;
    }
    for call in &execution.calls {
        let projection = checked_execution_projection_v3(
            &mut builder,
            CheckedImageRowDomainV2::Call,
            call.checked_call.0 as usize,
        )?;
        let callable = execution
            .callables
            .get(call.callable.as_usize())
            .filter(|candidate| candidate.id == call.callable)
            .ok_or_else(|| format!("execution call {} has missing callable", call.id))?;
        let relocations = vec![checked_execution_projection_v3(
            &mut builder,
            CheckedImageRowDomainV2::Callable,
            callable.checked_callable.0 as usize,
        )?];
        builder.push(
            projection,
            ExecutionImageRowDomainV3::Call,
            call,
            relocations,
        )?;
        builder.route(
            ExecutionImageRowDomainV3::Call,
            call.id.as_usize(),
            projection,
        )?;
    }
    trace_execution_handoff_phase(trace, "v3_callable_call_rows", &mut trace_started);

    for (semantic, executable) in execution
        .call_occurrences
        .iter()
        .zip(&executable.call_occurrences)
    {
        if executable.id != semantic.id.as_usize() {
            return Err(format!(
                "call occurrence {} maps to executable {}",
                semantic.id, executable.id
            ));
        }
        let projection = invocation_projections
            .get(semantic.id.as_usize())
            .copied()
            .ok_or_else(|| format!("call occurrence {} has no V3 overlay", semantic.id))?;
        let mut relocations = semantic
            .parent
            .and_then(|parent| invocation_projections.get(parent.as_usize()).copied())
            .into_iter()
            .collect::<Vec<_>>();
        if let Some(call) = semantic.call {
            let call = execution
                .calls
                .get(call.as_usize())
                .filter(|candidate| candidate.id == call)
                .ok_or_else(|| format!("call occurrence {} has missing call", semantic.id))?;
            relocations.push(checked_execution_projection_v3(
                &mut builder,
                CheckedImageRowDomainV2::Call,
                call.checked_call.0 as usize,
            )?);
        }
        let payload_digest = execution_payload_seal_v3(
            &payload_seals.call_occurrences,
            ExecutionImageRowDomainV3::CallOccurrence,
            semantic.id.as_usize(),
        )?;
        builder.push_presealed(
            projection,
            ExecutionImageRowDomainV3::CallOccurrence,
            payload_digest,
            relocations,
        )?;
        builder.route(
            ExecutionImageRowDomainV3::CallOccurrence,
            semantic.id.as_usize(),
            projection,
        )?;
    }
    trace_execution_handoff_phase(trace, "v3_occurrence_rows", &mut trace_started);

    for semantic in &execution.sources {
        let fallback = expression_projection(semantic.expression)?;
        let projection = semantic.call_instance.map_or(Ok(fallback), |occurrence| {
            invocation_projections
                .get(occurrence.as_usize())
                .copied()
                .ok_or_else(|| format!("source {} has missing invocation", semantic.id))
        })?;
        let payload_digest = execution_payload_seal_v3(
            &payload_seals.sources,
            ExecutionImageRowDomainV3::Source,
            semantic.id.as_usize(),
        )?;
        builder.push_presealed(
            projection,
            ExecutionImageRowDomainV3::Source,
            payload_digest,
            vec![fallback],
        )?;
        builder.route(
            ExecutionImageRowDomainV3::Source,
            semantic.id.as_usize(),
            projection,
        )?;
    }
    for semantic in &execution.states {
        let fallback = expression_projection(semantic.expression)?;
        let projection = semantic.call_instance.map_or(Ok(fallback), |occurrence| {
            invocation_projections
                .get(occurrence.as_usize())
                .copied()
                .ok_or_else(|| format!("state {} has missing invocation", semantic.id))
        })?;
        let mut relocations = vec![fallback, expression_projection(semantic.initial)?];
        if let crate::SemanticStateLifetimeV1::ActivationLocal { then_expression } =
            semantic.lifetime
        {
            relocations.push(expression_projection(then_expression)?);
        }
        let payload_digest = execution_payload_seal_v3(
            &payload_seals.states,
            ExecutionImageRowDomainV3::State,
            semantic.id.as_usize(),
        )?;
        builder.push_presealed(
            projection,
            ExecutionImageRowDomainV3::State,
            payload_digest,
            relocations,
        )?;
        builder.route(
            ExecutionImageRowDomainV3::State,
            semantic.id.as_usize(),
            projection,
        )?;
    }
    trace_execution_handoff_phase(trace, "v3_resource_rows", &mut trace_started);

    for (index, semantic) in execution.roots.iter().enumerate() {
        let projection = expression_projection(semantic.expression)?;
        let payload_digest = execution_payload_seal_v3(
            &payload_seals.roots,
            ExecutionImageRowDomainV3::Root,
            index,
        )?;
        builder.push_presealed(
            projection,
            ExecutionImageRowDomainV3::Root,
            payload_digest,
            vec![projection],
        )?;
        builder.route(
            ExecutionImageRowDomainV3::Root,
            semantic.ordinal,
            projection,
        )?;
    }
    for (index, semantic) in execution.functions.iter().enumerate() {
        let projection = function_projection_v3(&mut builder, execution, semantic)?;
        let mut relocations = vec![expression_projection(semantic.root)?];
        if let Some(source) = semantic.invocation_source {
            relocations.push(expression_projection(source)?);
        }
        let payload_digest = execution_payload_seal_v3(
            &payload_seals.functions,
            ExecutionImageRowDomainV3::Function,
            index,
        )?;
        builder.push_presealed(
            projection,
            ExecutionImageRowDomainV3::Function,
            payload_digest,
            relocations,
        )?;
        builder.route(ExecutionImageRowDomainV3::Function, index, projection)?;
    }
    for semantic in &execution.materializations {
        let projection = owner_routes
            .get(semantic.owner.as_usize())
            .copied()
            .unwrap_or(expression_projection(semantic.source)?);
        let relocations = semantic
            .expression_roots()
            .into_iter()
            .map(expression_projection)
            .collect::<Result<Vec<_>, _>>()?;
        let payload_digest = execution_payload_seal_v3(
            &payload_seals.materializations,
            ExecutionImageRowDomainV3::Materialization,
            semantic.id.as_usize(),
        )?;
        builder.push_presealed(
            projection,
            ExecutionImageRowDomainV3::Materialization,
            payload_digest,
            relocations,
        )?;
        builder.route(
            ExecutionImageRowDomainV3::Materialization,
            semantic.id.as_usize(),
            projection,
        )?;
    }
    for (semantic, executable) in execution.static_owners.iter().zip(&core.scope_index.owners) {
        if (semantic.id, semantic.parent, semantic.child_ordinal)
            != (executable.id, executable.parent, executable.child_ordinal)
        {
            return Err(format!(
                "static owner {} disagrees with executable owner",
                semantic.id
            ));
        }
        let projection = owner_routes
            .get(semantic.id.as_usize())
            .copied()
            .ok_or_else(|| format!("static owner {} has no V3 route", semantic.id))?;
        let relocations = semantic
            .parent
            .and_then(|parent| owner_routes.get(parent.as_usize()).copied())
            .into_iter()
            .collect();
        let payload_digest = execution_payload_seal_v3(
            &payload_seals.static_owners,
            ExecutionImageRowDomainV3::StaticOwner,
            semantic.id.as_usize(),
        )?;
        builder.push_presealed(
            projection,
            ExecutionImageRowDomainV3::StaticOwner,
            payload_digest,
            relocations,
        )?;
        builder.route(
            ExecutionImageRowDomainV3::StaticOwner,
            semantic.id.as_usize(),
            projection,
        )?;
    }
    trace_execution_handoff_phase(trace, "v3_final_domains", &mut trace_started);
    let manifest_prefix = crate::dependency_manifest::ManifestCheckedExecutionPrefixBuilderV7::new(
        checked, execution,
    )
    .map_err(|error| error.to_string())?;
    builder
        .finish(
            checked.source_bundle_digest_v1,
            checked.role,
            manifest_prefix,
        )
        .map(|(handoff, _)| handoff)
}

#[cfg(test)]
fn execution_payload_seal_v3(
    seals: &[[u8; 32]],
    domain: ExecutionImageRowDomainV3,
    dense_index: usize,
) -> Result<[u8; 32], String> {
    seals.get(dense_index).copied().ok_or_else(|| {
        format!("execution V3 {domain:?} row {dense_index} has no construction-owned payload seal")
    })
}

#[cfg(test)]
#[derive(Clone, Debug, Eq, PartialEq)]
enum ExecutionRouteOracleOwner {
    Checked(boon_checked::CheckedShardOwnerKeyV2),
    Invocation {
        root: DistributedCallOccurrenceRoot,
        definition: boon_checked::CheckedShardOwnerKeyV2,
        path_digest: Option<[u8; 32]>,
    },
}

#[cfg(test)]
fn checked_route_oracle_owner(
    checked: &CheckedImageHandoffV4,
    projection: CheckedImageProjectionIdV2,
) -> Result<boon_checked::CheckedShardOwnerKeyV2, String> {
    checked
        .projection(projection)
        .map(|projection| projection.stable_key.owner.clone())
        .ok_or_else(|| format!("route oracle has no checked projection {}", projection.0))
}

#[cfg(test)]
fn v3_route_oracle_owner(
    handoff: &ExecutionImageHandoffV3,
    checked: &CheckedImageHandoffV4,
    projection: ExecutionImageProjectionIdV3,
) -> Result<ExecutionRouteOracleOwner, String> {
    let projection = handoff
        .projection(projection)
        .ok_or_else(|| "route oracle has no V3 projection".to_owned())?;
    Ok(match projection.identity {
        ExecutionConstructionProjectionV3::Checked { projection } => {
            ExecutionRouteOracleOwner::Checked(checked_route_oracle_owner(checked, projection)?)
        }
        ExecutionConstructionProjectionV3::Invocation { occurrence } => {
            let overlay = handoff
                .invocation(occurrence)
                .ok_or_else(|| format!("route oracle has no V3 overlay {occurrence}"))?;
            ExecutionRouteOracleOwner::Invocation {
                root: overlay.root,
                definition: checked_route_oracle_owner(checked, overlay.definition)?,
                path_digest: overlay.stable_path_digest,
            }
        }
        ExecutionConstructionProjectionV3::Producer {
            identity,
            definition,
        } => ExecutionRouteOracleOwner::Invocation {
            root: DistributedCallOccurrenceRoot::Producer(identity),
            definition: checked_route_oracle_owner(checked, definition)?,
            path_digest: None,
        },
    })
}

#[cfg(test)]
fn v2_route_oracle_owner(
    handoff: &ExecutionImageHandoffV2,
    checked: &CheckedImageHandoffV4,
    projection: ExecutionImageProjectionIdV2,
) -> Result<ExecutionRouteOracleOwner, String> {
    let projection = handoff
        .projection(projection)
        .ok_or_else(|| "route oracle has no V2 projection".to_owned())?;
    Ok(match projection.identity {
        SemanticImageProjectionIdentityV2::Checked { projection } => {
            ExecutionRouteOracleOwner::Checked(checked_route_oracle_owner(checked, projection)?)
        }
        SemanticImageProjectionIdentityV2::Invocation {
            root,
            definition,
            call_path,
        } => ExecutionRouteOracleOwner::Invocation {
            root,
            definition: checked_route_oracle_owner(checked, definition)?,
            path_digest: call_path
                .map(|path| {
                    handoff
                        .invocation_paths
                        .get(path.as_usize())
                        .map(|path| path.stable_path_digest)
                        .ok_or_else(|| format!("route oracle has no V2 path {}", path.0))
                })
                .transpose()?,
        },
    })
}

#[cfg(test)]
fn v3_domain_in_v2(domain: ExecutionImageRowDomainV3) -> ExecutionImageRowDomainV2 {
    match domain {
        ExecutionImageRowDomainV3::Scope => ExecutionImageRowDomainV2::Scope,
        ExecutionImageRowDomainV3::Expression => ExecutionImageRowDomainV2::Expression,
        ExecutionImageRowDomainV3::Statement => ExecutionImageRowDomainV2::Statement,
        ExecutionImageRowDomainV3::Callable => ExecutionImageRowDomainV2::Callable,
        ExecutionImageRowDomainV3::Call => ExecutionImageRowDomainV2::Call,
        ExecutionImageRowDomainV3::CallOccurrence => ExecutionImageRowDomainV2::CallOccurrence,
        ExecutionImageRowDomainV3::Source => ExecutionImageRowDomainV2::Source,
        ExecutionImageRowDomainV3::State => ExecutionImageRowDomainV2::State,
        ExecutionImageRowDomainV3::Root => ExecutionImageRowDomainV2::Root,
        ExecutionImageRowDomainV3::Function => ExecutionImageRowDomainV2::Function,
        ExecutionImageRowDomainV3::Materialization => ExecutionImageRowDomainV2::Materialization,
        ExecutionImageRowDomainV3::StaticOwner => ExecutionImageRowDomainV2::StaticOwner,
    }
}

#[cfg(test)]
fn validate_v3_routes_against_v2_oracle(
    handoff: &ExecutionImageHandoffV3,
    oracle: &ExecutionImageHandoffV2,
    checked: &CheckedImageHandoffV4,
) -> Result<(), String> {
    for route in &handoff.entity_routes {
        let oracle_projection = oracle
            .entity_projection(v3_domain_in_v2(route.domain), route.dense_index as usize)
            .ok_or_else(|| {
                format!(
                    "V2 route oracle has no {:?} {}",
                    route.domain, route.dense_index
                )
            })?;
        let actual = v3_route_oracle_owner(handoff, checked, route.projection)?;
        let expected = v2_route_oracle_owner(oracle, checked, oracle_projection)?;
        if actual != expected {
            return Err(format!(
                "V3 {:?} {} route owner differs from the V2 oracle: expected {expected:?}, got {actual:?}",
                route.domain, route.dense_index
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
fn trace_execution_handoff_phase(enabled: bool, name: &str, started: &mut std::time::Instant) {
    if enabled {
        eprintln!(
            "boon_semantic execution_handoff phase={name} elapsed_ms={:.3}",
            started.elapsed().as_secs_f64() * 1_000.0
        );
        *started = std::time::Instant::now();
    }
}

#[cfg(test)]
fn execution_image_handoff_v2_oracle(
    checked: &CheckedImageHandoffV4,
    construction_image: &ExecutionConstructionImageV3,
    _out: &ResolvedOutGraph,
    execution: &SemanticExecutionImageColumnsV1,
) -> Result<ExecutionImageHandoffV2, String> {
    let trace_handoff = std::env::var_os("BOON_SEMANTIC_TRACE").is_some();
    let mut trace_started = std::time::Instant::now();
    if trace_handoff {
        eprintln!(
            "boon_semantic execution_routes_v3 receipt_prefix={:02x}{:02x}{:02x}{:02x}",
            construction_image.routes.local_digest[0],
            construction_image.routes.local_digest[1],
            construction_image.routes.local_digest[2],
            construction_image.routes.local_digest[3],
        );
    }
    let mut builder = ExecutionImageHandoffBuilderV2::new(checked, construction_image)?;
    let invocations = call_instance_projections(&mut builder)?;
    let owner_projection_routes = execution
        .static_owners
        .iter()
        .map(|owner| {
            let occurrence = construction_image.owner_occurrence(owner.id)?;
            invocations
                .get(occurrence.as_usize())
                .copied()
                .ok_or_else(|| format!("static owner route has missing invocation {occurrence}"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    trace_execution_handoff_phase(trace_handoff, "projection_setup", &mut trace_started);

    let expression_routes = execution
        .expressions
        .iter()
        .map(|expression| construction_image.expression_route(expression.id))
        .map(|route| v2_projection_for_construction_route(&mut builder, &invocations, route?))
        .collect::<Result<Vec<_>, _>>()?;
    let expression_projection = |id: SemanticExprId| {
        expression_routes
            .get(id.as_usize())
            .copied()
            .ok_or_else(|| format!("execution image references missing expression {id}"))
    };
    if trace_handoff {
        let checked_routes = expression_routes
            .iter()
            .filter(|route| {
                matches!(
                    builder.identity(**route),
                    Ok(SemanticImageProjectionIdentityV2::Checked { .. })
                )
            })
            .count();
        eprintln!(
            "boon_semantic execution_handoff expression_routes checked={} invocation={}",
            checked_routes,
            expression_routes.len() - checked_routes,
        );
    }
    trace_execution_handoff_phase(trace_handoff, "expression_routes", &mut trace_started);
    let statement_routes = execution
        .statements
        .iter()
        .map(|statement| construction_image.statement_route(statement.id))
        .map(|route| v2_projection_for_construction_route(&mut builder, &invocations, route?))
        .collect::<Result<Vec<_>, _>>()?;
    let statement_projection = |id: SemanticStatementId| {
        execution
            .statements
            .get(id.as_usize())
            .filter(|statement| statement.id == id)
            .and_then(|_| statement_routes.get(id.as_usize()))
            .copied()
            .ok_or_else(|| format!("execution image references missing statement {id}"))
    };
    #[cfg(test)]
    {
        let legacy_owner_projections = owner_projections(_out, &invocations)?;
        let legacy_expression_routes = execution
            .expressions
            .iter()
            .map(|expression| {
                route_for_expression(
                    &mut builder,
                    execution,
                    &invocations,
                    &legacy_owner_projections,
                    expression.id,
                )
            })
            .collect::<Result<Vec<_>, _>>()?;
        if legacy_expression_routes != expression_routes {
            return Err(
                "execution V3 expression routes disagree with the independent V2 route oracle"
                    .to_owned(),
            );
        }
        let legacy_statement_routes = execution
            .statements
            .iter()
            .map(|statement| {
                let checked_statement = match &statement.origin {
                    crate::SemanticStatementOrigin::Checked { statement } => *statement,
                    crate::SemanticStatementOrigin::ProducerResult {
                        checked_statement, ..
                    } => *checked_statement,
                };
                let fallback = checked_execution_projection(
                    &mut builder,
                    CheckedImageRowDomainV2::Statement,
                    checked_statement.0 as usize,
                )?;
                route_for_frame(statement.call_instance, fallback, &invocations)
            })
            .collect::<Result<Vec<_>, String>>()?;
        if legacy_statement_routes != statement_routes {
            return Err(
                "execution V3 statement routes disagree with the independent V2 route oracle"
                    .to_owned(),
            );
        }
    }
    trace_execution_handoff_phase(trace_handoff, "statement_routes", &mut trace_started);

    for scope in &execution.scopes {
        let projection = checked_execution_projection(
            &mut builder,
            CheckedImageRowDomainV2::Scope,
            scope.checked_scope.0 as usize,
        )?;
        builder.push(
            projection,
            ExecutionImageRowDomainV2::Scope,
            scope,
            Vec::new(),
        )?;
        builder.route(
            ExecutionImageRowDomainV2::Scope,
            scope.id.as_usize(),
            projection,
        )?;
    }
    trace_execution_handoff_phase(trace_handoff, "scope_rows", &mut trace_started);
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
            relocations.push(checked_execution_projection(
                &mut builder,
                CheckedImageRowDomainV2::Callable,
                callable.checked_callable.0 as usize,
            )?);
        }
        builder.push(
            projection,
            ExecutionImageRowDomainV2::Expression,
            expression,
            relocations,
        )?;
        builder.route(
            ExecutionImageRowDomainV2::Expression,
            expression.id.as_usize(),
            projection,
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
            relocations.push(invocations.get(frame.as_usize()).copied().ok_or_else(|| {
                format!(
                    "expression origin {} references missing invocation frame {frame}",
                    origin.expression
                )
            })?);
        }
        builder.push(
            projection,
            ExecutionImageRowDomainV2::ExpressionOrigin,
            origin,
            relocations,
        )?;
        builder.route(
            ExecutionImageRowDomainV2::ExpressionOrigin,
            origin.expression.as_usize(),
            projection,
        )?;
    }
    trace_execution_handoff_phase(
        trace_handoff,
        "expression_and_origin_rows",
        &mut trace_started,
    );
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
            projection,
            ExecutionImageRowDomainV2::Statement,
            statement,
            relocations,
        )?;
        builder.route(
            ExecutionImageRowDomainV2::Statement,
            statement.id.as_usize(),
            projection,
        )?;
    }
    trace_execution_handoff_phase(trace_handoff, "statement_rows", &mut trace_started);
    for callable in &execution.callables {
        let projection = checked_execution_projection(
            &mut builder,
            CheckedImageRowDomainV2::Callable,
            callable.checked_callable.0 as usize,
        )?;
        let relocations = callable
            .semantic_root
            .map(expression_projection)
            .transpose()?
            .into_iter()
            .collect();
        builder.push(
            projection,
            ExecutionImageRowDomainV2::Callable,
            callable,
            relocations,
        )?;
        builder.route(
            ExecutionImageRowDomainV2::Callable,
            callable.id.as_usize(),
            projection,
        )?;
    }
    trace_execution_handoff_phase(trace_handoff, "callable_rows", &mut trace_started);
    for call in &execution.calls {
        let projection = checked_execution_projection(
            &mut builder,
            CheckedImageRowDomainV2::Call,
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
        let relocations = vec![checked_execution_projection(
            &mut builder,
            CheckedImageRowDomainV2::Callable,
            callable.checked_callable.0 as usize,
        )?];
        builder.push(
            projection,
            ExecutionImageRowDomainV2::Call,
            call,
            relocations,
        )?;
        builder.route(
            ExecutionImageRowDomainV2::Call,
            call.id.as_usize(),
            projection,
        )?;
    }
    trace_execution_handoff_phase(trace_handoff, "call_rows", &mut trace_started);
    for occurrence in &execution.call_occurrences {
        let projection = invocations
            .get(occurrence.id.as_usize())
            .copied()
            .ok_or_else(|| {
                format!(
                    "call occurrence {} has no invocation projection",
                    occurrence.id
                )
            })?;
        let mut relocations = occurrence
            .parent
            .and_then(|parent| invocations.get(parent.as_usize()).copied())
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
            relocations.push(checked_execution_projection(
                &mut builder,
                CheckedImageRowDomainV2::Call,
                call.checked_call.0 as usize,
            )?);
        }
        builder.push(
            projection,
            ExecutionImageRowDomainV2::CallOccurrence,
            occurrence,
            relocations,
        )?;
        builder.route(
            ExecutionImageRowDomainV2::CallOccurrence,
            occurrence.id.as_usize(),
            projection,
        )?;
    }
    trace_execution_handoff_phase(trace_handoff, "call_occurrence_rows", &mut trace_started);
    for source in &execution.sources {
        let projection = route_for_frame(
            source.call_instance,
            expression_projection(source.expression)?,
            &invocations,
        )?;
        let relocations = vec![expression_projection(source.expression)?];
        builder.push(
            projection,
            ExecutionImageRowDomainV2::Source,
            source,
            relocations,
        )?;
        builder.route(
            ExecutionImageRowDomainV2::Source,
            source.id.as_usize(),
            projection,
        )?;
    }
    trace_execution_handoff_phase(trace_handoff, "source_rows", &mut trace_started);
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
            projection,
            ExecutionImageRowDomainV2::State,
            state,
            relocations,
        )?;
        builder.route(
            ExecutionImageRowDomainV2::State,
            state.id.as_usize(),
            projection,
        )?;
    }
    trace_execution_handoff_phase(trace_handoff, "state_rows", &mut trace_started);
    for root in &execution.roots {
        let projection = expression_projection(root.expression)?;
        builder.push(
            projection,
            ExecutionImageRowDomainV2::Root,
            root,
            vec![expression_projection(root.expression)?],
        )?;
        builder.route(ExecutionImageRowDomainV2::Root, root.ordinal, projection)?;
    }
    trace_execution_handoff_phase(trace_handoff, "root_rows", &mut trace_started);
    for (index, function) in execution.functions.iter().enumerate() {
        let projection = function_projection(&mut builder, execution, function)?;
        let mut relocations = vec![expression_projection(function.root)?];
        if let Some(source) = function.invocation_source {
            relocations.push(expression_projection(source)?);
        }
        builder.push(
            projection,
            ExecutionImageRowDomainV2::Function,
            function,
            relocations,
        )?;
        builder.route(ExecutionImageRowDomainV2::Function, index, projection)?;
    }
    trace_execution_handoff_phase(trace_handoff, "function_rows", &mut trace_started);
    for materialization in &execution.materializations {
        let projection = owner_projection_routes
            .get(materialization.owner.as_usize())
            .copied()
            .unwrap_or(expression_projection(materialization.source)?);
        let relocations = materialization
            .expression_roots()
            .into_iter()
            .map(expression_projection)
            .collect::<Result<Vec<_>, _>>()?;
        builder.push(
            projection,
            ExecutionImageRowDomainV2::Materialization,
            materialization,
            relocations,
        )?;
        builder.route(
            ExecutionImageRowDomainV2::Materialization,
            materialization.id.as_usize(),
            projection,
        )?;
    }
    trace_execution_handoff_phase(trace_handoff, "materialization_rows", &mut trace_started);
    for owner in &execution.static_owners {
        let projection = owner_projection_routes
            .get(owner.id.as_usize())
            .copied()
            .ok_or_else(|| format!("static owner {} has no invocation projection", owner.id))?;
        let relocations = owner
            .parent
            .and_then(|parent| owner_projection_routes.get(parent.as_usize()).copied())
            .into_iter()
            .collect();
        builder.push(
            projection,
            ExecutionImageRowDomainV2::StaticOwner,
            owner,
            relocations,
        )?;
        builder.route(
            ExecutionImageRowDomainV2::StaticOwner,
            owner.id.as_usize(),
            projection,
        )?;
    }
    trace_execution_handoff_phase(trace_handoff, "static_owner_rows", &mut trace_started);

    let handoff = builder.finish(checked.source_bundle_digest_v1, checked.role);
    trace_execution_handoff_phase(trace_handoff, "finish", &mut trace_started);
    handoff
}

fn semantic_image_seal_digest(
    schema: &str,
    checked: &CheckedImageHandoffV4,
    execution: &ExecutionImageHandoffV3,
) -> Result<[u8; 32], String> {
    boon_contract::canonical_serde_hash_v1(
        SEMANTIC_IMAGE_SEAL_DOMAIN_V3,
        &(
            schema,
            checked.local_image_digest,
            execution.local_image_digest,
        ),
    )
    .map_err(|error| format!("failed to hash semantic image seal: {error}"))
}
