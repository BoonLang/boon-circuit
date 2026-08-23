//! Opaque, construction-only checked-image publication.
//!
//! This module is intentionally outside the serialized/public classifier
//! inventory. The kernel linker builds the value, the compiler appends its
//! project metadata rows, and the typechecker consumes it exactly once.

use crate::{
    CheckedImageHandoffV4, CheckedImageRowDomainV2, CheckedShardProjectionKeyV2, ProgramRole,
    SourceBundleDigestV1,
};
use sha2::{Digest, Sha256};
use std::sync::{Arc, OnceLock};

const CHECKED_IMAGE_KERNEL_ROUTE_PAIRING_DOMAIN_V2: &[u8] =
    b"boon.checked-image-kernel-route-pairing.v2\0";
const CHECKED_IMAGE_KERNEL_OWNERSHIP_EXPECTATION_DOMAIN_V1: &[u8] =
    b"boon.checked-image-kernel-ownership-expectation.v1\0";
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
    ownership_seal: OnceLock<CheckedImageKernelOwnershipSealV1>,
}

/// Fixed-size identity retained after the typechecker has consumed and dropped
/// the full construction-time topology proof.
#[derive(Debug, Eq, PartialEq)]
struct CheckedImageKernelOwnershipSealV1 {
    source_bundle_digest_v1: SourceBundleDigestV1,
    role: ProgramRole,
    entity_route_digest_v1: CheckedImageEntityRouteDigestV1,
    ownership_digest_v1: [u8; 32],
}

#[doc(hidden)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CheckedImageKernelExpectedRouteV1 {
    domain: CheckedImageRowDomainV2,
    dense_index: u32,
    projection: CheckedImageKernelProjectionIdV1,
}

impl CheckedImageKernelExpectedRouteV1 {
    #[doc(hidden)]
    pub const fn __kernel_new(
        domain: CheckedImageRowDomainV2,
        dense_index: u32,
        projection_digest_id: u32,
    ) -> Self {
        Self {
            domain,
            dense_index,
            projection: CheckedImageKernelProjectionIdV1(projection_digest_id),
        }
    }

    #[doc(hidden)]
    pub const fn __kernel_coordinates(self) -> (CheckedImageRowDomainV2, u32) {
        (self.domain, self.dense_index)
    }

    #[doc(hidden)]
    pub const fn __kernel_projection(self) -> CheckedImageKernelProjectionIdV1 {
        self.projection
    }
}

/// One canonical projection owned by the compact kernel topology.
///
/// The key is retained exactly once until the typechecker moves it into the
/// public V4 handoff. `definition_projection` names the Definition-region
/// sibling for compiler metadata without reconstructing an owner string.
#[doc(hidden)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CheckedImageKernelPlannedProjectionV1 {
    key: CheckedShardProjectionKeyV2,
    key_digest: [u8; 32],
    definition_projection: Option<CheckedImageKernelProjectionIdV1>,
}

impl CheckedImageKernelPlannedProjectionV1 {
    #[doc(hidden)]
    pub const fn __kernel_new(
        key: CheckedShardProjectionKeyV2,
        key_digest: [u8; 32],
        definition_projection: Option<u32>,
    ) -> Self {
        Self {
            key,
            key_digest,
            definition_projection: match definition_projection {
                Some(projection) => Some(CheckedImageKernelProjectionIdV1(projection)),
                None => None,
            },
        }
    }

    #[doc(hidden)]
    pub fn __kernel_key(&self) -> &CheckedShardProjectionKeyV2 {
        &self.key
    }

    #[doc(hidden)]
    pub const fn __kernel_digest(&self) -> [u8; 32] {
        self.key_digest
    }

    #[doc(hidden)]
    pub const fn __kernel_definition_projection(&self) -> Option<CheckedImageKernelProjectionIdV1> {
        self.definition_projection
    }

    #[doc(hidden)]
    pub fn __typechecker_into_parts(
        self,
    ) -> (
        CheckedShardProjectionKeyV2,
        [u8; 32],
        Option<CheckedImageKernelProjectionIdV1>,
    ) {
        (self.key, self.key_digest, self.definition_projection)
    }
}

#[doc(hidden)]
#[derive(Debug, Eq, PartialEq)]
pub struct FrozenCheckedImageKernelOwnershipTopologyV1 {
    projections: Box<[CheckedImageKernelPlannedProjectionV1]>,
    routes: Box<[CheckedImageKernelExpectedRouteV1]>,
}

impl FrozenCheckedImageKernelOwnershipTopologyV1 {
    #[doc(hidden)]
    pub fn __typechecker_into_parts(
        self,
    ) -> (
        Box<[CheckedImageKernelPlannedProjectionV1]>,
        Box<[CheckedImageKernelExpectedRouteV1]>,
    ) {
        (self.projections, self.routes)
    }
}

