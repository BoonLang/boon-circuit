//! Opaque, construction-only checked-image publication.
//!
//! This module is intentionally outside the serialized/public classifier
//! inventory. The kernel linker builds the value, the compiler appends its
//! project metadata rows, and the typechecker consumes it exactly once.

use crate::{
    CheckedImageHandoffV4, CheckedImageRowDomainV2, CheckedShardProjectionKeyV2, ProgramRole,
    SourceBundleDigestV1,
};
use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, OnceLock};

const CHECKED_IMAGE_KERNEL_ROUTE_PAIRING_DOMAIN_V1: &[u8] =
    b"boon.checked-image-kernel-route-pairing.v1\0";
const CHECKED_IMAGE_PROJECTION_KEY_DOMAIN_V4: &[u8] = b"boon.checked-image-projection-key.v4\0";

/// Process-local construction identity shared by exactly one packed semantic
/// input and one checked-image publication.
///
/// Equality of checked artifacts is deterministic and content-based, but a
/// construction token must not be paired with an independently produced
/// image that merely has the same source and row counts. Pointer identity is
/// therefore intentional here; this value is never serialized or published.
#[doc(hidden)]
#[derive(Debug, Eq, PartialEq)]
pub struct CheckedImageKernelPairingV1 {
    entity_route_digest: OnceLock<CheckedImageEntityRouteDigestV1>,
}

/// Compact identity of every routed checked entity and its exact stable
/// projection key. The packed semantic construction owns an independent copy
/// so a malformed publication cannot certify its own owner topology.
#[doc(hidden)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CheckedImageEntityRouteDigestV1([u8; 32]);

fn checked_image_route_digest(
    mut routes: Vec<(CheckedImageRowDomainV2, u32, [u8; 32])>,
) -> Result<CheckedImageEntityRouteDigestV1, String> {
    routes.sort_unstable_by_key(|(domain, dense_index, _)| (*domain, *dense_index));
    boon_contract::canonical_serde_hash_v1_streaming(
        CHECKED_IMAGE_KERNEL_ROUTE_PAIRING_DOMAIN_V1,
        &routes,
    )
    .map(CheckedImageEntityRouteDigestV1)
    .map_err(|error| format!("failed to hash checked-image entity routes: {error}"))
}

/// Shared stable-key identity used by direct packed publication and the
/// canonical checked-image handoff.
#[doc(hidden)]
pub fn checked_image_projection_key_digest_v4(
    key: &CheckedShardProjectionKeyV2,
) -> Result<[u8; 32], String> {
    boon_contract::canonical_serde_hash_v1_streaming(CHECKED_IMAGE_PROJECTION_KEY_DOMAIN_V4, key)
        .map_err(|error| format!("failed to hash checked projection key: {error}"))
}

/// Derive the exact route identity from a completed checked-image handoff.
#[doc(hidden)]
pub fn checked_image_entity_route_digest_v1(
    handoff: &CheckedImageHandoffV4,
) -> Result<CheckedImageEntityRouteDigestV1, String> {
    if handoff.entity_routes.windows(2).any(|routes| {
        (routes[0].domain, routes[0].dense_index) >= (routes[1].domain, routes[1].dense_index)
    }) {
        return Err("checked-image entity routes are not globally strict-sorted".to_owned());
    }
    checked_image_route_digest(
        handoff
            .entity_routes
            .iter()
            .map(|route| {
                handoff
                    .projections
                    .get(route.projection.as_usize())
                    .map(|projection| {
                        (
                            route.domain,
                            route.dense_index,
                            projection.stable_key_digest,
                        )
                    })
                    .ok_or_else(|| {
                        format!(
                            "checked-image route {:?}/{} references missing projection {}",
                            route.domain, route.dense_index, route.projection.0,
                        )
                    })
            })
            .collect::<Result<Vec<_>, _>>()?,
    )
}

