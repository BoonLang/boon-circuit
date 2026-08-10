use crate::{
    OwnerAbiEvaluationScope, OwnerArgumentKind, OwnerConstraintEdgeRole, OwnerConstraintNodeKind,
    OwnerConstraintSeed, OwnerConstraintSummary, OwnerInferenceAbiEnvironment,
    OwnerInterfaceEvaluationScope, OwnerLexicalAccess, OwnerLexicalBoundaryBindings,
    OwnerLexicalContainmentPlan, OwnerLexicalDeclarationTarget, OwnerLexicalPlan,
    OwnerParameterKind, OwnerPublicInterface, OwnerReferenceKind, OwnerSymbolReference,
    OwnerSymbolResolution, stable_check_owner_key_fingerprint_v1,
};
use boon_checked::{
    CheckedCallContextKind, CheckedCallableKind, CheckedParameterKind, CheckedParameterRequirement,
    OwnerDeclarationStableKey, OwnerLexicalDeclarationCapability, OwnerLexicalTargetRef,
    OwnerScopeStableKey, OwnerStableScopeRef,
};
use boon_compilation_db::{
    DenseProjectionGraphBuilder, ProjectionGraphDigestDomains, ProjectionGraphStats, ProjectionId,
};
use boon_syntax::{StableCheckOwnerKey, StableExpressionKey};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt;
use std::sync::Arc;

const OWNER_SIGNATURE_LEXICAL_PLAN_DOMAIN_V3: &[u8] = b"boon.owner-signature-lexical-plan.v3\0";
const OWNER_SIGNATURE_LEXICAL_READS_DOMAIN_V2: &[u8] = b"boon.owner-signature-lexical-reads.v2\0";
const OWNER_SIGNATURE_LEXICAL_INPUTS_DOMAIN_V2: &[u8] = b"boon.owner-signature-lexical-inputs.v2\0";
const OWNER_SIGNATURE_LEXICAL_ENVIRONMENT_DOMAIN_V2: &[u8] =
    b"boon.owner-signature-lexical-environment.v2\0";
const OWNER_CALLABLE_SCOPE_COMPONENT_DOMAIN_V2: &[u8] = b"boon.owner-callable-scope-component.v2\0";
const OWNER_LEXICAL_CONTAINMENT_CLUSTER_DOMAIN_V1: &[u8] =
    b"boon.owner-lexical-containment-cluster.v1\0";
const OWNER_CALLABLE_RESOLUTION_PLAN_DOMAIN_V2: &[u8] = b"boon.owner-callable-resolution-plan.v2\0";
const OWNER_CALLABLE_SCOPE_SCC_DOMAIN_V2: &[u8] = b"boon.owner-callable-scope-scc.v2\0";
const OWNER_CALLABLE_SCOPE_TOPOLOGY_DOMAIN_V2: &[u8] = b"boon.owner-callable-scope-topology.v2\0";
const OWNER_CALLABLE_SCOPE_RESULT_DOMAIN_V2: &[u8] = b"boon.owner-callable-scope-result.v2\0";
const OWNER_CALLABLE_SCOPE_CURRENTNESS_DOMAIN_V2: &[u8] =
    b"boon.owner-callable-scope-currentness.v2\0";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OwnerSignatureLexicalPlanError {
    message: String,
}

/// Exact callable-only symbol outcomes for one owner.
///
/// This projection intentionally precedes ordinary value resolution. It is
/// enough to match call entries and infer lexical scope effects without
/// accidentally creating a project dependency for a spelling later owned by
/// FreshOut or CallContext.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OwnerCallableResolutionPlan {
    owner: StableCheckOwnerKey,
    resolutions: Box<[OwnerSymbolResolution]>,
    #[serde(skip)]
    containment: Arc<OwnerLexicalContainmentPlan>,
    #[serde(skip)]
    fingerprint_v1: [u8; 32],
}

impl OwnerCallableResolutionPlan {
    pub fn owner(&self) -> &StableCheckOwnerKey {
        &self.owner
    }

    pub fn resolutions(&self) -> &[OwnerSymbolResolution] {
        &self.resolutions
    }

    pub fn containment(&self) -> &Arc<OwnerLexicalContainmentPlan> {
        &self.containment
    }

    pub const fn fingerprint_v1(&self) -> [u8; 32] {
        self.fingerprint_v1
    }

    pub fn authoritative_abi_names(&self) -> Box<[String]> {
        self.resolutions
            .iter()
            .filter(|resolution| {
                matches!(
                    resolution,
                    OwnerSymbolResolution::Authoritative { .. }
                        | OwnerSymbolResolution::Unresolved { .. }
                )
            })
            .map(|resolution| resolution.reference().parts.join("/"))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>()
            .into_boxed_slice()
    }

