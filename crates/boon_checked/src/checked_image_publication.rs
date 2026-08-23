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
use std::collections::{BTreeMap, HashMap};
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
    projection_digest_id: u32,
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
            projection_digest_id,
        }
    }

    #[doc(hidden)]
    pub const fn __kernel_coordinates(self) -> (CheckedImageRowDomainV2, u32) {
        (self.domain, self.dense_index)
    }
}

#[derive(Debug, Eq, PartialEq)]
struct FrozenCheckedImageKernelOwnershipTopologyV1 {
    projection_digests: Box<[[u8; 32]]>,
    routes: Box<[CheckedImageKernelExpectedRouteV1]>,
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
    projection_digests: &[[u8; 32]],
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
        let projection_digest = projection_digests
            .get(route.projection_digest_id as usize)
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
    projection_digests: &[[u8; 32]],
    routes: &[CheckedImageKernelExpectedRouteV1],
) -> Result<[u8; 32], String> {
    let mut hasher = Sha256::new();
    hasher.update(CHECKED_IMAGE_KERNEL_OWNERSHIP_EXPECTATION_DOMAIN_V1);
    hasher.update(source_bundle_digest_v1.as_bytes());
    hasher.update([checked_image_role_code_v1(role)]);
    hasher.update(
        u64::try_from(projection_digests.len())
            .map_err(|_| "checked-image projection count exceeds u64".to_owned())?
            .to_be_bytes(),
    );
    for digest in projection_digests {
        hasher.update(digest);
    }
    hasher.update(
        u64::try_from(routes.len())
            .map_err(|_| "checked-image route count exceeds u64".to_owned())?
            .to_be_bytes(),
    );
    for route in routes {
        let projection_digest = projection_digests
            .get(route.projection_digest_id as usize)
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
        projection_digests: Box<[[u8; 32]]>,
        routes: Box<[CheckedImageKernelExpectedRouteV1]>,
    ) -> Result<(), String> {
        if self.pairing.ownership_seal.get().is_some() || self.topology.is_some() {
            return Err("kernel checked-image ownership expectation is already frozen".to_owned());
        }
        if projection_digests.windows(2).any(|pair| pair[0] >= pair[1]) {
            return Err(
                "kernel checked-image ownership projection catalog is not strict-sorted".to_owned(),
            );
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
            .any(|route| route.projection_digest_id as usize >= projection_digests.len())
        {
            return Err(
                "kernel checked-image ownership route references a foreign projection".to_owned(),
            );
        }
        self.topology = Some(FrozenCheckedImageKernelOwnershipTopologyV1 {
            projection_digests,
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
        if topology.projection_digests.len() != publication.projections.len()
            || topology
                .projection_digests
                .iter()
                .any(|digest| !publication.projection_digest_ids.contains_key(digest))
        {
            return Err(
                "kernel checked-image projection catalog differs from its independent ownership plan"
                    .to_owned(),
            );
        }
        if topology.routes.len() != publication.routes.len() {
            return Err(format!(
                "kernel checked-image publication has {} routes but its independent ownership plan has {}",
                publication.routes.len(),
                topology.routes.len(),
            ));
        }
        for route in topology.routes.iter() {
            let actual = publication
                .routes
                .get(&(route.domain, route.dense_index))
                .and_then(|projection| publication.projections.get(projection.as_usize()))
                .map(|projection| projection.key_digest)
                .ok_or_else(|| {
                    format!(
                        "kernel checked-image publication is missing expected {:?}/{} route",
                        route.domain, route.dense_index,
                    )
                })?;
            let expected = topology
                .projection_digests
                .get(route.projection_digest_id as usize)
                .ok_or_else(|| {
                    "kernel checked-image ownership route references a missing projection digest"
                        .to_owned()
                })?;
            if actual != *expected {
                return Err(format!(
                    "kernel checked-image {:?}/{} route differs from its independent ownership plan",
                    route.domain, route.dense_index,
                ));
            }
        }
        let entity_route_digest_v1 =
            checked_image_route_digest(&topology.projection_digests, &topology.routes)?;
        let ownership_digest_v1 = checked_image_ownership_digest_v1(
            self.source_bundle_digest_v1,
            self.role,
            &topology.projection_digests,
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

    fn frozen_seal(&self) -> Result<&CheckedImageKernelOwnershipSealV1, String> {
        self.pairing
            .ownership_seal
            .get()
            .ok_or_else(|| "kernel checked-image ownership expectation is not frozen".to_owned())
    }

    fn take_frozen_topology(
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

fn validate_handoff_against_frozen_ownership_expectation(
    handoff: &CheckedImageHandoffV4,
    seal: &CheckedImageKernelOwnershipSealV1,
    expected: &FrozenCheckedImageKernelOwnershipTopologyV1,
) -> Result<(), String> {
    if handoff.source_bundle_digest_v1 != seal.source_bundle_digest_v1 || handoff.role != seal.role
    {
        return Err(
            "checked-image handoff differs from its frozen ownership source authority".to_owned(),
        );
    }
    if handoff.projections.len() != expected.projection_digests.len() {
        return Err(format!(
            "checked-image handoff has {} projections but its ownership plan has {}",
            handoff.projections.len(),
            expected.projection_digests.len(),
        ));
    }
    let mut actual_projection_digests = handoff
        .projections
        .iter()
        .map(|projection| projection.stable_key_digest)
        .collect::<Vec<_>>();
    actual_projection_digests.sort_unstable();
    if actual_projection_digests
        .windows(2)
        .any(|pair| pair[0] == pair[1])
        || actual_projection_digests.as_slice() != expected.projection_digests.as_ref()
    {
        return Err(
            "checked-image projection catalog differs from its frozen ownership plan".to_owned(),
        );
    }
    if handoff.entity_routes.len() != expected.routes.len() {
        return Err(format!(
            "checked-image handoff has {} routes but its ownership plan has {}",
            handoff.entity_routes.len(),
            expected.routes.len(),
        ));
    }
    for (actual, expected_route) in handoff.entity_routes.iter().zip(expected.routes.iter()) {
        if (actual.domain, actual.dense_index)
            != (expected_route.domain, expected_route.dense_index)
        {
            return Err(
                "checked-image entity routes differ from their frozen ownership plan".to_owned(),
            );
        }
        let actual_projection_digest = handoff
            .projections
            .get(actual.projection.as_usize())
            .map(|projection| projection.stable_key_digest)
            .ok_or_else(|| {
                format!(
                    "checked-image route {:?}/{} references missing projection {}",
                    actual.domain, actual.dense_index, actual.projection.0,
                )
            })?;
        let expected_projection_digest = expected
            .projection_digests
            .get(expected_route.projection_digest_id as usize)
            .ok_or_else(|| {
                "checked-image ownership route references a missing projection digest".to_owned()
            })?;
        if actual_projection_digest != *expected_projection_digest {
            return Err(
                "checked-image entity routes differ from their frozen ownership plan".to_owned(),
            );
        }
    }
    Ok(())
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
        mut ownership_expectation: CheckedImageKernelOwnershipExpectationV1,
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
        let topology = ownership_expectation.take_frozen_topology()?;
        validate_handoff_against_frozen_ownership_expectation(handoff, seal, &topology)?;
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
    projection_digest_ids: HashMap<[u8; 32], CheckedImageKernelProjectionIdV1>,
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
    pub fn __kernel_new_pair(
        source_bundle_digest_v1: SourceBundleDigestV1,
        role: ProgramRole,
    ) -> (Self, CheckedImageKernelOwnershipExpectationV1) {
        let pairing = Arc::new(CheckedImageKernelPairingV1 {
            ownership_seal: OnceLock::new(),
        });
        let publication = Self {
            source_bundle_digest_v1,
            role,
            projection_ids: BTreeMap::new(),
            projection_digest_ids: HashMap::new(),
            projections: Vec::new(),
            routes: HashMap::new(),
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
    pub fn __kernel_projection_digest(
        &self,
        projection: CheckedImageKernelProjectionIdV1,
    ) -> Option<[u8; 32]> {
        self.projections
            .get(projection.as_usize())
            .map(|projection| projection.key_digest)
    }

    #[doc(hidden)]
    pub fn __kernel_intern_projection(
        &mut self,
        key: CheckedShardProjectionKeyV2,
    ) -> Result<CheckedImageKernelProjectionIdV1, String> {
        let key_digest = checked_image_projection_key_digest_v4(&key)?;
        self.__kernel_intern_prehashed_projection(key, key_digest)
    }

    /// Intern an independently hashed exact projection key.
    ///
    /// This entry point exists so the packed ownership plan can hash stable
    /// keys once before the separate publication traversal. Both same-key
    /// digest disagreement and distinct-key digest collisions fail closed.
    #[doc(hidden)]
    pub fn __kernel_intern_prehashed_projection(
        &mut self,
        key: CheckedShardProjectionKeyV2,
        key_digest: [u8; 32],
    ) -> Result<CheckedImageKernelProjectionIdV1, String> {
        self.require_unfrozen()?;
        if let Some(id) = self.projection_ids.get(&key).copied() {
            let previous_digest = self
                .projections
                .get(id.as_usize())
                .map(|projection| projection.key_digest)
                .ok_or_else(|| {
                    "kernel checked-image projection index is internally inconsistent".to_owned()
                })?;
            if previous_digest != key_digest {
                return Err(
                    "kernel checked-image projection key was supplied with two digests".to_owned(),
                );
            }
            return Ok(id);
        }
        if self.projection_digest_ids.contains_key(&key_digest) {
            return Err(
                "kernel checked-image distinct projection keys share a stable digest".to_owned(),
            );
        }
        let id = CheckedImageKernelProjectionIdV1(
            u32::try_from(self.projections.len())
                .map_err(|_| "kernel checked-image projection count exceeds u32".to_owned())?,
        );
        self.projection_ids.insert(key.clone(), id);
        self.projection_digest_ids.insert(key_digest, id);
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
        self.require_unfrozen()?;
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
        self.require_unfrozen()?;
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

    /// Resolve an already-linked projection without extending topology.
    #[doc(hidden)]
    pub fn __kernel_projection_id(
        &self,
        key: &CheckedShardProjectionKeyV2,
    ) -> Option<CheckedImageKernelProjectionIdV1> {
        self.projection_ids.get(key).copied()
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
                [u8; 32],
                u32,
                u32,
                Vec<CheckedImageKernelProjectionIdV1>,
            )>,
            HashMap<(CheckedImageRowDomainV2, u32), CheckedImageKernelProjectionIdV1>,
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
            self.projections
                .into_iter()
                .map(|projection| {
                    (
                        projection.key,
                        projection.key_digest,
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