/// Proof that the typechecker consumed the exact publication created beside a
/// packed semantic input and bound it to one completed checked image.
#[doc(hidden)]
#[derive(Debug)]
pub struct CheckedImageKernelPairingReceiptV1 {
    pairing: Arc<CheckedImageKernelPairingV1>,
    source_bundle_digest_v1: SourceBundleDigestV1,
    role: ProgramRole,
    checked_image_digest: [u8; 32],
    entity_route_digest_v1: CheckedImageEntityRouteDigestV1,
}

impl CheckedImageKernelPairingReceiptV1 {
    #[doc(hidden)]
    pub fn __typechecker_new(
        pairing: Arc<CheckedImageKernelPairingV1>,
        handoff: &CheckedImageHandoffV4,
        entity_route_digest_v1: CheckedImageEntityRouteDigestV1,
    ) -> Self {
        Self {
            pairing,
            source_bundle_digest_v1: handoff.source_bundle_digest_v1,
            role: handoff.role,
            checked_image_digest: handoff.local_image_digest,
            entity_route_digest_v1,
        }
    }

    #[doc(hidden)]
    pub fn __kernel_validate(
        &self,
        pairing: &Arc<CheckedImageKernelPairingV1>,
        handoff: &CheckedImageHandoffV4,
    ) -> Result<(), String> {
        let actual_route_digest = checked_image_entity_route_digest_v1(handoff)?;
        if actual_route_digest != self.entity_route_digest_v1 {
            return Err(
                "kernel checked-image entity routes differ from their sealed receipt".to_owned(),
            );
        }
        self.__kernel_validate_sealed(pairing, handoff)
    }

    /// Validate an immutable checked seal whose route digest was computed and
    /// compared by the typechecker before the handoff and receipt were coupled.
    #[doc(hidden)]
    pub fn __kernel_validate_sealed(
        &self,
        pairing: &Arc<CheckedImageKernelPairingV1>,
        handoff: &CheckedImageHandoffV4,
    ) -> Result<(), String> {
        self.__kernel_validate_identity(pairing, handoff)?;
        let expected_route_digest = pairing.entity_route_digest.get().ok_or_else(|| {
            "kernel checked-image construction identity has no finalized entity routes".to_owned()
        })?;
        if &self.entity_route_digest_v1 != expected_route_digest {
            return Err(
                "kernel checked-image entity routes differ from their packed construction"
                    .to_owned(),
            );
        }
        Ok(())
    }

    /// Revalidate the process-local construction identity after the complete
    /// checked-image seal has already established exact route topology.
    #[doc(hidden)]
    pub fn __kernel_validate_identity(
        &self,
        pairing: &Arc<CheckedImageKernelPairingV1>,
        handoff: &CheckedImageHandoffV4,
    ) -> Result<(), String> {
        if !Arc::ptr_eq(&self.pairing, pairing) {
            return Err(
                "kernel semantic input and checked image have different construction identities"
                    .to_owned(),
            );
        }
        if self.source_bundle_digest_v1 != handoff.source_bundle_digest_v1
            || self.role != handoff.role
            || self.checked_image_digest != handoff.local_image_digest
        {
            return Err(
                "kernel checked-image pairing receipt is stale for the supplied checked image"
                    .to_owned(),
            );
        }
        Ok(())
    }
}

