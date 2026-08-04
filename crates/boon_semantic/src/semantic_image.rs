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
    CHECKED_IMAGE_HANDOFF_SCHEMA_V2, CheckedImageHandoffV2, CheckedImageProjectionIdV2,
    CheckedImageRowDomainV2, CheckedShardRegionV2, ProgramRole,
};
use boon_contract::SourceBundleDigestV1;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::marker::PhantomData;

pub const SEMANTIC_IMAGE_SCHEMA_V2: &str = "boon.semantic-image.v2";
pub const EXECUTION_IMAGE_HANDOFF_SCHEMA_V2: &str = "boon.execution-image-handoff.v2";

const EXECUTION_INVOCATION_PATH_DOMAIN_V2: &[u8] = b"boon.execution-invocation-path.v2\0";
const EXECUTION_IMAGE_PROJECTION_KEY_DOMAIN_V2: &[u8] = b"boon.execution-image-projection-key.v2\0";
const EXECUTION_IMAGE_ROW_PAYLOAD_DOMAIN_V2: &[u8] = b"boon.execution-image-row-payload.v2\0";
const EXECUTION_IMAGE_ROW_DOMAIN_V2: &[u8] = b"boon.execution-image-row.v2\0";
const EXECUTION_IMAGE_SHARD_DOMAIN_V2: &[u8] = b"boon.execution-image-shard.v2\0";
const EXECUTION_IMAGE_HANDOFF_DOMAIN_V2: &[u8] = b"boon.execution-image-handoff.v2\0";
const SEMANTIC_IMAGE_SEAL_DOMAIN_V2: &[u8] = b"boon.semantic-image-seal.v2\0";

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
pub struct ExecutionInvocationPathIdV2(pub u32);

