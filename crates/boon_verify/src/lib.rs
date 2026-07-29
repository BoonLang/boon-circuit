#![forbid(unsafe_code)]

mod bundle;

pub use bundle::*;

use boon_semantic::{
    CallableDependencyManifestDigestV1, CheckedProgramDigestV1, DependencyClassifierSchemaDigestV1,
    SemanticActivationId, SemanticEventCauseV1, SemanticProgram, SemanticProgramDigestV1,
    SemanticPulseBatchDigestV1, SemanticPulseBatchId, SemanticStateId, SemanticStateUpdateArmId,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt;

pub const REQUIRED_OBLIGATION_MANIFEST_SCHEMA_V1: &str = "boon.required-obligation-manifest.v1";
pub const VERIFICATION_MANIFEST_SCHEMA_V1: &str = "boon.verification-manifest.v1";
pub const EXPLICIT_CONTRACT_COVERAGE_V1: &str = "explicit_contracts_v1";
const REQUIREMENT_DIGEST_DOMAIN: &[u8] = b"boon.required-obligation-manifest.v1\0";
const VERIFICATION_MANIFEST_DIGEST_DOMAIN: &[u8] = b"boon.verification-manifest.v1\0";
const VERIFICATION_POLICY_DIGEST_DOMAIN: &[u8] = b"boon.verification-policy.v1\0";
const EXPLICIT_CONTRACTS_BOOTSTRAP_POLICY_V1: &[u8] =
    b"explicit-contracts-bootstrap;no-authored-contract-syntax";

macro_rules! digest_type {
    ($name:ident) => {
        #[derive(
            Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize,
        )]
        #[serde(transparent)]
        pub struct $name([u8; 32]);

        impl $name {
            pub const fn from_bytes(bytes: [u8; 32]) -> Self {
                Self(bytes)
            }

            pub const fn as_bytes(&self) -> &[u8; 32] {
                &self.0
            }

            pub fn is_zero(self) -> bool {
                self.0.iter().all(|byte| *byte == 0)
            }

            pub fn to_hex(self) -> String {
                self.0.iter().map(|byte| format!("{byte:02x}")).collect()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(&self.to_hex())
            }
        }
    };
}

digest_type!(RequirementDigestV1);
digest_type!(VerificationManifestDigestV1);
digest_type!(BundleVerificationManifestDigestV1);
digest_type!(VerificationPolicyDigestV1);
digest_type!(ProofContextKeyDigestV1);
digest_type!(AuthorityActivationRequirementDigestV1);
digest_type!(VerifiedBundleDigestV1);
digest_type!(SemanticProfileDigestV1);
digest_type!(SummaryDigestV1);
digest_type!(NormalizedVerificationConditionDigestV1);
digest_type!(EvidenceReferenceDigestV1);
digest_type!(EvidenceBodyDigestV1);
digest_type!(AssuranceDependencyDigestV1);
digest_type!(VerifierComponentDigestV1);

/// A verifier-selected policy value rather than a caller-supplied digest.
///
/// Construction is intentionally closed over the policies implemented by this
/// verifier. A bundle accepts one value and uses its digest for every role.
///
/// External code cannot claim an arbitrary digest is an implemented policy:
///
/// ```compile_fail
/// use boon_verify::{VerificationPolicyDigestV1, VerificationPolicyV1};
///
/// let _forged = VerificationPolicyV1 {
///     digest: VerificationPolicyDigestV1::from_bytes([7; 32]),
/// };
/// ```
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VerificationPolicyV1 {
    digest: VerificationPolicyDigestV1,
}

impl VerificationPolicyV1 {
    pub fn explicit_contracts_bootstrap() -> Self {
        Self {
            digest: VerificationPolicyDigestV1(domain_hash(
                VERIFICATION_POLICY_DIGEST_DOMAIN,
                EXPLICIT_CONTRACTS_BOOTSTRAP_POLICY_V1,
            )),
        }
    }

    pub const fn digest(self) -> VerificationPolicyDigestV1 {
        self.digest
    }

    fn validate(self) -> Result<(), VerifyError> {
        if self != Self::explicit_contracts_bootstrap() {
            return Err(VerifyError::new(
                "verification policy is not implemented by this verifier",
            ));
        }
        Ok(())
    }
}

macro_rules! id_type {
    ($name:ident) => {
        #[derive(
            Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize,
        )]
        #[serde(transparent)]
        pub struct $name(u64);

        impl $name {
            pub const fn new(value: u64) -> Self {
                Self(value)
            }

            pub const fn get(self) -> u64 {
                self.0
            }
        }
    };
}