/// Move-only ownership authority constructed beside, but not from, the
/// checked-image publication.
///
/// The kernel writes the projection catalog and entity routes into this plan
/// while it derives ownership from packed syntax/layout facts. Publication
/// writes go into a distinct store. The compiler freezes both only after its
/// final metadata rows have been appended, so the typechecker can reject a
/// self-consistent mutation of either sibling.
#[doc(hidden)]
#[derive(Debug, Eq, PartialEq)]
pub struct CheckedImageKernelOwnershipExpectationV1 {
    pairing: Arc<CheckedImageKernelPairingV1>,
    source_bundle_digest_v1: SourceBundleDigestV1,
    role: ProgramRole,
    topology: Option<FrozenCheckedImageKernelOwnershipTopologyV1>,
}

/// Compact identity of every routed checked entity and its exact stable
/// projection key. It is computed from the completed independent packed
/// ownership plan and retained after the typechecker drops that plan's slabs.
#[doc(hidden)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CheckedImageEntityRouteDigestV1([u8; 32]);

fn checked_image_route_digest(
    projections: &[CheckedImageKernelPlannedProjectionV1],
    routes: &[CheckedImageKernelExpectedRouteV1],
) -> Result<CheckedImageEntityRouteDigestV1, String> {
    let mut hasher = Sha256::new();
    hasher.update(CHECKED_IMAGE_KERNEL_ROUTE_PAIRING_DOMAIN_V2);
    hasher.update(
        u64::try_from(routes.len())
            .map_err(|_| "checked-image route count exceeds u64".to_owned())?
            .to_be_bytes(),
    );
    for route in routes {
        let projection_digest = projections
            .get(route.projection.as_usize())
            .map(|projection| projection.key_digest)
            .ok_or_else(|| {
                "checked-image expected route references a missing projection digest".to_owned()
            })?;
        hasher.update([checked_image_row_domain_code_v2(route.domain)]);
        hasher.update(route.dense_index.to_be_bytes());
        hasher.update(projection_digest);
    }
    Ok(CheckedImageEntityRouteDigestV1(hasher.finalize().into()))
}

fn checked_image_role_code_v1(role: ProgramRole) -> u8 {
    match role {
        ProgramRole::Client => 0,
        ProgramRole::Session => 1,
        ProgramRole::Server => 2,
    }
}

fn checked_image_ownership_digest_v1(
    source_bundle_digest_v1: SourceBundleDigestV1,
    role: ProgramRole,
    projections: &[CheckedImageKernelPlannedProjectionV1],
    routes: &[CheckedImageKernelExpectedRouteV1],
) -> Result<[u8; 32], String> {
    let mut hasher = Sha256::new();
    hasher.update(CHECKED_IMAGE_KERNEL_OWNERSHIP_EXPECTATION_DOMAIN_V1);
    hasher.update(source_bundle_digest_v1.as_bytes());
    hasher.update([checked_image_role_code_v1(role)]);
    hasher.update(
        u64::try_from(projections.len())
            .map_err(|_| "checked-image projection count exceeds u64".to_owned())?
            .to_be_bytes(),
    );
    for projection in projections {
        hasher.update(projection.key_digest);
    }
    hasher.update(
        u64::try_from(routes.len())
            .map_err(|_| "checked-image route count exceeds u64".to_owned())?
            .to_be_bytes(),
    );
    for route in routes {
        let projection_digest = projections
            .get(route.projection.as_usize())
            .map(|projection| projection.key_digest)
            .ok_or_else(|| {
                "checked-image ownership route references a missing projection digest".to_owned()
            })?;
        hasher.update([checked_image_row_domain_code_v2(route.domain)]);
        hasher.update(route.dense_index.to_be_bytes());
        hasher.update(projection_digest);
    }
    Ok(hasher.finalize().into())
}

/// Stable fixed-width encoding for the process-local route capability.
///
/// Do not derive this from the Rust discriminant: the explicit mapping keeps
/// the proof format independent of compiler layout and makes additions review
/// visible. The precomputed projection digest names the exact owner and region,
/// so route sealing never serializes or clones their strings a second time.
fn checked_image_row_domain_code_v2(domain: CheckedImageRowDomainV2) -> u8 {
    match domain {
        CheckedImageRowDomainV2::Header => 0,
        CheckedImageRowDomainV2::Scope => 1,
        CheckedImageRowDomainV2::Declaration => 2,
        CheckedImageRowDomainV2::Statement => 3,
        CheckedImageRowDomainV2::Expression => 4,
        CheckedImageRowDomainV2::Callable => 5,
        CheckedImageRowDomainV2::ContextFormal => 6,
        CheckedImageRowDomainV2::Call => 7,
        CheckedImageRowDomainV2::CallResultPath => 8,
        CheckedImageRowDomainV2::OrderChain => 9,
        CheckedImageRowDomainV2::PatternBinding => 10,
        CheckedImageRowDomainV2::ResourceProjection => 11,
        CheckedImageRowDomainV2::Source => 12,
        CheckedImageRowDomainV2::State => 13,
        CheckedImageRowDomainV2::List => 14,
        CheckedImageRowDomainV2::Occurrence => 15,
        CheckedImageRowDomainV2::SourceUnitMetadata => 16,
        CheckedImageRowDomainV2::SourcePayloadShape => 17,
        CheckedImageRowDomainV2::HostPort => 18,
        CheckedImageRowDomainV2::OutputRootType => 19,
        CheckedImageRowDomainV2::ExpressionType => 20,
        CheckedImageRowDomainV2::FunctionType => 21,
        CheckedImageRowDomainV2::NamedValueType => 22,
        CheckedImageRowDomainV2::RenderSlot => 23,
        CheckedImageRowDomainV2::Diagnostic => 24,
    }
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
    let mut hasher = Sha256::new();
    hasher.update(CHECKED_IMAGE_KERNEL_ROUTE_PAIRING_DOMAIN_V2);
    hasher.update(
        u64::try_from(handoff.entity_routes.len())
            .map_err(|_| "checked-image route count exceeds u64".to_owned())?
            .to_be_bytes(),
    );
    for route in &handoff.entity_routes {
        let projection_digest = handoff
            .projections
            .get(route.projection.as_usize())
            .map(|projection| projection.stable_key_digest)
            .ok_or_else(|| {
                format!(
                    "checked-image route {:?}/{} references missing projection {}",
                    route.domain, route.dense_index, route.projection.0,
                )
            })?;
        hasher.update([checked_image_row_domain_code_v2(route.domain)]);
        hasher.update(route.dense_index.to_be_bytes());
        hasher.update(projection_digest);
    }
    Ok(CheckedImageEntityRouteDigestV1(hasher.finalize().into()))
}

