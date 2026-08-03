use super::{
    BundleVerificationManifestDigestV1, EXPLICIT_CONTRACT_COVERAGE_V1, VerificationManifestV1,
    VerificationPolicyDigestV1, VerificationPolicyV1, VerifyError, canonical_encoding, domain_hash,
    verify_semantic_program,
};
use boon_checked::ProgramRole;
use boon_semantic::{
    BundleSemanticProgramDigestV1, BundleSemanticProgramV1, SemanticProgramDigestV1,
};
use serde::{Deserialize, Serialize};

pub const BUNDLE_VERIFICATION_MANIFEST_SCHEMA_V1: &str = "boon.bundle-verification-manifest.v1";
const BUNDLE_VERIFICATION_MANIFEST_DIGEST_DOMAIN: &[u8] = b"boon.bundle-verification-manifest.v1\0";

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct BundleRoleVerificationV1 {
    pub role: ProgramRole,
    pub semantic_program_digest: SemanticProgramDigestV1,
    pub requirement_digest: super::RequirementDigestV1,
    pub verification_manifest_digest: super::VerificationManifestDigestV1,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct BundleVerificationManifestV1 {
    pub schema: String,
    pub bundle_semantic_program_digest: BundleSemanticProgramDigestV1,
    pub role_verifications: Vec<BundleRoleVerificationV1>,
    pub verifier_policy: VerificationPolicyDigestV1,
    pub manifest_digest: BundleVerificationManifestDigestV1,
}

impl BundleVerificationManifestV1 {
    pub fn validate(&self) -> Result<(), VerifyError> {
        if self.schema != BUNDLE_VERIFICATION_MANIFEST_SCHEMA_V1 {
            return Err(VerifyError::new(format!(
                "unsupported bundle-verification manifest schema `{}`",
                self.schema
            )));
        }
        if self
            .bundle_semantic_program_digest
            .as_bytes()
            .iter()
            .all(|byte| *byte == 0)
        {
            return Err(VerifyError::new(
                "bundle-verification manifest has a zero semantic bundle digest",
            ));
        }
        require_exact_role_order(
            self.role_verifications
                .iter()
                .map(|verification| verification.role),
            "bundle-verification manifest",
        )?;
        for verification in &self.role_verifications {
            if verification
                .semantic_program_digest
                .as_bytes()
                .iter()
                .all(|byte| *byte == 0)
                || verification.requirement_digest.is_zero()
                || verification.verification_manifest_digest.is_zero()
            {
                return Err(VerifyError::new(format!(
                    "bundle-verification manifest has a zero {:?} role digest",
                    verification.role
                )));
            }
        }
        if self.verifier_policy != VerificationPolicyV1::explicit_contracts_bootstrap().digest() {
            return Err(VerifyError::new(
                "bundle-verification manifest uses an unsupported verifier policy",
            ));
        }
        if self.manifest_digest != bundle_verification_manifest_digest(self)? {
            return Err(VerifyError::new(
                "bundle-verification manifest digest does not match its canonical payload",
            ));
        }
        Ok(())
    }
}

struct VerifiedBundleRoleV1 {
    role: ProgramRole,
    verification_manifest: VerificationManifestV1,
}

/// Atomic proof-gated ownership token for distributed semantic lowering.
///
/// It owns the exact frozen semantic bundle and all three role manifests under
/// one policy. Fields and construction are private:
///
/// ```compile_fail
/// use boon_verify::ContractVerifiedBundle;
///
/// let _forged = ContractVerifiedBundle {
///     semantic_bundle: todo!(),
///     role_verifications: todo!(),
///     bundle_manifest: todo!(),
///     coverage: "explicit_contracts_v1",
/// };
/// ```
///
/// The token is deliberately not cloneable:
///
/// ```compile_fail
/// use boon_verify::ContractVerifiedBundle;
///
/// fn requires_clone<T: Clone>() {}
/// requires_clone::<ContractVerifiedBundle>();
/// ```
pub struct ContractVerifiedBundle {
    semantic_bundle: BundleSemanticProgramV1,
    role_verifications: Vec<VerifiedBundleRoleV1>,
    bundle_manifest: BundleVerificationManifestV1,
    coverage: &'static str,
}

impl ContractVerifiedBundle {
    pub const fn bundle_semantic_program_digest(&self) -> BundleSemanticProgramDigestV1 {
        self.bundle_manifest.bundle_semantic_program_digest
    }

    pub const fn bundle_verification_manifest_digest(&self) -> BundleVerificationManifestDigestV1 {
        self.bundle_manifest.manifest_digest
    }

    pub const fn bundle_verification_manifest(&self) -> &BundleVerificationManifestV1 {
        &self.bundle_manifest
    }

    pub const fn verifier_policy(&self) -> VerificationPolicyDigestV1 {
        self.bundle_manifest.verifier_policy
    }

    pub const fn coverage(&self) -> &'static str {
        self.coverage
    }

    pub fn role_verification_manifest(&self, role: ProgramRole) -> Option<&VerificationManifestV1> {
        self.role_verifications.iter().find_map(|verification| {
            (verification.role == role).then_some(&verification.verification_manifest)
        })
    }

    /// Consumed only by the atomic distributed erasure entrypoint in
    /// `boon_ir`. The compiler never receives independently constructible role
    /// verification tokens.
    #[doc(hidden)]
    pub fn into_lowering_parts(
        self,
    ) -> (
        BundleSemanticProgramV1,
        Vec<(ProgramRole, VerificationManifestV1)>,
        BundleVerificationManifestV1,
    ) {
        (
            self.semantic_bundle,
            self.role_verifications
                .into_iter()
                .map(|verification| (verification.role, verification.verification_manifest))
                .collect(),
            self.bundle_manifest,
        )
    }

    fn validate(&self) -> Result<(), VerifyError> {
        self.semantic_bundle
            .validate()
            .map_err(|error| VerifyError::new(error.to_string()))?;
        self.bundle_manifest.validate()?;
        if self.coverage != EXPLICIT_CONTRACT_COVERAGE_V1 {
            return Err(VerifyError::new(
                "verified semantic bundle reports unsupported proof coverage",
            ));
        }
        if self.semantic_bundle.digest() != self.bundle_manifest.bundle_semantic_program_digest {
            return Err(VerifyError::new(
                "verified semantic bundle digest differs from its verification manifest",
            ));
        }
        require_exact_role_order(
            self.role_verifications
                .iter()
                .map(|verification| verification.role),
            "verified semantic bundle",
        )?;
        reject_v1_contracted_role_crossings(&self.semantic_bundle, &self.role_verifications)?;

        let policy = VerificationPolicyV1::explicit_contracts_bootstrap();
        if self.bundle_manifest.verifier_policy != policy.digest() {
            return Err(VerifyError::new(
                "verified semantic bundle uses an unsupported verifier policy",
            ));
        }

        let semantic_roles = self.semantic_bundle.role_programs().collect::<Vec<_>>();
        if semantic_roles.len() != self.role_verifications.len()
            || semantic_roles.len() != self.bundle_manifest.role_verifications.len()
        {
            return Err(VerifyError::new(
                "verified semantic bundle role coverage is incomplete",
            ));
        }
        for (((semantic_role, semantic_program), verified), manifest_role) in semantic_roles
            .into_iter()
            .zip(&self.role_verifications)
            .zip(&self.bundle_manifest.role_verifications)
        {
            if semantic_role != verified.role || verified.role != manifest_role.role {
                return Err(VerifyError::new(
                    "verified semantic bundle role identities do not align",
                ));
            }
            verified.verification_manifest.validate()?;
            let requirements = &verified.verification_manifest.requirements;
            if requirements.verifier_policy != self.bundle_manifest.verifier_policy {
                return Err(VerifyError::new(format!(
                    "{:?} role uses a different verifier policy",
                    verified.role
                )));
            }
            if semantic_program.digest() != requirements.semantic_program_digest
                || manifest_role.semantic_program_digest != semantic_program.digest()
                || manifest_role.requirement_digest != requirements.requirement_digest
                || manifest_role.verification_manifest_digest
                    != verified.verification_manifest.manifest_digest
            {
                return Err(VerifyError::new(format!(
                    "{:?} role verification does not bind its exact semantic program and manifests",
                    verified.role
                )));
            }

            let expected = verify_semantic_program(semantic_program, policy)?;
            if verified.verification_manifest != expected {
                return Err(VerifyError::new(format!(
                    "{:?} role verification manifest is not the complete manifest derived from its semantic program",
                    verified.role
                )));
            }
        }
        Ok(())
    }
}