id_type!(ContractIdV1);
id_type!(ConditionIdV1);
id_type!(ObligationIdV1);
id_type!(SemanticMaterializationIdV1);

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContractDefinitionCoverageV1 {
    Symbolic,
    MaterializationDependent,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct ObligationInstantiationV1 {
    pub proof_context_key_hash: ProofContextKeyDigestV1,
    pub materialization_ids: Vec<SemanticMaterializationIdV1>,
    pub condition_ids: Vec<ConditionIdV1>,
    pub obligation_ids: Vec<ObligationIdV1>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ContractCoverageV1 {
    pub contract_id: ContractIdV1,
    pub condition_ids: Vec<ConditionIdV1>,
    pub definition: ContractDefinitionCoverageV1,
    pub definition_obligation_ids: Vec<ObligationIdV1>,
    pub instantiations: Vec<ObligationInstantiationV1>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ConditionCoverageV1 {
    pub condition_id: ConditionIdV1,
    pub symbolic_obligation_ids: Vec<ObligationIdV1>,
    pub instantiated_obligation_ids: Vec<ObligationIdV1>,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AcceptedLogicalStatusV1 {
    Valid,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AcceptedEvidenceKindV1 {
    ReplayableProof,
    CheckedCertificate,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct AcceptedEvidenceReferenceV1 {
    pub kind: AcceptedEvidenceKindV1,
    pub reference_digest: EvidenceReferenceDigestV1,
}

/// Deterministic provider-local evidence accepted for one required obligation.
///
/// This core intentionally contains neither its enclosing verification-manifest
/// digest nor an exported bundle digest, keeping the manifest hash acyclic.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AcceptedObligationEvidenceCoreV1 {
    pub obligation_id: ObligationIdV1,
    pub discharged_condition_ids: Vec<ConditionIdV1>,
    pub normalized_vc_digest: NormalizedVerificationConditionDigestV1,
    pub logical_status: AcceptedLogicalStatusV1,
    pub replayable_proof_or_certificate_reference: AcceptedEvidenceReferenceV1,
    pub proof_or_checked_certificate_digest: EvidenceBodyDigestV1,
    pub callable_dependency_manifest_hashes: Vec<CallableDependencyManifestDigestV1>,
    pub proof_context_key_hashes: Vec<ProofContextKeyDigestV1>,
    pub imported_assumption_bundle_hashes: Vec<VerifiedBundleDigestV1>,
    pub summary_evidence_hashes: Vec<SummaryDigestV1>,
    pub semantic_profile_hashes: Vec<SemanticProfileDigestV1>,
    pub assurance_dependencies: Vec<AssuranceDependencyDigestV1>,
    pub verifier_policy: VerificationPolicyDigestV1,
    pub kernel_and_verifier_versions: Vec<VerifierComponentDigestV1>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RequiredObligationManifestV1 {
    pub schema: String,
    pub checked_program_digest: CheckedProgramDigestV1,
    pub semantic_program_digest: SemanticProgramDigestV1,
    pub declared_contract_ids: Vec<ContractIdV1>,
    pub declared_condition_ids: Vec<ConditionIdV1>,
    pub contract_coverage_by_id: Vec<ContractCoverageV1>,
    pub condition_coverage_by_id: Vec<ConditionCoverageV1>,
    pub required_obligation_ids: Vec<ObligationIdV1>,
    pub callable_dependency_manifest_hashes: Vec<CallableDependencyManifestDigestV1>,
    pub proof_context_key_hashes: Vec<ProofContextKeyDigestV1>,
    pub dependency_classifier_schema_hash: DependencyClassifierSchemaDigestV1,
    pub authority_activation_requirement_hashes: Vec<AuthorityActivationRequirementDigestV1>,
    pub imported_verified_bundle_hashes: Vec<VerifiedBundleDigestV1>,
    pub semantic_profile_hashes: Vec<SemanticProfileDigestV1>,
    pub summary_hashes: Vec<SummaryDigestV1>,
    pub verifier_policy: VerificationPolicyDigestV1,
    pub requirement_digest: RequirementDigestV1,
}

impl RequiredObligationManifestV1 {
    pub fn validate(&self) -> Result<(), VerifyError> {
        if self.schema != REQUIRED_OBLIGATION_MANIFEST_SCHEMA_V1 {
            return Err(VerifyError::new(format!(
                "unsupported required-obligation manifest schema `{}`",
                self.schema
            )));
        }
        require_strictly_sorted_unique("declared contract", &self.declared_contract_ids)?;
        require_strictly_sorted_unique("declared condition", &self.declared_condition_ids)?;
        require_strictly_sorted_unique("required obligation", &self.required_obligation_ids)?;
        require_strictly_sorted_unique(
            "callable dependency manifest hash",
            &self.callable_dependency_manifest_hashes,
        )?;
        require_strictly_sorted_unique("proof-context-key hash", &self.proof_context_key_hashes)?;
        require_strictly_sorted_unique(
            "authority activation requirement hash",
            &self.authority_activation_requirement_hashes,
        )?;
        require_strictly_sorted_unique(
            "imported verified bundle hash",
            &self.imported_verified_bundle_hashes,
        )?;
        require_strictly_sorted_unique("semantic profile hash", &self.semantic_profile_hashes)?;
        require_strictly_sorted_unique("summary hash", &self.summary_hashes)?;
        validate_required_coverage(self)?;
        if self.requirement_digest != requirement_digest(self)? {
            return Err(VerifyError::new(
                "required-obligation manifest digest does not match its canonical payload",
            ));
        }
        Ok(())
    }
}

/// Canonical verifier output for one exact semantic pulse slice.
///
/// Downstream optimizers may consume only `Eligible`; ineligible batches
/// remain valid programs and execute the baseline scheduler.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct VerifiedPulseFusionDecisionV1 {
    pub pulse_batch: SemanticPulseBatchId,
    pub semantic_slice_digest: SemanticPulseBatchDigestV1,
    pub status: VerifiedPulseFusionStatusV1,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum VerifiedPulseFusionStatusV1 {
    Eligible {
        fact: VerifiedPulseFusionFactV1,
    },
    Ineligible {
        reasons: Vec<PulseFusionIneligibilityV1>,
    },
}

/// Exact semantic identities and proof policies admitted by the current
/// activation-local recurrence optimization.
///
/// The verifier binds the frozen count expression and the semantic slice.
/// The concrete count is checked against the selected target profile by the
/// executor before it initializes activation state or commits a microturn.
/// A verified list-mutation lane is preserved in full; only routing from the
/// otherwise-unobserved recurrence state may be elided.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct VerifiedPulseFusionFactV1 {
    pub activation: SemanticActivationId,
    pub state: SemanticStateId,
    pub state_update_arm: SemanticStateUpdateArmId,
    pub count_policy: VerifiedPulseFusionCountPolicyV1,
    pub trace_policy: VerifiedPulseFusionTracePolicyV1,
    pub elision_policy: VerifiedPulseFusionElisionPolicyV1,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerifiedPulseFusionCountPolicyV1 {
    FrozenAndRuntimeTargetGuardedBeforeFirstMicroturn,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerifiedPulseFusionTracePolicyV1 {
    PreserveCommittedStateDeltasAndEmissionRoutes,
    PreserveCommittedStateAndListDeltasAndEmissionRoutes,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerifiedPulseFusionElisionPolicyV1 {
    ElideOnlyUnobservedRecurrenceStateRouting,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PulseFusionIneligibilityV1 {
    NoActivationLocalState,
    StateUpdateCardinality,
    StateUpdateTargetMismatch,
    HostEffect,
    Flush,
    StateHasReactiveObservers,
    DistributedInvocation,
    UnaccountedPulseConsumer,
}

impl PulseFusionIneligibilityV1 {
    pub const fn diagnostic(self) -> &'static str {
        match self {
            Self::NoActivationLocalState => {
                "pulse state is not owned by one enclosing causal activation"
            }
            Self::StateUpdateCardinality => {
                "pulse batch does not have exactly one recurrence state update"
            }
            Self::StateUpdateTargetMismatch => {
                "pulse recurrence update does not target its activation-local state"
            }
            Self::HostEffect => "pulse microturn schedules a host effect",
            Self::Flush => "pulse microturn may execute FLUSH",
            Self::StateHasReactiveObservers => {
                "pulse recurrence state has reactive observers outside the fused batch"
            }
            Self::DistributedInvocation => "pulse microturn may schedule a distributed invocation",
            Self::UnaccountedPulseConsumer => {
                "pulse batch has a consumer outside its recurrence update and emission routes"
            }
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct VerificationManifestV1 {
    pub schema: String,
    pub requirements: RequiredObligationManifestV1,
    pub accepted_obligation_evidence_core_by_id: Vec<AcceptedObligationEvidenceCoreV1>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pulse_fusion_decisions: Vec<VerifiedPulseFusionDecisionV1>,
    pub manifest_digest: VerificationManifestDigestV1,
}

impl VerificationManifestV1 {
    pub fn validate(&self) -> Result<(), VerifyError> {
        if self.schema != VERIFICATION_MANIFEST_SCHEMA_V1 {
            return Err(VerifyError::new(format!(
                "unsupported verification manifest schema `{}`",
                self.schema
            )));
        }
        self.requirements.validate()?;
        let evidence_ids = self
            .accepted_obligation_evidence_core_by_id
            .iter()
            .map(|evidence| evidence.obligation_id)
            .collect::<Vec<_>>();
        require_strictly_sorted_unique("accepted obligation evidence", &evidence_ids)?;
        if self.requirements.required_obligation_ids != evidence_ids {
            return Err(VerifyError::new(
                "required obligation ids do not exactly equal accepted evidence ids",
            ));
        }
        for evidence in &self.accepted_obligation_evidence_core_by_id {
            validate_accepted_evidence(&self.requirements, evidence)?;
        }
        validate_pulse_fusion_decisions(&self.pulse_fusion_decisions)?;
        if self.manifest_digest != verification_manifest_digest(self)? {
            return Err(VerifyError::new(
                "verification manifest digest does not match its canonical payload",
            ));
        }
        Ok(())
    }
}

fn validate_required_coverage(manifest: &RequiredObligationManifestV1) -> Result<(), VerifyError> {
    let contract_coverage_ids = manifest
        .contract_coverage_by_id
        .iter()
        .map(|coverage| coverage.contract_id)
        .collect::<Vec<_>>();
    require_strictly_sorted_unique("contract coverage", &contract_coverage_ids)?;
    if manifest.declared_contract_ids != contract_coverage_ids {
        return Err(VerifyError::new(
            "declared contract ids do not exactly equal contract coverage ids",
        ));
    }

    let condition_coverage_ids = manifest
        .condition_coverage_by_id
        .iter()
        .map(|coverage| coverage.condition_id)
        .collect::<Vec<_>>();
    require_strictly_sorted_unique("condition coverage", &condition_coverage_ids)?;
    if manifest.declared_condition_ids != condition_coverage_ids {
        return Err(VerifyError::new(
            "declared condition ids do not exactly equal condition coverage ids",
        ));
    }
    let condition_coverage_by_id = manifest
        .condition_coverage_by_id
        .iter()
        .map(|coverage| (coverage.condition_id, coverage))
        .collect::<BTreeMap<_, _>>();
    for coverage in &manifest.condition_coverage_by_id {
        require_strictly_sorted_unique(
            "symbolic condition obligation",
            &coverage.symbolic_obligation_ids,
        )?;
        require_strictly_sorted_unique(
            "instantiated condition obligation",
            &coverage.instantiated_obligation_ids,
        )?;
        if !disjoint(
            &coverage.symbolic_obligation_ids,
            &coverage.instantiated_obligation_ids,
        ) {
            return Err(VerifyError::new(format!(
                "condition {} classifies one obligation as both symbolic and instantiated",
                coverage.condition_id.get()
            )));
        }
    }

    let declared_conditions = manifest
        .declared_condition_ids
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    let required_obligations = manifest
        .required_obligation_ids
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    let permitted_proof_contexts = manifest
        .proof_context_key_hashes
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    let mut child_conditions = BTreeSet::new();
    let mut coverage_obligations = BTreeSet::new();
    let mut obligation_owner = BTreeMap::<ObligationIdV1, ContractIdV1>::new();

    for contract in &manifest.contract_coverage_by_id {
        require_strictly_sorted_unique("contract child condition", &contract.condition_ids)?;
        require_strictly_sorted_unique(
            "definition obligation",
            &contract.definition_obligation_ids,
        )?;
        if contract.condition_ids.is_empty() {
            return Err(VerifyError::new(format!(
                "contract {} has no covered conditions",
                contract.contract_id.get()
            )));
        }
        let instantiation_keys = contract
            .instantiations
            .iter()
            .map(|instantiation| instantiation.proof_context_key_hash)
            .collect::<Vec<_>>();
        require_strictly_sorted_unique("obligation instantiation", &instantiation_keys)?;

        let contract_conditions = contract
            .condition_ids
            .iter()
            .copied()
            .collect::<BTreeSet<_>>();
        for condition in &contract.condition_ids {
            if !child_conditions.insert(*condition) {
                return Err(VerifyError::new(format!(
                    "condition {} belongs to more than one contract coverage record",
                    condition.get()
                )));
            }
        }

        let definition_obligations = contract
            .definition_obligation_ids
            .iter()
            .copied()
            .collect::<BTreeSet<_>>();
        for obligation in &definition_obligations {
            insert_obligation_owner(&mut obligation_owner, *obligation, contract.contract_id)?;
            coverage_obligations.insert(*obligation);
        }

        let mut instantiated_obligations = BTreeSet::new();
        let mut instantiated_by_condition =
            BTreeMap::<ConditionIdV1, BTreeSet<ObligationIdV1>>::new();
        for instantiation in &contract.instantiations {
            require_strictly_sorted_unique(
                "instantiation materialization",
                &instantiation.materialization_ids,
            )?;
            require_strictly_sorted_unique(
                "instantiation condition",
                &instantiation.condition_ids,
            )?;
            require_strictly_sorted_unique(
                "instantiation obligation",
                &instantiation.obligation_ids,
            )?;
            if !permitted_proof_contexts.contains(&instantiation.proof_context_key_hash) {
                return Err(VerifyError::new(format!(
                    "contract {} instantiation references an unrecorded proof-context key",
                    contract.contract_id.get()
                )));
            }
            if instantiation.condition_ids.is_empty() || instantiation.obligation_ids.is_empty() {
                return Err(VerifyError::new(format!(
                    "contract {} has an empty obligation instantiation",
                    contract.contract_id.get()
                )));
            }
            if !instantiation
                .condition_ids
                .iter()
                .all(|condition| contract_conditions.contains(condition))
            {
                return Err(VerifyError::new(format!(
                    "contract {} instantiation covers a condition owned elsewhere",
                    contract.contract_id.get()
                )));
            }
            for obligation in &instantiation.obligation_ids {
                insert_obligation_owner(&mut obligation_owner, *obligation, contract.contract_id)?;
                instantiated_obligations.insert(*obligation);
                coverage_obligations.insert(*obligation);
            }
            for condition in &instantiation.condition_ids {
                instantiated_by_condition
                    .entry(*condition)
                    .or_default()
                    .extend(instantiation.obligation_ids.iter().copied());
            }
        }

        let mut symbolically_covered = BTreeSet::new();
        for condition in &contract.condition_ids {
            let coverage = condition_coverage_by_id.get(condition).ok_or_else(|| {
                VerifyError::new(format!(
                    "condition {} has no condition coverage record",
                    condition.get()
                ))
            })?;
            let symbolic = coverage
                .symbolic_obligation_ids
                .iter()
                .copied()
                .collect::<BTreeSet<_>>();
            if !symbolic.is_subset(&definition_obligations) {
                return Err(VerifyError::new(format!(
                    "condition {} records a symbolic obligation outside its contract definition",
                    condition.get()
                )));
            }
            symbolically_covered.extend(symbolic.iter().copied());

            let expected_instantiated = instantiated_by_condition
                .get(condition)
                .cloned()
                .unwrap_or_default();
            let recorded_instantiated = coverage
                .instantiated_obligation_ids
                .iter()
                .copied()
                .collect::<BTreeSet<_>>();
            if expected_instantiated != recorded_instantiated {
                return Err(VerifyError::new(format!(
                    "condition {} instantiated coverage does not exactly match its contract instantiations",
                    condition.get()
                )));
            }
            let has_definition_coverage = match contract.definition {
                ContractDefinitionCoverageV1::Symbolic => !symbolic.is_empty(),
                ContractDefinitionCoverageV1::MaterializationDependent => {
                    !recorded_instantiated.is_empty()
                }
            };
            if !has_definition_coverage {
                return Err(VerifyError::new(format!(
                    "condition {} has no coverage for its contract definition mode",
                    condition.get()
                )));
            }
        }
        if symbolically_covered != definition_obligations {
            return Err(VerifyError::new(format!(
                "contract {} definition obligations are not exactly covered by its conditions",
                contract.contract_id.get()
            )));
        }
        if !definition_obligations.is_disjoint(&instantiated_obligations) {
            return Err(VerifyError::new(format!(
                "contract {} reuses an obligation as definition and instantiation evidence",
                contract.contract_id.get()
            )));
        }
    }

    if child_conditions != declared_conditions {
        return Err(VerifyError::new(
            "declared condition ids do not exactly equal contract child condition ids",
        ));
    }
    if coverage_obligations != required_obligations {
        return Err(VerifyError::new(
            "the union of coverage obligation ids does not exactly equal required obligation ids",
        ));
    }
    Ok(())
}

fn insert_obligation_owner(
    owners: &mut BTreeMap<ObligationIdV1, ContractIdV1>,
    obligation: ObligationIdV1,
    contract: ContractIdV1,
) -> Result<(), VerifyError> {
    if let Some(previous) = owners.insert(obligation, contract) {
        return Err(VerifyError::new(format!(
            "obligation {} occurs more than once in contract coverage {} and {}",
            obligation.get(),
            previous.get(),
            contract.get()
        )));
    }
    Ok(())
}

fn validate_accepted_evidence(
    requirements: &RequiredObligationManifestV1,
    evidence: &AcceptedObligationEvidenceCoreV1,
) -> Result<(), VerifyError> {
    require_strictly_sorted_unique(
        "evidence discharged condition",
        &evidence.discharged_condition_ids,
    )?;
    require_strictly_sorted_unique(
        "evidence callable dependency manifest hash",
        &evidence.callable_dependency_manifest_hashes,
    )?;
    require_strictly_sorted_unique(
        "evidence proof-context-key hash",
        &evidence.proof_context_key_hashes,
    )?;
    require_strictly_sorted_unique(
        "evidence imported assumption bundle hash",
        &evidence.imported_assumption_bundle_hashes,
    )?;
    require_strictly_sorted_unique("evidence summary hash", &evidence.summary_evidence_hashes)?;
    require_strictly_sorted_unique(
        "evidence semantic profile hash",
        &evidence.semantic_profile_hashes,
    )?;
    require_strictly_sorted_unique(
        "evidence assurance dependency hash",
        &evidence.assurance_dependencies,
    )?;
    require_strictly_sorted_unique(
        "evidence verifier component hash",
        &evidence.kernel_and_verifier_versions,
    )?;
    if evidence.discharged_condition_ids.is_empty() {
        return Err(VerifyError::new(format!(
            "accepted obligation {} discharges no condition",
            evidence.obligation_id.get()
        )));
    }
    let expected_conditions = requirements
        .condition_coverage_by_id
        .iter()
        .filter(|coverage| {
            coverage
                .symbolic_obligation_ids
                .binary_search(&evidence.obligation_id)
                .is_ok()
                || coverage
                    .instantiated_obligation_ids
                    .binary_search(&evidence.obligation_id)
                    .is_ok()
        })
        .map(|coverage| coverage.condition_id)
        .collect::<Vec<_>>();
    if evidence.discharged_condition_ids != expected_conditions {
        return Err(VerifyError::new(format!(
            "accepted obligation {} discharged conditions do not exactly match condition coverage",
            evidence.obligation_id.get()
        )));
    }
    if evidence.normalized_vc_digest.is_zero()
        || evidence
            .replayable_proof_or_certificate_reference
            .reference_digest
            .is_zero()
        || evidence.proof_or_checked_certificate_digest.is_zero()
    {
        return Err(VerifyError::new(format!(
            "accepted obligation {} has a zero proof identity digest",
            evidence.obligation_id.get()
        )));
    }
    if evidence.kernel_and_verifier_versions.is_empty()
        || evidence
            .kernel_and_verifier_versions
            .iter()
            .any(|digest| digest.is_zero())
    {
        return Err(VerifyError::new(format!(
            "accepted obligation {} has no concrete kernel/verifier version identity",
            evidence.obligation_id.get()
        )));
    }
    if evidence.verifier_policy != requirements.verifier_policy {
        return Err(VerifyError::new(format!(
            "accepted obligation {} uses a different verifier policy",
            evidence.obligation_id.get()
        )));
    }
    require_subset(
        "evidence callable dependency manifest hash",
        &evidence.callable_dependency_manifest_hashes,
        &requirements.callable_dependency_manifest_hashes,
    )?;
    require_subset(
        "evidence proof-context-key hash",
        &evidence.proof_context_key_hashes,
        &requirements.proof_context_key_hashes,
    )?;
    require_subset(
        "evidence imported assumption bundle hash",
        &evidence.imported_assumption_bundle_hashes,
        &requirements.imported_verified_bundle_hashes,
    )?;
    require_subset(
        "evidence summary hash",
        &evidence.summary_evidence_hashes,
        &requirements.summary_hashes,
    )?;
    require_subset(
        "evidence semantic profile hash",
        &evidence.semantic_profile_hashes,
        &requirements.semantic_profile_hashes,
    )?;
    Ok(())
}

fn require_subset<T: Copy + Ord>(
    kind: &str,
    values: &[T],
    permitted: &[T],
) -> Result<(), VerifyError> {
    let permitted = permitted.iter().copied().collect::<BTreeSet<_>>();
    if values.iter().any(|value| !permitted.contains(value)) {
        return Err(VerifyError::new(format!(
            "{kind} is not present in the required-obligation manifest",
        )));
    }
    Ok(())
}

fn disjoint<T: Copy + Ord>(left: &[T], right: &[T]) -> bool {
    let left = left.iter().copied().collect::<BTreeSet<_>>();
    right.iter().all(|value| !left.contains(value))
}

/// Proof-gated ownership token for the sole production IR entrypoint.
///
/// Fields and construction are private. Even programs with no authored
/// contracts traverse the same complete-manifest validation.
///
/// External code cannot forge the token with a struct literal:
///
/// ```compile_fail
/// use boon_verify::ContractVerifiedProgram;
///
/// let _forged = ContractVerifiedProgram {
///     semantic_program: todo!(),
///     verification_manifest: todo!(),
///     coverage: "explicit_contracts_v1",
/// };
/// ```
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContractVerifiedProgram {
    semantic_program: SemanticProgram,
    verification_manifest: VerificationManifestV1,
    coverage: &'static str,
}

impl ContractVerifiedProgram {
    pub const fn semantic_program_digest(&self) -> SemanticProgramDigestV1 {
        self.verification_manifest
            .requirements
            .semantic_program_digest
    }

    pub const fn verification_manifest_digest(&self) -> VerificationManifestDigestV1 {
        self.verification_manifest.manifest_digest
    }

    pub const fn verification_manifest(&self) -> &VerificationManifestV1 {
        &self.verification_manifest
    }

    pub const fn coverage(&self) -> &'static str {
        self.coverage
    }

    #[doc(hidden)]
    pub fn into_lowering_parts(self) -> (SemanticProgram, VerificationManifestV1) {
        (self.semantic_program, self.verification_manifest)
    }
}

/// Verifies the current explicit-contract surface and constructs the mandatory
/// ownership token. The current language has no accepted `WHERE` syntax, so a
/// complete manifest is correctly empty; imported/local obligations become
/// nonempty when the formal phase adds those checked nodes.
pub fn verify_explicit_contracts(
    semantic_program: SemanticProgram,
) -> Result<ContractVerifiedProgram, VerifyError> {
    let policy = VerificationPolicyV1::explicit_contracts_bootstrap();
    let verification_manifest = verify_semantic_program(&semantic_program, policy)?;
    Ok(ContractVerifiedProgram {
        semantic_program,
        verification_manifest,
        coverage: EXPLICIT_CONTRACT_COVERAGE_V1,
    })
}

fn verify_semantic_program(
    semantic_program: &SemanticProgram,
    policy: VerificationPolicyV1,
) -> Result<VerificationManifestV1, VerifyError> {
    policy.validate()?;
    semantic_program
        .validate()
        .map_err(|error| VerifyError::new(error.to_string()))?;
    let dependency_manifest = semantic_program.dependency_manifest();
    let verifier_policy = policy.digest();
    let mut requirements = RequiredObligationManifestV1 {
        schema: REQUIRED_OBLIGATION_MANIFEST_SCHEMA_V1.to_owned(),
        checked_program_digest: semantic_program.checked_program_digest(),
        semantic_program_digest: semantic_program.digest(),
        declared_contract_ids: Vec::new(),
        declared_condition_ids: Vec::new(),
        contract_coverage_by_id: Vec::new(),
        condition_coverage_by_id: Vec::new(),
        required_obligation_ids: Vec::new(),
        callable_dependency_manifest_hashes: vec![dependency_manifest.manifest_digest],
        proof_context_key_hashes: Vec::new(),
        dependency_classifier_schema_hash: dependency_manifest.dependency_classifier_schema_digest,
        authority_activation_requirement_hashes: Vec::new(),
        imported_verified_bundle_hashes: Vec::new(),
        semantic_profile_hashes: Vec::new(),
        summary_hashes: Vec::new(),
        verifier_policy,
        requirement_digest: RequirementDigestV1([0; 32]),
    };
    requirements.requirement_digest = requirement_digest(&requirements)?;
    requirements.validate()?;
    let mut verification_manifest = VerificationManifestV1 {
        schema: VERIFICATION_MANIFEST_SCHEMA_V1.to_owned(),
        requirements,
        accepted_obligation_evidence_core_by_id: Vec::new(),
        pulse_fusion_decisions: derive_pulse_fusion_decisions(semantic_program)?,
        manifest_digest: VerificationManifestDigestV1([0; 32]),
    };
    verification_manifest.manifest_digest = verification_manifest_digest(&verification_manifest)?;
    verification_manifest.validate()?;
    Ok(verification_manifest)
}

/// Prove the narrow full-trace fusion classes without recognizing source names.
///
/// The executor still evaluates and commits every microturn and publishes its
/// semantic state/list deltas and emission routes. The fact authorizes only
/// elimination of recurrence-state routing; host effects, FLUSH, distributed
/// invocations, and foreign recurrence-state observers remain ineligible.
fn derive_pulse_fusion_decisions(
    semantic_program: &SemanticProgram,
) -> Result<Vec<VerifiedPulseFusionDecisionV1>, VerifyError> {
    let graph = semantic_program.reactive_graph();
    graph
        .pulse_batches
        .iter()
        .map(|batch| {
            let mut reasons = BTreeSet::new();

            let activation_state =
                batch
                    .enclosing_activation
                    .zip(batch.state)
                    .and_then(|(activation_id, state)| {
                        graph
                            .activations
                            .get(activation_id.as_usize())
                            .filter(|activation| {
                                activation.id == activation_id && activation.states.contains(&state)
                            })
                            .map(|_| (activation_id, state))
                    });
            if activation_state.is_none() {
                reasons.insert(PulseFusionIneligibilityV1::NoActivationLocalState);
            }

            let update_arm = if batch.state_update_arms.len() == 1 {
                let update_id = batch.state_update_arms[0];
                Some(
                    graph
                        .state_update_arms
                        .get(update_id.as_usize())
                        .filter(|arm| arm.id == update_id)
                        .ok_or_else(|| {
                            VerifyError::new(format!(
                                "pulse batch {} references missing state-update arm {}",
                                batch.id, update_id
                            ))
                        })?,
                )
            } else {
                reasons.insert(PulseFusionIneligibilityV1::StateUpdateCardinality);
                None
            };
            if let (Some((_, state)), Some(update_arm)) = (activation_state, update_arm)
                && update_arm.state != state
            {
                reasons.insert(PulseFusionIneligibilityV1::StateUpdateTargetMismatch);
            }

            if !batch.host_effect_schedules.is_empty() {
                reasons.insert(PulseFusionIneligibilityV1::HostEffect);
            }
            if !batch.flush_roots.is_empty() {
                reasons.insert(PulseFusionIneligibilityV1::Flush);
            }

            if let Some((_, state)) = activation_state {
                let state_cause = SemanticEventCauseV1::State(state);
                let has_observer = graph
                    .trigger_arms
                    .iter()
                    .any(|arm| arm.cause == state_cause)
                    || graph
                        .dependencies
                        .iter()
                        .any(|edge| edge.from == state_cause)
                    || graph
                        .list_mutations
                        .iter()
                        .any(|mutation| mutation.cause == state_cause)
                    || graph
                        .derived_values
                        .iter()
                        .any(|derived| derived.causes.contains(&state_cause));
                if has_observer {
                    reasons.insert(PulseFusionIneligibilityV1::StateHasReactiveObservers);
                }
            }

            let pulse_arms = batch.trigger_arms.iter().copied().collect::<BTreeSet<_>>();
            if graph.call_invocations.iter().any(|invocation| {
                invocation
                    .invocation_arms
                    .iter()
                    .any(|arm| pulse_arms.contains(arm))
            }) {
                reasons.insert(PulseFusionIneligibilityV1::DistributedInvocation);
            }

            let mut accounted_arms = BTreeSet::new();
            for update_id in &batch.state_update_arms {
                let update = graph
                    .state_update_arms
                    .get(update_id.as_usize())
                    .filter(|arm| arm.id == *update_id)
                    .ok_or_else(|| {
                        VerifyError::new(format!(
                            "pulse batch {} references missing state-update arm {}",
                            batch.id, update_id
                        ))
                    })?;
                accounted_arms.insert(update.trigger);
            }
            for mutation_id in &batch.list_mutations {
                let mutation = graph
                    .list_mutations
                    .get(mutation_id.as_usize())
                    .filter(|mutation| mutation.id == *mutation_id)
                    .ok_or_else(|| {
                        VerifyError::new(format!(
                            "pulse batch {} references missing list mutation {}",
                            batch.id, mutation_id
                        ))
                    })?;
                accounted_arms.insert(exact_list_mutation_trigger(graph, mutation)?.id);
            }
            for derived_id in &batch.derived_values {
                let derived = graph
                    .derived_values
                    .get(derived_id.as_usize())
                    .filter(|derived| derived.id == *derived_id)
                    .ok_or_else(|| {
                        VerifyError::new(format!(
                            "pulse batch {} references missing derived value {}",
                            batch.id, derived_id
                        ))
                    })?;
                accounted_arms.extend(derived.trigger_arms.iter().copied());
            }
            if pulse_arms != accounted_arms {
                reasons.insert(PulseFusionIneligibilityV1::UnaccountedPulseConsumer);
            }

            let status = if reasons.is_empty() {
                let (activation, state) = activation_state.expect("checked above");
                let state_update_arm = update_arm.expect("checked above").id;
                VerifiedPulseFusionStatusV1::Eligible {
                    fact: VerifiedPulseFusionFactV1 {
                        activation,
                        state,
                        state_update_arm,
                        count_policy:
                            VerifiedPulseFusionCountPolicyV1::FrozenAndRuntimeTargetGuardedBeforeFirstMicroturn,
                        trace_policy: if batch.list_mutations.is_empty() {
                            VerifiedPulseFusionTracePolicyV1::PreserveCommittedStateDeltasAndEmissionRoutes
                        } else {
                            VerifiedPulseFusionTracePolicyV1::PreserveCommittedStateAndListDeltasAndEmissionRoutes
                        },
                        elision_policy:
                            VerifiedPulseFusionElisionPolicyV1::ElideOnlyUnobservedRecurrenceStateRouting,
                    },
                }
            } else {
                VerifiedPulseFusionStatusV1::Ineligible {
                    reasons: reasons.into_iter().collect(),
                }
            };
            Ok(VerifiedPulseFusionDecisionV1 {
                pulse_batch: batch.id,
                semantic_slice_digest: batch.slice_digest,
                status,
            })
        })
        .collect()
}

fn exact_list_mutation_trigger<'a>(
    graph: &'a boon_semantic::SemanticReactiveGraphV1,
    mutation: &boon_semantic::SemanticListMutationV1,
) -> Result<&'a boon_semantic::SemanticTriggerOwnedArmV1, VerifyError> {
    let (gate, gate_value, output, output_value) = match mutation.kind {
        boon_semantic::SemanticListMutationKindV1::Append {
            gate,
            gate_value,
            item,
            item_value,
        } => (gate, gate_value, item, item_value),
        boon_semantic::SemanticListMutationKindV1::Remove {
            gate,
            gate_value,
            predicate,
            predicate_value,
            ..
        } => (gate, gate_value, predicate, predicate_value),
    };
    let matches = graph
        .trigger_arms
        .iter()
        .filter(|trigger| {
            trigger.cause == mutation.cause
                && trigger.gate_expression == gate
                && trigger.gate_value == gate_value
                && trigger.owner == mutation.owner
                && trigger.route_scope == mutation.route_scope
                && trigger.row_scope == mutation.row_scope
                && trigger.output_expression == output
                && trigger.output_value == output_value
        })
        .collect::<Vec<_>>();
    let [trigger] = matches.as_slice() else {
        return Err(VerifyError::new(format!(
            "pulse list mutation {} resolves to {} exact trigger arms",
            mutation.id,
            matches.len()
        )));
    };
    Ok(*trigger)
}

fn validate_pulse_fusion_decisions(
    decisions: &[VerifiedPulseFusionDecisionV1],
) -> Result<(), VerifyError> {
    let ids = decisions
        .iter()
        .map(|decision| decision.pulse_batch)
        .collect::<Vec<_>>();
    require_strictly_sorted_unique("pulse-fusion decision", &ids)?;
    for decision in decisions {
        if decision
            .semantic_slice_digest
            .0
            .iter()
            .all(|byte| *byte == 0)
        {
            return Err(VerifyError::new(format!(
                "pulse-fusion decision {} has a zero semantic slice digest",
                decision.pulse_batch
            )));
        }
        if let VerifiedPulseFusionStatusV1::Ineligible { reasons } = &decision.status {
            if reasons.is_empty() {
                return Err(VerifyError::new(format!(
                    "ineligible pulse-fusion decision {} has no diagnostic reason",
                    decision.pulse_batch
                )));
            }
            require_strictly_sorted_unique("pulse-fusion ineligibility reason", reasons)?;
        }
    }
    Ok(())
}

#[derive(Serialize)]
struct RequiredObligationDigestPayloadV1<'a> {
    schema: &'a str,
    checked_program_digest: CheckedProgramDigestV1,
    semantic_program_digest: SemanticProgramDigestV1,
    declared_contract_ids: &'a [ContractIdV1],
    declared_condition_ids: &'a [ConditionIdV1],
    contract_coverage_by_id: &'a [ContractCoverageV1],
    condition_coverage_by_id: &'a [ConditionCoverageV1],
    required_obligation_ids: &'a [ObligationIdV1],
    callable_dependency_manifest_hashes: &'a [CallableDependencyManifestDigestV1],
    proof_context_key_hashes: &'a [ProofContextKeyDigestV1],
    dependency_classifier_schema_hash: DependencyClassifierSchemaDigestV1,
    authority_activation_requirement_hashes: &'a [AuthorityActivationRequirementDigestV1],
    imported_verified_bundle_hashes: &'a [VerifiedBundleDigestV1],
    semantic_profile_hashes: &'a [SemanticProfileDigestV1],
    summary_hashes: &'a [SummaryDigestV1],
    verifier_policy: VerificationPolicyDigestV1,
}

fn requirement_digest_payload(
    manifest: &RequiredObligationManifestV1,
) -> RequiredObligationDigestPayloadV1<'_> {
    RequiredObligationDigestPayloadV1 {
        schema: &manifest.schema,
        checked_program_digest: manifest.checked_program_digest,
        semantic_program_digest: manifest.semantic_program_digest,
        declared_contract_ids: &manifest.declared_contract_ids,
        declared_condition_ids: &manifest.declared_condition_ids,
        contract_coverage_by_id: &manifest.contract_coverage_by_id,
        condition_coverage_by_id: &manifest.condition_coverage_by_id,
        required_obligation_ids: &manifest.required_obligation_ids,
        callable_dependency_manifest_hashes: &manifest.callable_dependency_manifest_hashes,
        proof_context_key_hashes: &manifest.proof_context_key_hashes,
        dependency_classifier_schema_hash: manifest.dependency_classifier_schema_hash,
        authority_activation_requirement_hashes: &manifest.authority_activation_requirement_hashes,
        imported_verified_bundle_hashes: &manifest.imported_verified_bundle_hashes,
        semantic_profile_hashes: &manifest.semantic_profile_hashes,
        summary_hashes: &manifest.summary_hashes,
        verifier_policy: manifest.verifier_policy,
    }
}

fn requirement_canonical_bytes(
    manifest: &RequiredObligationManifestV1,
) -> Result<Vec<u8>, VerifyError> {
    canonical_encoding(&requirement_digest_payload(manifest))
}

fn requirement_digest(
    manifest: &RequiredObligationManifestV1,
) -> Result<RequirementDigestV1, VerifyError> {
    Ok(RequirementDigestV1(domain_hash(
        REQUIREMENT_DIGEST_DOMAIN,
        &requirement_canonical_bytes(manifest)?,
    )))
}

#[derive(Serialize)]
struct VerificationManifestDigestPayloadV1<'a> {
    schema: &'a str,
    requirements: &'a RequiredObligationManifestV1,
    accepted_obligation_evidence_core_by_id: &'a [AcceptedObligationEvidenceCoreV1],
    #[serde(skip_serializing_if = "slice_is_empty")]
    pulse_fusion_decisions: &'a [VerifiedPulseFusionDecisionV1],
}

fn verification_manifest_digest_payload(
    manifest: &VerificationManifestV1,
) -> VerificationManifestDigestPayloadV1<'_> {
    VerificationManifestDigestPayloadV1 {
        schema: &manifest.schema,
        requirements: &manifest.requirements,
        accepted_obligation_evidence_core_by_id: &manifest.accepted_obligation_evidence_core_by_id,
        pulse_fusion_decisions: &manifest.pulse_fusion_decisions,
    }
}

fn slice_is_empty<T>(value: &&[T]) -> bool {
    value.is_empty()
}

fn verification_manifest_canonical_bytes(
    manifest: &VerificationManifestV1,
) -> Result<Vec<u8>, VerifyError> {
    canonical_encoding(&verification_manifest_digest_payload(manifest))
}

fn verification_manifest_digest(
    manifest: &VerificationManifestV1,
) -> Result<VerificationManifestDigestV1, VerifyError> {
    Ok(VerificationManifestDigestV1(domain_hash(
        VERIFICATION_MANIFEST_DIGEST_DOMAIN,
        &verification_manifest_canonical_bytes(manifest)?,
    )))
}

fn require_strictly_sorted_unique<T: Ord>(kind: &str, values: &[T]) -> Result<(), VerifyError> {
    if values.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err(VerifyError::new(format!(
            "{kind} values must be strictly sorted and unique"
        )));
    }
    Ok(())
}

fn canonical_encoding<T: Serialize + ?Sized>(value: &T) -> Result<Vec<u8>, VerifyError> {
    boon_contract::canonical_serde_cbor_v1(value)
        .map_err(|error| VerifyError::new(format!("canonical verifier encoding failed: {error}")))
}

fn domain_hash(domain: &[u8], bytes: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(domain);
    hasher.update(
        u64::try_from(bytes.len())
            .expect("Rust byte slices cannot exceed u64")
            .to_be_bytes(),
    );
    hasher.update(bytes);
    hasher.finalize().into()
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifyError {
    message: String,
}

impl VerifyError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for VerifyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for VerifyError {}

#[cfg(test)]
mod tests {
    use super::*;
    use boon_semantic::elaborate;
    use boon_typecheck::CheckedProgram;
    use serde::de::DeserializeOwned;

    fn checked_fixture() -> CheckedProgram {
        let parsed = boon_parser::parse_source("verify-fixture.bn", "value: 0").unwrap();
        boon_typecheck::check_program(&parsed)
            .program
            .expect("valid verifier fixture has one checked program")
    }

    fn empty_manifest() -> VerificationManifestV1 {
        let semantic = elaborate(checked_fixture(), &[]).unwrap();
        verify_explicit_contracts(semantic)
            .unwrap()
            .verification_manifest()
            .clone()
    }

    fn fibonacci_fusion_manifest() -> VerificationManifestV1 {
        let parsed = boon_parser::parse_source(
            "verify-fibonacci-fusion.bn",
            r#"
value: fibonacci(position: 10)

FUNCTION fibonacci(position) {
    position
    |> THEN {
        position |> WHILE {
            1 => 1

            n =>
                [previous: 0, current: 1]
                |> HOLD state {
                    n - 1
                    |> Stream/pulses()
                    |> THEN {
                        [
                            previous: state.current
                            current: state.previous + state.current
                        ]
                    }
                }
                |> Stream/skip(count: n - 1)
                |> .current
        }
    }
}
"#,
        )
        .unwrap();
        let checked = boon_typecheck::check_program(&parsed)
            .program
            .expect("valid Fibonacci verifier fixture");
        let semantic = elaborate(checked, &[]).unwrap();
        verify_explicit_contracts(semantic)
            .unwrap()
            .verification_manifest()
            .clone()
    }

    fn external_digest<T: DeserializeOwned>(byte: u8) -> T {
        let bytes = serde_json::to_vec(&[byte; 32]).unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    fn normalized_golden_hex(contents: &str) -> String {
        contents.split_whitespace().collect()
    }

    fn rebind_manifest(manifest: &mut VerificationManifestV1) {
        manifest.requirements.requirement_digest =
            requirement_digest(&manifest.requirements).unwrap();
        manifest.manifest_digest = verification_manifest_digest(manifest).unwrap();
    }

    fn assert_rejected_after_rebinding(
        mut manifest: VerificationManifestV1,
        expected_message: &str,
    ) {
        rebind_manifest(&mut manifest);
        let error = manifest.validate().unwrap_err();
        assert!(
            error.to_string().contains(expected_message),
            "expected `{expected_message}`, got `{error}`"
        );
    }

    fn assert_requirement_mutation_bound(
        label: &str,
        base: &RequiredObligationManifestV1,
        mutate: impl FnOnce(&mut RequiredObligationManifestV1),
    ) {
        let mut changed = base.clone();
        mutate(&mut changed);
        assert_ne!(
            requirement_digest(&changed).unwrap(),
            base.requirement_digest,
            "requirement digest does not bind {label}"
        );
        assert!(
            changed.validate().is_err(),
            "stale requirement digest accepted mutation of {label}"
        );
    }

    fn assert_verification_mutation_bound(
        label: &str,
        base: &VerificationManifestV1,
        mutate: impl FnOnce(&mut VerificationManifestV1),
    ) {
        let mut changed = base.clone();
        mutate(&mut changed);
        assert_ne!(
            verification_manifest_digest(&changed).unwrap(),
            base.manifest_digest,
            "verification digest does not bind {label}"
        );
        assert!(
            changed.validate().is_err(),
            "stale verification digest accepted mutation of {label}"
        );
    }

    fn deterministic_empty_manifest() -> VerificationManifestV1 {
        let requirements = RequiredObligationManifestV1 {
            schema: REQUIRED_OBLIGATION_MANIFEST_SCHEMA_V1.to_owned(),
            checked_program_digest: external_digest(1),
            semantic_program_digest: external_digest(2),
            declared_contract_ids: Vec::new(),
            declared_condition_ids: Vec::new(),
            contract_coverage_by_id: Vec::new(),
            condition_coverage_by_id: Vec::new(),
            required_obligation_ids: Vec::new(),
            callable_dependency_manifest_hashes: vec![external_digest(3)],
            proof_context_key_hashes: Vec::new(),
            dependency_classifier_schema_hash: external_digest(4),
            authority_activation_requirement_hashes: Vec::new(),
            imported_verified_bundle_hashes: Vec::new(),
            semantic_profile_hashes: Vec::new(),
            summary_hashes: Vec::new(),
            verifier_policy: VerificationPolicyDigestV1::from_bytes([5; 32]),
            requirement_digest: RequirementDigestV1::from_bytes([0; 32]),
        };
        let mut manifest = VerificationManifestV1 {
            schema: VERIFICATION_MANIFEST_SCHEMA_V1.to_owned(),
            requirements,
            accepted_obligation_evidence_core_by_id: Vec::new(),
            pulse_fusion_decisions: Vec::new(),
            manifest_digest: VerificationManifestDigestV1::from_bytes([0; 32]),
        };
        rebind_manifest(&mut manifest);
        manifest.validate().unwrap();
        manifest
    }

    fn evidence_core(
        obligation_id: ObligationIdV1,
        discharged_condition_ids: Vec<ConditionIdV1>,
        kind: AcceptedEvidenceKindV1,
        identity_byte: u8,
        requirements: &RequiredObligationManifestV1,
    ) -> AcceptedObligationEvidenceCoreV1 {
        AcceptedObligationEvidenceCoreV1 {
            obligation_id,
            discharged_condition_ids,
            normalized_vc_digest: NormalizedVerificationConditionDigestV1::from_bytes(
                [identity_byte; 32],
            ),
            logical_status: AcceptedLogicalStatusV1::Valid,
            replayable_proof_or_certificate_reference: AcceptedEvidenceReferenceV1 {
                kind,
                reference_digest: EvidenceReferenceDigestV1::from_bytes(
                    [identity_byte.wrapping_add(1); 32],
                ),
            },
            proof_or_checked_certificate_digest: EvidenceBodyDigestV1::from_bytes(
                [identity_byte.wrapping_add(2); 32],
            ),
            callable_dependency_manifest_hashes: requirements
                .callable_dependency_manifest_hashes
                .clone(),
            proof_context_key_hashes: requirements.proof_context_key_hashes.clone(),
            imported_assumption_bundle_hashes: requirements.imported_verified_bundle_hashes.clone(),
            summary_evidence_hashes: requirements.summary_hashes.clone(),
            semantic_profile_hashes: requirements.semantic_profile_hashes.clone(),
            assurance_dependencies: vec![AssuranceDependencyDigestV1::from_bytes([21; 32])],
            verifier_policy: requirements.verifier_policy,
            kernel_and_verifier_versions: vec![VerifierComponentDigestV1::from_bytes([22; 32])],
        }
    }

    fn nonempty_manifest() -> VerificationManifestV1 {
        let contract_id = ContractIdV1::new(10);
        let first_condition = ConditionIdV1::new(20);
        let second_condition = ConditionIdV1::new(21);
        let definition_obligation = ObligationIdV1::new(100);
        let instantiated_obligation = ObligationIdV1::new(101);
        let proof_context = ProofContextKeyDigestV1::from_bytes([15; 32]);

        let requirements = RequiredObligationManifestV1 {
            schema: REQUIRED_OBLIGATION_MANIFEST_SCHEMA_V1.to_owned(),
            checked_program_digest: external_digest(10),
            semantic_program_digest: external_digest(11),
            declared_contract_ids: vec![contract_id],
            declared_condition_ids: vec![first_condition, second_condition],
            contract_coverage_by_id: vec![ContractCoverageV1 {
                contract_id,
                condition_ids: vec![first_condition, second_condition],
                definition: ContractDefinitionCoverageV1::Symbolic,
                definition_obligation_ids: vec![definition_obligation],
                instantiations: vec![ObligationInstantiationV1 {
                    proof_context_key_hash: proof_context,
                    materialization_ids: vec![SemanticMaterializationIdV1::new(30)],
                    condition_ids: vec![first_condition, second_condition],
                    obligation_ids: vec![instantiated_obligation],
                }],
            }],
            condition_coverage_by_id: vec![
                ConditionCoverageV1 {
                    condition_id: first_condition,
                    symbolic_obligation_ids: vec![definition_obligation],
                    instantiated_obligation_ids: vec![instantiated_obligation],
                },
                ConditionCoverageV1 {
                    condition_id: second_condition,
                    symbolic_obligation_ids: vec![definition_obligation],
                    instantiated_obligation_ids: vec![instantiated_obligation],
                },
            ],
            required_obligation_ids: vec![definition_obligation, instantiated_obligation],
            callable_dependency_manifest_hashes: vec![external_digest(12)],
            proof_context_key_hashes: vec![proof_context],
            dependency_classifier_schema_hash: external_digest(14),
            authority_activation_requirement_hashes: vec![
                AuthorityActivationRequirementDigestV1::from_bytes([16; 32]),
            ],
            imported_verified_bundle_hashes: vec![VerifiedBundleDigestV1::from_bytes([17; 32])],
            semantic_profile_hashes: vec![SemanticProfileDigestV1::from_bytes([18; 32])],
            summary_hashes: vec![SummaryDigestV1::from_bytes([19; 32])],
            verifier_policy: VerificationPolicyDigestV1::from_bytes([20; 32]),
            requirement_digest: RequirementDigestV1::from_bytes([0; 32]),
        };
        let evidence = vec![
            evidence_core(
                definition_obligation,
                vec![first_condition, second_condition],
                AcceptedEvidenceKindV1::ReplayableProof,
                6,
                &requirements,
            ),
            evidence_core(
                instantiated_obligation,
                vec![first_condition, second_condition],
                AcceptedEvidenceKindV1::CheckedCertificate,
                9,
                &requirements,
            ),
        ];
        let mut manifest = VerificationManifestV1 {
            schema: VERIFICATION_MANIFEST_SCHEMA_V1.to_owned(),
            requirements,
            accepted_obligation_evidence_core_by_id: evidence,
            pulse_fusion_decisions: Vec::new(),
            manifest_digest: VerificationManifestDigestV1::from_bytes([0; 32]),
        };
        rebind_manifest(&mut manifest);
        manifest.validate().unwrap();
        manifest
    }

    #[test]
    fn no_where_program_has_complete_empty_contract_sets() {
        let manifest = empty_manifest();
        assert!(manifest.requirements.declared_contract_ids.is_empty());
        assert!(manifest.requirements.declared_condition_ids.is_empty());
        assert!(manifest.requirements.contract_coverage_by_id.is_empty());
        assert!(manifest.requirements.condition_coverage_by_id.is_empty());
        assert!(manifest.requirements.required_obligation_ids.is_empty());
        assert!(manifest.accepted_obligation_evidence_core_by_id.is_empty());
        manifest.validate().unwrap();
    }

    #[test]
    fn pulse_fusion_decisions_are_canonical_and_manifest_digest_bound() {
        let manifest = fibonacci_fusion_manifest();
        let [decision] = manifest.pulse_fusion_decisions.as_slice() else {
            panic!("canonical Fibonacci must have one pulse-fusion decision");
        };
        let VerifiedPulseFusionStatusV1::Eligible { fact } = &decision.status else {
            panic!("canonical Fibonacci must be fusion eligible");
        };
        assert_eq!(
            fact.count_policy,
            VerifiedPulseFusionCountPolicyV1::FrozenAndRuntimeTargetGuardedBeforeFirstMicroturn
        );
        assert_eq!(
            fact.trace_policy,
            VerifiedPulseFusionTracePolicyV1::PreserveCommittedStateDeltasAndEmissionRoutes
        );
        assert_eq!(
            fact.elision_policy,
            VerifiedPulseFusionElisionPolicyV1::ElideOnlyUnobservedRecurrenceStateRouting
        );
        let encoded = serde_json::to_vec(&manifest).unwrap();
        let decoded: VerificationManifestV1 = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(decoded, manifest);
        decoded.validate().unwrap();
        assert!(
            decision
                .semantic_slice_digest
                .0
                .iter()
                .any(|byte| *byte != 0)
        );

        let mut stale = manifest.clone();
        stale.pulse_fusion_decisions[0].status = VerifiedPulseFusionStatusV1::Ineligible {
            reasons: vec![PulseFusionIneligibilityV1::NoActivationLocalState],
        };
        assert!(
            stale
                .validate()
                .unwrap_err()
                .to_string()
                .contains("verification manifest digest does not match")
        );

        let mut empty_reasons = manifest.clone();
        empty_reasons.pulse_fusion_decisions[0].status = VerifiedPulseFusionStatusV1::Ineligible {
            reasons: Vec::new(),
        };
        assert_rejected_after_rebinding(
            empty_reasons,
            "ineligible pulse-fusion decision 0 has no diagnostic reason",
        );

        let mut duplicate = manifest;
        duplicate
            .pulse_fusion_decisions
            .push(duplicate.pulse_fusion_decisions[0].clone());
        assert_rejected_after_rebinding(
            duplicate,
            "pulse-fusion decision values must be strictly sorted and unique",
        );
    }

    #[test]
    fn empty_contract_sets_reject_any_single_nonempty_peer() {
        let nonempty = nonempty_manifest();

        let mut declared_only = empty_manifest();
        declared_only
            .requirements
            .declared_contract_ids
            .push(ContractIdV1::new(10));
        assert_rejected_after_rebinding(
            declared_only,
            "declared contract ids do not exactly equal contract coverage ids",
        );

        let mut covered_only = empty_manifest();
        covered_only.requirements.contract_coverage_by_id =
            nonempty.requirements.contract_coverage_by_id.clone();
        assert_rejected_after_rebinding(
            covered_only,
            "declared contract ids do not exactly equal contract coverage ids",
        );

        let mut required_only = empty_manifest();
        required_only
            .requirements
            .required_obligation_ids
            .push(ObligationIdV1::new(100));
        assert_rejected_after_rebinding(
            required_only,
            "union of coverage obligation ids does not exactly equal required obligation ids",
        );

        let mut evidence_only = empty_manifest();
        evidence_only.accepted_obligation_evidence_core_by_id =
            vec![nonempty.accepted_obligation_evidence_core_by_id[0].clone()];
        assert_rejected_after_rebinding(
            evidence_only,
            "required obligation ids do not exactly equal accepted evidence ids",
        );
    }

    #[test]
    fn nonempty_manifest_binds_declared_covered_required_and_evidence_sets() {
        let manifest = nonempty_manifest();
        let contract_ids = manifest
            .requirements
            .contract_coverage_by_id
            .iter()
            .map(|coverage| coverage.contract_id)
            .collect::<Vec<_>>();
        let condition_ids = manifest
            .requirements
            .condition_coverage_by_id
            .iter()
            .map(|coverage| coverage.condition_id)
            .collect::<Vec<_>>();
        let coverage_obligation_ids = manifest
            .requirements
            .contract_coverage_by_id
            .iter()
            .flat_map(|coverage| {
                coverage.definition_obligation_ids.iter().copied().chain(
                    coverage
                        .instantiations
                        .iter()
                        .flat_map(|instantiation| instantiation.obligation_ids.iter().copied()),
                )
            })
            .collect::<BTreeSet<_>>();
        let evidence_ids = manifest
            .accepted_obligation_evidence_core_by_id
            .iter()
            .map(|evidence| evidence.obligation_id)
            .collect::<Vec<_>>();

        assert_eq!(manifest.requirements.declared_contract_ids, contract_ids);
        assert_eq!(manifest.requirements.declared_condition_ids, condition_ids);
        assert_eq!(
            manifest
                .requirements
                .required_obligation_ids
                .iter()
                .copied()
                .collect::<BTreeSet<_>>(),
            coverage_obligation_ids
        );
        assert_eq!(manifest.requirements.required_obligation_ids, evidence_ids);
    }

    #[test]
    fn declared_contract_ids_must_equal_contract_coverage_ids() {
        let mut manifest = nonempty_manifest();
        manifest.requirements.contract_coverage_by_id.clear();
        assert_rejected_after_rebinding(
            manifest,
            "declared contract ids do not exactly equal contract coverage ids",
        );
    }

    #[test]
    fn declared_condition_ids_must_equal_condition_coverage_ids() {
        let mut manifest = nonempty_manifest();
        manifest.requirements.condition_coverage_by_id.pop();
        assert_rejected_after_rebinding(
            manifest,
            "declared condition ids do not exactly equal condition coverage ids",
        );
    }

    #[test]
    fn declared_condition_ids_must_equal_contract_child_condition_ids() {
        let mut manifest = nonempty_manifest();
        manifest.requirements.contract_coverage_by_id[0]
            .condition_ids
            .pop();
        manifest.requirements.contract_coverage_by_id[0].instantiations[0]
            .condition_ids
            .pop();
        assert_rejected_after_rebinding(
            manifest,
            "declared condition ids do not exactly equal contract child condition ids",
        );
    }

    #[test]
    fn contract_definition_obligations_must_equal_symbolic_condition_coverage() {
        let mut manifest = nonempty_manifest();
        manifest.requirements.contract_coverage_by_id[0]
            .definition_obligation_ids
            .insert(0, ObligationIdV1::new(99));
        assert_rejected_after_rebinding(
            manifest,
            "definition obligations are not exactly covered by its conditions",
        );
    }

    #[test]
    fn every_condition_requires_definition_mode_coverage() {
        let mut symbolic = nonempty_manifest();
        symbolic.requirements.condition_coverage_by_id[1]
            .symbolic_obligation_ids
            .clear();
        assert_rejected_after_rebinding(
            symbolic,
            "condition 21 has no coverage for its contract definition mode",
        );

        let mut materialization_dependent = nonempty_manifest();
        materialization_dependent
            .requirements
            .contract_coverage_by_id[0]
            .definition = ContractDefinitionCoverageV1::MaterializationDependent;
        materialization_dependent
            .requirements
            .contract_coverage_by_id[0]
            .instantiations[0]
            .condition_ids
            .pop();
        materialization_dependent
            .requirements
            .condition_coverage_by_id[1]
            .instantiated_obligation_ids
            .clear();
        assert_rejected_after_rebinding(
            materialization_dependent,
            "condition 21 has no coverage for its contract definition mode",
        );
    }

    #[test]
    fn instantiated_condition_coverage_must_equal_contract_instantiations() {
        let mut manifest = nonempty_manifest();
        manifest.requirements.condition_coverage_by_id[0]
            .instantiated_obligation_ids
            .clear();
        assert_rejected_after_rebinding(
            manifest,
            "instantiated coverage does not exactly match its contract instantiations",
        );
    }

    #[test]
    fn coverage_obligation_union_must_equal_required_obligations() {
        let mut manifest = nonempty_manifest();
        manifest
            .requirements
            .required_obligation_ids
            .push(ObligationIdV1::new(102));
        assert_rejected_after_rebinding(
            manifest,
            "union of coverage obligation ids does not exactly equal required obligation ids",
        );
    }

    #[test]
    fn required_obligations_must_equal_accepted_evidence_ids() {
        let mut manifest = nonempty_manifest();
        manifest.accepted_obligation_evidence_core_by_id.pop();
        assert_rejected_after_rebinding(
            manifest,
            "required obligation ids do not exactly equal accepted evidence ids",
        );
    }

    #[test]
    fn evidence_condition_ids_must_equal_condition_coverage() {
        let mut manifest = nonempty_manifest();
        manifest.accepted_obligation_evidence_core_by_id[0]
            .discharged_condition_ids
            .pop();
        assert_rejected_after_rebinding(
            manifest,
            "discharged conditions do not exactly match condition coverage",
        );
    }

    #[test]
    fn requirement_dependency_profile_and_import_hashes_are_sorted_unique() {
        let mut dependencies = nonempty_manifest();
        let dependency = dependencies
            .requirements
            .callable_dependency_manifest_hashes[0];
        dependencies
            .requirements
            .callable_dependency_manifest_hashes
            .push(dependency);
        assert_rejected_after_rebinding(
            dependencies,
            "callable dependency manifest hash values must be strictly sorted and unique",
        );

        let mut profiles = nonempty_manifest();
        profiles.requirements.semantic_profile_hashes = vec![
            SemanticProfileDigestV1::from_bytes([1; 32]),
            SemanticProfileDigestV1::from_bytes([1; 32]),
        ];
        assert_rejected_after_rebinding(
            profiles,
            "semantic profile hash values must be strictly sorted and unique",
        );

        let mut imports = nonempty_manifest();
        imports.requirements.imported_verified_bundle_hashes = vec![
            VerifiedBundleDigestV1::from_bytes([2; 32]),
            VerifiedBundleDigestV1::from_bytes([1; 32]),
        ];
        assert_rejected_after_rebinding(
            imports,
            "imported verified bundle hash values must be strictly sorted and unique",
        );
    }

    #[test]
    fn evidence_dependency_profile_and_import_hashes_are_sorted_unique() {
        let mut dependencies = nonempty_manifest();
        let dependency = dependencies.accepted_obligation_evidence_core_by_id[0]
            .callable_dependency_manifest_hashes[0];
        dependencies.accepted_obligation_evidence_core_by_id[0]
            .callable_dependency_manifest_hashes
            .push(dependency);
        assert_rejected_after_rebinding(
            dependencies,
            "evidence callable dependency manifest hash values must be strictly sorted and unique",
        );

        let profile_one = SemanticProfileDigestV1::from_bytes([1; 32]);
        let profile_two = SemanticProfileDigestV1::from_bytes([2; 32]);
        let mut profiles = nonempty_manifest();
        profiles.requirements.semantic_profile_hashes = vec![profile_one, profile_two];
        profiles.accepted_obligation_evidence_core_by_id[0].semantic_profile_hashes =
            vec![profile_one, profile_one];
        assert_rejected_after_rebinding(
            profiles,
            "evidence semantic profile hash values must be strictly sorted and unique",
        );

        let import_one = VerifiedBundleDigestV1::from_bytes([1; 32]);
        let import_two = VerifiedBundleDigestV1::from_bytes([2; 32]);
        let mut imports = nonempty_manifest();
        imports.requirements.imported_verified_bundle_hashes = vec![import_one, import_two];
        imports.accepted_obligation_evidence_core_by_id[0].imported_assumption_bundle_hashes =
            vec![import_two, import_one];
        assert_rejected_after_rebinding(
            imports,
            "evidence imported assumption bundle hash values must be strictly sorted and unique",
        );
    }

    #[test]
    fn requirement_digest_is_bound_to_the_canonical_requirement_payload() {
        let mut manifest = nonempty_manifest();
        manifest.requirements.requirement_digest = RequirementDigestV1::from_bytes([7; 32]);
        let error = manifest.requirements.validate().unwrap_err();
        assert!(
            error
                .to_string()
                .contains("digest does not match its canonical payload")
        );
    }

    #[test]
    fn verification_digest_is_bound_to_the_canonical_evidence_payload() {
        let mut manifest = nonempty_manifest();
        manifest.accepted_obligation_evidence_core_by_id[0].normalized_vc_digest =
            NormalizedVerificationConditionDigestV1::from_bytes([77; 32]);
        let error = manifest.validate().unwrap_err();
        assert!(
            error
                .to_string()
                .contains("verification manifest digest does not match its canonical payload")
        );
    }

    #[test]
    fn requirement_digest_binds_every_v1_input_class() {
        let manifest = nonempty_manifest();
        let requirements = &manifest.requirements;

        assert_requirement_mutation_bound("schema", requirements, |value| {
            value.schema.push_str(".mutated");
        });
        assert_requirement_mutation_bound("checked program digest", requirements, |value| {
            value.checked_program_digest = external_digest(90);
        });
        assert_requirement_mutation_bound("semantic program digest", requirements, |value| {
            value.semantic_program_digest = external_digest(91);
        });
        assert_requirement_mutation_bound("declared contract IDs", requirements, |value| {
            value.declared_contract_ids[0] = ContractIdV1::new(11);
        });
        assert_requirement_mutation_bound("declared condition IDs", requirements, |value| {
            value.declared_condition_ids[0] = ConditionIdV1::new(19);
        });
        assert_requirement_mutation_bound("contract coverage identity", requirements, |value| {
            value.contract_coverage_by_id[0].contract_id = ContractIdV1::new(11);
        });
        assert_requirement_mutation_bound(
            "contract child condition coverage",
            requirements,
            |value| {
                value.contract_coverage_by_id[0].condition_ids[0] = ConditionIdV1::new(19);
            },
        );
        assert_requirement_mutation_bound("contract definition mode", requirements, |value| {
            value.contract_coverage_by_id[0].definition =
                ContractDefinitionCoverageV1::MaterializationDependent;
        });
        assert_requirement_mutation_bound(
            "definition obligation coverage",
            requirements,
            |value| {
                value.contract_coverage_by_id[0].definition_obligation_ids[0] =
                    ObligationIdV1::new(99);
            },
        );
        assert_requirement_mutation_bound("instantiation proof context", requirements, |value| {
            value.contract_coverage_by_id[0].instantiations[0].proof_context_key_hash =
                ProofContextKeyDigestV1::from_bytes([23; 32]);
        });
        assert_requirement_mutation_bound(
            "instantiation materialization IDs",
            requirements,
            |value| {
                value.contract_coverage_by_id[0].instantiations[0].materialization_ids[0] =
                    SemanticMaterializationIdV1::new(29);
            },
        );
        assert_requirement_mutation_bound("instantiation condition IDs", requirements, |value| {
            value.contract_coverage_by_id[0].instantiations[0].condition_ids[0] =
                ConditionIdV1::new(19);
        });
        assert_requirement_mutation_bound("instantiation obligation IDs", requirements, |value| {
            value.contract_coverage_by_id[0].instantiations[0].obligation_ids[0] =
                ObligationIdV1::new(102);
        });
        assert_requirement_mutation_bound("condition coverage identity", requirements, |value| {
            value.condition_coverage_by_id[0].condition_id = ConditionIdV1::new(19);
        });
        assert_requirement_mutation_bound("symbolic condition coverage", requirements, |value| {
            value.condition_coverage_by_id[0].symbolic_obligation_ids[0] = ObligationIdV1::new(99);
        });
        assert_requirement_mutation_bound(
            "instantiated condition coverage",
            requirements,
            |value| {
                value.condition_coverage_by_id[0].instantiated_obligation_ids[0] =
                    ObligationIdV1::new(102);
            },
        );
        assert_requirement_mutation_bound("required obligation IDs", requirements, |value| {
            value.required_obligation_ids[0] = ObligationIdV1::new(99);
        });
        assert_requirement_mutation_bound(
            "callable dependency manifest hashes",
            requirements,
            |value| {
                value.callable_dependency_manifest_hashes[0] = external_digest(11);
            },
        );
        assert_requirement_mutation_bound("proof context key hashes", requirements, |value| {
            value.proof_context_key_hashes[0] = ProofContextKeyDigestV1::from_bytes([19; 32]);
        });
        assert_requirement_mutation_bound(
            "dependency classifier schema hash",
            requirements,
            |value| {
                value.dependency_classifier_schema_hash = external_digest(92);
            },
        );
        assert_requirement_mutation_bound(
            "authority activation requirement hashes",
            requirements,
            |value| {
                value.authority_activation_requirement_hashes[0] =
                    AuthorityActivationRequirementDigestV1::from_bytes([29; 32]);
            },
        );
        assert_requirement_mutation_bound(
            "imported verified bundle hashes",
            requirements,
            |value| {
                value.imported_verified_bundle_hashes[0] =
                    VerifiedBundleDigestV1::from_bytes([39; 32]);
            },
        );
        assert_requirement_mutation_bound("semantic profile hashes", requirements, |value| {
            value.semantic_profile_hashes[0] = SemanticProfileDigestV1::from_bytes([49; 32]);
        });
        assert_requirement_mutation_bound("summary hashes", requirements, |value| {
            value.summary_hashes[0] = SummaryDigestV1::from_bytes([51; 32]);
        });
        assert_requirement_mutation_bound("verifier policy", requirements, |value| {
            value.verifier_policy = VerificationPolicyDigestV1::from_bytes([55; 32]);
        });
    }

    #[test]
    fn verification_digest_binds_finalized_requirements_and_every_evidence_input_class() {
        let manifest = nonempty_manifest();

        assert_verification_mutation_bound("schema", &manifest, |value| {
            value.schema.push_str(".mutated");
        });
        assert_verification_mutation_bound("finalized requirement digest", &manifest, |value| {
            value.requirements.requirement_digest = RequirementDigestV1::from_bytes([99; 32]);
        });
        assert_verification_mutation_bound("evidence obligation ID", &manifest, |value| {
            value.accepted_obligation_evidence_core_by_id[0].obligation_id =
                ObligationIdV1::new(99);
        });
        assert_verification_mutation_bound("discharged condition IDs", &manifest, |value| {
            value.accepted_obligation_evidence_core_by_id[0]
                .discharged_condition_ids
                .remove(0);
        });
        assert_verification_mutation_bound(
            "normalized verification condition",
            &manifest,
            |value| {
                value.accepted_obligation_evidence_core_by_id[0].normalized_vc_digest =
                    NormalizedVerificationConditionDigestV1::from_bytes([79; 32]);
            },
        );
        assert_verification_mutation_bound("evidence kind", &manifest, |value| {
            value.accepted_obligation_evidence_core_by_id[0]
                .replayable_proof_or_certificate_reference
                .kind = AcceptedEvidenceKindV1::CheckedCertificate;
        });
        assert_verification_mutation_bound("evidence reference digest", &manifest, |value| {
            value.accepted_obligation_evidence_core_by_id[0]
                .replayable_proof_or_certificate_reference
                .reference_digest = EvidenceReferenceDigestV1::from_bytes([82; 32]);
        });
        assert_verification_mutation_bound(
            "proof or certificate body digest",
            &manifest,
            |value| {
                value.accepted_obligation_evidence_core_by_id[0]
                    .proof_or_checked_certificate_digest =
                    EvidenceBodyDigestV1::from_bytes([83; 32]);
            },
        );
        assert_verification_mutation_bound(
            "evidence callable dependency hashes",
            &manifest,
            |value| {
                value.accepted_obligation_evidence_core_by_id[0]
                    .callable_dependency_manifest_hashes
                    .remove(0);
            },
        );
        assert_verification_mutation_bound("evidence proof context hashes", &manifest, |value| {
            value.accepted_obligation_evidence_core_by_id[0]
                .proof_context_key_hashes
                .remove(0);
        });
        assert_verification_mutation_bound("evidence imported bundle hashes", &manifest, |value| {
            value.accepted_obligation_evidence_core_by_id[0]
                .imported_assumption_bundle_hashes
                .remove(0);
        });
        assert_verification_mutation_bound("evidence summary hashes", &manifest, |value| {
            value.accepted_obligation_evidence_core_by_id[0]
                .summary_evidence_hashes
                .remove(0);
        });
        assert_verification_mutation_bound(
            "evidence semantic profile hashes",
            &manifest,
            |value| {
                value.accepted_obligation_evidence_core_by_id[0]
                    .semantic_profile_hashes
                    .remove(0);
            },
        );
        assert_verification_mutation_bound("evidence assurance dependencies", &manifest, |value| {
            value.accepted_obligation_evidence_core_by_id[0]
                .assurance_dependencies
                .remove(0);
        });
        assert_verification_mutation_bound("evidence verifier policy", &manifest, |value| {
            value.accepted_obligation_evidence_core_by_id[0].verifier_policy =
                VerificationPolicyDigestV1::from_bytes([55; 32]);
        });
        assert_verification_mutation_bound("kernel and verifier versions", &manifest, |value| {
            value.accepted_obligation_evidence_core_by_id[0]
                .kernel_and_verifier_versions
                .remove(0);
        });

        let mut rebound_requirement = manifest.clone();
        rebound_requirement.requirements.checked_program_digest = external_digest(93);
        rebound_requirement.requirements.requirement_digest =
            requirement_digest(&rebound_requirement.requirements).unwrap();
        rebound_requirement.requirements.validate().unwrap();
        assert_ne!(
            verification_manifest_digest(&rebound_requirement).unwrap(),
            manifest.manifest_digest,
            "verification digest does not bind the complete finalized requirements"
        );
        assert!(
            rebound_requirement
                .validate()
                .unwrap_err()
                .to_string()
                .contains("verification manifest digest does not match")
        );

        assert_eq!(
            manifest.accepted_obligation_evidence_core_by_id[0].logical_status,
            AcceptedLogicalStatusV1::Valid,
            "V1 has one accepted logical-status discriminant, frozen by the byte golden"
        );
    }

    #[test]
    fn requirement_ordered_vectors_reject_duplicates_and_reversal() {
        macro_rules! reject {
            ($mutation:expr, $message:literal) => {{
                let mut manifest = nonempty_manifest();
                $mutation(&mut manifest);
                assert_rejected_after_rebinding(manifest, $message);
            }};
        }

        reject!(
            |value: &mut VerificationManifestV1| value
                .requirements
                .declared_contract_ids
                .push(ContractIdV1::new(10)),
            "declared contract values must be strictly sorted and unique"
        );
        reject!(
            |value: &mut VerificationManifestV1| value
                .requirements
                .declared_condition_ids
                .reverse(),
            "declared condition values must be strictly sorted and unique"
        );
        reject!(
            |value: &mut VerificationManifestV1| value
                .requirements
                .required_obligation_ids
                .reverse(),
            "required obligation values must be strictly sorted and unique"
        );
        reject!(
            |value: &mut VerificationManifestV1| {
                let duplicate = value.requirements.contract_coverage_by_id[0].clone();
                value.requirements.contract_coverage_by_id.push(duplicate);
            },
            "contract coverage values must be strictly sorted and unique"
        );
        reject!(
            |value: &mut VerificationManifestV1| value
                .requirements
                .condition_coverage_by_id
                .reverse(),
            "condition coverage values must be strictly sorted and unique"
        );
        reject!(
            |value: &mut VerificationManifestV1| value.requirements.contract_coverage_by_id[0]
                .condition_ids
                .reverse(),
            "contract child condition values must be strictly sorted and unique"
        );
        reject!(
            |value: &mut VerificationManifestV1| value.requirements.contract_coverage_by_id[0]
                .definition_obligation_ids
                .push(ObligationIdV1::new(100)),
            "definition obligation values must be strictly sorted and unique"
        );
        reject!(
            |value: &mut VerificationManifestV1| {
                let duplicate =
                    value.requirements.contract_coverage_by_id[0].instantiations[0].clone();
                value.requirements.contract_coverage_by_id[0]
                    .instantiations
                    .push(duplicate);
            },
            "obligation instantiation values must be strictly sorted and unique"
        );
        reject!(
            |value: &mut VerificationManifestV1| {
                value.requirements.contract_coverage_by_id[0].instantiations[0]
                    .materialization_ids
                    .push(SemanticMaterializationIdV1::new(30));
            },
            "instantiation materialization values must be strictly sorted and unique"
        );
        reject!(
            |value: &mut VerificationManifestV1| value.requirements.contract_coverage_by_id[0]
                .instantiations[0]
                .condition_ids
                .reverse(),
            "instantiation condition values must be strictly sorted and unique"
        );
        reject!(
            |value: &mut VerificationManifestV1| value.requirements.contract_coverage_by_id[0]
                .instantiations[0]
                .obligation_ids
                .push(ObligationIdV1::new(101)),
            "instantiation obligation values must be strictly sorted and unique"
        );
        reject!(
            |value: &mut VerificationManifestV1| value.requirements.condition_coverage_by_id[0]
                .symbolic_obligation_ids
                .push(ObligationIdV1::new(100)),
            "symbolic condition obligation values must be strictly sorted and unique"
        );
        reject!(
            |value: &mut VerificationManifestV1| value.requirements.condition_coverage_by_id[0]
                .instantiated_obligation_ids
                .push(ObligationIdV1::new(101)),
            "instantiated condition obligation values must be strictly sorted and unique"
        );
        reject!(
            |value: &mut VerificationManifestV1| {
                let duplicate = value.requirements.callable_dependency_manifest_hashes[0];
                value
                    .requirements
                    .callable_dependency_manifest_hashes
                    .push(duplicate);
            },
            "callable dependency manifest hash values must be strictly sorted and unique"
        );
        reject!(
            |value: &mut VerificationManifestV1| {
                let duplicate = value.requirements.proof_context_key_hashes[0];
                value.requirements.proof_context_key_hashes.push(duplicate);
            },
            "proof-context-key hash values must be strictly sorted and unique"
        );
        reject!(
            |value: &mut VerificationManifestV1| {
                let duplicate = value.requirements.authority_activation_requirement_hashes[0];
                value
                    .requirements
                    .authority_activation_requirement_hashes
                    .push(duplicate);
            },
            "authority activation requirement hash values must be strictly sorted and unique"
        );
        reject!(
            |value: &mut VerificationManifestV1| {
                let duplicate = value.requirements.imported_verified_bundle_hashes[0];
                value
                    .requirements
                    .imported_verified_bundle_hashes
                    .push(duplicate);
            },
            "imported verified bundle hash values must be strictly sorted and unique"
        );
        reject!(
            |value: &mut VerificationManifestV1| {
                let duplicate = value.requirements.semantic_profile_hashes[0];
                value.requirements.semantic_profile_hashes.push(duplicate);
            },
            "semantic profile hash values must be strictly sorted and unique"
        );
        reject!(
            |value: &mut VerificationManifestV1| {
                let duplicate = value.requirements.summary_hashes[0];
                value.requirements.summary_hashes.push(duplicate);
            },
            "summary hash values must be strictly sorted and unique"
        );
    }

    #[test]
    fn evidence_ordered_vectors_reject_duplicates_and_reversal() {
        macro_rules! reject {
            ($mutation:expr, $message:literal) => {{
                let mut manifest = nonempty_manifest();
                $mutation(&mut manifest);
                assert_rejected_after_rebinding(manifest, $message);
            }};
        }

        reject!(
            |value: &mut VerificationManifestV1| value
                .accepted_obligation_evidence_core_by_id
                .reverse(),
            "accepted obligation evidence values must be strictly sorted and unique"
        );
        reject!(
            |value: &mut VerificationManifestV1| value.accepted_obligation_evidence_core_by_id[0]
                .discharged_condition_ids
                .reverse(),
            "evidence discharged condition values must be strictly sorted and unique"
        );
        reject!(
            |value: &mut VerificationManifestV1| {
                let evidence = &mut value.accepted_obligation_evidence_core_by_id[0];
                let duplicate = evidence.callable_dependency_manifest_hashes[0];
                evidence.callable_dependency_manifest_hashes.push(duplicate);
            },
            "evidence callable dependency manifest hash values must be strictly sorted and unique"
        );
        reject!(
            |value: &mut VerificationManifestV1| {
                let evidence = &mut value.accepted_obligation_evidence_core_by_id[0];
                let duplicate = evidence.proof_context_key_hashes[0];
                evidence.proof_context_key_hashes.push(duplicate);
            },
            "evidence proof-context-key hash values must be strictly sorted and unique"
        );
        reject!(
            |value: &mut VerificationManifestV1| {
                let evidence = &mut value.accepted_obligation_evidence_core_by_id[0];
                let duplicate = evidence.imported_assumption_bundle_hashes[0];
                evidence.imported_assumption_bundle_hashes.push(duplicate);
            },
            "evidence imported assumption bundle hash values must be strictly sorted and unique"
        );
        reject!(
            |value: &mut VerificationManifestV1| {
                let evidence = &mut value.accepted_obligation_evidence_core_by_id[0];
                let duplicate = evidence.summary_evidence_hashes[0];
                evidence.summary_evidence_hashes.push(duplicate);
            },
            "evidence summary hash values must be strictly sorted and unique"
        );
        reject!(
            |value: &mut VerificationManifestV1| {
                let evidence = &mut value.accepted_obligation_evidence_core_by_id[0];
                let duplicate = evidence.semantic_profile_hashes[0];
                evidence.semantic_profile_hashes.push(duplicate);
            },
            "evidence semantic profile hash values must be strictly sorted and unique"
        );
        reject!(
            |value: &mut VerificationManifestV1| {
                let evidence = &mut value.accepted_obligation_evidence_core_by_id[0];
                let duplicate = evidence.assurance_dependencies[0];
                evidence.assurance_dependencies.push(duplicate);
            },
            "evidence assurance dependency hash values must be strictly sorted and unique"
        );
        reject!(
            |value: &mut VerificationManifestV1| {
                let evidence = &mut value.accepted_obligation_evidence_core_by_id[0];
                let duplicate = evidence.kernel_and_verifier_versions[0];
                evidence.kernel_and_verifier_versions.push(duplicate);
            },
            "evidence verifier component hash values must be strictly sorted and unique"
        );
    }

    #[test]
    fn empty_no_where_manifest_has_frozen_canonical_payloads_and_sha256() {
        let manifest = deterministic_empty_manifest();

        assert_eq!(
            hex(&requirement_canonical_bytes(&manifest.requirements).unwrap()),
            normalized_golden_hex(include_str!("../testdata/empty_requirement_payload_v1.hex"))
        );
        assert_eq!(
            manifest.requirements.requirement_digest.to_hex(),
            "ea6ea43f3dd865f799879e5b15fd2a1753da31db7c9c0a0982e5a74c2897755d"
        );
        assert_eq!(
            hex(&verification_manifest_canonical_bytes(&manifest).unwrap()),
            normalized_golden_hex(include_str!(
                "../testdata/empty_verification_payload_v1.hex"
            ))
        );
        assert_eq!(
            manifest.manifest_digest.to_hex(),
            "e29eeb0d9e43e0cb9e1dd5ce4806525751cf60f24c5367c83684ccc3745398e0"
        );
        manifest.validate().unwrap();
    }

    #[test]
    fn fully_populated_manifest_has_frozen_canonical_payloads_and_sha256() {
        let manifest = nonempty_manifest();
        let requirements = &manifest.requirements;
        let contract = &requirements.contract_coverage_by_id[0];
        let instantiation = &contract.instantiations[0];

        assert!(!requirements.declared_contract_ids.is_empty());
        assert!(!requirements.declared_condition_ids.is_empty());
        assert!(!requirements.contract_coverage_by_id.is_empty());
        assert!(!requirements.condition_coverage_by_id.is_empty());
        assert!(!contract.condition_ids.is_empty());
        assert!(!contract.definition_obligation_ids.is_empty());
        assert!(!contract.instantiations.is_empty());
        assert!(!instantiation.materialization_ids.is_empty());
        assert!(!instantiation.condition_ids.is_empty());
        assert!(!instantiation.obligation_ids.is_empty());
        assert!(!requirements.required_obligation_ids.is_empty());
        assert!(!requirements.callable_dependency_manifest_hashes.is_empty());
        assert!(!requirements.proof_context_key_hashes.is_empty());
        assert!(
            !requirements
                .authority_activation_requirement_hashes
                .is_empty()
        );
        assert!(!requirements.imported_verified_bundle_hashes.is_empty());
        assert!(!requirements.semantic_profile_hashes.is_empty());
        assert!(!requirements.summary_hashes.is_empty());
        assert!(
            requirements
                .condition_coverage_by_id
                .iter()
                .all(|coverage| !coverage.symbolic_obligation_ids.is_empty()
                    && !coverage.instantiated_obligation_ids.is_empty())
        );
        assert!(
            manifest
                .accepted_obligation_evidence_core_by_id
                .iter()
                .all(|evidence| {
                    !evidence.discharged_condition_ids.is_empty()
                        && !evidence.callable_dependency_manifest_hashes.is_empty()
                        && !evidence.proof_context_key_hashes.is_empty()
                        && !evidence.imported_assumption_bundle_hashes.is_empty()
                        && !evidence.summary_evidence_hashes.is_empty()
                        && !evidence.semantic_profile_hashes.is_empty()
                        && !evidence.assurance_dependencies.is_empty()
                        && !evidence.kernel_and_verifier_versions.is_empty()
                })
        );
        assert_eq!(
            manifest
                .accepted_obligation_evidence_core_by_id
                .iter()
                .map(|evidence| evidence.replayable_proof_or_certificate_reference.kind)
                .collect::<Vec<_>>(),
            vec![
                AcceptedEvidenceKindV1::ReplayableProof,
                AcceptedEvidenceKindV1::CheckedCertificate,
            ]
        );

        assert_eq!(
            hex(&requirement_canonical_bytes(requirements).unwrap()),
            normalized_golden_hex(include_str!(
                "../testdata/nonempty_requirement_payload_v1.hex"
            ))
        );
        assert_eq!(
            requirements.requirement_digest.to_hex(),
            "3b9c7a2c63cd34a97ab043930c1d8c3769839dfe40238e1c7cf3bf9057fc128f"
        );
        assert_eq!(
            hex(&verification_manifest_canonical_bytes(&manifest).unwrap()),
            normalized_golden_hex(include_str!(
                "../testdata/nonempty_verification_payload_v1.hex"
            ))
        );
        assert_eq!(
            manifest.manifest_digest.to_hex(),
            "ad206609b7f535f1778314b1f6cc601665bcff848bead2a61120569c6b1b2923"
        );
        manifest.validate().unwrap();
    }
}