impl CheckedImageKernelOwnershipExpectationV1 {
    fn new(
        pairing: Arc<CheckedImageKernelPairingV1>,
        source_bundle_digest_v1: SourceBundleDigestV1,
        role: ProgramRole,
    ) -> Self {
        Self {
            pairing,
            source_bundle_digest_v1,
            role,
            topology: None,
        }
    }

    /// Move one completed compact ownership plan into this pairing.
    ///
    /// The kernel finishes and validates these slabs before publication starts,
    /// borrows only its separate exact-key prehash table while publishing, and
    /// transfers the slabs here afterward. No route is expanded back to an
    /// inline digest and no second full topology allocation is retained.
    #[doc(hidden)]
    pub fn __kernel_install_compact_topology(
        &mut self,
        projections: Box<[CheckedImageKernelPlannedProjectionV1]>,
        routes: Box<[CheckedImageKernelExpectedRouteV1]>,
    ) -> Result<(), String> {
        if self.pairing.ownership_seal.get().is_some() || self.topology.is_some() {
            return Err("kernel checked-image ownership expectation is already frozen".to_owned());
        }
        if projections
            .windows(2)
            .any(|pair| pair[0].key >= pair[1].key)
        {
            return Err(
                "kernel checked-image ownership projection keys are not strict-sorted".to_owned(),
            );
        }
        let mut digests = projections
            .iter()
            .map(|projection| projection.key_digest)
            .collect::<Vec<_>>();
        digests.sort_unstable();
        if digests.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(
                "kernel checked-image ownership projection digests are not unique".to_owned(),
            );
        }
        for (ordinal, projection) in projections.iter().enumerate() {
            if checked_image_projection_key_digest_v4(&projection.key)? != projection.key_digest {
                return Err(format!(
                    "kernel checked-image ownership projection {ordinal} has a foreign digest"
                ));
            }
            if let Some(definition_projection) = projection.definition_projection {
                let definition = projections
                    .get(definition_projection.as_usize())
                    .ok_or_else(|| {
                        format!(
                            "kernel checked-image ownership projection {ordinal} has a foreign definition sibling"
                        )
                    })?;
                if definition.key.owner != projection.key.owner
                    || definition.key.region != crate::CheckedShardRegionV2::Definition
                {
                    return Err(format!(
                        "kernel checked-image ownership projection {ordinal} has an invalid definition sibling"
                    ));
                }
            }
        }
        if routes.windows(2).any(|pair| {
            (pair[0].domain, pair[0].dense_index) >= (pair[1].domain, pair[1].dense_index)
        }) {
            return Err(
                "kernel checked-image ownership routes are not globally strict-sorted".to_owned(),
            );
        }
        if routes
            .iter()
            .any(|route| route.projection.as_usize() >= projections.len())
        {
            return Err(
                "kernel checked-image ownership route references a foreign projection".to_owned(),
            );
        }
        self.topology = Some(FrozenCheckedImageKernelOwnershipTopologyV1 {
            projections,
            routes,
        });
        Ok(())
    }

    /// Freeze the independently populated plan against its sibling
    /// publication. This is the only operation that compares the two stores;
    /// it never derives expected rows by iterating publication state.
    #[doc(hidden)]
    pub fn __kernel_freeze_against(
        &mut self,
        publication: &CheckedImageKernelPublicationV1,
    ) -> Result<(), String> {
        if !Arc::ptr_eq(&self.pairing, &publication.pairing) {
            return Err(
                "kernel checked-image publication and ownership plan have different construction identities"
                    .to_owned(),
            );
        }
        if self.source_bundle_digest_v1 != publication.source_bundle_digest_v1
            || self.role != publication.role
        {
            return Err(
                "kernel checked-image publication and ownership plan have different source authority"
                    .to_owned(),
            );
        }
        if self.pairing.ownership_seal.get().is_some() {
            return Err("kernel checked-image ownership expectation is already frozen".to_owned());
        }
        let topology = self.topology.as_ref().ok_or_else(|| {
            "kernel checked-image ownership expectation has no compact topology".to_owned()
        })?;
        if topology.projections.len() != publication.projections.len() {
            return Err(
                "kernel checked-image payload count differs from its independent ownership plan"
                    .to_owned(),
            );
        }
        if topology.routes.len() != publication.route_coverage.len() {
            return Err(format!(
                "kernel checked-image publication covers {} routes but its independent ownership plan has {}",
                publication.route_coverage.len(),
                topology.routes.len(),
            ));
        }
        if let Some((index, _)) = publication
            .route_coverage
            .iter()
            .enumerate()
            .find(|(_, published)| !**published)
        {
            let route = topology.routes[index];
            return Err(format!(
                "kernel checked-image publication is missing expected {:?}/{} route",
                route.domain, route.dense_index,
            ));
        }
        if let Some((ordinal, _)) = publication
            .projections
            .iter()
            .enumerate()
            .find(|(_, projection)| projection.0 == 0)
        {
            return Err(format!(
                "kernel checked-image projection {ordinal} has no published rows"
            ));
        }
        let entity_route_digest_v1 =
            checked_image_route_digest(&topology.projections, &topology.routes)?;
        let ownership_digest_v1 = checked_image_ownership_digest_v1(
            self.source_bundle_digest_v1,
            self.role,
            &topology.projections,
            &topology.routes,
        )?;
        self.pairing
            .ownership_seal
            .set(CheckedImageKernelOwnershipSealV1 {
                source_bundle_digest_v1: self.source_bundle_digest_v1,
                role: self.role,
                entity_route_digest_v1,
                ownership_digest_v1,
            })
            .map_err(|_| {
                "kernel checked-image ownership expectation was frozen concurrently".to_owned()
            })?;
        Ok(())
    }

    /// Resolve one compiler metadata route from the sole compact topology.
    ///
    /// This query is available only before the typechecker consumes the
    /// topology. It returns a dense projection ID and never clones or exposes
    /// the stable owner key retained for the eventual V4 handoff.
    #[doc(hidden)]
    pub fn __compiler_projection_for_route(
        &self,
        domain: CheckedImageRowDomainV2,
        dense_index: usize,
    ) -> Result<Option<CheckedImageKernelProjectionIdV1>, String> {
        let dense_index = u32::try_from(dense_index)
            .map_err(|_| "checked-image compiler route exceeds u32".to_owned())?;
        let topology = self.topology.as_ref().ok_or_else(|| {
            "kernel checked-image ownership topology is unavailable to the compiler".to_owned()
        })?;
        Ok(topology
            .routes
            .binary_search_by_key(&(domain, dense_index), |route| route.__kernel_coordinates())
            .ok()
            .map(|index| topology.routes[index].projection))
    }

    /// Resolve the Definition-region sibling of one compact projection.
    #[doc(hidden)]
    pub fn __compiler_definition_projection(
        &self,
        projection: CheckedImageKernelProjectionIdV1,
    ) -> Result<Option<CheckedImageKernelProjectionIdV1>, String> {
        self.topology
            .as_ref()
            .ok_or_else(|| {
                "kernel checked-image ownership topology is unavailable to the compiler".to_owned()
            })?
            .projections
            .get(projection.as_usize())
            .map(CheckedImageKernelPlannedProjectionV1::__kernel_definition_projection)
            .ok_or_else(|| {
                "kernel checked-image compiler projection references a missing owner".to_owned()
            })
    }

    /// Resolve the program root Definition projection without constructing a
    /// string-bearing stable key in the compiler facade.
    #[doc(hidden)]
    pub fn __compiler_root_definition_projection(
        &self,
    ) -> Result<CheckedImageKernelProjectionIdV1, String> {
        let topology = self.topology.as_ref().ok_or_else(|| {
            "kernel checked-image ownership topology is unavailable to the compiler".to_owned()
        })?;
        topology
            .projections
            .iter()
            .position(|projection| {
                projection.key.region == crate::CheckedShardRegionV2::Definition
                    && projection.key.owner
                        == crate::CheckedShardOwnerKeyV2::ProgramTopLevel { role: self.role }
            })
            .map(|index| {
                CheckedImageKernelProjectionIdV1(
                    u32::try_from(index)
                        .expect("validated checked-image projection catalog fits u32"),
                )
            })
            .ok_or_else(|| {
                "kernel checked-image ownership topology has no root Definition projection"
                    .to_owned()
            })
    }

    fn frozen_seal(&self) -> Result<&CheckedImageKernelOwnershipSealV1, String> {
        self.pairing
            .ownership_seal
            .get()
            .ok_or_else(|| "kernel checked-image ownership expectation is not frozen".to_owned())
    }

    #[doc(hidden)]
    pub fn __typechecker_take_frozen_topology(
        &mut self,
    ) -> Result<FrozenCheckedImageKernelOwnershipTopologyV1, String> {
        self.frozen_seal()?;
        self.topology.take().ok_or_else(|| {
            "kernel checked-image ownership topology was already consumed".to_owned()
        })
    }

    #[doc(hidden)]
    pub fn __kernel_validate_pairing(
        &self,
        pairing: &Arc<CheckedImageKernelPairingV1>,
    ) -> Result<(), String> {
        if !Arc::ptr_eq(&self.pairing, pairing) {
            return Err(
                "kernel checked-image publication and ownership expectation have different construction identities"
                    .to_owned(),
            );
        }
        self.frozen_seal().map(|_| ())
    }

    #[doc(hidden)]
    pub fn __kernel_entity_route_digest_v1(
        &self,
    ) -> Result<CheckedImageEntityRouteDigestV1, String> {
        Ok(self.frozen_seal()?.entity_route_digest_v1)
    }
}