/// Verifies all roles in one frozen semantic bundle under one explicit policy.
///
/// The bundle is consumed, and successful verification returns one non-clone
/// token. No independently verified role token is accepted as input.
pub fn verify_bundle(
    semantic_bundle: BundleSemanticProgramV1,
    policy: VerificationPolicyV1,
) -> Result<ContractVerifiedBundle, VerifyError> {
    policy.validate()?;
    semantic_bundle
        .validate()
        .map_err(|error| VerifyError::new(error.to_string()))?;

    let mut role_verifications = Vec::with_capacity(semantic_bundle.role_programs().len());
    let mut role_manifest_entries = Vec::with_capacity(semantic_bundle.role_programs().len());
    for (role, semantic_program) in semantic_bundle.role_programs() {
        let verification_manifest = verify_semantic_program(semantic_program, policy)?;
        role_manifest_entries.push(BundleRoleVerificationV1 {
            role,
            semantic_program_digest: semantic_program.digest(),
            requirement_digest: verification_manifest.requirements.requirement_digest,
            verification_manifest_digest: verification_manifest.manifest_digest,
        });
        role_verifications.push(VerifiedBundleRoleV1 {
            role,
            verification_manifest,
        });
    }
    reject_v1_contracted_role_crossings(&semantic_bundle, &role_verifications)?;
    require_exact_role_order(
        role_verifications
            .iter()
            .map(|verification| verification.role),
        "semantic bundle verification",
    )?;

    let mut bundle_manifest = BundleVerificationManifestV1 {
        schema: BUNDLE_VERIFICATION_MANIFEST_SCHEMA_V1.to_owned(),
        bundle_semantic_program_digest: semantic_bundle.digest(),
        role_verifications: role_manifest_entries,
        verifier_policy: policy.digest(),
        manifest_digest: BundleVerificationManifestDigestV1::from_bytes([0; 32]),
    };
    bundle_manifest.manifest_digest = bundle_verification_manifest_digest(&bundle_manifest)?;
    bundle_manifest.validate()?;

    let verified = ContractVerifiedBundle {
        semantic_bundle,
        role_verifications,
        bundle_manifest,
        coverage: EXPLICIT_CONTRACT_COVERAGE_V1,
    };
    verified.validate()?;
    Ok(verified)
}