/// Dense, move-only checked-image publication assembled by the kernel linker.
///
/// This is deliberately not a serialized checked artifact. It carries only
/// projection topology, row cardinalities, and exact entity routes; the
/// definition/currentness authority remains in the independently sealed
/// checked-image authority.
#[derive(Debug, Eq, PartialEq)]
pub struct CheckedImageKernelPublicationV1 {
    source_bundle_digest_v1: SourceBundleDigestV1,
    role: ProgramRole,
    projection_ids: BTreeMap<CheckedShardProjectionKeyV2, CheckedImageKernelProjectionIdV1>,
    projections: Vec<CheckedImageKernelProjectionV1>,
    routes: HashMap<(CheckedImageRowDomainV2, u32), CheckedImageKernelProjectionIdV1>,
    pairing: Arc<CheckedImageKernelPairingV1>,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct CheckedImageKernelProjectionIdV1(u32);

impl CheckedImageKernelProjectionIdV1 {
    pub const fn as_usize(self) -> usize {
        self.0 as usize
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct CheckedImageKernelProjectionV1 {
    key: CheckedShardProjectionKeyV2,
    key_digest: [u8; 32],
    row_count: u32,
    dependency_row_count: u32,
    relocations: Vec<CheckedImageKernelProjectionIdV1>,
}

impl CheckedImageKernelPublicationV1 {
    #[doc(hidden)]
    pub fn __kernel_new(source_bundle_digest_v1: SourceBundleDigestV1, role: ProgramRole) -> Self {
        Self {
            source_bundle_digest_v1,
            role,
            projection_ids: BTreeMap::new(),
            projections: Vec::new(),
            routes: HashMap::new(),
            pairing: Arc::new(CheckedImageKernelPairingV1 {
                entity_route_digest: OnceLock::new(),
            }),
        }
    }

    #[doc(hidden)]
    pub fn __kernel_pairing(&self) -> Result<Arc<CheckedImageKernelPairingV1>, String> {
        self.__kernel_pairing_with_entity_route_digest()
            .map(|(pairing, _)| pairing)
    }

    /// Finalize the process-local pairing and return the independently owned
    /// route identity installed into the sibling packed semantic construction.
    #[doc(hidden)]
    pub fn __kernel_pairing_with_entity_route_digest(
        &self,
    ) -> Result<
        (
            Arc<CheckedImageKernelPairingV1>,
            CheckedImageEntityRouteDigestV1,
        ),
        String,
    > {
        if let Some(route_digest) = self.pairing.entity_route_digest.get().copied() {
            return Ok((Arc::clone(&self.pairing), route_digest));
        }
        let route_digest = self.__kernel_entity_route_digest()?;
        self.pairing
            .entity_route_digest
            .set(route_digest)
            .map_err(|_| {
                "kernel checked-image entity-route pairing was finalized concurrently".to_owned()
            })?;
        Ok((Arc::clone(&self.pairing), route_digest))
    }

    /// Snapshot the exact route identity for the sibling packed semantic
    /// construction before this move-only publication leaves the linker.
    #[doc(hidden)]
    pub fn __kernel_entity_route_digest(&self) -> Result<CheckedImageEntityRouteDigestV1, String> {
        checked_image_route_digest(
            self.routes
                .iter()
                .map(|(&(domain, dense_index), &projection)| {
                    (
                        domain,
                        dense_index,
                        self.projections[projection.as_usize()].key_digest,
                    )
                })
                .collect(),
        )
    }

    #[doc(hidden)]
    pub fn __kernel_intern_projection(
        &mut self,
        key: CheckedShardProjectionKeyV2,
    ) -> Result<CheckedImageKernelProjectionIdV1, String> {
        if let Some(id) = self.projection_ids.get(&key).copied() {
            return Ok(id);
        }
        let id = CheckedImageKernelProjectionIdV1(
            u32::try_from(self.projections.len())
                .map_err(|_| "kernel checked-image projection count exceeds u32".to_owned())?,
        );
        self.projection_ids.insert(key.clone(), id);
        let key_digest = checked_image_projection_key_digest_v4(&key)?;
        self.projections.push(CheckedImageKernelProjectionV1 {
            key,
            key_digest,
            row_count: 0,
            dependency_row_count: 0,
            relocations: Vec::new(),
        });
        Ok(id)
    }

    #[doc(hidden)]
    pub fn __kernel_publish_rows(
        &mut self,
        projection: CheckedImageKernelProjectionIdV1,
        row_count: u32,
    ) -> Result<(), String> {
        let row = self
            .projections
            .get_mut(projection.as_usize())
            .ok_or_else(|| {
                format!(
                    "kernel checked-image publication references missing projection {}",
                    projection.0
                )
            })?;
        row.row_count = row
            .row_count
            .checked_add(row_count)
            .ok_or_else(|| "kernel checked-image row count exceeds u32".to_owned())?;
        Ok(())
    }

    #[doc(hidden)]
    pub fn __kernel_publish_dependency_row(
        &mut self,
        projection: CheckedImageKernelProjectionIdV1,
        relocations: impl IntoIterator<Item = CheckedImageKernelProjectionIdV1>,
    ) -> Result<(), String> {
        let mut relocations = relocations.into_iter().collect::<Vec<_>>();
        relocations.sort_unstable();
        relocations.dedup();
        relocations.retain(|target| *target != projection);
        let projection_count = self.projections.len();
        if relocations
            .iter()
            .any(|target| target.as_usize() >= projection_count)
        {
            return Err(
                "kernel checked-image dependency references a missing projection".to_owned(),
            );
        }
        let row = self
            .projections
            .get_mut(projection.as_usize())
            .ok_or_else(|| {
                format!(
                    "kernel checked-image publication references missing projection {}",
                    projection.0
                )
            })?;
        row.row_count = row
            .row_count
            .checked_add(1)
            .ok_or_else(|| "kernel checked-image row count exceeds u32".to_owned())?;
        if !relocations.is_empty() {
            row.dependency_row_count = row
                .dependency_row_count
                .checked_add(1)
                .ok_or_else(|| "kernel checked-image dependency count exceeds u32".to_owned())?;
            row.relocations.extend(relocations);
        }
        Ok(())
    }

    #[doc(hidden)]
    pub fn __kernel_route(
        &mut self,
        domain: CheckedImageRowDomainV2,
        dense_index: usize,
        projection: CheckedImageKernelProjectionIdV1,
    ) -> Result<(), String> {
        if self.pairing.entity_route_digest.get().is_some() {
            return Err(
                "kernel checked-image entity routes are sealed by their semantic pairing"
                    .to_owned(),
            );
        }
        if projection.as_usize() >= self.projections.len() {
            return Err("kernel checked-image route references a missing projection".to_owned());
        }
        let dense_index = u32::try_from(dense_index)
            .map_err(|_| "kernel checked-image route exceeds u32".to_owned())?;
        if self
            .routes
            .insert((domain, dense_index), projection)
            .is_some()
        {
            return Err(format!(
                "kernel checked-image {domain:?} route {dense_index} is published twice"
            ));
        }
        Ok(())
    }

    #[doc(hidden)]
    pub fn __kernel_projection_for_route(
        &self,
        domain: CheckedImageRowDomainV2,
        dense_index: usize,
    ) -> Option<CheckedImageKernelProjectionIdV1> {
        u32::try_from(dense_index)
            .ok()
            .and_then(|dense_index| self.routes.get(&(domain, dense_index)).copied())
    }

    #[doc(hidden)]
    pub fn __kernel_projection_key(
        &self,
        projection: CheckedImageKernelProjectionIdV1,
    ) -> Option<&CheckedShardProjectionKeyV2> {
        self.projections
            .get(projection.as_usize())
            .map(|projection| &projection.key)
    }

    #[doc(hidden)]
    pub fn __typechecker_into_parts(
        self,
    ) -> Result<
        (
            SourceBundleDigestV1,
            ProgramRole,
            Vec<(
                CheckedShardProjectionKeyV2,
                u32,
                u32,
                Vec<CheckedImageKernelProjectionIdV1>,
            )>,
            HashMap<(CheckedImageRowDomainV2, u32), CheckedImageKernelProjectionIdV1>,
            Arc<CheckedImageKernelPairingV1>,
        ),
        String,
    > {
        if self.pairing.entity_route_digest.get().is_none() {
            return Err(
                "kernel checked-image publication has no finalized semantic pairing".to_owned(),
            );
        }
        Ok((
            self.source_bundle_digest_v1,
            self.role,
            self.projections
                .into_iter()
                .map(|projection| {
                    (
                        projection.key,
                        projection.row_count,
                        projection.dependency_row_count,
                        projection.relocations,
                    )
                })
                .collect(),
            self.routes,
            self.pairing,
        ))
    }
}