fn checked_image_ownership_digest_from_handoff(
    handoff: &CheckedImageHandoffV4,
) -> Result<[u8; 32], String> {
    let mut hasher = Sha256::new();
    hasher.update(CHECKED_IMAGE_KERNEL_OWNERSHIP_EXPECTATION_DOMAIN_V1);
    hasher.update(handoff.source_bundle_digest_v1.as_bytes());
    hasher.update([checked_image_role_code_v1(handoff.role)]);
    hasher.update(
        u64::try_from(handoff.projections.len())
            .map_err(|_| "checked-image projection count exceeds u64".to_owned())?
            .to_be_bytes(),
    );
    for projection in &handoff.projections {
        hasher.update(projection.stable_key_digest);
    }
    hasher.update(
        u64::try_from(handoff.entity_routes.len())
            .map_err(|_| "checked-image route count exceeds u64".to_owned())?
            .to_be_bytes(),
    );
    for route in &handoff.entity_routes {
        let projection_digest = handoff
            .projections
            .get(route.projection.as_usize())
            .map(|projection| projection.stable_key_digest)
            .ok_or_else(|| {
                format!(
                    "checked-image route {:?}/{} references missing projection {}",
                    route.domain, route.dense_index, route.projection.0,
                )
            })?;
        hasher.update([checked_image_row_domain_code_v2(route.domain)]);
        hasher.update(route.dense_index.to_be_bytes());
        hasher.update(projection_digest);
    }
    Ok(hasher.finalize().into())
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
    ownership_digest_v1: [u8; 32],
}