impl ExecutionInvocationPathIdV2 {
    pub const fn as_usize(self) -> usize {
        self.0 as usize
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct ExecutionImageProjectionIdV2(pub u32);

impl ExecutionImageProjectionIdV2 {
    pub const fn as_usize(self) -> usize {
        self.0 as usize
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ExecutionImageRelocationSpanV2 {
    pub start: u32,
    pub len: u32,
}

impl ExecutionImageRelocationSpanV2 {
    pub fn checked_range(self) -> Option<std::ops::Range<usize>> {
        let end = self.start.checked_add(self.len)?;
        Some(self.start as usize..end as usize)
    }
}

/// Collision-checked parent-pointer invocation path. Cumulative logical depth
/// never becomes an owned vector in a row or projection key.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ExecutionInvocationPathNodeV2 {
    pub parent: Option<ExecutionInvocationPathIdV2>,
    pub call_site: CheckedImageProjectionIdV2,
    pub stable_path_digest: [u8; 32],
}

/// Snapshot-local projection identity. Its separate stable-key digest commits
/// checked stable identities and invocation-path digests, never these dense IDs.
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

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ExecutionImageProjectionV2 {
    pub identity: SemanticImageProjectionIdentityV2,
    pub stable_key_digest: [u8; 32],
    pub local_content_digest: [u8; 32],
    pub row_count: u32,
    pub dependency_row_count: u32,
    pub relocation_span: ExecutionImageRelocationSpanV2,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ExecutionImageEntityRouteV2 {
    pub domain: ExecutionImageRowDomainV2,
    pub dense_index: u32,
    pub projection: ExecutionImageProjectionIdV2,
}

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
pub struct SealedSemanticImageV2 {
    schema: String,
    checked_handoff: CheckedImageHandoffV2,
    execution_handoff: ExecutionImageHandoffV2,
    execution: SemanticExecutionImageColumnsV1,
    seal_digest: [u8; 32],
}

impl SealedSemanticImageV2 {
    pub const fn checked_handoff(&self) -> &CheckedImageHandoffV2 {
        &self.checked_handoff
    }

    pub const fn execution_handoff(&self) -> &ExecutionImageHandoffV2 {
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
        if self.schema != SEMANTIC_IMAGE_SCHEMA_V2
            || self.checked_handoff.schema != CHECKED_IMAGE_HANDOFF_SCHEMA_V2
            || self.execution_handoff.schema != EXECUTION_IMAGE_HANDOFF_SCHEMA_V2
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
    checked_handoff: CheckedImageHandoffV2,
    execution: SemanticExecutionImageColumnsV1,
    execution_handoff: Option<ExecutionImageHandoffV2>,
    state: PhantomData<State>,
}

impl SemanticImageBuilder<ExecutionPending> {
    pub(crate) fn execution_pending(
        checked_handoff: CheckedImageHandoffV2,
        execution: SemanticExecutionImageColumnsV1,
    ) -> Result<Self, String> {
        if checked_handoff.schema != CHECKED_IMAGE_HANDOFF_SCHEMA_V2 {
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
        let witness = PostResourceValidatedV2;
        self.finish_execution(witness, out)
    }

    fn finish_execution(
        self,
        _witness: PostResourceValidatedV2,
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

    pub(crate) const fn checked_handoff(&self) -> &CheckedImageHandoffV2 {
        &self.checked_handoff
    }

    pub(crate) fn execution_handoff(&self) -> &ExecutionImageHandoffV2 {
        self.execution_handoff
            .as_ref()
            .expect("execution-finalized typestate always carries a handoff")
    }

    pub(crate) fn seal(self) -> Result<SealedSemanticImageV2, String> {
        let execution_handoff = self
            .execution_handoff
            .ok_or_else(|| "finalized execution builder has no handoff".to_owned())?;
        let schema = SEMANTIC_IMAGE_SCHEMA_V2.to_owned();
        let seal_digest =
            semantic_image_seal_digest(&schema, &self.checked_handoff, &execution_handoff)?;
        Ok(SealedSemanticImageV2 {
            schema,
            checked_handoff: self.checked_handoff,
            execution_handoff,
            execution: self.execution,
            seal_digest,
        })
    }
}

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

#[derive(Serialize)]
struct ExecutionImageRowFingerprintV2<'a> {
    projection_stable_key_digest: [u8; 32],
    domain: ExecutionImageRowDomainV2,
    payload_digest: [u8; 32],
    relocation_stable_key_digests: &'a [[u8; 32]],
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
struct PendingExecutionProjectionIdV2(u32);

impl PendingExecutionProjectionIdV2 {
    const fn as_usize(self) -> usize {
        self.0 as usize
    }
}

struct PendingInvocationPathV2 {
    parent: Option<ExecutionInvocationPathIdV2>,
    call_site: CheckedImageProjectionIdV2,
    stable_path_digest: [u8; 32],
}

struct PendingExecutionProjectionV2 {
    identity: SemanticImageProjectionIdentityV2,
    stable_key_digest: [u8; 32],
    row_digests: Vec<[u8; 32]>,
    dependency_row_count: u32,
    relocations: Vec<PendingExecutionProjectionIdV2>,
}

struct ExecutionImageHandoffBuilderV2<'a> {
    checked: &'a CheckedImageHandoffV2,
    definition_by_checked_projection: Vec<CheckedImageProjectionIdV2>,
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

impl<'a> ExecutionImageHandoffBuilderV2<'a> {
    fn new(checked: &'a CheckedImageHandoffV2) -> Result<Self, String> {
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
        let definition_by_checked_projection = checked
            .projections
            .iter()
            .enumerate()
            .map(|(index, projection)| {
                by_owner
                    .get(&projection.stable_key.owner)
                    .and_then(|(definition, interface)| (*definition).or(*interface))
                    .unwrap_or(CheckedImageProjectionIdV2(index as u32))
            })
            .collect();
        Ok(Self {
            checked,
            definition_by_checked_projection,
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
        self.definition_by_checked_projection
            .get(projection.as_usize())
            .copied()
            .ok_or_else(|| {
                format!(
                    "execution image has no definition route for checked projection {}",
                    projection.0
                )
            })
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
        let Self {
            checked: _,
            definition_by_checked_projection: _,
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
    checked: &CheckedImageHandoffV2,
    domain: CheckedImageRowDomainV2,
    dense_index: usize,
) -> Result<CheckedImageProjectionIdV2, String> {
    checked
        .entity_projection(domain, dense_index)
        .ok_or_else(|| {
            format!("checked image has no {domain:?} route for dense index {dense_index}")
        })
}

fn checked_execution_projection(
    builder: &mut ExecutionImageHandoffBuilderV2<'_>,
    domain: CheckedImageRowDomainV2,
    dense_index: usize,
) -> Result<PendingExecutionProjectionIdV2, String> {
    let checked = checked_projection(builder.checked, domain, dense_index)?;
    builder.checked(checked)
}

fn call_instance_projections(
    builder: &mut ExecutionImageHandoffBuilderV2<'_>,
    out: &ResolvedOutGraph,
) -> Result<Vec<PendingExecutionProjectionIdV2>, String> {
    let producer_roots = out
        .producer_roots()
        .iter()
        .map(|root| (root.call, root.spec.identity))
        .collect::<BTreeMap<_, _>>();
    let mut projections: Vec<PendingExecutionProjectionIdV2> =
        Vec::with_capacity(out.call_instances.len());
    for instance in &out.call_instances {
        if instance.id.as_usize() != projections.len() {
            return Err(format!(
                "OUT call instance {} is not dense while sealing execution image",
                instance.id
            ));
        }
        let (root, mut call_path) = match instance.parent {
            Some(parent) => {
                let parent_projection = projections.get(parent.as_usize()).ok_or_else(|| {
                    format!("OUT call {} has missing parent {parent}", instance.id)
                })?;
                let SemanticImageProjectionIdentityV2::Invocation {
                    root,
                    definition: _,
                    call_path,
                } = builder.identity(*parent_projection)?
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
                None,
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
                builder.checked,
                CheckedImageRowDomainV2::Call,
                checked_call.0 as usize,
            )?;
            let projection_record = builder.checked_projection(projection)?;
            if !matches!(
                projection_record.stable_key.region,
                CheckedShardRegionV2::Invocation { .. }
            ) {
                return Err(format!(
                    "checked call {} does not route to an invocation shard",
                    checked_call.0
                ));
            }
            call_path = Some(builder.append_path(call_path, projection)?);
        }
        let callable_projection = checked_projection(
            builder.checked,
            CheckedImageRowDomainV2::Callable,
            instance.provenance.callable.0 as usize,
        )?;
        let definition_projection = builder.definition_projection(callable_projection)?;
        projections.push(builder.invocation(root, definition_projection, call_path)?);
    }
    Ok(projections)
}

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

fn execution_image_handoff(
    checked: &CheckedImageHandoffV2,
    out: &ResolvedOutGraph,
    execution: &SemanticExecutionImageColumnsV1,
) -> Result<ExecutionImageHandoffV2, String> {
    let mut builder = ExecutionImageHandoffBuilderV2::new(checked)?;
    let invocations = call_instance_projections(&mut builder, out)?;
    let owner_projections = owner_projections(out, &invocations)?;

    let expression_routes = execution
        .expressions
        .iter()
        .map(|expression| {
            route_for_expression(
                &mut builder,
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
            .copied()
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
            let fallback = checked_execution_projection(
                &mut builder,
                CheckedImageRowDomainV2::Statement,
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
            .copied()
            .ok_or_else(|| format!("execution image references missing statement {id}"))
    };

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
    for materialization in &execution.materializations {
        let projection = owner_projections
            .get(&materialization.owner)
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
    for owner in &execution.static_owners {
        let projection = owner_projections
            .get(&owner.id)
            .copied()
            .ok_or_else(|| format!("static owner {} has no invocation projection", owner.id))?;
        let relocations = owner
            .parent
            .and_then(|parent| owner_projections.get(&parent).copied())
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

    builder.finish(checked.source_bundle_digest_v1, checked.role)
}

fn semantic_image_seal_digest(
    schema: &str,
    checked: &CheckedImageHandoffV2,
    execution: &ExecutionImageHandoffV2,
) -> Result<[u8; 32], String> {
    boon_contract::canonical_serde_hash_v1(
        SEMANTIC_IMAGE_SEAL_DOMAIN_V2,
        &(
            schema,
            checked.local_image_digest,
            execution.local_image_digest,
        ),
    )
    .map_err(|error| format!("failed to hash semantic image seal: {error}"))
}
