use crate::{
    AuthoritativeCallableSignature, AuthoritativeParameter, BuiltinSignatureRegistry,
    ContextualBuiltinKind, RenderContractRegistry, SourcePayloadPathLookup, TypecheckSyntaxProgram,
    checked_intrinsic_v1, function_param_requirements, host_effect_signature, host_port_table,
    merge_external_function_param_requirements, scene_root, session_info_intrinsic_type,
    source_payload_shape_table, syntax_source_sites,
};
use boon_checked::{
    CheckedCallContextKind, CheckedCallableKind, CheckedEffectSummary,
    CheckedExternalDeclarationIdentityV1, CheckedIntrinsicV1, CheckedParameterKind,
    CheckedParameterRequirement, ExternalTypeEnvironment, FlowType, ProgramRole, Type,
};
use boon_parser::ProjectSyntaxSnapshot;
use boon_syntax::StableCheckOwnerKey;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt;

const OWNER_ABI_ENVIRONMENT_DOMAIN_V1: &[u8] = b"boon.owner-abi-environment.v1\0";
const OWNER_CALLABLE_ABI_ENVIRONMENT_DOMAIN_V1: &[u8] = b"boon.owner-callable-abi-environment.v1\0";
const OWNER_CALLABLE_ABI_LOOKUP_DOMAIN_V1: &[u8] = b"boon.owner-callable-abi-lookup.v1\0";
const OWNER_VALUE_ABI_LOOKUP_DOMAIN_V1: &[u8] = b"boon.owner-value-abi-lookup.v1\0";
const OWNER_INFERENCE_ABI_ENVIRONMENT_DOMAIN_V2: &[u8] =
    b"boon.owner-inference-abi-environment.v2\0";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OwnerAbiEnvironmentError {
    message: String,
}

impl OwnerAbiEnvironmentError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for OwnerAbiEnvironmentError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for OwnerAbiEnvironmentError {}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OwnerAbiRenderRoot {
    Document,
    Scene,
}