impl CheckedImageKernelPairingReceiptV1 {
    #[doc(hidden)]
    pub fn __typechecker_new(
        pairing: Arc<CheckedImageKernelPairingV1>,
        handoff: &CheckedImageHandoffV4,
        ownership_expectation: CheckedImageKernelOwnershipExpectationV1,
    ) -> Result<Self, String> {
        if !Arc::ptr_eq(&pairing, &ownership_expectation.pairing) {
            return Err(
                "kernel checked-image receipt received a foreign ownership expectation".to_owned(),
            );
        }
        let seal = pairing.ownership_seal.get().ok_or_else(|| {
            "kernel checked-image construction identity has no frozen ownership expectation"
                .to_owned()
        })?;
        if ownership_expectation.topology.is_some() {
            return Err(
                "kernel checked-image ownership topology was not consumed by the typechecker"
                    .to_owned(),
            );
        }
        if handoff.source_bundle_digest_v1 != seal.source_bundle_digest_v1
            || handoff.role != seal.role
            || checked_image_entity_route_digest_v1(handoff)? != seal.entity_route_digest_v1
            || checked_image_ownership_digest_from_handoff(handoff)? != seal.ownership_digest_v1
        {
            return Err("checked-image handoff differs from its frozen ownership plan".to_owned());
        }
        let entity_route_digest_v1 = seal.entity_route_digest_v1;
        let ownership_digest_v1 = seal.ownership_digest_v1;
        Ok(Self {
            pairing,
            source_bundle_digest_v1: handoff.source_bundle_digest_v1,
            role: handoff.role,
            checked_image_digest: handoff.local_image_digest,
            entity_route_digest_v1,
            ownership_digest_v1,
        })
    }