fn reject_v1_contracted_role_crossings(
    semantic_bundle: &BundleSemanticProgramV1,
    role_verifications: &[VerifiedBundleRoleV1],
) -> Result<(), VerifyError> {
    if semantic_bundle.call_crossings().is_empty() && semantic_bundle.value_crossings().is_empty() {
        return Ok(());
    }
    for role in role_verifications {
        let requirements = &role.verification_manifest.requirements;
        let has_contract_surface = !requirements.declared_contract_ids.is_empty()
            || !requirements.declared_condition_ids.is_empty()
            || !requirements.contract_coverage_by_id.is_empty()
            || !requirements.condition_coverage_by_id.is_empty()
            || !requirements.required_obligation_ids.is_empty()
            || !requirements.proof_context_key_hashes.is_empty()
            || !requirements
                .authority_activation_requirement_hashes
                .is_empty()
            || !requirements.imported_verified_bundle_hashes.is_empty()
            || !requirements.semantic_profile_hashes.is_empty()
            || !requirements.summary_hashes.is_empty()
            || !role
                .verification_manifest
                .accepted_obligation_evidence_core_by_id
                .is_empty();
        if has_contract_surface {
            return Err(VerifyError::new(format!(
                "{:?} role has contract obligations across a distributed role boundary; V1 rejects contracted role/external crossings",
                role.role
            )));
        }
    }
    Ok(())
}