impl OwnerAbiRenderRoot {
    const fn name(self) -> &'static str {
        match self {
            Self::Document => "document",
            Self::Scene => "scene",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct OwnerAbiPolicy {
    pub allow_unresolved_external: bool,
    pub require_resolved_external_identities: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OwnerAbiEvaluationScope {
    Parent,
    Output { parameter_ordinal: u32 },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OwnerAbiParameterContract {
    pub name: String,
    pub kind: CheckedParameterKind,
    pub ordinal: u32,
    pub flow_type: FlowType,
    pub requirement: CheckedParameterRequirement,
    pub evaluation_scope: OwnerAbiEvaluationScope,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OwnerAbiCallContextContract {
    pub name: String,
    pub kind: CheckedCallContextKind,
    pub provider_parameter_ordinal: u32,
    pub flow_type: FlowType,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OwnerAbiContextualOperation {
    Map {
        list: u32,
        row: u32,
        body: u32,
    },
    Filter {
        list: u32,
        row: u32,
        predicate: u32,
    },
    Retain {
        list: u32,
        row: u32,
        predicate: u32,
    },
    Remove {
        list: u32,
        row: u32,
        predicate: u32,
    },
    Every {
        list: u32,
        row: u32,
        predicate: u32,
    },
    Any {
        list: u32,
        row: u32,
        predicate: u32,
    },
    Find {
        list: u32,
        row: u32,
        predicate: u32,
    },
    SortBy {
        list: u32,
        row: u32,
        key: u32,
        direction: u32,
    },
    ThenBy {
        list: u32,
        row: u32,
        key: u32,
        direction: u32,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OwnerAbiCallableContract {
    pub name: String,
    pub kind: CheckedCallableKind,
    pub intrinsic: Option<CheckedIntrinsicV1>,
    pub external_identity: Option<CheckedExternalDeclarationIdentityV1>,
    pub parameters: Box<[OwnerAbiParameterContract]>,
    pub contexts: Box<[OwnerAbiCallContextContract]>,
    pub result: FlowType,
    pub role: ProgramRole,
    pub effect: CheckedEffectSummary,
    pub contextual_operation: Option<OwnerAbiContextualOperation>,
}

/// Parameter surface consumed by owner interface and body inference.
///
/// This is intentionally separate from the construction ABI so future link or
/// runtime metadata cannot silently widen an inference dependency.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OwnerInferenceParameterContract {
    pub name: String,
    pub kind: CheckedParameterKind,
    pub ordinal: u32,
    pub flow_type: FlowType,
    pub requirement: CheckedParameterRequirement,
    pub evaluation_scope: OwnerAbiEvaluationScope,
}

impl From<&OwnerAbiParameterContract> for OwnerInferenceParameterContract {
    fn from(contract: &OwnerAbiParameterContract) -> Self {
        Self {
            name: contract.name.clone(),
            kind: contract.kind,
            ordinal: contract.ordinal,
            flow_type: contract.flow_type.clone(),
            requirement: contract.requirement.clone(),
            evaluation_scope: contract.evaluation_scope,
        }
    }
}

/// Minimal authoritative callable contract consumed by type inference.
///
/// Intrinsic lowering, external identity, program role, callable-context rows,
/// and contextual-operation metadata belong to checked-shard construction and
/// linking. Changes to those fields must not reopen an otherwise unchanged
/// owner inference cone.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OwnerInferenceCallableContract {
    pub name: String,
    pub kind: CheckedCallableKind,
    pub parameters: Box<[OwnerInferenceParameterContract]>,
    pub result: FlowType,
    pub effect: CheckedEffectSummary,
}

impl From<&OwnerAbiCallableContract> for OwnerInferenceCallableContract {
    fn from(contract: &OwnerAbiCallableContract) -> Self {
        Self {
            name: contract.name.clone(),
            kind: contract.kind,
            parameters: contract
                .parameters
                .iter()
                .map(OwnerInferenceParameterContract::from)
                .collect::<Vec<_>>()
                .into_boxed_slice(),
            result: contract.result.clone(),
            effect: contract.effect,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OwnerAbiValueContract {
    pub canonical_path: String,
    pub flow_type: FlowType,
    pub external_identity: Option<CheckedExternalDeclarationIdentityV1>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OwnerAbiSourcePayloadContract {
    pub canonical_path: String,
    pub payload_type: Type,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OwnerAbiNamedTypeRequirement {
    pub name: String,
    pub ty: Type,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OwnerAbiLocalFunctionRequirement {
    pub function: String,
    pub parameters: Box<[OwnerAbiNamedTypeRequirement]>,
}

/// Frozen authoritative input shared by interface and owner-body requests.
///
/// This is the only owner path representation of builtin, render, host,
/// distributed/external, source-payload, and project policy contracts. It is
/// span-free, canonically ordered, and fingerprints complete contracts rather
/// than merely the authoritative names an owner happens to mention.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OwnerAbiEnvironment {
    pub role: ProgramRole,
    pub active_render_root: OwnerAbiRenderRoot,
    pub policy: OwnerAbiPolicy,
    pub callables: Box<[OwnerAbiCallableContract]>,
    pub values: Box<[OwnerAbiValueContract]>,
    pub source_payloads: Box<[OwnerAbiSourcePayloadContract]>,
    pub local_function_requirements: Box<[OwnerAbiLocalFunctionRequirement]>,
    fingerprint_v1: [u8; 32],
}

/// Backdatable projection consumed by call resolution and inference.
///
/// Project-local parameter requirements, values, and source payload shapes do
/// not invalidate call consumers until those consumers explicitly request the
/// corresponding ABI projection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OwnerCallableAbiEnvironment {
    pub role: ProgramRole,
    pub active_render_root: OwnerAbiRenderRoot,
    pub policy: OwnerAbiPolicy,
    pub callables: Box<[OwnerAbiCallableContract]>,
    fingerprint_v1: [u8; 32],
}

/// Exact authoritative lookup state for one canonical callable name.
///
/// Missing and conflicting providers are values rather than absent dependency
/// edges, so adding or removing one contract invalidates precisely the owners
/// that asked for that name.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum OwnerCallableAbiLookupOutcome {
    Found {
        contract: OwnerInferenceCallableContract,
    },
    Missing,
    Conflict {
        contracts: Box<[OwnerInferenceCallableContract]>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OwnerCallableAbiLookup {
    canonical_name: String,
    outcome: OwnerCallableAbiLookupOutcome,
    #[serde(skip)]
    fingerprint_v1: [u8; 32],
}

impl OwnerCallableAbiLookup {
    fn new(
        canonical_name: String,
        outcome: OwnerCallableAbiLookupOutcome,
    ) -> Result<Self, OwnerAbiEnvironmentError> {
        if canonical_name.is_empty() {
            return Err(OwnerAbiEnvironmentError::new(
                "owner callable ABI lookup name is empty",
            ));
        }
        let contracts = match &outcome {
            OwnerCallableAbiLookupOutcome::Found { contract } => std::slice::from_ref(contract),
            OwnerCallableAbiLookupOutcome::Missing => &[],
            OwnerCallableAbiLookupOutcome::Conflict { contracts } => {
                if contracts.len() < 2 {
                    return Err(OwnerAbiEnvironmentError::new(format!(
                        "owner callable ABI conflict `{canonical_name}` has fewer than two contracts"
                    )));
                }
                contracts
            }
        };
        if contracts
            .iter()
            .any(|contract| contract.name != canonical_name)
        {
            return Err(OwnerAbiEnvironmentError::new(format!(
                "owner callable ABI lookup `{canonical_name}` contains a differently named contract"
            )));
        }
        let fingerprint_v1 = boon_contract::canonical_serde_hash_v1(
            OWNER_CALLABLE_ABI_LOOKUP_DOMAIN_V1,
            &(&canonical_name, &outcome),
        )
        .map_err(|error| {
            OwnerAbiEnvironmentError::new(format!(
                "cannot fingerprint owner callable ABI lookup `{canonical_name}`: {error}"
            ))
        })?;
        Ok(Self {
            canonical_name,
            outcome,
            fingerprint_v1,
        })
    }

    pub fn found(
        canonical_name: impl Into<String>,
        contract: OwnerInferenceCallableContract,
    ) -> Result<Self, OwnerAbiEnvironmentError> {
        Self::new(
            canonical_name.into(),
            OwnerCallableAbiLookupOutcome::Found { contract },
        )
    }

    pub fn missing(canonical_name: impl Into<String>) -> Result<Self, OwnerAbiEnvironmentError> {
        Self::new(
            canonical_name.into(),
            OwnerCallableAbiLookupOutcome::Missing,
        )
    }

    pub fn conflict(
        canonical_name: impl Into<String>,
        contracts: impl IntoIterator<Item = OwnerInferenceCallableContract>,
    ) -> Result<Self, OwnerAbiEnvironmentError> {
        Self::new(
            canonical_name.into(),
            OwnerCallableAbiLookupOutcome::Conflict {
                contracts: contracts.into_iter().collect::<Vec<_>>().into_boxed_slice(),
            },
        )
    }

    pub fn canonical_name(&self) -> &str {
        &self.canonical_name
    }

    pub const fn outcome(&self) -> &OwnerCallableAbiLookupOutcome {
        &self.outcome
    }

    pub fn contract(&self) -> Option<&OwnerInferenceCallableContract> {
        match &self.outcome {
            OwnerCallableAbiLookupOutcome::Found { contract } => Some(contract),
            OwnerCallableAbiLookupOutcome::Missing
            | OwnerCallableAbiLookupOutcome::Conflict { .. } => None,
        }
    }

    pub const fn fingerprint_v1(&self) -> [u8; 32] {
        self.fingerprint_v1
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "reason", rename_all = "snake_case")]
pub enum OwnerValueAbiForbiddenReason {
    NonStoreRoot {
        producer: ProgramRole,
    },
    SameRole {
        role: ProgramRole,
    },
    DependencyDirection {
        consumer: ProgramRole,
        producer: ProgramRole,
    },
}

/// Exact inference result for one role-qualified external value path.
///
/// `Missing` retains the active provisional policy so changing that policy
/// invalidates only owners which actually depend on an absent external value.
/// Source-bound declaration identity is deliberately absent; it belongs to
/// checked-shard construction and linking rather than type inference.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum OwnerValueAbiLookupOutcome {
    Found {
        flow_type: FlowType,
    },
    Missing {
        allow_unresolved: bool,
    },
    Forbidden {
        reason: OwnerValueAbiForbiddenReason,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OwnerValueAbiLookup {
    canonical_path: String,
    outcome: OwnerValueAbiLookupOutcome,
    #[serde(skip)]
    fingerprint_v1: [u8; 32],
}

impl OwnerValueAbiLookup {
    fn new(
        canonical_path: String,
        outcome: OwnerValueAbiLookupOutcome,
    ) -> Result<Self, OwnerAbiEnvironmentError> {
        if canonical_path.is_empty() {
            return Err(OwnerAbiEnvironmentError::new(
                "owner value ABI lookup path is empty",
            ));
        }
        let fingerprint_v1 = boon_contract::canonical_serde_hash_v1(
            OWNER_VALUE_ABI_LOOKUP_DOMAIN_V1,
            &(&canonical_path, &outcome),
        )
        .map_err(|error| {
            OwnerAbiEnvironmentError::new(format!(
                "cannot fingerprint owner value ABI lookup `{canonical_path}`: {error}"
            ))
        })?;
        Ok(Self {
            canonical_path,
            outcome,
            fingerprint_v1,
        })
    }

    pub fn canonical_path(&self) -> &str {
        &self.canonical_path
    }

    pub const fn outcome(&self) -> &OwnerValueAbiLookupOutcome {
        &self.outcome
    }

    pub fn flow_type(&self) -> Option<&FlowType> {
        match &self.outcome {
            OwnerValueAbiLookupOutcome::Found { flow_type } => Some(flow_type),
            OwnerValueAbiLookupOutcome::Missing { .. }
            | OwnerValueAbiLookupOutcome::Forbidden { .. } => None,
        }
    }

    pub const fn fingerprint_v1(&self) -> [u8; 32] {
        self.fingerprint_v1
    }
}

/// Exact callable ABI surface consumed by one owner or one interface SCC.
///
/// This value contains only requested names, including explicit missing or
/// conflict outcomes. Its fingerprint therefore does not change when an
/// unrelated builtin, field projection, host function, or external callable
/// is added elsewhere in the project.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OwnerInferenceAbiEnvironment {
    subjects: Box<[StableCheckOwnerKey]>,
    lookups: Box<[OwnerCallableAbiLookup]>,
    value_lookups: Box<[OwnerValueAbiLookup]>,
    #[serde(skip)]
    fingerprint_v1: [u8; 32],
}

impl OwnerInferenceAbiEnvironment {
    pub fn from_lookups(
        subjects: impl IntoIterator<Item = StableCheckOwnerKey>,
        lookups: impl IntoIterator<Item = OwnerCallableAbiLookup>,
    ) -> Result<Self, OwnerAbiEnvironmentError> {
        Self::from_lookup_sets(subjects, lookups, [])
    }

    pub fn from_lookup_sets(
        subjects: impl IntoIterator<Item = StableCheckOwnerKey>,
        lookups: impl IntoIterator<Item = OwnerCallableAbiLookup>,
        value_lookups: impl IntoIterator<Item = OwnerValueAbiLookup>,
    ) -> Result<Self, OwnerAbiEnvironmentError> {
        let mut subjects = subjects.into_iter().collect::<Vec<_>>();
        subjects.sort();
        subjects.dedup();
        if subjects.is_empty() {
            return Err(OwnerAbiEnvironmentError::new(
                "owner inference ABI environment has no subjects",
            ));
        }

        let mut by_name = BTreeMap::new();
        for lookup in lookups {
            match by_name.entry(lookup.canonical_name.clone()) {
                std::collections::btree_map::Entry::Vacant(entry) => {
                    entry.insert(lookup);
                }
                std::collections::btree_map::Entry::Occupied(entry) if entry.get() == &lookup => {}
                std::collections::btree_map::Entry::Occupied(entry) => {
                    return Err(OwnerAbiEnvironmentError::new(format!(
                        "owner inference ABI environment has inconsistent duplicate lookup `{}`",
                        entry.key()
                    )));
                }
            }
        }
        let lookups = by_name.into_values().collect::<Vec<_>>();
        let mut values_by_path = BTreeMap::new();
        for lookup in value_lookups {
            match values_by_path.entry(lookup.canonical_path.clone()) {
                std::collections::btree_map::Entry::Vacant(entry) => {
                    entry.insert(lookup);
                }
                std::collections::btree_map::Entry::Occupied(entry) if entry.get() == &lookup => {}
                std::collections::btree_map::Entry::Occupied(entry) => {
                    return Err(OwnerAbiEnvironmentError::new(format!(
                        "owner inference ABI environment has inconsistent duplicate value lookup `{}`",
                        entry.key()
                    )));
                }
            }
        }
        let value_lookups = values_by_path.into_values().collect::<Vec<_>>();
        let fingerprint_v1 = boon_contract::canonical_serde_hash_v1(
            OWNER_INFERENCE_ABI_ENVIRONMENT_DOMAIN_V2,
            &(&subjects, &lookups, &value_lookups),
        )
        .map_err(|error| {
            OwnerAbiEnvironmentError::new(format!(
                "cannot fingerprint owner inference ABI environment: {error}"
            ))
        })?;
        Ok(Self {
            subjects: subjects.into_boxed_slice(),
            lookups: lookups.into_boxed_slice(),
            value_lookups: value_lookups.into_boxed_slice(),
            fingerprint_v1,
        })
    }

    pub fn merge<'a>(
        environments: impl IntoIterator<Item = &'a Self>,
    ) -> Result<Self, OwnerAbiEnvironmentError> {
        let environments = environments.into_iter().collect::<Vec<_>>();
        Self::from_lookup_sets(
            environments
                .iter()
                .flat_map(|environment| environment.subjects.iter().cloned()),
            environments
                .iter()
                .flat_map(|environment| environment.lookups.iter().cloned()),
            environments
                .iter()
                .flat_map(|environment| environment.value_lookups.iter().cloned()),
        )
    }

    pub fn subjects(&self) -> &[StableCheckOwnerKey] {
        &self.subjects
    }

    pub fn lookups(&self) -> &[OwnerCallableAbiLookup] {
        &self.lookups
    }

    pub fn lookup(&self, canonical_name: &str) -> Option<&OwnerCallableAbiLookup> {
        self.lookups
            .binary_search_by(|lookup| lookup.canonical_name.as_str().cmp(canonical_name))
            .ok()
            .and_then(|index| self.lookups.get(index))
    }

    pub fn value_lookups(&self) -> &[OwnerValueAbiLookup] {
        &self.value_lookups
    }

    pub fn value_lookup(&self, canonical_path: &str) -> Option<&OwnerValueAbiLookup> {
        self.value_lookups
            .binary_search_by(|lookup| lookup.canonical_path.as_str().cmp(canonical_path))
            .ok()
            .and_then(|index| self.value_lookups.get(index))
    }

    pub fn callable(&self, canonical_name: &str) -> Option<&OwnerInferenceCallableContract> {
        self.lookup(canonical_name)
            .and_then(OwnerCallableAbiLookup::contract)
    }

    pub const fn fingerprint_v1(&self) -> [u8; 32] {
        self.fingerprint_v1
    }
}

impl OwnerCallableAbiEnvironment {
    pub const fn fingerprint_v1(&self) -> [u8; 32] {
        self.fingerprint_v1
    }

    pub fn callable(&self, name: &str) -> Option<&OwnerAbiCallableContract> {
        self.callables
            .binary_search_by(|contract| contract.name.as_str().cmp(name))
            .ok()
            .and_then(|index| self.callables.get(index))
    }

    pub fn lookup(
        &self,
        canonical_name: &str,
    ) -> Result<OwnerCallableAbiLookup, OwnerAbiEnvironmentError> {
        self.callable(canonical_name).cloned().map_or_else(
            || OwnerCallableAbiLookup::missing(canonical_name),
            |contract| {
                OwnerCallableAbiLookup::found(
                    canonical_name,
                    OwnerInferenceCallableContract::from(&contract),
                )
            },
        )
    }

    pub fn inference_environment(
        &self,
        subjects: impl IntoIterator<Item = StableCheckOwnerKey>,
        canonical_names: impl IntoIterator<Item = String>,
    ) -> Result<OwnerInferenceAbiEnvironment, OwnerAbiEnvironmentError> {
        OwnerInferenceAbiEnvironment::from_lookups(
            subjects,
            canonical_names
                .into_iter()
                .collect::<BTreeSet<_>>()
                .into_iter()
                .map(|name| self.lookup(&name))
                .collect::<Result<Vec<_>, _>>()?,
        )
    }
}

impl OwnerAbiEnvironment {
    pub const fn fingerprint_v1(&self) -> [u8; 32] {
        self.fingerprint_v1
    }

    pub fn callable(&self, name: &str) -> Option<&OwnerAbiCallableContract> {
        self.callables
            .binary_search_by(|contract| contract.name.as_str().cmp(name))
            .ok()
            .and_then(|index| self.callables.get(index))
    }

    pub fn value(&self, canonical_path: &str) -> Option<&OwnerAbiValueContract> {
        self.values
            .binary_search_by(|contract| contract.canonical_path.as_str().cmp(canonical_path))
            .ok()
            .and_then(|index| self.values.get(index))
    }

    pub fn value_lookup(
        &self,
        canonical_path: &str,
    ) -> Result<OwnerValueAbiLookup, OwnerAbiEnvironmentError> {
        let (namespace, suffix) = canonical_path.split_once('/').ok_or_else(|| {
            OwnerAbiEnvironmentError::new(format!(
                "owner value ABI lookup `{canonical_path}` is not role-qualified"
            ))
        })?;
        let producer = match boon_syntax::program_role_root(namespace) {
            Some(boon_syntax::ProgramRoleRoot::Client) => ProgramRole::Client,
            Some(boon_syntax::ProgramRoleRoot::Session) => ProgramRole::Session,
            Some(boon_syntax::ProgramRoleRoot::Server) => ProgramRole::Server,
            None => {
                return Err(OwnerAbiEnvironmentError::new(format!(
                    "owner value ABI lookup `{canonical_path}` has no role namespace"
                )));
            }
        };
        let outcome = if suffix.split('.').next() != Some("store") {
            OwnerValueAbiLookupOutcome::Forbidden {
                reason: OwnerValueAbiForbiddenReason::NonStoreRoot { producer },
            }
        } else if self.role == producer {
            OwnerValueAbiLookupOutcome::Forbidden {
                reason: OwnerValueAbiForbiddenReason::SameRole { role: self.role },
            }
        } else if !self.role.can_depend_on(producer) {
            OwnerValueAbiLookupOutcome::Forbidden {
                reason: OwnerValueAbiForbiddenReason::DependencyDirection {
                    consumer: self.role,
                    producer,
                },
            }
        } else if let Some(contract) = self.value(canonical_path) {
            OwnerValueAbiLookupOutcome::Found {
                flow_type: contract.flow_type.clone(),
            }
        } else {
            OwnerValueAbiLookupOutcome::Missing {
                allow_unresolved: self.policy.allow_unresolved_external,
            }
        };
        OwnerValueAbiLookup::new(canonical_path.to_owned(), outcome)
    }

    pub fn source_payload(&self, canonical_path: &str) -> Option<&OwnerAbiSourcePayloadContract> {
        self.source_payloads
            .binary_search_by(|contract| contract.canonical_path.as_str().cmp(canonical_path))
            .ok()
            .and_then(|index| self.source_payloads.get(index))
    }

    pub fn callable_environment(
        &self,
    ) -> Result<OwnerCallableAbiEnvironment, OwnerAbiEnvironmentError> {
        let fingerprint_v1 = boon_contract::canonical_serde_hash_v1(
            OWNER_CALLABLE_ABI_ENVIRONMENT_DOMAIN_V1,
            &(
                self.role,
                self.active_render_root,
                self.policy,
                &self.callables,
            ),
        )
        .map_err(|error| {
            OwnerAbiEnvironmentError::new(format!(
                "cannot fingerprint owner callable ABI environment: {error}"
            ))
        })?;
        Ok(OwnerCallableAbiEnvironment {
            role: self.role,
            active_render_root: self.active_render_root,
            policy: self.policy,
            callables: self.callables.clone(),
            fingerprint_v1,
        })
    }

    pub fn inference_environment(
        &self,
        subjects: impl IntoIterator<Item = StableCheckOwnerKey>,
        canonical_names: impl IntoIterator<Item = String>,
    ) -> Result<OwnerInferenceAbiEnvironment, OwnerAbiEnvironmentError> {
        self.exact_inference_environment(subjects, canonical_names, [])
    }

    pub fn exact_inference_environment(
        &self,
        subjects: impl IntoIterator<Item = StableCheckOwnerKey>,
        canonical_names: impl IntoIterator<Item = String>,
        canonical_value_paths: impl IntoIterator<Item = String>,
    ) -> Result<OwnerInferenceAbiEnvironment, OwnerAbiEnvironmentError> {
        let callable_provider = self.callable_environment()?;
        OwnerInferenceAbiEnvironment::from_lookup_sets(
            subjects,
            canonical_names
                .into_iter()
                .collect::<BTreeSet<_>>()
                .into_iter()
                .map(|name| callable_provider.lookup(&name))
                .collect::<Result<Vec<_>, _>>()?,
            canonical_value_paths
                .into_iter()
                .collect::<BTreeSet<_>>()
                .into_iter()
                .map(|path| self.value_lookup(&path))
                .collect::<Result<Vec<_>, _>>()?,
        )
    }
}

fn checked_u32(value: usize, context: &str) -> Result<u32, OwnerAbiEnvironmentError> {
    u32::try_from(value)
        .map_err(|_| OwnerAbiEnvironmentError::new(format!("{context} exceeds u32")))
}

fn parameter_ordinal(
    parameters: &[OwnerAbiParameterContract],
    name: &str,
    callable: &str,
) -> Result<u32, OwnerAbiEnvironmentError> {
    parameters
        .iter()
        .find(|parameter| parameter.name == name)
        .map(|parameter| parameter.ordinal)
        .ok_or_else(|| {
            OwnerAbiEnvironmentError::new(format!(
                "authoritative callable `{callable}` is missing `{name}`"
            ))
        })
}

fn contextual_operation(
    callable: &str,
    kind: ContextualBuiltinKind,
    parameters: &[OwnerAbiParameterContract],
) -> Result<OwnerAbiContextualOperation, OwnerAbiEnvironmentError> {
    let list = parameter_ordinal(parameters, "list", callable)?;
    let row = parameter_ordinal(parameters, "item", callable)?;
    Ok(match kind {
        ContextualBuiltinKind::Map => OwnerAbiContextualOperation::Map {
            list,
            row,
            body: parameter_ordinal(parameters, "new", callable)?,
        },
        ContextualBuiltinKind::Filter => OwnerAbiContextualOperation::Filter {
            list,
            row,
            predicate: parameter_ordinal(parameters, "if", callable)?,
        },
        ContextualBuiltinKind::Retain => OwnerAbiContextualOperation::Retain {
            list,
            row,
            predicate: parameter_ordinal(parameters, "if", callable)?,
        },
        ContextualBuiltinKind::Remove => OwnerAbiContextualOperation::Remove {
            list,
            row,
            predicate: parameter_ordinal(parameters, "when", callable)?,
        },
        ContextualBuiltinKind::Every => OwnerAbiContextualOperation::Every {
            list,
            row,
            predicate: parameter_ordinal(parameters, "if", callable)?,
        },
        ContextualBuiltinKind::Any => OwnerAbiContextualOperation::Any {
            list,
            row,
            predicate: parameter_ordinal(parameters, "if", callable)?,
        },
        ContextualBuiltinKind::Find => OwnerAbiContextualOperation::Find {
            list,
            row,
            predicate: parameter_ordinal(parameters, "if", callable)?,
        },
        ContextualBuiltinKind::SortBy => OwnerAbiContextualOperation::SortBy {
            list,
            row,
            key: parameter_ordinal(parameters, "key", callable)?,
            direction: parameter_ordinal(parameters, "direction", callable)?,
        },
        ContextualBuiltinKind::ThenBy => OwnerAbiContextualOperation::ThenBy {
            list,
            row,
            key: parameter_ordinal(parameters, "key", callable)?,
            direction: parameter_ordinal(parameters, "direction", callable)?,
        },
    })
}

fn callable_contract(
    name: String,
    kind: CheckedCallableKind,
    signature: AuthoritativeCallableSignature,
    role: ProgramRole,
    external_identity: Option<CheckedExternalDeclarationIdentityV1>,
) -> Result<OwnerAbiCallableContract, OwnerAbiEnvironmentError> {
    let AuthoritativeCallableSignature {
        parameters,
        call_contexts,
        result,
        effect,
        contextual_builtin,
    } = signature;
    let output_ordinal = parameters
        .iter()
        .position(|parameter| parameter.kind == CheckedParameterKind::Out)
        .map(|ordinal| checked_u32(ordinal, "authoritative output parameter ordinal"))
        .transpose()?;
    let parameters = parameters
        .into_iter()
        .enumerate()
        .map(|(ordinal, parameter)| {
            let AuthoritativeParameter {
                name,
                kind,
                flow_type,
                requirement,
            } = parameter;
            let ordinal = checked_u32(ordinal, "authoritative parameter ordinal")?;
            let evaluation_scope = if kind == CheckedParameterKind::Value
                && matches!(name.as_str(), "new" | "if" | "when" | "key")
            {
                output_ordinal.map_or(OwnerAbiEvaluationScope::Parent, |parameter_ordinal| {
                    OwnerAbiEvaluationScope::Output { parameter_ordinal }
                })
            } else {
                OwnerAbiEvaluationScope::Parent
            };
            Ok(OwnerAbiParameterContract {
                name,
                kind,
                ordinal,
                flow_type,
                requirement,
                evaluation_scope,
            })
        })
        .collect::<Result<Vec<_>, OwnerAbiEnvironmentError>>()?;
    let contexts = call_contexts
        .into_iter()
        .map(|context| {
            Ok(OwnerAbiCallContextContract {
                provider_parameter_ordinal: parameter_ordinal(
                    &parameters,
                    &context.provider,
                    &name,
                )?,
                name: context.name,
                kind: context.kind,
                flow_type: context.flow_type,
            })
        })
        .collect::<Result<Vec<_>, OwnerAbiEnvironmentError>>()?;
    let contextual_operation = contextual_builtin
        .map(|kind| contextual_operation(&name, kind, &parameters))
        .transpose()?;
    Ok(OwnerAbiCallableContract {
        intrinsic: checked_intrinsic_v1(kind, &name),
        name,
        kind,
        external_identity,
        parameters: parameters.into_boxed_slice(),
        contexts: contexts.into_boxed_slice(),
        result,
        role,
        effect,
        contextual_operation,
    })
}

fn provisional_external_signatures(
    program: &TypecheckSyntaxProgram,
    external_types: &ExternalTypeEnvironment,
) -> BTreeMap<String, Vec<String>> {
    if !external_types.allow_unresolved {
        return BTreeMap::new();
    }
    let mut provisional = BTreeMap::new();
    for expression in program.expressions().iter() {
        let (function, arguments) = match &expression.kind {
            boon_syntax::AstExprKind::Call { function, args, .. }
            | boon_syntax::AstExprKind::Pipe {
                op: function, args, ..
            } => (function, args),
            _ => continue,
        };
        if crate::external_function_role(function).is_none()
            || external_types.functions.contains_key(function)
        {
            continue;
        }
        let Some(names) = arguments
            .iter()
            .map(|argument| argument.named_name().map(str::to_owned))
            .collect::<Option<Vec<_>>>()
        else {
            continue;
        };
        provisional.entry(function.clone()).or_insert(names);
    }
    provisional
}

fn field_projection_names(program: &TypecheckSyntaxProgram) -> BTreeSet<String> {
    program
        .expressions()
        .iter()
        .filter_map(|expression| match &expression.kind {
            boon_syntax::AstExprKind::Call { function, .. }
            | boon_syntax::AstExprKind::Pipe { op: function, .. } => {
                function.strip_prefix("Field/").map(str::to_owned)
            }
            _ => None,
        })
        .collect()
}

fn insert_callable(
    callables: &mut BTreeMap<String, OwnerAbiCallableContract>,
    name: &str,
    kind: CheckedCallableKind,
    signature: AuthoritativeCallableSignature,
    role: ProgramRole,
    external_identity: Option<CheckedExternalDeclarationIdentityV1>,
) -> Result<(), OwnerAbiEnvironmentError> {
    if callables.contains_key(name) {
        return Ok(());
    }
    let contract = callable_contract(name.to_owned(), kind, signature, role, external_identity)?;
    callables.insert(name.to_owned(), contract);
    Ok(())
}

pub fn project_owner_abi_environment(
    project: &ProjectSyntaxSnapshot,
    external_types: &ExternalTypeEnvironment,
) -> Result<OwnerAbiEnvironment, OwnerAbiEnvironmentError> {
    let program = TypecheckSyntaxProgram::UnitNative(project.clone());
    let active_render_root = if scene_root(&program).is_some() {
        OwnerAbiRenderRoot::Scene
    } else {
        OwnerAbiRenderRoot::Document
    };
    let render = RenderContractRegistry::default().with_active_root(active_render_root.name());
    let builtins = BuiltinSignatureRegistry::default();
    let role = external_types.current_role;
    let mut callables = BTreeMap::new();

    for (name, mut signature) in builtins.authoritative_signatures() {
        if host_effect_signature(name).is_some() {
            continue;
        }
        if boon_effect_schema::host_effect_spec(name).is_some() {
            signature.effect.invokes_host = true;
        }
        insert_callable(
            &mut callables,
            name,
            CheckedCallableKind::Builtin,
            signature,
            role,
            None,
        )?;
    }
    for name in ["Stream/pulses", "Stream/skip"] {
        if let Some(signature) = builtins.authoritative_signature(name) {
            insert_callable(
                &mut callables,
                name,
                CheckedCallableKind::Builtin,
                signature,
                role,
                None,
            )?;
        }
    }
    for name in ["SessionInfo/status", "SessionInfo/principal"] {
        let result = session_info_intrinsic_type(name).ok_or_else(|| {
            OwnerAbiEnvironmentError::new(format!("missing SessionInfo intrinsic `{name}`"))
        })?;
        insert_callable(
            &mut callables,
            name,
            CheckedCallableKind::Builtin,
            AuthoritativeCallableSignature {
                parameters: Vec::new(),
                call_contexts: Vec::new(),
                result: crate::continuous_flow_type(result),
                effect: CheckedEffectSummary::default(),
                contextual_builtin: None,
            },
            role,
            None,
        )?;
    }
    for (name, signature) in render.authoritative_signatures() {
        insert_callable(
            &mut callables,
            name,
            CheckedCallableKind::Builtin,
            signature,
            role,
            None,
        )?;
    }
    for name in boon_effect_schema::HOST_EFFECT_OPERATIONS {
        let Some(signature) = host_effect_signature(name) else {
            continue;
        };
        insert_callable(
            &mut callables,
            name,
            CheckedCallableKind::Builtin,
            AuthoritativeCallableSignature {
                parameters: signature
                    .intent_fields
                    .into_iter()
                    .map(|field| AuthoritativeParameter {
                        name: field.name,
                        kind: CheckedParameterKind::Value,
                        flow_type: crate::continuous_flow_type(field.ty),
                        requirement: field
                            .default
                            .map_or(CheckedParameterRequirement::Required, |default| {
                                CheckedParameterRequirement::Optional { default }
                            }),
                    })
                    .collect(),
                call_contexts: Vec::new(),
                result: crate::continuous_flow_type(signature.result_type),
                effect: CheckedEffectSummary {
                    invokes_host: true,
                    ..CheckedEffectSummary::default()
                },
                contextual_builtin: None,
            },
            role,
            None,
        )?;
    }
    for field in field_projection_names(&program) {
        let name = format!("Field/{field}");
        insert_callable(
            &mut callables,
            &name,
            CheckedCallableKind::Builtin,
            AuthoritativeCallableSignature {
                parameters: vec![AuthoritativeParameter {
                    name: "input".to_owned(),
                    kind: CheckedParameterKind::Value,
                    flow_type: crate::unknown_flow_type(),
                    requirement: CheckedParameterRequirement::Required,
                }],
                call_contexts: Vec::new(),
                result: crate::unknown_flow_type(),
                effect: CheckedEffectSummary::default(),
                contextual_builtin: None,
            },
            role,
            None,
        )?;
    }
    for (name, function) in &external_types.functions {
        insert_callable(
            &mut callables,
            name,
            CheckedCallableKind::External,
            AuthoritativeCallableSignature {
                parameters: function
                    .args
                    .iter()
                    .map(|argument| AuthoritativeParameter {
                        name: argument.name.clone(),
                        kind: CheckedParameterKind::Value,
                        flow_type: argument.flow_type.clone(),
                        requirement: CheckedParameterRequirement::Required,
                    })
                    .collect(),
                call_contexts: Vec::new(),
                result: function.result.clone(),
                effect: function.effect,
                contextual_builtin: None,
            },
            role,
            external_types.external_identities.get(name).copied(),
        )?;
    }
    for (name, arguments) in provisional_external_signatures(&program, external_types) {
        insert_callable(
            &mut callables,
            &name,
            CheckedCallableKind::External,
            AuthoritativeCallableSignature {
                parameters: arguments
                    .into_iter()
                    .map(|name| AuthoritativeParameter {
                        name,
                        kind: CheckedParameterKind::Value,
                        flow_type: crate::unknown_flow_type(),
                        requirement: CheckedParameterRequirement::Required,
                    })
                    .collect(),
                call_contexts: Vec::new(),
                result: crate::unknown_flow_type(),
                effect: CheckedEffectSummary::default(),
                contextual_builtin: None,
            },
            role,
            external_types.external_identities.get(&name).copied(),
        )?;
    }

    let values = external_types
        .values
        .iter()
        .map(|(canonical_path, flow_type)| OwnerAbiValueContract {
            canonical_path: canonical_path.clone(),
            flow_type: flow_type.clone(),
            external_identity: external_types
                .external_identities
                .get(canonical_path)
                .copied(),
        })
        .collect::<Vec<_>>();

    let source_sites = syntax_source_sites(&program);
    let source_paths = source_sites
        .iter()
        .map(|source| source.path.clone())
        .collect::<BTreeSet<_>>();
    let source_payload_lookup = SourcePayloadPathLookup::new(&source_paths);
    let (host_ports, _) = host_port_table(&program, &source_payload_lookup);
    let source_payloads =
        source_payload_shape_table(&program, &source_sites, &source_payload_lookup, &host_ports)
            .into_iter()
            .map(|payload| OwnerAbiSourcePayloadContract {
                canonical_path: payload.source_path,
                payload_type: payload.payload_type,
            })
            .collect::<Vec<_>>();

    let mut local_requirements = function_param_requirements(&program);
    merge_external_function_param_requirements(
        &mut local_requirements,
        &external_types.local_function_requirements,
    );
    let local_function_requirements = local_requirements
        .into_iter()
        .map(|(function, parameters)| OwnerAbiLocalFunctionRequirement {
            function,
            parameters: parameters
                .into_iter()
                .map(|(name, ty)| OwnerAbiNamedTypeRequirement { name, ty })
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        })
        .collect::<Vec<_>>();

    let callables = callables.into_values().collect::<Vec<_>>();
    let policy = OwnerAbiPolicy {
        allow_unresolved_external: external_types.allow_unresolved,
        require_resolved_external_identities: external_types.require_resolved_identities,
    };
    let fingerprint_v1 = boon_contract::canonical_serde_hash_v1(
        OWNER_ABI_ENVIRONMENT_DOMAIN_V1,
        &(
            role,
            active_render_root,
            policy,
            &callables,
            &values,
            &source_payloads,
            &local_function_requirements,
        ),
    )
    .map_err(|error| {
        OwnerAbiEnvironmentError::new(format!("cannot fingerprint owner ABI environment: {error}"))
    })?;
    Ok(OwnerAbiEnvironment {
        role,
        active_render_root,
        policy,
        callables: callables.into_boxed_slice(),
        values: values.into_boxed_slice(),
        source_payloads: source_payloads.into_boxed_slice(),
        local_function_requirements: local_function_requirements.into_boxed_slice(),
        fingerprint_v1,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use boon_parser::{parse_project_source_unit, project_unit_link_keys};
    use std::sync::Arc;

    fn project(source: &str) -> ProjectSyntaxSnapshot {
        let parsed = parse_project_source_unit("app/RUN.bn", source).unwrap();
        let source_unit_id = parsed.source_unit_id.clone();
        let link_key = project_unit_link_keys(
            "app/RUN.bn",
            [(source_unit_id.clone(), parsed.declared_functions.clone())],
        )
        .unwrap()
        .remove(&source_unit_id)
        .unwrap();
        let unit = parsed.into_unit_syntax_snapshot(link_key).unwrap();
        ProjectSyntaxSnapshot::from_unit_snapshots("app/RUN.bn", vec![Arc::new(unit)]).unwrap()
    }

    fn owner(source: &str, name: &str) -> StableCheckOwnerKey {
        let parsed = parse_project_source_unit("app/RUN.bn", source).unwrap();
        let source_unit_id = parsed.source_unit_id.clone();
        let link_key = project_unit_link_keys(
            "app/RUN.bn",
            [(source_unit_id.clone(), parsed.declared_functions.clone())],
        )
        .unwrap()
        .remove(&source_unit_id)
        .unwrap();
        parsed
            .into_unit_syntax_snapshot(link_key)
            .unwrap()
            .stable_check_owner_keys()
            .find(|owner| {
                matches!(
                    owner,
                    StableCheckOwnerKey::Item(owner)
                        if owner.item_route.segments().last().is_some_and(|segment| {
                            segment.names.as_ref() == [name]
                        })
                )
            })
            .unwrap()
    }

    #[test]
    fn abi_environment_is_canonical_complete_and_role_sensitive() {
        let project = project("document: Document/new(title: TEXT { Demo })\n");
        let client = project_owner_abi_environment(
            &project,
            &ExternalTypeEnvironment::empty(ProgramRole::Client),
        )
        .unwrap();
        let second = project_owner_abi_environment(
            &project,
            &ExternalTypeEnvironment::empty(ProgramRole::Client),
        )
        .unwrap();
        let server = project_owner_abi_environment(
            &project,
            &ExternalTypeEnvironment::empty(ProgramRole::Server),
        )
        .unwrap();

        assert_eq!(client, second);
        assert_ne!(client.fingerprint_v1(), server.fingerprint_v1());
        assert_eq!(client.active_render_root, OwnerAbiRenderRoot::Document);
        let map = client.callable("List/map").unwrap();
        assert!(matches!(
            map.contextual_operation,
            Some(OwnerAbiContextualOperation::Map { .. })
        ));
        assert!(map.parameters.iter().any(|parameter| matches!(
            parameter.evaluation_scope,
            OwnerAbiEvaluationScope::Output { .. }
        )));
        assert!(client.callable("File/read_stream").is_some());
        assert!(client.callable("Document/new").is_some());
    }

    #[test]
    fn active_render_root_changes_render_contract_identity() {
        let document = project("document: Document/new(title: TEXT { Demo })\n");
        let scene = project("scene: Scene/new()\n");
        let document = project_owner_abi_environment(
            &document,
            &ExternalTypeEnvironment::empty(ProgramRole::Client),
        )
        .unwrap();
        let scene = project_owner_abi_environment(
            &scene,
            &ExternalTypeEnvironment::empty(ProgramRole::Client),
        )
        .unwrap();
        assert_eq!(scene.active_render_root, OwnerAbiRenderRoot::Scene);
        assert_ne!(document.fingerprint_v1(), scene.fingerprint_v1());
    }

    #[test]
    fn external_contracts_and_policies_are_fingerprint_inputs() {
        let project = project("value: 1\n");
        let mut first = ExternalTypeEnvironment::empty(ProgramRole::Client);
        first.values.insert(
            "Server.value".to_owned(),
            FlowType {
                mode: boon_checked::FlowMode::Continuous,
                ty: Type::Number,
            },
        );
        let first = project_owner_abi_environment(&project, &first).unwrap();
        let mut second = ExternalTypeEnvironment::empty(ProgramRole::Client);
        second.allow_unresolved = true;
        second.values.insert(
            "Server.value".to_owned(),
            FlowType {
                mode: boon_checked::FlowMode::Continuous,
                ty: Type::Text,
            },
        );
        let second = project_owner_abi_environment(&project, &second).unwrap();
        assert_ne!(first.fingerprint_v1(), second.fingerprint_v1());
        assert_eq!(
            first.value("Server.value").unwrap().flow_type.ty,
            Type::Number
        );
        assert_eq!(
            second.value("Server.value").unwrap().flow_type.ty,
            Type::Text
        );
    }

    #[test]
    fn exact_inference_abi_ignores_unrequested_callable_additions() {
        let before = concat!(
            "FUNCTION keep(input) {\n",
            "    Number/to_text(value: input)\n",
            "}\n",
            "other: Field/known(input: [known: 1])\n",
        );
        let after = concat!(
            "FUNCTION keep(input) {\n",
            "    Number/to_text(value: input)\n",
            "}\n",
            "other: Field/unrelated(input: [unrelated: 2])\n",
        );
        let before_provider = project_owner_abi_environment(
            &project(before),
            &ExternalTypeEnvironment::empty(ProgramRole::Client),
        )
        .unwrap()
        .callable_environment()
        .unwrap();
        let after_provider = project_owner_abi_environment(
            &project(after),
            &ExternalTypeEnvironment::empty(ProgramRole::Client),
        )
        .unwrap()
        .callable_environment()
        .unwrap();
        assert_ne!(
            before_provider.fingerprint_v1(),
            after_provider.fingerprint_v1()
        );

        let subject = owner(before, "keep");
        assert_eq!(subject, owner(after, "keep"));
        let names = ["Number/to_text".to_owned(), "Missing/function".to_owned()];
        let before = before_provider
            .inference_environment([subject.clone()], names.clone())
            .unwrap();
        let after = after_provider
            .inference_environment([subject], names)
            .unwrap();

        assert_eq!(before.fingerprint_v1(), after.fingerprint_v1());
        assert!(matches!(
            before.lookup("Number/to_text").unwrap().outcome(),
            OwnerCallableAbiLookupOutcome::Found { .. }
        ));
        assert!(matches!(
            before.lookup("Missing/function").unwrap().outcome(),
            OwnerCallableAbiLookupOutcome::Missing
        ));
        assert!(before.lookup("Field/unrelated").is_none());
    }

    #[test]
    fn callable_lookup_conflict_is_an_explicit_fingerprinted_outcome() {
        let provider = project_owner_abi_environment(
            &project("value: 1\n"),
            &ExternalTypeEnvironment::empty(ProgramRole::Client),
        )
        .unwrap()
        .callable_environment()
        .unwrap();
        let contract =
            OwnerInferenceCallableContract::from(provider.callable("Number/to_text").unwrap());
        let found = OwnerCallableAbiLookup::found("Number/to_text", contract.clone()).unwrap();
        let conflict =
            OwnerCallableAbiLookup::conflict("Number/to_text", [contract.clone(), contract])
                .unwrap();

        assert!(matches!(
            conflict.outcome(),
            OwnerCallableAbiLookupOutcome::Conflict { contracts } if contracts.len() == 2
        ));
        assert_ne!(found.fingerprint_v1(), conflict.fingerprint_v1());
    }

    #[test]
    fn construction_only_callable_metadata_stays_outside_inference_fingerprints() {
        let provider = project_owner_abi_environment(
            &project("value: 1\n"),
            &ExternalTypeEnvironment::empty(ProgramRole::Client),
        )
        .unwrap()
        .callable_environment()
        .unwrap();
        let contract = provider.callable("Number/to_text").unwrap();
        let before = OwnerCallableAbiLookup::found(
            "Number/to_text",
            OwnerInferenceCallableContract::from(contract),
        )
        .unwrap();
        let mut construction_changed = contract.clone();
        construction_changed.role = if construction_changed.role == ProgramRole::Client {
            ProgramRole::Server
        } else {
            ProgramRole::Client
        };
        let after = OwnerCallableAbiLookup::found(
            "Number/to_text",
            OwnerInferenceCallableContract::from(&construction_changed),
        )
        .unwrap();

        assert_ne!(contract, &construction_changed);
        assert_eq!(before.outcome(), after.outcome());
        assert_eq!(before.fingerprint_v1(), after.fingerprint_v1());
    }

    #[test]
    fn external_value_lookup_is_exact_typed_and_access_checked() {
        let syntax = project("value: 1\n");
        let mut external = ExternalTypeEnvironment::empty(ProgramRole::Client);
        external.values.insert(
            "Session/store.count".to_owned(),
            FlowType {
                mode: boon_checked::FlowMode::Continuous,
                ty: Type::Number,
            },
        );
        let abi = project_owner_abi_environment(&syntax, &external).unwrap();
        assert!(matches!(
            abi.value_lookup("Session/store.count").unwrap().outcome(),
            OwnerValueAbiLookupOutcome::Found { flow_type }
                if flow_type.ty == Type::Number
        ));
        assert!(matches!(
            abi.value_lookup("Session/store.missing").unwrap().outcome(),
            OwnerValueAbiLookupOutcome::Missing {
                allow_unresolved: false
            }
        ));
        assert!(matches!(
            abi.value_lookup("Session/output.count").unwrap().outcome(),
            OwnerValueAbiLookupOutcome::Forbidden {
                reason: OwnerValueAbiForbiddenReason::NonStoreRoot {
                    producer: ProgramRole::Session
                }
            }
        ));
        assert!(matches!(
            abi.value_lookup("Client/store.count").unwrap().outcome(),
            OwnerValueAbiLookupOutcome::Forbidden {
                reason: OwnerValueAbiForbiddenReason::SameRole {
                    role: ProgramRole::Client
                }
            }
        ));
        assert!(matches!(
            abi.value_lookup("Server/store.count").unwrap().outcome(),
            OwnerValueAbiLookupOutcome::Forbidden {
                reason: OwnerValueAbiForbiddenReason::DependencyDirection {
                    consumer: ProgramRole::Client,
                    producer: ProgramRole::Server
                }
            }
        ));

        external.allow_unresolved = true;
        let provisional = project_owner_abi_environment(&syntax, &external).unwrap();
        let strict_missing = abi.value_lookup("Session/store.missing").unwrap();
        let provisional_missing = provisional.value_lookup("Session/store.missing").unwrap();
        assert!(matches!(
            provisional_missing.outcome(),
            OwnerValueAbiLookupOutcome::Missing {
                allow_unresolved: true
            }
        ));
        assert_ne!(
            strict_missing.fingerprint_v1(),
            provisional_missing.fingerprint_v1()
        );
    }
}