    #[doc(hidden)]
    pub fn __kernel_validate(
        &self,
        pairing: &Arc<CheckedImageKernelPairingV1>,
        handoff: &CheckedImageHandoffV4,
    ) -> Result<(), String> {
        let expected = pairing.ownership_seal.get().ok_or_else(|| {
            "kernel checked-image construction identity has no frozen ownership expectation"
                .to_owned()
        })?;
        if checked_image_entity_route_digest_v1(handoff)? != expected.entity_route_digest_v1 {
            return Err(
                "checked-image entity routes differ from their frozen ownership plan".to_owned(),
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
        let expected = pairing.ownership_seal.get().ok_or_else(|| {
            "kernel checked-image construction identity has no frozen ownership expectation"
                .to_owned()
        })?;
        if self.entity_route_digest_v1 != expected.entity_route_digest_v1
            || self.ownership_digest_v1 != expected.ownership_digest_v1
        {
            return Err(
                "kernel checked-image ownership differs from its packed construction".to_owned(),
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

/// Dense, move-only checked-image payload assembled beside the kernel topology.
///
/// Stable keys and entity routes live only in the independent compact
/// ownership plan. This sibling owns aligned row counts, one flat relocation
/// edge slab, and route-coverage bits produced by a second logical walk.
#[derive(Debug, Eq, PartialEq)]
pub struct CheckedImageKernelPublicationV1 {
    source_bundle_digest_v1: SourceBundleDigestV1,
    role: ProgramRole,
    projections: Box<[(u32, u32)]>,
    relocations: Vec<(
        CheckedImageKernelProjectionIdV1,
        CheckedImageKernelProjectionIdV1,
    )>,
    route_coverage: Box<[bool]>,
    pairing: Arc<CheckedImageKernelPairingV1>,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct CheckedImageKernelProjectionIdV1(u32);

impl CheckedImageKernelProjectionIdV1 {
    #[doc(hidden)]
    pub const fn __kernel_new(value: u32) -> Self {
        Self(value)
    }

    pub const fn as_usize(self) -> usize {
        self.0 as usize
    }
}

impl CheckedImageKernelPublicationV1 {
    #[doc(hidden)]
    pub fn __kernel_new_pair(
        source_bundle_digest_v1: SourceBundleDigestV1,
        role: ProgramRole,
        projection_count: usize,
        route_count: usize,
    ) -> (Self, CheckedImageKernelOwnershipExpectationV1) {
        let pairing = Arc::new(CheckedImageKernelPairingV1 {
            ownership_seal: OnceLock::new(),
        });
        let publication = Self {
            source_bundle_digest_v1,
            role,
            projections: vec![(0, 0); projection_count].into_boxed_slice(),
            relocations: Vec::new(),
            route_coverage: vec![false; route_count].into_boxed_slice(),
            pairing: Arc::clone(&pairing),
        };
        let expectation =
            CheckedImageKernelOwnershipExpectationV1::new(pairing, source_bundle_digest_v1, role);
        (publication, expectation)
    }

    fn require_unfrozen(&self) -> Result<(), String> {
        if self.pairing.ownership_seal.get().is_some() {
            return Err("kernel checked-image publication is frozen".to_owned());
        }
        Ok(())
    }

    #[doc(hidden)]
    pub fn __kernel_pairing(&self) -> Result<Arc<CheckedImageKernelPairingV1>, String> {
        if self.pairing.ownership_seal.get().is_none() {
            return Err("kernel checked-image publication is not frozen".to_owned());
        }
        Ok(Arc::clone(&self.pairing))
    }

    /// Borrow the process-local identity while the two sibling constructions
    /// are still mutable. Only the kernel linker may retain this handle; all
    /// public validation requires the later frozen ownership expectation.
    #[doc(hidden)]
    pub fn __kernel_unfrozen_pairing(&self) -> Arc<CheckedImageKernelPairingV1> {
        Arc::clone(&self.pairing)
    }

    #[doc(hidden)]
    pub fn __kernel_publish_rows(
        &mut self,
        projection: CheckedImageKernelProjectionIdV1,
        row_count: u32,
    ) -> Result<(), String> {
        self.require_unfrozen()?;
        let row = self
            .projections
            .get_mut(projection.as_usize())
            .ok_or_else(|| {
                format!(
                    "kernel checked-image publication references missing projection {}",
                    projection.0
                )
            })?;
        row.0 = row
            .0
            .checked_add(row_count)
            .ok_or_else(|| "kernel checked-image row count exceeds u32".to_owned())?;
        Ok(())
    }

    #[doc(hidden)]
    pub fn __kernel_publish_dependency_row(
        &mut self,
        projection: CheckedImageKernelProjectionIdV1,
        relocations: &[CheckedImageKernelProjectionIdV1],
    ) -> Result<(), String> {
        self.require_unfrozen()?;
        let projection_count = self.projections.len();
        let row = self
            .projections
            .get(projection.as_usize())
            .ok_or_else(|| "kernel checked-image dependency source is missing".to_owned())?;
        let mut has_relocation = false;
        for &target in relocations {
            if target.as_usize() >= projection_count {
                return Err(
                    "kernel checked-image dependency references a missing projection".to_owned(),
                );
            }
            has_relocation |= target != projection;
        }
        let next_row_count = row
            .0
            .checked_add(1)
            .ok_or_else(|| "kernel checked-image row count exceeds u32".to_owned())?;
        let next_dependency_row_count = if has_relocation {
            row.1
                .checked_add(1)
                .ok_or_else(|| "kernel checked-image dependency count exceeds u32".to_owned())?
        } else {
            row.1
        };
        self.relocations.extend(
            relocations
                .iter()
                .copied()
                .filter(|target| *target != projection)
                .map(|target| (projection, target)),
        );
        let row = &mut self.projections[projection.as_usize()];
        row.0 = next_row_count;
        row.1 = next_dependency_row_count;
        Ok(())
    }

    fn require_unpublished_route(&self, route_ordinal: usize) -> Result<(), String> {
        self.require_unfrozen()?;
        let published = self.route_coverage.get(route_ordinal).ok_or_else(|| {
            "kernel checked-image route coverage references a missing route".to_owned()
        })?;
        if *published {
            return Err("kernel checked-image route is published twice".to_owned());
        }
        Ok(())
    }

    /// Atomically publish one independently claimed routed row family.
    #[doc(hidden)]
    pub fn __kernel_publish_routed_rows(
        &mut self,
        route_ordinal: usize,
        projection: CheckedImageKernelProjectionIdV1,
        row_count: u32,
    ) -> Result<(), String> {
        self.require_unpublished_route(route_ordinal)?;
        if row_count == 0 {
            return Err("kernel checked-image routed row count must be nonzero".to_owned());
        }
        let row = self.projections.get(projection.as_usize()).ok_or_else(|| {
            format!(
                "kernel checked-image publication references missing projection {}",
                projection.0
            )
        })?;
        let next_row_count = row
            .0
            .checked_add(row_count)
            .ok_or_else(|| "kernel checked-image row count exceeds u32".to_owned())?;
        self.projections[projection.as_usize()].0 = next_row_count;
        self.route_coverage[route_ordinal] = true;
        Ok(())
    }

    /// Atomically publish one dependency row and its independently claimed
    /// route. The iterator is cloned for validation so a rejected target never
    /// leaves a partial relocation edge in the construction.
    #[doc(hidden)]
    pub fn __kernel_publish_routed_dependency_row(
        &mut self,
        route_ordinal: usize,
        projection: CheckedImageKernelProjectionIdV1,
        relocations: &[CheckedImageKernelProjectionIdV1],
    ) -> Result<(), String> {
        self.require_unpublished_route(route_ordinal)?;
        let projection_count = self.projections.len();
        let row = self
            .projections
            .get(projection.as_usize())
            .ok_or_else(|| "kernel checked-image dependency source is missing".to_owned())?;
        let mut has_relocation = false;
        for &target in relocations {
            if target.as_usize() >= projection_count {
                return Err(
                    "kernel checked-image dependency references a missing projection".to_owned(),
                );
            }
            has_relocation |= target != projection;
        }
        let next_row_count = row
            .0
            .checked_add(1)
            .ok_or_else(|| "kernel checked-image row count exceeds u32".to_owned())?;
        let next_dependency_row_count = if has_relocation {
            row.1
                .checked_add(1)
                .ok_or_else(|| "kernel checked-image dependency count exceeds u32".to_owned())?
        } else {
            row.1
        };
        self.relocations.extend(
            relocations
                .iter()
                .copied()
                .filter(|target| *target != projection)
                .map(|target| (projection, target)),
        );
        let row = &mut self.projections[projection.as_usize()];
        row.0 = next_row_count;
        row.1 = next_dependency_row_count;
        self.route_coverage[route_ordinal] = true;
        Ok(())
    }

    #[doc(hidden)]
    pub fn __typechecker_into_parts(
        self,
    ) -> Result<
        (
            SourceBundleDigestV1,
            ProgramRole,
            Box<[(u32, u32)]>,
            Vec<(
                CheckedImageKernelProjectionIdV1,
                CheckedImageKernelProjectionIdV1,
            )>,
            Arc<CheckedImageKernelPairingV1>,
        ),
        String,
    > {
        if self.pairing.ownership_seal.get().is_none() {
            return Err(
                "kernel checked-image publication has no frozen ownership expectation".to_owned(),
            );
        }
        Ok((
            self.source_bundle_digest_v1,
            self.role,
            self.projections,
            self.relocations,
            self.pairing,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CheckedShardCallableKindV2, CheckedShardOwnerKeyV2, CheckedShardRegionV2};
    use boon_contract::SourceBundleUnit;

    fn source_digest() -> SourceBundleDigestV1 {
        SourceBundleDigestV1::new(
            "checked-publication-test.bn",
            [SourceBundleUnit::new(
                "checked-publication-test.bn",
                "value: 1",
            )],
        )
        .expect("build checked-publication test source digest")
    }

    #[test]
    fn raw_dependency_publication_rejects_without_partial_mutation() {
        let (mut publication, _) = CheckedImageKernelPublicationV1::__kernel_new_pair(
            source_digest(),
            ProgramRole::Client,
            2,
            0,
        );
        let source = CheckedImageKernelProjectionIdV1(0);
        let target = CheckedImageKernelProjectionIdV1(1);
        let foreign = CheckedImageKernelProjectionIdV1(2);

        let error = publication
            .__kernel_publish_dependency_row(source, &[target, foreign])
            .expect_err("foreign relocation must fail before publication");
        assert!(error.contains("missing projection"), "{error}");
        assert_eq!(publication.projections.as_ref(), &[(0, 0), (0, 0)]);
        assert!(publication.relocations.is_empty());

        publication
            .__kernel_publish_dependency_row(source, &[target])
            .expect("valid dependency publishes after rejected attempt");
        assert_eq!(publication.projections.as_ref(), &[(1, 1), (0, 0)]);
        assert_eq!(publication.relocations, [(source, target)]);
    }

    #[test]
    fn routed_dependency_publication_rolls_back_payload_edges_and_coverage() {
        let (mut publication, _) = CheckedImageKernelPublicationV1::__kernel_new_pair(
            source_digest(),
            ProgramRole::Client,
            2,
            1,
        );
        let source = CheckedImageKernelProjectionIdV1(0);
        let target = CheckedImageKernelProjectionIdV1(1);
        let foreign = CheckedImageKernelProjectionIdV1(2);

        let error = publication
            .__kernel_publish_routed_dependency_row(0, source, &[target, foreign])
            .expect_err("foreign routed relocation must fail before publication");
        assert!(error.contains("missing projection"), "{error}");
        assert_eq!(publication.projections.as_ref(), &[(0, 0), (0, 0)]);
        assert!(publication.relocations.is_empty());
        assert_eq!(publication.route_coverage.as_ref(), &[false]);

        publication
            .__kernel_publish_routed_dependency_row(0, source, &[target])
            .expect("valid routed dependency publishes after rejected attempt");
        assert_eq!(publication.projections.as_ref(), &[(1, 1), (0, 0)]);
        assert_eq!(publication.relocations, [(source, target)]);
        assert_eq!(publication.route_coverage.as_ref(), &[true]);
    }

    #[test]
    fn routed_rows_cannot_be_used_as_a_zero_payload_coverage_marker() {
        let (mut publication, _) = CheckedImageKernelPublicationV1::__kernel_new_pair(
            source_digest(),
            ProgramRole::Client,
            1,
            1,
        );
        let projection = CheckedImageKernelProjectionIdV1(0);
        let error = publication
            .__kernel_publish_routed_rows(0, projection, 0)
            .expect_err("zero routed rows must not mark route coverage");
        assert!(error.contains("must be nonzero"), "{error}");
        assert_eq!(publication.projections.as_ref(), &[(0, 0)]);
        assert_eq!(publication.route_coverage.as_ref(), &[false]);
    }

    #[test]
    fn builtin_interface_projection_has_no_definition_sibling_or_definition_key() {
        let role = ProgramRole::Client;
        let root_key = CheckedShardProjectionKeyV2 {
            owner: CheckedShardOwnerKeyV2::ProgramTopLevel { role },
            region: CheckedShardRegionV2::Definition,
        };
        let builtin_owner = CheckedShardOwnerKeyV2::Callable {
            role,
            callable_kind: CheckedShardCallableKindV2::Builtin,
            name: "TEST_BUILTIN".to_owned(),
            external_identity: None,
        };
        let builtin_interface_key = CheckedShardProjectionKeyV2 {
            owner: builtin_owner.clone(),
            region: CheckedShardRegionV2::Interface,
        };
        let mut keys = [root_key.clone(), builtin_interface_key.clone()];
        keys.sort_unstable();
        let root = keys
            .iter()
            .position(|key| *key == root_key)
            .expect("root projection is present");
        let builtin = keys
            .iter()
            .position(|key| *key == builtin_interface_key)
            .expect("builtin interface projection is present");
        let projections = keys
            .into_iter()
            .enumerate()
            .map(|(ordinal, key)| {
                let digest = checked_image_projection_key_digest_v4(&key)
                    .expect("projection key has a stable digest");
                CheckedImageKernelPlannedProjectionV1::__kernel_new(
                    key,
                    digest,
                    (ordinal == root)
                        .then_some(u32::try_from(root).expect("test projection ordinal fits u32")),
                )
            })
            .collect::<Vec<_>>()
            .into_boxed_slice();
        let builtin = CheckedImageKernelProjectionIdV1(
            u32::try_from(builtin).expect("test projection ordinal fits u32"),
        );
        let root = CheckedImageKernelProjectionIdV1(
            u32::try_from(root).expect("test projection ordinal fits u32"),
        );
        let routes = Box::new([CheckedImageKernelExpectedRouteV1::__kernel_new(
            CheckedImageRowDomainV2::Callable,
            0,
            builtin.0,
        )]);
        let (mut publication, mut expectation) = CheckedImageKernelPublicationV1::__kernel_new_pair(
            source_digest(),
            role,
            projections.len(),
            routes.len(),
        );
        publication
            .__kernel_publish_rows(root, 1)
            .expect("root Definition publishes one row");
        publication
            .__kernel_publish_routed_rows(0, builtin, 1)
            .expect("builtin Interface publishes its routed row");
        expectation
            .__kernel_install_compact_topology(projections, routes)
            .expect("install interface-only builtin topology");

        assert_eq!(
            expectation
                .__compiler_definition_projection(builtin)
                .expect("query builtin Definition sibling"),
            None,
        );
        let topology = expectation
            .topology
            .as_ref()
            .expect("installed topology remains available before sealing");
        assert!(!topology.projections.iter().any(|projection| {
            projection.key.owner == builtin_owner
                && projection.key.region == CheckedShardRegionV2::Definition
        }));
        expectation
            .__kernel_freeze_against(&publication)
            .expect("interface-only builtin topology seals");
    }
}