#[derive(Serialize)]
struct BundleVerificationManifestDigestPayloadV1<'a> {
    schema: &'a str,
    bundle_semantic_program_digest: BundleSemanticProgramDigestV1,
    role_verifications: &'a [BundleRoleVerificationV1],
    verifier_policy: VerificationPolicyDigestV1,
}

fn bundle_verification_manifest_digest_payload(
    manifest: &BundleVerificationManifestV1,
) -> BundleVerificationManifestDigestPayloadV1<'_> {
    BundleVerificationManifestDigestPayloadV1 {
        schema: &manifest.schema,
        bundle_semantic_program_digest: manifest.bundle_semantic_program_digest,
        role_verifications: &manifest.role_verifications,
        verifier_policy: manifest.verifier_policy,
    }
}

fn bundle_verification_manifest_digest(
    manifest: &BundleVerificationManifestV1,
) -> Result<BundleVerificationManifestDigestV1, VerifyError> {
    Ok(BundleVerificationManifestDigestV1::from_bytes(domain_hash(
        BUNDLE_VERIFICATION_MANIFEST_DIGEST_DOMAIN,
        &canonical_encoding(&bundle_verification_manifest_digest_payload(manifest))?,
    )))
}

fn require_exact_role_order(
    roles: impl IntoIterator<Item = ProgramRole>,
    context: &str,
) -> Result<(), VerifyError> {
    let roles = roles.into_iter().collect::<Vec<_>>();
    let expected = [
        ProgramRole::Client,
        ProgramRole::Session,
        ProgramRole::Server,
    ];
    if roles != expected {
        return Err(VerifyError::new(format!(
            "{context} must contain exactly Client, Session, and Server in canonical order",
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use boon_checked::{
        CheckedExternalDeclarationIdentityV1, CheckedExternalDeclarationKind,
        ExternalTypeEnvironment, FlowMode, FlowType, Type,
    };
    use boon_parser::parse_source;
    use boon_semantic::elaborate;
    use boon_typecheck::check_runtime_program_profiled_with_external_types;
    use serde::de::DeserializeOwned;

    type BundleManifestMutation = (&'static str, fn(&mut BundleVerificationManifestV1));

    fn external_digest<T: DeserializeOwned>(byte: u8) -> T {
        let bytes = serde_json::to_vec(&[byte; 32]).unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    fn semantic_role(role: ProgramRole, value: u8) -> boon_semantic::SemanticProgram {
        let parsed = parse_source(
            format!("verify-bundle-{}.bn", role.as_str()),
            format!("store: [value: {value}]"),
        )
        .unwrap();
        let (checked, _) = check_runtime_program_profiled_with_external_types(
            &parsed,
            &ExternalTypeEnvironment::empty(role),
        );
        let checked = checked
            .program
            .expect("valid verifier bundle role has one checked program");
        elaborate(checked, &[]).expect("valid verifier bundle role elaborates")
    }

    fn semantic_bundle(value: u8) -> BundleSemanticProgramV1 {
        BundleSemanticProgramV1::freeze([
            semantic_role(ProgramRole::Client, value),
            semantic_role(ProgramRole::Session, value),
            semantic_role(ProgramRole::Server, value),
        ])
        .expect("three empty-crossing roles freeze")
    }

    fn semantic_value_crossing_bundle() -> BundleSemanticProgramV1 {
        let session_parsed = parse_source(
            "verify-bundle-session-crossing.bn",
            "store: [\n    value: 1\n]\n",
        )
        .unwrap();
        let (session_checked, _) = check_runtime_program_profiled_with_external_types(
            &session_parsed,
            &ExternalTypeEnvironment::empty(ProgramRole::Session),
        );
        let session_checked = session_checked
            .program
            .expect("session crossing fixture checks");
        let declaration = session_checked
            .declarations
            .iter()
            .find(|declaration| {
                session_checked.declaration_path(declaration.id).as_deref() == Some("store.value")
            })
            .expect("session crossing fixture has store value");
        let external_identity = CheckedExternalDeclarationIdentityV1 {
            producer_role: ProgramRole::Session,
            producer_source_bundle_digest_v1: session_checked.source_bundle_digest_v1,
            producer_declaration: declaration.id,
            kind: CheckedExternalDeclarationKind::Value,
        };
        let mut client_environment = ExternalTypeEnvironment::sealed(ProgramRole::Client);
        client_environment.values.insert(
            "Session/store.value".to_owned(),
            FlowType {
                mode: FlowMode::Continuous,
                ty: Type::Number,
            },
        );
        client_environment
            .external_identities
            .insert("Session/store.value".to_owned(), external_identity);
        let client_parsed = parse_source(
            "verify-bundle-client-crossing.bn",
            "store: [\n    remote: Session/store.value\n]\n",
        )
        .unwrap();
        let (client_checked, _) =
            check_runtime_program_profiled_with_external_types(&client_parsed, &client_environment);
        let client_checked = client_checked
            .program
            .expect("client crossing fixture checks");

        BundleSemanticProgramV1::freeze([
            elaborate(client_checked, &[]).expect("client crossing role elaborates"),
            elaborate(session_checked, &[]).expect("session crossing role elaborates"),
            semantic_role(ProgramRole::Server, 0),
        ])
        .expect("value-crossing semantic bundle freezes")
    }

    fn verified_bundle(value: u8) -> ContractVerifiedBundle {
        verify_bundle(
            semantic_bundle(value),
            VerificationPolicyV1::explicit_contracts_bootstrap(),
        )
        .expect("empty-contract semantic bundle verifies")
    }

    fn rebind_role_manifest(manifest: &mut VerificationManifestV1) {
        manifest.requirements.requirement_digest =
            super::super::requirement_digest(&manifest.requirements).unwrap();
        manifest.manifest_digest = super::super::verification_manifest_digest(manifest).unwrap();
    }

    fn rebind_bundle_manifest(manifest: &mut BundleVerificationManifestV1) {
        manifest.manifest_digest = bundle_verification_manifest_digest(manifest).unwrap();
    }

    #[test]
    fn bundle_manifest_binds_every_role_and_one_policy() {
        let verified = verified_bundle(1);
        let manifest = verified.bundle_verification_manifest();
        manifest.validate().unwrap();
        assert_eq!(
            manifest
                .role_verifications
                .iter()
                .map(|verification| verification.role)
                .collect::<Vec<_>>(),
            [
                ProgramRole::Client,
                ProgramRole::Session,
                ProgramRole::Server,
            ]
        );
        assert_eq!(
            manifest.verifier_policy,
            VerificationPolicyV1::explicit_contracts_bootstrap().digest()
        );
        for role in [
            ProgramRole::Client,
            ProgramRole::Session,
            ProgramRole::Server,
        ] {
            let role_manifest = verified
                .role_verification_manifest(role)
                .expect("every role has a verification manifest");
            assert_eq!(
                role_manifest.requirements.verifier_policy,
                manifest.verifier_policy
            );
        }
    }

    #[test]
    fn v1_rejects_contract_obligations_when_a_role_crossing_exists() {
        let mut crossing = verify_bundle(
            semantic_value_crossing_bundle(),
            VerificationPolicyV1::explicit_contracts_bootstrap(),
        )
        .expect("uncontracted value crossing verifies");
        assert_eq!(crossing.semantic_bundle.value_crossings().len(), 1);
        crossing.role_verifications[0]
            .verification_manifest
            .requirements
            .declared_contract_ids
            .push(super::super::ContractIdV1::new(1));
        let error = crossing.validate().unwrap_err();
        assert!(
            error
                .to_string()
                .contains("V1 rejects contracted role/external crossings"),
            "unexpected error: {error}"
        );

        type AssuranceMutation = fn(&mut VerificationManifestV1);
        let assurance_mutations: [(&str, AssuranceMutation); 4] = [
            ("proof context", |manifest| {
                manifest
                    .requirements
                    .proof_context_key_hashes
                    .push(super::super::ProofContextKeyDigestV1::from_bytes([1; 32]));
            }),
            ("authority activation", |manifest| {
                manifest
                    .requirements
                    .authority_activation_requirement_hashes
                    .push(
                        super::super::AuthorityActivationRequirementDigestV1::from_bytes([2; 32]),
                    );
            }),
            ("semantic profile", |manifest| {
                manifest
                    .requirements
                    .semantic_profile_hashes
                    .push(super::super::SemanticProfileDigestV1::from_bytes([3; 32]));
            }),
            ("summary", |manifest| {
                manifest
                    .requirements
                    .summary_hashes
                    .push(super::super::SummaryDigestV1::from_bytes([4; 32]));
            }),
        ];
        for (label, mutate) in assurance_mutations {
            let mut crossing = verify_bundle(
                semantic_value_crossing_bundle(),
                VerificationPolicyV1::explicit_contracts_bootstrap(),
            )
            .expect("uncontracted value crossing verifies");
            mutate(&mut crossing.role_verifications[0].verification_manifest);
            let error = reject_v1_contracted_role_crossings(
                &crossing.semantic_bundle,
                &crossing.role_verifications,
            )
            .unwrap_err();
            assert!(
                error
                    .to_string()
                    .contains("V1 rejects contracted role/external crossings"),
                "{label} was not rejected: {error}"
            );
        }

        let mut local_only = verified_bundle(12);
        local_only.role_verifications[0]
            .verification_manifest
            .requirements
            .declared_contract_ids
            .push(super::super::ContractIdV1::new(1));
        reject_v1_contracted_role_crossings(
            &local_only.semantic_bundle,
            &local_only.role_verifications,
        )
        .expect("a bundle with no role crossing is not rejected by the crossing ratchet");
    }

    #[test]
    fn bundle_verification_is_deterministic_for_the_same_frozen_semantics_and_policy() {
        let first = verified_bundle(8);
        let second = verified_bundle(8);
        assert_eq!(
            first.bundle_semantic_program_digest(),
            second.bundle_semantic_program_digest()
        );
        assert_eq!(
            first.bundle_verification_manifest(),
            second.bundle_verification_manifest()
        );
        for role in [
            ProgramRole::Client,
            ProgramRole::Session,
            ProgramRole::Server,
        ] {
            assert_eq!(
                first.role_verification_manifest(role),
                second.role_verification_manifest(role)
            );
        }
    }

    #[test]
    fn bundle_manifest_digest_binds_all_payload_fields() {
        let verified = verified_bundle(2);
        let base = verified.bundle_verification_manifest();
        let mutations: [BundleManifestMutation; 5] = [
            ("schema", |manifest| manifest.schema.push_str(".mutated")),
            ("bundle semantic digest", |manifest| {
                manifest.bundle_semantic_program_digest = external_digest(91)
            }),
            ("role semantic digest", |manifest| {
                manifest.role_verifications[0].semantic_program_digest = external_digest(92)
            }),
            ("role requirement digest", |manifest| {
                manifest.role_verifications[0].requirement_digest =
                    super::super::RequirementDigestV1::from_bytes([93; 32])
            }),
            ("role verification digest", |manifest| {
                manifest.role_verifications[0].verification_manifest_digest =
                    super::super::VerificationManifestDigestV1::from_bytes([94; 32])
            }),
        ];
        for (label, mutate) in mutations {
            let mut changed = base.clone();
            mutate(&mut changed);
            assert_ne!(
                bundle_verification_manifest_digest(&changed).unwrap(),
                base.manifest_digest,
                "bundle manifest digest does not bind {label}"
            );
            assert!(
                changed.validate().is_err(),
                "stale bundle manifest digest accepted {label} mutation"
            );
        }

        let mut policy = base.clone();
        policy.verifier_policy = VerificationPolicyDigestV1::from_bytes([95; 32]);
        assert_ne!(
            bundle_verification_manifest_digest(&policy).unwrap(),
            base.manifest_digest
        );
        assert!(policy.validate().is_err());
    }

    #[test]
    fn bundle_manifest_rejects_missing_duplicate_and_reordered_roles_after_rebinding() {
        let base = verified_bundle(3).bundle_manifest;
        let mut missing = base.clone();
        missing.role_verifications.remove(1);
        rebind_bundle_manifest(&mut missing);
        assert!(
            missing
                .validate()
                .unwrap_err()
                .to_string()
                .contains("exactly Client, Session, and Server in canonical order")
        );

        let mut duplicate = base.clone();
        duplicate.role_verifications[1].role = ProgramRole::Client;
        rebind_bundle_manifest(&mut duplicate);
        assert!(duplicate.validate().is_err());

        let mut reordered = base;
        reordered.role_verifications.swap(0, 1);
        rebind_bundle_manifest(&mut reordered);
        assert!(reordered.validate().is_err());
    }

    #[test]
    fn verified_bundle_rejects_role_policy_drift_even_after_all_digests_are_rebound() {
        let mut verified = verified_bundle(4);
        let session = &mut verified.role_verifications[1].verification_manifest;
        session.requirements.verifier_policy = VerificationPolicyDigestV1::from_bytes([71; 32]);
        rebind_role_manifest(session);
        verified.bundle_manifest.role_verifications[1].requirement_digest =
            session.requirements.requirement_digest;
        verified.bundle_manifest.role_verifications[1].verification_manifest_digest =
            session.manifest_digest;
        rebind_bundle_manifest(&mut verified.bundle_manifest);

        let error = verified.validate().unwrap_err();
        assert!(
            error
                .to_string()
                .contains("role uses a different verifier policy"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn verified_bundle_rejects_rebound_role_digest_substitution() {
        let mut verified = verified_bundle(9);
        verified.bundle_manifest.role_verifications[0].requirement_digest =
            super::super::RequirementDigestV1::from_bytes([61; 32]);
        rebind_bundle_manifest(&mut verified.bundle_manifest);
        verified.bundle_manifest.validate().unwrap();

        let error = verified.validate().unwrap_err();
        assert!(
            error
                .to_string()
                .contains("role verification does not bind its exact semantic program"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn verified_bundle_rejects_role_omission_and_cross_bundle_substitution() {
        let mut omitted = verified_bundle(5);
        omitted.role_verifications.remove(1);
        assert!(
            omitted
                .validate()
                .unwrap_err()
                .to_string()
                .contains("exactly Client, Session, and Server in canonical order")
        );

        let mut mixed = verified_bundle(6);
        mixed.semantic_bundle = semantic_bundle(7);
        mixed.bundle_manifest.bundle_semantic_program_digest = mixed.semantic_bundle.digest();
        rebind_bundle_manifest(&mut mixed.bundle_manifest);
        assert!(
            mixed
                .validate()
                .unwrap_err()
                .to_string()
                .contains("role verification does not bind its exact semantic program")
        );
    }

    #[test]
    fn explicit_policy_is_selected_once_and_rejects_internal_forgery() {
        let implemented = VerificationPolicyV1::explicit_contracts_bootstrap();
        implemented.validate().unwrap();
        let forged = VerificationPolicyV1 {
            digest: VerificationPolicyDigestV1::from_bytes([99; 32]),
        };
        assert!(
            forged
                .validate()
                .unwrap_err()
                .to_string()
                .contains("not implemented")
        );
    }

    #[test]
    fn verified_bundle_is_consumed_as_one_ordered_lowering_authority() {
        let verified = verified_bundle(10);
        let semantic_digest = verified.bundle_semantic_program_digest();
        let manifest_digest = verified.bundle_verification_manifest_digest();
        let (semantic, roles, manifest) = verified.into_lowering_parts();

        assert_eq!(semantic.digest(), semantic_digest);
        assert_eq!(manifest.manifest_digest, manifest_digest);
        assert_eq!(
            roles.iter().map(|(role, _)| *role).collect::<Vec<_>>(),
            [
                ProgramRole::Client,
                ProgramRole::Session,
                ProgramRole::Server,
            ]
        );
        assert!(roles.iter().all(|(_, role_manifest)| {
            role_manifest.requirements.verifier_policy == manifest.verifier_policy
        }));
    }
}