    pub(crate) fn matches_seed(&self, seed: &OwnerConstraintSeed) -> bool {
        self.owner == seed.owner
            && self.containment.fingerprint_v1()
                == seed.signature_regions().containment().fingerprint_v1()
            && self
                .resolutions
                .iter()
                .map(OwnerSymbolResolution::reference)
                .collect::<BTreeSet<_>>()
                == seed
                    .references
                    .iter()
                    .filter(|reference| reference.kind == OwnerReferenceKind::Callable)
                    .collect::<BTreeSet<_>>()
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct OwnerCallableScopeSccKey {
    pub members: Box<[StableCheckOwnerKey]>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OwnerLexicalContainmentCluster {
    pub root: StableCheckOwnerKey,
    pub members: Box<[StableCheckOwnerKey]>,
    fingerprint_v1: [u8; 32],
}

impl OwnerLexicalContainmentCluster {
    pub const fn fingerprint_v1(&self) -> [u8; 32] {
        self.fingerprint_v1
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OwnerCallableScopeScc {
    pub key: OwnerCallableScopeSccKey,
    pub containment_clusters: Box<[OwnerLexicalContainmentCluster]>,
    pub call_cyclic: bool,
    pub dependencies: Box<[OwnerCallableScopeSccKey]>,
    fingerprint_v1: [u8; 32],
}

impl OwnerCallableScopeScc {
    pub const fn fingerprint_v1(&self) -> [u8; 32] {
        self.fingerprint_v1
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OwnerCallableScopeTopology {
    pub sccs: Box<[OwnerCallableScopeScc]>,
    pub stats: ProjectionGraphStats,
    scc_by_owner: BTreeMap<StableCheckOwnerKey, usize>,
    fingerprint_v1: [u8; 32],
}

impl OwnerCallableScopeTopology {
    pub fn scc_for_owner(&self, owner: &StableCheckOwnerKey) -> Option<&OwnerCallableScopeScc> {
        self.sccs.get(*self.scc_by_owner.get(owner)?)
    }

    pub const fn fingerprint_v1(&self) -> [u8; 32] {
        self.fingerprint_v1
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OwnerCallableScopeOwnerResult {
    owner: StableCheckOwnerKey,
    signature: OwnerCallableLexicalSignature,
    lexical_plan: OwnerSignatureLexicalPlan,
}

impl OwnerCallableScopeOwnerResult {
    pub fn owner(&self) -> &StableCheckOwnerKey {
        &self.owner
    }

    pub fn signature(&self) -> &OwnerCallableLexicalSignature {
        &self.signature
    }

    pub fn lexical_plan(&self) -> &OwnerSignatureLexicalPlan {
        &self.lexical_plan
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OwnerCallableScopeSccResult {
    pub key: OwnerCallableScopeSccKey,
    pub owners: Box<[OwnerCallableScopeOwnerResult]>,
    fingerprint_v1: [u8; 32],
}

impl OwnerCallableScopeSccResult {
    pub fn owner(&self, owner: &StableCheckOwnerKey) -> Option<&OwnerCallableScopeOwnerResult> {
        self.owners
            .binary_search_by(|candidate| candidate.owner.cmp(owner))
            .ok()
            .and_then(|index| self.owners.get(index))
    }

    pub const fn fingerprint_v1(&self) -> [u8; 32] {
        self.fingerprint_v1
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OwnerCallableScopeSccOwnerBasis {
    pub owner: StableCheckOwnerKey,
    pub seed_fingerprint_v1: [u8; 32],
    pub callable_resolution_fingerprint_v1: [u8; 32],
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OwnerCallableScopeSccDependencyBasis {
    pub key: OwnerCallableScopeSccKey,
    pub result_fingerprint_v1: [u8; 32],
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OwnerCallableScopeSccBasis {
    pub key: OwnerCallableScopeSccKey,
    pub topology_fingerprint_v1: [u8; 32],
    pub owners: Box<[OwnerCallableScopeSccOwnerBasis]>,
    pub dependency_results: Box<[OwnerCallableScopeSccDependencyBasis]>,
    pub callable_abi_fingerprint_v1: [u8; 32],
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OwnerCallableScopeSccCurrentnessReceipt {
    basis: OwnerCallableScopeSccBasis,
    result_fingerprint_v1: [u8; 32],
    fingerprint_v1: [u8; 32],
}

impl OwnerCallableScopeSccCurrentnessReceipt {
    pub const fn basis(&self) -> &OwnerCallableScopeSccBasis {
        &self.basis
    }

    pub const fn fingerprint_v1(&self) -> [u8; 32] {
        self.fingerprint_v1
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OwnerCallableScopeSccEvaluation {
    pub currentness: OwnerCallableScopeSccCurrentnessReceipt,
    pub result: Arc<OwnerCallableScopeSccResult>,
}

impl OwnerSignatureLexicalPlanError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for OwnerSignatureLexicalPlanError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for OwnerSignatureLexicalPlanError {}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OwnerSignatureDeclarationTarget {
    FreshOut {
        call: StableExpressionKey,
        formal_ordinal: u32,
    },
    CallContext {
        call: StableExpressionKey,
        context_ordinal: u32,
    },
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OwnerEffectiveLexicalTarget {
    Static {
        target: OwnerLexicalDeclarationTarget,
    },
    FreshOut {
        call: StableExpressionKey,
        formal_ordinal: u32,
    },
    CallContext {
        call: StableExpressionKey,
        context_ordinal: u32,
    },
    Imported {
        target: OwnerLexicalTargetRef,
    },
    InvalidBareBinding {
        call: StableExpressionKey,
        entry_ordinal: u32,
        name: String,
    },
    Ambiguous {
        name: String,
    },
}

impl From<&OwnerSignatureDeclarationTarget> for OwnerEffectiveLexicalTarget {
    fn from(target: &OwnerSignatureDeclarationTarget) -> Self {
        match target {
            OwnerSignatureDeclarationTarget::FreshOut {
                call,
                formal_ordinal,
            } => Self::FreshOut {
                call: call.clone(),
                formal_ordinal: *formal_ordinal,
            },
            OwnerSignatureDeclarationTarget::CallContext {
                call,
                context_ordinal,
            } => Self::CallContext {
                call: call.clone(),
                context_ordinal: *context_ordinal,
            },
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OwnerEffectiveLexicalReadPlan {
    pub target: OwnerEffectiveLexicalTarget,
    pub declaration_scope: Option<u32>,
    pub projection: Box<[String]>,
    pub access: OwnerLexicalAccess,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OwnerSignatureDeclarationKind {
    FreshOut {
        formal_ordinal: u32,
    },
    CallContext {
        context_ordinal: u32,
        context_kind: CheckedCallContextKind,
        provider_parameter_ordinal: u32,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OwnerSignatureDeclarationPlan {
    pub target: OwnerSignatureDeclarationTarget,
    pub name: String,
    pub call_expression: u32,
    pub boundary_scope: u32,
    pub parent_evaluation_scope: Option<OwnerEffectiveLexicalTarget>,
    pub declaration_kind: OwnerSignatureDeclarationKind,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OwnerSignatureEvaluationScopePlan {
    pub target: OwnerEffectiveLexicalTarget,
    pub boundary_scope: u32,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OwnerSignatureCallTarget {
    Owner {
        owner: StableCheckOwnerKey,
    },
    Authoritative,
    Unresolved,
    Ambiguous {
        candidates: Box<[StableCheckOwnerKey]>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OwnerSignatureMatchedInputPlan {
    pub formal_ordinal: u32,
    pub formal_name: String,
    pub formal_kind: OwnerParameterKind,
    pub expression: u32,
    pub argument_kind: OwnerArgumentKind,
    pub from_pipe: bool,
    pub source: OwnerSignatureMatchedInputSource,
    pub evaluation_scope: OwnerInterfaceEvaluationScope,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OwnerSignatureMatchedInputSource {
    PipeInput,
    CallArgument { ordinal: u32 },
    PipeArgument { ordinal: u32 },
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OwnerSignaturePassSource {
    Call,
    Pipe,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct OwnerSignaturePassPlan {
    pub expression: u32,
    pub source: OwnerSignaturePassSource,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OwnerSignatureCallLexicalError {
    PipeWithoutValueInput,
    UnexpectedCallEntry {
        name: String,
        source: OwnerSignatureMatchedInputSource,
    },
    MisorderedCallEntry {
        position: u32,
        expected_name: String,
        actual_name: String,
        source: OwnerSignatureMatchedInputSource,
    },
    MissingCallEntry {
        name: String,
    },
    BareOrdinaryInput {
        name: String,
        source: OwnerSignatureMatchedInputSource,
    },
    PassOnAuthoritativeCallable {
        source: OwnerSignaturePassSource,
        callable_kind: CheckedCallableKind,
    },
    InvalidForwardOutTarget {
        formal_ordinal: u32,
        formal_name: String,
        expression: u32,
    },
    MissingEnclosingOut {
        formal_ordinal: u32,
        formal_name: String,
        expression: u32,
        target_name: String,
    },
    DuplicateCallContext {
        name: String,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OwnerCallableLexicalParameter {
    pub name: String,
    pub kind: OwnerParameterKind,
    pub ordinal: u32,
    pub requirement: CheckedParameterRequirement,
    pub evaluation_scope: OwnerInterfaceEvaluationScope,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OwnerCallableLexicalSignature {
    pub owner: StableCheckOwnerKey,
    pub parameters: Box<[OwnerCallableLexicalParameter]>,
}

impl OwnerCallableLexicalSignature {
    pub fn from_interface(interface: &OwnerPublicInterface) -> Self {
        Self {
            owner: interface.owner.clone(),
            parameters: interface
                .parameters
                .iter()
                .map(|parameter| OwnerCallableLexicalParameter {
                    name: parameter.name.clone(),
                    kind: parameter.kind,
                    ordinal: parameter.ordinal,
                    requirement: parameter.requirement.clone(),
                    evaluation_scope: parameter.evaluation_scope,
                })
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OwnerSignatureOutputBindingPlan {
    Fresh {
        formal_ordinal: u32,
        name: String,
        expression: u32,
        target: OwnerSignatureDeclarationTarget,
    },
    Forward {
        formal_ordinal: u32,
        name: String,
        expression: u32,
        target: OwnerEffectiveLexicalTarget,
    },
}

impl OwnerSignatureOutputBindingPlan {
    pub const fn formal_ordinal(&self) -> u32 {
        match self {
            Self::Fresh { formal_ordinal, .. } | Self::Forward { formal_ordinal, .. } => {
                *formal_ordinal
            }
        }
    }

    pub fn name(&self) -> &str {
        match self {
            Self::Fresh { name, .. } | Self::Forward { name, .. } => name,
        }
    }

    pub fn effective_target(&self) -> OwnerEffectiveLexicalTarget {
        match self {
            Self::Fresh { target, .. } => target.into(),
            Self::Forward { target, .. } => target.clone(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OwnerSignatureCallContextPlan {
    pub context_ordinal: u32,
    pub name: String,
    pub kind: CheckedCallContextKind,
    pub provider_parameter_ordinal: u32,
    pub target: OwnerSignatureDeclarationTarget,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OwnerSignatureCallPlan {
    pub expression: u32,
    pub stable_expression: StableExpressionKey,
    pub structural_ordinal: u32,
    pub function: String,
    pub target: OwnerSignatureCallTarget,
    pub valid: bool,
    pub matched_inputs: Box<[OwnerSignatureMatchedInputPlan]>,
    pub outputs: Box<[OwnerSignatureOutputBindingPlan]>,
    pub contexts: Box<[OwnerSignatureCallContextPlan]>,
    pub explicit_pass: Option<OwnerSignaturePassPlan>,
    pub lexical_errors: Box<[OwnerSignatureCallLexicalError]>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OwnerSignatureLexicalEnvironment {
    pub provider: StableCheckOwnerKey,
    pub consumer: StableCheckOwnerKey,
    pub observation_expression: StableExpressionKey,
    pub boundary_scope: Option<OwnerStableScopeRef>,
    pub bindings: OwnerLexicalBoundaryBindings,
    pub evaluation_scope: Option<OwnerLexicalTargetRef>,
    fingerprint_v1: [u8; 32],
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OwnerImportedLexicalCaptureSites {
    pub target: OwnerLexicalTargetRef,
    pub sites: Box<[StableExpressionKey]>,
}

impl OwnerSignatureLexicalEnvironment {
    pub const fn fingerprint_v1(&self) -> [u8; 32] {
        self.fingerprint_v1
    }
}

/// Signature-backed lexical authority for one owner.
///
/// The table is exhaustive for value reads: a missing row is truly external,
/// while static, dynamic, invalid, and ambiguous rows are all explicit. This
/// prevents inference and checked construction from independently merging a
/// base scope with FreshOut or compiler-context spelling rules.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OwnerSignatureLexicalPlan {
    owner: StableCheckOwnerKey,
    seed_fingerprint_v1: [u8; 32],
    signature_inputs_fingerprint_v1: [u8; 32],
    base_reads_fingerprint_v1: [u8; 32],
    signature_regions_fingerprint_v1: [u8; 32],
    reads: Arc<[Option<OwnerEffectiveLexicalReadPlan>]>,
    expression_evaluation_scopes: Arc<[Option<OwnerSignatureEvaluationScopePlan>]>,
    declarations: Arc<[OwnerSignatureDeclarationPlan]>,
    calls: Arc<[OwnerSignatureCallPlan]>,
    call_indices: Arc<[Option<usize>]>,
    inherited_environment: Option<Arc<OwnerSignatureLexicalEnvironment>>,
    child_environments: Arc<[OwnerSignatureLexicalEnvironment]>,
    imported_captures: Arc<[OwnerLexicalTargetRef]>,
    imported_capture_sites: Arc<[OwnerImportedLexicalCaptureSites]>,
    external_candidates: Arc<[OwnerSymbolReference]>,
    reads_fingerprint_v1: [u8; 32],
    fingerprint_v1: [u8; 32],
}

/// Dependency-first callable scope signatures and exact dynamic lexical plans
/// for one closed owner set.
///
/// This projection is type-free. It deliberately runs before ordinary value
/// resolution so a spelling owned by FreshOut or CallContext cannot first
/// acquire a false project-value dependency. Recursive call components remain
/// legal only while every parameter is evaluated in its parent scope; the
/// language plan reserves recursive contextual scope effects for a future
/// finite joint fixed point.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OwnerSignatureLexicalScopeProjection {
    signatures: BTreeMap<StableCheckOwnerKey, OwnerCallableLexicalSignature>,
    plans: BTreeMap<StableCheckOwnerKey, OwnerSignatureLexicalPlan>,
}

impl OwnerSignatureLexicalScopeProjection {
    pub fn signature(&self, owner: &StableCheckOwnerKey) -> Option<&OwnerCallableLexicalSignature> {
        self.signatures.get(owner)
    }

    pub fn plan(&self, owner: &StableCheckOwnerKey) -> Option<&OwnerSignatureLexicalPlan> {
        self.plans.get(owner)
    }

    pub fn signatures(&self) -> &BTreeMap<StableCheckOwnerKey, OwnerCallableLexicalSignature> {
        &self.signatures
    }

    pub fn plans(&self) -> &BTreeMap<StableCheckOwnerKey, OwnerSignatureLexicalPlan> {
        &self.plans
    }
}

impl OwnerSignatureLexicalPlan {
    pub fn owner(&self) -> &StableCheckOwnerKey {
        &self.owner
    }

    pub fn reads(&self) -> &[Option<OwnerEffectiveLexicalReadPlan>] {
        &self.reads
    }

    pub fn declarations(&self) -> &[OwnerSignatureDeclarationPlan] {
        &self.declarations
    }

    pub fn expression_evaluation_scopes(&self) -> &[Option<OwnerSignatureEvaluationScopePlan>] {
        &self.expression_evaluation_scopes
    }

    pub fn calls(&self) -> &[OwnerSignatureCallPlan] {
        &self.calls
    }

    pub fn call(&self, expression: usize) -> Option<&OwnerSignatureCallPlan> {
        self.call_indices
            .get(expression)
            .copied()
            .flatten()
            .and_then(|index| self.calls.get(index))
    }

    pub fn external_candidates(&self) -> &[OwnerSymbolReference] {
        &self.external_candidates
    }

    pub fn inherited_environment(&self) -> Option<&OwnerSignatureLexicalEnvironment> {
        self.inherited_environment.as_deref()
    }

    pub fn child_environments(&self) -> &[OwnerSignatureLexicalEnvironment] {
        &self.child_environments
    }

    pub fn imported_captures(&self) -> &[OwnerLexicalTargetRef] {
        &self.imported_captures
    }

    pub fn imported_capture_sites(&self) -> &[OwnerImportedLexicalCaptureSites] {
        &self.imported_capture_sites
    }

    pub(crate) fn is_external_candidate(&self, reference: &OwnerSymbolReference) -> bool {
        self.external_candidates.binary_search(reference).is_ok()
    }

    pub const fn reads_fingerprint_v1(&self) -> [u8; 32] {
        self.reads_fingerprint_v1
    }

    pub const fn fingerprint_v1(&self) -> [u8; 32] {
        self.fingerprint_v1
    }

    pub(crate) fn matches_base(&self, plan: &OwnerLexicalPlan) -> bool {
        self.owner == *plan.owner()
            && self.base_reads_fingerprint_v1 == plan.reads_fingerprint_v1()
            && self.signature_regions_fingerprint_v1 == plan.signature_regions().fingerprint_v1()
    }

    pub(crate) fn matches_seed(&self, seed: &OwnerConstraintSeed) -> bool {
        self.owner == seed.owner
            && self.seed_fingerprint_v1 == seed.fingerprint_v1()
            && self.base_reads_fingerprint_v1 == seed.lexical_reads_fingerprint_v1()
            && self.signature_regions_fingerprint_v1 == seed.signature_regions().fingerprint_v1()
    }

    pub(crate) fn matches_signature_inputs(
        &self,
        seed: &OwnerConstraintSeed,
        summary: &OwnerConstraintSummary,
        abi: &OwnerInferenceAbiEnvironment,
        mut signature: impl FnMut(&StableCheckOwnerKey) -> Option<OwnerCallableLexicalSignature>,
    ) -> Result<bool, OwnerSignatureLexicalPlanError> {
        if !self.matches_seed(seed) {
            return Ok(false);
        }
        let callable_resolutions = project_owner_callable_resolution_plan(
            seed,
            summary
                .symbol_resolutions
                .iter()
                .filter(|resolution| resolution.reference().kind == OwnerReferenceKind::Callable)
                .cloned(),
        )?;
        let required_owners = callable_resolutions
            .resolutions()
            .iter()
            .filter_map(|resolution| match resolution {
                OwnerSymbolResolution::Resolved { owner, .. } => Some(owner.clone()),
                _ => None,
            })
            .collect::<BTreeSet<_>>();
        let mut by_owner = BTreeMap::new();
        for owner in required_owners {
            let Some(signature) = signature(&owner) else {
                continue;
            };
            if signature.owner != owner {
                return Err(OwnerSignatureLexicalPlanError::new(format!(
                    "owner signature input validation requested {owner:?} but received {:?}",
                    signature.owner
                )));
            }
            by_owner.insert(owner, signature);
        }
        let current = signature_inputs_fingerprint_v1(
            seed,
            &callable_resolutions,
            abi,
            &by_owner,
            self.inherited_environment.as_deref(),
        )?;
        Ok(self.signature_inputs_fingerprint_v1 == current)
    }
}

pub(crate) fn effective_narrowed_selector_read_matches(
    seed: &OwnerConstraintSeed,
    plan: &OwnerSignatureLexicalPlan,
    selector: u32,
    projection: &[String],
    candidate: u32,
) -> bool {
    let selector_read = plan.reads().get(selector as usize).and_then(Option::as_ref);
    let candidate_read = plan
        .reads()
        .get(candidate as usize)
        .and_then(Option::as_ref);
    match (selector_read, candidate_read) {
        (Some(selector_read), Some(candidate_read)) => {
            let target_is_narrowable = |target: &OwnerEffectiveLexicalTarget| {
                matches!(
                    target,
                    OwnerEffectiveLexicalTarget::Static { target }
                        if !matches!(target, OwnerLexicalDeclarationTarget::Ambiguous { .. })
                ) || matches!(
                    target,
                    OwnerEffectiveLexicalTarget::FreshOut { .. }
                        | OwnerEffectiveLexicalTarget::CallContext { .. }
                        | OwnerEffectiveLexicalTarget::Imported {
                            target: OwnerLexicalTargetRef::Declaration {
                                capability: OwnerLexicalDeclarationCapability::Value
                                    | OwnerLexicalDeclarationCapability::Out { .. },
                                ..
                            } | OwnerLexicalTargetRef::ContextFormal { .. },
                        }
                )
            };
            selector_read.access == OwnerLexicalAccess::Read
                && candidate_read.access == OwnerLexicalAccess::Read
                && target_is_narrowable(&selector_read.target)
                && selector_read.target == candidate_read.target
                && candidate_read
                    .projection
                    .starts_with(&selector_read.projection)
                && candidate_read.projection.len()
                    == selector_read.projection.len() + projection.len()
                && candidate_read.projection[selector_read.projection.len()..] == *projection
        }
        // Missing effective rows are exact surviving external candidates. The
        // seed edge already proves their spelling/projection relationship.
        (None, None) => [selector, candidate].into_iter().all(|expression| {
            matches!(
                seed.expressions
                    .get(expression as usize)
                    .map(|expression| &expression.kind),
                Some(OwnerConstraintNodeKind::Reference { .. })
            )
        }),
        _ => false,
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct SignatureParameter {
    name: String,
    kind: OwnerParameterKind,
    ordinal: u32,
    requirement: CheckedParameterRequirement,
    evaluation_scope: OwnerInterfaceEvaluationScope,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct SignatureContext {
    name: String,
    kind: CheckedCallContextKind,
    provider_parameter_ordinal: u32,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct CallSignature {
    target: OwnerSignatureCallTarget,
    parameters: Vec<SignatureParameter>,
    contexts: Vec<SignatureContext>,
    kind: Option<CheckedCallableKind>,
    authoritative: bool,
    available: bool,
}

#[derive(Clone)]
struct ActiveBinding {
    name: String,
    target: OwnerEffectiveLexicalTarget,
    boundary_scope: u32,
    owns_evaluation_scope: bool,
}

fn checked_u32(value: usize, context: &str) -> Result<u32, OwnerSignatureLexicalPlanError> {
    u32::try_from(value).map_err(|_| {
        OwnerSignatureLexicalPlanError::new(format!("{context} exceeds the owner-local u32 bound"))
    })
}

fn fingerprint<T: Serialize>(
    domain: &[u8],
    value: &T,
) -> Result<[u8; 32], OwnerSignatureLexicalPlanError> {
    boon_contract::canonical_serde_hash_v1(domain, value).map_err(|error| {
        OwnerSignatureLexicalPlanError::new(format!(
            "cannot fingerprint owner signature lexical plan: {error}"
        ))
    })
}

pub fn project_owner_callable_resolution_plan(
    seed: &OwnerConstraintSeed,
    resolutions: impl IntoIterator<Item = OwnerSymbolResolution>,
) -> Result<OwnerCallableResolutionPlan, OwnerSignatureLexicalPlanError> {
    let expected = seed
        .references
        .iter()
        .filter(|reference| reference.kind == OwnerReferenceKind::Callable)
        .cloned()
        .collect::<BTreeSet<_>>();
    let mut by_reference = BTreeMap::new();
    for resolution in resolutions {
        let reference = resolution.reference().clone();
        if reference.kind != OwnerReferenceKind::Callable || !expected.contains(&reference) {
            return Err(OwnerSignatureLexicalPlanError::new(format!(
                "owner callable resolution contains a non-callable or absent reference {reference:?}"
            )));
        }
        if by_reference.insert(reference.clone(), resolution).is_some() {
            return Err(OwnerSignatureLexicalPlanError::new(format!(
                "owner callable resolution repeats {reference:?}"
            )));
        }
    }
    if by_reference.keys().collect::<BTreeSet<_>>() != expected.iter().collect() {
        return Err(OwnerSignatureLexicalPlanError::new(format!(
            "owner callable resolution does not cover every callable reference in {:?}",
            seed.owner
        )));
    }
    let resolutions = by_reference.into_values().collect::<Vec<_>>();
    let fingerprint_v1 = fingerprint(
        OWNER_CALLABLE_RESOLUTION_PLAN_DOMAIN_V2,
        &(
            &seed.owner,
            &resolutions,
            seed.signature_regions().containment().fingerprint_v1(),
        ),
    )?;
    Ok(OwnerCallableResolutionPlan {
        owner: seed.owner.clone(),
        resolutions: resolutions.into_boxed_slice(),
        containment: Arc::clone(seed.signature_regions().containment()),
        fingerprint_v1,
    })
}

pub fn build_owner_callable_scope_topology<'a>(
    plans: impl IntoIterator<Item = &'a OwnerCallableResolutionPlan>,
) -> Result<OwnerCallableScopeTopology, OwnerSignatureLexicalPlanError> {
    let mut plans = plans.into_iter().collect::<Vec<_>>();
    plans.sort_by(|left, right| left.owner.cmp(&right.owner));
    if plans.is_empty() || plans.windows(2).any(|pair| pair[0].owner == pair[1].owner) {
        return Err(OwnerSignatureLexicalPlanError::new(
            "owner callable scope topology requires one unique non-empty owner set",
        ));
    }

    let plan_by_owner = plans
        .iter()
        .map(|plan| (plan.owner.clone(), *plan))
        .collect::<BTreeMap<_, _>>();
    let mut parent_by_child = BTreeMap::<StableCheckOwnerKey, StableCheckOwnerKey>::new();
    for plan in &plans {
        if plan.containment().owner != plan.owner {
            return Err(OwnerSignatureLexicalPlanError::new(
                "owner callable resolution carries foreign lexical containment",
            ));
        }
        for child in &plan.containment().children {
            if !child.inherits_lexical_environment() {
                continue;
            }
            let child_plan = plan_by_owner.get(&child.owner).ok_or_else(|| {
                OwnerSignatureLexicalPlanError::new(format!(
                    "owner lexical containment child {:?} is not registered",
                    child.owner
                ))
            })?;
            let parent_statement = child.parent_statement.as_ref().ok_or_else(|| {
                OwnerSignatureLexicalPlanError::new(
                    "inheriting owner lexical child has no parent statement",
                )
            })?;
            match &child_plan.containment().containing_scope {
                crate::OwnerContainingScopeInput::OwnerStatement { owner, statement }
                    if owner == &plan.owner && statement == parent_statement => {}
                _ => {
                    return Err(OwnerSignatureLexicalPlanError::new(format!(
                        "owner lexical containment child {:?} disagrees with its parent boundary",
                        child.owner
                    )));
                }
            }
            if parent_by_child
                .insert(child.owner.clone(), plan.owner.clone())
                .is_some()
            {
                return Err(OwnerSignatureLexicalPlanError::new(format!(
                    "owner lexical containment child {:?} has more than one parent",
                    child.owner
                )));
            }
        }
    }

    let mut members_by_root = BTreeMap::<StableCheckOwnerKey, Vec<StableCheckOwnerKey>>::new();
    for plan in &plans {
        let mut root = plan.owner.clone();
        let mut visited = BTreeSet::new();
        while let Some(parent) = parent_by_child.get(&root) {
            if !visited.insert(root.clone()) {
                return Err(OwnerSignatureLexicalPlanError::new(
                    "owner lexical containment graph contains a cycle",
                ));
            }
            root = parent.clone();
        }
        members_by_root
            .entry(root)
            .or_default()
            .push(plan.owner.clone());
    }
    let mut clusters = Vec::with_capacity(members_by_root.len());
    for (root, mut members) in members_by_root {
        members.sort();
        let member_fingerprints = members
            .iter()
            .map(|owner| {
                (
                    stable_check_owner_key_fingerprint_v1(owner),
                    plan_by_owner[owner].fingerprint_v1(),
                )
            })
            .collect::<Vec<_>>();
        let fingerprint_v1 = fingerprint(
            OWNER_LEXICAL_CONTAINMENT_CLUSTER_DOMAIN_V1,
            &(&root, &member_fingerprints),
        )?;
        clusters.push(OwnerLexicalContainmentCluster {
            root,
            members: members.into_boxed_slice(),
            fingerprint_v1,
        });
    }
    clusters.sort_by(|left, right| left.root.cmp(&right.root));

    let mut builder = DenseProjectionGraphBuilder::new();
    let mut projection_by_cluster = Vec::with_capacity(clusters.len());
    let mut cluster_by_projection = BTreeMap::<ProjectionId, usize>::new();
    let mut cluster_by_owner = BTreeMap::<StableCheckOwnerKey, usize>::new();
    for (cluster_index, cluster) in clusters.iter().enumerate() {
        let projection = builder
            .register(
                stable_check_owner_key_fingerprint_v1(&cluster.root),
                cluster.fingerprint_v1(),
            )
            .map_err(|error| {
                OwnerSignatureLexicalPlanError::new(format!(
                    "cannot register owner callable containment cluster: {error}"
                ))
            })?;
        projection_by_cluster.push(projection);
        cluster_by_projection.insert(projection, cluster_index);
        for member in &cluster.members {
            if cluster_by_owner
                .insert(member.clone(), cluster_index)
                .is_some()
            {
                return Err(OwnerSignatureLexicalPlanError::new(
                    "owner belongs to more than one lexical containment cluster",
                ));
            }
        }
    }

    let mut edges = BTreeSet::<(usize, usize)>::new();
    for plan in &plans {
        for resolution in plan.resolutions() {
            let OwnerSymbolResolution::Resolved {
                reference,
                owner: dependency,
                ..
            } = resolution
            else {
                continue;
            };
            if reference.kind != OwnerReferenceKind::Callable {
                continue;
            }
            let Some(dependency_cluster) = cluster_by_owner.get(dependency).copied() else {
                return Err(OwnerSignatureLexicalPlanError::new(format!(
                    "owner callable scope dependency {dependency:?} is not registered"
                )));
            };
            edges.insert((cluster_by_owner[&plan.owner], dependency_cluster));
        }
    }
    for (request, dependency) in &edges {
        builder
            .add_dependency(
                projection_by_cluster[*request],
                projection_by_cluster[*dependency],
            )
            .map_err(|error| {
                OwnerSignatureLexicalPlanError::new(format!(
                    "cannot add owner callable scope topology edge: {error}"
                ))
            })?;
    }
    let graph = builder
        .seal(ProjectionGraphDigestDomains {
            component: OWNER_CALLABLE_SCOPE_COMPONENT_DOMAIN_V2,
        })
        .map_err(|error| {
            OwnerSignatureLexicalPlanError::new(format!(
                "cannot seal owner callable scope topology: {error}"
            ))
        })?;
    let mut component_keys = Vec::with_capacity(graph.component_count());
    let mut component_clusters = Vec::with_capacity(graph.component_count());
    let mut component_by_owner = BTreeMap::new();
    for component in 0..graph.component_count() {
        let mut cluster_indices = graph
            .component_members_by_ordinal(component)
            .ok_or_else(|| {
                OwnerSignatureLexicalPlanError::new(
                    "owner callable scope topology has a missing component",
                )
            })?
            .map(|projection| {
                cluster_by_projection
                    .get(&projection)
                    .copied()
                    .ok_or_else(|| {
                        OwnerSignatureLexicalPlanError::new(
                            "owner callable scope topology contains an unknown projection",
                        )
                    })
            })
            .collect::<Result<Vec<_>, _>>()?;
        cluster_indices.sort();
        let mut members = cluster_indices
            .iter()
            .flat_map(|index| clusters[*index].members.iter().cloned())
            .collect::<Vec<_>>();
        members.sort();
        if members.is_empty() {
            return Err(OwnerSignatureLexicalPlanError::new(
                "owner callable scope topology produced an empty component",
            ));
        }
        for member in &members {
            component_by_owner.insert(member.clone(), component);
        }
        component_keys.push(OwnerCallableScopeSccKey {
            members: members.into_boxed_slice(),
        });
        component_clusters.push(cluster_indices);
    }

    let mut dependencies_by_component = std::iter::repeat_with(BTreeSet::new)
        .take(component_keys.len())
        .collect::<Vec<_>>();
    for (request, dependency) in &edges {
        let request_component = component_by_owner[&clusters[*request].root];
        let dependency_component = component_by_owner[&clusters[*dependency].root];
        if request_component != dependency_component {
            dependencies_by_component[request_component].insert(dependency_component);
        }
    }
    let mut sccs = Vec::with_capacity(component_keys.len());
    for (component, key) in component_keys.iter().cloned().enumerate() {
        let dependencies = std::mem::take(&mut dependencies_by_component[component]);
        if dependencies
            .iter()
            .any(|dependency| *dependency >= component)
        {
            return Err(OwnerSignatureLexicalPlanError::new(
                "owner callable scope components are not dependency-first",
            ));
        }
        let dependencies = dependencies
            .into_iter()
            .map(|dependency| component_keys[dependency].clone())
            .collect::<Vec<_>>();
        let member_fingerprints = key
            .members
            .iter()
            .map(|owner| {
                let plan = plans
                    .binary_search_by(|plan| plan.owner.cmp(owner))
                    .ok()
                    .map(|index| plans[index])
                    .expect("callable scope component member has a plan");
                (
                    stable_check_owner_key_fingerprint_v1(owner),
                    plan.fingerprint_v1(),
                )
            })
            .collect::<Vec<_>>();
        let containment_clusters = component_clusters[component]
            .iter()
            .map(|cluster| clusters[*cluster].clone())
            .collect::<Vec<_>>();
        let call_cyclic = containment_clusters.len() > 1
            || component_clusters[component]
                .iter()
                .any(|cluster| edges.contains(&(*cluster, *cluster)));
        let containment_fingerprints = containment_clusters
            .iter()
            .map(OwnerLexicalContainmentCluster::fingerprint_v1)
            .collect::<Vec<_>>();
        let fingerprint_v1 = fingerprint(
            OWNER_CALLABLE_SCOPE_SCC_DOMAIN_V2,
            &(
                &key,
                &dependencies,
                &member_fingerprints,
                &containment_fingerprints,
                call_cyclic,
            ),
        )?;
        sccs.push(OwnerCallableScopeScc {
            key,
            containment_clusters: containment_clusters.into_boxed_slice(),
            call_cyclic,
            dependencies: dependencies.into_boxed_slice(),
            fingerprint_v1,
        });
    }
    let stats = graph.stats();
    let stat_values = (
        stats.nodes,
        stats.edges,
        stats.components,
        stats.cyclic_components,
        stats.maximum_component_nodes,
        stats.component_edges,
    );
    let scc_fingerprints = sccs
        .iter()
        .map(OwnerCallableScopeScc::fingerprint_v1)
        .collect::<Vec<_>>();
    let fingerprint_v1 = fingerprint(
        OWNER_CALLABLE_SCOPE_TOPOLOGY_DOMAIN_V2,
        &(&stat_values, &scc_fingerprints),
    )?;
    Ok(OwnerCallableScopeTopology {
        sccs: sccs.into_boxed_slice(),
        stats,
        scc_by_owner: component_by_owner,
        fingerprint_v1,
    })
}

pub fn evaluate_owner_callable_scope_scc<'a>(
    scc: &OwnerCallableScopeScc,
    seeds: impl IntoIterator<Item = &'a OwnerConstraintSeed>,
    callable_resolutions: impl IntoIterator<Item = &'a OwnerCallableResolutionPlan>,
    abi: &OwnerInferenceAbiEnvironment,
    dependency_results: impl IntoIterator<Item = &'a OwnerCallableScopeSccResult>,
) -> Result<OwnerCallableScopeSccEvaluation, OwnerSignatureLexicalPlanError> {
    let seeds = seeds
        .into_iter()
        .map(|seed| (seed.owner.clone(), seed))
        .collect::<BTreeMap<_, _>>();
    let callable_resolutions = callable_resolutions
        .into_iter()
        .map(|plan| (plan.owner().clone(), plan))
        .collect::<BTreeMap<_, _>>();
    let expected = scc.key.members.iter().cloned().collect::<BTreeSet<_>>();
    if seeds.keys().cloned().collect::<BTreeSet<_>>() != expected
        || callable_resolutions
            .keys()
            .cloned()
            .collect::<BTreeSet<_>>()
            != expected
        || abi.subjects().iter().cloned().collect::<BTreeSet<_>>() != expected
    {
        return Err(OwnerSignatureLexicalPlanError::new(
            "owner callable scope SCC inputs do not match its exact member set",
        ));
    }
    let expected_abi_names = callable_resolutions
        .values()
        .flat_map(|plan| plan.authoritative_abi_names().into_vec())
        .collect::<BTreeSet<_>>();
    let actual_abi_names = abi
        .lookups()
        .iter()
        .map(|lookup| lookup.canonical_name().to_owned())
        .collect::<BTreeSet<_>>();
    if expected_abi_names != actual_abi_names
        || !abi.value_lookups().is_empty()
        || !abi.source_payload_lookups().is_empty()
        || !abi.parameter_requirement_lookups().is_empty()
    {
        return Err(OwnerSignatureLexicalPlanError::new(
            "owner callable scope SCC ABI is not the exact callable-only surface",
        ));
    }

    let dependency_results = dependency_results.into_iter().collect::<Vec<_>>();
    let dependency_keys = dependency_results
        .iter()
        .map(|result| result.key.clone())
        .collect::<BTreeSet<_>>();
    if dependency_keys != scc.dependencies.iter().cloned().collect() {
        return Err(OwnerSignatureLexicalPlanError::new(
            "owner callable scope SCC dependency results do not match its topology",
        ));
    }
    let mut dependency_signatures = Vec::new();
    let mut dependency_owners = BTreeSet::new();
    for result in &dependency_results {
        for owner in &result.owners {
            if !dependency_owners.insert(owner.owner.clone()) {
                return Err(OwnerSignatureLexicalPlanError::new(
                    "owner callable scope SCC dependencies repeat an owner",
                ));
            }
            dependency_signatures.push(owner.signature.clone());
        }
    }
    let projection = project_owner_signature_lexical_scope_plans_with_callable_resolutions(
        seeds.values().copied(),
        callable_resolutions.values().copied(),
        abi,
        dependency_signatures,
    )?;
    let owners = scc
        .key
        .members
        .iter()
        .map(|owner| {
            Ok(OwnerCallableScopeOwnerResult {
                owner: owner.clone(),
                signature: projection.signature(owner).cloned().ok_or_else(|| {
                    OwnerSignatureLexicalPlanError::new(
                        "owner callable scope result omits a member signature",
                    )
                })?,
                lexical_plan: projection.plan(owner).cloned().ok_or_else(|| {
                    OwnerSignatureLexicalPlanError::new(
                        "owner callable scope result omits a member lexical plan",
                    )
                })?,
            })
        })
        .collect::<Result<Vec<_>, OwnerSignatureLexicalPlanError>>()?;
    let semantic_fingerprints = owners
        .iter()
        .map(|owner| {
            (
                &owner.owner,
                &owner.signature,
                owner.lexical_plan.fingerprint_v1(),
            )
        })
        .collect::<Vec<_>>();
    let fingerprint_v1 = fingerprint(
        OWNER_CALLABLE_SCOPE_RESULT_DOMAIN_V2,
        &(&scc.key, &semantic_fingerprints),
    )?;
    let result = Arc::new(OwnerCallableScopeSccResult {
        key: scc.key.clone(),
        owners: owners.into_boxed_slice(),
        fingerprint_v1,
    });
    let mut dependency_basis = dependency_results
        .iter()
        .map(|result| OwnerCallableScopeSccDependencyBasis {
            key: result.key.clone(),
            result_fingerprint_v1: result.fingerprint_v1(),
        })
        .collect::<Vec<_>>();
    dependency_basis.sort_by(|left, right| left.key.cmp(&right.key));
    let basis = OwnerCallableScopeSccBasis {
        key: scc.key.clone(),
        topology_fingerprint_v1: scc.fingerprint_v1(),
        owners: scc
            .key
            .members
            .iter()
            .map(|owner| OwnerCallableScopeSccOwnerBasis {
                owner: owner.clone(),
                seed_fingerprint_v1: seeds[owner].fingerprint_v1(),
                callable_resolution_fingerprint_v1: callable_resolutions[owner].fingerprint_v1(),
            })
            .collect::<Vec<_>>()
            .into_boxed_slice(),
        dependency_results: dependency_basis.into_boxed_slice(),
        callable_abi_fingerprint_v1: abi.fingerprint_v1(),
    };
    let result_fingerprint_v1 = result.fingerprint_v1();
    let currentness_fingerprint_v1 = fingerprint(
        OWNER_CALLABLE_SCOPE_CURRENTNESS_DOMAIN_V2,
        &(&basis, result_fingerprint_v1),
    )?;
    Ok(OwnerCallableScopeSccEvaluation {
        currentness: OwnerCallableScopeSccCurrentnessReceipt {
            basis,
            result_fingerprint_v1,
            fingerprint_v1: currentness_fingerprint_v1,
        },
        result,
    })
}

fn owner_kind(kind: CheckedParameterKind) -> OwnerParameterKind {
    match kind {
        CheckedParameterKind::Value => OwnerParameterKind::Value,
        CheckedParameterKind::Out => OwnerParameterKind::Out,
    }
}

fn owner_scope(scope: OwnerAbiEvaluationScope) -> OwnerInterfaceEvaluationScope {
    match scope {
        OwnerAbiEvaluationScope::Parent => OwnerInterfaceEvaluationScope::Parent,
        OwnerAbiEvaluationScope::Output { parameter_ordinal } => {
            OwnerInterfaceEvaluationScope::Output { parameter_ordinal }
        }
    }
}

fn expression_read_parts(expression: &OwnerConstraintNodeKind) -> Option<(String, Box<[String]>)> {
    let parts = match expression {
        OwnerConstraintNodeKind::Reference { parts } | OwnerConstraintNodeKind::Drain { parts } => {
            parts
        }
        _ => return None,
    };
    let (root, projection) = parts.split_first()?;
    Some((root.clone(), projection.to_vec().into_boxed_slice()))
}

fn expression_binding_parts(
    expression: &OwnerConstraintNodeKind,
) -> Option<(String, Box<[String]>)> {
    let OwnerConstraintNodeKind::Reference { parts } = expression else {
        return None;
    };
    let (root, projection) = parts.split_first()?;
    Some((root.clone(), projection.to_vec().into_boxed_slice()))
}

fn access(expression: &OwnerConstraintNodeKind) -> OwnerLexicalAccess {
    if matches!(expression, OwnerConstraintNodeKind::Drain { .. }) {
        OwnerLexicalAccess::Drain
    } else {
        OwnerLexicalAccess::Read
    }
}

fn is_signature_lexical_structural_edge(role: &OwnerConstraintEdgeRole) -> bool {
    !matches!(
        role,
        OwnerConstraintEdgeRole::MatchSelector
            | OwnerConstraintEdgeRole::MatchBinding { .. }
            | OwnerConstraintEdgeRole::MatchNarrowedSelector { .. }
    )
}

fn signature_for_call(
    seed: &OwnerConstraintSeed,
    abi: &OwnerInferenceAbiEnvironment,
    interfaces: &BTreeMap<StableCheckOwnerKey, OwnerCallableLexicalSignature>,
    resolution_by_expression: &BTreeMap<StableExpressionKey, &OwnerSymbolResolution>,
    expression: u32,
    function: &str,
) -> Option<CallSignature> {
    let stable = &seed.expressions[expression as usize].expression;
    match resolution_by_expression.get(stable).copied() {
        Some(OwnerSymbolResolution::Resolved { owner, .. }) => {
            let Some(interface) = interfaces.get(owner) else {
                return Some(CallSignature {
                    target: OwnerSignatureCallTarget::Owner {
                        owner: owner.clone(),
                    },
                    parameters: Vec::new(),
                    contexts: Vec::new(),
                    kind: Some(CheckedCallableKind::User),
                    authoritative: false,
                    available: false,
                });
            };
            Some(CallSignature {
                target: OwnerSignatureCallTarget::Owner {
                    owner: owner.clone(),
                },
                parameters: interface
                    .parameters
                    .iter()
                    .map(|parameter| SignatureParameter {
                        name: parameter.name.clone(),
                        kind: parameter.kind,
                        ordinal: parameter.ordinal,
                        requirement: parameter.requirement.clone(),
                        evaluation_scope: parameter.evaluation_scope,
                    })
                    .collect(),
                contexts: Vec::new(),
                kind: Some(CheckedCallableKind::User),
                authoritative: false,
                available: true,
            })
        }
        Some(OwnerSymbolResolution::Authoritative { .. }) => {
            let Some(signature) = abi.callable(function) else {
                return Some(CallSignature {
                    target: OwnerSignatureCallTarget::Authoritative,
                    parameters: Vec::new(),
                    contexts: Vec::new(),
                    kind: None,
                    authoritative: true,
                    available: false,
                });
            };
            Some(CallSignature {
                target: OwnerSignatureCallTarget::Authoritative,
                parameters: signature
                    .parameters
                    .iter()
                    .map(|parameter| SignatureParameter {
                        name: parameter.name.clone(),
                        kind: owner_kind(parameter.kind),
                        ordinal: parameter.ordinal,
                        requirement: parameter.requirement.clone(),
                        evaluation_scope: owner_scope(parameter.evaluation_scope),
                    })
                    .collect(),
                contexts: signature
                    .contexts
                    .iter()
                    .map(|context| SignatureContext {
                        name: context.name.clone(),
                        kind: context.kind,
                        provider_parameter_ordinal: context.provider_parameter_ordinal,
                    })
                    .collect(),
                kind: Some(signature.kind),
                authoritative: true,
                available: true,
            })
        }
        Some(OwnerSymbolResolution::Ambiguous { candidates, .. }) => Some(CallSignature {
            target: OwnerSignatureCallTarget::Ambiguous {
                candidates: candidates
                    .iter()
                    .map(|candidate| candidate.owner.clone())
                    .collect::<Vec<_>>()
                    .into_boxed_slice(),
            },
            parameters: Vec::new(),
            contexts: Vec::new(),
            kind: None,
            authoritative: false,
            available: false,
        }),
        Some(
            OwnerSymbolResolution::Unresolved { .. }
            | OwnerSymbolResolution::CallableAsValue { .. },
        )
        | None => Some(CallSignature {
            target: OwnerSignatureCallTarget::Unresolved,
            parameters: Vec::new(),
            contexts: Vec::new(),
            kind: None,
            authoritative: false,
            available: false,
        }),
    }
}

fn signature_inputs_fingerprint_v1(
    seed: &OwnerConstraintSeed,
    callable_resolutions: &OwnerCallableResolutionPlan,
    abi: &OwnerInferenceAbiEnvironment,
    interfaces: &BTreeMap<StableCheckOwnerKey, OwnerCallableLexicalSignature>,
    inherited_environment: Option<&OwnerSignatureLexicalEnvironment>,
) -> Result<[u8; 32], OwnerSignatureLexicalPlanError> {
    if !callable_resolutions.matches_seed(seed) {
        return Err(OwnerSignatureLexicalPlanError::new(
            "owner signature lexical inputs do not share one current callable plan",
        ));
    }
    if let Some(environment) = inherited_environment {
        let containing_owner = match &seed.signature_regions().containment().containing_scope {
            crate::OwnerContainingScopeInput::OwnerStatement { owner, .. } => Some(owner),
            crate::OwnerContainingScopeInput::ProjectRoot => None,
        };
        if environment.consumer != seed.owner || containing_owner != Some(&environment.provider) {
            return Err(OwnerSignatureLexicalPlanError::new(
                "owner signature inherited environment does not match its child boundary",
            ));
        }
        let unique_names = environment
            .bindings
            .iter()
            .map(|binding| binding.name.as_str())
            .collect::<BTreeSet<_>>();
        if unique_names.len() != environment.bindings.len() {
            return Err(OwnerSignatureLexicalPlanError::new(
                "owner signature inherited environment repeats a binding name",
            ));
        }
    }
    let resolution_by_expression = callable_resolutions
        .resolutions()
        .iter()
        .map(|resolution| (resolution.reference().expression.clone(), resolution))
        .collect::<BTreeMap<_, _>>();
    let calls = seed
        .expressions
        .iter()
        .enumerate()
        .filter_map(|(expression, node)| {
            let function = match &node.kind {
                OwnerConstraintNodeKind::Call { function }
                | OwnerConstraintNodeKind::Pipe {
                    operation: function,
                } => function,
                _ => return None,
            };
            Some((
                node.expression.clone(),
                function.clone(),
                signature_for_call(
                    seed,
                    abi,
                    interfaces,
                    &resolution_by_expression,
                    expression as u32,
                    function,
                ),
            ))
        })
        .collect::<Vec<_>>();
    fingerprint(
        OWNER_SIGNATURE_LEXICAL_INPUTS_DOMAIN_V2,
        &(
            seed.fingerprint_v1(),
            inherited_environment.map(OwnerSignatureLexicalEnvironment::fingerprint_v1),
            &calls,
        ),
    )
}

struct Planner<'a> {
    seed: &'a OwnerConstraintSeed,
    abi: &'a OwnerInferenceAbiEnvironment,
    interfaces: &'a BTreeMap<StableCheckOwnerKey, OwnerCallableLexicalSignature>,
    resolution_by_expression: BTreeMap<StableExpressionKey, &'a OwnerSymbolResolution>,
    inherited_environment: Option<OwnerSignatureLexicalEnvironment>,
    inherited_bindings: OwnerLexicalBoundaryBindings,
    child_binding_cache: BTreeMap<ChildBindingCacheKey, OwnerLexicalBoundaryBindings>,
    child_boundary_by_result:
        BTreeMap<StableExpressionKey, &'a crate::OwnerLexicalChildBoundaryPlan>,
    child_environments: BTreeMap<StableCheckOwnerKey, OwnerSignatureLexicalEnvironment>,
    reads: Vec<Option<OwnerEffectiveLexicalReadPlan>>,
    expression_evaluation_scopes: Vec<Option<OwnerSignatureEvaluationScopePlan>>,
    expression_evaluation_scope_seen: Vec<bool>,
    declarations: BTreeMap<OwnerSignatureDeclarationTarget, OwnerSignatureDeclarationPlan>,
    calls: BTreeMap<u32, OwnerSignatureCallPlan>,
    visited_environments: Vec<BTreeSet<Vec<ActiveBindingKey>>>,
    out_parameters: BTreeSet<u32>,
    signature_inputs_fingerprint_v1: [u8; 32],
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct ActiveBindingKey {
    name: String,
    target: OwnerEffectiveLexicalTarget,
    boundary_scope: u32,
    owns_evaluation_scope: bool,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct ChildBindingCacheKey {
    boundary_scope: Option<OwnerStableScopeRef>,
    active: Vec<ActiveBindingKey>,
}

impl From<&ActiveBinding> for ActiveBindingKey {
    fn from(binding: &ActiveBinding) -> Self {
        Self {
            name: binding.name.clone(),
            target: binding.target.clone(),
            boundary_scope: binding.boundary_scope,
            owns_evaluation_scope: binding.owns_evaluation_scope,
        }
    }
}

impl<'a> Planner<'a> {
    fn new(
        seed: &'a OwnerConstraintSeed,
        callable_resolutions: &'a OwnerCallableResolutionPlan,
        abi: &'a OwnerInferenceAbiEnvironment,
        interfaces: &'a BTreeMap<StableCheckOwnerKey, OwnerCallableLexicalSignature>,
    ) -> Result<Self, OwnerSignatureLexicalPlanError> {
        Self::new_with_environment(seed, callable_resolutions, abi, interfaces, None)
    }

    fn new_with_environment(
        seed: &'a OwnerConstraintSeed,
        callable_resolutions: &'a OwnerCallableResolutionPlan,
        abi: &'a OwnerInferenceAbiEnvironment,
        interfaces: &'a BTreeMap<StableCheckOwnerKey, OwnerCallableLexicalSignature>,
        inherited_environment: Option<&OwnerSignatureLexicalEnvironment>,
    ) -> Result<Self, OwnerSignatureLexicalPlanError> {
        let signature_inputs_fingerprint_v1 = signature_inputs_fingerprint_v1(
            seed,
            callable_resolutions,
            abi,
            interfaces,
            inherited_environment,
        )?;
        let resolution_by_expression = callable_resolutions
            .resolutions()
            .iter()
            .map(|resolution| (resolution.reference().expression.clone(), resolution))
            .collect();
        let inherited_environment = inherited_environment.cloned();
        let inherited_bindings = inherited_environment
            .as_ref()
            .map(|environment| environment.bindings.clone())
            .unwrap_or_default();
        let mut child_boundary_by_result = BTreeMap::new();
        for child in seed
            .signature_regions()
            .containment()
            .children
            .iter()
            .filter(|child| child.inherits_lexical_environment())
        {
            let expression = match &child.result_placement {
                crate::OwnerChildResultPlacementInput::ExpressionEdge { edge } => {
                    &edge.child_expression
                }
                crate::OwnerChildResultPlacementInput::StatementLane => {
                    child.result_expression.as_ref().ok_or_else(|| {
                        OwnerSignatureLexicalPlanError::new(
                            "inheriting statement-lane child boundary has no stable result expression",
                        )
                    })?
                }
                crate::OwnerChildResultPlacementInput::Valueless => {
                    return Err(OwnerSignatureLexicalPlanError::new(
                        "inheriting child boundary is marked valueless",
                    ));
                }
            };
            if child_boundary_by_result
                .insert(expression.clone(), child)
                .is_some()
            {
                return Err(OwnerSignatureLexicalPlanError::new(
                    "owner signature child boundaries repeat a stable result expression",
                ));
            }
        }
        let out_parameters = seed
            .declarations
            .iter()
            .find(|declaration| declaration.public)
            .into_iter()
            .flat_map(|declaration| &declaration.parameters)
            .filter(|parameter| parameter.kind == OwnerParameterKind::Out)
            .map(|parameter| parameter.ordinal)
            .collect();
        Ok(Self {
            seed,
            abi,
            interfaces,
            resolution_by_expression,
            inherited_environment,
            inherited_bindings,
            child_binding_cache: BTreeMap::new(),
            child_boundary_by_result,
            child_environments: BTreeMap::new(),
            reads: vec![None; seed.expressions.len()],
            expression_evaluation_scopes: vec![None; seed.expressions.len()],
            expression_evaluation_scope_seen: vec![false; seed.expressions.len()],
            declarations: BTreeMap::new(),
            calls: BTreeMap::new(),
            visited_environments: vec![BTreeSet::new(); seed.expressions.len()],
            out_parameters,
            signature_inputs_fingerprint_v1,
        })
    }

    fn signature(&self, expression: u32, function: &str) -> Option<CallSignature> {
        signature_for_call(
            self.seed,
            self.abi,
            self.interfaces,
            &self.resolution_by_expression,
            expression,
            function,
        )
    }

    fn matched_inputs(
        &self,
        expression: u32,
        signature: &CallSignature,
    ) -> (
        Vec<OwnerSignatureMatchedInputPlan>,
        Vec<OwnerSignatureCallLexicalError>,
    ) {
        if !signature.available
            || matches!(
                signature.target,
                OwnerSignatureCallTarget::Unresolved | OwnerSignatureCallTarget::Ambiguous { .. }
            )
        {
            return (Vec::new(), Vec::new());
        }
        let call = &self.seed.expressions[expression as usize];
        let pipe_inputs = call
            .inputs
            .iter()
            .filter(|input| matches!(input.role, OwnerConstraintEdgeRole::PipeInput))
            .collect::<Vec<_>>();
        let mut errors = Vec::new();
        let piped = pipe_inputs.first().copied();
        let piped_parameter = piped.and_then(|_| {
            signature
                .parameters
                .iter()
                .filter(|parameter| parameter.kind == OwnerParameterKind::Value)
                .min_by_key(|parameter| parameter.ordinal)
        });
        if piped.is_some() && piped_parameter.is_none() {
            errors.push(OwnerSignatureCallLexicalError::PipeWithoutValueInput);
        }
        let mut matched = Vec::new();
        if let (Some(input), Some(parameter)) = (piped, piped_parameter) {
            matched.push(OwnerSignatureMatchedInputPlan {
                formal_ordinal: parameter.ordinal,
                formal_name: parameter.name.clone(),
                formal_kind: parameter.kind,
                expression: input.expression,
                argument_kind: OwnerArgumentKind::Named,
                from_pipe: true,
                source: OwnerSignatureMatchedInputSource::PipeInput,
                evaluation_scope: parameter.evaluation_scope,
            });
        }
        let expected = signature
            .parameters
            .iter()
            .filter(|parameter| {
                piped_parameter.is_none_or(|piped| parameter.ordinal != piped.ordinal)
            })
            .collect::<Vec<_>>();
        let mut expected_index = 0usize;
        for (call_index, input) in call
            .inputs
            .iter()
            .filter(|input| {
                matches!(
                    input.role,
                    OwnerConstraintEdgeRole::CallArgument { .. }
                        | OwnerConstraintEdgeRole::PipeArgument { .. }
                )
            })
            .enumerate()
        {
            let (kind, name, source) = match &input.role {
                OwnerConstraintEdgeRole::CallArgument {
                    kind,
                    name,
                    ordinal,
                } => (
                    *kind,
                    name,
                    OwnerSignatureMatchedInputSource::CallArgument { ordinal: *ordinal },
                ),
                OwnerConstraintEdgeRole::PipeArgument {
                    kind,
                    name,
                    ordinal,
                } => (
                    *kind,
                    name,
                    OwnerSignatureMatchedInputSource::PipeArgument { ordinal: *ordinal },
                ),
                _ => unreachable!("filtered call argument role"),
            };
            while let Some(parameter) = expected.get(expected_index).copied()
                && parameter.name != *name
                && parameter.requirement.is_optional()
            {
                expected_index += 1;
            }
            let Some(parameter) = expected.get(expected_index).copied() else {
                errors.push(OwnerSignatureCallLexicalError::UnexpectedCallEntry {
                    name: name.clone(),
                    source,
                });
                continue;
            };
            if parameter.name != *name {
                errors.push(OwnerSignatureCallLexicalError::MisorderedCallEntry {
                    position: call_index as u32 + 1,
                    expected_name: parameter.name.clone(),
                    actual_name: name.clone(),
                    source,
                });
                expected_index += 1;
                continue;
            }
            expected_index += 1;
            if parameter.kind == OwnerParameterKind::Value && kind == OwnerArgumentKind::BareBinding
            {
                errors.push(OwnerSignatureCallLexicalError::BareOrdinaryInput {
                    name: name.clone(),
                    source,
                });
            }
            matched.push(OwnerSignatureMatchedInputPlan {
                formal_ordinal: parameter.ordinal,
                formal_name: parameter.name.clone(),
                formal_kind: parameter.kind,
                expression: input.expression,
                argument_kind: kind,
                from_pipe: false,
                source,
                evaluation_scope: parameter.evaluation_scope,
            });
        }
        for parameter in expected.iter().skip(expected_index) {
            if !parameter.requirement.is_optional() {
                errors.push(OwnerSignatureCallLexicalError::MissingCallEntry {
                    name: parameter.name.clone(),
                });
            }
        }
        let explicit_pass = call.inputs.iter().find_map(|input| match input.role {
            OwnerConstraintEdgeRole::CallPass { .. } => Some(OwnerSignaturePassSource::Call),
            OwnerConstraintEdgeRole::PipePass { .. } => Some(OwnerSignaturePassSource::Pipe),
            _ => None,
        });
        if signature.authoritative
            && let (Some(source), Some(callable_kind)) = (explicit_pass, signature.kind)
        {
            errors.push(
                OwnerSignatureCallLexicalError::PassOnAuthoritativeCallable {
                    source,
                    callable_kind,
                },
            );
        }
        matched.sort_by_key(|input| input.formal_ordinal);
        (matched, errors)
    }

    fn scope_descends_strictly(&self, mut scope: u32, ancestor: u32) -> bool {
        if scope == ancestor {
            return false;
        }
        let mut visited = BTreeSet::new();
        while visited.insert(scope) {
            let Some(parent) = self
                .seed
                .signature_regions()
                .scopes()
                .get(scope as usize)
                .and_then(|scope| scope.parent)
            else {
                return false;
            };
            if parent == ancestor {
                return true;
            }
            scope = parent;
        }
        false
    }

    fn effective_imported_target(target: &OwnerLexicalTargetRef) -> OwnerEffectiveLexicalTarget {
        match target {
            OwnerLexicalTargetRef::Ambiguous { name, .. } => {
                OwnerEffectiveLexicalTarget::Ambiguous { name: name.clone() }
            }
            _ => OwnerEffectiveLexicalTarget::Imported {
                target: target.clone(),
            },
        }
    }

    fn effective_base_target(
        target: &OwnerLexicalDeclarationTarget,
    ) -> OwnerEffectiveLexicalTarget {
        match target {
            OwnerLexicalDeclarationTarget::Imported { target } => {
                Self::effective_imported_target(target)
            }
            _ => OwnerEffectiveLexicalTarget::Static {
                target: target.clone(),
            },
        }
    }

    fn stable_effective_target(
        &self,
        target: &OwnerEffectiveLexicalTarget,
    ) -> Result<Option<OwnerLexicalTargetRef>, OwnerSignatureLexicalPlanError> {
        let target = match target {
            OwnerEffectiveLexicalTarget::Static { target } => self
                .seed
                .signature_regions()
                .stable_target(target)
                .cloned()
                .ok_or_else(|| {
                    OwnerSignatureLexicalPlanError::new(
                        "owner signature static target has no stable lexical identity",
                    )
                })?,
            OwnerEffectiveLexicalTarget::FreshOut {
                call,
                formal_ordinal,
            } => OwnerLexicalTargetRef::Declaration {
                owner: self.seed.owner.clone(),
                declaration: OwnerDeclarationStableKey::FreshOut {
                    call: call.clone(),
                    formal_ordinal: *formal_ordinal,
                },
                capability: OwnerLexicalDeclarationCapability::Out {
                    evaluation_scope: OwnerStableScopeRef {
                        owner: self.seed.owner.clone(),
                        scope: OwnerScopeStableKey::GeneratedOut {
                            call: call.clone(),
                            formal_ordinal: *formal_ordinal,
                        },
                    },
                },
            },
            OwnerEffectiveLexicalTarget::CallContext {
                call,
                context_ordinal,
            } => OwnerLexicalTargetRef::Declaration {
                owner: self.seed.owner.clone(),
                declaration: OwnerDeclarationStableKey::CallContext {
                    call: call.clone(),
                    ordinal: *context_ordinal,
                },
                capability: OwnerLexicalDeclarationCapability::Value,
            },
            OwnerEffectiveLexicalTarget::Imported { target } => target.clone(),
            OwnerEffectiveLexicalTarget::Ambiguous { name } => OwnerLexicalTargetRef::Ambiguous {
                owner: self.seed.owner.clone(),
                name: name.clone(),
            },
            OwnerEffectiveLexicalTarget::InvalidBareBinding { .. } => return Ok(None),
        };
        Ok(Some(target))
    }

    fn emit_child_environment(
        &mut self,
        boundary: &crate::OwnerLexicalChildBoundaryPlan,
        active: &[ActiveBinding],
        forced_scope: Option<&OwnerSignatureEvaluationScopePlan>,
    ) -> Result<(), OwnerSignatureLexicalPlanError> {
        let binding_cache_key = ChildBindingCacheKey {
            boundary_scope: boundary.scope.clone(),
            active: active.iter().map(ActiveBindingKey::from).collect(),
        };
        let bindings = if let Some(bindings) = self.child_binding_cache.get(&binding_cache_key) {
            bindings.clone()
        } else {
            let mut bindings = self
                .inherited_bindings
                .iter()
                .cloned()
                .map(|binding| (binding.name.clone(), binding))
                .collect::<BTreeMap<_, _>>();
            bindings.extend(
                boundary
                    .bindings
                    .iter()
                    .cloned()
                    .map(|binding| (binding.name.clone(), binding)),
            );
            for dynamic in active {
                let nested_authored = bindings
                    .get(&dynamic.name)
                    .and_then(|binding| binding.declaration_scope.as_ref())
                    .and_then(|scope| {
                        self.seed
                            .signature_regions()
                            .stable_scopes()
                            .iter()
                            .position(|candidate| candidate.as_ref() == Some(scope))
                    })
                    .and_then(|scope| u32::try_from(scope).ok())
                    .is_some_and(|scope| {
                        self.scope_descends_strictly(scope, dynamic.boundary_scope)
                    });
                if nested_authored {
                    continue;
                }
                let Some(target) = self.stable_effective_target(&dynamic.target)? else {
                    continue;
                };
                bindings.insert(
                    dynamic.name.clone(),
                    crate::OwnerLexicalBoundaryBindingPlan {
                        name: dynamic.name.clone(),
                        target,
                        declaration_scope: None,
                    },
                );
            }
            let bindings = OwnerLexicalBoundaryBindings::try_new(bindings.into_values().collect())
                .map_err(|error| {
                    OwnerSignatureLexicalPlanError::new(format!(
                        "cannot fingerprint signature lexical boundary bindings: {error}"
                    ))
                })?;
            self.child_binding_cache
                .insert(binding_cache_key, bindings.clone());
            bindings
        };
        let inherited_evaluation_scope = self
            .inherited_environment
            .as_ref()
            .and_then(|environment| environment.evaluation_scope.as_ref())
            .map(Self::effective_imported_target);
        let evaluation_scope = forced_scope
            .map(|scope| &scope.target)
            .or_else(|| {
                active
                    .iter()
                    .rev()
                    .find(|binding| binding.owns_evaluation_scope)
                    .map(|binding| &binding.target)
            })
            .or(inherited_evaluation_scope.as_ref())
            .map(|target| self.stable_effective_target(target))
            .transpose()?
            .flatten();
        let observation_expression = boundary.result_expression.clone().ok_or_else(|| {
            OwnerSignatureLexicalPlanError::new(
                "inheriting owner child boundary has no observation expression",
            )
        })?;
        let fingerprint_v1 = fingerprint(
            OWNER_SIGNATURE_LEXICAL_ENVIRONMENT_DOMAIN_V2,
            &(
                &self.seed.owner,
                &boundary.owner,
                &observation_expression,
                &boundary.scope,
                bindings.fingerprint_v1(),
                &evaluation_scope,
            ),
        )?;
        let environment = OwnerSignatureLexicalEnvironment {
            provider: self.seed.owner.clone(),
            consumer: boundary.owner.clone(),
            observation_expression,
            boundary_scope: boundary.scope.clone(),
            bindings,
            evaluation_scope,
            fingerprint_v1,
        };
        match self.child_environments.entry(boundary.owner.clone()) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(environment);
            }
            std::collections::btree_map::Entry::Occupied(entry) if entry.get() == &environment => {}
            std::collections::btree_map::Entry::Occupied(_) => {
                return Err(OwnerSignatureLexicalPlanError::new(format!(
                    "owner child {:?} is observed under conflicting lexical environments",
                    boundary.owner
                )));
            }
        }
        Ok(())
    }

    fn record_child_environment(
        &mut self,
        expression: u32,
        active: &[ActiveBinding],
        forced_scope: Option<&OwnerSignatureEvaluationScopePlan>,
    ) -> Result<(), OwnerSignatureLexicalPlanError> {
        let Some(external) = (expression as usize)
            .checked_sub(self.seed.expressions.len())
            .and_then(|index| self.seed.external_expressions.get(index))
        else {
            return Ok(());
        };
        let Some(boundary) = self
            .child_boundary_by_result
            .get(&external.expression)
            .copied()
            .cloned()
        else {
            return Ok(());
        };
        self.emit_child_environment(&boundary, active, forced_scope)
    }

    fn effective_target(
        &self,
        expression: u32,
        root: &str,
        active: &[ActiveBinding],
    ) -> Option<(OwnerEffectiveLexicalTarget, Option<u32>)> {
        let base = self
            .seed
            .lexical_reads()
            .get(expression as usize)
            .and_then(Option::as_ref);
        if matches!(
            base.map(|read| &read.target),
            Some(OwnerLexicalDeclarationTarget::Passed)
        ) {
            if self
                .seed
                .signature_regions()
                .stable_target(&OwnerLexicalDeclarationTarget::Passed)
                .is_none()
                && let Some(binding) = self.inherited_bindings.get(root)
            {
                return Some((Self::effective_imported_target(&binding.target), None));
            }
            return base.map(|read| {
                (
                    Self::effective_base_target(&read.target),
                    read.declaration_scope,
                )
            });
        }
        if root == "PASSED"
            && let Some(binding) = self.inherited_bindings.get(root)
        {
            return Some((Self::effective_imported_target(&binding.target), None));
        }
        let dynamic = active.iter().rev().find(|binding| binding.name == root);
        if let Some(dynamic) = dynamic {
            let nested_static = base
                .and_then(|read| read.declaration_scope)
                .is_some_and(|scope| self.scope_descends_strictly(scope, dynamic.boundary_scope));
            if !nested_static {
                return Some((dynamic.target.clone(), None));
            }
        }
        base.map(|read| {
            (
                Self::effective_base_target(&read.target),
                read.declaration_scope,
            )
        })
        .or_else(|| {
            self.inherited_bindings
                .get(root)
                .map(|binding| (Self::effective_imported_target(&binding.target), None))
        })
    }

    fn record_read(
        &mut self,
        expression: u32,
        target: OwnerEffectiveLexicalTarget,
        declaration_scope: Option<u32>,
        projection: Box<[String]>,
        access: OwnerLexicalAccess,
    ) {
        let row = OwnerEffectiveLexicalReadPlan {
            target,
            declaration_scope,
            projection,
            access,
        };
        let slot = &mut self.reads[expression as usize];
        match slot {
            None => *slot = Some(row),
            Some(previous) if previous == &row => {}
            Some(previous) => {
                let name = match &row.target {
                    OwnerEffectiveLexicalTarget::Ambiguous { name }
                    | OwnerEffectiveLexicalTarget::InvalidBareBinding { name, .. } => name.clone(),
                    _ => expression_read_parts(&self.seed.expressions[expression as usize].kind)
                        .map(|(name, _)| name)
                        .unwrap_or_else(|| "dynamic".to_owned()),
                };
                *previous = OwnerEffectiveLexicalReadPlan {
                    target: OwnerEffectiveLexicalTarget::Ambiguous { name },
                    declaration_scope: None,
                    projection: row.projection,
                    access: row.access,
                };
            }
        }
    }

    fn invalid_bare_target(
        &self,
        call: &StableExpressionKey,
        expression: u32,
        entry_ordinal: u32,
    ) -> Option<OwnerEffectiveLexicalTarget> {
        let (name, projection) =
            expression_read_parts(&self.seed.expressions.get(expression as usize)?.kind)?;
        projection
            .is_empty()
            .then(|| OwnerEffectiveLexicalTarget::InvalidBareBinding {
                call: call.clone(),
                entry_ordinal,
                name,
            })
    }

    fn process_call(
        &mut self,
        expression: u32,
        active: &[ActiveBinding],
    ) -> Result<(), OwnerSignatureLexicalPlanError> {
        if self.calls.contains_key(&expression) {
            return Err(OwnerSignatureLexicalPlanError::new(format!(
                "owner signature call expression {expression} is reachable through multiple lexical environments"
            )));
        }
        let node = &self.seed.expressions[expression as usize];
        let function = match &node.kind {
            OwnerConstraintNodeKind::Call { function } => function,
            OwnerConstraintNodeKind::Pipe { operation } => operation,
            _ => return Ok(()),
        };
        let signature = self.signature(expression, function).ok_or_else(|| {
            OwnerSignatureLexicalPlanError::new(format!(
                "owner signature lexical plan has no signature outcome for `{function}`"
            ))
        })?;
        let (matched_inputs, mut lexical_errors) = self.matched_inputs(expression, &signature);
        let boundary_scope = *self
            .seed
            .signature_regions()
            .expression_scopes()
            .get(expression as usize)
            .ok_or_else(|| {
                OwnerSignatureLexicalPlanError::new(
                    "owner signature call is outside the base expression-scope table",
                )
            })?;
        let parent_evaluation_scope = active
            .iter()
            .rev()
            .find(|binding| binding.owns_evaluation_scope)
            .map(|binding| binding.target.clone())
            .or_else(|| {
                self.inherited_environment
                    .as_ref()
                    .and_then(|environment| environment.evaluation_scope.as_ref())
                    .map(Self::effective_imported_target)
            });
        let mut valid = signature.available && lexical_errors.is_empty();
        let mut forced = BTreeMap::<u32, OwnerEffectiveLexicalTarget>::new();
        let mut invalid_bare = BTreeMap::<u32, OwnerEffectiveLexicalTarget>::new();
        for input in node.inputs.iter().filter(|input| {
            matches!(
                input.role,
                OwnerConstraintEdgeRole::CallArgument {
                    kind: OwnerArgumentKind::BareBinding,
                    ..
                } | OwnerConstraintEdgeRole::PipeArgument {
                    kind: OwnerArgumentKind::BareBinding,
                    ..
                }
            )
        }) {
            let ordinal = match input.role {
                OwnerConstraintEdgeRole::CallArgument { ordinal, .. }
                | OwnerConstraintEdgeRole::PipeArgument { ordinal, .. } => ordinal,
                _ => unreachable!("filtered bare argument"),
            };
            if let Some(target) =
                self.invalid_bare_target(&node.expression, input.expression, ordinal)
            {
                forced.insert(input.expression, target.clone());
                invalid_bare.insert(input.expression, target);
            }
        }

        let mut outputs = Vec::new();
        if valid {
            for input in matched_inputs
                .iter()
                .filter(|input| input.formal_kind == OwnerParameterKind::Out)
            {
                let Some((name, projection)) = self
                    .seed
                    .expressions
                    .get(input.expression as usize)
                    .and_then(|expression| expression_binding_parts(&expression.kind))
                else {
                    lexical_errors.push(OwnerSignatureCallLexicalError::InvalidForwardOutTarget {
                        formal_ordinal: input.formal_ordinal,
                        formal_name: input.formal_name.clone(),
                        expression: input.expression,
                    });
                    valid = false;
                    break;
                };
                if !projection.is_empty() {
                    lexical_errors.push(OwnerSignatureCallLexicalError::InvalidForwardOutTarget {
                        formal_ordinal: input.formal_ordinal,
                        formal_name: input.formal_name.clone(),
                        expression: input.expression,
                    });
                    valid = false;
                    break;
                }
                match input.argument_kind {
                    OwnerArgumentKind::BareBinding => {
                        let target = OwnerSignatureDeclarationTarget::FreshOut {
                            call: node.expression.clone(),
                            formal_ordinal: input.formal_ordinal,
                        };
                        let effective = OwnerEffectiveLexicalTarget::from(&target);
                        forced.insert(input.expression, effective);
                        self.declarations.insert(
                            target.clone(),
                            OwnerSignatureDeclarationPlan {
                                target: target.clone(),
                                name: name.clone(),
                                call_expression: expression,
                                boundary_scope,
                                parent_evaluation_scope: parent_evaluation_scope.clone(),
                                declaration_kind: OwnerSignatureDeclarationKind::FreshOut {
                                    formal_ordinal: input.formal_ordinal,
                                },
                            },
                        );
                        outputs.push(OwnerSignatureOutputBindingPlan::Fresh {
                            formal_ordinal: input.formal_ordinal,
                            name,
                            expression: input.expression,
                            target,
                        });
                    }
                    OwnerArgumentKind::Named => {
                        let Some((target, _)) =
                            self.effective_target(input.expression, &name, active)
                        else {
                            lexical_errors.push(
                                OwnerSignatureCallLexicalError::MissingEnclosingOut {
                                    formal_ordinal: input.formal_ordinal,
                                    formal_name: input.formal_name.clone(),
                                    expression: input.expression,
                                    target_name: name,
                                },
                            );
                            valid = false;
                            break;
                        };
                        let is_out = match &target {
                            OwnerEffectiveLexicalTarget::Static {
                                target: OwnerLexicalDeclarationTarget::Parameter { ordinal },
                            } => self.out_parameters.contains(ordinal),
                            OwnerEffectiveLexicalTarget::FreshOut { .. } => true,
                            OwnerEffectiveLexicalTarget::Imported {
                                target:
                                    OwnerLexicalTargetRef::Declaration {
                                        capability: OwnerLexicalDeclarationCapability::Out { .. },
                                        ..
                                    },
                            } => true,
                            _ => false,
                        };
                        if !is_out {
                            lexical_errors.push(
                                OwnerSignatureCallLexicalError::MissingEnclosingOut {
                                    formal_ordinal: input.formal_ordinal,
                                    formal_name: input.formal_name.clone(),
                                    expression: input.expression,
                                    target_name: name,
                                },
                            );
                            valid = false;
                            break;
                        }
                        outputs.push(OwnerSignatureOutputBindingPlan::Forward {
                            formal_ordinal: input.formal_ordinal,
                            name,
                            expression: input.expression,
                            target,
                        });
                    }
                }
            }
        }

        let mut contexts = Vec::new();
        if valid {
            let mut names = BTreeSet::new();
            for (context_ordinal, context) in signature.contexts.iter().enumerate() {
                if !names.insert(context.name.clone()) {
                    lexical_errors.push(OwnerSignatureCallLexicalError::DuplicateCallContext {
                        name: context.name.clone(),
                    });
                    valid = false;
                    break;
                }
                if !matched_inputs.iter().any(|input| {
                    input.formal_ordinal == context.provider_parameter_ordinal
                        && input.formal_kind == OwnerParameterKind::Value
                }) {
                    continue;
                }
                let context_ordinal = checked_u32(context_ordinal, "call context ordinal")?;
                let target = OwnerSignatureDeclarationTarget::CallContext {
                    call: node.expression.clone(),
                    context_ordinal,
                };
                self.declarations.insert(
                    target.clone(),
                    OwnerSignatureDeclarationPlan {
                        target: target.clone(),
                        name: context.name.clone(),
                        call_expression: expression,
                        boundary_scope,
                        parent_evaluation_scope: parent_evaluation_scope.clone(),
                        declaration_kind: OwnerSignatureDeclarationKind::CallContext {
                            context_ordinal,
                            context_kind: context.kind,
                            provider_parameter_ordinal: context.provider_parameter_ordinal,
                        },
                    },
                );
                contexts.push(OwnerSignatureCallContextPlan {
                    context_ordinal,
                    name: context.name.clone(),
                    kind: context.kind,
                    provider_parameter_ordinal: context.provider_parameter_ordinal,
                    target,
                });
            }
        }
        if !valid {
            for output in &outputs {
                if let OwnerSignatureOutputBindingPlan::Fresh {
                    expression, target, ..
                } = output
                {
                    self.declarations.remove(target);
                    if let Some(invalid) = invalid_bare.get(expression).cloned() {
                        forced.insert(*expression, invalid);
                    } else {
                        forced.remove(expression);
                    }
                }
            }
            for context in &contexts {
                self.declarations.remove(&context.target);
            }
            outputs.clear();
            contexts.clear();
        }

        let explicit_pass = node.inputs.iter().find_map(|input| {
            let source = match input.role {
                OwnerConstraintEdgeRole::CallPass { .. } => OwnerSignaturePassSource::Call,
                OwnerConstraintEdgeRole::PipePass { .. } => OwnerSignaturePassSource::Pipe,
                _ => return None,
            };
            Some(OwnerSignaturePassPlan {
                expression: input.expression,
                source,
            })
        });
        self.calls.insert(
            expression,
            OwnerSignatureCallPlan {
                expression,
                stable_expression: node.expression.clone(),
                structural_ordinal: checked_u32(
                    self.calls.len(),
                    "signature lexical call structural ordinal",
                )?,
                function: function.clone(),
                target: signature.target,
                valid,
                matched_inputs: matched_inputs.clone().into_boxed_slice(),
                outputs: outputs.clone().into_boxed_slice(),
                contexts: contexts.clone().into_boxed_slice(),
                explicit_pass,
                lexical_errors: lexical_errors.into_boxed_slice(),
            },
        );

        let matched_by_expression = matched_inputs
            .iter()
            .map(|input| (input.expression, input))
            .collect::<BTreeMap<_, _>>();
        for child in node.inputs.iter().map(|input| input.expression) {
            let mut child_active = active.to_vec();
            if valid && let Some(input) = matched_by_expression.get(&child).copied() {
                if let OwnerInterfaceEvaluationScope::Output { parameter_ordinal } =
                    input.evaluation_scope
                    && let Some(output) = outputs
                        .iter()
                        .find(|output| output.formal_ordinal() == parameter_ordinal)
                {
                    child_active.push(ActiveBinding {
                        name: output.name().to_owned(),
                        target: output.effective_target(),
                        boundary_scope,
                        owns_evaluation_scope: true,
                    });
                }
                if input.formal_kind == OwnerParameterKind::Value {
                    for context in &contexts {
                        if input.formal_ordinal != context.provider_parameter_ordinal {
                            child_active.push(ActiveBinding {
                                name: context.name.clone(),
                                target: OwnerEffectiveLexicalTarget::from(&context.target),
                                boundary_scope,
                                owns_evaluation_scope: false,
                            });
                        }
                    }
                }
            }
            let forced_scope = outputs.iter().find_map(|output| match output {
                OwnerSignatureOutputBindingPlan::Fresh {
                    expression, target, ..
                } if *expression == child => Some(OwnerSignatureEvaluationScopePlan {
                    target: OwnerEffectiveLexicalTarget::from(target),
                    boundary_scope,
                }),
                OwnerSignatureOutputBindingPlan::Fresh { .. }
                | OwnerSignatureOutputBindingPlan::Forward { .. } => None,
            });
            self.visit(
                child,
                &child_active,
                forced.get(&child).cloned(),
                forced_scope,
            )?;
        }
        Ok(())
    }

    fn visit(
        &mut self,
        expression: u32,
        active: &[ActiveBinding],
        forced: Option<OwnerEffectiveLexicalTarget>,
        forced_scope: Option<OwnerSignatureEvaluationScopePlan>,
    ) -> Result<(), OwnerSignatureLexicalPlanError> {
        if expression as usize >= self.seed.expressions.len() {
            self.record_child_environment(expression, active, forced_scope.as_ref())?;
            return Ok(());
        }
        let environment = active
            .iter()
            .map(ActiveBindingKey::from)
            .collect::<Vec<_>>();
        if !self.visited_environments[expression as usize].insert(environment) {
            return Ok(());
        }
        let evaluation_scope = forced_scope
            .or_else(|| {
                active
                    .iter()
                    .rev()
                    .find(|binding| binding.owns_evaluation_scope)
                    .map(|binding| OwnerSignatureEvaluationScopePlan {
                        target: binding.target.clone(),
                        boundary_scope: binding.boundary_scope,
                    })
            })
            .or_else(|| {
                self.inherited_environment
                    .as_ref()
                    .and_then(|environment| environment.evaluation_scope.as_ref())
                    .map(|target| OwnerSignatureEvaluationScopePlan {
                        target: Self::effective_imported_target(target),
                        boundary_scope: 0,
                    })
            });
        let index = expression as usize;
        if self.expression_evaluation_scope_seen[index] {
            if self.expression_evaluation_scopes[index] != evaluation_scope {
                return Err(OwnerSignatureLexicalPlanError::new(format!(
                    "owner signature lexical expression {expression} ({:?}) has conflicting evaluation scopes: {:?} versus {:?}",
                    self.seed.expressions[index].kind,
                    self.expression_evaluation_scopes[index],
                    evaluation_scope,
                )));
            }
        } else {
            self.expression_evaluation_scope_seen[index] = true;
            self.expression_evaluation_scopes[index] = evaluation_scope;
        }
        let node = &self.seed.expressions[expression as usize];
        if let Some((root, projection)) =
            expression_read_parts(&self.seed.expressions[expression as usize].kind)
        {
            if let Some(target) = forced {
                self.record_read(expression, target, None, projection, access(&node.kind));
            } else if let Some((target, declaration_scope)) =
                self.effective_target(expression, &root, active)
            {
                self.record_read(
                    expression,
                    target,
                    declaration_scope,
                    projection,
                    access(&node.kind),
                );
            }
        }
        if matches!(
            node.kind,
            OwnerConstraintNodeKind::Call { .. } | OwnerConstraintNodeKind::Pipe { .. }
        ) {
            return self.process_call(expression, active);
        }
        let children = node
            .inputs
            .iter()
            .filter(|input| is_signature_lexical_structural_edge(&input.role))
            .map(|input| input.expression)
            .collect::<Vec<_>>();
        for child in children {
            self.visit(child, active, None, None)?;
        }
        Ok(())
    }

    fn finish(mut self) -> Result<OwnerSignatureLexicalPlan, OwnerSignatureLexicalPlanError> {
        let mut children = BTreeSet::new();
        for expression in &self.seed.expressions {
            children.extend(
                expression
                    .inputs
                    .iter()
                    .filter(|input| is_signature_lexical_structural_edge(&input.role))
                    .filter(|input| (input.expression as usize) < self.seed.expressions.len())
                    .map(|input| input.expression),
            );
        }
        for expression in 0..self.seed.expressions.len() {
            let expression = checked_u32(expression, "signature lexical expression")?;
            if !children.contains(&expression) {
                self.visit(expression, &[], None, None)?;
            }
        }
        for expression in 0..self.seed.expressions.len() {
            let expression = checked_u32(expression, "signature lexical expression")?;
            if self.visited_environments[expression as usize].is_empty() {
                self.visit(expression, &[], None, None)?;
            }
        }
        for (index, base) in self.seed.lexical_reads().iter().enumerate() {
            if self.reads[index].is_none()
                && let Some(base) = base
            {
                self.reads[index] = Some(OwnerEffectiveLexicalReadPlan {
                    target: Self::effective_base_target(&base.target),
                    declaration_scope: base.declaration_scope,
                    projection: base.projection.clone(),
                    access: base.access,
                });
            }
        }
        let expression_by_key = self
            .seed
            .expressions
            .iter()
            .enumerate()
            .map(|(index, expression)| (expression.expression.clone(), index))
            .collect::<BTreeMap<_, _>>();
        let external_candidates = self
            .seed
            .references
            .iter()
            .filter(|reference| {
                reference.kind == OwnerReferenceKind::Callable
                    || expression_by_key
                        .get(&reference.expression)
                        .is_none_or(|index| self.reads[*index].is_none())
            })
            .cloned()
            .collect::<Vec<_>>();
        let declaration_targets = self.declarations.keys().cloned().collect::<BTreeSet<_>>();
        let call_declaration_targets = self
            .calls
            .values()
            .flat_map(|call| {
                call.outputs
                    .iter()
                    .filter_map(|output| match output {
                        OwnerSignatureOutputBindingPlan::Fresh { target, .. } => {
                            Some(target.clone())
                        }
                        OwnerSignatureOutputBindingPlan::Forward { .. } => None,
                    })
                    .chain(call.contexts.iter().map(|context| context.target.clone()))
            })
            .collect::<BTreeSet<_>>();
        if declaration_targets != call_declaration_targets {
            return Err(OwnerSignatureLexicalPlanError::new(
                "owner signature declarations do not match exact call-owned bindings",
            ));
        }
        for target in self
            .reads
            .iter()
            .filter_map(Option::as_ref)
            .map(|read| &read.target)
            .chain(
                self.expression_evaluation_scopes
                    .iter()
                    .filter_map(Option::as_ref)
                    .map(|scope| &scope.target),
            )
            .chain(self.calls.values().flat_map(|call| {
                call.outputs.iter().filter_map(|output| match output {
                    OwnerSignatureOutputBindingPlan::Forward { target, .. } => Some(target),
                    OwnerSignatureOutputBindingPlan::Fresh { .. } => None,
                })
            }))
        {
            let declaration = match target {
                OwnerEffectiveLexicalTarget::FreshOut {
                    call,
                    formal_ordinal,
                } => Some(OwnerSignatureDeclarationTarget::FreshOut {
                    call: call.clone(),
                    formal_ordinal: *formal_ordinal,
                }),
                OwnerEffectiveLexicalTarget::CallContext {
                    call,
                    context_ordinal,
                } => Some(OwnerSignatureDeclarationTarget::CallContext {
                    call: call.clone(),
                    context_ordinal: *context_ordinal,
                }),
                OwnerEffectiveLexicalTarget::Static { .. }
                | OwnerEffectiveLexicalTarget::Imported { .. }
                | OwnerEffectiveLexicalTarget::InvalidBareBinding { .. }
                | OwnerEffectiveLexicalTarget::Ambiguous { .. } => None,
            };
            if declaration.is_some_and(|target| !declaration_targets.contains(&target)) {
                return Err(OwnerSignatureLexicalPlanError::new(
                    "owner signature read or evaluation scope references an undeclared dynamic target",
                ));
            }
        }
        let unobserved_children = self
            .child_boundary_by_result
            .values()
            .filter(|child| !self.child_environments.contains_key(&child.owner))
            .copied()
            .cloned()
            .collect::<Vec<_>>();
        for child in &unobserved_children {
            self.emit_child_environment(child, &[], None)?;
        }
        let declarations = self.declarations.into_values().collect::<Vec<_>>();
        let calls = self.calls.into_values().collect::<Vec<_>>();
        let expected_children = self
            .child_boundary_by_result
            .values()
            .map(|child| child.owner.clone())
            .collect::<BTreeSet<_>>();
        if self
            .child_environments
            .keys()
            .cloned()
            .collect::<BTreeSet<_>>()
            != expected_children
        {
            return Err(OwnerSignatureLexicalPlanError::new(
                "owner signature plan did not observe every inheriting child boundary",
            ));
        }
        let child_environments = self.child_environments.into_values().collect::<Vec<_>>();
        let mut imported_capture_sites =
            BTreeMap::<OwnerLexicalTargetRef, BTreeSet<StableExpressionKey>>::new();
        for (index, read) in self.reads.iter().enumerate() {
            let Some(OwnerEffectiveLexicalReadPlan {
                target: OwnerEffectiveLexicalTarget::Imported { target },
                ..
            }) = read
            else {
                continue;
            };
            if matches!(
                target,
                OwnerLexicalTargetRef::Declaration {
                    capability: OwnerLexicalDeclarationCapability::Value
                        | OwnerLexicalDeclarationCapability::Out { .. },
                    ..
                } | OwnerLexicalTargetRef::ContextFormal { .. }
            ) {
                imported_capture_sites
                    .entry(target.clone())
                    .or_default()
                    .insert(self.seed.expressions[index].expression.clone());
            }
        }
        // PASSED is also an implicit call input. A value child may need the
        // originating callable formal without spelling `PASSED` in a direct
        // read, so retain that one inherited capability even when the read
        // table itself does not demand it.
        if let Some(environment) = &self.inherited_environment {
            for binding in environment.bindings.iter() {
                if matches!(binding.target, OwnerLexicalTargetRef::ContextFormal { .. }) {
                    imported_capture_sites
                        .entry(binding.target.clone())
                        .or_default()
                        .insert(environment.observation_expression.clone());
                }
            }
        }
        let imported_captures = imported_capture_sites.keys().cloned().collect::<Vec<_>>();
        let imported_capture_sites = imported_capture_sites
            .into_iter()
            .map(|(target, sites)| OwnerImportedLexicalCaptureSites {
                target,
                sites: sites.into_iter().collect::<Vec<_>>().into_boxed_slice(),
            })
            .collect::<Vec<_>>();
        let mut call_indices = vec![None; self.seed.expressions.len()];
        for (index, call) in calls.iter().enumerate() {
            let slot = call_indices
                .get_mut(call.expression as usize)
                .ok_or_else(|| {
                    OwnerSignatureLexicalPlanError::new(
                        "owner signature call is outside the expression namespace",
                    )
                })?;
            if slot.replace(index).is_some() {
                return Err(OwnerSignatureLexicalPlanError::new(
                    "owner signature plan repeats one call expression",
                ));
            }
        }
        let reads_fingerprint_v1 = fingerprint(
            OWNER_SIGNATURE_LEXICAL_READS_DOMAIN_V2,
            &(&self.seed.owner, &self.reads, &external_candidates),
        )?;
        let child_environment_fingerprints = child_environments
            .iter()
            .map(OwnerSignatureLexicalEnvironment::fingerprint_v1)
            .collect::<Vec<_>>();
        let fingerprint_v1 = fingerprint(
            OWNER_SIGNATURE_LEXICAL_PLAN_DOMAIN_V3,
            &(
                &self.seed.owner,
                self.seed.fingerprint_v1(),
                self.signature_inputs_fingerprint_v1,
                self.seed.lexical_reads_fingerprint_v1(),
                self.seed.signature_regions().fingerprint_v1(),
                &self.reads,
                &self.expression_evaluation_scopes,
                &declarations,
                &calls,
                self.inherited_environment
                    .as_ref()
                    .map(OwnerSignatureLexicalEnvironment::fingerprint_v1),
                &child_environment_fingerprints,
                &imported_captures,
                &imported_capture_sites,
                &external_candidates,
            ),
        )?;
        Ok(OwnerSignatureLexicalPlan {
            owner: self.seed.owner.clone(),
            seed_fingerprint_v1: self.seed.fingerprint_v1(),
            signature_inputs_fingerprint_v1: self.signature_inputs_fingerprint_v1,
            base_reads_fingerprint_v1: self.seed.lexical_reads_fingerprint_v1(),
            signature_regions_fingerprint_v1: self.seed.signature_regions().fingerprint_v1(),
            reads: Arc::from(self.reads),
            expression_evaluation_scopes: Arc::from(self.expression_evaluation_scopes),
            declarations: Arc::from(declarations),
            calls: Arc::from(calls),
            call_indices: Arc::from(call_indices),
            inherited_environment: self.inherited_environment.map(Arc::new),
            child_environments: Arc::from(child_environments),
            imported_captures: Arc::from(imported_captures),
            imported_capture_sites: Arc::from(imported_capture_sites),
            external_candidates: Arc::from(external_candidates),
            reads_fingerprint_v1,
            fingerprint_v1,
        })
    }
}

/// Project the exact signature-backed lexical plan after callable signatures
/// are frozen. The same function is used by body inference and, during the
/// interface-scope prepass, against dependency-first callable signatures.
pub fn project_owner_signature_lexical_plan<'a>(
    seed: &'a OwnerConstraintSeed,
    lexical_plan: &'a OwnerLexicalPlan,
    summary: &'a OwnerConstraintSummary,
    abi: &'a OwnerInferenceAbiEnvironment,
    interfaces: impl IntoIterator<Item = &'a OwnerPublicInterface>,
) -> Result<OwnerSignatureLexicalPlan, OwnerSignatureLexicalPlanError> {
    if seed.lexical_reads_fingerprint_v1() != lexical_plan.reads_fingerprint_v1()
        || seed.signature_regions().fingerprint_v1()
            != lexical_plan.signature_regions().fingerprint_v1()
    {
        return Err(OwnerSignatureLexicalPlanError::new(
            "owner signature lexical plan received a stale base lexical plan",
        ));
    }
    project_owner_signature_lexical_plan_from_signatures(
        seed,
        summary,
        abi,
        interfaces
            .into_iter()
            .map(OwnerCallableLexicalSignature::from_interface),
    )
}

pub fn project_owner_signature_lexical_plan_from_signatures<'a>(
    seed: &'a OwnerConstraintSeed,
    summary: &'a OwnerConstraintSummary,
    abi: &'a OwnerInferenceAbiEnvironment,
    signatures: impl IntoIterator<Item = OwnerCallableLexicalSignature>,
) -> Result<OwnerSignatureLexicalPlan, OwnerSignatureLexicalPlanError> {
    let callable_resolutions = project_owner_callable_resolution_plan(
        seed,
        summary
            .symbol_resolutions
            .iter()
            .filter(|resolution| resolution.reference().kind == OwnerReferenceKind::Callable)
            .cloned(),
    )?;
    project_owner_signature_lexical_plan_from_callable_resolutions(
        seed,
        &callable_resolutions,
        abi,
        signatures,
    )
}

pub fn project_owner_signature_lexical_plan_from_callable_resolutions<'a>(
    seed: &'a OwnerConstraintSeed,
    callable_resolutions: &'a OwnerCallableResolutionPlan,
    abi: &'a OwnerInferenceAbiEnvironment,
    signatures: impl IntoIterator<Item = OwnerCallableLexicalSignature>,
) -> Result<OwnerSignatureLexicalPlan, OwnerSignatureLexicalPlanError> {
    let mut by_owner = BTreeMap::new();
    for signature in signatures {
        if by_owner
            .insert(signature.owner.clone(), signature.clone())
            .is_some()
        {
            return Err(OwnerSignatureLexicalPlanError::new(format!(
                "owner signature lexical plan received duplicate interface {:?}",
                signature.owner
            )));
        }
    }
    project_owner_signature_lexical_plan_with_signature_map(
        seed,
        callable_resolutions,
        abi,
        &by_owner,
    )
}

fn project_owner_signature_lexical_plan_with_signature_map<'a>(
    seed: &'a OwnerConstraintSeed,
    callable_resolutions: &'a OwnerCallableResolutionPlan,
    abi: &'a OwnerInferenceAbiEnvironment,
    signatures: &'a BTreeMap<StableCheckOwnerKey, OwnerCallableLexicalSignature>,
) -> Result<OwnerSignatureLexicalPlan, OwnerSignatureLexicalPlanError> {
    Planner::new(seed, callable_resolutions, abi, signatures)?.finish()
}

fn project_owner_signature_lexical_plan_with_environment<'a>(
    seed: &'a OwnerConstraintSeed,
    callable_resolutions: &'a OwnerCallableResolutionPlan,
    abi: &'a OwnerInferenceAbiEnvironment,
    signatures: &'a BTreeMap<StableCheckOwnerKey, OwnerCallableLexicalSignature>,
    inherited_environment: Option<&OwnerSignatureLexicalEnvironment>,
) -> Result<OwnerSignatureLexicalPlan, OwnerSignatureLexicalPlanError> {
    Planner::new_with_environment(
        seed,
        callable_resolutions,
        abi,
        signatures,
        inherited_environment,
    )?
    .finish()
}

fn containment_cluster_members<'a>(
    callable_resolutions: &BTreeMap<StableCheckOwnerKey, &'a OwnerCallableResolutionPlan>,
) -> Result<
    (
        BTreeMap<StableCheckOwnerKey, Box<[StableCheckOwnerKey]>>,
        BTreeMap<StableCheckOwnerKey, StableCheckOwnerKey>,
    ),
    OwnerSignatureLexicalPlanError,
> {
    let mut parent_by_child = BTreeMap::<StableCheckOwnerKey, StableCheckOwnerKey>::new();
    for (owner, plan) in callable_resolutions {
        for child in &plan.containment().children {
            if !child.inherits_lexical_environment() {
                continue;
            }
            if !callable_resolutions.contains_key(&child.owner) {
                return Err(OwnerSignatureLexicalPlanError::new(format!(
                    "owner lexical containment cluster omits child {:?}",
                    child.owner
                )));
            }
            if parent_by_child
                .insert(child.owner.clone(), owner.clone())
                .is_some()
            {
                return Err(OwnerSignatureLexicalPlanError::new(
                    "owner lexical containment child has multiple parents",
                ));
            }
        }
    }
    let mut root_by_owner = BTreeMap::new();
    let mut members_by_root = BTreeMap::<StableCheckOwnerKey, Vec<StableCheckOwnerKey>>::new();
    for owner in callable_resolutions.keys() {
        let mut root = owner.clone();
        let mut visited = BTreeSet::new();
        while let Some(parent) = parent_by_child.get(&root) {
            if !visited.insert(root.clone()) {
                return Err(OwnerSignatureLexicalPlanError::new(
                    "owner lexical containment cluster contains a cycle",
                ));
            }
            root = parent.clone();
        }
        root_by_owner.insert(owner.clone(), root.clone());
        members_by_root.entry(root).or_default().push(owner.clone());
    }
    Ok((
        members_by_root
            .into_iter()
            .map(|(root, mut members)| {
                members.sort();
                (root, members.into_boxed_slice())
            })
            .collect(),
        root_by_owner,
    ))
}

fn project_containment_cluster(
    root: &StableCheckOwnerKey,
    members: &[StableCheckOwnerKey],
    seeds: &BTreeMap<StableCheckOwnerKey, &OwnerConstraintSeed>,
    callable_resolutions: &BTreeMap<StableCheckOwnerKey, &OwnerCallableResolutionPlan>,
    abi: &OwnerInferenceAbiEnvironment,
    signatures: &BTreeMap<StableCheckOwnerKey, OwnerCallableLexicalSignature>,
) -> Result<BTreeMap<StableCheckOwnerKey, OwnerSignatureLexicalPlan>, OwnerSignatureLexicalPlanError>
{
    let expected = members.iter().cloned().collect::<BTreeSet<_>>();
    if !expected.contains(root) {
        return Err(OwnerSignatureLexicalPlanError::new(
            "owner lexical containment cluster root is not a member",
        ));
    }
    let mut environments = BTreeMap::<StableCheckOwnerKey, OwnerSignatureLexicalEnvironment>::new();
    let mut pending = vec![root.clone()];
    let mut plans = BTreeMap::new();
    while let Some(owner) = pending.pop() {
        if plans.contains_key(&owner) {
            continue;
        }
        let environment = environments.get(&owner);
        let plan = project_owner_signature_lexical_plan_with_environment(
            seeds[&owner],
            callable_resolutions[&owner],
            abi,
            signatures,
            environment,
        )?;
        for child_environment in plan.child_environments() {
            if !expected.contains(&child_environment.consumer) {
                return Err(OwnerSignatureLexicalPlanError::new(
                    "owner signature plan exports an environment outside its containment cluster",
                ));
            }
            match environments.entry(child_environment.consumer.clone()) {
                std::collections::btree_map::Entry::Vacant(entry) => {
                    entry.insert(child_environment.clone());
                    pending.push(child_environment.consumer.clone());
                }
                std::collections::btree_map::Entry::Occupied(entry)
                    if entry.get() == child_environment => {}
                std::collections::btree_map::Entry::Occupied(_) => {
                    return Err(OwnerSignatureLexicalPlanError::new(
                        "owner lexical containment child receives conflicting environments",
                    ));
                }
            }
        }
        plans.insert(owner, plan);
    }
    if plans.keys().cloned().collect::<BTreeSet<_>>() != expected {
        return Err(OwnerSignatureLexicalPlanError::new(
            "owner lexical containment projection did not reach every cluster member",
        ));
    }
    Ok(plans)
}

fn base_callable_signature(seed: &OwnerConstraintSeed) -> OwnerCallableLexicalSignature {
    let parameters = seed
        .declarations
        .iter()
        .find(|declaration| declaration.public)
        .into_iter()
        .flat_map(|declaration| &declaration.parameters)
        .map(|parameter| OwnerCallableLexicalParameter {
            name: parameter.name.clone(),
            kind: parameter.kind,
            ordinal: parameter.ordinal,
            requirement: CheckedParameterRequirement::Required,
            evaluation_scope: OwnerInterfaceEvaluationScope::Parent,
        })
        .collect::<Vec<_>>()
        .into_boxed_slice();
    OwnerCallableLexicalSignature {
        owner: seed.owner.clone(),
        parameters,
    }
}

fn public_output_scope_target(
    target: &OwnerEffectiveLexicalTarget,
    declarations: &BTreeMap<OwnerSignatureDeclarationTarget, &OwnerSignatureDeclarationPlan>,
) -> Result<Option<u32>, OwnerSignatureLexicalPlanError> {
    let mut current = target;
    let mut visited = BTreeSet::new();
    loop {
        match current {
            OwnerEffectiveLexicalTarget::Static {
                target: OwnerLexicalDeclarationTarget::Parameter { ordinal },
            } => return Ok(Some(*ordinal)),
            OwnerEffectiveLexicalTarget::FreshOut {
                call,
                formal_ordinal,
            } => {
                let declaration_target = OwnerSignatureDeclarationTarget::FreshOut {
                    call: call.clone(),
                    formal_ordinal: *formal_ordinal,
                };
                if !visited.insert(declaration_target.clone()) {
                    return Err(OwnerSignatureLexicalPlanError::new(
                        "owner signature evaluation scopes contain a dynamic cycle",
                    ));
                }
                let declaration = declarations.get(&declaration_target).ok_or_else(|| {
                    OwnerSignatureLexicalPlanError::new(
                        "owner signature evaluation scope references a missing fresh output",
                    )
                })?;
                let Some(parent) = declaration.parent_evaluation_scope.as_ref() else {
                    return Ok(None);
                };
                current = parent;
            }
            OwnerEffectiveLexicalTarget::Static { .. }
            | OwnerEffectiveLexicalTarget::Imported { .. }
            | OwnerEffectiveLexicalTarget::CallContext { .. }
            | OwnerEffectiveLexicalTarget::InvalidBareBinding { .. }
            | OwnerEffectiveLexicalTarget::Ambiguous { .. } => return Ok(None),
        }
    }
}

fn projected_parameter_scope_updates(
    seed: &OwnerConstraintSeed,
    signature: &OwnerCallableLexicalSignature,
    plan: &OwnerSignatureLexicalPlan,
) -> Result<BTreeMap<u32, OwnerInterfaceEvaluationScope>, OwnerSignatureLexicalPlanError> {
    let parameters = signature
        .parameters
        .iter()
        .map(|parameter| (parameter.ordinal, parameter))
        .collect::<BTreeMap<_, _>>();
    if plan.reads().len() != seed.expressions.len()
        || plan.expression_evaluation_scopes().len() != seed.expressions.len()
    {
        return Err(OwnerSignatureLexicalPlanError::new(
            "owner signature parameter-scope projection has mismatched expression tables",
        ));
    }
    let declarations = plan
        .declarations()
        .iter()
        .map(|declaration| (declaration.target.clone(), declaration))
        .collect::<BTreeMap<_, _>>();
    let mut updates = BTreeMap::new();
    for (read, evaluation_scope) in plan.reads().iter().zip(plan.expression_evaluation_scopes()) {
        let Some(OwnerEffectiveLexicalReadPlan {
            target:
                OwnerEffectiveLexicalTarget::Static {
                    target:
                        OwnerLexicalDeclarationTarget::Parameter {
                            ordinal: owner_parameter_ordinal,
                        },
                },
            ..
        }) = read
        else {
            continue;
        };
        if !parameters
            .get(owner_parameter_ordinal)
            .is_some_and(|parameter| parameter.kind == OwnerParameterKind::Value)
        {
            continue;
        }
        let Some(evaluation_scope) = evaluation_scope else {
            continue;
        };
        let Some(owner_output_ordinal) =
            public_output_scope_target(&evaluation_scope.target, &declarations)?
        else {
            continue;
        };
        if !parameters
            .get(&owner_output_ordinal)
            .is_some_and(|parameter| parameter.kind == OwnerParameterKind::Out)
        {
            continue;
        }
        let incoming = OwnerInterfaceEvaluationScope::Output {
            parameter_ordinal: owner_output_ordinal,
        };
        match updates.entry(*owner_parameter_ordinal) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(incoming);
            }
            std::collections::btree_map::Entry::Occupied(entry) if *entry.get() == incoming => {}
            std::collections::btree_map::Entry::Occupied(entry) => {
                let parameter = parameters[owner_parameter_ordinal];
                return Err(OwnerSignatureLexicalPlanError::new(format!(
                    "owner signature {:?} parameter `{}` requires incompatible evaluation scopes {:?} and {:?}",
                    seed.owner,
                    parameter.name,
                    entry.get(),
                    incoming
                )));
            }
        }
    }
    Ok(updates)
}

/// Infer callable parameter scope effects and project exact dynamic lexical
/// plans for one closed owner set.
///
/// The call graph is decomposed independently of the type/value interface
/// graph. Acyclic callees are frozen before callers. Recursive components are
/// accepted only when no member acquires a contextual parameter scope, which
/// is the language's current explicit boundary for recursive contextual
/// functions.
pub fn project_owner_signature_lexical_scope_plans<'a>(
    seeds: impl IntoIterator<Item = &'a OwnerConstraintSeed>,
    summaries: impl IntoIterator<Item = &'a OwnerConstraintSummary>,
    abi: &OwnerInferenceAbiEnvironment,
    dependency_interfaces: impl IntoIterator<Item = &'a OwnerPublicInterface>,
) -> Result<OwnerSignatureLexicalScopeProjection, OwnerSignatureLexicalPlanError> {
    let seeds = seeds.into_iter().collect::<Vec<_>>();
    let summaries = summaries
        .into_iter()
        .map(|summary| (summary.owner.clone(), summary))
        .collect::<BTreeMap<_, _>>();
    let expected = seeds
        .iter()
        .map(|seed| &seed.owner)
        .collect::<BTreeSet<_>>();
    if seeds.is_empty() || expected != summaries.keys().collect() {
        return Err(OwnerSignatureLexicalPlanError::new(
            "owner signature scope projection inputs do not name one non-empty owner set",
        ));
    }
    let callable_resolutions = seeds
        .iter()
        .map(|seed| {
            let summary = summaries[&seed.owner];
            if summary.seed_fingerprint_v1 != seed.fingerprint_v1() {
                return Err(OwnerSignatureLexicalPlanError::new(format!(
                    "owner signature scope projection has a stale summary for {:?}",
                    seed.owner
                )));
            }
            project_owner_callable_resolution_plan(
                seed,
                summary
                    .symbol_resolutions
                    .iter()
                    .filter(|resolution| {
                        resolution.reference().kind == OwnerReferenceKind::Callable
                    })
                    .cloned(),
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    project_owner_signature_lexical_scope_plans_with_callable_resolutions(
        seeds,
        &callable_resolutions,
        abi,
        dependency_interfaces
            .into_iter()
            .map(OwnerCallableLexicalSignature::from_interface),
    )
}

pub fn project_owner_signature_lexical_scope_plans_with_callable_resolutions<'a>(
    seeds: impl IntoIterator<Item = &'a OwnerConstraintSeed>,
    callable_resolutions: impl IntoIterator<Item = &'a OwnerCallableResolutionPlan>,
    abi: &OwnerInferenceAbiEnvironment,
    dependency_signatures: impl IntoIterator<Item = OwnerCallableLexicalSignature>,
) -> Result<OwnerSignatureLexicalScopeProjection, OwnerSignatureLexicalPlanError> {
    let seeds = seeds
        .into_iter()
        .map(|seed| (seed.owner.clone(), seed))
        .collect::<BTreeMap<_, _>>();
    let callable_resolutions = callable_resolutions
        .into_iter()
        .map(|plan| (plan.owner().clone(), plan))
        .collect::<BTreeMap<_, _>>();
    if seeds.is_empty()
        || seeds.keys().collect::<BTreeSet<_>>() != callable_resolutions.keys().collect()
    {
        return Err(OwnerSignatureLexicalPlanError::new(
            "owner signature scope projection inputs do not name one non-empty owner set",
        ));
    }
    for (owner, seed) in &seeds {
        if !callable_resolutions[owner].matches_seed(seed) {
            return Err(OwnerSignatureLexicalPlanError::new(format!(
                "owner signature scope projection has a stale callable plan for {owner:?}"
            )));
        }
    }

    let mut available_signatures = BTreeMap::new();
    for signature in dependency_signatures {
        match available_signatures.entry(signature.owner.clone()) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(signature);
            }
            std::collections::btree_map::Entry::Occupied(entry) if entry.get() == &signature => {}
            std::collections::btree_map::Entry::Occupied(entry) => {
                return Err(OwnerSignatureLexicalPlanError::new(format!(
                    "owner signature scope projection received conflicting dependency interfaces for {:?}",
                    entry.key()
                )));
            }
        }
    }
    let local_signatures = seeds
        .iter()
        .map(|(owner, seed)| (owner.clone(), base_callable_signature(seed)))
        .collect::<BTreeMap<_, _>>();
    for (owner, signature) in &local_signatures {
        if available_signatures
            .insert(owner.clone(), signature.clone())
            .is_some()
        {
            return Err(OwnerSignatureLexicalPlanError::new(format!(
                "owner signature scope projection dependency set contains local owner {owner:?}"
            )));
        }
    }

    let (containment_clusters, containment_root_by_owner) =
        containment_cluster_members(&callable_resolutions)?;

    // Resolve shape validity and inherited environments before deciding
    // recursion. A misspelled call is not a call edge, and a multi-owner
    // lexical containment cluster is not itself recursive.
    let mut preliminary_plans = BTreeMap::new();
    for (root, members) in &containment_clusters {
        preliminary_plans.extend(project_containment_cluster(
            root,
            members,
            &seeds,
            &callable_resolutions,
            abi,
            &available_signatures,
        )?);
    }
    let call_edges = preliminary_plans
        .iter()
        .flat_map(|(caller, plan)| {
            plan.calls().iter().filter_map(|call| {
                let OwnerSignatureCallTarget::Owner { owner: callee } = &call.target else {
                    return None;
                };
                (call.valid && seeds.contains_key(callee)).then(|| (caller.clone(), callee.clone()))
            })
        })
        .collect::<BTreeSet<_>>();
    let mut callees_by_caller =
        BTreeMap::<StableCheckOwnerKey, BTreeSet<StableCheckOwnerKey>>::new();
    for (caller, callee) in &call_edges {
        callees_by_caller
            .entry(caller.clone())
            .or_default()
            .insert(callee.clone());
    }
    let cluster_call_edges = call_edges
        .iter()
        .map(|(caller, callee)| {
            (
                containment_root_by_owner[caller].clone(),
                containment_root_by_owner[callee].clone(),
            )
        })
        .collect::<BTreeSet<_>>();

    let components = if containment_clusters.len() == 1 {
        vec![vec![containment_clusters.keys().next().unwrap().clone()]]
    } else {
        let mut builder = DenseProjectionGraphBuilder::new();
        let mut projection_by_root = BTreeMap::<StableCheckOwnerKey, ProjectionId>::new();
        let mut root_by_projection = BTreeMap::<ProjectionId, StableCheckOwnerKey>::new();
        for (root, members) in &containment_clusters {
            let region_fingerprints = members
                .iter()
                .map(|owner| seeds[owner].signature_regions().fingerprint_v1())
                .collect::<Vec<_>>();
            let projection = builder
                .register(
                    stable_check_owner_key_fingerprint_v1(root),
                    fingerprint(
                        OWNER_LEXICAL_CONTAINMENT_CLUSTER_DOMAIN_V1,
                        &(root, &region_fingerprints),
                    )?,
                )
                .map_err(|error| {
                    OwnerSignatureLexicalPlanError::new(format!(
                        "cannot register owner callable scope projection: {error}"
                    ))
                })?;
            projection_by_root.insert(root.clone(), projection);
            root_by_projection.insert(projection, root.clone());
        }
        for (caller, callee) in &cluster_call_edges {
            builder
                .add_dependency(projection_by_root[caller], projection_by_root[callee])
                .map_err(|error| {
                    OwnerSignatureLexicalPlanError::new(format!(
                        "cannot add owner callable scope dependency: {error}"
                    ))
                })?;
        }
        let graph = builder
            .seal(ProjectionGraphDigestDomains {
                component: OWNER_CALLABLE_SCOPE_COMPONENT_DOMAIN_V2,
            })
            .map_err(|error| {
                OwnerSignatureLexicalPlanError::new(format!(
                    "cannot seal owner callable scope graph: {error}"
                ))
            })?;
        (0..graph.component_count())
            .map(|component| {
                let mut members = graph
                    .component_members_by_ordinal(component)
                    .ok_or_else(|| {
                        OwnerSignatureLexicalPlanError::new(
                            "owner callable scope graph has a missing component",
                        )
                    })?
                    .map(|projection| {
                        root_by_projection.get(&projection).cloned().ok_or_else(|| {
                            OwnerSignatureLexicalPlanError::new(
                                "owner callable scope graph contains an unknown projection",
                            )
                        })
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                members.sort();
                Ok(members)
            })
            .collect::<Result<Vec<_>, OwnerSignatureLexicalPlanError>>()?
    };

    let mut plans = BTreeMap::new();
    let mut projected_signatures = BTreeMap::new();
    for component_roots in components {
        let cyclic = component_roots.len() > 1
            || component_roots
                .first()
                .is_some_and(|root| cluster_call_edges.contains(&(root.clone(), root.clone())));
        let component_member_capacity: usize = component_roots
            .iter()
            .map(|root| containment_clusters[root].len())
            .sum();
        let mut component_updates = Vec::with_capacity(component_member_capacity);
        let mut component_plans = BTreeMap::new();
        for root in &component_roots {
            let members = &containment_clusters[root];
            let needs_reprojection = members.iter().any(|owner| {
                callees_by_caller.get(owner).is_some_and(|callees| {
                    callees
                        .iter()
                        .any(|callee| available_signatures[callee] != local_signatures[callee])
                })
            });
            let mut cluster_plans = if needs_reprojection {
                project_containment_cluster(
                    root,
                    members,
                    &seeds,
                    &callable_resolutions,
                    abi,
                    &available_signatures,
                )?
            } else {
                members
                    .iter()
                    .map(|owner| {
                        preliminary_plans
                            .remove(owner)
                            .map(|plan| (owner.clone(), plan))
                            .ok_or_else(|| {
                                OwnerSignatureLexicalPlanError::new(
                                    "owner callable scope preliminary plan omits a containment member",
                                )
                            })
                    })
                    .collect::<Result<BTreeMap<_, _>, _>>()?
            };
            for owner in members {
                let plan = cluster_plans.remove(owner).ok_or_else(|| {
                    OwnerSignatureLexicalPlanError::new(
                        "owner containment reprojection omits a member plan",
                    )
                })?;
                let final_edges = plan
                    .calls()
                    .iter()
                    .filter_map(|call| {
                        let OwnerSignatureCallTarget::Owner { owner: callee } = &call.target else {
                            return None;
                        };
                        (call.valid && seeds.contains_key(callee))
                            .then(|| (owner.clone(), callee.clone()))
                    })
                    .map(|(_, callee)| callee)
                    .collect::<BTreeSet<_>>();
                let expected_edges = callees_by_caller.get(owner).cloned().unwrap_or_default();
                if final_edges != expected_edges {
                    return Err(OwnerSignatureLexicalPlanError::new(format!(
                        "owner callable scope validity changed while projecting {owner:?}"
                    )));
                }
                let updates = projected_parameter_scope_updates(
                    seeds[owner],
                    &available_signatures[owner],
                    &plan,
                )?;
                component_updates.push((owner.clone(), updates));
                component_plans.insert(owner.clone(), plan);
            }
        }
        if cyclic
            && component_updates
                .iter()
                .any(|(_, updates)| !updates.is_empty())
        {
            return Err(OwnerSignatureLexicalPlanError::new(format!(
                "recursive contextual callable component {component_roots:?} requires a finite scope-effect proof"
            )));
        }
        let expected_updates = component_updates.into_iter().collect::<BTreeMap<_, _>>();
        for (owner, updates) in &expected_updates {
            let mut signature = available_signatures[&owner].clone();
            for parameter in &mut signature.parameters {
                if let Some(scope) = updates.get(&parameter.ordinal) {
                    parameter.evaluation_scope = *scope;
                }
            }
            available_signatures.insert(owner.clone(), signature.clone());
            projected_signatures.insert(owner.clone(), signature);
        }

        // Most plans do not call a signature updated by their own component
        // and can be reused after a compact input-seal check. Invalid self or
        // intra-component calls do not form recursion edges, but they still
        // retain signature-shaped diagnostics; reproject only their containing
        // lexical cluster when that seal changed.
        for root in &component_roots {
            let members = &containment_clusters[root];
            let mut needs_freeze_reprojection = false;
            for owner in members {
                let plan = component_plans.get(owner).ok_or_else(|| {
                    OwnerSignatureLexicalPlanError::new(
                        "owner callable component omits a projected member plan",
                    )
                })?;
                let current_inputs = signature_inputs_fingerprint_v1(
                    seeds[owner],
                    callable_resolutions[owner],
                    abi,
                    &available_signatures,
                    plan.inherited_environment(),
                )?;
                needs_freeze_reprojection |= current_inputs != plan.signature_inputs_fingerprint_v1;
            }

            let mut frozen_plans = if needs_freeze_reprojection {
                project_containment_cluster(
                    root,
                    members,
                    &seeds,
                    &callable_resolutions,
                    abi,
                    &available_signatures,
                )?
            } else {
                members
                    .iter()
                    .map(|owner| {
                        component_plans
                            .remove(owner)
                            .map(|plan| (owner.clone(), plan))
                            .ok_or_else(|| {
                                OwnerSignatureLexicalPlanError::new(
                                    "owner callable component lost a reusable member plan",
                                )
                            })
                    })
                    .collect::<Result<BTreeMap<_, _>, _>>()?
            };
            for owner in members {
                let plan = frozen_plans.remove(owner).ok_or_else(|| {
                    OwnerSignatureLexicalPlanError::new(
                        "owner final containment projection omits a member plan",
                    )
                })?;
                if needs_freeze_reprojection {
                    let final_edges = plan
                        .calls()
                        .iter()
                        .filter_map(|call| {
                            let OwnerSignatureCallTarget::Owner { owner: callee } = &call.target
                            else {
                                return None;
                            };
                            (call.valid && seeds.contains_key(callee)).then(|| callee.clone())
                        })
                        .collect::<BTreeSet<_>>();
                    let expected_edges = callees_by_caller.get(owner).cloned().unwrap_or_default();
                    if final_edges != expected_edges
                        || projected_parameter_scope_updates(
                            seeds[owner],
                            &available_signatures[owner],
                            &plan,
                        )? != expected_updates[owner]
                    {
                        return Err(OwnerSignatureLexicalPlanError::new(format!(
                            "owner callable scope changed while freezing {owner:?}"
                        )));
                    }
                }
                component_plans.remove(owner);
                plans.insert(owner.clone(), plan);
            }
        }
        if !component_plans.is_empty() {
            return Err(OwnerSignatureLexicalPlanError::new(
                "owner callable component retained an unconsumed member plan",
            ));
        }
    }
    if plans.len() != seeds.len() || projected_signatures.len() != seeds.len() {
        return Err(OwnerSignatureLexicalPlanError::new(
            "owner callable scope graph did not project every owner",
        ));
    }
    Ok(OwnerSignatureLexicalScopeProjection {
        signatures: projected_signatures,
        plans,
    })
}
