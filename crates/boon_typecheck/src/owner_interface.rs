use crate::owner_body::{
    EvaluatedResultValue, OwnerInterfaceTransferModule, OwnerResidualAbiContract,
    OwnerResidualCallTarget, OwnerResidualDraft, OwnerResidualDraftArguments,
    OwnerResidualEvaluationWork, OwnerResidualExpressionRef, OwnerResidualInput, OwnerResidualNode,
    OwnerResidualParameterRead, evaluate_owner_result_transfer_occurrence,
    owner_interface_transfer_dependency_owners, project_owner_interface_transfer_module,
};
use crate::owner_signature_lexical::effective_narrowed_selector_read_matches;
use crate::{
    AuthoritativeCallableSignature, AuthoritativeParameter, BuiltinSignatureRegistry,
    OwnerArgumentKind, OwnerCallableLexicalSignature, OwnerCallableScopeOwnerResult,
    OwnerCollectionKind, OwnerConstraintEdgeRole, OwnerConstraintNodeKind, OwnerConstraintSeed,
    OwnerConstraintSeedError, OwnerConstraintSummary, OwnerDeclarationKind,
    OwnerEffectiveLexicalReadPlan, OwnerEffectiveLexicalTarget, OwnerInferenceAbiEnvironment,
    OwnerInheritedPatternNarrowingPlan, OwnerInterfaceScc, OwnerInterfaceSccKey,
    OwnerLexicalAccess, OwnerLexicalDeclarationTarget, OwnerParameterKind, OwnerPatternConstraint,
    OwnerReferenceKind, OwnerSignatureDeclarationTarget, OwnerSignatureLexicalPlan,
    OwnerSymbolResolution, RenderContractRegistry, host_effect_signature,
    infix_requires_number_operands, infix_returns_bool,
    project_owner_signature_lexical_scope_plans, session_info_intrinsic_type,
};
use boon_checked::{
    BytesType, CheckedEffectSummary, CheckedParameterKind, CheckedParameterRequirement, FlowMode,
    FlowType, ObjectShape, OwnerDeclarationStableKey, OwnerLexicalDeclarationCapability,
    OwnerLexicalTargetRef, Type, TypeVar, Variant, widen_structural_type,
};
use boon_data::ExactNumber;
use boon_syntax::{StableCheckOwnerKey, StableExpressionKey};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::time::Duration;
#[cfg(not(target_arch = "wasm32"))]
use std::time::Instant;
#[cfg(target_arch = "wasm32")]
use web_time::Instant;

const OWNER_INTERFACE_SCC_RESULT_DOMAIN_V8: &[u8] = b"boon.owner-interface-scc-result.v8\0";
const OWNER_INTERFACE_SCC_KEY_DOMAIN_V1: &[u8] = b"boon.owner-interface-scc-key.v1\0";
const OWNER_INTERFACE_SCC_CURRENTNESS_DOMAIN_V9: &[u8] =
    b"boon.owner-interface-scc-currentness.v9\0";
const OWNER_BODY_INTERFACE_IMPORT_DOMAIN_V6: &[u8] = b"boon.owner-body-interface-import.v6\0";

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OwnerInterfaceParameter {
    pub name: String,
    pub kind: OwnerParameterKind,
    pub ordinal: u32,
    pub flow_type: FlowType,
    pub requirement: CheckedParameterRequirement,
    pub evaluation_scope: OwnerInterfaceEvaluationScope,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OwnerInterfaceEvaluationScope {
    Parent,
    Output { parameter_ordinal: u32 },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OwnerContextInterface {
    pub flow_type: FlowType,
    pub projections: Box<[Box<[String]>]>,
}

/// Exact enclosing expression captured by one child-owner interface.
///
/// Captures are demand-shaped: the consumer publishes only the expressions
/// named by its owner-bounded syntax. The provider body remains private, while
/// the consumer body can be re-evaluated from this frozen interface surface.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OwnerInterfaceCapture {
    pub owner: StableCheckOwnerKey,
    pub expression: StableExpressionKey,
    pub flow_type: FlowType,
    pub flush_type: Option<Type>,
}

/// Demand-shaped type surface for one exact lexical declaration imported from
/// another owner in the same authored containment cluster.
///
/// The target keeps its original declaring owner and stable declaration key;
/// consumers never publish aliases relative to their own shard.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OwnerInterfaceLexicalCapture {
    pub target: OwnerLexicalTargetRef,
    /// Exact prefix-minimal terminal projections used to shape `flow_type`.
    /// `[]` means the full declaration is captured. Otherwise Object
    /// ancestors contain only demanded fields while preserving their original
    /// open/closed status; each terminal retains its complete subtype and all
    /// paths share one alpha frame.
    pub demand_paths: Box<[Box<[String]>]>,
    pub flow_type: FlowType,
}

/// Alpha-normalized public currentness surface of one authored check owner.
///
/// Source positions, body-only literal payloads, dense IDs, implementation
/// fingerprints, and result-transfer instructions are absent. Parameters,
/// exact context projections, results, captures, and effects are the complete
/// public surface. Occurrence specialization lives only in the separately
/// sealed [`OwnerInterfaceTransferModule`].
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OwnerPublicInterface {
    pub owner: StableCheckOwnerKey,
    pub declaration_kind: Option<OwnerDeclarationKind>,
    pub names: Box<[String]>,
    pub parameters: Box<[OwnerInterfaceParameter]>,
    pub result: FlowType,
    /// Control payload escaping the callable/declaration boundary before it is
    /// unioned into `result`. Kept separately so occurrence-specialized result
    /// transfers cannot accidentally discard a FLUSH alternative.
    pub result_flush_type: Option<Type>,
    pub captures: Box<[OwnerInterfaceCapture]>,
    pub lexical_captures: Box<[OwnerInterfaceLexicalCapture]>,
    pub context: Option<OwnerContextInterface>,
    pub effect: CheckedEffectSummary,
    pub type_variables: Box<[TypeVar]>,
    #[serde(skip)]
    fingerprint_v1: [u8; 32],
}

impl OwnerPublicInterface {
    pub const fn fingerprint_v1(&self) -> [u8; 32] {
        self.fingerprint_v1
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
pub struct OwnerInterfaceSolveWork {
    pub owners: u64,
    pub expressions: u64,
    pub local_constraints: u64,
    pub cross_owner_constraints: u64,
    pub solve_rounds: u64,
    pub unification_steps: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OwnerInterfaceSccOwnerBasis {
    pub owner: StableCheckOwnerKey,
    pub lexical_reads_fingerprint_v1: [u8; 32],
    pub signature_lexical_plan_fingerprint_v1: [u8; 32],
    pub seed_fingerprint_v1: [u8; 32],
    pub summary_fingerprint_v1: [u8; 32],
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OwnerInterfaceSccDependencyBasis {
    pub key: OwnerInterfaceSccKey,
    pub result_fingerprint_v1: [u8; 32],
}

/// Exact frozen inputs used for one interface-SCC solve.
///
/// This receipt is deliberately separate from the semantic result
/// fingerprint. A changed basis whose normalized public interfaces are equal
/// may therefore backdate dependents, while direct artifact consumers can
/// still reject a result paired with stale inputs.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OwnerInterfaceSccBasis {
    pub key: OwnerInterfaceSccKey,
    pub topology_fingerprint_v1: [u8; 32],
    pub owners: Box<[OwnerInterfaceSccOwnerBasis]>,
    pub dependency_results: Box<[OwnerInterfaceSccDependencyBasis]>,
    pub inference_abi_fingerprint_v1: [u8; 32],
}

/// Atomic result for one tagged interface SCC.
///
/// `work` is telemetry and is deliberately excluded from `fingerprint_v1`.
/// Backdating compares only the normalized semantic surface.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OwnerInterfaceSccResult {
    pub key: OwnerInterfaceSccKey,
    pub owners: Box<[OwnerPublicInterface]>,
    /// Size of the SCC-local alpha-normalized type-variable namespace.
    /// Consumers must instantiate this namespace instead of treating the raw
    /// `TypeVar` values from distinct SCCs as globally unique.
    pub type_variable_count: u32,
    pub work: OwnerInterfaceSolveWork,
    key_fingerprint_v1: [u8; 32],
    fingerprint_v1: [u8; 32],
}

/// Atomic two-pass projection from one converged interface solver state.
///
/// `result` freezes the public alpha prefix. `residuals` continue in the same
/// component alpha namespace but remain private drafts consumed immediately by
/// the transfer-module compiler.
#[derive(Clone)]
pub(crate) struct OwnerInterfaceSccProjection {
    pub(crate) result: Arc<OwnerInterfaceSccResult>,
    pub(crate) residuals: Box<[OwnerResidualDraft]>,
    pub(crate) residual_type_variable_count: u32,
}

impl OwnerInterfaceSccResult {
    pub const fn key_fingerprint_v1(&self) -> [u8; 32] {
        self.key_fingerprint_v1
    }

    pub const fn fingerprint_v1(&self) -> [u8; 32] {
        self.fingerprint_v1
    }

    pub fn owner(&self, owner: &StableCheckOwnerKey) -> Option<&OwnerPublicInterface> {
        self.owners
            .binary_search_by(|candidate| candidate.owner.cmp(owner))
            .ok()
            .and_then(|index| self.owners.get(index))
    }
}

/// Latest evaluator-owned pairing of exact solve inputs with a retained
/// semantic interface result.
///
/// This receipt intentionally changes when the basis changes even if the
/// normalized public interface backdates to an older semantic value.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OwnerInterfaceSccCurrentnessReceipt {
    basis: OwnerInterfaceSccBasis,
    result_fingerprint_v1: [u8; 32],
    fingerprint_v1: [u8; 32],
}

impl OwnerInterfaceSccCurrentnessReceipt {
    pub const fn basis(&self) -> &OwnerInterfaceSccBasis {
        &self.basis
    }

    pub const fn result_fingerprint_v1(&self) -> [u8; 32] {
        self.result_fingerprint_v1
    }

    pub const fn fingerprint_v1(&self) -> [u8; 32] {
        self.fingerprint_v1
    }

    fn from_current_evaluation(
        basis: OwnerInterfaceSccBasis,
        result: &OwnerInterfaceSccResult,
    ) -> Result<Self, OwnerConstraintSeedError> {
        if basis.key != result.key {
            return Err(OwnerConstraintSeedError::new(
                "interface currentness basis and semantic result name different SCCs",
            ));
        }
        let result_fingerprint_v1 = result.fingerprint_v1();
        let fingerprint_v1 = boon_contract::canonical_serde_hash_v1(
            OWNER_INTERFACE_SCC_CURRENTNESS_DOMAIN_V9,
            &(&basis, result_fingerprint_v1),
        )
        .map_err(|error| {
            OwnerConstraintSeedError::new(format!(
                "cannot fingerprint owner interface SCC currentness: {error}"
            ))
        })?;
        Ok(Self {
            basis,
            result_fingerprint_v1,
            fingerprint_v1,
        })
    }
}

/// Latest exact evaluation paired with its semantic public projection.
///
/// Evaluators fingerprint this value by `currentness`; a child projection
/// request publishes `result` by its semantic fingerprint and can therefore
/// backdate without retaining a stale basis as current proof.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OwnerInterfaceSccEvaluation {
    pub currentness: OwnerInterfaceSccCurrentnessReceipt,
    pub result: Arc<OwnerInterfaceSccResult>,
}

/// One interface component evaluated once, together with the transfer module
/// compiled from the same final semantic surface.
///
/// `transfer_iterations` counts only quiescent residual-transfer epochs. It
/// never counts a fresh reconstruction of the SCC solver.
#[derive(Clone)]
pub struct OwnerInterfaceSccComponentEvaluation {
    pub evaluation: OwnerInterfaceSccEvaluation,
    pub module: Arc<OwnerInterfaceTransferModule>,
    pub transfer_iterations: u32,
    pub transfer_work: OwnerResidualEvaluationWork,
}

struct OwnerInterfaceSccSolveOutput {
    evaluation: OwnerInterfaceSccEvaluation,
    module: Option<Arc<OwnerInterfaceTransferModule>>,
    transfer_iterations: u32,
    transfer_work: OwnerResidualEvaluationWork,
}

fn type_contains_inference_variable(ty: &Type) -> bool {
    match ty {
        Type::Var(_) => true,
        Type::Object(shape) => shape.fields.values().any(type_contains_inference_variable),
        Type::List(item) | Type::Set(item) => type_contains_inference_variable(item),
        Type::Map { key, value } => {
            type_contains_inference_variable(key) || type_contains_inference_variable(value)
        }
        Type::Function { args, result } => {
            args.iter().any(type_contains_inference_variable)
                || type_contains_inference_variable(&result.ty)
        }
        Type::VariantSet(variants) => variants.iter().any(|variant| match variant {
            Variant::Tag(_) => false,
            Variant::Tagged { fields, .. } => {
                fields.fields.values().any(type_contains_inference_variable)
            }
        }),
        Type::Union(members) => members.iter().any(type_contains_inference_variable),
        Type::Text
        | Type::Number
        | Type::Bytes(_)
        | Type::Bits { .. }
        | Type::Absent
        | Type::RenderContract
        | Type::UnresolvedShape { .. }
        | Type::Unknown => false,
    }
}

/// Push a consumer requirement only into inference holes already present in a
/// provider snapshot. Concrete provider structure is authoritative and is
/// never widened by this operation.
fn bind_provider_inference_holes(unifier: &mut TypeUnifier, provider: &Type, requirement: &Type) {
    let requirement = unifier.resolve(requirement);
    bind_provider_inference_holes_resolved(unifier, provider, &requirement);
}

fn bind_provider_inference_holes_resolved(
    unifier: &mut TypeUnifier,
    provider: &Type,
    requirement: &Type,
) {
    if matches!(requirement, Type::Unknown | Type::UnresolvedShape { .. })
        || matches!(
            requirement,
            Type::Object(shape) if shape.open && shape.fields.is_empty()
        )
    {
        return;
    }
    match (provider, requirement) {
        (Type::Var(variable), Type::Union(members)) => {
            let own = Type::Var(unifier.root_readonly(*variable));
            let residual = members
                .iter()
                .filter(|member| !unifier.same_live_type(&own, member))
                .cloned()
                .collect::<Vec<_>>();
            if !residual.is_empty() {
                unifier.bind_var(*variable, boon_checked::canonical_union_type(residual));
            }
        }
        (Type::Var(variable), requirement) => unifier.bind_var(*variable, requirement.clone()),
        (Type::Object(provider), Type::Object(requirement)) => {
            for (name, provider) in &provider.fields {
                if let Some(requirement) = requirement.fields.get(name) {
                    bind_provider_inference_holes_resolved(unifier, provider, requirement);
                }
            }
        }
        (Type::List(provider), Type::List(requirement))
        | (Type::Set(provider), Type::Set(requirement)) => {
            bind_provider_inference_holes_resolved(unifier, provider, requirement);
        }
        (
            Type::Map {
                key: provider_key,
                value: provider_value,
            },
            Type::Map {
                key: requirement_key,
                value: requirement_value,
            },
        ) => {
            bind_provider_inference_holes_resolved(unifier, provider_key, requirement_key);
            bind_provider_inference_holes_resolved(unifier, provider_value, requirement_value);
        }
        (
            Type::Function {
                args: provider_args,
                result: provider_result,
            },
            Type::Function {
                args: requirement_args,
                result: requirement_result,
            },
        ) => {
            for (provider, requirement) in provider_args.iter().zip(requirement_args) {
                bind_provider_inference_holes_resolved(unifier, provider, requirement);
            }
            bind_provider_inference_holes_resolved(
                unifier,
                &provider_result.ty,
                &requirement_result.ty,
            );
        }
        (Type::VariantSet(provider), Type::VariantSet(requirement)) => {
            for provider in provider.iter() {
                let Variant::Tagged {
                    tag: provider_tag,
                    fields: provider_fields,
                } = provider
                else {
                    continue;
                };
                let Some(Variant::Tagged {
                    fields: requirement_fields,
                    ..
                }) = requirement.iter().find(|candidate| {
                    matches!(
                        candidate,
                        Variant::Tagged { tag, .. } if tag == provider_tag
                    )
                })
                else {
                    continue;
                };
                for (name, provider) in &provider_fields.fields {
                    if let Some(requirement) = requirement_fields.fields.get(name) {
                        bind_provider_inference_holes_resolved(unifier, provider, requirement);
                    }
                }
            }
        }
        (Type::Union(provider), Type::Union(requirement))
            if provider.len() == requirement.len() =>
        {
            for (provider, requirement) in provider.iter().zip(requirement) {
                bind_provider_inference_holes_resolved(unifier, provider, requirement);
            }
        }
        _ => {}
    }
}

/// Build only the structural additions that a consumer requirement may make
/// to an open provider surface. Existing fields are traversed recursively;
/// missing fields are admitted only by an open Object at that exact path.
/// Closed provider structure and non-Object barriers remain authoritative.
fn open_provider_requirement_patch(provider: &Type, requirement: &Type) -> Option<Type> {
    match (provider, requirement) {
        (Type::Object(provider), Type::Object(requirement)) => {
            let mut fields = Vec::new();
            let mut seen = BTreeSet::new();
            for name in requirement
                .field_order
                .iter()
                .chain(requirement.fields.keys())
            {
                if !seen.insert(name.clone()) {
                    continue;
                }
                let Some(requirement_field) = requirement.fields.get(name) else {
                    continue;
                };
                let patch = match provider.fields.get(name) {
                    Some(provider_field) => {
                        open_provider_requirement_patch(provider_field, requirement_field)
                    }
                    None if provider.open => Some(requirement_field.clone()),
                    None => None,
                };
                if let Some(patch) = patch {
                    fields.push((name.clone(), patch));
                }
            }
            (!fields.is_empty())
                .then(|| Type::object(ObjectShape::from_ordered_fields(fields, false)))
        }
        (Type::List(provider), Type::List(requirement)) => {
            open_provider_requirement_patch(provider, requirement)
                .map(|item| Type::List(Type::shared(item)))
        }
        (Type::Set(provider), Type::Set(requirement)) => {
            open_provider_requirement_patch(provider, requirement)
                .map(|item| Type::Set(Type::shared(item)))
        }
        (
            Type::Map {
                key: provider_key,
                value: provider_value,
            },
            Type::Map {
                key: requirement_key,
                value: requirement_value,
            },
        ) => {
            let key = open_provider_requirement_patch(provider_key, requirement_key);
            let value = open_provider_requirement_patch(provider_value, requirement_value);
            match (key, value) {
                (None, None) => None,
                (key, value) => Some(Type::Map {
                    key: Box::new(key.unwrap_or_else(|| provider_key.as_ref().clone())),
                    value: Box::new(value.unwrap_or_else(|| provider_value.as_ref().clone())),
                }),
            }
        }
        _ => None,
    }
}

fn bind_open_provider_requirement(
    unifier: &mut TypeUnifier,
    provider: TypeVar,
    provider_surface: &Type,
    requirement: &Type,
) {
    if let Some(patch) = open_provider_requirement_patch(provider_surface, requirement) {
        unifier.bind_var(provider, patch);
    }
}

/// Project closed call-site alternatives into one instantiated formal without
/// rewriting the authoritative provider union. Alternatives retain their
/// identity; only formal inference holes receive the union of corresponding
/// structural leaves.
fn bind_call_formal_from_closed_alternatives(
    unifier: &mut TypeUnifier,
    formal: &Type,
    actual: &[Type],
) {
    if actual.is_empty() {
        return;
    }
    match formal {
        Type::Var(_) => {
            let concrete = actual
                .iter()
                .filter(|actual| !matches!(actual, Type::Absent))
                .cloned()
                .collect::<Vec<_>>();
            if !concrete.is_empty() {
                let provider = boon_checked::canonical_union_type(concrete);
                let Type::Var(variable) = formal else {
                    unreachable!("matched a non-variable call formal")
                };
                let current = unifier.resolve(formal);
                if boon_checked::type_is_recursively_closed(&provider)
                    && !boon_checked::type_is_recursively_closed(&current)
                {
                    // FreshOut/context consumers can shape a shared formal
                    // alpha before its parent-scope provider becomes exact.
                    // That open shape is a requirement, not a co-authority.
                    // Once the provider is recursively closed, preserve its
                    // genuine holes and replace only that provisional root.
                    // This formal may already own projected FreshOut/context
                    // consumers. Raw replacement would sever those slots and
                    // leave later signature-read replay observing their stale
                    // alpha even though the provider is now exact.
                    unifier.replace_derived_provider(*variable, provider);
                } else {
                    // A partial provider or a second closed provider retains
                    // ordinary equality semantics; no argument can become a
                    // traversal-order last writer.
                    bind_provider_inference_holes(unifier, formal, &provider);
                }
            }
        }
        Type::Object(formal) => {
            if !actual
                .iter()
                .all(|actual| matches!(actual, Type::Object(_)))
            {
                return;
            }
            for (name, formal) in &formal.fields {
                let fields = actual
                    .iter()
                    .filter_map(|actual| match actual {
                        Type::Object(shape) => shape.fields.get(name).cloned(),
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                if fields.len() == actual.len() {
                    bind_call_formal_from_closed_alternatives(unifier, formal, &fields);
                }
            }
        }
        Type::List(formal) => {
            let items = actual
                .iter()
                .filter_map(|actual| match actual {
                    Type::List(item) => Some(item.as_ref().clone()),
                    _ => None,
                })
                .collect::<Vec<_>>();
            if items.len() == actual.len() {
                bind_call_formal_from_closed_alternatives(unifier, formal, &items);
            }
        }
        Type::Set(formal) => {
            let items = actual
                .iter()
                .filter_map(|actual| match actual {
                    Type::Set(item) => Some(item.as_ref().clone()),
                    _ => None,
                })
                .collect::<Vec<_>>();
            if items.len() == actual.len() {
                bind_call_formal_from_closed_alternatives(unifier, formal, &items);
            }
        }
        Type::Map {
            key: formal_key,
            value: formal_value,
        } => {
            let entries = actual
                .iter()
                .filter_map(|actual| match actual {
                    Type::Map { key, value } => {
                        Some((key.as_ref().clone(), value.as_ref().clone()))
                    }
                    _ => None,
                })
                .collect::<Vec<_>>();
            if entries.len() == actual.len() {
                bind_call_formal_from_closed_alternatives(
                    unifier,
                    formal_key,
                    &entries
                        .iter()
                        .map(|(key, _)| key.clone())
                        .collect::<Vec<_>>(),
                );
                bind_call_formal_from_closed_alternatives(
                    unifier,
                    formal_value,
                    &entries
                        .iter()
                        .map(|(_, value)| value.clone())
                        .collect::<Vec<_>>(),
                );
            }
        }
        Type::Function {
            args: formal_args,
            result: formal_result,
        } => {
            let functions = actual
                .iter()
                .filter_map(|actual| match actual {
                    Type::Function { args, result } if args.len() == formal_args.len() => {
                        Some((args, result))
                    }
                    _ => None,
                })
                .collect::<Vec<_>>();
            if functions.len() == actual.len() {
                for (index, formal) in formal_args.iter().enumerate() {
                    bind_call_formal_from_closed_alternatives(
                        unifier,
                        formal,
                        &functions
                            .iter()
                            .map(|(args, _)| args[index].clone())
                            .collect::<Vec<_>>(),
                    );
                }
                bind_call_formal_from_closed_alternatives(
                    unifier,
                    &formal_result.ty,
                    &functions
                        .iter()
                        .map(|(_, result)| result.ty.clone())
                        .collect::<Vec<_>>(),
                );
            }
        }
        Type::VariantSet(_)
        | Type::Union(_)
        | Type::Text
        | Type::Number
        | Type::Bytes(_)
        | Type::Bits { .. }
        | Type::Absent
        | Type::RenderContract
        | Type::UnresolvedShape { .. }
        | Type::Unknown => {}
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct LexicalCaptureDemand {
    full: bool,
    children: BTreeMap<String, LexicalCaptureDemand>,
}

impl LexicalCaptureDemand {
    fn from_paths(paths: &[Box<[String]>]) -> Result<Self, OwnerConstraintSeedError> {
        if paths.is_empty()
            || paths.windows(2).any(|paths| paths[0] >= paths[1])
            || paths
                .iter()
                .enumerate()
                .any(|(index, path)| paths[..index].iter().any(|prefix| path.starts_with(prefix)))
        {
            return Err(OwnerConstraintSeedError::new(
                "lexical capture demand paths are empty, unsorted, duplicate, or not prefix-minimal",
            ));
        }
        let mut demand = Self::default();
        for path in paths {
            demand.insert(path);
        }
        Ok(demand)
    }

    fn insert(&mut self, path: &[String]) {
        if self.full {
            return;
        }
        let Some((field, rest)) = path.split_first() else {
            self.full = true;
            self.children.clear();
            return;
        };
        self.children.entry(field.clone()).or_default().insert(rest);
    }

    fn merge(&mut self, other: &Self) {
        if self.full || other.children.is_empty() && !other.full {
            return;
        }
        if other.full {
            self.full = true;
            self.children.clear();
            return;
        }
        for (field, demand) in &other.children {
            self.children
                .entry(field.clone())
                .or_default()
                .merge(demand);
        }
    }

    fn project_resolved(&self, ty: &Type) -> Type {
        if self.full {
            return ty.clone();
        }
        let Type::Object(shape) = ty else {
            // The first exact cut prunes only Object siblings. Every other
            // constructor is a correlation barrier and retains its complete
            // subtree.
            return ty.clone();
        };
        let fields = self
            .children
            .iter()
            .filter_map(|(name, demand)| {
                shape
                    .fields
                    .get(name)
                    .map(|ty| (name.clone(), demand.project_resolved(ty)))
            })
            .collect::<BTreeMap<_, _>>();
        let mut field_order = shape
            .field_order
            .iter()
            .filter(|name| fields.contains_key(*name))
            .cloned()
            .collect::<Vec<_>>();
        for name in fields.keys() {
            if !field_order.contains(name) {
                field_order.push(name.clone());
            }
        }
        Type::object(ObjectShape {
            // `demand_paths` is the authority for omitted siblings. Preserve
            // the provider node's own openness so a closed missing selector
            // cannot be mistaken for a schematic/open value by inherited
            // narrowing. An empty demand path always retains the full node.
            open: shape.open,
            fields,
            field_order,
        })
    }
}

#[derive(Clone, Debug)]
struct InternalLexicalCaptureProvider {
    consumer: StableCheckOwnerKey,
    target: OwnerLexicalTargetRef,
    capture: TypeVar,
    provider: TypeVar,
    demand: LexicalCaptureDemand,
}

#[derive(Default)]
struct LexicalCaptureAlphaFrame {
    variables: BTreeMap<TypeVar, TypeVar>,
    demands: BTreeMap<TypeVar, LexicalCaptureDemand>,
}

#[derive(Default)]
pub(crate) struct TypeUnifier {
    parents: Vec<u32>,
    ranks: Vec<u8>,
    bindings: Vec<Option<Type>>,
    contextual_holes: Vec<bool>,
    authoritative_providers: Vec<bool>,
    provider_projections: BTreeMap<TypeVar, Vec<AuthoritativeProviderProjection>>,
    call_input_requirements: BTreeMap<TypeVar, Vec<Type>>,
    mutation_events: Vec<TypeMutationEvent>,
    steps: u64,
    changes: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TypeMutationKind {
    Binding,
    Union,
    Authority,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct TypeMutationEvent {
    variable: TypeVar,
    kind: TypeMutationKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct TypeMutationCursor(usize);

#[derive(Clone)]
struct AuthoritativeProviderProjection {
    projection: Box<[String]>,
    consumer: TypeVar,
}

impl TypeUnifier {
    fn mutation_cursor(&self) -> TypeMutationCursor {
        TypeMutationCursor(self.mutation_events.len())
    }

    fn mutations_since(&self, cursor: TypeMutationCursor) -> &[TypeMutationEvent] {
        &self.mutation_events[cursor.0..]
    }

    fn record_mutation(&mut self, variable: TypeVar, kind: TypeMutationKind) {
        self.mutation_events
            .push(TypeMutationEvent { variable, kind });
    }

    fn publish_authority_epoch(&mut self, variable: TypeVar) {
        let variable = self.root_readonly(variable);
        self.record_mutation(variable, TypeMutationKind::Authority);
    }

    pub(crate) fn fresh(&mut self) -> TypeVar {
        let id = u32::try_from(self.parents.len()).expect("interface type-variable bound");
        self.parents.push(id);
        self.ranks.push(0);
        self.bindings.push(None);
        self.contextual_holes.push(false);
        self.authoritative_providers.push(false);
        TypeVar(id)
    }

    pub(crate) fn fresh_contextual_hole(&mut self) -> TypeVar {
        let variable = self.fresh();
        self.contextual_holes[variable.0 as usize] = true;
        variable
    }

    pub(crate) fn mark_authoritative_provider(&mut self, variable: TypeVar) {
        let variable = self.root(variable);
        self.authoritative_providers[variable.0 as usize] = true;
    }

    fn root(&mut self, variable: TypeVar) -> TypeVar {
        let index = variable.0 as usize;
        let parent = self.parents[index];
        if parent == variable.0 {
            return variable;
        }
        let root = self.root(TypeVar(parent));
        self.parents[index] = root.0;
        root
    }

    fn root_readonly(&self, mut variable: TypeVar) -> TypeVar {
        loop {
            let parent = self.parents[variable.0 as usize];
            if parent == variable.0 {
                return variable;
            }
            variable = TypeVar(parent);
        }
    }

    fn same_live_object_shape_inner(
        &self,
        left: &boon_checked::SharedObjectShape,
        right: &boon_checked::SharedObjectShape,
        active: &mut BTreeSet<TypeVar>,
    ) -> bool {
        boon_checked::SharedObjectShape::ptr_eq(left, right)
            || (left.open == right.open
                && left.field_order == right.field_order
                && left.fields.len() == right.fields.len()
                && left.fields.iter().all(|(name, left)| {
                    right
                        .fields
                        .get(name)
                        .is_some_and(|right| self.same_live_type_inner(left, right, active))
                }))
    }

    fn same_live_type(&self, left: &Type, right: &Type) -> bool {
        self.same_live_type_inner(left, right, &mut BTreeSet::new())
    }

    fn same_live_type_inner(
        &self,
        left: &Type,
        right: &Type,
        active: &mut BTreeSet<TypeVar>,
    ) -> bool {
        match (left, right) {
            (Type::Var(left), Type::Var(right)) => {
                let left = self.root_readonly(*left);
                let right = self.root_readonly(*right);
                if left == right {
                    return true;
                }
                let Some(left_binding) = self.bindings[left.0 as usize].as_ref() else {
                    return false;
                };
                let Some(right_binding) = self.bindings[right.0 as usize].as_ref() else {
                    return false;
                };
                if !active.insert(left) {
                    return false;
                }
                if !active.insert(right) {
                    active.remove(&left);
                    return false;
                }
                let same = self.same_live_type_inner(left_binding, right_binding, active);
                active.remove(&left);
                active.remove(&right);
                same
            }
            (Type::Var(variable), right) => {
                let root = self.root_readonly(*variable);
                let Some(binding) = self.bindings[root.0 as usize].as_ref() else {
                    return false;
                };
                if !active.insert(root) {
                    return false;
                }
                let same = self.same_live_type_inner(binding, right, active);
                active.remove(&root);
                same
            }
            (left, Type::Var(variable)) => {
                let root = self.root_readonly(*variable);
                let Some(binding) = self.bindings[root.0 as usize].as_ref() else {
                    return false;
                };
                if !active.insert(root) {
                    return false;
                }
                let same = self.same_live_type_inner(left, binding, active);
                active.remove(&root);
                same
            }
            (Type::Object(left), Type::Object(right)) => {
                self.same_live_object_shape_inner(left, right, active)
            }
            (Type::List(left), Type::List(right)) | (Type::Set(left), Type::Set(right)) => {
                boon_checked::SharedType::ptr_eq(left, right)
                    || self.same_live_type_inner(left, right, active)
            }
            (
                Type::Map {
                    key: left_key,
                    value: left_value,
                },
                Type::Map {
                    key: right_key,
                    value: right_value,
                },
            ) => {
                self.same_live_type_inner(left_key, right_key, active)
                    && self.same_live_type_inner(left_value, right_value, active)
            }
            (
                Type::Function {
                    args: left_args,
                    result: left_result,
                },
                Type::Function {
                    args: right_args,
                    result: right_result,
                },
            ) => {
                left_args.len() == right_args.len()
                    && left_args
                        .iter()
                        .zip(right_args)
                        .all(|(left, right)| self.same_live_type_inner(left, right, active))
                    && left_result.mode == right_result.mode
                    && self.same_live_type_inner(&left_result.ty, &right_result.ty, active)
            }
            (Type::VariantSet(left), Type::VariantSet(right)) => {
                boon_checked::SharedVariantSet::ptr_eq(left, right)
                    || (left.len() == right.len()
                        && left
                            .iter()
                            .zip(right.iter())
                            .all(|(left, right)| match (left, right) {
                                (Variant::Tag(left), Variant::Tag(right)) => left == right,
                                (
                                    Variant::Tagged {
                                        tag: left_tag,
                                        fields: left_fields,
                                    },
                                    Variant::Tagged {
                                        tag: right_tag,
                                        fields: right_fields,
                                    },
                                ) => {
                                    left_tag == right_tag
                                        && self.same_live_object_shape_inner(
                                            left_fields,
                                            right_fields,
                                            active,
                                        )
                                }
                                _ => false,
                            }))
            }
            (Type::Union(left), Type::Union(right)) => {
                left.len() == right.len()
                    && left
                        .iter()
                        .zip(right)
                        .all(|(left, right)| self.same_live_type_inner(left, right, active))
            }
            (left, right) => left == right,
        }
    }

    pub(crate) fn resolve(&mut self, ty: &Type) -> Type {
        self.resolve_with_cache(ty, &mut Vec::new())
    }

    fn resolve_with_cache(&mut self, ty: &Type, resolved_roots: &mut Vec<Option<Type>>) -> Type {
        if !type_contains_inference_variable(ty) {
            return ty.clone();
        }
        resolved_roots.resize(self.parents.len(), None);
        self.resolve_inner(ty, &mut BTreeSet::new(), resolved_roots)
    }

    #[cfg(test)]
    fn resolve_lexical_capture_surface(
        &mut self,
        provider: TypeVar,
        demand: &LexicalCaptureDemand,
        resolved_roots: &mut Vec<Option<Type>>,
    ) -> Type {
        self.resolve_lexical_capture_surface_with_demands(provider, demand, resolved_roots)
            .0
    }

    fn resolve_lexical_capture_surface_with_demands(
        &mut self,
        provider: TypeVar,
        demand: &LexicalCaptureDemand,
        resolved_roots: &mut Vec<Option<Type>>,
    ) -> (Type, BTreeMap<TypeVar, LexicalCaptureDemand>) {
        resolved_roots.resize(self.parents.len(), None);
        let mut unresolved_demands = BTreeMap::new();
        let ty = self.resolve_lexical_capture_surface_inner(
            &Type::Var(provider),
            demand,
            &mut BTreeSet::new(),
            resolved_roots,
            &mut unresolved_demands,
        );
        (ty, unresolved_demands)
    }

    fn resolve_lexical_capture_surface_inner(
        &mut self,
        ty: &Type,
        demand: &LexicalCaptureDemand,
        active: &mut BTreeSet<TypeVar>,
        resolved_roots: &mut [Option<Type>],
        unresolved_demands: &mut BTreeMap<TypeVar, LexicalCaptureDemand>,
    ) -> Type {
        if demand.full {
            let resolved = self.resolve_inner(ty, active, resolved_roots);
            let mut variables = BTreeSet::new();
            collect_type_variables(&resolved, &mut variables);
            for variable in variables {
                unresolved_demands
                    .entry(variable)
                    .or_default()
                    .merge(demand);
            }
            return resolved;
        }
        match ty {
            Type::Var(variable) => {
                let root = self.root(*variable);
                if !active.insert(root) {
                    return Type::Var(root);
                }
                let binding = self.bindings[root.0 as usize].clone();
                let projected = match binding {
                    None => {
                        unresolved_demands.entry(root).or_default().merge(demand);
                        Type::Var(root)
                    }
                    Some(binding) => self.resolve_lexical_capture_surface_inner(
                        &binding,
                        demand,
                        active,
                        resolved_roots,
                        unresolved_demands,
                    ),
                };
                active.remove(&root);
                projected
            }
            Type::Object(shape) => {
                let fields = demand
                    .children
                    .iter()
                    .filter_map(|(name, demand)| {
                        shape.fields.get(name).map(|ty| {
                            (
                                name.clone(),
                                self.resolve_lexical_capture_surface_inner(
                                    ty,
                                    demand,
                                    active,
                                    resolved_roots,
                                    unresolved_demands,
                                ),
                            )
                        })
                    })
                    .collect::<BTreeMap<_, _>>();
                let mut field_order = shape
                    .field_order
                    .iter()
                    .filter(|name| fields.contains_key(*name))
                    .cloned()
                    .collect::<Vec<_>>();
                for name in fields.keys() {
                    if !field_order.contains(name) {
                        field_order.push(name.clone());
                    }
                }
                Type::object(ObjectShape {
                    open: shape.open,
                    fields,
                    field_order,
                })
            }
            // Union/VariantSet/List/Set/Map/Function and scalar/unknown nodes
            // are full-subtree barriers in the first exact cut. This keeps
            // branch/member correlation identical to the whole capture.
            _ => {
                let resolved = self.resolve_inner(ty, active, resolved_roots);
                let mut variables = BTreeSet::new();
                collect_type_variables(&resolved, &mut variables);
                let full = LexicalCaptureDemand {
                    full: true,
                    children: BTreeMap::new(),
                };
                for variable in variables {
                    unresolved_demands.entry(variable).or_default().merge(&full);
                }
                resolved
            }
        }
    }

    fn resolve_inner(
        &mut self,
        ty: &Type,
        active: &mut BTreeSet<TypeVar>,
        resolved_roots: &mut [Option<Type>],
    ) -> Type {
        match ty {
            Type::Var(variable) => {
                let root = self.root(*variable);
                if let Some(resolved) = &resolved_roots[root.0 as usize] {
                    return resolved.clone();
                }
                if !active.insert(root) {
                    return Type::Var(root);
                }
                let binding = self.bindings[root.0 as usize].clone();
                let resolved = binding.map_or(Type::Var(root), |binding| {
                    self.resolve_inner(&binding, active, resolved_roots)
                });
                active.remove(&root);
                resolved_roots[root.0 as usize] = Some(resolved.clone());
                resolved
            }
            Type::Object(shape) => Type::object(ObjectShape {
                fields: shape
                    .fields
                    .iter()
                    .map(|(name, ty)| {
                        (name.clone(), self.resolve_inner(ty, active, resolved_roots))
                    })
                    .collect(),
                field_order: shape.field_order.clone(),
                open: shape.open,
            }),
            Type::List(item) => Type::List(Type::shared(self.resolve_inner(
                item,
                active,
                resolved_roots,
            ))),
            Type::Set(item) => Type::Set(Type::shared(self.resolve_inner(
                item,
                active,
                resolved_roots,
            ))),
            Type::Map { key, value } => Type::Map {
                key: Box::new(self.resolve_inner(key, active, resolved_roots)),
                value: Box::new(self.resolve_inner(value, active, resolved_roots)),
            },
            Type::Function { args, result } => Type::Function {
                args: args
                    .iter()
                    .map(|argument| self.resolve_inner(argument, active, resolved_roots))
                    .collect(),
                result: Box::new(FlowType {
                    mode: result.mode,
                    ty: self.resolve_inner(&result.ty, active, resolved_roots),
                }),
            },
            Type::VariantSet(variants) => Type::VariantSet(
                variants
                    .iter()
                    .map(|variant| match variant {
                        Variant::Tag(tag) => Variant::Tag(tag.clone()),
                        Variant::Tagged { tag, fields } => Variant::Tagged {
                            tag: tag.clone(),
                            fields: ObjectShape {
                                fields: fields
                                    .fields
                                    .iter()
                                    .map(|(name, ty)| {
                                        (
                                            name.clone(),
                                            self.resolve_inner(ty, active, resolved_roots),
                                        )
                                    })
                                    .collect(),
                                field_order: fields.field_order.clone(),
                                open: fields.open,
                            }
                            .into(),
                        },
                    })
                    .collect(),
            ),
            Type::Union(members) => boon_checked::canonical_union_type(
                members
                    .iter()
                    .map(|member| self.resolve_inner(member, active, resolved_roots))
                    .collect(),
            ),
            Type::Text
            | Type::Number
            | Type::Bytes(_)
            | Type::Bits { .. }
            | Type::Absent
            | Type::RenderContract
            | Type::UnresolvedShape { .. }
            | Type::Unknown => ty.clone(),
        }
    }

    /// Resolve a type for public artifact projection while allowing a closed
    /// child-owner endpoint to supersede its stale persistent result root.
    ///
    /// These authorities are deliberately read-only and never enter the live
    /// unifier. A transfer residual can close a child expression after the
    /// containing record was wired to the child's persistent result root;
    /// mutating that root during residual convergence makes the solve
    /// non-monotone. Final projection can safely follow the closed endpoint
    /// without changing any inference equation.
    fn resolve_with_projection_authorities(
        &mut self,
        ty: &Type,
        authorities: &BTreeMap<TypeVar, Type>,
    ) -> Type {
        if authorities.is_empty() || !type_contains_inference_variable(ty) {
            return self.resolve(ty);
        }
        let mut resolved_roots = vec![None; self.parents.len()];
        self.resolve_with_projection_authorities_inner(
            ty,
            authorities,
            &mut BTreeSet::new(),
            &mut resolved_roots,
        )
    }

    fn resolve_with_projection_authorities_inner(
        &mut self,
        ty: &Type,
        authorities: &BTreeMap<TypeVar, Type>,
        active: &mut BTreeSet<TypeVar>,
        resolved_roots: &mut [Option<Type>],
    ) -> Type {
        match ty {
            Type::Var(variable) => {
                let root = self.root(*variable);
                if let Some(authority) = authorities.get(&root) {
                    return authority.clone();
                }
                if let Some(resolved) = &resolved_roots[root.0 as usize] {
                    return resolved.clone();
                }
                if !active.insert(root) {
                    return Type::Var(root);
                }
                let binding = self.bindings[root.0 as usize].clone();
                let resolved = binding.map_or(Type::Var(root), |binding| {
                    self.resolve_with_projection_authorities_inner(
                        &binding,
                        authorities,
                        active,
                        resolved_roots,
                    )
                });
                active.remove(&root);
                resolved_roots[root.0 as usize] = Some(resolved.clone());
                resolved
            }
            Type::Object(shape) => Type::object(ObjectShape {
                fields: shape
                    .fields
                    .iter()
                    .map(|(name, ty)| {
                        (
                            name.clone(),
                            self.resolve_with_projection_authorities_inner(
                                ty,
                                authorities,
                                active,
                                resolved_roots,
                            ),
                        )
                    })
                    .collect(),
                field_order: shape.field_order.clone(),
                open: shape.open,
            }),
            Type::List(item) => Type::List(Type::shared(
                self.resolve_with_projection_authorities_inner(
                    item,
                    authorities,
                    active,
                    resolved_roots,
                ),
            )),
            Type::Set(item) => Type::Set(Type::shared(
                self.resolve_with_projection_authorities_inner(
                    item,
                    authorities,
                    active,
                    resolved_roots,
                ),
            )),
            Type::Map { key, value } => Type::Map {
                key: Box::new(self.resolve_with_projection_authorities_inner(
                    key,
                    authorities,
                    active,
                    resolved_roots,
                )),
                value: Box::new(self.resolve_with_projection_authorities_inner(
                    value,
                    authorities,
                    active,
                    resolved_roots,
                )),
            },
            Type::Function { args, result } => Type::Function {
                args: args
                    .iter()
                    .map(|argument| {
                        self.resolve_with_projection_authorities_inner(
                            argument,
                            authorities,
                            active,
                            resolved_roots,
                        )
                    })
                    .collect(),
                result: Box::new(FlowType {
                    mode: result.mode,
                    ty: self.resolve_with_projection_authorities_inner(
                        &result.ty,
                        authorities,
                        active,
                        resolved_roots,
                    ),
                }),
            },
            Type::VariantSet(variants) => Type::VariantSet(
                variants
                    .iter()
                    .map(|variant| match variant {
                        Variant::Tag(tag) => Variant::Tag(tag.clone()),
                        Variant::Tagged { tag, fields } => Variant::Tagged {
                            tag: tag.clone(),
                            fields: ObjectShape {
                                fields: fields
                                    .fields
                                    .iter()
                                    .map(|(name, ty)| {
                                        (
                                            name.clone(),
                                            self.resolve_with_projection_authorities_inner(
                                                ty,
                                                authorities,
                                                active,
                                                resolved_roots,
                                            ),
                                        )
                                    })
                                    .collect(),
                                field_order: fields.field_order.clone(),
                                open: fields.open,
                            }
                            .into(),
                        },
                    })
                    .collect(),
            ),
            Type::Union(members) => boon_checked::canonical_union_type(
                members
                    .iter()
                    .map(|member| {
                        self.resolve_with_projection_authorities_inner(
                            member,
                            authorities,
                            active,
                            resolved_roots,
                        )
                    })
                    .collect(),
            ),
            Type::Text
            | Type::Number
            | Type::Bytes(_)
            | Type::Bits { .. }
            | Type::Absent
            | Type::RenderContract
            | Type::UnresolvedShape { .. }
            | Type::Unknown => ty.clone(),
        }
    }

    fn occurs(&self, variable: TypeVar, ty: &Type) -> bool {
        if !type_contains_inference_variable(ty) {
            return false;
        }
        match ty {
            Type::Var(candidate) => self.root_readonly(*candidate) == variable,
            Type::Object(shape) => shape.fields.values().any(|ty| self.occurs(variable, ty)),
            Type::List(item) | Type::Set(item) => self.occurs(variable, item),
            Type::Map { key, value } => self.occurs(variable, key) || self.occurs(variable, value),
            Type::Function { args, result } => {
                args.iter().any(|ty| self.occurs(variable, ty)) || self.occurs(variable, &result.ty)
            }
            Type::VariantSet(variants) => variants.iter().any(|variant| match variant {
                Variant::Tag(_) => false,
                Variant::Tagged { fields, .. } => {
                    fields.fields.values().any(|ty| self.occurs(variable, ty))
                }
            }),
            Type::Union(members) => members.iter().any(|ty| self.occurs(variable, ty)),
            Type::Text
            | Type::Number
            | Type::Bytes(_)
            | Type::Bits { .. }
            | Type::Absent
            | Type::RenderContract
            | Type::UnresolvedShape { .. }
            | Type::Unknown => false,
        }
    }

    fn union(&mut self, left: TypeVar, right: TypeVar) {
        let mut left = self.root(left);
        let mut right = self.root(right);
        if left == right {
            return;
        }
        if self.ranks[left.0 as usize] < self.ranks[right.0 as usize] {
            std::mem::swap(&mut left, &mut right);
        }
        self.parents[right.0 as usize] = left.0;
        self.record_mutation(left, TypeMutationKind::Union);
        self.record_mutation(right, TypeMutationKind::Union);
        self.contextual_holes[left.0 as usize] |= self.contextual_holes[right.0 as usize];
        self.contextual_holes[right.0 as usize] = false;
        self.authoritative_providers[left.0 as usize] |=
            self.authoritative_providers[right.0 as usize];
        self.authoritative_providers[right.0 as usize] = false;
        if let Some(mut projections) = self.provider_projections.remove(&right) {
            self.provider_projections
                .entry(left)
                .or_default()
                .append(&mut projections);
        }
        if let Some(mut requirements) = self.call_input_requirements.remove(&right) {
            self.call_input_requirements
                .entry(left)
                .or_default()
                .append(&mut requirements);
        }
        self.changes = self.changes.saturating_add(1);
        if self.ranks[left.0 as usize] == self.ranks[right.0 as usize] {
            self.ranks[left.0 as usize] = self.ranks[left.0 as usize].saturating_add(1);
        }
        let right_binding = self.bindings[right.0 as usize].take();
        if let Some(right_binding) = right_binding {
            self.bind_var(left, right_binding);
        }
    }

    fn transparent_alias_reaches(&self, candidate: TypeVar, destination: TypeVar) -> bool {
        let mut candidate = candidate;
        let mut visited = BTreeSet::new();
        loop {
            let root = self.root_readonly(candidate);
            if root == destination {
                return true;
            }
            if !visited.insert(root) {
                return false;
            }
            let Some(Type::Var(next)) = self.bindings[root.0 as usize].as_ref() else {
                return false;
            };
            candidate = *next;
        }
    }

    fn remove_transparent_self_aliases(
        &self,
        variable: TypeVar,
        binding: Type,
    ) -> (Option<Type>, bool) {
        match binding {
            Type::Var(candidate) if self.transparent_alias_reaches(candidate, variable) => {
                (None, true)
            }
            Type::Union(members) => {
                let original_len = members.len();
                let retained = members
                    .iter()
                    .filter(|member| {
                        !matches!(
                            member,
                            Type::Var(candidate)
                                if self.transparent_alias_reaches(*candidate, variable)
                        )
                    })
                    .cloned()
                    .collect::<Vec<_>>();
                if retained.len() == original_len {
                    (Some(Type::Union(members)), false)
                } else if retained.is_empty() {
                    (None, true)
                } else {
                    (Some(boon_checked::canonical_union_type(retained)), true)
                }
            }
            binding => (Some(binding), false),
        }
    }

    pub(crate) fn bind_var(&mut self, variable: TypeVar, incoming: Type) {
        self.steps = self.steps.saturating_add(1);
        let variable = self.root(variable);
        // Replaying the same live equation does not require resolving either
        // side. The stored and incoming trees already reference the same
        // unifier roots, so later bindings remain observable through both.
        // This avoids repeatedly materializing large generic call results in
        // fixed-point rounds.
        if self.bindings[variable.0 as usize]
            .as_ref()
            .is_some_and(|current| self.same_live_type(current, &incoming))
        {
            return;
        }
        let incoming = self.resolve(&incoming);
        if let Type::Var(other) = incoming {
            self.union(variable, other);
            return;
        }
        if self.occurs(variable, &incoming) {
            return;
        }
        // Union-find can coalesce transparent flow aliases after their raw
        // union was stored, turning one or more members into direct aliases of
        // the binding's own root. `T = T | X` carries no information from the
        // self member; retain only X before merging a later exact equation.
        // Otherwise resolving the temporarily removed slot exposes T again
        // and a concrete late capture refresh can remain `T | Concrete`.
        let raw_slot = self.bindings[variable.0 as usize].take();
        let (slot, removed_self_alias) = raw_slot.map_or((None, false), |binding| {
            self.remove_transparent_self_aliases(variable, binding)
        });
        let (merged, changed) = match slot {
            None => (incoming, true),
            Some(current) if current == incoming => (current, removed_self_alias),
            Some(current) => {
                let merged = self.merge_resolved(current.clone(), incoming);
                let changed = removed_self_alias || merged != current;
                (merged, changed)
            }
        };
        self.bindings[variable.0 as usize] = Some(merged);
        if changed {
            self.record_mutation(variable, TypeMutationKind::Binding);
            self.changes = self.changes.saturating_add(1);
        }
    }

    /// Bind the result of a value-flow edge without turning that edge into an
    /// equality constraint on its producer.
    ///
    /// Branch joins, transparent wrappers, and block results are covariant:
    /// a richer alternative can widen the consumer result, but it must not add
    /// fields to a parameter returned by another alternative. The ordinary
    /// unifier remains appropriate for true type equations such as a HOLD's
    /// stored value or a numeric operand contract.
    pub(crate) fn bind_flow_result(&mut self, variable: TypeVar, incoming: Type) {
        self.steps = self.steps.saturating_add(1);
        let variable = self.root(variable);
        let incoming = self.resolve(&incoming);
        if self.occurs(variable, &incoming) {
            return;
        }
        let slot = self.bindings[variable.0 as usize].take();
        let (merged, changed) = match slot {
            None => (incoming, true),
            Some(current) if current == incoming => (current, false),
            Some(current) => {
                let merged = boon_checked::canonical_union_type(vec![current.clone(), incoming]);
                let changed = merged != current;
                (merged, changed)
            }
        };
        self.bindings[variable.0 as usize] = Some(merged);
        if changed {
            self.record_mutation(variable, TypeMutationKind::Binding);
            self.changes = self.changes.saturating_add(1);
        }
    }

    /// Publish a directional provider surface whose outer shape is authoritative.
    ///
    /// A transfer-aware call deliberately withholds its broad definition-site
    /// principal while local consumers are being solved. Those consumers can
    /// still shape the empty occurrence root (for example, a later field read
    /// creates an open Object requirement). Merging the exact transfer result
    /// with that provisional requirement would retain the open scaffold and
    /// make a closed record permanently open. Project only genuine provider
    /// holes from the frozen requirement, then replace the provisional root
    /// with the live provider result. Concrete consumer mismatches remain for
    /// checked-call validation; they do not rewrite provider authority.
    pub(crate) fn publish_authoritative_provider(&mut self, variable: TypeVar, provider: Type) {
        self.steps = self.steps.saturating_add(1);
        let variable = self.root(variable);
        self.publish_authority_epoch(variable);
        if self.bindings[variable.0 as usize]
            .as_ref()
            .is_some_and(|current| self.same_live_type(current, &provider))
        {
            return;
        }
        let requirement = if self.bindings[variable.0 as usize].is_some() {
            Some(self.resolve(&Type::Var(variable)))
        } else {
            None
        };
        if let Some(requirement) = &requirement {
            match &provider {
                Type::Union(members) => {
                    bind_call_formal_from_closed_alternatives(self, requirement, members)
                }
                provider => bind_call_formal_from_closed_alternatives(
                    self,
                    requirement,
                    std::slice::from_ref(provider),
                ),
            }
            bind_provider_inference_holes(self, &provider, requirement);
        }
        self.replace_authoritative_binding(variable, provider);
    }

    fn replace_authoritative_binding(&mut self, variable: TypeVar, provider: Type) {
        let variable = self.root(variable);
        if self.bindings[variable.0 as usize]
            .as_ref()
            .is_some_and(|current| self.same_live_type(current, &provider))
            || self.occurs(variable, &provider)
        {
            return;
        }
        self.bindings[variable.0 as usize] = Some(provider);
        self.record_mutation(variable, TypeMutationKind::Binding);
        self.changes = self.changes.saturating_add(1);
    }

    /// Replace a derived provider without interpreting its previous value as
    /// a consumer requirement. Only projections recorded by `bind_projection`
    /// receive the corresponding provider leaf. This preserves late reads
    /// without sending a HOLD/flow result backwards into generic producer
    /// holes that happened to occur in an older provider snapshot.
    pub(crate) fn replace_derived_provider(&mut self, variable: TypeVar, provider: Type) {
        self.steps = self.steps.saturating_add(1);
        let mut pending = vec![(self.root(variable), provider)];
        let mut visited = BTreeSet::new();
        while let Some((variable, provider)) = pending.pop() {
            if !visited.insert(variable) {
                continue;
            }
            self.mark_authoritative_provider(variable);
            self.replace_authoritative_binding(variable, provider.clone());
            let requirements = self
                .call_input_requirements
                .get(&variable)
                .cloned()
                .unwrap_or_default();
            let alternatives = match &provider {
                Type::Union(members) => members.as_slice(),
                provider => std::slice::from_ref(provider),
            };
            for formal in requirements {
                bind_call_formal_from_closed_alternatives(self, &formal, alternatives);
            }
            let projections = self
                .provider_projections
                .get(&variable)
                .cloned()
                .unwrap_or_default();
            for projection in projections {
                let projected = crate::type_for_nested_path(&provider, &projection.projection)
                    .unwrap_or_else(|| Type::UnresolvedShape {
                        reason: format!(
                            "authoritative provider omits projection `{}`",
                            projection.projection.join(".")
                        ),
                    });
                let consumer = self.root(projection.consumer);
                self.mark_authoritative_provider(consumer);
                pending.push((consumer, projected));
            }
        }
    }

    fn publish_derived_provider_epoch(&mut self, variable: TypeVar, provider: Type) {
        self.publish_authority_epoch(variable);
        self.replace_derived_provider(variable, provider);
    }

    fn merge_resolved(&mut self, left: Type, right: Type) -> Type {
        // A flow binding can retain a raw union whose member variables have
        // since collapsed to one structural type. Normalize only at that
        // union seam. Resolving every enclosing record/variant binding here
        // materializes large semantic DAGs during each SCC round.
        let left = if matches!(left, Type::Union(_)) {
            self.resolve(&left)
        } else {
            left
        };
        let right = if matches!(right, Type::Union(_)) {
            self.resolve(&right)
        } else {
            right
        };
        match (left, right) {
            // `Unknown` and `UnresolvedShape` are diagnostic placeholders,
            // not evidence about a generic type variable. In particular, a
            // same-component call can observe a provisional placeholder in
            // one caller before another equation closes the callee scheme.
            // Binding the scheme variable to that placeholder permanently
            // turns a generic public field into literal `Unknown`. The dense
            // call-pattern matcher already treats these pairs as no-ops; keep
            // the owner unifier's structural merge consistent with it.
            (Type::Var(variable), Type::Unknown | Type::UnresolvedShape { .. })
            | (Type::Unknown | Type::UnresolvedShape { .. }, Type::Var(variable)) => {
                Type::Var(self.root(variable))
            }
            (Type::Var(variable), right) => {
                self.bind_var(variable, right);
                Type::Var(self.root(variable))
            }
            (left, Type::Var(variable)) => {
                self.bind_var(variable, left);
                Type::Var(self.root(variable))
            }
            (Type::Object(left), Type::Object(right)) => {
                let mut fields = left.fields.clone();
                for (name, right_ty) in &right.fields {
                    let merged = fields.remove(name).map_or_else(
                        || right_ty.clone(),
                        |left_ty| self.merge_resolved(left_ty, right_ty.clone()),
                    );
                    fields.insert(name.clone(), merged);
                }
                let mut field_order = Vec::new();
                let mut seen = BTreeSet::new();
                for name in left
                    .field_order
                    .iter()
                    .chain(&right.field_order)
                    .chain(fields.keys())
                {
                    if fields.contains_key(name) && seen.insert(name.clone()) {
                        field_order.push(name.clone());
                    }
                }
                Type::object(ObjectShape {
                    fields,
                    field_order,
                    open: left.open || right.open,
                })
            }
            (Type::List(left), Type::List(right)) => Type::List(Type::shared(
                self.merge_resolved(left.as_ref().clone(), right.as_ref().clone()),
            )),
            (Type::Set(left), Type::Set(right)) => Type::Set(Type::shared(
                self.merge_resolved(left.as_ref().clone(), right.as_ref().clone()),
            )),
            (
                Type::Map {
                    key: left_key,
                    value: left_value,
                },
                Type::Map {
                    key: right_key,
                    value: right_value,
                },
            ) => Type::Map {
                key: Box::new(self.merge_resolved(*left_key, *right_key)),
                value: Box::new(self.merge_resolved(*left_value, *right_value)),
            },
            (left, right) if left == right => left,
            (left, right) => widen_structural_type(&left, &right),
        }
    }

    fn contains_contextual_hole(&self, ty: &Type, active: &mut BTreeSet<TypeVar>) -> bool {
        match ty {
            Type::Var(variable) => {
                let root = self.root_readonly(*variable);
                if self.contextual_holes[root.0 as usize] {
                    return true;
                }
                if !active.insert(root) {
                    return false;
                }
                let contains = self.bindings[root.0 as usize]
                    .as_ref()
                    .is_some_and(|binding| self.contains_contextual_hole(binding, active));
                active.remove(&root);
                contains
            }
            Type::Object(shape) => shape
                .fields
                .values()
                .any(|ty| self.contains_contextual_hole(ty, active)),
            Type::List(item) | Type::Set(item) => self.contains_contextual_hole(item, active),
            Type::Map { key, value } => {
                self.contains_contextual_hole(key, active)
                    || self.contains_contextual_hole(value, active)
            }
            Type::Function { args, result } => {
                args.iter()
                    .any(|ty| self.contains_contextual_hole(ty, active))
                    || self.contains_contextual_hole(&result.ty, active)
            }
            Type::VariantSet(variants) => variants.iter().any(|variant| match variant {
                Variant::Tag(_) => false,
                Variant::Tagged { fields, .. } => fields
                    .fields
                    .values()
                    .any(|ty| self.contains_contextual_hole(ty, active)),
            }),
            Type::Union(members) => members
                .iter()
                .any(|ty| self.contains_contextual_hole(ty, active)),
            Type::Text
            | Type::Number
            | Type::Bytes(_)
            | Type::Bits { .. }
            | Type::Absent
            | Type::RenderContract
            | Type::UnresolvedShape { .. }
            | Type::Unknown => false,
        }
    }

    fn is_concrete_contextual_donor(ty: &Type) -> bool {
        !type_contains_inference_variable(ty)
            && !matches!(ty, Type::Unknown | Type::UnresolvedShape { .. })
            && !matches!(ty, Type::Object(shape) if shape.open && shape.fields.is_empty())
    }

    fn bind_contextual_holes(&mut self, provider: &Type, requirement: &Type) {
        let provider = self.resolve(provider);
        let requirement = self.resolve(requirement);
        if !Self::is_concrete_contextual_donor(&requirement) {
            return;
        }
        match (provider, requirement) {
            (Type::Var(variable), requirement) => {
                let root = self.root(variable);
                if self.contextual_holes[root.0 as usize] {
                    self.bind_var(root, requirement);
                }
            }
            (Type::Object(provider), Type::Object(requirement)) => {
                for (name, provider) in &provider.fields {
                    if let Some(requirement) = requirement.fields.get(name) {
                        self.bind_contextual_holes(provider, requirement);
                    }
                }
            }
            (Type::List(provider), Type::List(requirement))
            | (Type::Set(provider), Type::Set(requirement)) => {
                self.bind_contextual_holes(&provider, &requirement);
            }
            (
                Type::Map {
                    key: provider_key,
                    value: provider_value,
                },
                Type::Map {
                    key: requirement_key,
                    value: requirement_value,
                },
            ) => {
                self.bind_contextual_holes(&provider_key, &requirement_key);
                self.bind_contextual_holes(&provider_value, &requirement_value);
            }
            (Type::Union(provider), requirement) => {
                for provider in provider.iter() {
                    self.bind_contextual_holes(provider, &requirement);
                }
            }
            _ => {}
        }
    }

    fn refine_contextual_union(&mut self, members: &[Type]) {
        let hole_members = members
            .iter()
            .filter(|member| self.contains_contextual_hole(member, &mut BTreeSet::new()))
            .cloned()
            .collect::<Vec<_>>();
        if hole_members.is_empty() {
            return;
        }
        let mut donors = Vec::new();
        for donor in members
            .iter()
            .map(|member| self.resolve(member))
            .filter(Self::is_concrete_contextual_donor)
        {
            if !donors.contains(&donor) {
                donors.push(donor);
            }
        }
        // Canonical unions do not retain authored alternative order. One
        // unique concrete donor is therefore the largest exact refinement we
        // can make here; multiple distinct donors require the future ordered
        // residual-flow program to reproduce dense source-order widening.
        let [donor] = donors.as_slice() else {
            return;
        };
        for member in &hole_members {
            self.bind_contextual_holes(member, donor);
        }
    }

    fn refine_contextual_holes_in_type(&mut self, ty: &Type, active: &mut BTreeSet<TypeVar>) {
        match ty {
            Type::Var(variable) => {
                let root = self.root(*variable);
                if !active.insert(root) {
                    return;
                }
                if let Some(binding) = self.bindings[root.0 as usize].clone() {
                    self.refine_contextual_holes_in_type(&binding, active);
                }
                active.remove(&root);
            }
            Type::Union(members) => {
                self.refine_contextual_union(members);
                for member in members.iter() {
                    self.refine_contextual_holes_in_type(member, active);
                }
            }
            Type::Object(shape) => {
                for ty in shape.fields.values() {
                    self.refine_contextual_holes_in_type(ty, active);
                }
            }
            Type::List(item) | Type::Set(item) => {
                self.refine_contextual_holes_in_type(item, active);
            }
            Type::Map { key, value } => {
                self.refine_contextual_holes_in_type(key, active);
                self.refine_contextual_holes_in_type(value, active);
            }
            Type::Function { args, result } => {
                for ty in args {
                    self.refine_contextual_holes_in_type(ty, active);
                }
                self.refine_contextual_holes_in_type(&result.ty, active);
            }
            Type::VariantSet(variants) => {
                for variant in variants.iter() {
                    if let Variant::Tagged { fields, .. } = variant {
                        for ty in fields.fields.values() {
                            self.refine_contextual_holes_in_type(ty, active);
                        }
                    }
                }
            }
            Type::Text
            | Type::Number
            | Type::Bytes(_)
            | Type::Bits { .. }
            | Type::Absent
            | Type::RenderContract
            | Type::UnresolvedShape { .. }
            | Type::Unknown => {}
        }
    }

    pub(crate) fn refine_contextual_flow_holes(&mut self) {
        loop {
            let changes_before = self.changes;
            let bindings = self.bindings.iter().flatten().cloned().collect::<Vec<_>>();
            for binding in &bindings {
                self.refine_contextual_holes_in_type(binding, &mut BTreeSet::new());
            }
            if self.changes == changes_before {
                break;
            }
        }
    }

    fn refine_contextual_binding_against(&mut self, variable: TypeVar, requirement: &Type) {
        let root = self.root(variable);
        let Some(binding) = self.bindings[root.0 as usize].clone() else {
            return;
        };
        if self.contains_contextual_hole(&binding, &mut BTreeSet::new()) {
            self.bind_contextual_holes(&binding, requirement);
        }
    }

    pub(crate) fn unify(&mut self, left: Type, right: Type) {
        self.steps = self.steps.saturating_add(1);
        match (left, right) {
            (Type::Var(left), Type::Var(right)) => self.union(left, right),
            (Type::Var(_), Type::Unknown | Type::UnresolvedShape { .. })
            | (Type::Unknown | Type::UnresolvedShape { .. }, Type::Var(_)) => {}
            (Type::Var(variable), ty) | (ty, Type::Var(variable)) => {
                let root = self.root_readonly(variable);
                if self.bindings[root.0 as usize]
                    .as_ref()
                    .is_some_and(|current| self.same_live_type(current, &ty))
                {
                    return;
                }
                self.refine_contextual_binding_against(variable, &ty);
                self.bind_var(variable, ty)
            }
            (Type::Union(members), ty) | (ty, Type::Union(members)) => {
                if members
                    .iter()
                    .any(|member| self.contains_contextual_hole(member, &mut BTreeSet::new()))
                {
                    for member in members.iter() {
                        self.bind_contextual_holes(member, &ty);
                    }
                }
            }
            (Type::Object(left), Type::Object(right)) => {
                for (name, left_ty) in &left.fields {
                    if let Some(right_ty) = right.fields.get(name) {
                        self.unify(left_ty.clone(), right_ty.clone());
                    }
                }
            }
            (Type::List(left), Type::List(right)) | (Type::Set(left), Type::Set(right)) => {
                self.unify(left.as_ref().clone(), right.as_ref().clone());
            }
            (
                Type::Map {
                    key: left_key,
                    value: left_value,
                },
                Type::Map {
                    key: right_key,
                    value: right_value,
                },
            ) => {
                self.unify(*left_key, *right_key);
                self.unify(*left_value, *right_value);
            }
            _ => {}
        }
    }

    /// Match one call-site provider against its instantiated formal scheme.
    ///
    /// A resolved authored value is authoritative over an open formal scheme.
    /// Project that proven shape into the formal holes instead of structurally
    /// merging it with the scheme, which would preserve neither provider
    /// authority nor the formal/result alpha equations. Open generic actuals
    /// retain the ordinary equation so callee requirements can still constrain
    /// the caller. Union alternatives need their correlation-preserving
    /// projection even while some provider leaves remain live.
    pub(crate) fn bind_call_pattern_input(&mut self, actual: &Type, formal: &Type) -> bool {
        if let Type::Union(actual) = actual {
            bind_call_formal_from_closed_alternatives(self, formal, actual);
            // Preserve every provider alternative, including its live alpha
            // leaves, in a directional formal projection. This does not bind
            // provider holes from consumer requirements; unresolved providers
            // therefore remain fail-closed while later provider closure stays
            // visible through the formal/result scheme.
            return true;
        }
        if boon_checked::type_is_recursively_closed(actual) {
            bind_call_formal_from_closed_alternatives(self, formal, std::slice::from_ref(actual));
            return true;
        }
        false
    }

    pub(crate) fn bind_call_input(&mut self, actual: TypeVar, formal: Type) {
        let actual = self.root(actual);
        let requirements = self.call_input_requirements.entry(actual).or_default();
        if !requirements.contains(&formal) {
            requirements.push(formal.clone());
        }
        let actual_type = self.resolve(&Type::Var(actual));
        if self.bind_call_pattern_input(&actual_type, &formal) {
            return;
        }
        self.unify(Type::Var(actual), formal);
    }

    pub(crate) const fn steps(&self) -> u64 {
        self.steps
    }

    pub(crate) const fn changes(&self) -> u64 {
        self.changes
    }
}

#[derive(Clone)]
struct OwnerSolveParameter {
    name: String,
    kind: OwnerParameterKind,
    ordinal: u32,
    variable: TypeVar,
    evaluation_scope: OwnerInterfaceEvaluationScope,
}

#[derive(Clone, Copy)]
enum PlannedLexicalRead {
    Unplanned,
    Bound(TypeVar),
    Imported(TypeVar),
    Dynamic,
    Reserved,
}

#[derive(Clone)]
struct OwnerSolveState<'a> {
    seed: &'a OwnerConstraintSeed,
    summary: &'a OwnerConstraintSummary,
    signature_lexical_plan: OwnerSignatureLexicalPlan,
    signature_declaration_variables: BTreeMap<OwnerSignatureDeclarationTarget, TypeVar>,
    lexical_declaration_variables: BTreeMap<OwnerLexicalTargetRef, TypeVar>,
    lexical_capture_variables: BTreeMap<OwnerLexicalTargetRef, TypeVar>,
    lexical_capture_reads: BTreeMap<OwnerLexicalTargetRef, Vec<usize>>,
    lexical_capture_read_variables: Vec<Option<TypeVar>>,
    lexical_capture_modes: BTreeMap<OwnerLexicalTargetRef, Option<FlowMode>>,
    signature_declaration_modes: BTreeMap<OwnerSignatureDeclarationTarget, Option<FlowMode>>,
    signature_read_expressions: BTreeMap<OwnerSignatureDeclarationTarget, Vec<usize>>,
    signature_dynamic_expressions: Vec<bool>,
    inherited_pattern_plans: Box<[OwnerInheritedPatternTargetPlan]>,
    pattern_local_expressions: BTreeSet<u32>,
    declaration_kind: Option<OwnerDeclarationKind>,
    names: Box<[String]>,
    parameters: Vec<OwnerSolveParameter>,
    context: TypeVar,
    result: TypeVar,
    result_flush: TypeVar,
    expressions: Vec<TypeVar>,
    expression_flushes: Vec<TypeVar>,
    call_flushes: Vec<Option<TypeVar>>,
    expression_by_key: BTreeMap<StableExpressionKey, usize>,
    external_expressions: Vec<TypeVar>,
    external_expression_flushes: Vec<TypeVar>,
    planned_lexical_reads: Vec<PlannedLexicalRead>,
    modes: Vec<Option<FlowMode>>,
    effect: CheckedEffectSummary,
}

#[derive(Clone)]
struct CrossCall {
    caller: StableCheckOwnerKey,
    expression: usize,
    target: Option<StableCheckOwnerKey>,
    function: String,
    stable_expression: StableExpressionKey,
    flush: TypeVar,
}

#[derive(Clone)]
pub(crate) struct OwnerPatternNarrowing {
    pub selector: TypeVar,
    pub pattern: OwnerPatternConstraint,
    /// One preallocated root for each exact pattern-binding authority or read.
    /// Projection equations are installed once while the narrowing is built;
    /// fixed-point refinement only updates these roots and never grows the
    /// unifier graph.
    pub bindings: Box<[(String, TypeVar)]>,
    /// Exact local reads projected from a pattern binding. Authoritative
    /// replacement of the binding root can detach a projection that was
    /// shaped during an earlier selector epoch, so replay these occurrences
    /// from the current matched payload on every narrowing pass.
    pub binding_reads: Box<[(String, Box<[String]>, TypeVar)]>,
    pub selector_reads: Box<[(Box<[String]>, TypeVar)]>,
}

#[derive(Clone)]
pub(crate) struct OwnerFlowConstraint {
    output: TypeVar,
    inputs: Box<[TypeVar]>,
    kind: OwnerFlowConstraintKind,
}

#[derive(Clone, Copy)]
enum OwnerFlowConstraintKind {
    Union,
    StructuralWiden,
}

#[derive(Clone)]
pub(crate) struct OwnerInheritedPatternReadPlan {
    pub expression: u32,
    pub projection: Box<[String]>,
}

#[derive(Clone)]
pub(crate) struct OwnerInheritedPatternTargetPlan {
    pub target: OwnerLexicalTargetRef,
    pub frames: Box<[OwnerInheritedPatternNarrowingPlan]>,
    pub reads: Box<[OwnerInheritedPatternReadPlan]>,
}

#[derive(Clone)]
pub(crate) struct OwnerInstantiatedPatternFrame {
    pub projection: Box<[String]>,
    pub pattern: OwnerPatternConstraint,
    pub schematic: Type,
}

#[derive(Clone)]
pub(crate) struct OwnerInstantiatedPatternRead {
    pub projection: Box<[String]>,
    pub variable: TypeVar,
}

#[derive(Clone)]
pub(crate) struct OwnerInheritedPatternNarrowing {
    pub root: TypeVar,
    pub frames: Box<[OwnerInstantiatedPatternFrame]>,
    pub reads: Box<[OwnerInstantiatedPatternRead]>,
}

/// Project the type-free boundary frames onto the exact imported read rows in
/// one consumer. Frames remain outer-to-inner for each stable target; reads
/// retain absolute projections from that target.
pub(crate) fn inherited_pattern_read_plans(
    plan: &OwnerSignatureLexicalPlan,
) -> Box<[OwnerInheritedPatternTargetPlan]> {
    let Some(environment) = plan.inherited_environment() else {
        return Box::new([]);
    };
    let mut frames =
        BTreeMap::<OwnerLexicalTargetRef, Vec<OwnerInheritedPatternNarrowingPlan>>::new();
    for frame in environment.pattern_narrowings() {
        frames
            .entry(frame.selector_target.clone())
            .or_default()
            .push(frame.clone());
    }
    frames
        .into_iter()
        .filter_map(|(target, frames)| {
            let reads = plan
                .reads()
                .iter()
                .enumerate()
                .filter_map(|(expression, read)| {
                    let read = read.as_ref()?;
                    if read.access != OwnerLexicalAccess::Read
                        || !matches!(
                            &read.target,
                            OwnerEffectiveLexicalTarget::Imported {
                                target: candidate,
                            } if candidate == &target
                        )
                        || !frames
                            .iter()
                            .any(|frame| read.projection.starts_with(&frame.selector_projection))
                    {
                        return None;
                    }
                    Some(OwnerInheritedPatternReadPlan {
                        expression: u32::try_from(expression)
                            .expect("owner expression index exceeds u32"),
                        projection: read.projection.clone(),
                    })
                })
                .collect::<Vec<_>>();
            (!reads.is_empty()).then(|| OwnerInheritedPatternTargetPlan {
                target,
                frames: frames.into_boxed_slice(),
                reads: reads.into_boxed_slice(),
            })
        })
        .collect::<Vec<_>>()
        .into_boxed_slice()
}

pub(crate) fn instantiate_owner_inherited_pattern_narrowings(
    plans: &[OwnerInheritedPatternTargetPlan],
    mut root_for_target: impl FnMut(&OwnerLexicalTargetRef) -> Option<TypeVar>,
    expressions: &[TypeVar],
    unifier: &mut TypeUnifier,
) -> Result<Vec<OwnerInheritedPatternNarrowing>, String> {
    plans
        .iter()
        .map(|plan| {
            let root = root_for_target(&plan.target).ok_or_else(|| {
                format!(
                    "inherited pattern target {:?} has no imported capture root",
                    plan.target
                )
            })?;
            let frames = plan
                .frames
                .iter()
                .map(|frame| OwnerInstantiatedPatternFrame {
                    projection: frame.selector_projection.clone(),
                    pattern: frame.pattern.clone(),
                    schematic: pattern_type(&frame.pattern, unifier),
                })
                .collect::<Vec<_>>();
            let reads = plan
                .reads
                .iter()
                .map(|read| {
                    let variable = expressions
                        .get(read.expression as usize)
                        .copied()
                        .ok_or_else(|| {
                            "inherited pattern read is outside the expression namespace".to_owned()
                        })?;
                    Ok(OwnerInstantiatedPatternRead {
                        projection: read.projection.clone(),
                        variable,
                    })
                })
                .collect::<Result<Vec<_>, String>>()?;
            Ok(OwnerInheritedPatternNarrowing {
                root,
                frames: frames.into_boxed_slice(),
                reads: reads.into_boxed_slice(),
            })
        })
        .collect()
}

fn matching_pattern_variants(ty: &Type, tag: &str, matches: &mut Vec<Variant>) {
    match ty {
        Type::VariantSet(variants) => matches.extend(variants.iter().filter_map(|variant| {
            let candidate = match variant {
                Variant::Tag(candidate) | Variant::Tagged { tag: candidate, .. } => candidate,
            };
            (candidate == tag).then(|| variant.clone())
        })),
        Type::Union(members) => {
            for member in members {
                matching_pattern_variants(member, tag, matches);
            }
        }
        Type::Text
        | Type::Number
        | Type::Bytes(_)
        | Type::Bits { .. }
        | Type::Absent
        | Type::Object(_)
        | Type::List(_)
        | Type::Set(_)
        | Type::Map { .. }
        | Type::Function { .. }
        | Type::RenderContract
        | Type::Var(_)
        | Type::UnresolvedShape { .. }
        | Type::Unknown => {}
    }
}

fn matching_pattern_type(selector: &Type, pattern: &OwnerPatternConstraint) -> Option<Type> {
    match pattern {
        OwnerPatternConstraint::Wildcard | OwnerPatternConstraint::Binding { .. } => {
            Some(selector.clone())
        }
        OwnerPatternConstraint::Number if matches!(selector, Type::Number) => Some(Type::Number),
        OwnerPatternConstraint::Text if matches!(selector, Type::Text) => Some(Type::Text),
        OwnerPatternConstraint::Bits { width } if matches!(selector, Type::Bits { width: actual } if actual == width) => {
            Some(Type::Bits { width: *width })
        }
        OwnerPatternConstraint::Tag { name, .. } => {
            let mut variants = Vec::new();
            matching_pattern_variants(selector, name, &mut variants);
            (!variants.is_empty()).then(|| Type::VariantSet(variants.into()))
        }
        OwnerPatternConstraint::Number
        | OwnerPatternConstraint::Text
        | OwnerPatternConstraint::Bits { .. }
        | OwnerPatternConstraint::Invalid => None,
    }
}

fn matching_pattern_field(
    selector: &Type,
    pattern: &OwnerPatternConstraint,
    name: &str,
) -> Option<Type> {
    match pattern {
        OwnerPatternConstraint::Binding { name: binding } if binding == name => {
            Some(selector.clone())
        }
        OwnerPatternConstraint::Tag { name: tag, fields }
            if fields.iter().any(|field| field == name) =>
        {
            let mut variants = Vec::new();
            matching_pattern_variants(selector, tag, &mut variants);
            variants
                .into_iter()
                .filter_map(|variant| match variant {
                    Variant::Tagged { fields, .. } => fields.fields.get(name).cloned(),
                    Variant::Tag(_) => None,
                })
                .reduce(|left, right| boon_checked::canonical_union_type(vec![left, right]))
        }
        _ => None,
    }
}

fn matching_selector_projection(
    selector: &Type,
    pattern: &OwnerPatternConstraint,
    projection: &[String],
) -> Option<Type> {
    let selected = matching_pattern_type(selector, pattern)?;
    if projection.is_empty() {
        return Some(selected);
    }
    let OwnerPatternConstraint::Tag { name, .. } = pattern else {
        return None;
    };
    let mut variants = Vec::new();
    matching_pattern_variants(&selected, name, &mut variants);
    variants
        .into_iter()
        .filter_map(|variant| match variant {
            Variant::Tagged { fields, .. } => {
                crate::type_for_nested_path(&Type::Object(fields), projection)
            }
            Variant::Tag(_) => None,
        })
        .reduce(|left, right| boon_checked::canonical_union_type(vec![left, right]))
}

fn project_pattern_overlay(base: &Type, projection: &[String]) -> Option<Type> {
    if projection.is_empty() {
        return Some(base.clone());
    }
    match base {
        Type::VariantSet(variants) => variants
            .iter()
            .filter_map(|variant| match variant {
                Variant::Tagged { fields, .. } => {
                    crate::type_for_nested_path(&Type::Object(fields.clone()), projection)
                }
                Variant::Tag(_) => None,
            })
            .reduce(|left, right| boon_checked::canonical_union_type(vec![left, right])),
        Type::Union(members) => members
            .iter()
            .filter_map(|member| project_pattern_overlay(member, projection))
            .reduce(|left, right| boon_checked::canonical_union_type(vec![left, right])),
        _ => crate::type_for_nested_path(base, projection),
    }
}

fn pattern_selector_is_open(ty: &Type) -> bool {
    type_contains_inference_variable(ty)
        || matches!(ty, Type::Unknown | Type::UnresolvedShape { .. })
        || matches!(ty, Type::Object(shape) if shape.open)
        || matches!(ty, Type::Union(members) if members.iter().any(pattern_selector_is_open))
}

#[derive(Clone)]
struct OwnerResolvedPatternOverlay {
    projection: Box<[String]>,
    selected: Type,
    schematic: bool,
}

fn projected_pattern_overlay_type(
    root: &Type,
    overlays: &[OwnerResolvedPatternOverlay],
    projection: &[String],
) -> (Option<Type>, bool) {
    let overlay = overlays
        .iter()
        .enumerate()
        .filter(|(_, overlay)| projection.starts_with(overlay.projection.as_ref()))
        .max_by_key(|(index, overlay)| (overlay.projection.len(), *index));
    if let Some((_, overlay)) = overlay {
        let remaining = &projection[overlay.projection.len()..];
        let projected = project_pattern_overlay(&overlay.selected, remaining).or_else(|| {
            overlay
                .schematic
                .then(|| project_pattern_overlay(root, projection))
                .flatten()
        });
        let open = projected.as_ref().is_some_and(pattern_selector_is_open);
        return (projected, open);
    }
    let projected = project_pattern_overlay(root, projection);
    let open = projected.as_ref().is_some_and(pattern_selector_is_open);
    (projected, open)
}

fn projected_type_from_active_pattern_overlay(
    root: &Type,
    overlays: &[OwnerResolvedPatternOverlay],
    projection: &[String],
) -> Option<Type> {
    let overlay = overlays
        .iter()
        .enumerate()
        .filter(|(_, overlay)| projection.starts_with(overlay.projection.as_ref()))
        .max_by_key(|(index, overlay)| (overlay.projection.len(), *index));
    let Some((_, overlay)) = overlay else {
        return project_pattern_overlay(root, projection);
    };
    project_pattern_overlay(&overlay.selected, &projection[overlay.projection.len()..]).or_else(
        || {
            overlay
                .schematic
                .then(|| project_pattern_overlay(root, projection))
                .flatten()
        },
    )
}

/// Apply inherited arm facts only to consumer-owned occurrence variables.
/// The imported capture root is resolved but never unified or constrained, so
/// an arm-local demand cannot widen its provider's authoritative declaration.
/// Nested frames compose outer-to-inner through the latest longest overlay.
pub(crate) fn refine_owner_inherited_pattern_narrowings(
    unifier: &mut TypeUnifier,
    narrowings: &[OwnerInheritedPatternNarrowing],
) {
    for narrowing in narrowings {
        let root = unifier.resolve(&Type::Var(narrowing.root));
        let mut overlays = Vec::<OwnerResolvedPatternOverlay>::new();
        for frame in &narrowing.frames {
            let (selector, inherited_open) =
                projected_pattern_overlay_type(&root, &overlays, &frame.projection);
            let selected = selector
                .as_ref()
                .and_then(|selector| matching_pattern_type(selector, &frame.pattern));
            let (selected, schematic) = match selected {
                Some(selected) => (selected, false),
                None if inherited_open
                    || selector.as_ref().is_some_and(pattern_selector_is_open) =>
                {
                    (frame.schematic.clone(), true)
                }
                None => continue,
            };
            overlays.push(OwnerResolvedPatternOverlay {
                projection: frame.projection.clone(),
                selected,
                schematic,
            });
        }
        for read in &narrowing.reads {
            let selected =
                projected_type_from_active_pattern_overlay(&root, &overlays, &read.projection);
            if let Some(selected) = selected {
                // This consumer-owned read is an occurrence of the selected
                // arm payload, not a second provider. A previously published
                // generic branch join can leave it as `Text | alpha` even
                // after the imported selector closes to a variant whose field
                // is exactly Text. Preserve provisional behavior while the
                // selector is open, but let a closed selector-derived value
                // replace that stale occurrence surface authoritatively.
                if boon_checked::type_is_recursively_closed(&selected) {
                    unifier.publish_authoritative_provider(read.variable, selected);
                } else {
                    unifier.bind_var(read.variable, selected);
                }
            }
        }
    }
}

pub(crate) fn refine_owner_pattern_narrowings(
    unifier: &mut TypeUnifier,
    narrowings: &[OwnerPatternNarrowing],
) {
    let publish = |unifier: &mut TypeUnifier, variable: TypeVar, ty: Type| {
        if boon_checked::type_is_recursively_closed(&ty) {
            unifier.publish_authoritative_provider(variable, ty);
        } else {
            unifier.bind_var(variable, ty);
        }
    };
    for narrowing in narrowings {
        let selector = unifier.resolve(&Type::Var(narrowing.selector));
        for (name, binding) in &narrowing.bindings {
            if let Some(ty) = matching_pattern_field(&selector, &narrowing.pattern, name) {
                // The selector owns the matched payload surface. A projected
                // arm read may already have shaped this binding as an open
                // Object requirement before a late lexical-capture epoch
                // closes the selector. In particular, a closed
                // `Union<Object, Object>` payload must project its common
                // fields into that requirement; symmetric Object/Union
                // widening would retain the arm's schematic alpha forever.
                publish(unifier, *binding, ty);
            }
        }
        for (name, projection, read) in &narrowing.binding_reads {
            if let Some(ty) = matching_pattern_field(&selector, &narrowing.pattern, name)
                && let Some(ty) = project_pattern_overlay(&ty, projection)
            {
                publish(unifier, *read, ty);
            }
        }
        for (projection, read) in &narrowing.selector_reads {
            if let Some(ty) =
                matching_selector_projection(&selector, &narrowing.pattern, projection)
            {
                // This occurrence is owned by one match arm and is therefore
                // an exact type equation, not a covariant branch join.  A
                // generic consumer may already have shaped the occurrence as
                // (for example) `List<alpha>`; structurally bind that alpha to
                // the selector-derived item type instead of widening the read
                // to `List<alpha> | List<Number>`.
                publish(unifier, *read, ty);
            }
        }
    }
}

#[derive(Clone)]
struct InstantiatedInterfaceParameter {
    name: String,
    ordinal: u32,
    ty: Type,
    mode: FlowMode,
    evaluation_scope: OwnerInterfaceEvaluationScope,
}

#[derive(Clone)]
struct InstantiatedInterfaceCallContext {
    ordinal: u32,
    name: String,
    provider_parameter_ordinal: u32,
    ty: Type,
    mode: FlowMode,
}

pub(crate) fn flow_mode_join(left: Option<FlowMode>, right: Option<FlowMode>) -> Option<FlowMode> {
    match (left, right) {
        (None, mode) | (mode, None) => mode,
        (Some(FlowMode::Absent), _) | (_, Some(FlowMode::Absent)) => Some(FlowMode::Absent),
        (Some(FlowMode::PresentOrAbsent), _) | (_, Some(FlowMode::PresentOrAbsent)) => {
            Some(FlowMode::PresentOrAbsent)
        }
        (Some(FlowMode::TickPresent), _) | (_, Some(FlowMode::TickPresent)) => {
            Some(FlowMode::TickPresent)
        }
        (Some(FlowMode::Continuous), Some(FlowMode::Continuous)) => Some(FlowMode::Continuous),
    }
}

pub(crate) fn merge_effects(
    left: CheckedEffectSummary,
    right: CheckedEffectSummary,
) -> CheckedEffectSummary {
    CheckedEffectSummary {
        reads_state: left.reads_state || right.reads_state,
        writes_state: left.writes_state || right.writes_state,
        emits_source: left.emits_source || right.emits_source,
        invokes_host: left.invokes_host || right.invokes_host,
    }
}

pub(crate) fn true_false_type() -> Type {
    Type::VariantSet(
        vec![
            Variant::Tag("False".to_owned()),
            Variant::Tag("True".to_owned()),
        ]
        .into(),
    )
}

pub(crate) fn pattern_type(pattern: &OwnerPatternConstraint, unifier: &mut TypeUnifier) -> Type {
    match pattern {
        OwnerPatternConstraint::Wildcard | OwnerPatternConstraint::Invalid => Type::Unknown,
        OwnerPatternConstraint::Number => Type::Number,
        OwnerPatternConstraint::Text => Type::Text,
        OwnerPatternConstraint::Binding { .. } => Type::Var(unifier.fresh()),
        OwnerPatternConstraint::Bits { width } => Type::Bits { width: *width },
        OwnerPatternConstraint::Tag { name, fields } if fields.is_empty() => {
            Type::VariantSet(vec![Variant::Tag(name.clone())].into())
        }
        OwnerPatternConstraint::Tag { name, fields } => Type::VariantSet(
            vec![Variant::Tagged {
                tag: name.clone(),
                fields: ObjectShape::from_ordered_fields(
                    fields
                        .iter()
                        .map(|field| (field.clone(), Type::Var(unifier.fresh()))),
                    false,
                ),
            }]
            .into(),
        ),
    }
}

pub(crate) fn pattern_binding_type_from_pattern(
    pattern: &OwnerPatternConstraint,
    pattern_ty: &Type,
    name: &str,
) -> Option<Type> {
    match pattern {
        OwnerPatternConstraint::Binding { name: binding } if binding == name => {
            Some(pattern_ty.clone())
        }
        OwnerPatternConstraint::Tag { name: tag, fields }
            if fields.iter().any(|field| field == name) =>
        {
            let Type::VariantSet(variants) = pattern_ty else {
                return None;
            };
            variants.iter().find_map(|variant| match variant {
                Variant::Tagged {
                    tag: candidate,
                    fields,
                } if candidate == tag => fields.fields.get(name).cloned(),
                _ => None,
            })
        }
        _ => None,
    }
}

pub(crate) fn bind_projection(
    unifier: &mut TypeUnifier,
    root: TypeVar,
    fields: &[String],
) -> TypeVar {
    let root = unifier.root(root);
    if let Some(consumer) = unifier
        .provider_projections
        .get(&root)
        .into_iter()
        .flatten()
        .find(|candidate| candidate.projection.as_ref() == fields)
        .map(|candidate| candidate.consumer)
    {
        return consumer;
    }
    let authoritative = unifier.authoritative_providers[root.0 as usize];
    if fields.is_empty() && !authoritative {
        return root;
    }
    let consumer = if authoritative {
        let consumer = unifier.fresh();
        let provider = unifier.resolve(&Type::Var(root));
        match crate::type_for_nested_path(&provider, fields) {
            Some(projected)
                if !matches!(
                    projected,
                    Type::Var(variable) if unifier.root_readonly(variable) == root
                ) =>
            {
                unifier.replace_authoritative_binding(consumer, projected);
            }
            Some(_) => {}
            None => unifier.replace_authoritative_binding(
                consumer,
                Type::UnresolvedShape {
                    reason: format!(
                        "authoritative provider omits projection `{}`",
                        fields.join(".")
                    ),
                },
            ),
        }
        unifier.mark_authoritative_provider(consumer);
        consumer
    } else {
        // Ordinary formal/parameter projections remain true inference
        // equations. Record their final read slot below so a later promotion
        // to a derived provider can refresh it without retaining the old
        // producer scaffold.
        let mut current = root;
        for field in fields {
            let next = unifier.fresh();
            let resolved = unifier.resolve(&Type::Var(current));
            let existing = match &resolved {
                Type::Object(shape) => shape.fields.get(field).cloned(),
                Type::Union(members) => members
                    .iter()
                    .filter_map(|member| {
                        crate::type_for_nested_path(member, std::slice::from_ref(field))
                    })
                    .reduce(|left, right| boon_checked::canonical_union_type(vec![left, right])),
                _ => None,
            };
            if let Some(existing) = existing {
                unifier.unify(Type::Var(next), existing);
            } else {
                unifier.bind_var(
                    current,
                    Type::object(ObjectShape::from_ordered_fields(
                        [(field.clone(), Type::Var(next))],
                        true,
                    )),
                );
            }
            current = next;
        }
        current
    };
    let projection = AuthoritativeProviderProjection {
        projection: fields.to_vec().into_boxed_slice(),
        consumer,
    };
    unifier
        .provider_projections
        .entry(root)
        .or_default()
        .push(projection);
    consumer
}

fn owner_node_has_derived_provider(kind: &OwnerConstraintNodeKind) -> bool {
    matches!(
        kind,
        OwnerConstraintNodeKind::Draining
            | OwnerConstraintNodeKind::Hold { .. }
            | OwnerConstraintNodeKind::Latest
            | OwnerConstraintNodeKind::When
            | OwnerConstraintNodeKind::Then
            | OwnerConstraintNodeKind::MatchArm { .. }
            | OwnerConstraintNodeKind::Block
            | OwnerConstraintNodeKind::Collection { .. }
            | OwnerConstraintNodeKind::Arrow { .. }
    )
}

pub(crate) fn mark_owner_derived_providers(
    unifier: &mut TypeUnifier,
    seed: &OwnerConstraintSeed,
    expressions: &[TypeVar],
) {
    for (expression, variable) in seed.expressions.iter().zip(expressions) {
        if owner_node_has_derived_provider(&expression.kind) {
            unifier.mark_authoritative_provider(*variable);
        }
    }
}

pub(crate) fn bind_flow_variables(
    unifier: &mut TypeUnifier,
    output: TypeVar,
    inputs: impl IntoIterator<Item = TypeVar>,
) {
    let inputs = inputs.into_iter().map(Type::Var).collect::<Vec<_>>();
    unifier.mark_authoritative_provider(output);
    unifier.replace_derived_provider(output, boon_checked::canonical_union_type(inputs));
}

pub(crate) fn bind_and_record_flow_variables(
    unifier: &mut TypeUnifier,
    constraints: &mut Vec<OwnerFlowConstraint>,
    output: TypeVar,
    inputs: impl IntoIterator<Item = TypeVar>,
) {
    let inputs = inputs.into_iter().collect::<Vec<_>>();
    bind_flow_variables(unifier, output, inputs.iter().copied());
    constraints.push(OwnerFlowConstraint {
        output,
        inputs: inputs.into_boxed_slice(),
        kind: OwnerFlowConstraintKind::Union,
    });
}

pub(crate) fn bind_and_record_structural_flow_variables(
    unifier: &mut TypeUnifier,
    constraints: &mut Vec<OwnerFlowConstraint>,
    output: TypeVar,
    inputs: impl IntoIterator<Item = TypeVar>,
) {
    let inputs = inputs.into_iter().collect::<Vec<_>>();
    let provider = structural_flow_provider(unifier, &inputs);
    unifier.mark_authoritative_provider(output);
    unifier.replace_derived_provider(output, provider);
    constraints.push(OwnerFlowConstraint {
        output,
        inputs: inputs.into_boxed_slice(),
        kind: OwnerFlowConstraintKind::StructuralWiden,
    });
}

fn structural_flow_provider(unifier: &mut TypeUnifier, inputs: &[TypeVar]) -> Type {
    inputs
        .iter()
        .map(|input| unifier.resolve(&Type::Var(*input)))
        .reduce(|left, right| boon_checked::widen_structural_type(&left, &right))
        .unwrap_or(Type::Absent)
}

fn flow_constraint_provider(unifier: &mut TypeUnifier, constraint: &OwnerFlowConstraint) -> Type {
    match constraint.kind {
        OwnerFlowConstraintKind::Union => boon_checked::canonical_union_type(
            constraint
                .inputs
                .iter()
                .map(|input| unifier.resolve(&Type::Var(*input)))
                .collect(),
        ),
        OwnerFlowConstraintKind::StructuralWiden => {
            structural_flow_provider(unifier, &constraint.inputs)
        }
    }
}

fn flow_constraint_dependency_roots(
    unifier: &mut TypeUnifier,
    constraint: &OwnerFlowConstraint,
) -> Vec<TypeVar> {
    let mut dependencies = BTreeSet::from([constraint.output]);
    for input in &constraint.inputs {
        dependencies.insert(*input);
        let resolved = unifier.resolve(&Type::Var(*input));
        collect_type_variables(&resolved, &mut dependencies);
    }
    dependencies
        .into_iter()
        .map(|variable| unifier.root_readonly(variable))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

struct OwnerFlowConstraintProgram {
    constraints: Vec<OwnerFlowConstraint>,
    consumers: HashMap<TypeVar, Vec<u32>>,
    cursor: TypeMutationCursor,
    initialized: bool,
}

impl OwnerFlowConstraintProgram {
    fn new(unifier: &mut TypeUnifier, constraints: Vec<OwnerFlowConstraint>) -> Self {
        let cursor = unifier.mutation_cursor();
        let mut program = Self {
            constraints,
            consumers: HashMap::new(),
            cursor,
            initialized: false,
        };
        for index in 0..program.constraints.len() {
            program.refresh_dependencies(unifier, index);
        }
        program
    }

    fn refresh_dependencies(&mut self, unifier: &mut TypeUnifier, index: usize) {
        let dependencies = flow_constraint_dependency_roots(unifier, &self.constraints[index]);
        let index = u32::try_from(index).expect("owner flow constraint index exceeds u32");
        for dependency in dependencies {
            let consumers = self.consumers.entry(dependency).or_default();
            if !consumers.contains(&index) {
                consumers.push(index);
            }
        }
    }

    fn enqueue_mutations(
        &mut self,
        unifier: &TypeUnifier,
        pending: &mut VecDeque<usize>,
        queued: &mut [bool],
    ) {
        let events = unifier
            .mutations_since(self.cursor)
            .iter()
            .map(|event| event.variable)
            .collect::<Vec<_>>();
        self.cursor = unifier.mutation_cursor();
        for variable in events {
            let root = unifier.root_readonly(variable);
            for dependency in [variable, root] {
                for consumer in self
                    .consumers
                    .get(&dependency)
                    .into_iter()
                    .flatten()
                    .copied()
                {
                    let consumer = consumer as usize;
                    if !queued[consumer] {
                        queued[consumer] = true;
                        pending.push_back(consumer);
                    }
                }
            }
        }
    }

    fn replay(&mut self, unifier: &mut TypeUnifier) -> bool {
        let trace =
            std::env::var_os("BOON_OWNER_REQUEST_TRACE").is_some() && self.constraints.len() >= 100;
        let started = Instant::now();
        if self.constraints.is_empty() {
            self.cursor = unifier.mutation_cursor();
            self.initialized = true;
            return true;
        }
        let mut pending = VecDeque::new();
        let mut queued = vec![false; self.constraints.len()];
        if self.initialized {
            self.enqueue_mutations(unifier, &mut pending, &mut queued);
        } else {
            self.cursor = unifier.mutation_cursor();
            pending.extend(0..self.constraints.len());
            queued.fill(true);
            self.initialized = true;
        }
        let maximum_evaluations = self
            .constraints
            .len()
            .saturating_mul(self.constraints.len().saturating_add(1));
        let mut evaluations = 0usize;
        while let Some(index) = pending.pop_front() {
            queued[index] = false;
            evaluations = evaluations.saturating_add(1);
            if evaluations > maximum_evaluations {
                if trace {
                    eprintln!(
                        "boon owner flow replay constraints={} evaluations={} replay_ms={:.3} converged=false",
                        self.constraints.len(),
                        evaluations,
                        started.elapsed().as_secs_f64() * 1_000.0,
                    );
                }
                return false;
            }
            self.refresh_dependencies(unifier, index);
            let provider = flow_constraint_provider(unifier, &self.constraints[index]);
            unifier.replace_derived_provider(self.constraints[index].output, provider);
            self.enqueue_mutations(unifier, &mut pending, &mut queued);
        }
        if trace {
            eprintln!(
                "boon owner flow replay constraints={} evaluations={} replay_ms={:.3}",
                self.constraints.len(),
                evaluations,
                started.elapsed().as_secs_f64() * 1_000.0,
            );
        }
        true
    }
}

/// Re-evaluate covariant expression-flow equations after a provider epoch.
///
/// The persistent production program consumes the unifier's complete mutation
/// and authority journal through an independent cursor. Each equation indexes
/// its inputs, nested live alphas, and its authoritative output, so external
/// provider epochs and later output overwrites both wake the exact dependency
/// cone. The former full-sweep evaluation budget remains the fail-closed cap.
pub(crate) fn replay_flow_constraints(
    unifier: &mut TypeUnifier,
    constraints: &[OwnerFlowConstraint],
) -> bool {
    OwnerFlowConstraintProgram::new(unifier, constraints.to_vec()).replay(unifier)
}

pub(crate) fn bind_hold_variables(
    unifier: &mut TypeUnifier,
    authorities: &mut BTreeMap<TypeVar, Type>,
    output: TypeVar,
    initial: TypeVar,
    updates: impl IntoIterator<Item = TypeVar>,
) -> bool {
    let initial = unifier.resolve(&Type::Var(initial));
    let previous = authorities
        .get(&output)
        .map(|previous| unifier.resolve(previous))
        .unwrap_or_else(|| initial.clone());
    let mut ty = crate::widen_checked_hold_type(&initial, &previous);
    // A HOLD update may return the prior state directly, or retain that
    // self-reference as one alternative of a nested WHEN/LATEST result. Solve
    // that recursive reference from the current ordered accumulator, rather
    // than from the unresolved or stale live HOLD root. This is the ordinary
    // finite approximation of `H = initial widen updates(H)` and preserves
    // structural self uses while preventing an old provisional H from
    // widening an otherwise closed update into an open object.
    let output_root = unifier.root(output);
    for update in updates {
        let recursive_state = BTreeMap::from([(output_root, ty.clone())]);
        let update =
            unifier.resolve_with_projection_authorities(&Type::Var(update), &recursive_state);
        if !matches!(update, Type::Absent) {
            ty = if hold_update_contains_current(unifier, &ty, &update) {
                update
            } else {
                crate::widen_checked_hold_type(&ty, &update)
            };
        }
    }
    let authority_changed = !authorities.contains_key(&output) || previous != ty;
    authorities.insert(output, ty.clone());
    // The ordered dense HOLD fold is the single authority even while its
    // provider leaves remain live. The private authority map, rather than the
    // mutable output root, carries prior epochs: consumer projections may
    // legitimately shape that root, but they must never become state history.
    // Ordinary flow unioning would retain a provisional prior epoch beside
    // the newly widened result.
    // HOLD's prior value is an input to the fold, never a consumer requirement
    // on the newly computed state. Replacing directly prevents a generic
    // initializer alias from being specialized by the first concrete update.
    unifier.publish_derived_provider_epoch(output, ty);
    authority_changed
}

/// Detect the monotone `current | new` form produced by a recursive HOLD arm.
/// Structural widening remains authoritative for genuinely transformed state;
/// this only avoids widening a union superset against its own prior subset.
fn hold_update_contains_current(unifier: &TypeUnifier, current: &Type, update: &Type) -> bool {
    if unifier.same_live_type(current, update) {
        return true;
    }
    match (current, update) {
        (Type::Union(current), update) => current
            .iter()
            .all(|current| hold_update_contains_current(unifier, current, update)),
        (current, Type::Union(update)) => update
            .iter()
            .any(|update| hold_update_contains_current(unifier, current, update)),
        (Type::VariantSet(current), Type::VariantSet(update)) => current.iter().all(|current| {
            update.iter().any(|update| match (current, update) {
                (Variant::Tag(current), Variant::Tag(update)) => current == update,
                (
                    Variant::Tagged {
                        tag: current_tag,
                        fields: current_fields,
                    },
                    Variant::Tagged {
                        tag: update_tag,
                        fields: update_fields,
                    },
                ) => current_tag == update_tag && current_fields == update_fields,
                _ => false,
            })
        }),
        _ => false,
    }
}

pub(crate) fn replay_owner_hold_constraints(
    unifier: &mut TypeUnifier,
    authorities: &mut BTreeMap<TypeVar, Type>,
    seed: &OwnerConstraintSeed,
    expressions: &[TypeVar],
    external_expressions: &[TypeVar],
) -> bool {
    let changes_before = unifier.changes();
    let mut authority_changed = false;
    let expression_variable = |reference: u32| {
        let reference = reference as usize;
        expressions.get(reference).copied().or_else(|| {
            external_expressions
                .get(reference.checked_sub(expressions.len())?)
                .copied()
        })
    };
    for (index, expression) in seed.expressions.iter().enumerate() {
        if !matches!(expression.kind, OwnerConstraintNodeKind::Hold { .. }) {
            continue;
        }
        let Some(initial) = expression
            .inputs
            .iter()
            .find(|input| matches!(input.role, OwnerConstraintEdgeRole::HoldInitial))
            .and_then(|input| expression_variable(input.expression))
        else {
            continue;
        };
        let updates = expression
            .inputs
            .iter()
            .filter(|input| matches!(input.role, OwnerConstraintEdgeRole::HoldUpdate))
            .filter_map(|input| expression_variable(input.expression));
        authority_changed |=
            bind_hold_variables(unifier, authorities, expressions[index], initial, updates);
    }
    authority_changed || unifier.changes() != changes_before
}

pub(crate) fn initialize_owner_hold_constraints(
    unifier: &mut TypeUnifier,
    authorities: &mut BTreeMap<TypeVar, Type>,
    seed: &OwnerConstraintSeed,
    expressions: &[TypeVar],
    external_expressions: &[TypeVar],
) -> bool {
    let mut authority_changed = false;
    let expression_variable = |reference: u32| {
        let reference = reference as usize;
        expressions.get(reference).copied().or_else(|| {
            external_expressions
                .get(reference.checked_sub(expressions.len())?)
                .copied()
        })
    };
    for (index, expression) in seed.expressions.iter().enumerate() {
        if !matches!(expression.kind, OwnerConstraintNodeKind::Hold { .. }) {
            continue;
        }
        let Some(initial) = expression
            .inputs
            .iter()
            .find(|input| matches!(input.role, OwnerConstraintEdgeRole::HoldInitial))
            .and_then(|input| expression_variable(input.expression))
        else {
            continue;
        };
        let initial = unifier.resolve(&Type::Var(initial));
        authority_changed |= authorities.get(&expressions[index]) != Some(&initial);
        authorities.insert(expressions[index], initial.clone());
        // Bootstrap the visible state from its initializer without treating
        // that initializer as a consumer requirement. A later HOLD fold uses
        // one-way authoritative replacement as well, so a generic initializer
        // cannot receive constraints from an update epoch.
        unifier.mark_authoritative_provider(expressions[index]);
        unifier.publish_derived_provider_epoch(expressions[index], initial);
    }
    authority_changed
}

fn replay_interface_hold_constraints(
    unifier: &mut TypeUnifier,
    authorities: &mut BTreeMap<TypeVar, Type>,
    states: &BTreeMap<StableCheckOwnerKey, OwnerSolveState<'_>>,
) -> bool {
    let changes_before = unifier.changes();
    let mut authority_changed = false;
    for state in states.values() {
        authority_changed |= replay_owner_hold_constraints(
            unifier,
            authorities,
            state.seed,
            &state.expressions,
            &state.external_expressions,
        );
    }
    authority_changed || unifier.changes() != changes_before
}

fn expression_variable(state: &OwnerSolveState<'_>, reference: u32) -> Option<TypeVar> {
    let reference = reference as usize;
    state.expressions.get(reference).copied().or_else(|| {
        state
            .external_expressions
            .get(reference.checked_sub(state.expressions.len())?)
            .copied()
    })
}

fn expression_flush_variable(state: &OwnerSolveState<'_>, reference: u32) -> Option<TypeVar> {
    let reference = reference as usize;
    state
        .expression_flushes
        .get(reference)
        .copied()
        .or_else(|| {
            state
                .external_expression_flushes
                .get(reference.checked_sub(state.expression_flushes.len())?)
                .copied()
        })
}

fn expression_boundary_variable(
    state: &OwnerSolveState<'_>,
    reference: u32,
    unifier: &mut TypeUnifier,
) -> Option<TypeVar> {
    let value = expression_variable(state, reference)?;
    let flush = expression_flush_variable(state, reference)?;
    let boundary = unifier.fresh();
    unifier.bind_var(
        boundary,
        boon_checked::canonical_union_type(vec![Type::Var(value), Type::Var(flush)]),
    );
    Some(boundary)
}

fn insert_lexical_declaration_variable(
    variables: &mut BTreeMap<OwnerLexicalTargetRef, TypeVar>,
    target: OwnerLexicalTargetRef,
    variable: TypeVar,
    unifier: &mut TypeUnifier,
) {
    match variables.entry(target) {
        std::collections::btree_map::Entry::Vacant(entry) => {
            entry.insert(variable);
        }
        std::collections::btree_map::Entry::Occupied(entry) => {
            unifier.unify(Type::Var(*entry.get()), Type::Var(variable));
        }
    }
}

fn signature_declaration_target_ref(
    owner: &StableCheckOwnerKey,
    target: &OwnerSignatureDeclarationTarget,
) -> OwnerLexicalTargetRef {
    match target {
        OwnerSignatureDeclarationTarget::FreshOut {
            call,
            formal_ordinal,
        } => OwnerLexicalTargetRef::Declaration {
            owner: owner.clone(),
            declaration: OwnerDeclarationStableKey::FreshOut {
                call: call.clone(),
                formal_ordinal: *formal_ordinal,
            },
            capability: OwnerLexicalDeclarationCapability::Out {
                evaluation_scope: boon_checked::OwnerStableScopeRef {
                    owner: owner.clone(),
                    scope: boon_checked::OwnerScopeStableKey::GeneratedOut {
                        call: call.clone(),
                        formal_ordinal: *formal_ordinal,
                    },
                },
            },
        },
        OwnerSignatureDeclarationTarget::CallContext {
            call,
            context_ordinal,
        } => OwnerLexicalTargetRef::Declaration {
            owner: owner.clone(),
            declaration: OwnerDeclarationStableKey::CallContext {
                call: call.clone(),
                ordinal: *context_ordinal,
            },
            capability: OwnerLexicalDeclarationCapability::Value,
        },
    }
}

fn signature_declaration_target(
    target: &OwnerEffectiveLexicalTarget,
) -> Option<OwnerSignatureDeclarationTarget> {
    match target {
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
        OwnerEffectiveLexicalTarget::Imported {
            target:
                OwnerLexicalTargetRef::Declaration {
                    declaration:
                        OwnerDeclarationStableKey::FreshOut {
                            call,
                            formal_ordinal,
                        },
                    ..
                },
        } => Some(OwnerSignatureDeclarationTarget::FreshOut {
            call: call.clone(),
            formal_ordinal: *formal_ordinal,
        }),
        OwnerEffectiveLexicalTarget::Imported {
            target:
                OwnerLexicalTargetRef::Declaration {
                    declaration: OwnerDeclarationStableKey::CallContext { call, ordinal },
                    ..
                },
        } => Some(OwnerSignatureDeclarationTarget::CallContext {
            call: call.clone(),
            context_ordinal: *ordinal,
        }),
        OwnerEffectiveLexicalTarget::Static { .. }
        | OwnerEffectiveLexicalTarget::Imported { .. }
        | OwnerEffectiveLexicalTarget::InvalidBareBinding { .. }
        | OwnerEffectiveLexicalTarget::Ambiguous { .. } => None,
    }
}

/// Mark every expression whose value depends on one FreshOut or call-context
/// declaration. Call inputs in this set must be staged until all calls have
/// published their output declarations for the current solve round; otherwise
/// a consumer can impose its open formal shape before the producer supplies a
/// closed authoritative value.
pub(crate) fn signature_dynamic_expression_index(
    seed: &OwnerConstraintSeed,
    plan: &OwnerSignatureLexicalPlan,
) -> Vec<bool> {
    let mut parents = vec![Vec::new(); seed.expressions.len()];
    for (parent, expression) in seed.expressions.iter().enumerate() {
        for input in &expression.inputs {
            if let Some(parents) = parents.get_mut(input.expression as usize) {
                parents.push(parent);
            }
        }
    }
    for parents in &mut parents {
        parents.sort_unstable();
        parents.dedup();
    }
    let mut pending = VecDeque::new();
    let mut dynamic = vec![false; seed.expressions.len()];
    for (expression, read) in plan.reads().iter().enumerate() {
        let Some(_target) = read
            .as_ref()
            .and_then(|read| signature_declaration_target(&read.target))
        else {
            continue;
        };
        if !dynamic[expression] {
            dynamic[expression] = true;
            pending.push_back(expression);
        }
    }
    while let Some(expression) = pending.pop_front() {
        for parent in &parents[expression] {
            if !dynamic[*parent] {
                dynamic[*parent] = true;
                pending.push_back(*parent);
            }
        }
    }
    dynamic
}

fn initialize_lexical_declaration_variables(
    state: &mut OwnerSolveState<'_>,
    unifier: &mut TypeUnifier,
) -> Result<(), OwnerConstraintSeedError> {
    let public_capability = match state.declaration_kind {
        Some(OwnerDeclarationKind::Function) => OwnerLexicalDeclarationCapability::CallableOnly,
        Some(_) => OwnerLexicalDeclarationCapability::Value,
        None => OwnerLexicalDeclarationCapability::Value,
    };
    if !matches!(
        &public_capability,
        OwnerLexicalDeclarationCapability::CallableOnly
    ) {
        insert_lexical_declaration_variable(
            &mut state.lexical_declaration_variables,
            OwnerLexicalTargetRef::Declaration {
                owner: state.seed.owner.clone(),
                declaration: OwnerDeclarationStableKey::Public,
                capability: public_capability,
            },
            state.result,
            unifier,
        );
    }

    for (local, stable) in state.seed.signature_regions().stable_targets() {
        let variable = match local {
            OwnerLexicalDeclarationTarget::Parameter { ordinal } => state
                .parameters
                .iter()
                .find(|parameter| parameter.ordinal == *ordinal)
                .map(|parameter| parameter.variable)
                .ok_or_else(|| {
                    OwnerConstraintSeedError::new(format!(
                        "interface lexical declaration references missing parameter {ordinal}"
                    ))
                })?,
            OwnerLexicalDeclarationTarget::Statement { statement } => {
                if matches!(
                    stable,
                    OwnerLexicalTargetRef::Declaration {
                        capability: OwnerLexicalDeclarationCapability::CallableOnly,
                        ..
                    }
                ) {
                    continue;
                }
                let expression =
                    state
                        .seed
                        .statement_values
                        .iter()
                        .find_map(|(candidate, expression)| {
                            (candidate == statement).then_some(*expression)
                        });
                if let Some(expression) = expression {
                    expression_boundary_variable(state, expression, unifier).ok_or_else(|| {
                        OwnerConstraintSeedError::new(
                            "interface statement value is outside its expression namespace",
                        )
                    })?
                } else {
                    // Valueless authored fields remain real declarations with
                    // an Unknown continuous surface. They are required by
                    // render/output metadata even when no read demands them.
                    unifier.fresh()
                }
            }
            OwnerLexicalDeclarationTarget::RecordField {
                object, ordinal, ..
            } => {
                let object = state
                    .seed
                    .expressions
                    .get(*object as usize)
                    .ok_or_else(|| {
                        OwnerConstraintSeedError::new(
                            "interface record declaration references a missing expression",
                        )
                    })?;
                let input = object.inputs.get(*ordinal as usize).ok_or_else(|| {
                    OwnerConstraintSeedError::new(
                        "interface record declaration references a missing field ordinal",
                    )
                })?;
                if !matches!(
                    input.role,
                    OwnerConstraintEdgeRole::RecordField { spread: false, .. }
                ) {
                    return Err(OwnerConstraintSeedError::new(
                        "interface record declaration does not match its field edge",
                    ));
                }
                expression_boundary_variable(state, input.expression, unifier).ok_or_else(|| {
                    OwnerConstraintSeedError::new(
                        "interface record field value is outside its expression namespace",
                    )
                })?
            }
            OwnerLexicalDeclarationTarget::PatternBinding { .. } => unifier.fresh(),
            OwnerLexicalDeclarationTarget::Passed => state.context,
            OwnerLexicalDeclarationTarget::Imported { .. }
            | OwnerLexicalDeclarationTarget::Ambiguous { .. } => continue,
        };
        insert_lexical_declaration_variable(
            &mut state.lexical_declaration_variables,
            stable.clone(),
            variable,
            unifier,
        );
    }

    if state.declaration_kind == Some(OwnerDeclarationKind::Function) {
        insert_lexical_declaration_variable(
            &mut state.lexical_declaration_variables,
            OwnerLexicalTargetRef::ContextFormal {
                owner: state.seed.owner.clone(),
            },
            state.context,
            unifier,
        );
    }
    for (target, variable) in &state.signature_declaration_variables {
        insert_lexical_declaration_variable(
            &mut state.lexical_declaration_variables,
            signature_declaration_target_ref(&state.seed.owner, target),
            *variable,
            unifier,
        );
    }
    for target in state.signature_lexical_plan.imported_captures() {
        if state
            .lexical_capture_variables
            .insert(target.clone(), unifier.fresh())
            .is_some()
        {
            return Err(OwnerConstraintSeedError::new(
                "interface signature plan repeats one imported lexical capture",
            ));
        }
        state.lexical_capture_modes.insert(target.clone(), None);
    }
    for (index, read) in state.signature_lexical_plan.reads().iter().enumerate() {
        let Some(OwnerEffectiveLexicalReadPlan {
            target: OwnerEffectiveLexicalTarget::Imported { target },
            ..
        }) = read
        else {
            continue;
        };
        if state.lexical_capture_variables.contains_key(target) {
            state.lexical_capture_read_variables[index] = Some(unifier.fresh());
            state
                .lexical_capture_reads
                .entry(target.clone())
                .or_default()
                .push(index);
        }
    }
    Ok(())
}

fn planned_lexical_read_variables(
    state: &OwnerSolveState<'_>,
) -> Result<Vec<PlannedLexicalRead>, OwnerConstraintSeedError> {
    if state.signature_lexical_plan.reads().len() != state.seed.expressions.len()
        || !state.signature_lexical_plan.matches_seed(state.seed)
    {
        return Err(OwnerConstraintSeedError::new(
            "interface signature lexical plan does not cover its current expression table",
        ));
    }

    let mut reads = Vec::with_capacity(state.signature_lexical_plan.reads().len());
    for read in state.signature_lexical_plan.reads() {
        let Some(read) = read else {
            reads.push(PlannedLexicalRead::Unplanned);
            continue;
        };
        let (root, imported) = match &read.target {
            OwnerEffectiveLexicalTarget::Static { target } => (
                state
                    .seed
                    .signature_regions()
                    .stable_target(target)
                    .and_then(|target| state.lexical_declaration_variables.get(target))
                    .copied(),
                false,
            ),
            OwnerEffectiveLexicalTarget::FreshOut {
                call,
                formal_ordinal,
            } => (
                state
                    .signature_declaration_variables
                    .get(&OwnerSignatureDeclarationTarget::FreshOut {
                        call: call.clone(),
                        formal_ordinal: *formal_ordinal,
                    })
                    .copied(),
                false,
            ),
            OwnerEffectiveLexicalTarget::CallContext {
                call,
                context_ordinal,
            } => (
                state
                    .signature_declaration_variables
                    .get(&OwnerSignatureDeclarationTarget::CallContext {
                        call: call.clone(),
                        context_ordinal: *context_ordinal,
                    })
                    .copied(),
                false,
            ),
            OwnerEffectiveLexicalTarget::Imported { target } => (
                state
                    .lexical_capture_variables
                    .contains_key(target)
                    .then(|| state.lexical_capture_read_variables[reads.len()])
                    .flatten(),
                true,
            ),
            OwnerEffectiveLexicalTarget::InvalidBareBinding { .. }
            | OwnerEffectiveLexicalTarget::Ambiguous { .. } => (None, false),
        };
        // Keep the root lazy. A selector projection owned by branch-local
        // pattern narrowing must not constrain the public root merely because
        // the base lexical plan can name it.
        let dynamic = matches!(
            &read.target,
            OwnerEffectiveLexicalTarget::FreshOut { .. }
                | OwnerEffectiveLexicalTarget::CallContext { .. }
        );
        let planned = root.map_or(PlannedLexicalRead::Reserved, |root| {
            if dynamic {
                let _ = root;
                PlannedLexicalRead::Dynamic
            } else if imported {
                PlannedLexicalRead::Imported(root)
            } else {
                PlannedLexicalRead::Bound(root)
            }
        });
        reads.push(planned);
    }
    Ok(reads)
}

fn signature_read_preserved_projection_for(
    seed: &OwnerConstraintSeed,
    signature_lexical_plan: &OwnerSignatureLexicalPlan,
    expression: u32,
) -> Option<Box<[String]>> {
    let Some(base) = seed
        .lexical_reads()
        .get(expression as usize)
        .and_then(Option::as_ref)
    else {
        return None;
    };
    let read = signature_lexical_plan
        .reads()
        .get(expression as usize)
        .and_then(Option::as_ref)?;
    (matches!(
        &read.target,
        OwnerEffectiveLexicalTarget::Static { target } if target == &base.target
    ) && read.projection == base.projection)
        .then(|| read.projection.clone())
}

fn exact_pattern_local_expressions(
    seed: &OwnerConstraintSeed,
    signature_lexical_plan: &OwnerSignatureLexicalPlan,
) -> BTreeSet<u32> {
    let mut expressions = BTreeSet::new();
    for arm in &seed.expressions {
        let selector = arm.inputs.iter().find_map(|input| {
            matches!(input.role, OwnerConstraintEdgeRole::MatchSelector).then_some(input.expression)
        });
        for input in &arm.inputs {
            match &input.role {
                OwnerConstraintEdgeRole::MatchBinding { .. }
                    if signature_read_preserved_projection_for(
                        seed,
                        signature_lexical_plan,
                        input.expression,
                    )
                    .is_some() =>
                {
                    expressions.insert(input.expression);
                }
                OwnerConstraintEdgeRole::MatchNarrowedSelector { projection }
                    if selector.is_some_and(|selector| {
                        effective_narrowed_selector_read_matches(
                            seed,
                            signature_lexical_plan,
                            selector,
                            projection,
                            input.expression,
                        )
                    }) =>
                {
                    expressions.insert(input.expression);
                }
                _ => {}
            }
        }
    }
    expressions
}

fn signature_narrowed_selector_read_matches(
    state: &OwnerSolveState<'_>,
    selector: u32,
    projection: &[String],
    candidate: u32,
) -> bool {
    effective_narrowed_selector_read_matches(
        state.seed,
        &state.signature_lexical_plan,
        selector,
        projection,
        candidate,
    )
}

fn lexical_declaration_mode(
    state: &OwnerSolveState<'_>,
    target: &OwnerLexicalTargetRef,
) -> Result<Option<FlowMode>, OwnerConstraintSeedError> {
    match target {
        OwnerLexicalTargetRef::Declaration {
            owner,
            declaration:
                OwnerDeclarationStableKey::FreshOut {
                    call,
                    formal_ordinal,
                },
            ..
        } if owner == &state.seed.owner => Ok(state
            .signature_declaration_modes
            .get(&OwnerSignatureDeclarationTarget::FreshOut {
                call: call.clone(),
                formal_ordinal: *formal_ordinal,
            })
            .copied()
            .flatten()),
        OwnerLexicalTargetRef::Declaration {
            owner,
            declaration: OwnerDeclarationStableKey::CallContext { call, ordinal },
            ..
        } if owner == &state.seed.owner => Ok(state
            .signature_declaration_modes
            .get(&OwnerSignatureDeclarationTarget::CallContext {
                call: call.clone(),
                context_ordinal: *ordinal,
            })
            .copied()
            .flatten()),
        OwnerLexicalTargetRef::Declaration {
            owner,
            capability:
                OwnerLexicalDeclarationCapability::Value | OwnerLexicalDeclarationCapability::Out { .. },
            ..
        } if owner == &state.seed.owner => {
            let local = state
                .seed
                .signature_regions()
                .stable_targets()
                .iter()
                .find_map(|(local, stable)| (stable == target).then_some(local))
                .ok_or_else(|| {
                    OwnerConstraintSeedError::new(
                        "interface lexical declaration has no local stable target",
                    )
                })?;
            match local {
                OwnerLexicalDeclarationTarget::Statement { statement } => {
                    let expression =
                        state
                            .seed
                            .statement_values
                            .iter()
                            .find_map(|(candidate, expression)| {
                                (candidate == statement).then_some(*expression as usize)
                            });
                    Ok(Some(expression.map_or(
                        FlowMode::Continuous,
                        |expression| {
                            state
                                .modes
                                .get(expression)
                                .copied()
                                .flatten()
                                .unwrap_or(FlowMode::Continuous)
                        },
                    )))
                }
                OwnerLexicalDeclarationTarget::RecordField {
                    object,
                    ordinal,
                    name,
                } => {
                    let input = state
                        .seed
                        .expressions
                        .get(*object as usize)
                        .into_iter()
                        .flat_map(|expression| expression.inputs.iter())
                        .enumerate()
                        .find_map(|(candidate, input)| {
                            matches!(
                                &input.role,
                                OwnerConstraintEdgeRole::RecordField {
                                    name: candidate_name,
                                    spread: false,
                                } if candidate as u32 == *ordinal && candidate_name == name
                            )
                            .then_some(input.expression as usize)
                        })
                        .ok_or_else(|| {
                            OwnerConstraintSeedError::new(
                                "interface lexical record field has no value expression",
                            )
                        })?;
                    Ok(Some(
                        state
                            .modes
                            .get(input)
                            .copied()
                            .flatten()
                            .unwrap_or(FlowMode::Continuous),
                    ))
                }
                OwnerLexicalDeclarationTarget::Parameter { .. }
                | OwnerLexicalDeclarationTarget::PatternBinding { .. }
                | OwnerLexicalDeclarationTarget::Passed => Ok(Some(FlowMode::Continuous)),
                OwnerLexicalDeclarationTarget::Imported { .. }
                | OwnerLexicalDeclarationTarget::Ambiguous { .. } => {
                    Err(OwnerConstraintSeedError::new(
                        "interface local lexical target resolves through a foreign or ambiguous declaration",
                    ))
                }
            }
        }
        OwnerLexicalTargetRef::ContextFormal { owner } if owner == &state.seed.owner => {
            Ok(Some(FlowMode::Continuous))
        }
        OwnerLexicalTargetRef::Declaration {
            capability: OwnerLexicalDeclarationCapability::CallableOnly,
            ..
        }
        | OwnerLexicalTargetRef::Ambiguous { .. } => Err(OwnerConstraintSeedError::new(
            "non-readable lexical target cannot supply an interface capture mode",
        )),
        OwnerLexicalTargetRef::Declaration { owner, .. }
        | OwnerLexicalTargetRef::ContextFormal { owner } => {
            Err(OwnerConstraintSeedError::new(format!(
                "interface lexical mode target belongs to {owner:?}, not {:?}",
                state.seed.owner
            )))
        }
    }
}

fn propagate_lexical_capture_types(
    states: &BTreeMap<StableCheckOwnerKey, OwnerSolveState<'_>>,
    internal_providers: &[InternalLexicalCaptureProvider],
    capture_alpha_frames: &mut BTreeMap<
        (StableCheckOwnerKey, OwnerLexicalTargetRef),
        LexicalCaptureAlphaFrame,
    >,
    previous_capture_surfaces: &mut BTreeMap<(StableCheckOwnerKey, OwnerLexicalTargetRef), Type>,
    allow_requirement_backflow: bool,
    unifier: &mut TypeUnifier,
) -> Result<(), OwnerConstraintSeedError> {
    let trace =
        std::env::var_os("BOON_OWNER_REQUEST_TRACE").is_some() && internal_providers.len() >= 100;
    let started = Instant::now();
    // A readable public declaration is projected from its exact result
    // expression boundary, while its persistent lexical root (`state.result`)
    // also receives consumer requirements from same-component captures. A
    // consumer can therefore leave that persistent root holding a stale
    // sparse scaffold even after the expression boundary has become closed.
    // Before exporting a closed public value, restore the same authority used
    // by `project_owner_interface_scc_result`; otherwise the provider can
    // publish a closed interface while a full-root lexical capture of it still
    // observes the earlier consumer shape.
    let mut refreshed_public_roots = BTreeSet::new();
    for provider in internal_providers {
        if !matches!(
            &provider.target,
            OwnerLexicalTargetRef::Declaration {
                declaration: OwnerDeclarationStableKey::Public,
                capability: OwnerLexicalDeclarationCapability::Value,
                ..
            }
        ) || !refreshed_public_roots.insert(unifier.root_readonly(provider.provider))
        {
            continue;
        }
        let provider_owner = match &provider.target {
            OwnerLexicalTargetRef::Declaration { owner, .. } => owner,
            OwnerLexicalTargetRef::ContextFormal { .. }
            | OwnerLexicalTargetRef::Ambiguous { .. } => unreachable!(),
        };
        // Production construction admits only same-component providers here.
        // The propagation primitive is also exercised directly with a frozen
        // provider surface and no owner-state arena; in that case there is no
        // expression boundary to refresh before applying the capture.
        let Some(state) = states.get(provider_owner) else {
            continue;
        };
        let Some(expression) = owner_result_expression(state) else {
            continue;
        };
        let raw_mode = state
            .modes
            .get(expression as usize)
            .copied()
            .flatten()
            .unwrap_or(FlowMode::Continuous);
        let authority = resolved_expression_boundary(state, expression, unifier, raw_mode);
        if boon_checked::type_is_recursively_closed(&authority.ty) {
            unifier.publish_authoritative_provider(provider.provider, authority.ty);
        }
    }

    // Provider roots may coalesce between rounds. Canonicalize each persistent
    // frame before taking the frozen provider epoch and merge the corresponding
    // consumer copies. Otherwise a newly canonical provider alpha could be
    // absent from the frame even though both pre-union roots were mapped.
    for frame in capture_alpha_frames.values_mut() {
        let previous_variables = std::mem::take(&mut frame.variables);
        let mut previous_demands = std::mem::take(&mut frame.demands);
        for (provider, consumer) in previous_variables {
            let demand = previous_demands.remove(&provider).ok_or_else(|| {
                OwnerConstraintSeedError::new(
                    "interface lexical capture alpha has no relative demand",
                )
            })?;
            let provider = unifier.root(provider);
            let consumer = unifier.root(consumer);
            frame.demands.entry(provider).or_default().merge(&demand);
            if let Some(existing) = frame.variables.get(&provider).copied() {
                unifier.unify(Type::Var(existing), Type::Var(consumer));
                frame.variables.insert(provider, unifier.root(existing));
            } else {
                frame.variables.insert(provider, consumer);
            }
        }
        if !previous_demands.is_empty() {
            return Err(OwnerConstraintSeedError::new(
                "interface lexical capture retains demand for an unknown alpha",
            ));
        }
    }

    // Classify providers before mutating any capture in this round. Otherwise
    // the first consumer of one generic provider can close it and make later
    // consumers look authoritative, turning inference into owner-route order.
    let mut resolved_provider_roots = Vec::new();
    let mut resolved_capture_roots = Vec::new();
    let mut capture_surfaces = Vec::with_capacity(internal_providers.len());
    let mut frozen_capture_requirements = BTreeMap::new();
    let mut seen_capture_keys = BTreeSet::new();
    for provider in internal_providers {
        let key = (provider.consumer.clone(), provider.target.clone());
        if !seen_capture_keys.insert(key.clone()) {
            return Err(OwnerConstraintSeedError::new(
                "interface lexical capture repeats one consumer target",
            ));
        }
        let (ty, demands) = unifier.resolve_lexical_capture_surface_with_demands(
            provider.provider,
            &provider.demand,
            &mut resolved_provider_roots,
        );
        let changed = previous_capture_surfaces.get(&key) != Some(&ty);
        let open_variables = if allow_requirement_backflow {
            let mut variables = BTreeSet::new();
            collect_type_variables(&ty, &mut variables);
            variables
        } else {
            BTreeSet::new()
        };
        if allow_requirement_backflow {
            let requirement = unifier
                .resolve_with_cache(&Type::Var(provider.capture), &mut resolved_capture_roots);
            frozen_capture_requirements.insert(key.clone(), requirement);
        }
        capture_surfaces.push((key, ty, changed, open_variables, demands));
    }

    // A provider alpha can initially stand for a strict demanded prefix and
    // acquire an Object binding in a later round. Retain that relative demand
    // on the persistent frame; refreshing it with an ordinary full resolve
    // would reintroduce every omitted sibling through the alpha seam.
    for (key, _, _, _, demands) in &capture_surfaces {
        let frame = capture_alpha_frames.entry(key.clone()).or_default();
        for (variable, demand) in demands {
            if frame.variables.contains_key(variable) {
                frame.demands.entry(*variable).or_default().merge(demand);
            }
        }
    }

    for (provider, (key, provider_type, provider_changed, _, surface_demands)) in
        internal_providers.iter().zip(&capture_surfaces)
    {
        let frame = capture_alpha_frames.entry(key.clone()).or_default();
        // `variables` is the stable provider-alpha -> consumer-alpha frame
        // created when this capture first observes an open provider. A later
        // fixed-point round can close those provider alphas without changing
        // the provider's outer surface. Refresh the already-published
        // consumer copies before merging that newer surface; otherwise the
        // capture retains detached `Var` members even though its provider is
        // now concrete.
        if *provider_changed {
            let copied_variables = frame
                .variables
                .iter()
                .map(|(provider, consumer)| (*provider, *consumer))
                .collect::<Vec<_>>();
            for (provider_variable, consumer_variable) in copied_variables {
                let demand = frame
                    .demands
                    .get(&provider_variable)
                    .cloned()
                    .ok_or_else(|| {
                        OwnerConstraintSeedError::new(
                            "interface lexical capture alpha refresh has no relative demand",
                        )
                    })?;
                let (resolved, nested_demands) = unifier
                    .resolve_lexical_capture_surface_with_demands(
                        provider_variable,
                        &demand,
                        &mut resolved_provider_roots,
                    );
                let resolved = instantiate_type(&resolved, unifier, &mut frame.variables);
                for (variable, demand) in nested_demands {
                    frame.demands.entry(variable).or_default().merge(&demand);
                }
                // This slot is the consumer-owned copy of one exact provider
                // alpha. A consumer may have shaped it while the provider was
                // unresolved, but that scaffold is a requirement rather than
                // a second provider epoch. Preserve requirements in genuine
                // provider holes, then let the refreshed provider alpha own
                // the copied slot's outer structure.
                unifier.publish_authoritative_provider(consumer_variable, resolved);
            }
            let captured = instantiate_type(provider_type, unifier, &mut frame.variables);
            for (variable, demand) in surface_demands {
                frame.demands.entry(*variable).or_default().merge(demand);
            }
            if frame
                .variables
                .keys()
                .any(|variable| !frame.demands.contains_key(variable))
            {
                return Err(OwnerConstraintSeedError::new(
                    "interface lexical capture instantiated an alpha without demand provenance",
                ));
            }
            // Once the copied alpha frame resolves to the current provider
            // surface, rebinding the outer capture would merge its stale raw
            // `Union<Var...>` slot with the same concrete value. Compare
            // resolved surfaces only for providers whose exact surface
            // changed since the previous propagation.
            let capture_root = unifier.root_readonly(provider.capture);
            let same_live = unifier.bindings[capture_root.0 as usize]
                .as_ref()
                .is_some_and(|current| unifier.same_live_type(current, &captured));
            let resolved_capture =
                (!same_live).then(|| unifier.resolve(&Type::Var(provider.capture)));
            let resolved_captured = (!same_live).then(|| unifier.resolve(&captured));
            let resolved_equal = same_live || resolved_capture == resolved_captured;
            if !resolved_equal {
                // A lexical capture is a directional provider surface, not a
                // branch join. Consumer work from an earlier round may have
                // shaped its independent copy while the provider was still
                // unresolved. Once the provider closes, retain requirements
                // only in genuine copied holes and replace that provisional
                // outer scaffold; unioning the two epochs would make stale
                // consumer shape co-authoritative forever.
                unifier.publish_authoritative_provider(provider.capture, captured);
            }
        }
    }
    // Apply the consumer requirements that were frozen before any forward
    // capture publication in this round. Resolving the requirement here would
    // combine it with the provider echo into a semantic Union, obscuring the
    // exact Object/List requirement that must be projected into the independent
    // alpha frame. The frame then supplies the exact provider-hole ->
    // consumer-hole seams for deterministic backflow.
    let mut capture_requirements = Vec::new();
    if allow_requirement_backflow {
        // The outer capture is a directional flow surface. Project consumer
        // constraints from that surface into its independent alpha slots
        // before reading the exact provider-alpha seams; never union the whole
        // provider and capture roots merely to make nested requirements visible.
        let mut slot_requirements = Vec::new();
        for (provider, (key, provider_type, _, _, _)) in
            internal_providers.iter().zip(&capture_surfaces)
        {
            let frame = capture_alpha_frames.get_mut(key).ok_or_else(|| {
                OwnerConstraintSeedError::new(
                    "interface lexical capture has no alpha frame for requirement projection",
                )
            })?;
            let template = instantiate_type(provider_type, unifier, &mut frame.variables);
            let requirement = frozen_capture_requirements
                .get(key)
                .cloned()
                .ok_or_else(|| {
                    OwnerConstraintSeedError::new(
                        "interface lexical capture has no frozen consumer requirement",
                    )
                })?;
            let demanded_requirement = provider.demand.project_resolved(&requirement);
            bind_open_provider_requirement(
                unifier,
                provider.provider,
                provider_type,
                &demanded_requirement,
            );
            slot_requirements.push((template, requirement));
        }
        for (template, requirement) in slot_requirements {
            bind_provider_inference_holes_resolved(unifier, &template, &requirement);
        }
        resolved_capture_roots.clear();
        for (key, _, _, open_variables, _) in &capture_surfaces {
            let frame = capture_alpha_frames.get(key).ok_or_else(|| {
                OwnerConstraintSeedError::new(
                    "interface lexical capture has no instantiated alpha frame",
                )
            })?;
            for provider in open_variables {
                let demand = frame.demands.get(provider).ok_or_else(|| {
                    OwnerConstraintSeedError::new(
                        "interface lexical capture omits relative demand for an open alpha",
                    )
                })?;
                let consumer = frame.variables.get(provider).copied().ok_or_else(|| {
                    OwnerConstraintSeedError::new(
                        "interface lexical capture omits an open provider alpha",
                    )
                })?;
                let requirement =
                    unifier.resolve_with_cache(&Type::Var(consumer), &mut resolved_capture_roots);
                if !demand.full
                    && (matches!(
                        requirement,
                        Type::Var(_) | Type::Unknown | Type::UnresolvedShape { .. }
                    ) || matches!(
                        &requirement,
                        Type::Object(shape) if shape.open && shape.fields.is_empty()
                    ))
                {
                    // A bare consumer alpha at a strict projection prefix is
                    // not an equation for the provider's unseen outer shape.
                    // Keep the seam detached until the consumer contributes a
                    // structural requirement; that sparse requirement can then
                    // flow back directionally without unioning the two roots.
                    continue;
                }
                capture_requirements.push((*provider, requirement));
            }
        }
    }
    for (provider, requirement) in capture_requirements {
        bind_provider_inference_holes_resolved(unifier, &Type::Var(provider), &requirement);
    }
    *previous_capture_surfaces = capture_surfaces
        .into_iter()
        .map(|(key, ty, _, _, _)| (key, ty))
        .collect();

    for (_owner, state) in states {
        for (target, capture) in &state.lexical_capture_variables {
            let capture_variable = *capture;
            for index in state
                .lexical_capture_reads
                .get(target)
                .into_iter()
                .flatten()
            {
                let read = state
                    .lexical_capture_read_variables
                    .get(*index)
                    .copied()
                    .flatten()
                    .ok_or_else(|| {
                        OwnerConstraintSeedError::new(
                            "interface lexical capture read has no consumer-owned type root",
                        )
                    })?;
                unifier.unify(Type::Var(read), Type::Var(capture_variable));

                // The expression is a directional occurrence of this capture.
                // Its one-time flow alias can detach when a consumer shapes it
                // before the internal provider epoch closes. Republish the
                // current projected provider into the occurrence after every
                // capture epoch so downstream call formals observe late
                // authority. Pattern-local occurrences remain owned by their
                // inherited narrowing overlay below.
                if state.pattern_local_expressions.contains(&(*index as u32)) {
                    continue;
                }
                let plan = state.signature_lexical_plan.reads()[*index]
                    .as_ref()
                    .ok_or_else(|| {
                        OwnerConstraintSeedError::new(
                            "interface lexical capture occurrence lost its read plan",
                        )
                    })?;
                let provider = unifier.resolve(&Type::Var(capture_variable));
                if let Some(projected) = crate::type_for_nested_path(&provider, &plan.projection) {
                    unifier.publish_authoritative_provider(state.expressions[*index], projected);
                }
            }
        }
    }
    if trace {
        eprintln!(
            "boon owner capture replay providers={} backflow={} replay_ms={:.3}",
            internal_providers.len(),
            allow_requirement_backflow,
            started.elapsed().as_secs_f64() * 1_000.0,
        );
    }
    Ok(())
}

fn propagate_lexical_capture_modes(
    states: &mut BTreeMap<StableCheckOwnerKey, OwnerSolveState<'_>>,
) -> Result<bool, OwnerConstraintSeedError> {
    let mut updates = Vec::new();
    for (consumer, state) in states.iter() {
        for target in state.lexical_capture_variables.keys() {
            let provider_owner = match target {
                OwnerLexicalTargetRef::Declaration { owner, .. }
                | OwnerLexicalTargetRef::ContextFormal { owner } => owner,
                OwnerLexicalTargetRef::Ambiguous { .. } => {
                    return Err(OwnerConstraintSeedError::new(
                        "ambiguous lexical target cannot supply a capture mode",
                    ));
                }
            };
            let mode = if let Some(provider) = states.get(provider_owner) {
                lexical_declaration_mode(provider, target)?
            } else {
                state
                    .lexical_capture_modes
                    .get(target)
                    .copied()
                    .flatten()
                    .ok_or_else(|| {
                        OwnerConstraintSeedError::new(
                            "interface lexical capture mode provider is outside its SCC and has no frozen public mode",
                        )
                    })
                    .map(Some)?
            };
            updates.push((consumer.clone(), target.clone(), mode));
        }
    }
    let mut changed = false;
    for (consumer, target, mode) in updates {
        let Some(mode) = mode else {
            continue;
        };
        let state = states.get_mut(&consumer).expect("capture consumer exists");
        let slot = state
            .lexical_capture_modes
            .get_mut(&target)
            .expect("capture mode slot was reserved");
        let merged = flow_mode_join(*slot, Some(mode));
        changed |= merged != *slot;
        *slot = merged;
        for read in state
            .lexical_capture_reads
            .get(&target)
            .into_iter()
            .flatten()
        {
            let merged = flow_mode_join(state.modes[*read], Some(mode));
            changed |= merged != state.modes[*read];
            state.modes[*read] = merged;
        }
    }
    Ok(changed)
}

fn effective_context_variable(
    state: &OwnerSolveState<'_>,
) -> Result<Option<TypeVar>, OwnerConstraintSeedError> {
    if state.declaration_kind == Some(OwnerDeclarationKind::Function) {
        return Ok(Some(state.context));
    }
    let inherited = state
        .lexical_capture_variables
        .iter()
        .filter_map(|(target, variable)| {
            matches!(target, OwnerLexicalTargetRef::ContextFormal { .. }).then_some(*variable)
        })
        .collect::<Vec<_>>();
    match inherited.as_slice() {
        [] => Ok(None),
        [variable] => Ok(Some(*variable)),
        _ => Err(OwnerConstraintSeedError::new(
            "owner interface has multiple inherited PASSED context formals",
        )),
    }
}

fn bind_signature_declaration_reads(
    state: &mut OwnerSolveState<'_>,
    target: &OwnerSignatureDeclarationTarget,
    root: TypeVar,
    mode: FlowMode,
    unifier: &mut TypeUnifier,
) {
    for expression in state
        .signature_read_expressions
        .get(target)
        .into_iter()
        .flatten()
        .copied()
    {
        let Some(read) = state.signature_lexical_plan.reads()[expression].as_ref() else {
            continue;
        };
        let projected = bind_projection(unifier, root, &read.projection);
        unifier.unify(
            Type::Var(state.expressions[expression]),
            Type::Var(projected),
        );
        state.modes[expression] = flow_mode_join(state.modes[expression], Some(mode));
    }
}

fn resolved_expression_flush_type(
    state: &OwnerSolveState<'_>,
    reference: u32,
    unifier: &mut TypeUnifier,
) -> Option<Type> {
    let ty = expression_flush_variable(state, reference)
        .map(|variable| unifier.resolve(&Type::Var(variable)))?;
    (!matches!(
        ty,
        Type::Unknown | Type::UnresolvedShape { .. } | Type::Absent
    ))
    .then_some(ty)
}

fn resolved_expression_flush_type_with_projection_authorities(
    state: &OwnerSolveState<'_>,
    reference: u32,
    unifier: &mut TypeUnifier,
    authorities: &BTreeMap<TypeVar, Type>,
) -> Option<Type> {
    let ty = expression_flush_variable(state, reference).map(|variable| {
        unifier.resolve_with_projection_authorities(&Type::Var(variable), authorities)
    })?;
    (!matches!(
        ty,
        Type::Unknown | Type::UnresolvedShape { .. } | Type::Absent
    ))
    .then_some(ty)
}

fn resolved_expression_boundary(
    state: &OwnerSolveState<'_>,
    reference: u32,
    unifier: &mut TypeUnifier,
    raw_mode: FlowMode,
) -> FlowType {
    let value = expression_variable(state, reference)
        .map(|variable| unifier.resolve(&Type::Var(variable)))
        .unwrap_or(Type::Absent);
    let flush = resolved_expression_flush_type(state, reference, unifier);
    FlowType {
        mode: if raw_mode == FlowMode::Absent && flush.is_some() {
            FlowMode::Continuous
        } else {
            raw_mode
        },
        ty: flush.map_or(value.clone(), |flush| {
            boon_checked::canonical_union_type(vec![value, flush])
        }),
    }
}

fn resolved_expression_boundary_with_projection_authorities(
    state: &OwnerSolveState<'_>,
    reference: u32,
    unifier: &mut TypeUnifier,
    raw_mode: FlowMode,
    authorities: &BTreeMap<TypeVar, Type>,
) -> FlowType {
    let value = expression_variable(state, reference)
        .map(|variable| {
            unifier.resolve_with_projection_authorities(&Type::Var(variable), authorities)
        })
        .unwrap_or(Type::Absent);
    let flush = resolved_expression_flush_type_with_projection_authorities(
        state,
        reference,
        unifier,
        authorities,
    );
    FlowType {
        mode: if raw_mode == FlowMode::Absent && flush.is_some() {
            FlowMode::Continuous
        } else {
            raw_mode
        },
        ty: flush.map_or(value.clone(), |flush| {
            boon_checked::canonical_union_type(vec![value, flush])
        }),
    }
}

fn interface_transfer_expression_value(
    state: &OwnerSolveState<'_>,
    reference: u32,
    unifier: &mut TypeUnifier,
) -> Option<EvaluatedResultValue> {
    let variable = expression_variable(state, reference)?;
    let index = reference as usize;
    let mode = state
        .modes
        .get(index)
        .copied()
        .flatten()
        .unwrap_or(FlowMode::Continuous);
    let static_number = state.seed.expressions.get(index).and_then(|expression| {
        state
            .seed
            .result_static_numbers
            .binary_search_by(|number| number.expression.cmp(&expression.expression))
            .ok()
            .and_then(|index| state.seed.result_static_numbers.get(index))
            .and_then(|number| ExactNumber::parse_strict(&number.literal, None).ok())
    });
    Some(EvaluatedResultValue {
        flow_type: FlowType {
            mode,
            ty: unifier.resolve(&Type::Var(variable)),
        },
        parameter_derived: false,
        syntax_selected: false,
        static_number,
    })
}

fn owner_result_expression(state: &OwnerSolveState<'_>) -> Option<u32> {
    let public = state
        .seed
        .declarations
        .iter()
        .find(|declaration| declaration.public)?;
    state
        .seed
        .statement_values
        .iter()
        .find(|(statement, _)| *statement == public.statement)
        .map(|(_, expression)| *expression)
        .or_else(|| {
            (public.kind == OwnerDeclarationKind::Function)
                .then(|| {
                    state
                        .seed
                        .statement_values
                        .last()
                        .map(|(_, expression)| *expression)
                })
                .flatten()
        })
}

fn owner_result_expression_ref(
    state: &OwnerSolveState<'_>,
    reference: u32,
) -> Result<OwnerResidualExpressionRef, OwnerConstraintSeedError> {
    let reference = reference as usize;
    if let Some(expression) = state.seed.expressions.get(reference) {
        return Ok(OwnerResidualExpressionRef::Local {
            expression: expression.expression.clone(),
        });
    }
    let external = state
        .seed
        .external_expressions
        .get(reference.saturating_sub(state.seed.expressions.len()))
        .ok_or_else(|| {
            OwnerConstraintSeedError::new(format!(
                "owner result transfer {:?} references expression {reference} outside its local/external namespace",
                state.seed.owner
            ))
        })?;
    Ok(OwnerResidualExpressionRef::Child {
        owner: external.owner.clone(),
        expression: external.expression.clone(),
    })
}

fn owner_result_parameter_read(
    state: &OwnerSolveState<'_>,
    expression: &crate::OwnerExpressionConstraint,
) -> Option<OwnerResidualParameterRead> {
    let index = state
        .expression_by_key
        .get(&expression.expression)
        .copied()?;
    let read = state.signature_lexical_plan.reads().get(index)?.as_ref()?;
    let OwnerEffectiveLexicalTarget::Static {
        target: OwnerLexicalDeclarationTarget::Parameter { ordinal },
    } = &read.target
    else {
        return None;
    };
    Some(OwnerResidualParameterRead {
        parameter_ordinal: *ordinal,
        projection: read.projection.clone(),
    })
}

fn owner_result_call_target(
    state: &OwnerSolveState<'_>,
    abi: &OwnerInferenceAbiEnvironment,
    expression: &crate::OwnerExpressionConstraint,
) -> Result<Option<OwnerResidualCallTarget>, OwnerConstraintSeedError> {
    let function = match &expression.kind {
        OwnerConstraintNodeKind::Call { function }
        | OwnerConstraintNodeKind::Pipe {
            operation: function,
        } => function,
        _ => return Ok(None),
    };
    let expression_index = state
        .expression_by_key
        .get(&expression.expression)
        .copied()
        .ok_or_else(|| {
            OwnerConstraintSeedError::new(
                "owner result transfer call is absent from the expression index",
            )
        })?;
    let call = state
        .signature_lexical_plan
        .call(expression_index)
        .ok_or_else(|| {
            OwnerConstraintSeedError::new(
                "owner result transfer call is absent from the exact signature plan",
            )
        })?;
    let target = match &call.target {
        crate::OwnerSignatureCallTarget::Owner { owner } => OwnerResidualCallTarget::Owner {
            owner: owner.clone(),
        },
        crate::OwnerSignatureCallTarget::Authoritative => {
            let contract = abi.callable(function).ok_or_else(|| {
                OwnerConstraintSeedError::new(format!(
                    "owner result transfer {:?} resolved `{function}` as authoritative without a frozen ABI contract",
                    state.seed.owner
                ))
            })?;
            OwnerResidualCallTarget::Abi {
                canonical_name: function.clone(),
                contract: OwnerResidualAbiContract::from(contract),
            }
        }
        crate::OwnerSignatureCallTarget::Ambiguous { candidates } => {
            OwnerResidualCallTarget::Ambiguous {
                candidates: candidates.clone(),
            }
        }
        crate::OwnerSignatureCallTarget::Unresolved => OwnerResidualCallTarget::Unresolved,
    };
    Ok(Some(target))
}

fn owner_result_parameter_alias(
    state: &OwnerSolveState<'_>,
    reference: u32,
) -> Option<OwnerResidualParameterRead> {
    fn append_projection(
        mut read: OwnerResidualParameterRead,
        projection: &[String],
    ) -> OwnerResidualParameterRead {
        let mut path = read.projection.into_vec();
        path.extend(projection.iter().cloned());
        read.projection = path.into_boxed_slice();
        read
    }

    fn resolve(
        state: &OwnerSolveState<'_>,
        reference: u32,
        active: &mut BTreeSet<u32>,
    ) -> Option<OwnerResidualParameterRead> {
        let expression = state.seed.expressions.get(reference as usize)?;
        if !active.insert(reference) {
            return None;
        }
        let result = if let Some(read) = state
            .signature_lexical_plan
            .reads()
            .get(reference as usize)
            .and_then(Option::as_ref)
        {
            let target = match &read.target {
                OwnerEffectiveLexicalTarget::Static {
                    target: OwnerLexicalDeclarationTarget::Parameter { ordinal },
                } => Some(OwnerResidualParameterRead {
                    parameter_ordinal: *ordinal,
                    projection: Box::new([]),
                }),
                OwnerEffectiveLexicalTarget::Static {
                    target: OwnerLexicalDeclarationTarget::Statement { statement },
                } => state
                    .seed
                    .statement_values
                    .iter()
                    .find(|(candidate, _)| candidate == statement)
                    .and_then(|(_, value)| resolve(state, *value, active)),
                OwnerEffectiveLexicalTarget::Static {
                    target:
                        OwnerLexicalDeclarationTarget::RecordField {
                            object, ordinal, ..
                        },
                } => state
                    .seed
                    .expressions
                    .get(*object as usize)
                    .and_then(|object| object.inputs.get(*ordinal as usize))
                    .and_then(|input| resolve(state, input.expression, active)),
                OwnerEffectiveLexicalTarget::Static {
                    target:
                        OwnerLexicalDeclarationTarget::PatternBinding { .. }
                        | OwnerLexicalDeclarationTarget::Passed
                        | OwnerLexicalDeclarationTarget::Imported { .. }
                        | OwnerLexicalDeclarationTarget::Ambiguous { .. },
                }
                | OwnerEffectiveLexicalTarget::FreshOut { .. }
                | OwnerEffectiveLexicalTarget::CallContext { .. }
                | OwnerEffectiveLexicalTarget::Imported { .. }
                | OwnerEffectiveLexicalTarget::InvalidBareBinding { .. }
                | OwnerEffectiveLexicalTarget::Ambiguous { .. } => None,
            };
            target.map(|target| append_projection(target, &read.projection))
        } else {
            match &expression.kind {
                OwnerConstraintNodeKind::Block => expression
                    .inputs
                    .iter()
                    .find(|input| matches!(input.role, OwnerConstraintEdgeRole::BlockResult))
                    .and_then(|input| resolve(state, input.expression, active)),
                _ => None,
            }
        };
        active.remove(&reference);
        result
    }

    resolve(state, reference, &mut BTreeSet::new())
}

fn owner_result_requires_occurrence_transfer(state: &OwnerSolveState<'_>) -> bool {
    state.declaration_kind == Some(OwnerDeclarationKind::Function)
        && owner_result_expression(state)
            .is_some_and(|root| owner_result_parameter_alias(state, root).is_none())
}

fn build_owner_result_transfer(
    state: &OwnerSolveState<'_>,
    abi: &OwnerInferenceAbiEnvironment,
    unifier: &mut TypeUnifier,
    alpha_variables: &mut BTreeMap<TypeVar, TypeVar>,
    next_alpha: &mut u32,
) -> Result<OwnerResidualDraft, OwnerConstraintSeedError> {
    if state.declaration_kind != Some(OwnerDeclarationKind::Function) {
        return Ok(OwnerResidualDraft::Principal);
    }
    let Some(root) = owner_result_expression(state) else {
        return Ok(OwnerResidualDraft::Principal);
    };
    if let Some(read) = owner_result_parameter_alias(state, root) {
        return Ok(OwnerResidualDraft::Parameter { read });
    }
    let root_ref = owner_result_expression_ref(state, root)?;
    let mut pending = vec![root];
    let mut reachable = BTreeSet::new();
    while let Some(reference) = pending.pop() {
        if reference as usize >= state.seed.expressions.len() || !reachable.insert(reference) {
            continue;
        }
        pending.extend(
            state.seed.expressions[reference as usize]
                .inputs
                .iter()
                .map(|input| input.expression),
        );
    }
    let mut reachable = reachable.into_iter().collect::<Vec<_>>();
    reachable.sort_by(|left, right| {
        state.seed.expressions[*left as usize]
            .expression
            .cmp(&state.seed.expressions[*right as usize].expression)
    });
    let nodes = reachable
        .into_iter()
        .map(|reference| {
            let expression = &state.seed.expressions[reference as usize];
            let signature_call = state.signature_lexical_plan.call(reference as usize);
            let mut formal_by_source = BTreeMap::new();
            if let Some(call) = signature_call {
                for input in &call.matched_inputs {
                    if formal_by_source
                        .insert((input.expression, input.source), input.formal_ordinal)
                        .is_some()
                    {
                        return Err(OwnerConstraintSeedError::new(
                            "owner result transfer call repeats an exact input source",
                        ));
                    }
                }
            }
            let inputs = expression
                .inputs
                .iter()
                .map(|input| {
                    let source = match input.role {
                        OwnerConstraintEdgeRole::PipeInput => {
                            Some(crate::OwnerSignatureMatchedInputSource::PipeInput)
                        }
                        OwnerConstraintEdgeRole::CallArgument { ordinal, .. } => {
                            Some(crate::OwnerSignatureMatchedInputSource::CallArgument { ordinal })
                        }
                        OwnerConstraintEdgeRole::PipeArgument { ordinal, .. } => {
                            Some(crate::OwnerSignatureMatchedInputSource::PipeArgument { ordinal })
                        }
                        _ => None,
                    };
                    let pass_source = match input.role {
                        OwnerConstraintEdgeRole::CallPass { .. } => {
                            Some(crate::OwnerSignaturePassSource::Call)
                        }
                        OwnerConstraintEdgeRole::PipePass { .. } => {
                            Some(crate::OwnerSignaturePassSource::Pipe)
                        }
                        _ => None,
                    };
                    Ok(OwnerResidualInput {
                        role: input.role.clone(),
                        expression: owner_result_expression_ref(state, input.expression)?,
                        formal_ordinal: source.and_then(|source| {
                            formal_by_source.get(&(input.expression, source)).copied()
                        }),
                        explicit_pass: signature_call
                            .and_then(|call| call.explicit_pass.as_ref())
                            .is_some_and(|pass| {
                                pass.expression == input.expression
                                    && pass_source == Some(pass.source)
                            }),
                    })
                })
                .collect::<Result<Vec<_>, OwnerConstraintSeedError>>()?;
            let variable = state.expressions[reference as usize];
            Ok(OwnerResidualNode {
                expression: expression.expression.clone(),
                flow_type: FlowType {
                    mode: state.modes[reference as usize].unwrap_or(FlowMode::Continuous),
                    ty: alpha_normalize_type(
                        &unifier.resolve(&Type::Var(variable)),
                        alpha_variables,
                        next_alpha,
                    ),
                },
                static_number: state
                    .seed
                    .result_static_numbers
                    .binary_search_by(|number| number.expression.cmp(&expression.expression))
                    .ok()
                    .and_then(|index| state.seed.result_static_numbers.get(index))
                    .map(|number| number.literal.clone()),
                kind: expression.kind.clone(),
                inputs: inputs.into_boxed_slice(),
                parameter_read: owner_result_parameter_read(state, expression),
                call_target: owner_result_call_target(state, abi, expression)?,
            })
        })
        .collect::<Result<Vec<_>, OwnerConstraintSeedError>>()?;
    Ok(OwnerResidualDraft::Expression {
        root: root_ref,
        nodes: nodes.into_boxed_slice(),
    })
}

fn alpha_normalized_resolved_type(
    unifier: &mut TypeUnifier,
    variable: TypeVar,
    variables: &mut BTreeMap<TypeVar, TypeVar>,
    next: &mut u32,
) -> Type {
    let resolved = unifier.resolve(&Type::Var(variable));
    alpha_normalize_type(&resolved, variables, next)
}

/// Write the exact alpha-equivalent in-process convergence surface.
///
/// One shared normalization frame spans every owner and slot so correlations
/// remain observable, while fresh invocation-owned alpha IDs do not manufacture
/// a semantic change. The ordered state/parameter/external inventories are
/// immutable for one solve, so direct `Type` equality is an exact comparison and
/// cannot be hidden by a digest collision.
fn write_solver_surface_snapshot(
    unifier: &mut TypeUnifier,
    states: &BTreeMap<StableCheckOwnerKey, OwnerSolveState<'_>>,
    output: &mut Vec<Type>,
) {
    output.clear();
    let mut variables = BTreeMap::new();
    let mut next = 0;
    for state in states.values() {
        for parameter in &state.parameters {
            output.push(alpha_normalized_resolved_type(
                unifier,
                parameter.variable,
                &mut variables,
                &mut next,
            ));
        }
        for variable in [state.result, state.result_flush, state.context] {
            output.push(alpha_normalized_resolved_type(
                unifier,
                variable,
                &mut variables,
                &mut next,
            ));
        }
        for (_, variable) in state
            .seed
            .external_expressions
            .iter()
            .zip(&state.external_expressions)
            .filter(|(external, _)| external.is_exact_enclosing_capture_for(&state.seed.owner))
        {
            output.push(alpha_normalized_resolved_type(
                unifier,
                *variable,
                &mut variables,
                &mut next,
            ));
        }
    }
}

fn owner_containment_depth(owner: &StableCheckOwnerKey) -> usize {
    match owner {
        StableCheckOwnerKey::UnitRoot(_) => 0,
        StableCheckOwnerKey::Item(owner) => owner.item_route.segments().len(),
    }
}

fn insert_unique_projection_authority(
    authorities: &mut BTreeMap<TypeVar, Type>,
    ambiguous: &mut BTreeSet<TypeVar>,
    root: TypeVar,
    authority: Type,
) {
    if ambiguous.contains(&root) {
        return;
    }
    match authorities.get(&root) {
        None => {
            authorities.insert(root, authority);
        }
        Some(existing) if existing == &authority => {}
        Some(_) => {
            authorities.remove(&root);
            ambiguous.insert(root);
        }
    }
}

/// Compose closed child-owner endpoints into their containing public records
/// without mutating the inference graph.
///
/// Child items are projected before their containing items. Every internal
/// child result and its parent-owned external expression share a persistent
/// root, but a late result-transfer publication may close only the child's
/// exact expression endpoint. Recording that closed endpoint here lets final
/// artifact projection follow it through the parent record. Conflicting
/// authorities for a coalesced root are deliberately ignored rather than
/// selecting a traversal-order winner.
fn closed_public_result_projection_authorities(
    states: &BTreeMap<StableCheckOwnerKey, OwnerSolveState<'_>>,
    unifier: &mut TypeUnifier,
) -> (
    BTreeMap<TypeVar, Type>,
    BTreeMap<StableCheckOwnerKey, FlowType>,
) {
    let mut owners = states.keys().collect::<Vec<_>>();
    owners.sort_by(|left, right| {
        owner_containment_depth(right)
            .cmp(&owner_containment_depth(left))
            .then_with(|| left.cmp(right))
    });

    let mut authorities = BTreeMap::new();
    let mut ambiguous = BTreeSet::new();
    let mut owner_results = BTreeMap::<StableCheckOwnerKey, FlowType>::new();
    for owner in owners {
        let state = &states[owner];
        for (external, variable) in state
            .seed
            .external_expressions
            .iter()
            .zip(&state.external_expressions)
        {
            if external.is_exact_enclosing_capture_for(&state.seed.owner) {
                continue;
            }
            let Some(authority) = owner_results.get(&external.owner) else {
                continue;
            };
            insert_unique_projection_authority(
                &mut authorities,
                &mut ambiguous,
                unifier.root(*variable),
                authority.ty.clone(),
            );
        }

        let Some(expression) = owner_result_expression(state) else {
            continue;
        };
        let raw_mode = state
            .modes
            .get(expression as usize)
            .copied()
            .flatten()
            .unwrap_or(FlowMode::Continuous);
        let authority = resolved_expression_boundary_with_projection_authorities(
            state,
            expression,
            unifier,
            raw_mode,
            &authorities,
        );
        if !boon_checked::type_is_recursively_closed(&authority.ty) {
            continue;
        }
        insert_unique_projection_authority(
            &mut authorities,
            &mut ambiguous,
            unifier.root(state.result),
            authority.ty.clone(),
        );
        owner_results.insert(owner.clone(), authority);
    }
    (authorities, owner_results)
}

fn project_owner_interface_scc_result(
    scc: &OwnerInterfaceScc,
    abi: &OwnerInferenceAbiEnvironment,
    states: &BTreeMap<StableCheckOwnerKey, OwnerSolveState<'_>>,
    unifier: &mut TypeUnifier,
    mut work: OwnerInterfaceSolveWork,
) -> Result<OwnerInterfaceSccProjection, OwnerConstraintSeedError> {
    let trace =
        std::env::var_os("BOON_OWNER_REQUEST_TRACE").is_some() && scc.key.members.len() >= 100;
    let total_started = Instant::now();
    let mut interfaces = Vec::with_capacity(states.len());
    let mut alpha_variables = BTreeMap::new();
    let mut next_alpha = 0;
    let (projection_authorities, closed_owner_results) =
        closed_public_result_projection_authorities(states, unifier);
    let authorities_ms = total_started.elapsed().as_secs_f64() * 1_000.0;
    let interfaces_started = Instant::now();
    for owner in &scc.key.members {
        let state = &states[owner];
        let parameters = state
            .parameters
            .iter()
            .map(|parameter| {
                let ty = unifier.resolve(&Type::Var(parameter.variable));
                OwnerInterfaceParameter {
                    name: parameter.name.clone(),
                    kind: parameter.kind,
                    ordinal: parameter.ordinal,
                    flow_type: FlowType {
                        mode: FlowMode::Continuous,
                        ty: alpha_normalize_type(&ty, &mut alpha_variables, &mut next_alpha),
                    },
                    requirement: CheckedParameterRequirement::Required,
                    evaluation_scope: parameter.evaluation_scope,
                }
            })
            .collect::<Vec<_>>();
        let result_expression = owner_result_expression(state);
        let raw_result_mode = result_expression
            .and_then(|expression| {
                ((expression as usize) < state.expressions.len())
                    .then(|| state.modes[expression as usize])
                    .flatten()
            })
            .unwrap_or(FlowMode::Continuous);
        let resolved_result = closed_owner_results.get(owner).cloned().unwrap_or_else(|| {
            result_expression.map_or(
                FlowType {
                    mode: FlowMode::Continuous,
                    ty: Type::Absent,
                },
                |expression| {
                    resolved_expression_boundary_with_projection_authorities(
                        state,
                        expression,
                        unifier,
                        raw_result_mode,
                        &projection_authorities,
                    )
                },
            )
        });
        let resolved_result_flush_type = result_expression.and_then(|expression| {
            resolved_expression_flush_type_with_projection_authorities(
                state,
                expression,
                unifier,
                &projection_authorities,
            )
        });
        let result = FlowType {
            mode: resolved_result.mode,
            ty: alpha_normalize_type(&resolved_result.ty, &mut alpha_variables, &mut next_alpha),
        };
        let result_flush_type = resolved_result_flush_type
            .map(|ty| alpha_normalize_type(&ty, &mut alpha_variables, &mut next_alpha));
        let context_ty = unifier.resolve(&Type::Var(state.context));
        let context = (state.declaration_kind == Some(OwnerDeclarationKind::Function)
            && !matches!(context_ty, Type::Var(_) | Type::Unknown))
        .then(|| {
            let flow_type = FlowType {
                mode: FlowMode::Continuous,
                ty: alpha_normalize_type(&context_ty, &mut alpha_variables, &mut next_alpha),
            };
            OwnerContextInterface {
                projections: boon_checked::context_scheme_projections(&flow_type.ty)
                    .into_iter()
                    .map(Vec::into_boxed_slice)
                    .collect::<Vec<_>>()
                    .into_boxed_slice(),
                flow_type,
            }
        });
        let captures = state
            .seed
            .external_expressions
            .iter()
            .filter(|external| external.is_exact_enclosing_capture_for(&state.seed.owner))
            .map(|external| {
                let provider = &states[&external.owner];
                let expression = *provider
                    .expression_by_key
                    .get(&external.expression)
                    .expect("validated enclosing capture has a provider expression");
                let mode = provider.modes[expression].unwrap_or(FlowMode::Continuous);
                let flow_type = resolved_expression_boundary_with_projection_authorities(
                    provider,
                    expression as u32,
                    unifier,
                    mode,
                    &projection_authorities,
                );
                let flush_type = resolved_expression_flush_type_with_projection_authorities(
                    provider,
                    expression as u32,
                    unifier,
                    &projection_authorities,
                );
                OwnerInterfaceCapture {
                    owner: external.owner.clone(),
                    expression: external.expression.clone(),
                    flow_type: FlowType {
                        mode: flow_type.mode,
                        ty: alpha_normalize_type(
                            &flow_type.ty,
                            &mut alpha_variables,
                            &mut next_alpha,
                        ),
                    },
                    flush_type: flush_type
                        .map(|ty| alpha_normalize_type(&ty, &mut alpha_variables, &mut next_alpha)),
                }
            })
            .collect::<Vec<_>>();
        let lexical_captures = state
            .lexical_capture_variables
            .iter()
            .map(|(target, variable)| {
                let mode = state
                    .lexical_capture_modes
                    .get(target)
                    .copied()
                    .flatten()
                    .ok_or_else(|| {
                        OwnerConstraintSeedError::new(format!(
                            "owner interface {owner:?} did not resolve lexical capture mode for {target:?}"
                        ))
                    })?;
                Ok(OwnerInterfaceLexicalCapture {
                    target: target.clone(),
                    demand_paths: state
                        .signature_lexical_plan
                        .imported_capture_sites_for(target)
                        .ok_or_else(|| {
                            OwnerConstraintSeedError::new(
                                "owner interface lexical capture has no exact demand manifest",
                            )
                        })?
                        .demand_paths
                        .clone(),
                    flow_type: FlowType {
                        mode,
                        ty: alpha_normalize_type(
                            &unifier.resolve(&Type::Var(*variable)),
                            &mut alpha_variables,
                            &mut next_alpha,
                        ),
                    },
                })
            })
            .collect::<Result<Vec<_>, OwnerConstraintSeedError>>()?;
        let mut type_variables = BTreeSet::new();
        for parameter in &parameters {
            collect_type_variables(&parameter.flow_type.ty, &mut type_variables);
        }
        collect_type_variables(&result.ty, &mut type_variables);
        if let Some(flush_type) = &result_flush_type {
            collect_type_variables(flush_type, &mut type_variables);
        }
        if let Some(context) = &context {
            collect_type_variables(&context.flow_type.ty, &mut type_variables);
        }
        for capture in &captures {
            collect_type_variables(&capture.flow_type.ty, &mut type_variables);
            if let Some(flush_type) = &capture.flush_type {
                collect_type_variables(flush_type, &mut type_variables);
            }
        }
        for capture in &lexical_captures {
            collect_type_variables(&capture.flow_type.ty, &mut type_variables);
        }
        let mut interface = OwnerPublicInterface {
            owner: owner.clone(),
            declaration_kind: state.declaration_kind,
            names: state.names.clone(),
            parameters: parameters.into_boxed_slice(),
            result,
            result_flush_type,
            captures: captures.into_boxed_slice(),
            lexical_captures: lexical_captures.into_boxed_slice(),
            context,
            effect: state.effect,
            type_variables: type_variables
                .into_iter()
                .collect::<Vec<_>>()
                .into_boxed_slice(),
            fingerprint_v1: [0; 32],
        };
        interface.fingerprint_v1 = boon_contract::canonical_serde_hash_v1(
            OWNER_BODY_INTERFACE_IMPORT_DOMAIN_V6,
            &interface,
        )
        .map_err(|error| {
            OwnerConstraintSeedError::new(format!(
                "cannot fingerprint owner body interface import: {error}"
            ))
        })?;
        interfaces.push(interface);
    }
    let interfaces_ms = interfaces_started.elapsed().as_secs_f64() * 1_000.0;
    let result_started = Instant::now();
    let public_type_variable_count = next_alpha;
    work.unification_steps = unifier.steps;
    let interface_fingerprints = interfaces
        .iter()
        .map(OwnerPublicInterface::fingerprint_v1)
        .collect::<Vec<_>>();
    let fingerprint_v1 = boon_contract::canonical_serde_hash_v1(
        OWNER_INTERFACE_SCC_RESULT_DOMAIN_V8,
        &(
            &scc.key,
            &interface_fingerprints,
            public_type_variable_count,
        ),
    )
    .map_err(|error| {
        OwnerConstraintSeedError::new(format!(
            "cannot fingerprint owner interface SCC result: {error}"
        ))
    })?;
    let key_fingerprint_v1 =
        boon_contract::canonical_serde_hash_v1(OWNER_INTERFACE_SCC_KEY_DOMAIN_V1, &scc.key)
            .map_err(|error| {
                OwnerConstraintSeedError::new(format!(
                    "cannot fingerprint owner interface SCC key: {error}"
                ))
            })?;
    let result = Arc::new(OwnerInterfaceSccResult {
        key: scc.key.clone(),
        owners: interfaces.into_boxed_slice(),
        type_variable_count: public_type_variable_count,
        work,
        key_fingerprint_v1,
        fingerprint_v1,
    });
    let result_ms = result_started.elapsed().as_secs_f64() * 1_000.0;
    let residuals_started = Instant::now();
    let residuals = scc
        .key
        .members
        .iter()
        .map(|owner| {
            build_owner_result_transfer(
                &states[owner],
                abi,
                unifier,
                &mut alpha_variables,
                &mut next_alpha,
            )
        })
        .collect::<Result<Vec<_>, _>>()?
        .into_boxed_slice();
    let residuals_ms = residuals_started.elapsed().as_secs_f64() * 1_000.0;
    if trace {
        eprintln!(
            "boon owner interface projection members={} authorities_ms={authorities_ms:.3} interfaces_ms={interfaces_ms:.3} result_ms={result_ms:.3} residuals_ms={residuals_ms:.3} total_ms={:.3}",
            scc.key.members.len(),
            total_started.elapsed().as_secs_f64() * 1_000.0,
        );
    }
    Ok(OwnerInterfaceSccProjection {
        result,
        residuals,
        residual_type_variable_count: next_alpha,
    })
}

pub(crate) fn authoritative_signature(
    function: &str,
    builtins: &BuiltinSignatureRegistry,
    render: &RenderContractRegistry,
) -> Option<AuthoritativeCallableSignature> {
    if let Some(signature) = builtins.authoritative_signature(function) {
        return Some(signature);
    }
    if let Some((_, signature)) = render
        .authoritative_signatures()
        .find(|(name, _)| *name == function)
    {
        return Some(signature);
    }
    if let Some(signature) = host_effect_signature(function) {
        return Some(AuthoritativeCallableSignature {
            parameters: signature
                .intent_fields
                .into_iter()
                .map(|field| AuthoritativeParameter {
                    name: field.name,
                    kind: CheckedParameterKind::Value,
                    flow_type: FlowType {
                        mode: FlowMode::Continuous,
                        ty: field.ty,
                    },
                    requirement: field.default.map_or(
                        boon_checked::CheckedParameterRequirement::Required,
                        |default| boon_checked::CheckedParameterRequirement::Optional { default },
                    ),
                })
                .collect(),
            call_contexts: Vec::new(),
            result: FlowType {
                mode: FlowMode::Continuous,
                ty: signature.result_type,
            },
            effect: CheckedEffectSummary {
                invokes_host: true,
                ..CheckedEffectSummary::default()
            },
            contextual_builtin: None,
        });
    }
    if let Some(result) = session_info_intrinsic_type(function) {
        return Some(AuthoritativeCallableSignature {
            parameters: Vec::new(),
            call_contexts: Vec::new(),
            result: FlowType {
                mode: FlowMode::Continuous,
                ty: result,
            },
            effect: CheckedEffectSummary::default(),
            contextual_builtin: None,
        });
    }
    function
        .strip_prefix("Field/")
        .map(|_| AuthoritativeCallableSignature {
            parameters: vec![AuthoritativeParameter {
                name: "input".to_owned(),
                kind: CheckedParameterKind::Value,
                flow_type: FlowType {
                    mode: FlowMode::Continuous,
                    ty: Type::Unknown,
                },
                requirement: boon_checked::CheckedParameterRequirement::Required,
            }],
            call_contexts: Vec::new(),
            result: FlowType {
                mode: FlowMode::Continuous,
                ty: Type::Unknown,
            },
            effect: CheckedEffectSummary::default(),
            contextual_builtin: None,
        })
}

/// Whether the default compiler ABI owns a callable name independently of
/// project symbol resolution. The exact signature is still fingerprinted by
/// the consuming owner-body request.
pub fn is_authoritative_callable_name(function: &str) -> bool {
    authoritative_signature(
        function,
        &BuiltinSignatureRegistry::default(),
        &RenderContractRegistry::default(),
    )
    .is_some()
}

pub(crate) fn instantiate_type(
    ty: &Type,
    unifier: &mut TypeUnifier,
    variables: &mut BTreeMap<TypeVar, TypeVar>,
) -> Type {
    if !type_contains_inference_variable(ty) {
        return ty.clone();
    }
    match ty {
        Type::Var(variable) => Type::Var(
            *variables
                .entry(*variable)
                .or_insert_with(|| unifier.fresh()),
        ),
        Type::Object(shape) => Type::object(ObjectShape {
            fields: shape
                .fields
                .iter()
                .map(|(name, ty)| (name.clone(), instantiate_type(ty, unifier, variables)))
                .collect(),
            field_order: shape.field_order.clone(),
            open: shape.open,
        }),
        Type::List(item) => Type::List(Type::shared(instantiate_type(item, unifier, variables))),
        Type::Set(item) => Type::Set(Type::shared(instantiate_type(item, unifier, variables))),
        Type::Map { key, value } => Type::Map {
            key: Box::new(instantiate_type(key, unifier, variables)),
            value: Box::new(instantiate_type(value, unifier, variables)),
        },
        Type::Function { args, result } => Type::Function {
            args: args
                .iter()
                .map(|argument| instantiate_type(argument, unifier, variables))
                .collect(),
            result: Box::new(FlowType {
                mode: result.mode,
                ty: instantiate_type(&result.ty, unifier, variables),
            }),
        },
        Type::VariantSet(variants) => Type::VariantSet(
            variants
                .iter()
                .map(|variant| match variant {
                    Variant::Tag(tag) => Variant::Tag(tag.clone()),
                    Variant::Tagged { tag, fields } => Variant::Tagged {
                        tag: tag.clone(),
                        fields: ObjectShape {
                            fields: fields
                                .fields
                                .iter()
                                .map(|(name, ty)| {
                                    (name.clone(), instantiate_type(ty, unifier, variables))
                                })
                                .collect(),
                            field_order: fields.field_order.clone(),
                            open: fields.open,
                        }
                        .into(),
                    },
                })
                .collect(),
        ),
        Type::Union(members) => Type::Union(
            members
                .iter()
                .map(|member| instantiate_type(member, unifier, variables))
                .collect(),
        ),
        Type::Text
        | Type::Number
        | Type::Bytes(_)
        | Type::Bits { .. }
        | Type::Absent
        | Type::RenderContract
        | Type::UnresolvedShape { .. }
        | Type::Unknown => ty.clone(),
    }
}

fn collect_type_variables(ty: &Type, variables: &mut BTreeSet<TypeVar>) {
    match ty {
        Type::Var(variable) => {
            variables.insert(*variable);
        }
        Type::Object(shape) => {
            for ty in shape.fields.values() {
                collect_type_variables(ty, variables);
            }
        }
        Type::List(item) | Type::Set(item) => collect_type_variables(item, variables),
        Type::Map { key, value } => {
            collect_type_variables(key, variables);
            collect_type_variables(value, variables);
        }
        Type::Function { args, result } => {
            for ty in args {
                collect_type_variables(ty, variables);
            }
            collect_type_variables(&result.ty, variables);
        }
        Type::VariantSet(variants) => {
            for variant in variants {
                if let Variant::Tagged { fields, .. } = variant {
                    for ty in fields.fields.values() {
                        collect_type_variables(ty, variables);
                    }
                }
            }
        }
        Type::Union(members) => {
            for ty in members {
                collect_type_variables(ty, variables);
            }
        }
        Type::Text
        | Type::Number
        | Type::Bytes(_)
        | Type::Bits { .. }
        | Type::Absent
        | Type::RenderContract
        | Type::UnresolvedShape { .. }
        | Type::Unknown => {}
    }
}

pub(crate) fn alpha_normalize_type(
    ty: &Type,
    variables: &mut BTreeMap<TypeVar, TypeVar>,
    next: &mut u32,
) -> Type {
    if !type_contains_inference_variable(ty) {
        return ty.clone();
    }
    match ty {
        Type::Var(variable) => Type::Var(*variables.entry(*variable).or_insert_with(|| {
            let normalized = TypeVar(*next);
            *next = next.saturating_add(1);
            normalized
        })),
        Type::Object(shape) => Type::object(ObjectShape {
            fields: shape
                .fields
                .iter()
                .map(|(name, ty)| (name.clone(), alpha_normalize_type(ty, variables, next)))
                .collect(),
            field_order: shape.field_order.clone(),
            open: shape.open,
        }),
        Type::List(item) => Type::List(Type::shared(alpha_normalize_type(item, variables, next))),
        Type::Set(item) => Type::Set(Type::shared(alpha_normalize_type(item, variables, next))),
        Type::Map { key, value } => Type::Map {
            key: Box::new(alpha_normalize_type(key, variables, next)),
            value: Box::new(alpha_normalize_type(value, variables, next)),
        },
        Type::Function { args, result } => Type::Function {
            args: args
                .iter()
                .map(|argument| alpha_normalize_type(argument, variables, next))
                .collect(),
            result: Box::new(FlowType {
                mode: result.mode,
                ty: alpha_normalize_type(&result.ty, variables, next),
            }),
        },
        Type::VariantSet(variants) => Type::VariantSet(
            variants
                .iter()
                .map(|variant| match variant {
                    Variant::Tag(tag) => Variant::Tag(tag.clone()),
                    Variant::Tagged { tag, fields } => Variant::Tagged {
                        tag: tag.clone(),
                        fields: ObjectShape {
                            fields: fields
                                .fields
                                .iter()
                                .map(|(name, ty)| {
                                    (name.clone(), alpha_normalize_type(ty, variables, next))
                                })
                                .collect(),
                            field_order: fields.field_order.clone(),
                            open: fields.open,
                        }
                        .into(),
                    },
                })
                .collect(),
        ),
        Type::Union(members) => Type::Union(
            members
                .iter()
                .map(|member| alpha_normalize_type(member, variables, next))
                .collect(),
        ),
        Type::Text
        | Type::Number
        | Type::Bytes(_)
        | Type::Bits { .. }
        | Type::Absent
        | Type::RenderContract
        | Type::UnresolvedShape { .. }
        | Type::Unknown => ty.clone(),
    }
}

/// Evaluate one dependency-first tagged interface SCC atomically.
pub fn evaluate_owner_interface_scc<'a>(
    scc: &OwnerInterfaceScc,
    abi: &OwnerInferenceAbiEnvironment,
    seeds: impl IntoIterator<Item = &'a OwnerConstraintSeed>,
    summaries: impl IntoIterator<Item = &'a OwnerConstraintSummary>,
    dependency_results: impl IntoIterator<Item = &'a OwnerInterfaceSccResult>,
) -> Result<OwnerInterfaceSccEvaluation, OwnerConstraintSeedError> {
    evaluate_owner_interface_scc_impl(scc, abi, seeds, summaries, dependency_results, None, None)
        .map(|output| output.evaluation)
}

pub fn evaluate_owner_interface_scc_with_signature_scopes<'a, 'b>(
    scc: &OwnerInterfaceScc,
    abi: &OwnerInferenceAbiEnvironment,
    seeds: impl IntoIterator<Item = &'a OwnerConstraintSeed>,
    summaries: impl IntoIterator<Item = &'a OwnerConstraintSummary>,
    dependency_results: impl IntoIterator<Item = &'a OwnerInterfaceSccResult>,
    signature_scopes: impl IntoIterator<Item = &'b OwnerCallableScopeOwnerResult>,
) -> Result<OwnerInterfaceSccEvaluation, OwnerConstraintSeedError> {
    evaluate_owner_interface_scc_impl(
        scc,
        abi,
        seeds,
        summaries,
        dependency_results,
        Some(signature_scopes.into_iter().collect()),
        None,
    )
    .map(|output| output.evaluation)
}

/// Solve one interface SCC once and run only its quiescent result-transfer
/// residuals until the public surface and compiled module stabilize.
///
/// The resolver is called once with the exact external owner set referenced by
/// the component's transfer programs or user-call occurrences. It lets a
/// persistent evaluator record precise module dependencies without rebuilding
/// the complete SCC solver in a second transaction.
pub fn evaluate_owner_interface_scc_component<'a, 'b, F>(
    scc: &OwnerInterfaceScc,
    abi: &OwnerInferenceAbiEnvironment,
    seeds: impl IntoIterator<Item = &'a OwnerConstraintSeed>,
    summaries: impl IntoIterator<Item = &'a OwnerConstraintSummary>,
    dependency_results: impl IntoIterator<Item = &'a OwnerInterfaceSccResult>,
    signature_scopes: impl IntoIterator<Item = &'b OwnerCallableScopeOwnerResult>,
    mut resolve_transfer_modules: F,
) -> Result<OwnerInterfaceSccComponentEvaluation, OwnerConstraintSeedError>
where
    F: FnMut(&[StableCheckOwnerKey]) -> Result<Vec<Arc<OwnerInterfaceTransferModule>>, String>,
{
    let output = evaluate_owner_interface_scc_impl(
        scc,
        abi,
        seeds,
        summaries,
        dependency_results,
        Some(signature_scopes.into_iter().collect()),
        Some(&mut resolve_transfer_modules),
    )?;
    Ok(OwnerInterfaceSccComponentEvaluation {
        evaluation: output.evaluation,
        module: output.module.ok_or_else(|| {
            OwnerConstraintSeedError::new(
                "component interface solve did not publish its transfer module",
            )
        })?,
        transfer_iterations: output.transfer_iterations,
        transfer_work: output.transfer_work,
    })
}

#[cfg(test)]
pub(crate) fn evaluate_owner_interface_scc_component_for_tests<'a>(
    scc: &OwnerInterfaceScc,
    abi: &OwnerInferenceAbiEnvironment,
    seeds: impl IntoIterator<Item = &'a OwnerConstraintSeed>,
    summaries: impl IntoIterator<Item = &'a OwnerConstraintSummary>,
    dependency_results: impl IntoIterator<Item = &'a OwnerInterfaceSccResult>,
    dependency_modules: impl IntoIterator<Item = Arc<OwnerInterfaceTransferModule>>,
) -> Result<OwnerInterfaceSccComponentEvaluation, OwnerConstraintSeedError> {
    let dependency_modules = dependency_modules.into_iter().collect::<Vec<_>>();
    let output = evaluate_owner_interface_scc_impl(
        scc,
        abi,
        seeds,
        summaries,
        dependency_results,
        None,
        Some(&mut |owners: &[StableCheckOwnerKey]| {
            let modules = dependency_modules
                .iter()
                .filter(|module| owners.iter().any(|owner| module.owns_owner(owner)))
                .cloned()
                .collect::<Vec<_>>();
            if owners
                .iter()
                .all(|owner| modules.iter().any(|module| module.owns_owner(owner)))
            {
                Ok(modules)
            } else {
                Err("test component resolver is missing a requested owner".to_owned())
            }
        }),
    )?;
    Ok(OwnerInterfaceSccComponentEvaluation {
        evaluation: output.evaluation,
        module: output.module.ok_or_else(|| {
            OwnerConstraintSeedError::new(
                "test component solve did not publish its residual module",
            )
        })?,
        transfer_iterations: output.transfer_iterations,
        transfer_work: output.transfer_work,
    })
}

type OwnerInterfaceTransferModuleResolver<'a> = dyn FnMut(&[StableCheckOwnerKey]) -> Result<Vec<Arc<OwnerInterfaceTransferModule>>, String>
    + 'a;

fn evaluate_owner_interface_scc_impl<'a>(
    scc: &OwnerInterfaceScc,
    abi: &OwnerInferenceAbiEnvironment,
    seeds: impl IntoIterator<Item = &'a OwnerConstraintSeed>,
    summaries: impl IntoIterator<Item = &'a OwnerConstraintSummary>,
    dependency_results: impl IntoIterator<Item = &'a OwnerInterfaceSccResult>,
    signature_scopes: Option<Vec<&OwnerCallableScopeOwnerResult>>,
    mut transfer_module_resolver: Option<&mut OwnerInterfaceTransferModuleResolver<'_>>,
) -> Result<OwnerInterfaceSccSolveOutput, OwnerConstraintSeedError> {
    let seeds = seeds
        .into_iter()
        .map(|seed| (seed.owner.clone(), seed))
        .collect::<BTreeMap<_, _>>();
    let summaries = summaries
        .into_iter()
        .map(|summary| (summary.owner.clone(), summary))
        .collect::<BTreeMap<_, _>>();
    let expected = scc.key.members.iter().cloned().collect::<BTreeSet<_>>();
    if abi.subjects().iter().cloned().collect::<BTreeSet<_>>() != expected {
        return Err(OwnerConstraintSeedError::new(
            "interface SCC inference ABI does not match its exact member set",
        ));
    }
    let expected_abi_names = summaries
        .values()
        .flat_map(|summary| summary.authoritative_abi_names().into_vec())
        .collect::<BTreeSet<_>>();
    let actual_abi_names = abi
        .lookups()
        .iter()
        .map(|lookup| lookup.canonical_name().to_owned())
        .collect::<BTreeSet<_>>();
    if actual_abi_names != expected_abi_names {
        return Err(OwnerConstraintSeedError::new(
            "interface SCC inference ABI does not match its exact callable lookup set",
        ));
    }
    let expected_value_paths = summaries
        .values()
        .flat_map(|summary| summary.authoritative_value_abi_paths().into_vec())
        .collect::<BTreeSet<_>>();
    let actual_value_paths = abi
        .value_lookups()
        .iter()
        .map(|lookup| lookup.canonical_path().to_owned())
        .collect::<BTreeSet<_>>();
    if actual_value_paths != expected_value_paths {
        return Err(OwnerConstraintSeedError::new(
            "interface SCC inference ABI does not match its exact external value lookup set",
        ));
    }
    let expected_source_payload_paths = seeds
        .values()
        .flat_map(|seed| seed.source_payload_abi_paths().into_vec())
        .collect::<BTreeSet<_>>();
    let actual_source_payload_paths = abi
        .source_payload_lookups()
        .iter()
        .map(|lookup| lookup.canonical_path().to_owned())
        .collect::<BTreeSet<_>>();
    if actual_source_payload_paths != expected_source_payload_paths {
        return Err(OwnerConstraintSeedError::new(
            "interface SCC inference ABI does not match its exact source payload lookup set",
        ));
    }
    for seed in seeds.values() {
        for query in &seed.source_payload_queries {
            if abi
                .source_payload_lookup(&query.canonical_path)
                .and_then(crate::OwnerSourcePayloadAbiLookup::payload_type)
                .is_none()
            {
                return Err(OwnerConstraintSeedError::new(format!(
                    "source `{}` has no unique payload ABI contract",
                    query.canonical_path
                )));
            }
        }
    }
    let expected_parameter_requirement_keys = seeds
        .values()
        .flat_map(|seed| seed.parameter_requirement_keys().into_vec())
        .collect::<BTreeSet<_>>();
    let actual_parameter_requirement_keys = abi
        .parameter_requirement_lookups()
        .iter()
        .map(|lookup| lookup.key().clone())
        .collect::<BTreeSet<_>>();
    if actual_parameter_requirement_keys != expected_parameter_requirement_keys {
        return Err(OwnerConstraintSeedError::new(
            "interface SCC inference ABI does not match its exact parameter requirement lookup set",
        ));
    }
    if seeds.keys().cloned().collect::<BTreeSet<_>>() != expected
        || summaries.keys().cloned().collect::<BTreeSet<_>>() != expected
    {
        return Err(OwnerConstraintSeedError::new(
            "interface SCC inputs do not match its exact member set",
        ));
    }
    for owner in &scc.key.members {
        if summaries[owner].seed_fingerprint_v1 != seeds[owner].fingerprint_v1() {
            return Err(OwnerConstraintSeedError::new(format!(
                "interface SCC owner {owner:?} has mismatched seed and summary"
            )));
        }
    }

    let dependency_results = dependency_results.into_iter().collect::<Vec<_>>();
    let mut dependency_interfaces = BTreeMap::new();
    let mut dependency_keys = BTreeSet::new();
    for result in &dependency_results {
        if !dependency_keys.insert(result.key.clone()) {
            return Err(OwnerConstraintSeedError::new(
                "interface SCC received a duplicate dependency result",
            ));
        }
        for interface in &result.owners {
            if dependency_interfaces
                .insert(interface.owner.clone(), interface.clone())
                .is_some()
            {
                return Err(OwnerConstraintSeedError::new(
                    "interface SCC dependencies publish a duplicate owner",
                ));
            }
        }
    }
    if dependency_keys != scc.dependencies.iter().cloned().collect() {
        return Err(OwnerConstraintSeedError::new(
            "interface SCC dependency results do not match its topology",
        ));
    }

    let supplied_signature_scopes = signature_scopes.is_some();
    let (signature_scopes, signature_lexical_plans) = if let Some(signature_scopes) =
        signature_scopes
    {
        let mut signatures = BTreeMap::new();
        let mut plans = BTreeMap::new();
        for scope in signature_scopes {
            let owner = scope.owner();
            if !expected.contains(owner)
                || !scope.lexical_plan().matches_seed(seeds[owner])
                || signatures
                    .insert(owner.clone(), scope.signature().clone())
                    .is_some()
                || plans
                    .insert(owner.clone(), scope.lexical_plan().clone())
                    .is_some()
            {
                return Err(OwnerConstraintSeedError::new(
                    "interface SCC received stale, duplicate, or foreign callable scope results",
                ));
            }
        }
        if signatures.keys().cloned().collect::<BTreeSet<_>>() != expected {
            return Err(OwnerConstraintSeedError::new(
                "interface SCC callable scope results do not cover every member",
            ));
        }
        for owner in &scc.key.members {
            if !plans[owner]
                .matches_signature_inputs(seeds[owner], summaries[owner], abi, |target| {
                    signatures.get(target).cloned().or_else(|| {
                        dependency_interfaces.get(target).map(|interface| {
                            OwnerCallableLexicalSignature::from_interface(interface)
                        })
                    })
                })
                .map_err(|error| {
                    OwnerConstraintSeedError::new(format!(
                        "cannot validate owner signature lexical inputs for {owner:?}: {error}"
                    ))
                })?
            {
                return Err(OwnerConstraintSeedError::new(
                    "interface SCC received a stale callable signature/lexical pair",
                ));
            }
        }
        (signatures, plans)
    } else {
        let projection = project_owner_signature_lexical_scope_plans(
            seeds.values().copied(),
            summaries.values().copied(),
            abi,
            dependency_interfaces.values(),
        )
        .map_err(|error| {
            OwnerConstraintSeedError::new(format!(
                "cannot project owner signature lexical scopes: {error}"
            ))
        })?;
        (projection.signatures().clone(), projection.plans().clone())
    };
    if supplied_signature_scopes {
        for owner in &scc.key.members {
            if summaries[owner].matches_signature_plan(&signature_lexical_plans[owner])
                && summaries[owner].matches_effective_references(
                    signature_lexical_plans[owner].external_candidates(),
                )
            {
                continue;
            }
            return Err(OwnerConstraintSeedError::new(format!(
                "interface SCC summary and exact signature lexical plan diverge for {owner:?}"
            )));
        }
    }

    let owners = scc
        .key
        .members
        .iter()
        .map(|owner| OwnerInterfaceSccOwnerBasis {
            owner: owner.clone(),
            lexical_reads_fingerprint_v1: seeds[owner].lexical_reads_fingerprint_v1(),
            signature_lexical_plan_fingerprint_v1: signature_lexical_plans
                .get(owner)
                .expect("signature scope projection covers every SCC owner")
                .fingerprint_v1(),
            seed_fingerprint_v1: seeds[owner].fingerprint_v1(),
            summary_fingerprint_v1: summaries[owner].fingerprint_v1(),
        })
        .collect::<Vec<_>>();
    let mut dependency_basis = dependency_results
        .iter()
        .map(|result| OwnerInterfaceSccDependencyBasis {
            key: result.key.clone(),
            result_fingerprint_v1: result.fingerprint_v1(),
        })
        .collect::<Vec<_>>();
    dependency_basis.sort_by(|left, right| left.key.cmp(&right.key));
    let basis = OwnerInterfaceSccBasis {
        key: scc.key.clone(),
        topology_fingerprint_v1: scc.fingerprint_v1(),
        owners: owners.into_boxed_slice(),
        dependency_results: dependency_basis.into_boxed_slice(),
        inference_abi_fingerprint_v1: abi.fingerprint_v1(),
    };
    let mut unifier = TypeUnifier::default();
    let mut hold_authorities = BTreeMap::new();
    let mut states = BTreeMap::<StableCheckOwnerKey, OwnerSolveState<'_>>::new();
    for owner in &scc.key.members {
        let seed = seeds[owner];
        let summary = summaries[owner];
        let signature = signature_scopes
            .get(owner)
            .expect("signature scope projection covers every SCC owner");
        let signature_lexical_plan = signature_lexical_plans
            .get(owner)
            .expect("signature scope projection covers every SCC owner")
            .clone();
        let public = seed
            .declarations
            .iter()
            .find(|declaration| declaration.public);
        let mut parameters = Vec::new();
        if let Some(public) = public {
            for parameter in &public.parameters {
                let variable = unifier.fresh();
                let requirement_key =
                    crate::OwnerParameterRequirementKey::new(seed.owner.clone(), parameter.ordinal);
                let requirement = abi
                    .parameter_requirement_lookup(&requirement_key)
                    .expect("parameter requirement lookup set was validated above");
                if let Some(ty) = requirement.ty() {
                    let mut variables = BTreeMap::new();
                    let ty = instantiate_type(ty, &mut unifier, &mut variables);
                    unifier.bind_var(variable, ty);
                }
                parameters.push(OwnerSolveParameter {
                    name: parameter.name.clone(),
                    kind: parameter.kind,
                    ordinal: parameter.ordinal,
                    variable,
                    evaluation_scope: signature
                        .parameters
                        .iter()
                        .find(|candidate| candidate.ordinal == parameter.ordinal)
                        .map(|candidate| candidate.evaluation_scope)
                        .ok_or_else(|| {
                            OwnerConstraintSeedError::new(format!(
                                "owner signature scope projection omits parameter {} for {owner:?}",
                                parameter.ordinal
                            ))
                        })?,
                });
            }
        }
        let expressions = (0..seed.expressions.len())
            .map(|_| unifier.fresh())
            .collect::<Vec<_>>();
        let expression_flushes = (0..seed.expressions.len())
            .map(|_| unifier.fresh())
            .collect::<Vec<_>>();
        let external_expressions = (0..seed.external_expressions.len())
            .map(|_| unifier.fresh())
            .collect::<Vec<_>>();
        let external_expression_flushes = (0..seed.external_expressions.len())
            .map(|_| unifier.fresh())
            .collect::<Vec<_>>();
        let expression_by_key = seed
            .expressions
            .iter()
            .enumerate()
            .map(|(index, expression)| (expression.expression.clone(), index))
            .collect();
        let result = unifier.fresh();
        let result_flush = unifier.fresh();
        let context = unifier.fresh();
        let mut effect = CheckedEffectSummary::default();
        if seed.effect_seed.declares_source {
            effect.emits_source = true;
        }
        if seed.effect_seed.declares_state || seed.effect_seed.declares_list {
            effect.reads_state = true;
            effect.writes_state = true;
        }
        let mut signature_declaration_variables = BTreeMap::new();
        for declaration in signature_lexical_plan.declarations() {
            if signature_declaration_variables
                .insert(declaration.target.clone(), unifier.fresh())
                .is_some()
            {
                return Err(OwnerConstraintSeedError::new(format!(
                    "owner signature lexical plan repeats a dynamic declaration for {owner:?}"
                )));
            }
        }
        let inherited_pattern_plans = inherited_pattern_read_plans(&signature_lexical_plan);
        let signature_dynamic_expressions =
            signature_dynamic_expression_index(seed, &signature_lexical_plan);
        let mut pattern_local_expressions =
            exact_pattern_local_expressions(seed, &signature_lexical_plan);
        pattern_local_expressions.extend(
            inherited_pattern_plans
                .iter()
                .flat_map(|plan| plan.reads.iter().map(|read| read.expression)),
        );
        let mut signature_read_expressions =
            BTreeMap::<OwnerSignatureDeclarationTarget, Vec<usize>>::new();
        for (index, read) in signature_lexical_plan.reads().iter().enumerate() {
            let target = match read.as_ref().map(|read| &read.target) {
                Some(OwnerEffectiveLexicalTarget::FreshOut {
                    call,
                    formal_ordinal,
                }) => Some(OwnerSignatureDeclarationTarget::FreshOut {
                    call: call.clone(),
                    formal_ordinal: *formal_ordinal,
                }),
                Some(OwnerEffectiveLexicalTarget::CallContext {
                    call,
                    context_ordinal,
                }) => Some(OwnerSignatureDeclarationTarget::CallContext {
                    call: call.clone(),
                    context_ordinal: *context_ordinal,
                }),
                _ => None,
            };
            if let Some(target) = target
                && !pattern_local_expressions.contains(&(index as u32))
            {
                signature_read_expressions
                    .entry(target)
                    .or_default()
                    .push(index);
            }
        }
        states.insert(
            owner.clone(),
            OwnerSolveState {
                seed,
                summary,
                signature_lexical_plan,
                signature_declaration_variables,
                lexical_declaration_variables: BTreeMap::new(),
                lexical_capture_variables: BTreeMap::new(),
                lexical_capture_reads: BTreeMap::new(),
                lexical_capture_read_variables: vec![None; seed.expressions.len()],
                lexical_capture_modes: BTreeMap::new(),
                signature_declaration_modes: BTreeMap::new(),
                signature_read_expressions,
                signature_dynamic_expressions,
                inherited_pattern_plans,
                pattern_local_expressions,
                declaration_kind: public.map(|declaration| declaration.kind),
                names: public
                    .map(|declaration| declaration.names.clone())
                    .unwrap_or_default(),
                parameters,
                context,
                result,
                result_flush,
                expressions,
                expression_flushes,
                call_flushes: vec![None; seed.expressions.len()],
                expression_by_key,
                external_expressions,
                external_expression_flushes,
                planned_lexical_reads: Vec::new(),
                modes: vec![None; seed.expressions.len()],
                effect,
            },
        );
    }

    for state in states.values() {
        mark_owner_derived_providers(&mut unifier, state.seed, &state.expressions);
    }

    // Project exact per-expression local bindings from the shared lexical
    // authority before solving constraints. This preserves whole-scope
    // shadowing without an owner-wide name map.
    for state in states.values_mut() {
        initialize_lexical_declaration_variables(state, &mut unifier)?;
        state.planned_lexical_reads = planned_lexical_read_variables(state)?;
    }

    // Private lexical captures are solved jointly with their declaring owner.
    // A public declaration, however, already has a complete dependency
    // interface and remains an ordinary one-way import; forcing a reverse edge
    // for every BLOCK sibling read would couple unrelated siblings into one
    // SCC and invalidate the provider on consumer-only edits.
    let lexical_capture_bindings = states
        .iter()
        .flat_map(|(consumer, state)| {
            state
                .lexical_capture_variables
                .iter()
                .map(move |(target, capture)| (consumer.clone(), target.clone(), *capture))
        })
        .collect::<Vec<_>>();
    let mut internal_lexical_capture_providers = Vec::new();
    for (consumer, target, capture) in lexical_capture_bindings {
        let provider_owner = match &target {
            OwnerLexicalTargetRef::Declaration { owner, .. }
            | OwnerLexicalTargetRef::ContextFormal { owner } => owner,
            OwnerLexicalTargetRef::Ambiguous { .. } => {
                return Err(OwnerConstraintSeedError::new(
                    "ambiguous lexical target cannot enter interface capture solving",
                ));
            }
        };
        if let Some(provider) = states.get(provider_owner) {
            let provider = provider
                .lexical_declaration_variables
                .get(&target)
                .copied()
                .ok_or_else(|| {
                    OwnerConstraintSeedError::new(format!(
                        "owner interface {consumer:?} captures missing lexical target {target:?}",
                    ))
                })?;
            let demand_paths = states[&consumer]
                .signature_lexical_plan
                .imported_capture_sites_for(&target)
                .ok_or_else(|| {
                    OwnerConstraintSeedError::new(
                        "interface lexical capture has no exact demand manifest",
                    )
                })?
                .demand_paths
                .clone();
            // Cross-owner lexical capture is a provider-to-consumer flow, not
            // a type equation. Keep the consumer root independent so a local
            // projection or use-site contract cannot widen the declaring
            // PatternBinding/FreshOut/public surface before it is frozen.
            internal_lexical_capture_providers.push(InternalLexicalCaptureProvider {
                consumer: consumer.clone(),
                target: target.clone(),
                capture,
                provider,
                demand: LexicalCaptureDemand::from_paths(&demand_paths)?,
            });
            continue;
        }
        if !matches!(
            &target,
            OwnerLexicalTargetRef::Declaration {
                declaration: OwnerDeclarationStableKey::Public,
                ..
            }
        ) {
            return Err(OwnerConstraintSeedError::new(format!(
                "owner interface {consumer:?} captures private lexical target from {provider_owner:?} outside its interface SCC"
            )));
        }
        let interface = dependency_interfaces.get(provider_owner).ok_or_else(|| {
            OwnerConstraintSeedError::new(format!(
                "owner interface {consumer:?} has no dependency interface for public lexical target {target:?}"
            ))
        })?;
        let demand_paths = states[&consumer]
            .signature_lexical_plan
            .imported_capture_sites_for(&target)
            .ok_or_else(|| {
                OwnerConstraintSeedError::new(
                    "interface public lexical capture has no exact demand manifest",
                )
            })?
            .demand_paths
            .clone();
        let demand = LexicalCaptureDemand::from_paths(&demand_paths)?;
        let mut variables = BTreeMap::new();
        let ty = instantiate_type(
            &demand.project_resolved(&interface.result.ty),
            &mut unifier,
            &mut variables,
        );
        unifier.bind_var(capture, ty);
        let mode = states
            .get_mut(&consumer)
            .and_then(|state| state.lexical_capture_modes.get_mut(&target))
            .ok_or_else(|| {
                OwnerConstraintSeedError::new(
                    "interface public lexical capture has no reserved mode slot",
                )
            })?;
        *mode = Some(interface.result.mode);
    }
    let mut lexical_capture_type_variables = BTreeMap::new();
    let mut lexical_capture_provider_types = BTreeMap::new();
    propagate_lexical_capture_types(
        &states,
        &internal_lexical_capture_providers,
        &mut lexical_capture_type_variables,
        &mut lexical_capture_provider_types,
        false,
        &mut unifier,
    )?;
    let mut inherited_pattern_narrowings = Vec::new();
    for state in states.values() {
        inherited_pattern_narrowings.extend(
            instantiate_owner_inherited_pattern_narrowings(
                &state.inherited_pattern_plans,
                |target| state.lexical_capture_variables.get(target).copied(),
                &state.expressions,
                &mut unifier,
            )
            .map_err(|error| {
                OwnerConstraintSeedError::new(format!(
                    "cannot instantiate inherited pattern narrowing for {:?}: {error}",
                    state.seed.owner
                ))
            })?,
        );
    }
    // A transparent result wrapper is part of the public type equation, not
    // merely result-transfer metadata.  In particular, BLOCK-local aliases
    // can be projected correctly even when their lexical reference is not a
    // unique owner-wide root.  Freeze that exact equation before solving the
    // remaining expression graph so an alpha-equivalent wrapper cannot split
    // a callable's parameter and result variables.
    for state in states.values() {
        let Some(root) = owner_result_expression(state) else {
            continue;
        };
        let Some(read) = owner_result_parameter_alias(state, root) else {
            continue;
        };
        let Some(parameter) = state
            .parameters
            .iter()
            .find(|parameter| parameter.ordinal == read.parameter_ordinal)
        else {
            return Err(OwnerConstraintSeedError::new(format!(
                "owner interface {:?} result aliases absent parameter ordinal {}",
                state.seed.owner, read.parameter_ordinal
            )));
        };
        let projected = bind_projection(&mut unifier, parameter.variable, &read.projection);
        if let Some(result) = expression_variable(state, root) {
            unifier.unify(Type::Var(result), Type::Var(projected));
        }
    }

    let mut calls = Vec::new();
    let mut pattern_narrowings = Vec::new();
    let mut flow_constraints = Vec::new();
    let mut work = OwnerInterfaceSolveWork {
        owners: states.len() as u64,
        ..OwnerInterfaceSolveWork::default()
    };
    for state in states.values_mut() {
        work.expressions = work
            .expressions
            .saturating_add(state.seed.expressions.len() as u64);
        let resolved = state
            .summary
            .resolved_references
            .iter()
            .map(|resolved| (resolved.reference.expression.clone(), resolved))
            .collect::<BTreeMap<_, _>>();
        let symbol_resolutions = state
            .summary
            .symbol_resolutions
            .iter()
            .map(|resolution| (resolution.reference().expression.clone(), resolution))
            .collect::<BTreeMap<_, _>>();
        for (index, expression) in state.seed.expressions.iter().enumerate() {
            let variable = state.expressions[index];
            let mut mode = Some(FlowMode::Continuous);
            match &expression.kind {
                OwnerConstraintNodeKind::Text | OwnerConstraintNodeKind::TextTemplate => {
                    unifier.bind_var(variable, Type::Text);
                }
                OwnerConstraintNodeKind::Number => unifier.bind_var(variable, Type::Number),
                OwnerConstraintNodeKind::Byte => {
                    unifier.bind_var(variable, Type::Bytes(BytesType::Fixed(1)));
                }
                OwnerConstraintNodeKind::Bits { width } => {
                    unifier.bind_var(variable, Type::Bits { width: *width });
                }
                OwnerConstraintNodeKind::Tag { name } if name == "SKIP" => {
                    unifier.bind_var(variable, Type::Absent);
                    mode = Some(FlowMode::Absent);
                }
                OwnerConstraintNodeKind::Tag { name } => unifier.bind_var(
                    variable,
                    Type::VariantSet(vec![Variant::Tag(name.clone())].into()),
                ),
                OwnerConstraintNodeKind::Source => {
                    if let Some(query) = state
                        .seed
                        .source_payload_queries
                        .iter()
                        .find(|query| query.expression == expression.expression)
                    {
                        let payload_type = abi
                            .source_payload_lookup(&query.canonical_path)
                            .and_then(crate::OwnerSourcePayloadAbiLookup::payload_type)
                            .expect("source payload lookup was validated above");
                        let mut variables = BTreeMap::new();
                        let payload_type =
                            instantiate_type(payload_type, &mut unifier, &mut variables);
                        unifier.bind_var(variable, payload_type);
                    }
                    mode = Some(FlowMode::PresentOrAbsent);
                    state.effect.emits_source = true;
                }
                OwnerConstraintNodeKind::Reference { parts }
                | OwnerConstraintNodeKind::Drain { parts } => {
                    if state.pattern_local_expressions.contains(&(index as u32)) {
                        // The owning match arm binds this occurrence against
                        // an arm-local pattern value below.
                    } else if let PlannedLexicalRead::Bound(root) =
                        state.planned_lexical_reads[index]
                    {
                        let read = state.signature_lexical_plan.reads()[index]
                            .as_ref()
                            .expect("bound lexical root must have a read plan");
                        let local = bind_projection(&mut unifier, root, &read.projection);
                        unifier.unify(Type::Var(variable), Type::Var(local));
                    } else if let PlannedLexicalRead::Imported(root) =
                        state.planned_lexical_reads[index]
                    {
                        let read = state.signature_lexical_plan.reads()[index]
                            .as_ref()
                            .expect("imported lexical root must have a read plan");
                        // Imported declarations are provider-to-consumer
                        // facts. Project through a consumer-owned copy so a
                        // child constraint cannot widen the provider's stable
                        // declaration surface while the joint SCC converges.
                        let imported = unifier.fresh();
                        unifier.bind_flow_result(imported, Type::Var(root));
                        let local = bind_projection(&mut unifier, imported, &read.projection);
                        unifier.bind_flow_result(variable, Type::Var(local));
                        mode = None;
                    } else if matches!(
                        state.planned_lexical_reads[index],
                        PlannedLexicalRead::Dynamic
                    ) {
                        // Signature-dependent roots and projections bind when
                        // this call occurrence instantiates its exact formal
                        // or context scheme below.
                    } else if matches!(
                        state.planned_lexical_reads[index],
                        PlannedLexicalRead::Reserved
                    ) {
                        // Planned ambiguous/contextless locals cannot fall
                        // through to a project or ABI symbol.
                    } else if let Some(target) = resolved.get(&expression.expression) {
                        if target.reference.kind == OwnerReferenceKind::Value {
                            // Cross-owner value reads are wired after all local
                            // interfaces exist.
                        }
                    } else if matches!(
                        symbol_resolutions.get(&expression.expression),
                        Some(OwnerSymbolResolution::Authoritative { reference })
                            if reference.kind == OwnerReferenceKind::Value
                    ) {
                        let canonical_path = boon_syntax::canonical_value_path(parts);
                        if let Some(flow_type) = abi
                            .value_lookup(&canonical_path)
                            .and_then(crate::OwnerValueAbiLookup::flow_type)
                        {
                            let mut variables = BTreeMap::new();
                            let ty = instantiate_type(&flow_type.ty, &mut unifier, &mut variables);
                            unifier.bind_var(variable, ty);
                            mode = Some(flow_type.mode);
                        }
                    }
                }
                OwnerConstraintNodeKind::Record { tag } => {
                    let fields = expression.inputs.iter().filter_map(|input| {
                        let OwnerConstraintEdgeRole::RecordField {
                            name,
                            spread: false,
                        } = &input.role
                        else {
                            return None;
                        };
                        Some((
                            name.clone(),
                            Type::Var(expression_variable(state, input.expression)?),
                        ))
                    });
                    let shape: ObjectShape = ObjectShape::from_ordered_fields(fields, false);
                    let ty = match tag {
                        Some(tag) => Type::VariantSet(
                            vec![Variant::Tagged {
                                tag: tag.clone(),
                                fields: shape.into(),
                            }]
                            .into(),
                        ),
                        None => Type::object(shape),
                    };
                    unifier.bind_var(variable, ty);
                }
                OwnerConstraintNodeKind::Flush => {
                    unifier.bind_var(variable, Type::Absent);
                }
                OwnerConstraintNodeKind::Call { function }
                | OwnerConstraintNodeKind::Pipe {
                    operation: function,
                } => {
                    if boon_effect_schema::host_effect_spec(function).is_some() {
                        state.effect.invokes_host = true;
                    }
                    let flush = unifier.fresh();
                    if state.call_flushes[index].replace(flush).is_some() {
                        return Err(OwnerConstraintSeedError::new(format!(
                            "owner interface {:?} repeats call expression {index}",
                            state.seed.owner
                        )));
                    }
                    calls.push(CrossCall {
                        caller: state.seed.owner.clone(),
                        expression: index,
                        target: resolved
                            .get(&expression.expression)
                            .filter(|resolved| {
                                resolved.reference.kind == OwnerReferenceKind::Callable
                            })
                            .map(|resolved| resolved.owner.clone()),
                        function: function.clone(),
                        stable_expression: expression.expression.clone(),
                        flush,
                    });
                    mode = None;
                }
                OwnerConstraintNodeKind::Draining => {
                    if let Some(input) = expression
                        .inputs
                        .first()
                        .and_then(|input| expression_variable(state, input.expression))
                    {
                        bind_and_record_flow_variables(
                            &mut unifier,
                            &mut flow_constraints,
                            variable,
                            [input],
                        );
                    }
                    mode = None;
                }
                OwnerConstraintNodeKind::Hold { .. } => {
                    state.effect.reads_state = true;
                    state.effect.writes_state = true;
                }
                OwnerConstraintNodeKind::Latest => {
                    let inputs = expression
                        .inputs
                        .iter()
                        .filter_map(|input| expression_variable(state, input.expression))
                        .collect::<Vec<_>>();
                    bind_and_record_flow_variables(
                        &mut unifier,
                        &mut flow_constraints,
                        variable,
                        inputs,
                    );
                    state.effect.reads_state = true;
                    state.effect.writes_state = true;
                    mode = None;
                }
                OwnerConstraintNodeKind::When => {
                    let inputs = expression
                        .inputs
                        .iter()
                        .filter(|input| matches!(input.role, OwnerConstraintEdgeRole::WhenArm))
                        .filter_map(|input| expression_variable(state, input.expression))
                        .collect::<Vec<_>>();
                    bind_and_record_flow_variables(
                        &mut unifier,
                        &mut flow_constraints,
                        variable,
                        inputs,
                    );
                    mode = None;
                }
                OwnerConstraintNodeKind::Then => {
                    let output = expression
                        .inputs
                        .iter()
                        .find(|input| matches!(input.role, OwnerConstraintEdgeRole::ThenOutput));
                    let input = output.or_else(|| {
                        expression
                            .inputs
                            .iter()
                            .find(|input| matches!(input.role, OwnerConstraintEdgeRole::ThenInput))
                    });
                    if let Some(input) =
                        input.and_then(|input| expression_variable(state, input.expression))
                    {
                        bind_and_record_flow_variables(
                            &mut unifier,
                            &mut flow_constraints,
                            variable,
                            [input],
                        );
                    }
                    mode = Some(FlowMode::PresentOrAbsent);
                }
                OwnerConstraintNodeKind::Infix { operation } => {
                    if infix_requires_number_operands(operation) {
                        for input in &expression.inputs {
                            if let Some(input) = expression_variable(state, input.expression) {
                                unifier.bind_var(input, Type::Number);
                            }
                        }
                    }
                    unifier.bind_var(
                        variable,
                        if infix_returns_bool(operation) {
                            true_false_type()
                        } else {
                            Type::Number
                        },
                    );
                }
                OwnerConstraintNodeKind::MatchArm { pattern } => {
                    if let Some(output) = expression
                        .inputs
                        .iter()
                        .find(|input| matches!(input.role, OwnerConstraintEdgeRole::MatchOutput))
                    {
                        if let Some(output) = expression_variable(state, output.expression) {
                            bind_and_record_flow_variables(
                                &mut unifier,
                                &mut flow_constraints,
                                variable,
                                [output],
                            );
                        }
                        mode = None;
                    } else {
                        unifier.bind_var(variable, Type::Absent);
                        mode = Some(FlowMode::Absent);
                    }
                    let pattern_ty = pattern_type(pattern, &mut unifier);
                    // A pattern describes one branch, not the public input
                    // contract of its selector. Feeding the pattern domain
                    // into the selector closes a callable parameter over the
                    // tags currently named by its body. That makes unrelated
                    // callers reverse dependencies of the callee interface
                    // and incorrectly rejects values handled by a wildcard
                    // (or by the ordinary no-matching-arm result). Keep the
                    // pattern-local type only for lexical binding projection;
                    // actual selector values specialize the frozen result
                    // transfer independently at each call occurrence.
                    let mut bindings = Vec::new();
                    for (target, stable) in state.seed.signature_regions().stable_targets() {
                        let OwnerLexicalDeclarationTarget::PatternBinding { arm, name } = target
                        else {
                            continue;
                        };
                        if *arm as usize != index {
                            continue;
                        }
                        let Some(binding) =
                            state.lexical_declaration_variables.get(stable).copied()
                        else {
                            return Err(OwnerConstraintSeedError::new(
                                "interface pattern declaration has no stable type root",
                            ));
                        };
                        // The stable declaration is the cross-owner authority.
                        // Bind it from the actual selector during narrowing;
                        // pre-unifying it with the pattern's fresh schematic
                        // field would retain an unconstrained union member in
                        // every child-only capture.
                        bindings.push((name.clone(), binding));
                    }
                    let local_bindings = expression
                        .inputs
                        .iter()
                        .filter_map(|input| {
                            let OwnerConstraintEdgeRole::MatchBinding { name } = &input.role else {
                                return None;
                            };
                            let projection = signature_read_preserved_projection_for(
                                state.seed,
                                &state.signature_lexical_plan,
                                input.expression,
                            )?;
                            expression_variable(state, input.expression)
                                .map(|read| (name.clone(), projection, read))
                        })
                        .collect::<Vec<_>>();
                    for (name, projection, read) in &local_bindings {
                        if let Some(binding_ty) =
                            pattern_binding_type_from_pattern(pattern, &pattern_ty, name)
                        {
                            let root = unifier.fresh();
                            unifier.bind_var(root, binding_ty);
                            let projected = bind_projection(&mut unifier, root, projection);
                            unifier.unify(Type::Var(*read), Type::Var(projected));
                            bindings.push((name.clone(), root));
                        }
                    }
                    let narrowed_payload = unifier.fresh();
                    if let (OwnerPatternConstraint::Tag { name, .. }, Type::VariantSet(variants)) =
                        (pattern, &pattern_ty)
                        && let Some(fields) = variants.iter().find_map(|variant| match variant {
                            Variant::Tagged { tag, fields } if tag == name => Some(fields.clone()),
                            Variant::Tag(_) | Variant::Tagged { .. } => None,
                        })
                    {
                        unifier.bind_var(narrowed_payload, Type::Object(fields));
                    }
                    let selector_reads = expression
                        .inputs
                        .iter()
                        .filter_map(|input| {
                            let OwnerConstraintEdgeRole::MatchNarrowedSelector { projection } =
                                &input.role
                            else {
                                return None;
                            };
                            let selector = expression.inputs.iter().find_map(|input| {
                                matches!(input.role, OwnerConstraintEdgeRole::MatchSelector)
                                    .then_some(input.expression)
                            })?;
                            if !signature_narrowed_selector_read_matches(
                                state,
                                selector,
                                projection,
                                input.expression,
                            ) {
                                return None;
                            }
                            expression_variable(state, input.expression)
                                .map(|read| (projection.clone(), read))
                        })
                        .collect::<Vec<_>>();
                    for (projection, read) in &selector_reads {
                        if projection.is_empty() {
                            unifier.bind_flow_result(*read, pattern_ty.clone());
                        } else {
                            let projected =
                                bind_projection(&mut unifier, narrowed_payload, projection);
                            unifier.unify(Type::Var(*read), Type::Var(projected));
                        }
                    }
                    if let Some(selector) = expression
                        .inputs
                        .iter()
                        .find(|input| matches!(input.role, OwnerConstraintEdgeRole::MatchSelector))
                        .and_then(|input| expression_variable(state, input.expression))
                    {
                        pattern_narrowings.push(OwnerPatternNarrowing {
                            selector,
                            pattern: pattern.clone(),
                            bindings: bindings.into_boxed_slice(),
                            binding_reads: local_bindings.into_boxed_slice(),
                            selector_reads: selector_reads.into_boxed_slice(),
                        });
                    }
                }
                OwnerConstraintNodeKind::Block => {
                    if let Some(result) = expression
                        .inputs
                        .iter()
                        .find(|input| matches!(input.role, OwnerConstraintEdgeRole::BlockResult))
                    {
                        if let Some(result) = expression_variable(state, result.expression) {
                            bind_and_record_flow_variables(
                                &mut unifier,
                                &mut flow_constraints,
                                variable,
                                [result],
                            );
                        }
                        mode = None;
                    } else {
                        unifier.bind_var(variable, Type::Absent);
                        mode = Some(FlowMode::Absent);
                    }
                }
                OwnerConstraintNodeKind::Collection { collection, .. } => match collection {
                    OwnerCollectionKind::List => {
                        let item = if expression.inputs.is_empty() {
                            unifier.fresh_contextual_hole()
                        } else {
                            unifier.fresh()
                        };
                        let inputs = expression
                            .inputs
                            .iter()
                            .filter_map(|input| expression_variable(state, input.expression))
                            .collect::<Vec<_>>();
                        if !inputs.is_empty() {
                            bind_and_record_structural_flow_variables(
                                &mut unifier,
                                &mut flow_constraints,
                                item,
                                inputs,
                            );
                        }
                        unifier.replace_derived_provider(
                            variable,
                            Type::List(Type::shared(Type::Var(item))),
                        );
                    }
                    OwnerCollectionKind::Set => {
                        let item = if expression.inputs.is_empty() {
                            unifier.fresh_contextual_hole()
                        } else {
                            unifier.fresh()
                        };
                        let inputs = expression
                            .inputs
                            .iter()
                            .filter_map(|input| expression_variable(state, input.expression))
                            .collect::<Vec<_>>();
                        if !inputs.is_empty() {
                            bind_and_record_structural_flow_variables(
                                &mut unifier,
                                &mut flow_constraints,
                                item,
                                inputs,
                            );
                        }
                        unifier.replace_derived_provider(
                            variable,
                            Type::Set(Type::shared(Type::Var(item))),
                        );
                    }
                    OwnerCollectionKind::Bytes => {
                        unifier.replace_derived_provider(variable, Type::Bytes(BytesType::Dynamic));
                    }
                    OwnerCollectionKind::Map => {
                        let empty = expression.inputs.is_empty();
                        let key = if empty {
                            unifier.fresh_contextual_hole()
                        } else {
                            unifier.fresh()
                        };
                        let value = if empty {
                            unifier.fresh_contextual_hole()
                        } else {
                            unifier.fresh()
                        };
                        let entries = expression
                            .inputs
                            .iter()
                            .filter_map(|input| expression_variable(state, input.expression))
                            .collect::<Vec<_>>();
                        let keys = entries
                            .iter()
                            .map(|entry| bind_projection(&mut unifier, *entry, &["key".to_owned()]))
                            .collect::<Vec<_>>();
                        let values = entries
                            .iter()
                            .map(|entry| {
                                bind_projection(&mut unifier, *entry, &["value".to_owned()])
                            })
                            .collect::<Vec<_>>();
                        if !keys.is_empty() {
                            bind_and_record_structural_flow_variables(
                                &mut unifier,
                                &mut flow_constraints,
                                key,
                                keys,
                            );
                            bind_and_record_structural_flow_variables(
                                &mut unifier,
                                &mut flow_constraints,
                                value,
                                values,
                            );
                        }
                        unifier.replace_derived_provider(
                            variable,
                            Type::Map {
                                key: Box::new(Type::Var(key)),
                                value: Box::new(Type::Var(value)),
                            },
                        );
                    }
                },
                OwnerConstraintNodeKind::Arrow { pattern } => {
                    if let Some(output) = expression
                        .inputs
                        .iter()
                        .find(|input| matches!(input.role, OwnerConstraintEdgeRole::ArrowOutput))
                    {
                        if let Some(output) = expression_variable(state, output.expression) {
                            bind_and_record_flow_variables(
                                &mut unifier,
                                &mut flow_constraints,
                                variable,
                                [output],
                            );
                        }
                    }
                    let _ = pattern_type(pattern, &mut unifier);
                }
                OwnerConstraintNodeKind::MapEntry => {
                    let key = expression.inputs.iter().find_map(|input| {
                        matches!(input.role, OwnerConstraintEdgeRole::MapKey)
                            .then(|| expression_variable(state, input.expression))
                            .flatten()
                    });
                    let value = expression.inputs.iter().find_map(|input| {
                        matches!(input.role, OwnerConstraintEdgeRole::MapValue)
                            .then(|| expression_variable(state, input.expression))
                            .flatten()
                    });
                    if let (Some(key), Some(value)) = (key, value) {
                        unifier.bind_var(
                            variable,
                            Type::object(ObjectShape::from_ordered_fields(
                                [
                                    ("key".to_owned(), Type::Var(key)),
                                    ("value".to_owned(), Type::Var(value)),
                                ],
                                false,
                            )),
                        );
                    }
                }
                OwnerConstraintNodeKind::Delimiter | OwnerConstraintNodeKind::Unknown { .. } => {}
            }
            state.modes[index] = flow_mode_join(state.modes[index], mode);
            work.local_constraints = work.local_constraints.saturating_add(1);
        }
    }
    calls.sort_by(|left, right| {
        left.caller.cmp(&right.caller).then_with(|| {
            let ordinal = |call: &CrossCall| {
                states[&call.caller]
                    .signature_lexical_plan
                    .call(call.expression)
                    .map_or(u32::MAX, |call| call.structural_ordinal)
            };
            // Signature ordinals are preorder (outer first). Solve nested
            // result producers before their enclosing consumers; dynamic
            // FreshOut/CallContext inputs remain staged until the parent has
            // published those declarations.
            ordinal(right).cmp(&ordinal(left))
        })
    });

    // Child expression boundaries and resolved value reads consume the exact
    // interface result instead of a copied child body.
    let internal_results = states
        .iter()
        .map(|(owner, state)| (owner.clone(), (state.result, state.result_flush)))
        .collect::<BTreeMap<_, _>>();
    let internal_expressions = states
        .iter()
        .flat_map(|(owner, state)| {
            state
                .expression_by_key
                .iter()
                .map(move |(expression, index)| {
                    (
                        (owner.clone(), expression.clone()),
                        (state.expressions[*index], state.expression_flushes[*index]),
                    )
                })
        })
        .collect::<BTreeMap<_, _>>();
    for state in states.values() {
        for ((external, variable), flush_variable) in state
            .seed
            .external_expressions
            .iter()
            .zip(&state.external_expressions)
            .zip(&state.external_expression_flushes)
        {
            if external.is_exact_enclosing_capture_for(&state.seed.owner) {
                let (expression, expression_flush) = internal_expressions
                    .get(&(external.owner.clone(), external.expression.clone()))
                    .copied()
                    .ok_or_else(|| {
                        OwnerConstraintSeedError::new(format!(
                            "owner interface {:?} captures expression {:?} outside its interface SCC",
                            state.seed.owner, external.expression
                        ))
                    })?;
                unifier.unify(Type::Var(*variable), Type::Var(expression));
                unifier.unify(Type::Var(*flush_variable), Type::Var(expression_flush));
            } else if let Some((result, result_flush)) = internal_results.get(&external.owner) {
                unifier.unify(Type::Var(*variable), Type::Var(*result));
                unifier.unify(Type::Var(*flush_variable), Type::Var(*result_flush));
            } else if let Some(interface) = dependency_interfaces.get(&external.owner) {
                let mut variables = BTreeMap::new();
                let ty = instantiate_type(&interface.result.ty, &mut unifier, &mut variables);
                unifier.bind_var(*variable, ty);
                let flush_type = interface
                    .result_flush_type
                    .as_ref()
                    .map_or(Type::Absent, |ty| {
                        instantiate_type(ty, &mut unifier, &mut variables)
                    });
                unifier.bind_var(*flush_variable, flush_type);
            } else {
                unifier.bind_var(*flush_variable, Type::Absent);
            }
            work.cross_owner_constraints = work.cross_owner_constraints.saturating_add(1);
        }
        for resolved in &state.summary.resolved_references {
            if resolved.reference.kind != OwnerReferenceKind::Value {
                continue;
            }
            let Some(index) = state
                .expression_by_key
                .get(&resolved.reference.expression)
                .copied()
            else {
                continue;
            };
            if state.signature_lexical_plan.reads()[index].is_some() {
                continue;
            }
            let expression = state.expressions[index];
            if let Some((result, _)) = internal_results.get(&resolved.owner) {
                let result = bind_projection(&mut unifier, *result, &resolved.projection);
                unifier.unify(Type::Var(expression), Type::Var(result));
            } else if let Some(interface) = dependency_interfaces.get(&resolved.owner) {
                let mut variables = BTreeMap::new();
                let ty = instantiate_type(&interface.result.ty, &mut unifier, &mut variables);
                let result = unifier.fresh();
                unifier.bind_var(result, ty);
                let result = bind_projection(&mut unifier, result, &resolved.projection);
                unifier.unify(Type::Var(expression), Type::Var(result));
            }
            work.cross_owner_constraints = work.cross_owner_constraints.saturating_add(1);
        }
    }

    // Establish the first prior-state epoch from each initializer before a
    // child-owner self-read is propagated. The complete ordered update fold
    // runs inside the fixed-point loop after captures and calls have observed
    // this epoch; evaluating it here would widen against detached provisional
    // capture alphas and can collapse a closed recursive tag state to an open
    // object before the first provider refresh.
    for state in states.values() {
        initialize_owner_hold_constraints(
            &mut unifier,
            &mut hold_authorities,
            state.seed,
            &state.expressions,
            &state.external_expressions,
        );
    }

    // Solve the parallel FLUSH-control graph before publishing boundary result
    // variables. Static edges come from the owner projection; a call adds its
    // callee escape through a per-occurrence variable that is instantiated in
    // the same substitution namespace as the call result below.
    for state in states.values() {
        for (index, plan) in state.seed.expression_flush_plans.iter().enumerate() {
            let mut candidates = Vec::new();
            candidates.extend(
                plan.value_inputs
                    .iter()
                    .filter_map(|input| expression_variable(state, *input))
                    .map(Type::Var),
            );
            candidates.extend(
                plan.escape_inputs
                    .iter()
                    .filter_map(|input| expression_flush_variable(state, *input))
                    .map(Type::Var),
            );
            candidates.extend(state.call_flushes[index].map(Type::Var));
            unifier.bind_var(
                state.expression_flushes[index],
                if candidates.is_empty() {
                    Type::Absent
                } else {
                    boon_checked::canonical_union_type(candidates)
                },
            );
        }
        if let Some(expression) = owner_result_expression(state) {
            if let Some(boundary) = expression_boundary_variable(state, expression, &mut unifier) {
                unifier.unify(Type::Var(state.result), Type::Var(boundary));
            }
            if let Some(flush) = expression_flush_variable(state, expression) {
                unifier.unify(Type::Var(state.result_flush), Type::Var(flush));
            }
        } else {
            unifier.bind_var(state.result, Type::Absent);
            unifier.bind_var(state.result_flush, Type::Absent);
        }
    }

    refine_owner_pattern_narrowings(&mut unifier, &pattern_narrowings);
    refine_owner_inherited_pattern_narrowings(&mut unifier, &inherited_pattern_narrowings);
    let mut flow_program = OwnerFlowConstraintProgram::new(&mut unifier, flow_constraints);
    if !flow_program.replay(&mut unifier) {
        return Err(OwnerConstraintSeedError::new(format!(
            "interface component {:?} has a non-convergent local value-flow graph",
            scc.key
        )));
    }
    unifier.refine_contextual_flow_holes();
    let internal_call_graph = calls.iter().fold(
        BTreeMap::<StableCheckOwnerKey, BTreeSet<StableCheckOwnerKey>>::new(),
        |mut graph, call| {
            if let Some(target) = call
                .target
                .as_ref()
                .filter(|target| states.contains_key(*target))
            {
                graph
                    .entry(call.caller.clone())
                    .or_default()
                    .insert(target.clone());
            }
            graph
        },
    );
    let recursive_owner_calls = calls
        .iter()
        .map(|call| {
            let Some(target) = call
                .target
                .as_ref()
                .filter(|target| states.contains_key(*target))
            else {
                return false;
            };
            let mut pending = vec![target.clone()];
            let mut visited = BTreeSet::new();
            while let Some(owner) = pending.pop() {
                if owner == call.caller {
                    return true;
                }
                if !visited.insert(owner.clone()) {
                    continue;
                }
                pending.extend(
                    internal_call_graph
                        .get(&owner)
                        .into_iter()
                        .flat_map(|targets| targets.iter().cloned()),
                );
            }
            false
        })
        .collect::<Vec<_>>();
    let mut call_variables = vec![BTreeMap::new(); calls.len()];
    // Context schemes are instantiated independently from the ordinary call
    // result/parameter scheme. The checked-program oracle deliberately gives
    // an inherited PASSED requirement its own per-call variables; sharing this
    // substitution map would incorrectly couple a wrapper's result type to its
    // inherited context leaf.
    let mut call_context_variables = vec![BTreeMap::new(); calls.len()];
    let live_component = transfer_module_resolver.is_some();
    let component_members = scc.key.members.iter().collect::<BTreeSet<_>>();
    let component_has_internal_edges = scc.edges.iter().any(|edge| {
        component_members.contains(&edge.request) && component_members.contains(&edge.dependency)
    });
    let hold_constraint_count = states
        .values()
        .flat_map(|state| state.seed.expressions.iter())
        .filter(|expression| {
            matches!(expression.kind, OwnerConstraintNodeKind::Hold { .. })
                && expression
                    .inputs
                    .iter()
                    .any(|input| matches!(input.role, OwnerConstraintEdgeRole::HoldUpdate))
        })
        .count();
    let has_hold_updates = hold_constraint_count != 0;
    let requires_fixed_point = live_component
        || !calls.is_empty()
        || !pattern_narrowings.is_empty()
        || has_hold_updates
        || !internal_lexical_capture_providers.is_empty()
        || states
            .values()
            .any(|state| !state.lexical_capture_variables.is_empty());
    let mut previous_surface = Vec::new();
    if requires_fixed_point {
        write_solver_surface_snapshot(&mut unifier, &states, &mut previous_surface);
    }
    let mut current_surface = Vec::with_capacity(previous_surface.len());
    let maximum_rounds = if requires_fixed_point {
        states
            .len()
            .saturating_add(calls.len())
            .saturating_add(hold_constraint_count)
            .saturating_add(if live_component { 4 } else { 2 })
    } else {
        0
    };
    let mut converged = !requires_fixed_point;
    let mut external_transfer_modules = None::<Vec<Arc<OwnerInterfaceTransferModule>>>;
    let mut resolved_transfer_owners = None::<Box<[StableCheckOwnerKey]>>;
    let mut final_transfer_module = None::<Arc<OwnerInterfaceTransferModule>>;
    let mut final_projection = None::<OwnerInterfaceSccProjection>;
    let mut transfer_iterations = 0_u32;
    let mut transfer_work = OwnerResidualEvaluationWork::default();
    let mut seen_transfer_surfaces = HashSet::new();
    let mut committed_transfer_results = vec![None::<Type>; calls.len()];
    let trace_rounds =
        std::env::var_os("BOON_OWNER_REQUEST_TRACE").is_some() && scc.key.members.len() >= 100;
    let rounds_started = Instant::now();
    let mut call_round_elapsed = Duration::ZERO;
    let mut provider_replay_elapsed = Duration::ZERO;
    let mut dynamic_replay_elapsed = Duration::ZERO;
    let mut stable_surface_elapsed = Duration::ZERO;
    let mut dependency_resolution_elapsed = Duration::ZERO;
    let mut residual_key_elapsed = Duration::ZERO;
    let mut residual_argument_elapsed = Duration::ZERO;
    let mut residual_evaluation_elapsed = Duration::ZERO;
    let mut residual_publication_elapsed = Duration::ZERO;
    let mut residual_post_replay_elapsed = Duration::ZERO;
    for round in 0..maximum_rounds {
        let call_round_started = Instant::now();
        work.solve_rounds = work.solve_rounds.saturating_add(1);
        let changes_before = unifier.changes();
        let mut surface_changed = false;
        let mut staged_provider_inputs = vec![Vec::<(TypeVar, Type)>::new(); calls.len()];
        let mut staged_dynamic_inputs = vec![Vec::<(TypeVar, Type)>::new(); calls.len()];
        let mut staged_dynamic_fields = vec![None::<(TypeVar, TypeVar, String)>; calls.len()];
        let mut staged_signature_reads = Vec::<(
            StableCheckOwnerKey,
            OwnerSignatureDeclarationTarget,
            TypeVar,
            FlowMode,
        )>::new();
        let mut staged_owner_transfers = vec![None::<StableCheckOwnerKey>; calls.len()];
        for (call_index, call) in calls.iter().enumerate() {
            let caller = states.get(&call.caller).ok_or_else(|| {
                OwnerConstraintSeedError::new("interface call has no caller state")
            })?;
            let variables = &mut call_variables[call_index];
            let context_variables = &mut call_context_variables[call_index];
            let (
                parameters,
                call_contexts,
                result,
                result_specialization,
                result_flush,
                result_mode,
                context,
                effect,
                signature_found,
            ) = if let Some(target) = &call.target {
                if let Some(callee) = states.get(target) {
                    let parameters = callee
                        .parameters
                        .iter()
                        .map(|parameter| InstantiatedInterfaceParameter {
                            name: parameter.name.clone(),
                            ordinal: parameter.ordinal,
                            ty: instantiate_type(
                                &unifier.resolve(&Type::Var(parameter.variable)),
                                &mut unifier,
                                &mut *variables,
                            ),
                            mode: FlowMode::Continuous,
                            evaluation_scope: parameter.evaluation_scope,
                        })
                        .collect::<Vec<_>>();
                    let result = instantiate_type(
                        &unifier.resolve(&Type::Var(callee.result)),
                        &mut unifier,
                        &mut *variables,
                    );
                    let result_flush = instantiate_type(
                        &unifier.resolve(&Type::Var(callee.result_flush)),
                        &mut unifier,
                        &mut *variables,
                    );
                    let resolved_context = unifier.resolve(&Type::Var(callee.context));
                    let context =
                        (!matches!(resolved_context, Type::Var(_) | Type::Unknown)).then(|| {
                            instantiate_type(
                                &resolved_context,
                                &mut unifier,
                                &mut *context_variables,
                            )
                        });
                    let result_mode = owner_result_expression(callee)
                        .map(|expression| {
                            let raw_mode = callee
                                .modes
                                .get(expression as usize)
                                .copied()
                                .flatten()
                                .unwrap_or(FlowMode::Continuous);
                            resolved_expression_boundary(callee, expression, &mut unifier, raw_mode)
                                .mode
                        })
                        .unwrap_or(FlowMode::Continuous);
                    (
                        parameters,
                        Vec::new(),
                        result,
                        crate::OwnerAbiResultSpecialization::Fixed,
                        result_flush,
                        result_mode,
                        context,
                        callee.effect,
                        true,
                    )
                } else if let Some(callee) = dependency_interfaces.get(target) {
                    let parameters = callee
                        .parameters
                        .iter()
                        .map(|parameter| InstantiatedInterfaceParameter {
                            name: parameter.name.clone(),
                            ordinal: parameter.ordinal,
                            ty: instantiate_type(
                                &parameter.flow_type.ty,
                                &mut unifier,
                                &mut *variables,
                            ),
                            mode: parameter.flow_type.mode,
                            evaluation_scope: parameter.evaluation_scope,
                        })
                        .collect();
                    let result = instantiate_type(&callee.result.ty, &mut unifier, &mut *variables);
                    let result_flush = callee
                        .result_flush_type
                        .as_ref()
                        .map_or(Type::Absent, |ty| {
                            instantiate_type(ty, &mut unifier, &mut *variables)
                        });
                    let context = callee.context.as_ref().map(|context| {
                        instantiate_type(
                            &context.flow_type.ty,
                            &mut unifier,
                            &mut *context_variables,
                        )
                    });
                    (
                        parameters,
                        Vec::new(),
                        result,
                        crate::OwnerAbiResultSpecialization::Fixed,
                        result_flush,
                        callee.result.mode,
                        context,
                        callee.effect,
                        true,
                    )
                } else {
                    (
                        Vec::new(),
                        Vec::new(),
                        Type::Unknown,
                        crate::OwnerAbiResultSpecialization::Fixed,
                        Type::Absent,
                        FlowMode::Continuous,
                        None,
                        CheckedEffectSummary::default(),
                        false,
                    )
                }
            } else if let Some(signature) = abi.callable(&call.function) {
                let parameters = signature
                    .parameters
                    .iter()
                    .enumerate()
                    .map(|(ordinal, parameter)| InstantiatedInterfaceParameter {
                        name: parameter.name.clone(),
                        ordinal: ordinal as u32,
                        ty: instantiate_type(
                            &parameter.flow_type.ty,
                            &mut unifier,
                            &mut *variables,
                        ),
                        mode: parameter.flow_type.mode,
                        evaluation_scope: match parameter.evaluation_scope {
                            crate::OwnerAbiEvaluationScope::Parent => {
                                OwnerInterfaceEvaluationScope::Parent
                            }
                            crate::OwnerAbiEvaluationScope::Output { parameter_ordinal } => {
                                OwnerInterfaceEvaluationScope::Output { parameter_ordinal }
                            }
                        },
                    })
                    .collect::<Vec<_>>();
                let call_contexts = signature
                    .contexts
                    .iter()
                    .enumerate()
                    .map(|(ordinal, context)| {
                        Ok(InstantiatedInterfaceCallContext {
                            ordinal: u32::try_from(ordinal).map_err(|_| {
                                OwnerConstraintSeedError::new(
                                    "interface call context ordinal exceeds u32",
                                )
                            })?,
                            name: context.name.clone(),
                            provider_parameter_ordinal: context.provider_parameter_ordinal,
                            ty: instantiate_type(
                                &context.flow_type.ty,
                                &mut unifier,
                                &mut *variables,
                            ),
                            mode: context.flow_type.mode,
                        })
                    })
                    .collect::<Result<Vec<_>, OwnerConstraintSeedError>>()?;
                let result = instantiate_type(&signature.result.ty, &mut unifier, &mut *variables);
                (
                    parameters,
                    call_contexts,
                    result,
                    signature.result_specialization,
                    Type::Absent,
                    signature.result.mode,
                    None,
                    signature.effect,
                    true,
                )
            } else {
                (
                    Vec::new(),
                    Vec::new(),
                    Type::Unknown,
                    crate::OwnerAbiResultSpecialization::Fixed,
                    Type::Absent,
                    FlowMode::Continuous,
                    None,
                    CheckedEffectSummary::default(),
                    false,
                )
            };
            let signature_call = caller
                .signature_lexical_plan
                .call(call.expression)
                .ok_or_else(|| {
                    OwnerConstraintSeedError::new(format!(
                        "interface call {:?} is absent from its signature lexical plan",
                        call.stable_expression
                    ))
                })?;
            let has_explicit_pass = signature_call.explicit_pass.is_some();
            let caller_context = effective_context_variable(caller)?;
            let call_valid = signature_found
                && signature_call.valid
                && (has_explicit_pass || context.is_none() || caller_context.is_some());
            // Only an expression transfer can make one occurrence narrower
            // than its callable principal (for example an ordered WHEN whose
            // selector is concrete at the call site). Principal and direct
            // parameter transfers are ordinary live equations; withholding
            // them adds an unnecessary residual cycle and can prevent a
            // mutually recursive alias component from reaching its least
            // fixed point.
            let transfer_target = live_component
                .then(|| call.target.as_ref())
                .flatten()
                .filter(|target| {
                    states.get(*target).is_some_and(|state| {
                        !recursive_owner_calls[call_index]
                            && owner_result_requires_occurrence_transfer(state)
                    }) || dependency_interfaces.contains_key(*target)
                })
                .cloned();
            if call_valid {
                staged_owner_transfers[call_index] = transfer_target.clone();
            }

            let call_variable = caller.expressions[call.expression];
            let result = crate::specialize_owner_abi_result_type(
                &result,
                result_specialization,
                signature_call.matched_inputs.iter().filter_map(|planned| {
                    let parameter = parameters
                        .binary_search_by_key(&planned.formal_ordinal, |parameter| {
                            parameter.ordinal
                        })
                        .ok()
                        .and_then(|index| parameters.get(index))?;
                    let input = expression_variable(caller, planned.expression)?;
                    Some((parameter.name.clone(), Type::Var(input)))
                }),
            );
            // Frozen dependency interfaces and fixed ABI signatures are
            // instantiated into one persistent per-occurrence alpha frame.
            // Their result equation remains live through that frame while
            // later rounds refine the inputs, so rebinding it would only
            // resolve and clone the increasingly rich result graph. Internal
            // SCC results and actual-dependent render constructors must still
            // be refreshed as their source surface changes.
            let refresh_result_binding = round == 0
                || result_specialization == crate::OwnerAbiResultSpecialization::RenderConstructor
                || call
                    .target
                    .as_ref()
                    .is_some_and(|target| states.contains_key(target));
            if call_valid
                && !has_explicit_pass
                && let Some(context) = &context
                && let Some(caller_context) = caller_context
            {
                unifier.unify(Type::Var(caller_context), context.clone());
            }
            if call_valid && let Some(field) = call.function.strip_prefix("Field/") {
                if let Some(input_expression) = signature_call
                    .matched_inputs
                    .iter()
                    .find(|input| {
                        input.source == crate::OwnerSignatureMatchedInputSource::PipeInput
                    })
                    .map(|input| input.expression)
                    && let Some(input) = expression_variable(caller, input_expression)
                {
                    if caller
                        .signature_dynamic_expressions
                        .get(input_expression as usize)
                        .copied()
                        .unwrap_or(false)
                    {
                        staged_dynamic_fields[call_index] =
                            Some((call_variable, input, field.to_owned()));
                    } else {
                        let projected = bind_projection(&mut unifier, input, &[field.to_owned()]);
                        unifier.unify(Type::Var(call_variable), Type::Var(projected));
                    }
                }
            } else if call_valid && transfer_target.is_none() && refresh_result_binding {
                unifier.bind_var(call_variable, result);
            }
            for planned in signature_call.matched_inputs.iter().filter(|_| call_valid) {
                let Some(input) = expression_variable(caller, planned.expression) else {
                    continue;
                };
                let Some(parameter) = parameters
                    .binary_search_by_key(&planned.formal_ordinal, |parameter| parameter.ordinal)
                    .ok()
                    .and_then(|index| parameters.get(index))
                else {
                    continue;
                };
                if planned.from_pipe || planned.argument_kind == OwnerArgumentKind::Named {
                    if caller
                        .signature_dynamic_expressions
                        .get(planned.expression as usize)
                        .copied()
                        .unwrap_or(false)
                    {
                        let staged = if matches!(
                            parameter.evaluation_scope,
                            OwnerInterfaceEvaluationScope::Parent
                        ) && (!signature_call.outputs.is_empty()
                            || !signature_call.contexts.is_empty())
                        {
                            &mut staged_provider_inputs[call_index]
                        } else {
                            &mut staged_dynamic_inputs[call_index]
                        };
                        staged.push((input, parameter.ty.clone()));
                    } else {
                        unifier.bind_call_input(input, parameter.ty.clone());
                    }
                }
            }
            if call_valid
                && let (Some(pass), Some(context)) = (&signature_call.explicit_pass, &context)
                && let Some(input) = expression_variable(caller, pass.expression)
            {
                if caller
                    .signature_dynamic_expressions
                    .get(pass.expression as usize)
                    .copied()
                    .unwrap_or(false)
                {
                    staged_dynamic_inputs[call_index].push((input, context.clone()));
                } else {
                    unifier.bind_call_input(input, context.clone());
                }
            }
            unifier.bind_var(
                call.flush,
                if call_valid {
                    result_flush
                } else {
                    Type::Absent
                },
            );
            if let Some(state) = states.get_mut(&call.caller) {
                let signature_call = state
                    .signature_lexical_plan
                    .call(call.expression)
                    .cloned()
                    .ok_or_else(|| {
                        OwnerConstraintSeedError::new(
                            "interface signature lexical call disappeared during solving",
                        )
                    })?;
                if call_valid && signature_call.valid {
                    for output in &signature_call.outputs {
                        let crate::OwnerSignatureOutputBindingPlan::Fresh { target, .. } = output
                        else {
                            continue;
                        };
                        let Some(variable) =
                            state.signature_declaration_variables.get(target).copied()
                        else {
                            continue;
                        };
                        let Some(parameter) = parameters
                            .iter()
                            .find(|parameter| parameter.ordinal == output.formal_ordinal())
                        else {
                            continue;
                        };
                        unifier.unify(Type::Var(variable), parameter.ty.clone());
                        let slot = state
                            .signature_declaration_modes
                            .entry(target.clone())
                            .or_insert(None);
                        let merged = flow_mode_join(*slot, Some(parameter.mode));
                        surface_changed |= merged != *slot;
                        *slot = merged;
                        staged_signature_reads.push((
                            call.caller.clone(),
                            target.clone(),
                            variable,
                            parameter.mode,
                        ));
                    }
                    for planned in &signature_call.contexts {
                        let Some(variable) = state
                            .signature_declaration_variables
                            .get(&planned.target)
                            .copied()
                        else {
                            continue;
                        };
                        let Some(context) = call_contexts.iter().find(|context| {
                            context.ordinal == planned.context_ordinal
                                && context.name == planned.name
                                && context.provider_parameter_ordinal
                                    == planned.provider_parameter_ordinal
                        }) else {
                            continue;
                        };
                        unifier.unify(Type::Var(variable), context.ty.clone());
                        let slot = state
                            .signature_declaration_modes
                            .entry(planned.target.clone())
                            .or_insert(None);
                        let merged = flow_mode_join(*slot, Some(context.mode));
                        surface_changed |= merged != *slot;
                        *slot = merged;
                        staged_signature_reads.push((
                            call.caller.clone(),
                            planned.target.clone(),
                            variable,
                            context.mode,
                        ));
                    }
                }
                if call_valid {
                    let merged = merge_effects(state.effect, effect);
                    surface_changed |= merged != state.effect;
                    state.effect = merged;
                }
                if call_valid && transfer_target.is_none() {
                    let mode = flow_mode_join(state.modes[call.expression], Some(result_mode));
                    surface_changed |= mode != state.modes[call.expression];
                    state.modes[call.expression] = mode;
                }
            }
            let _ = call.stable_expression;
            work.cross_owner_constraints = work.cross_owner_constraints.saturating_add(1);
        }
        call_round_elapsed += call_round_started.elapsed();
        let provider_replay_started = Instant::now();
        // Every local output declaration for this round is now published.
        // Forward those provider surfaces across child-owner capture seams
        // before any producer-dependent consumer formal can shape them. This
        // phase is deliberately one-way; consumer requirements flow back only
        // after all staged inputs have observed the frozen provider epoch.
        propagate_lexical_capture_types(
            &states,
            &internal_lexical_capture_providers,
            &mut lexical_capture_type_variables,
            &mut lexical_capture_provider_types,
            false,
            &mut unifier,
        )?;
        // Contextual calls derive their OUT/context declarations from one or
        // more parent-scope provider inputs (for example List/filter's list
        // parameter). Settle every such producer chain before projecting any
        // FreshOut read. Otherwise the child-first call walk lets `item.id`
        // create an open one-field item before the enclosing filter has seen
        // its exact list item.
        for inputs in &mut staged_provider_inputs {
            for (input, expected) in inputs.drain(..) {
                unifier.bind_call_input(input, expected);
            }
        }
        for (owner, target, variable, mode) in staged_signature_reads.drain(..) {
            let state = states.get_mut(&owner).ok_or_else(|| {
                OwnerConstraintSeedError::new(
                    "staged interface signature read lost its caller state",
                )
            })?;
            bind_signature_declaration_reads(state, &target, variable, mode, &mut unifier);
        }
        // FreshOut reads can themselves cross a child-owner boundary. Publish
        // the now-shaped provider epoch before replaying consumer/body inputs.
        propagate_lexical_capture_types(
            &states,
            &internal_lexical_capture_providers,
            &mut lexical_capture_type_variables,
            &mut lexical_capture_provider_types,
            false,
            &mut unifier,
        )?;
        refine_owner_pattern_narrowings(&mut unifier, &pattern_narrowings);
        refine_owner_inherited_pattern_narrowings(&mut unifier, &inherited_pattern_narrowings);
        if !flow_program.replay(&mut unifier) {
            return Err(OwnerConstraintSeedError::new(format!(
                "interface component {:?} has a non-convergent local value-flow graph",
                scc.key
            )));
        }
        unifier.refine_contextual_flow_holes();
        provider_replay_elapsed += provider_replay_started.elapsed();
        let dynamic_replay_started = Instant::now();

        // Apply producer-dependent inputs child-before-parent within each
        // caller. Keep authored argument and PASS order within each call.
        let mut caller_start = 0;
        while caller_start < calls.len() {
            let mut caller_end = caller_start + 1;
            while caller_end < calls.len() && calls[caller_end].caller == calls[caller_start].caller
            {
                caller_end += 1;
            }
            for call_index in caller_start..caller_end {
                for (input, expected) in staged_dynamic_inputs[call_index].drain(..) {
                    unifier.bind_call_input(input, expected);
                }
            }
            caller_start = caller_end;
        }
        // A Field/* call whose provider is FreshOut/CallContext-dependent is
        // itself a consumer of the staged epoch. Projecting it during the
        // child-first call walk would create a sparse open object before the
        // enclosing contextual call has bound its list/context provider.
        // Replay all provider inputs first, then publish each exact field
        // result authoritatively so an already-wired outer consumer cannot
        // turn its provisional scaffold into co-authority.
        for staged in staged_dynamic_fields.into_iter().flatten() {
            let (call_variable, input, field) = staged;
            let projected = bind_projection(&mut unifier, input, &[field]);
            unifier.mark_authoritative_provider(call_variable);
            unifier.publish_derived_provider_epoch(call_variable, Type::Var(projected));
        }
        refine_owner_pattern_narrowings(&mut unifier, &pattern_narrowings);
        propagate_lexical_capture_types(
            &states,
            &internal_lexical_capture_providers,
            &mut lexical_capture_type_variables,
            &mut lexical_capture_provider_types,
            true,
            &mut unifier,
        )?;
        refine_owner_inherited_pattern_narrowings(&mut unifier, &inherited_pattern_narrowings);
        if !flow_program.replay(&mut unifier) {
            return Err(OwnerConstraintSeedError::new(format!(
                "interface component {:?} has a non-convergent local value-flow graph",
                scc.key
            )));
        }
        surface_changed |= propagate_lexical_capture_modes(&mut states)?;
        dynamic_replay_elapsed += dynamic_replay_started.elapsed();
        let stable_surface_started = Instant::now();

        // A committed transfer result can still contain live occurrence
        // alphas. Later input replay must be allowed to constrain those holes,
        // but its open formal scaffold must not become the result's new outer
        // authority. Republish the retained provider program after every
        // replay epoch; this is a cheap live-root operation and does not
        // re-evaluate transfer syntax.
        for (call_index, provider) in committed_transfer_results.iter().enumerate() {
            let Some(provider) = provider else {
                continue;
            };
            let caller = states.get(&calls[call_index].caller).ok_or_else(|| {
                OwnerConstraintSeedError::new("committed interface transfer lost its caller state")
            })?;
            let call_variable = caller.expressions[calls[call_index].expression];
            unifier.mark_authoritative_provider(call_variable);
            unifier.publish_derived_provider_epoch(call_variable, provider.clone());
        }

        // Calls, captures, pattern reads, and committed result transfers can
        // close an authored HOLD update after its first boundary replay. Keep
        // the state value authoritative over the complete ordered update set
        // before deciding that the public solver surface is stable.
        surface_changed |=
            replay_interface_hold_constraints(&mut unifier, &mut hold_authorities, &states);

        let raw_stable_before_transfer = unifier.changes() == changes_before && !surface_changed;
        write_solver_surface_snapshot(&mut unifier, &states, &mut current_surface);
        let semantic_stable_before_transfer =
            current_surface == previous_surface && !surface_changed;
        let stable_before_transfer = raw_stable_before_transfer || semantic_stable_before_transfer;
        stable_surface_elapsed += stable_surface_started.elapsed();

        if stable_before_transfer && !live_component {
            converged = true;
            break;
        }

        if stable_before_transfer {
            let provisional =
                project_owner_interface_scc_result(scc, abi, &states, &mut unifier, work)?;
            let module_dependency_owners = owner_interface_transfer_dependency_owners(&provisional);
            let own_members = scc.key.members.iter().collect::<BTreeSet<_>>();
            let mut requested_owners = module_dependency_owners.iter().cloned().collect::<Vec<_>>();
            requested_owners.extend(
                staged_owner_transfers
                    .iter()
                    .flatten()
                    .filter(|owner| !own_members.contains(owner))
                    .cloned(),
            );
            requested_owners.sort();
            requested_owners.dedup();

            if let Some(previous) = &resolved_transfer_owners {
                if previous.as_ref() != requested_owners.as_slice() {
                    return Err(OwnerConstraintSeedError::new(format!(
                        "interface component {:?} changed its exact transfer dependency set while solving",
                        scc.key
                    )));
                }
            } else {
                let dependency_resolution_started = Instant::now();
                let resolver = transfer_module_resolver.as_mut().ok_or_else(|| {
                    OwnerConstraintSeedError::new(
                        "live interface component has no transfer-module resolver",
                    )
                })?;
                let modules = resolver(&requested_owners).map_err(|error| {
                    OwnerConstraintSeedError::new(format!(
                        "cannot resolve interface component {:?} transfer modules: {error}",
                        scc.key
                    ))
                })?;
                let mut canonical_modules =
                    BTreeMap::<OwnerInterfaceSccKey, Arc<OwnerInterfaceTransferModule>>::new();
                for module in modules {
                    if module.key() == &scc.key {
                        return Err(OwnerConstraintSeedError::new(format!(
                            "interface component {:?} received its own transfer module as an external dependency",
                            scc.key
                        )));
                    }
                    if !requested_owners
                        .iter()
                        .any(|owner| module.owns_owner(owner))
                    {
                        return Err(OwnerConstraintSeedError::new(format!(
                            "interface component {:?} received unused transfer module {:?}",
                            scc.key,
                            module.key()
                        )));
                    }
                    match canonical_modules.entry(module.key().clone()) {
                        std::collections::btree_map::Entry::Vacant(entry) => {
                            entry.insert(module);
                        }
                        std::collections::btree_map::Entry::Occupied(entry)
                            if entry.get().fingerprint_v1() == module.fingerprint_v1() => {}
                        std::collections::btree_map::Entry::Occupied(_) => {
                            return Err(OwnerConstraintSeedError::new(format!(
                                "interface component {:?} received conflicting versions of one transfer module",
                                scc.key
                            )));
                        }
                    }
                }
                let modules = canonical_modules.into_values().collect::<Vec<_>>();
                for owner in &requested_owners {
                    let matches = modules
                        .iter()
                        .filter(|module| module.owns_owner(owner))
                        .count();
                    if matches != 1 {
                        return Err(OwnerConstraintSeedError::new(format!(
                            "interface component {:?} resolved transfer owner {owner:?} through {matches} modules",
                            scc.key
                        )));
                    }
                }
                resolved_transfer_owners = Some(requested_owners.clone().into_boxed_slice());
                external_transfer_modules = Some(modules);
                dependency_resolution_elapsed += dependency_resolution_started.elapsed();
            }

            let external_modules = external_transfer_modules
                .as_ref()
                .expect("live interface transfer dependencies were initialized");
            let mut module_dependencies =
                BTreeMap::<OwnerInterfaceSccKey, Arc<OwnerInterfaceTransferModule>>::new();
            for owner in &module_dependency_owners {
                let module = external_modules
                    .iter()
                    .find(|module| module.owns_owner(owner))
                    .ok_or_else(|| {
                        OwnerConstraintSeedError::new(format!(
                            "interface component {:?} cannot route transfer dependency {owner:?}",
                            scc.key
                        ))
                    })?;
                module_dependencies.insert(module.key().clone(), Arc::clone(module));
            }
            let own_module = Arc::new(
                project_owner_interface_transfer_module(
                    scc,
                    provisional.clone(),
                    module_dependencies.into_values(),
                )
                .map_err(|error| {
                    OwnerConstraintSeedError::new(format!(
                        "cannot compile interface component {:?} transfer residual: {error}",
                        scc.key
                    ))
                })?,
            );

            let residual_key_started = Instant::now();
            let mut residual_types = Vec::new();
            let mut residual_variables = BTreeMap::new();
            let mut next_residual_variable = 0;
            for (call_index, target) in staged_owner_transfers.iter().enumerate() {
                if target.is_none() {
                    continue;
                }
                let call = &calls[call_index];
                let caller = states.get(&call.caller).ok_or_else(|| {
                    OwnerConstraintSeedError::new(
                        "interface transfer residual lost its caller state",
                    )
                })?;
                residual_types.push(alpha_normalized_resolved_type(
                    &mut unifier,
                    caller.expressions[call.expression],
                    &mut residual_variables,
                    &mut next_residual_variable,
                ));
                let signature_call = caller
                    .signature_lexical_plan
                    .call(call.expression)
                    .ok_or_else(|| {
                        OwnerConstraintSeedError::new(
                            "interface transfer residual lost its signature plan",
                        )
                    })?;
                for input in &signature_call.matched_inputs {
                    if let Some(variable) = expression_variable(caller, input.expression) {
                        residual_types.push(alpha_normalized_resolved_type(
                            &mut unifier,
                            variable,
                            &mut residual_variables,
                            &mut next_residual_variable,
                        ));
                    }
                }
            }
            let residual_key = (
                provisional.result.fingerprint_v1(),
                own_module.fingerprint_v1(),
                residual_types,
            );
            let repeated_residual = !seen_transfer_surfaces.insert(residual_key);
            residual_key_elapsed += residual_key_started.elapsed();
            let pre_transfer_surface = current_surface.clone();
            let mut transfer_surface_changed = false;
            let mut evaluated_any = false;
            for (call_index, target) in staged_owner_transfers.iter().enumerate() {
                let Some(target) = target.as_ref() else {
                    continue;
                };
                let module = if own_module.owns_owner(target) {
                    own_module.as_ref()
                } else {
                    external_modules
                        .iter()
                        .find(|module| module.owns_owner(target))
                        .map(Arc::as_ref)
                        .ok_or_else(|| {
                            OwnerConstraintSeedError::new(format!(
                                "interface transfer environment cannot route target {target:?}"
                            ))
                        })?
                };
                let call = &calls[call_index];
                let caller = states.get(&call.caller).ok_or_else(|| {
                    OwnerConstraintSeedError::new("interface transfer call has no caller state")
                })?;
                let signature_call = caller
                    .signature_lexical_plan
                    .call(call.expression)
                    .ok_or_else(|| {
                        OwnerConstraintSeedError::new(
                            "interface transfer call disappeared from its signature plan",
                        )
                    })?;
                let residual_argument_started = Instant::now();
                let mut arguments = OwnerResidualDraftArguments::default();
                for input in &signature_call.matched_inputs {
                    if let Some(value) =
                        interface_transfer_expression_value(caller, input.expression, &mut unifier)
                    {
                        arguments.insert(input.formal_ordinal, value);
                    }
                }
                let explicit_context = signature_call.explicit_pass.as_ref().and_then(|pass| {
                    interface_transfer_expression_value(caller, pass.expression, &mut unifier)
                });
                let inherited_context = if explicit_context.is_none() {
                    effective_context_variable(caller)?.map(|variable| EvaluatedResultValue {
                        flow_type: FlowType {
                            mode: FlowMode::Continuous,
                            ty: unifier.resolve(&Type::Var(variable)),
                        },
                        parameter_derived: false,
                        syntax_selected: false,
                        static_number: None,
                    })
                } else {
                    None
                };
                residual_argument_elapsed += residual_argument_started.elapsed();
                let residual_evaluation_started = Instant::now();
                let evaluated = evaluate_owner_result_transfer_occurrence(
                    module,
                    target,
                    &arguments,
                    explicit_context.as_ref().or(inherited_context.as_ref()),
                    &mut unifier,
                )
                .ok_or_else(|| {
                    OwnerConstraintSeedError::new(format!(
                        "interface transfer module {:?} cannot evaluate target {target:?}",
                        module.key()
                    ))
                })?;
                residual_evaluation_elapsed += residual_evaluation_started.elapsed();
                let residual_publication_started = Instant::now();
                transfer_work.merge(evaluated.work);
                evaluated_any = true;
                let call_variable = caller.expressions[call.expression];
                let provider = evaluated.value.flow_type.ty;
                unifier.mark_authoritative_provider(call_variable);
                unifier.publish_derived_provider_epoch(call_variable, provider.clone());
                committed_transfer_results[call_index] = Some(provider);
                let state = states.get_mut(&call.caller).ok_or_else(|| {
                    OwnerConstraintSeedError::new(
                        "interface transfer call lost its mutable caller state",
                    )
                })?;
                let mode = flow_mode_join(
                    state.modes[call.expression],
                    Some(evaluated.value.flow_type.mode),
                );
                transfer_surface_changed |= mode != state.modes[call.expression];
                state.modes[call.expression] = mode;
                residual_publication_elapsed += residual_publication_started.elapsed();
            }
            if evaluated_any {
                transfer_iterations = transfer_iterations.saturating_add(1);
            }
            final_transfer_module = Some(Arc::clone(&own_module));
            let residual_post_replay_started = Instant::now();
            // Residual evaluation is another provider epoch: a transfer call
            // may feed an ordinary flow equation or be the value of an
            // authored HOLD update. Replay both before the acyclic fast path
            // projects and seals the public interface.
            if !flow_program.replay(&mut unifier) {
                return Err(OwnerConstraintSeedError::new(format!(
                    "interface component {:?} has a non-convergent post-transfer value-flow graph",
                    scc.key
                )));
            }
            transfer_surface_changed |=
                replay_interface_hold_constraints(&mut unifier, &mut hold_authorities, &states);
            write_solver_surface_snapshot(&mut unifier, &states, &mut current_surface);
            transfer_surface_changed |= current_surface != pre_transfer_surface;
            residual_post_replay_elapsed += residual_post_replay_started.elapsed();
            if !transfer_surface_changed {
                final_projection = Some(provisional);
                converged = true;
                break;
            }
            if !component_has_internal_edges && !has_hold_updates {
                // With frozen dependency interfaces and no edge back into the
                // component, a quiescent occurrence-transfer sweep cannot
                // alter any of its own inputs. Nested calls are evaluated by
                // the sealed transfer module itself, and true recursive call
                // edges were conservatively left on their principal above.
                // Re-entering the complete interface solver merely resolves
                // and hashes the same potentially enormous result tree two or
                // three more times. Project the changed public surface once
                // and seal its final module directly.
                let projected =
                    project_owner_interface_scc_result(scc, abi, &states, &mut unifier, work)?;
                let final_dependency_owners =
                    owner_interface_transfer_dependency_owners(&projected);
                if final_dependency_owners != module_dependency_owners {
                    return Err(OwnerConstraintSeedError::new(format!(
                        "interface component {:?} changed its transfer dependency set after an acyclic residual sweep",
                        scc.key
                    )));
                }
                let mut final_dependencies =
                    BTreeMap::<OwnerInterfaceSccKey, Arc<OwnerInterfaceTransferModule>>::new();
                for owner in &final_dependency_owners {
                    let module = external_modules
                        .iter()
                        .find(|module| module.owns_owner(owner))
                        .ok_or_else(|| {
                            OwnerConstraintSeedError::new(format!(
                                "interface component {:?} cannot route final transfer dependency {owner:?}",
                                scc.key
                            ))
                        })?;
                    final_dependencies.insert(module.key().clone(), Arc::clone(module));
                }
                final_transfer_module = Some(Arc::new(
                    project_owner_interface_transfer_module(
                        scc,
                        projected.clone(),
                        final_dependencies.into_values(),
                    )
                    .map_err(|error| {
                        OwnerConstraintSeedError::new(format!(
                            "cannot compile final acyclic interface component {:?} transfer residual: {error}",
                            scc.key
                        ))
                    })?,
                ));
                final_projection = Some(projected);
                converged = true;
                break;
            }
            if repeated_residual {
                return Err(OwnerConstraintSeedError::new(format!(
                    "owner interface SCC {:?} oscillated while evaluating transfer residuals",
                    scc.key
                )));
            }
        }

        // Publish the post-round (and, when applicable, post-transfer) surface
        // as the next convergence baseline without reconstructing the SCC.
        write_solver_surface_snapshot(&mut unifier, &states, &mut current_surface);
        std::mem::swap(&mut previous_surface, &mut current_surface);
    }
    if trace_rounds {
        eprintln!(
            "boon owner interface rounds members={} total_ms={:.3} calls_ms={:.3} provider_replay_ms={:.3} dynamic_replay_ms={:.3} stable_surface_ms={:.3} dependency_resolution_ms={:.3} residual_key_ms={:.3} residual_arguments_ms={:.3} residual_evaluation_ms={:.3} residual_publication_ms={:.3} residual_post_replay_ms={:.3}",
            scc.key.members.len(),
            rounds_started.elapsed().as_secs_f64() * 1_000.0,
            call_round_elapsed.as_secs_f64() * 1_000.0,
            provider_replay_elapsed.as_secs_f64() * 1_000.0,
            dynamic_replay_elapsed.as_secs_f64() * 1_000.0,
            stable_surface_elapsed.as_secs_f64() * 1_000.0,
            dependency_resolution_elapsed.as_secs_f64() * 1_000.0,
            residual_key_elapsed.as_secs_f64() * 1_000.0,
            residual_argument_elapsed.as_secs_f64() * 1_000.0,
            residual_evaluation_elapsed.as_secs_f64() * 1_000.0,
            residual_publication_elapsed.as_secs_f64() * 1_000.0,
            residual_post_replay_elapsed.as_secs_f64() * 1_000.0,
        );
    }
    if !converged {
        return Err(OwnerConstraintSeedError::new(format!(
            "owner interface SCC {:?} did not converge in {maximum_rounds} rounds",
            scc.key
        )));
    }

    let projection = match final_projection {
        Some(projection) => projection,
        None => project_owner_interface_scc_result(scc, abi, &states, &mut unifier, work)?,
    };
    let result = projection.result;
    let currentness = OwnerInterfaceSccCurrentnessReceipt::from_current_evaluation(basis, &result)?;
    Ok(OwnerInterfaceSccSolveOutput {
        evaluation: OwnerInterfaceSccEvaluation {
            currentness,
            result,
        },
        module: final_transfer_module,
        transfer_iterations,
        transfer_work,
    })
}

/// Direct convenience projection for callers that do not retain evaluator
/// currentness. Persistent request graphs should publish the evaluation and
/// semantic result as two request families.
pub fn solve_owner_interface_scc<'a>(
    scc: &OwnerInterfaceScc,
    abi: &OwnerInferenceAbiEnvironment,
    seeds: impl IntoIterator<Item = &'a OwnerConstraintSeed>,
    summaries: impl IntoIterator<Item = &'a OwnerConstraintSummary>,
    dependency_results: impl IntoIterator<Item = &'a OwnerInterfaceSccResult>,
) -> Result<OwnerInterfaceSccResult, OwnerConstraintSeedError> {
    evaluate_owner_interface_scc(scc, abi, seeds, summaries, dependency_results)
        .map(|evaluation| Arc::unwrap_or_clone(evaluation.result))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ResolvedOwnerSymbolReference, build_owner_callable_scope_topology,
        build_owner_interface_topology, evaluate_owner_callable_scope_scc,
        project_owner_callable_resolution_plan, project_owner_constraint_seed,
        project_owner_syntax_input, resolve_owner_constraint_seed,
        resolve_owner_constraint_seed_with_signature_plan,
    };
    use boon_parser::{
        ProjectSyntaxSnapshot, UnitSyntaxSnapshot, parse_project_source_unit,
        project_unit_link_keys,
    };
    use std::sync::Arc;

    fn link(source: &str) -> UnitSyntaxSnapshot {
        let parsed = parse_project_source_unit("app/RUN.bn", source).unwrap();
        let key = project_unit_link_keys(
            "app/RUN.bn",
            [(
                parsed.source_unit_id.clone(),
                parsed.declared_functions.clone(),
            )],
        )
        .unwrap()
        .remove(&parsed.source_unit_id)
        .unwrap();
        parsed.into_unit_syntax_snapshot(key).unwrap()
    }

    fn owner_named(unit: &UnitSyntaxSnapshot, name: &str) -> StableCheckOwnerKey {
        unit.stable_check_owner_keys()
            .find(|owner| {
                matches!(
                    owner,
                    StableCheckOwnerKey::Item(owner)
                        if owner.item_route.segments().last().is_some_and(|segment| segment.names == [name])
                )
            })
            .unwrap()
    }

    fn full_internal_capture(
        consumer: StableCheckOwnerKey,
        target: OwnerLexicalTargetRef,
        capture: TypeVar,
        provider: TypeVar,
    ) -> InternalLexicalCaptureProvider {
        InternalLexicalCaptureProvider {
            consumer,
            target,
            capture,
            provider,
            demand: LexicalCaptureDemand {
                full: true,
                children: BTreeMap::new(),
            },
        }
    }

    fn capture_demand(paths: &[&[&str]]) -> LexicalCaptureDemand {
        let paths = paths
            .iter()
            .map(|path| {
                path.iter()
                    .map(|field| (*field).to_owned())
                    .collect::<Vec<_>>()
                    .into_boxed_slice()
            })
            .collect::<Vec<_>>();
        LexicalCaptureDemand::from_paths(&paths).unwrap()
    }

    fn projected_internal_capture(
        consumer: StableCheckOwnerKey,
        target: OwnerLexicalTargetRef,
        capture: TypeVar,
        provider: TypeVar,
        paths: &[&[&str]],
    ) -> InternalLexicalCaptureProvider {
        InternalLexicalCaptureProvider {
            consumer,
            target,
            capture,
            provider,
            demand: capture_demand(paths),
        }
    }

    fn seed(unit: &UnitSyntaxSnapshot, owner: &StableCheckOwnerKey) -> OwnerConstraintSeed {
        let syntax = project_owner_syntax_input(unit.owner_view_for_key(owner).unwrap()).unwrap();
        project_owner_constraint_seed(&syntax).unwrap()
    }

    fn test_abi() -> crate::OwnerAbiEnvironment {
        let unit = link("value: 1\n");
        let project =
            ProjectSyntaxSnapshot::from_unit_snapshots("app/RUN.bn", vec![Arc::new(unit)]).unwrap();
        crate::project_owner_abi_environment(
            &project,
            &boon_checked::ExternalTypeEnvironment::empty(boon_checked::ProgramRole::Client),
        )
        .unwrap()
    }

    fn solve(
        seeds: &[OwnerConstraintSeed],
        summaries: &[OwnerConstraintSummary],
    ) -> Vec<OwnerInterfaceSccResult> {
        let abi_provider = test_abi();
        solve_with_provider(seeds, summaries, &abi_provider)
    }

    fn solve_with_provider(
        seeds: &[OwnerConstraintSeed],
        summaries: &[OwnerConstraintSummary],
        abi_provider: &crate::OwnerAbiEnvironment,
    ) -> Vec<OwnerInterfaceSccResult> {
        let seeds = seeds
            .iter()
            .map(|seed| (seed.owner.clone(), seed))
            .collect::<BTreeMap<_, _>>();
        let base_summaries = summaries
            .iter()
            .map(|summary| (summary.owner.clone(), summary))
            .collect::<BTreeMap<_, _>>();
        let callable_plans = seeds
            .iter()
            .map(|(owner, seed)| {
                (
                    owner.clone(),
                    project_owner_callable_resolution_plan(
                        seed,
                        base_summaries[owner]
                            .symbol_resolutions
                            .iter()
                            .filter(|resolution| {
                                resolution.reference().kind == OwnerReferenceKind::Callable
                            })
                            .cloned(),
                    )
                    .unwrap(),
                )
            })
            .collect::<BTreeMap<_, _>>();
        let callable_topology =
            build_owner_callable_scope_topology(callable_plans.values()).unwrap();
        let callable_provider = abi_provider.callable_environment().unwrap();
        let mut callable_results = BTreeMap::<
            crate::OwnerCallableScopeSccKey,
            Arc<crate::OwnerCallableScopeSccResult>,
        >::new();
        for scc in &callable_topology.sccs {
            let abi = callable_provider
                .inference_environment(
                    scc.key.members.iter().cloned(),
                    scc.key.members.iter().flat_map(|owner| {
                        callable_plans[owner].authoritative_abi_names().into_vec()
                    }),
                )
                .unwrap();
            let dependencies = scc
                .dependencies
                .iter()
                .map(|dependency| callable_results[dependency].as_ref())
                .collect::<Vec<_>>();
            let evaluation = evaluate_owner_callable_scope_scc(
                scc,
                scc.key.members.iter().map(|owner| seeds[owner]),
                scc.key.members.iter().map(|owner| &callable_plans[owner]),
                &abi,
                dependencies,
            )
            .unwrap();
            callable_results.insert(scc.key.clone(), evaluation.result);
        }
        let callable_scopes = callable_results
            .values()
            .flat_map(|result| result.owners.iter())
            .map(|scope| (scope.owner().clone(), scope))
            .collect::<BTreeMap<_, _>>();
        let summaries = seeds
            .iter()
            .map(|(owner, seed)| {
                let plan = callable_scopes[owner].lexical_plan();
                let resolutions = plan.external_candidates().iter().map(|reference| {
                    base_summaries[owner]
                        .symbol_resolutions
                        .iter()
                        .find(|resolution| resolution.reference() == reference)
                        .cloned()
                        .unwrap()
                });
                (
                    owner.clone(),
                    resolve_owner_constraint_seed_with_signature_plan(seed, plan, resolutions)
                        .unwrap(),
                )
            })
            .collect::<BTreeMap<_, _>>();
        let topology = build_owner_interface_topology(summaries.values()).unwrap();
        let mut results = BTreeMap::<OwnerInterfaceSccKey, Arc<OwnerInterfaceSccResult>>::new();
        for scc in &topology.sccs {
            let parameter_requirement_lookups = scc
                .key
                .members
                .iter()
                .flat_map(|owner| {
                    seeds[owner]
                        .parameter_requirement_keys()
                        .into_vec()
                        .into_iter()
                        .map(|key| {
                            let (function, parameter) = seeds[owner]
                                .parameter_requirement_names(key.parameter_ordinal())
                                .unwrap();
                            abi_provider
                                .parameter_requirement_lookup(key, function, parameter)
                                .unwrap()
                        })
                })
                .collect::<Vec<_>>();
            let abi = abi_provider
                .complete_inference_environment_with_requirements(
                    scc.key.members.iter().cloned(),
                    scc.key
                        .members
                        .iter()
                        .flat_map(|owner| summaries[owner].authoritative_abi_names().into_vec()),
                    scc.key.members.iter().flat_map(|owner| {
                        summaries[owner].authoritative_value_abi_paths().into_vec()
                    }),
                    scc.key
                        .members
                        .iter()
                        .flat_map(|owner| seeds[owner].source_payload_abi_paths().into_vec()),
                    parameter_requirement_lookups,
                )
                .unwrap();
            let dependencies = scc
                .dependencies
                .iter()
                .map(|dependency| results[dependency].as_ref())
                .collect::<Vec<_>>();
            let evaluation = evaluate_owner_interface_scc_with_signature_scopes(
                scc,
                &abi,
                scc.key.members.iter().map(|owner| seeds[owner]),
                scc.key.members.iter().map(|owner| &summaries[owner]),
                dependencies,
                scc.key.members.iter().map(|owner| callable_scopes[owner]),
            )
            .unwrap();
            results.insert(scc.key.clone(), evaluation.result);
        }
        topology
            .sccs
            .iter()
            .map(|scc| Arc::unwrap_or_clone(results.remove(&scc.key).unwrap()))
            .collect()
    }

    fn solve_components(
        seeds: &[OwnerConstraintSeed],
        summaries: &[OwnerConstraintSummary],
    ) -> BTreeMap<OwnerInterfaceSccKey, OwnerInterfaceSccComponentEvaluation> {
        let abi_provider = test_abi();
        let seeds = seeds
            .iter()
            .map(|seed| (seed.owner.clone(), seed))
            .collect::<BTreeMap<_, _>>();
        let base_summaries = summaries
            .iter()
            .map(|summary| (summary.owner.clone(), summary))
            .collect::<BTreeMap<_, _>>();
        let callable_plans = seeds
            .iter()
            .map(|(owner, seed)| {
                (
                    owner.clone(),
                    project_owner_callable_resolution_plan(
                        seed,
                        base_summaries[owner]
                            .symbol_resolutions
                            .iter()
                            .filter(|resolution| {
                                resolution.reference().kind == OwnerReferenceKind::Callable
                            })
                            .cloned(),
                    )
                    .unwrap(),
                )
            })
            .collect::<BTreeMap<_, _>>();
        let callable_topology =
            build_owner_callable_scope_topology(callable_plans.values()).unwrap();
        let callable_provider = abi_provider.callable_environment().unwrap();
        let mut callable_results = BTreeMap::<
            crate::OwnerCallableScopeSccKey,
            Arc<crate::OwnerCallableScopeSccResult>,
        >::new();
        for scc in &callable_topology.sccs {
            let abi = callable_provider
                .inference_environment(
                    scc.key.members.iter().cloned(),
                    scc.key.members.iter().flat_map(|owner| {
                        callable_plans[owner].authoritative_abi_names().into_vec()
                    }),
                )
                .unwrap();
            let dependencies = scc
                .dependencies
                .iter()
                .map(|dependency| callable_results[dependency].as_ref())
                .collect::<Vec<_>>();
            let evaluation = evaluate_owner_callable_scope_scc(
                scc,
                scc.key.members.iter().map(|owner| seeds[owner]),
                scc.key.members.iter().map(|owner| &callable_plans[owner]),
                &abi,
                dependencies,
            )
            .unwrap();
            callable_results.insert(scc.key.clone(), evaluation.result);
        }
        let callable_scopes = callable_results
            .values()
            .flat_map(|result| result.owners.iter())
            .map(|scope| (scope.owner().clone(), scope))
            .collect::<BTreeMap<_, _>>();
        let summaries = seeds
            .iter()
            .map(|(owner, seed)| {
                let plan = callable_scopes[owner].lexical_plan();
                let resolutions = plan.external_candidates().iter().map(|reference| {
                    base_summaries[owner]
                        .symbol_resolutions
                        .iter()
                        .find(|resolution| resolution.reference() == reference)
                        .cloned()
                        .unwrap()
                });
                (
                    owner.clone(),
                    resolve_owner_constraint_seed_with_signature_plan(seed, plan, resolutions)
                        .unwrap(),
                )
            })
            .collect::<BTreeMap<_, _>>();
        let topology = build_owner_interface_topology(summaries.values()).unwrap();
        let mut components =
            BTreeMap::<OwnerInterfaceSccKey, OwnerInterfaceSccComponentEvaluation>::new();
        for scc in &topology.sccs {
            let parameter_requirement_lookups = scc
                .key
                .members
                .iter()
                .flat_map(|owner| {
                    seeds[owner]
                        .parameter_requirement_keys()
                        .into_vec()
                        .into_iter()
                        .map(|key| {
                            let (function, parameter) = seeds[owner]
                                .parameter_requirement_names(key.parameter_ordinal())
                                .unwrap();
                            abi_provider
                                .parameter_requirement_lookup(key, function, parameter)
                                .unwrap()
                        })
                })
                .collect::<Vec<_>>();
            let abi = abi_provider
                .complete_inference_environment_with_requirements(
                    scc.key.members.iter().cloned(),
                    scc.key
                        .members
                        .iter()
                        .flat_map(|owner| summaries[owner].authoritative_abi_names().into_vec()),
                    scc.key.members.iter().flat_map(|owner| {
                        summaries[owner].authoritative_value_abi_paths().into_vec()
                    }),
                    scc.key
                        .members
                        .iter()
                        .flat_map(|owner| seeds[owner].source_payload_abi_paths().into_vec()),
                    parameter_requirement_lookups,
                )
                .unwrap();
            let dependencies = scc
                .dependencies
                .iter()
                .map(|dependency| components[dependency].evaluation.result.as_ref())
                .collect::<Vec<_>>();
            let component = evaluate_owner_interface_scc_component(
                scc,
                &abi,
                scc.key.members.iter().map(|owner| seeds[owner]),
                scc.key.members.iter().map(|owner| &summaries[owner]),
                dependencies,
                scc.key.members.iter().map(|owner| callable_scopes[owner]),
                |owners| {
                    let provider_keys = owners
                        .iter()
                        .map(|owner| {
                            topology
                                .scc_for_owner(owner)
                                .map(|provider| provider.key.clone())
                                .ok_or_else(|| format!("no transfer provider for {owner:?}"))
                        })
                        .collect::<Result<BTreeSet<_>, _>>()?;
                    provider_keys
                        .into_iter()
                        .map(|key| {
                            components
                                .get(&key)
                                .map(|provider| Arc::clone(&provider.module))
                                .ok_or_else(|| {
                                    format!("transfer provider {key:?} is not dependency-first")
                                })
                        })
                        .collect()
                },
            )
            .unwrap();
            components.insert(scc.key.clone(), component);
        }
        components
    }

    #[derive(Debug, Eq, PartialEq)]
    struct NormalizedCheckedParameter {
        name: String,
        kind: OwnerParameterKind,
        ordinal: u32,
        flow_type: FlowType,
        requirement: CheckedParameterRequirement,
        evaluation_scope: OwnerInterfaceEvaluationScope,
    }

    #[derive(Debug, Eq, PartialEq)]
    struct NormalizedCheckedInterface {
        parameters: Vec<NormalizedCheckedParameter>,
        result: FlowType,
        context: Option<OwnerContextInterface>,
        effect: CheckedEffectSummary,
    }

    fn checked_callable_interface(source: &str, name: &str) -> NormalizedCheckedInterface {
        checked_callable_interface_with_external(
            source,
            name,
            &boon_checked::ExternalTypeEnvironment::empty(boon_checked::ProgramRole::Client),
        )
    }

    fn checked_callable_interface_with_external(
        source: &str,
        name: &str,
        external: &boon_checked::ExternalTypeEnvironment,
    ) -> NormalizedCheckedInterface {
        let parsed = boon_parser::parse_project(
            "app/RUN.bn",
            [("app/RUN.bn".to_owned(), source.to_owned())],
        )
        .unwrap();
        let checked = crate::check_program_with_external_types(&parsed, external);
        let fields = checked.checked_program_fields().unwrap();
        let callable = fields
            .callables
            .iter()
            .find(|callable| {
                callable.kind == boon_checked::CheckedCallableKind::User && callable.name == name
            })
            .unwrap();
        assert_eq!(callable.kind, boon_checked::CheckedCallableKind::User);
        assert_eq!(callable.intrinsic, None);
        assert_eq!(callable.external_identity, None);
        assert!(callable.contexts.is_empty());
        assert_eq!(callable.contextual_operation, None);
        assert_eq!(callable.role, boon_checked::ProgramRole::Client);
        let mut variables = BTreeMap::new();
        let mut next = 0;
        let parameters = callable
            .parameters
            .iter()
            .map(|parameter| NormalizedCheckedParameter {
                name: parameter.name.clone(),
                kind: match parameter.kind {
                    CheckedParameterKind::Value => OwnerParameterKind::Value,
                    CheckedParameterKind::Out => OwnerParameterKind::Out,
                },
                ordinal: u32::try_from(parameter.ordinal).unwrap(),
                flow_type: FlowType {
                    mode: parameter.flow_type.mode,
                    ty: alpha_normalize_type(&parameter.flow_type.ty, &mut variables, &mut next),
                },
                requirement: parameter.requirement.clone(),
                evaluation_scope: match parameter.evaluation_scope {
                    boon_checked::CheckedEvaluationScope::Parent => {
                        OwnerInterfaceEvaluationScope::Parent
                    }
                    boon_checked::CheckedEvaluationScope::Output { formal } => {
                        let parameter_ordinal = callable
                            .parameters
                            .iter()
                            .find(|parameter| parameter.decl_id == formal)
                            .map(|parameter| u32::try_from(parameter.ordinal).unwrap())
                            .unwrap();
                        OwnerInterfaceEvaluationScope::Output { parameter_ordinal }
                    }
                },
            })
            .collect();
        let result = FlowType {
            mode: callable.result.mode,
            ty: alpha_normalize_type(&callable.result.ty, &mut variables, &mut next),
        };
        let context = callable.context_formal.map(|formal| {
            let formal = fields.context_formal(formal).unwrap();
            OwnerContextInterface {
                flow_type: FlowType {
                    mode: formal.scheme.flow_type.mode,
                    ty: alpha_normalize_type(
                        &formal.scheme.flow_type.ty,
                        &mut variables,
                        &mut next,
                    ),
                },
                projections: formal
                    .scheme
                    .projections
                    .iter()
                    .cloned()
                    .map(Vec::into_boxed_slice)
                    .collect::<Vec<_>>()
                    .into_boxed_slice(),
            }
        });
        NormalizedCheckedInterface {
            parameters,
            result,
            context,
            effect: callable.effect,
        }
    }

    fn assert_checked_interface_parity(source: &str, name: &str) {
        let unit = link(source);
        let owner = owner_named(&unit, name);
        let seed = seed(&unit, &owner);
        let summary = resolve_owner_constraint_seed(&seed, []).unwrap();
        let results = solve(&[seed], &[summary]);
        let interface = results
            .iter()
            .find_map(|result| result.owner(&owner))
            .unwrap();
        let checked = checked_callable_interface(source, name);
        let parameters = interface
            .parameters
            .iter()
            .map(|parameter| NormalizedCheckedParameter {
                name: parameter.name.clone(),
                kind: parameter.kind,
                ordinal: parameter.ordinal,
                flow_type: parameter.flow_type.clone(),
                requirement: parameter.requirement.clone(),
                evaluation_scope: parameter.evaluation_scope,
            })
            .collect::<Vec<_>>();
        assert_eq!(parameters, checked.parameters, "{name} parameter interface");
        assert_eq!(interface.result, checked.result, "{name} result interface");
        assert_eq!(
            interface.context, checked.context,
            "{name} context interface"
        );
        assert_eq!(interface.effect, checked.effect, "{name} effect interface");
    }

    #[test]
    fn variable_free_shared_types_are_reused_across_owner_inference_transforms() {
        let ty = Type::object(ObjectShape {
            fields: [(
                "nested".to_owned(),
                Type::object(ObjectShape {
                    fields: [("value".to_owned(), Type::Number)].into(),
                    field_order: vec!["value".to_owned()],
                    open: false,
                }),
            )]
            .into(),
            field_order: vec!["nested".to_owned()],
            open: true,
        });
        let Type::Object(original) = &ty else {
            unreachable!();
        };

        let mut unifier = TypeUnifier::default();
        let resolved = unifier.resolve(&ty);
        let instantiated = instantiate_type(&ty, &mut unifier, &mut BTreeMap::new());
        let normalized = alpha_normalize_type(&ty, &mut BTreeMap::new(), &mut 0);

        for transformed in [&resolved, &instantiated, &normalized] {
            let Type::Object(transformed) = transformed else {
                panic!("variable-free object type changed representation");
            };
            assert!(boon_checked::SharedObjectShape::ptr_eq(
                original,
                transformed
            ));
        }
    }

    #[test]
    fn one_source_flow_alias_preserves_a_semantic_union() {
        let mut unifier = TypeUnifier::default();
        let source = unifier.fresh();
        let alias = unifier.fresh();
        let union = boon_checked::canonical_union_type(vec![Type::Number, Type::Text]);
        unifier.bind_var(source, union.clone());

        bind_flow_variables(&mut unifier, alias, [source]);

        assert_eq!(unifier.resolve(&Type::Var(alias)), union);
    }

    #[test]
    fn heterogeneous_collection_items_use_structural_widening_without_producer_equality() {
        let source = concat!(
            "FUNCTION rows() {\n",
            "    LIST {\n",
            "        [\n",
            "            kind: Header\n",
            "            file: TEXT { a }\n",
            "        ]\n",
            "        [\n",
            "            kind: Empty\n",
            "            file: TEXT { b }\n",
            "        ]\n",
            "    }\n",
            "}\n",
        );
        let unit = link(source);
        let owners = unit.stable_check_owner_keys().collect::<Vec<_>>();
        let seeds = owners
            .iter()
            .map(|owner| seed(&unit, owner))
            .collect::<Vec<_>>();
        let summaries = seeds
            .iter()
            .map(|seed| resolve_owner_constraint_seed(seed, []).unwrap())
            .collect::<Vec<_>>();
        let results = solve(&seeds, &summaries);
        let owner = owner_named(&unit, "rows");
        let interface = results
            .iter()
            .find_map(|result| result.owner(&owner))
            .unwrap();
        let Type::List(item) = &interface.result.ty else {
            panic!("rows must return a list: {interface:#?}")
        };
        let Type::Object(item) = item.as_ref() else {
            panic!("rows must structurally widen record items: {interface:#?}")
        };
        assert_eq!(item.fields.get("file"), Some(&Type::Text));
        assert_eq!(
            item.fields.get("kind"),
            Some(&Type::VariantSet(
                vec![
                    Variant::Tag("Empty".to_owned()),
                    Variant::Tag("Header".to_owned()),
                ]
                .into(),
            ))
        );
    }

    #[test]
    fn hold_authority_does_not_specialize_its_initializer_and_carries_prior_epochs() {
        let tag = |name: &str| Type::VariantSet(vec![Variant::Tag(name.to_owned())].into());
        let mut unifier = TypeUnifier::default();
        let initializer = unifier.fresh();
        let output = unifier.fresh();
        let provider = unifier.fresh();
        let update = unifier.fresh();
        unifier.bind_var(
            update,
            boon_checked::canonical_union_type(vec![Type::Var(output), Type::Var(provider)]),
        );
        let mut authorities = BTreeMap::new();

        unifier.publish_authoritative_provider(provider, tag("ASCII"));
        assert!(bind_hold_variables(
            &mut unifier,
            &mut authorities,
            output,
            initializer,
            [update],
        ));
        assert!(matches!(
            unifier.resolve(&Type::Var(initializer)),
            Type::Var(_)
        ));
        assert_eq!(
            unifier.resolve(&Type::Var(output)),
            boon_checked::canonical_union_type(vec![Type::Var(initializer), tag("ASCII"),])
        );

        // A later authoritative provider epoch must extend the private state
        // approximation instead of restarting from the initializer or reading
        // consumer-shaped output history.
        unifier.publish_authoritative_provider(provider, tag("Hexadecimal"));
        assert!(bind_hold_variables(
            &mut unifier,
            &mut authorities,
            output,
            initializer,
            [update],
        ));
        assert_eq!(
            unifier.resolve(&Type::Var(output)),
            boon_checked::canonical_union_type(vec![
                Type::Var(initializer),
                tag("ASCII"),
                tag("Hexadecimal"),
            ])
        );
        assert!(matches!(
            unifier.resolve(&Type::Var(initializer)),
            Type::Var(_)
        ));
    }

    #[test]
    fn hold_authority_refreshes_projected_reads_without_backflowing_root_aliases() {
        let tag = |name: &str| Type::VariantSet(vec![Variant::Tag(name.to_owned())].into());
        let mut unifier = TypeUnifier::default();
        let initializer = unifier.fresh();
        unifier.bind_var(
            initializer,
            Type::object(ObjectShape::from_ordered_fields(
                [("value".to_owned(), tag("Initial"))],
                false,
            )),
        );
        let output = unifier.fresh();
        unifier.mark_authoritative_provider(output);
        let projected = bind_projection(&mut unifier, output, &["value".to_owned()]);
        let mut authorities = BTreeMap::new();

        assert!(bind_hold_variables(
            &mut unifier,
            &mut authorities,
            output,
            initializer,
            [],
        ));
        assert_eq!(unifier.resolve(&Type::Var(projected)), tag("Initial"));

        let update = unifier.fresh();
        unifier.bind_var(
            update,
            Type::object(ObjectShape::from_ordered_fields(
                [("value".to_owned(), tag("Update"))],
                false,
            )),
        );
        assert!(bind_hold_variables(
            &mut unifier,
            &mut authorities,
            output,
            initializer,
            [update],
        ));
        assert_eq!(
            unifier.resolve(&Type::Var(projected)),
            Type::VariantSet(
                vec![
                    Variant::Tag("Initial".to_owned()),
                    Variant::Tag("Update".to_owned()),
                ]
                .into(),
            )
        );
        assert_eq!(
            unifier.resolve(&Type::Var(initializer)),
            Type::object(ObjectShape::from_ordered_fields(
                [("value".to_owned(), tag("Initial"))],
                false,
            ))
        );
    }

    #[test]
    fn hold_authority_does_not_backflow_through_nested_generic_initializer_fields() {
        let mut unifier = TypeUnifier::default();
        let parameter = unifier.fresh();
        let initializer = unifier.fresh();
        unifier.bind_var(
            initializer,
            Type::object(ObjectShape::from_ordered_fields(
                [("value".to_owned(), Type::Var(parameter))],
                false,
            )),
        );
        let output = unifier.fresh();
        unifier.mark_authoritative_provider(output);
        let projected = bind_projection(&mut unifier, output, &["value".to_owned()]);
        let mut authorities = BTreeMap::new();
        assert!(bind_hold_variables(
            &mut unifier,
            &mut authorities,
            output,
            initializer,
            [],
        ));

        let update = unifier.fresh();
        unifier.bind_var(
            update,
            Type::object(ObjectShape::from_ordered_fields(
                [("value".to_owned(), Type::Text)],
                false,
            )),
        );
        assert!(bind_hold_variables(
            &mut unifier,
            &mut authorities,
            output,
            initializer,
            [update],
        ));

        assert!(matches!(
            unifier.resolve(&Type::Var(parameter)),
            Type::Var(_)
        ));
        assert_eq!(unifier.resolve(&Type::Var(projected)), Type::Text);
    }

    #[test]
    fn structural_flow_refreshes_projected_reads_without_backflowing_sources() {
        let mut unifier = TypeUnifier::default();
        let parameter = unifier.fresh();
        let source = unifier.fresh();
        let second = unifier.fresh();
        let generic = Type::object(ObjectShape::from_ordered_fields(
            [("value".to_owned(), Type::Var(parameter))],
            false,
        ));
        unifier.bind_var(source, generic.clone());
        unifier.bind_var(second, generic);
        let output = unifier.fresh();
        let mut constraints = Vec::new();
        bind_and_record_structural_flow_variables(
            &mut unifier,
            &mut constraints,
            output,
            [source, second],
        );
        let projected = bind_projection(&mut unifier, output, &["value".to_owned()]);

        unifier.replace_authoritative_binding(
            second,
            Type::object(ObjectShape::from_ordered_fields(
                [("value".to_owned(), Type::Text)],
                false,
            )),
        );
        assert!(replay_flow_constraints(&mut unifier, &constraints));

        assert!(matches!(
            unifier.resolve(&Type::Var(parameter)),
            Type::Var(_)
        ));
        assert_eq!(unifier.resolve(&Type::Var(projected)), Type::Text);
    }

    #[test]
    fn mutation_journal_retains_binding_union_and_same_live_authority_epochs() {
        let mut unifier = TypeUnifier::default();
        let left = unifier.fresh();
        let right = unifier.fresh();
        let first_cursor = unifier.mutation_cursor();
        let second_cursor = unifier.mutation_cursor();

        unifier.bind_var(left, Type::Text);
        unifier.union(left, right);
        let after_union = unifier.mutation_cursor();
        let changes = unifier.changes();
        unifier.publish_authoritative_provider(left, Type::Text);

        let first = unifier.mutations_since(first_cursor);
        let second = unifier.mutations_since(second_cursor);
        assert_eq!(first, second, "independent consumers must see one journal");
        assert!(
            first
                .iter()
                .any(|event| { event.variable == left && event.kind == TypeMutationKind::Binding })
        );
        assert!(
            first
                .iter()
                .any(|event| event.variable == left && event.kind == TypeMutationKind::Union)
        );
        assert!(
            first
                .iter()
                .any(|event| event.variable == right && event.kind == TypeMutationKind::Union)
        );
        assert_eq!(
            unifier.mutations_since(after_union),
            &[TypeMutationEvent {
                variable: unifier.root_readonly(left),
                kind: TypeMutationKind::Authority,
            }],
        );
        assert_eq!(
            unifier.changes(),
            changes,
            "same-live authority epochs must wake dependents without changing convergence",
        );
    }

    #[test]
    fn flow_worklist_propagates_a_late_provider_through_reverse_order_constraints() {
        let tag = |name: &str| Type::VariantSet(vec![Variant::Tag(name.to_owned())].into());
        let mut unifier = TypeUnifier::default();
        let source = unifier.fresh();
        unifier.replace_authoritative_binding(source, tag("Initial"));
        let first = unifier.fresh();
        let second = unifier.fresh();
        let third = unifier.fresh();
        let mut constraints = Vec::new();
        bind_and_record_flow_variables(&mut unifier, &mut constraints, first, [source]);
        bind_and_record_flow_variables(&mut unifier, &mut constraints, second, [first]);
        bind_and_record_flow_variables(&mut unifier, &mut constraints, third, [second]);
        constraints.reverse();
        let mut program = OwnerFlowConstraintProgram::new(&mut unifier, constraints);
        assert!(program.replay(&mut unifier));

        unifier.replace_authoritative_binding(source, tag("Final"));
        assert!(program.replay(&mut unifier));
        assert_eq!(unifier.resolve(&Type::Var(first)), tag("Final"));
        assert_eq!(unifier.resolve(&Type::Var(second)), tag("Final"));
        assert_eq!(unifier.resolve(&Type::Var(third)), tag("Final"));
    }

    #[test]
    fn flow_worklist_watches_nested_dependencies_discovered_during_initial_replay() {
        let mut unifier = TypeUnifier::default();
        let nested = unifier.fresh();
        let left = unifier.fresh();
        let right = unifier.fresh();
        let output = unifier.fresh();
        let constraint = OwnerFlowConstraint {
            output,
            inputs: vec![left, right].into_boxed_slice(),
            kind: OwnerFlowConstraintKind::StructuralWiden,
        };
        let mut program = OwnerFlowConstraintProgram::new(&mut unifier, vec![constraint]);
        unifier.replace_authoritative_binding(left, Type::List(Type::shared(Type::Var(nested))));
        unifier.replace_authoritative_binding(right, Type::List(Type::shared(Type::Number)));
        assert!(program.replay(&mut unifier));
        assert_eq!(
            unifier.resolve(&Type::Var(output)),
            Type::List(Type::shared(Type::Number))
        );

        unifier.replace_authoritative_binding(nested, Type::Text);
        assert!(program.replay(&mut unifier));
        assert_eq!(
            unifier.resolve(&Type::Var(output)),
            Type::List(Type::shared(boon_checked::widen_structural_type(
                &Type::Text,
                &Type::Number,
            )))
        );
    }

    #[test]
    fn flow_worklist_reasserts_an_equation_after_its_output_is_overwritten() {
        let mut unifier = TypeUnifier::default();
        let source = unifier.fresh();
        let output = unifier.fresh();
        unifier.replace_authoritative_binding(source, Type::Text);
        let constraint = OwnerFlowConstraint {
            output,
            inputs: vec![source].into_boxed_slice(),
            kind: OwnerFlowConstraintKind::Union,
        };
        let mut program = OwnerFlowConstraintProgram::new(&mut unifier, vec![constraint]);
        assert!(program.replay(&mut unifier));
        assert_eq!(unifier.resolve(&Type::Var(output)), Type::Text);

        unifier.replace_authoritative_binding(output, Type::Number);
        assert!(program.replay(&mut unifier));
        assert_eq!(unifier.resolve(&Type::Var(output)), Type::Text);
    }

    #[test]
    fn derived_provider_projection_graph_refreshes_nested_and_missing_paths() {
        let tag = |name: &str| Type::VariantSet(vec![Variant::Tag(name.to_owned())].into());
        let provider = |value: Type| {
            Type::object(ObjectShape::from_ordered_fields(
                [(
                    "outer".to_owned(),
                    Type::object(ObjectShape::from_ordered_fields(
                        [("value".to_owned(), value)],
                        false,
                    )),
                )],
                false,
            ))
        };
        let mut unifier = TypeUnifier::default();
        let root = unifier.fresh();
        unifier.mark_authoritative_provider(root);
        unifier.replace_derived_provider(root, provider(tag("First")));
        let outer = bind_projection(&mut unifier, root, &["outer".to_owned()]);
        let value = bind_projection(&mut unifier, outer, &["value".to_owned()]);

        unifier.replace_derived_provider(root, provider(tag("Second")));
        assert_eq!(unifier.resolve(&Type::Var(value)), tag("Second"));

        unifier.replace_derived_provider(
            root,
            Type::object(ObjectShape::from_ordered_fields(
                [("different".to_owned(), Type::Text)],
                false,
            )),
        );
        assert!(matches!(
            unifier.resolve(&Type::Var(outer)),
            Type::UnresolvedShape { .. }
        ));
        assert!(matches!(
            unifier.resolve(&Type::Var(value)),
            Type::UnresolvedShape { .. }
        ));
    }

    #[test]
    fn projection_recorded_before_authority_promotion_survives_root_union() {
        let mut unifier = TypeUnifier::default();
        let formal = unifier.fresh();
        let projected = bind_projection(&mut unifier, formal, &["value".to_owned()]);
        let authority = unifier.fresh();
        unifier.mark_authoritative_provider(authority);
        unifier.unify(Type::Var(authority), Type::Var(formal));
        unifier.replace_derived_provider(
            authority,
            Type::object(ObjectShape::from_ordered_fields(
                [("value".to_owned(), Type::Text)],
                false,
            )),
        );

        assert_eq!(unifier.resolve(&Type::Var(projected)), Type::Text);
    }

    #[test]
    fn whole_value_projection_is_detached_from_an_authoritative_provider() {
        let mut unifier = TypeUnifier::default();
        let provider = unifier.fresh();
        unifier.mark_authoritative_provider(provider);
        unifier.replace_derived_provider(provider, Type::Text);
        let occurrence = bind_projection(&mut unifier, provider, &[]);
        assert_ne!(unifier.root(provider), unifier.root(occurrence));

        unifier.bind_var(occurrence, Type::Number);
        assert_eq!(unifier.resolve(&Type::Var(provider)), Type::Text);
        unifier.replace_derived_provider(provider, Type::Number);
        assert_eq!(unifier.resolve(&Type::Var(occurrence)), Type::Number);
    }

    #[test]
    fn hold_containment_does_not_drop_fields_from_tagged_payloads() {
        let unifier = TypeUnifier::default();
        let tagged = |fields: Vec<(String, Type)>| {
            Type::VariantSet(
                vec![Variant::Tagged {
                    tag: "State".to_owned(),
                    fields: ObjectShape::from_ordered_fields::<ObjectShape>(fields, false).into(),
                }]
                .into(),
            )
        };
        let current = tagged(vec![
            ("x".to_owned(), Type::Number),
            ("y".to_owned(), Type::Text),
        ]);
        let narrower = tagged(vec![("x".to_owned(), Type::Number)]);

        assert!(!hold_update_contains_current(&unifier, &current, &narrower,));
    }

    #[test]
    fn hold_updates_do_not_backflow_into_a_generic_record_parameter() {
        let source = concat!(
            "store: [elements: [ascii: SOURCE hex: SOURCE]]\n",
            "FUNCTION stateful(row) {\n",
            "    row.formatter |> HOLD formatter {\n",
            "        LATEST {\n",
            "            store.elements.ascii |> THEN { ASCII }\n",
            "            store.elements.hex |> THEN { Hexadecimal }\n",
            "        }\n",
            "    }\n",
            "}\n",
        );
        let unit = link(source);
        let owner = owner_named(&unit, "stateful");
        let seed = seed(&unit, &owner);
        let summary = resolve_owner_constraint_seed(&seed, []).unwrap();
        let results = solve(&[seed], &[summary]);
        let interface = results[0].owner(&owner).unwrap();
        let Type::Object(parameter) = &interface.parameters[0].flow_type.ty else {
            panic!("stateful row must retain an object parameter: {interface:#?}")
        };
        assert!(matches!(
            parameter.fields.get("formatter"),
            Some(Type::Var(_))
        ));
        assert_eq!(
            interface.result.ty,
            Type::VariantSet(
                vec![
                    Variant::Tag("ASCII".to_owned()),
                    Variant::Tag("Hexadecimal".to_owned()),
                ]
                .into(),
            )
        );
    }

    #[test]
    fn late_exact_binding_discards_flow_aliases_coalesced_into_their_own_root() {
        let mut unifier = TypeUnifier::default();
        let value = unifier.fresh();
        let flush = unifier.fresh();
        let boundary = unifier.fresh();
        let alias = unifier.fresh();

        unifier.bind_flow_result(
            boundary,
            boon_checked::canonical_union_type(vec![Type::Var(value), Type::Var(flush)]),
        );
        unifier.bind_flow_result(value, Type::Var(alias));
        unifier.bind_flow_result(flush, Type::Var(alias));
        unifier.unify(Type::Var(boundary), Type::Var(alias));
        unifier.bind_var(boundary, Type::Number);

        assert_eq!(unifier.resolve(&Type::Var(boundary)), Type::Number);
    }

    #[test]
    fn authored_empty_collection_holes_inherit_only_compatible_concrete_siblings() {
        let mut unifier = TypeUnifier::default();
        let empty_item = unifier.fresh_contextual_hole();
        let joined = unifier.fresh();
        unifier.bind_flow_result(
            joined,
            boon_checked::canonical_union_type(vec![
                Type::List(Type::shared(Type::Var(empty_item))),
                Type::List(Type::shared(Type::Number)),
            ]),
        );

        unifier.refine_contextual_flow_holes();

        assert_eq!(unifier.resolve(&Type::Var(empty_item)), Type::Number);
        assert_eq!(
            unifier.resolve(&Type::Var(joined)),
            Type::List(Type::shared(Type::Number)),
        );

        let mismatched_item = unifier.fresh_contextual_hole();
        unifier.unify(
            boon_checked::canonical_union_type(vec![
                Type::List(Type::shared(Type::Var(mismatched_item))),
                Type::Number,
            ]),
            Type::Number,
        );
        assert!(matches!(
            unifier.resolve(&Type::Var(mismatched_item)),
            Type::Var(_)
        ));

        let ambiguous_item = unifier.fresh_contextual_hole();
        let ambiguous_join = unifier.fresh();
        unifier.bind_flow_result(
            ambiguous_join,
            boon_checked::canonical_union_type(vec![
                Type::List(Type::shared(Type::Var(ambiguous_item))),
                Type::List(Type::shared(Type::object(
                    ObjectShape::from_ordered_fields([("x".to_owned(), Type::Number)], false),
                ))),
                Type::List(Type::shared(Type::object(
                    ObjectShape::from_ordered_fields([("y".to_owned(), Type::Number)], false),
                ))),
            ]),
        );
        unifier.refine_contextual_flow_holes();
        assert!(matches!(
            unifier.resolve(&Type::Var(ambiguous_item)),
            Type::Var(_)
        ));

        let outer = unifier.fresh();
        let inner = unifier.fresh();
        let outer_empty = unifier.fresh_contextual_hole();
        let inner_empty = unifier.fresh_contextual_hole();
        unifier.bind_flow_result(
            outer,
            boon_checked::canonical_union_type(vec![
                Type::Var(inner),
                Type::List(Type::shared(Type::Var(outer_empty))),
            ]),
        );
        unifier.bind_flow_result(
            inner,
            boon_checked::canonical_union_type(vec![
                Type::List(Type::shared(Type::Var(inner_empty))),
                Type::List(Type::shared(Type::Text)),
            ]),
        );
        unifier.refine_contextual_flow_holes();
        assert_eq!(unifier.resolve(&Type::Var(inner_empty)), Type::Text);
        assert_eq!(unifier.resolve(&Type::Var(outer_empty)), Type::Text);
        assert_eq!(
            unifier.resolve(&Type::Var(outer)),
            Type::List(Type::shared(Type::Text)),
        );
    }

    #[test]
    fn ordinary_union_producers_never_receive_contextual_backflow() {
        let mut unifier = TypeUnifier::default();
        let parameter = unifier.fresh();
        let requirement = Type::object(ObjectShape::from_ordered_fields(
            [("generated".to_owned(), Type::Number)],
            false,
        ));
        let joined =
            boon_checked::canonical_union_type(vec![Type::Var(parameter), requirement.clone()]);

        unifier.unify(joined, requirement);

        assert!(matches!(
            unifier.resolve(&Type::Var(parameter)),
            Type::Var(_)
        ));
    }

    #[test]
    fn flow_join_binding_is_resolved_before_a_later_structural_equation() {
        fn row_type() -> Type {
            Type::object(ObjectShape {
                fields: [("time".to_owned(), Type::Number)].into(),
                field_order: vec!["time".to_owned()],
                open: false,
            })
        }

        let mut unifier = TypeUnifier::default();
        let empty_item = unifier.fresh();
        let projected_branch = unifier.fresh();
        let joined = unifier.fresh();
        unifier.bind_flow_result(
            joined,
            boon_checked::canonical_union_type(vec![
                Type::List(Type::shared(Type::Var(empty_item))),
                Type::Var(projected_branch),
            ]),
        );
        unifier.bind_var(projected_branch, Type::List(Type::shared(row_type())));
        unifier.bind_var(empty_item, row_type());
        assert_eq!(
            unifier.resolve(&Type::Var(joined)),
            Type::List(Type::shared(row_type())),
        );

        let expected_item = unifier.fresh();
        unifier.bind_var(joined, Type::List(Type::shared(Type::Var(expected_item))));

        assert_eq!(
            unifier.resolve(&Type::Var(joined)),
            Type::List(Type::shared(row_type())),
        );
        assert_eq!(unifier.resolve(&Type::Var(expected_item)), row_type());
    }

    #[test]
    fn repeated_identical_live_equation_does_not_materialize_its_resolved_tree() {
        let mut unifier = TypeUnifier::default();
        let item = unifier.fresh();
        let equivalent_item = unifier.fresh();
        let result = unifier.fresh();
        let template = Type::List(Type::shared(Type::Var(item)));
        unifier.bind_var(result, template.clone());
        unifier.unify(Type::Var(item), Type::Var(equivalent_item));
        unifier.bind_var(item, Type::Number);
        let changes_before = unifier.changes();

        unifier.unify(
            Type::Var(result),
            Type::List(Type::shared(Type::Var(equivalent_item))),
        );
        unifier.unify(Type::Var(result), Type::List(Type::shared(Type::Number)));

        assert_eq!(unifier.changes(), changes_before);
        assert_eq!(
            unifier.resolve(&Type::Var(result)),
            Type::List(Type::shared(Type::Number)),
        );
    }

    #[test]
    fn every_capture_of_one_open_provider_contributes_before_openness_is_reclassified() {
        fn required_field(name: &str, ty: Type) -> Type {
            Type::object(ObjectShape {
                fields: [(name.to_owned(), ty)].into(),
                field_order: vec![name.to_owned()],
                open: true,
            })
        }

        fn solve(reverse: bool) -> Type {
            let unit = link("provider: 0\nfirst: 0\nsecond: 0\n");
            let provider_owner = owner_named(&unit, "provider");
            let first_owner = owner_named(&unit, "first");
            let second_owner = owner_named(&unit, "second");
            let target = OwnerLexicalTargetRef::Declaration {
                owner: provider_owner,
                declaration: OwnerDeclarationStableKey::Public,
                capability: OwnerLexicalDeclarationCapability::Value,
            };
            let mut unifier = TypeUnifier::default();
            let provider = unifier.fresh();
            let first = unifier.fresh();
            let second = unifier.fresh();
            unifier.bind_var(first, required_field("number", Type::Number));
            unifier.bind_var(second, required_field("text", Type::Text));
            let mut providers = vec![
                full_internal_capture(first_owner, target.clone(), first, provider),
                full_internal_capture(second_owner, target, second, provider),
            ];
            if reverse {
                providers.reverse();
            }
            let states = BTreeMap::<StableCheckOwnerKey, OwnerSolveState<'_>>::new();
            propagate_lexical_capture_types(
                &states,
                &providers,
                &mut BTreeMap::new(),
                &mut BTreeMap::new(),
                true,
                &mut unifier,
            )
            .unwrap();
            unifier.resolve(&Type::Var(provider))
        }

        let Type::Object(forward) = solve(false) else {
            panic!("generic provider must collect both capture requirements");
        };
        let Type::Object(reverse) = solve(true) else {
            panic!("generic provider must collect both reversed requirements");
        };
        assert_eq!(forward.fields, reverse.fields);
        assert_eq!(forward.fields.get("number"), Some(&Type::Number));
        assert_eq!(forward.fields.get("text"), Some(&Type::Text));
    }

    #[test]
    fn full_capture_backflow_adds_a_child_field_to_an_open_provider_object() {
        let unit = link("provider: 0\nconsumer: 0\n");
        let provider_owner = owner_named(&unit, "provider");
        let consumer_owner = owner_named(&unit, "consumer");
        let target = OwnerLexicalTargetRef::ContextFormal {
            owner: provider_owner,
        };
        let mut unifier = TypeUnifier::default();
        let direct = unifier.fresh();
        let provider = unifier.fresh();
        unifier.bind_var(
            provider,
            Type::object(ObjectShape::from_ordered_fields(
                [(
                    "store".to_owned(),
                    Type::object(ObjectShape::from_ordered_fields(
                        [(
                            "elements".to_owned(),
                            Type::object(ObjectShape::from_ordered_fields(
                                [("collapse".to_owned(), Type::Var(direct))],
                                true,
                            )),
                        )],
                        true,
                    )),
                )],
                true,
            )),
        );
        let capture = unifier.fresh();
        unifier.bind_var(
            capture,
            Type::object(ObjectShape::from_ordered_fields(
                [(
                    "store".to_owned(),
                    Type::object(ObjectShape::from_ordered_fields(
                        [
                            (
                                "elements".to_owned(),
                                Type::object(ObjectShape::from_ordered_fields(
                                    [("collapse".to_owned(), Type::Number)],
                                    true,
                                )),
                            ),
                            ("active_scope".to_owned(), Type::Text),
                        ],
                        true,
                    )),
                )],
                true,
            )),
        );
        let providers = [full_internal_capture(
            consumer_owner,
            target,
            capture,
            provider,
        )];
        let states = BTreeMap::<StableCheckOwnerKey, OwnerSolveState<'_>>::new();
        let mut frames = BTreeMap::new();
        let mut surfaces = BTreeMap::new();

        propagate_lexical_capture_types(
            &states,
            &providers,
            &mut frames,
            &mut surfaces,
            true,
            &mut unifier,
        )
        .unwrap();
        propagate_lexical_capture_types(
            &states,
            &providers,
            &mut frames,
            &mut surfaces,
            true,
            &mut unifier,
        )
        .unwrap();

        let Type::Object(provider) = unifier.resolve(&Type::Var(provider)) else {
            panic!("whole context provider must remain an object");
        };
        let Type::Object(store) = &provider.fields["store"] else {
            panic!("whole context provider must retain its store object");
        };
        assert_eq!(store.fields.get("active_scope"), Some(&Type::Text));
        assert_eq!(unifier.resolve(&Type::Var(direct)), Type::Number);

        let Type::Object(capture) = unifier.resolve(&Type::Var(capture)) else {
            panic!("whole context capture must remain an object");
        };
        let Type::Object(store) = &capture.fields["store"] else {
            panic!("whole context capture must retain its store object");
        };
        assert_eq!(store.fields.get("active_scope"), Some(&Type::Text));
    }

    #[test]
    fn lexical_capture_surface_prunes_unread_object_siblings_and_shares_selected_alphas() {
        let mut unifier = TypeUnifier::default();
        let shared = unifier.fresh();
        let omitted = unifier.fresh();
        let provider = unifier.fresh();
        unifier.bind_var(
            provider,
            Type::object(ObjectShape {
                fields: [
                    (
                        "session".to_owned(),
                        Type::object(ObjectShape {
                            fields: [
                                ("left".to_owned(), Type::Var(shared)),
                                ("right".to_owned(), Type::Var(shared)),
                            ]
                            .into(),
                            field_order: vec!["left".to_owned(), "right".to_owned()],
                            open: false,
                        }),
                    ),
                    ("unrelated".to_owned(), Type::Var(omitted)),
                ]
                .into(),
                field_order: vec!["session".to_owned(), "unrelated".to_owned()],
                open: false,
            }),
        );

        let surface = unifier.resolve_lexical_capture_surface(
            provider,
            &capture_demand(&[&["session", "left"], &["session", "right"]]),
            &mut Vec::new(),
        );
        let Type::Object(root) = &surface else {
            panic!("projected capture root must remain an object");
        };
        assert!(!root.open);
        assert!(!root.fields.contains_key("unrelated"));
        let Type::Object(session) = &root.fields["session"] else {
            panic!("selected object prefix must remain structural");
        };
        assert_eq!(session.field_order, ["left", "right"]);
        assert_eq!(session.fields["left"], session.fields["right"]);

        let mut frame = BTreeMap::new();
        let instantiated = instantiate_type(&surface, &mut unifier, &mut frame);
        assert_eq!(
            frame.len(),
            1,
            "omitted provider alpha entered capture frame"
        );
        let Type::Object(root) = instantiated else {
            panic!("instantiated capture root must remain an object");
        };
        let Type::Object(session) = &root.fields["session"] else {
            panic!("instantiated selected prefix must remain an object");
        };
        assert_eq!(session.fields["left"], session.fields["right"]);
    }

    #[test]
    fn lexical_capture_surface_keeps_union_terminals_correlated() {
        let choice = Type::Union(vec![
            Type::object(ObjectShape::from_ordered_fields(
                [("x".to_owned(), Type::Number), ("y".to_owned(), Type::Text)],
                false,
            )),
            Type::object(ObjectShape::from_ordered_fields(
                [("x".to_owned(), Type::Text), ("y".to_owned(), Type::Number)],
                false,
            )),
        ]);
        let provider = Type::object(ObjectShape::from_ordered_fields(
            [
                ("choice".to_owned(), choice.clone()),
                ("unrelated".to_owned(), Type::Bytes(BytesType::Fixed(8))),
            ],
            false,
        ));

        let surface = capture_demand(&[&["choice", "x"]]).project_resolved(&provider);
        let Type::Object(surface) = surface else {
            panic!("capture surface must retain its object root");
        };
        assert!(!surface.open);
        assert_eq!(surface.fields.len(), 1);
        assert_eq!(surface.fields.get("choice"), Some(&choice));
    }

    #[test]
    fn projected_capture_alpha_keeps_its_relative_demand_after_late_shape_binding() {
        let unit = link("provider: 0\nconsumer: 0\n");
        let provider_owner = owner_named(&unit, "provider");
        let consumer_owner = owner_named(&unit, "consumer");
        let target = OwnerLexicalTargetRef::Declaration {
            owner: provider_owner,
            declaration: OwnerDeclarationStableKey::Public,
            capability: OwnerLexicalDeclarationCapability::Value,
        };
        let mut unifier = TypeUnifier::default();
        let provider = unifier.fresh();
        let omitted = unifier.fresh();
        let capture = unifier.fresh();
        let providers = [projected_internal_capture(
            consumer_owner,
            target,
            capture,
            provider,
            &[&["selected"]],
        )];
        let states = BTreeMap::<StableCheckOwnerKey, OwnerSolveState<'_>>::new();
        let mut frames = BTreeMap::new();
        let mut surfaces = BTreeMap::new();

        propagate_lexical_capture_types(
            &states,
            &providers,
            &mut frames,
            &mut surfaces,
            false,
            &mut unifier,
        )
        .unwrap();
        unifier.bind_var(
            provider,
            Type::object(ObjectShape::from_ordered_fields(
                [
                    ("selected".to_owned(), Type::Number),
                    ("omitted".to_owned(), Type::Var(omitted)),
                ],
                false,
            )),
        );
        propagate_lexical_capture_types(
            &states,
            &providers,
            &mut frames,
            &mut surfaces,
            true,
            &mut unifier,
        )
        .unwrap();

        let Type::Object(surface) = unifier.resolve(&Type::Var(capture)) else {
            panic!("late-bound projected capture must become an object");
        };
        assert!(!surface.open);
        assert_eq!(surface.fields.get("selected"), Some(&Type::Number));
        assert!(!surface.fields.contains_key("omitted"));
        assert!(matches!(unifier.resolve(&Type::Var(omitted)), Type::Var(_)));

        let changes = unifier.changes();
        propagate_lexical_capture_types(
            &states,
            &providers,
            &mut frames,
            &mut surfaces,
            true,
            &mut unifier,
        )
        .unwrap();
        assert_eq!(unifier.changes(), changes);
    }

    #[test]
    fn late_authoritative_capture_replaces_an_earlier_consumer_scaffold() {
        let unit = link("provider: 0\nconsumer: 0\n");
        let provider_owner = owner_named(&unit, "provider");
        let consumer_owner = owner_named(&unit, "consumer");
        let target = OwnerLexicalTargetRef::Declaration {
            owner: provider_owner,
            declaration: OwnerDeclarationStableKey::Public,
            capability: OwnerLexicalDeclarationCapability::Value,
        };
        let mut unifier = TypeUnifier::default();
        let provider = unifier.fresh();
        let capture = unifier.fresh();
        let providers = [full_internal_capture(
            consumer_owner,
            target,
            capture,
            provider,
        )];
        let states = BTreeMap::<StableCheckOwnerKey, OwnerSolveState<'_>>::new();
        let mut frames = BTreeMap::new();
        let mut surfaces = BTreeMap::new();

        propagate_lexical_capture_types(
            &states,
            &providers,
            &mut frames,
            &mut surfaces,
            false,
            &mut unifier,
        )
        .unwrap();
        unifier.bind_var(
            capture,
            Type::List(Type::shared(Type::object(
                ObjectShape::from_ordered_fields([("id".to_owned(), Type::Unknown)], true),
            ))),
        );

        let rich = Type::List(Type::shared(Type::object(
            ObjectShape::from_ordered_fields(
                [
                    ("id".to_owned(), Type::Text),
                    ("family".to_owned(), Type::Text),
                ],
                false,
            ),
        )));
        unifier.bind_var(provider, rich.clone());
        propagate_lexical_capture_types(
            &states,
            &providers,
            &mut frames,
            &mut surfaces,
            true,
            &mut unifier,
        )
        .unwrap();

        assert_eq!(
            unifier.resolve(&Type::Var(capture)),
            rich,
            "a late provider epoch must replace, not union with, the consumer scaffold",
        );
    }

    #[test]
    fn late_capture_alpha_refresh_replaces_its_consumer_scaffold() {
        let unit = link("provider: 0\nconsumer: 0\n");
        let provider_owner = owner_named(&unit, "provider");
        let consumer_owner = owner_named(&unit, "consumer");
        let target = OwnerLexicalTargetRef::Declaration {
            owner: provider_owner,
            declaration: OwnerDeclarationStableKey::Public,
            capability: OwnerLexicalDeclarationCapability::Value,
        };
        let mut unifier = TypeUnifier::default();
        let provider_item = unifier.fresh();
        let provider = unifier.fresh();
        unifier.bind_var(provider, Type::List(Type::shared(Type::Var(provider_item))));
        let capture = unifier.fresh();
        let providers = [full_internal_capture(
            consumer_owner,
            target,
            capture,
            provider,
        )];
        let states = BTreeMap::<StableCheckOwnerKey, OwnerSolveState<'_>>::new();
        let mut frames = BTreeMap::new();
        let mut surfaces = BTreeMap::new();

        propagate_lexical_capture_types(
            &states,
            &providers,
            &mut frames,
            &mut surfaces,
            false,
            &mut unifier,
        )
        .unwrap();
        let copied_item = frames
            .values()
            .next()
            .and_then(|frame| frame.variables.get(&unifier.root_readonly(provider_item)))
            .copied()
            .expect("capture must copy the provider item alpha");
        unifier.bind_var(
            copied_item,
            Type::object(ObjectShape::from_ordered_fields(
                [("id".to_owned(), Type::Unknown)],
                true,
            )),
        );

        let rich_item = Type::object(ObjectShape::from_ordered_fields(
            [
                ("id".to_owned(), Type::Text),
                ("payload".to_owned(), Type::Text),
            ],
            false,
        ));
        unifier.bind_var(provider_item, rich_item.clone());
        propagate_lexical_capture_types(
            &states,
            &providers,
            &mut frames,
            &mut surfaces,
            true,
            &mut unifier,
        )
        .unwrap();

        assert_eq!(
            unifier.resolve(&Type::Var(copied_item)),
            rich_item.clone(),
            "the provider alpha must replace its copied consumer scaffold",
        );
        assert_eq!(
            unifier.resolve(&Type::Var(capture)),
            Type::List(Type::shared(rich_item)),
            "the capture and its consumer-shared item alpha must close together",
        );
    }

    #[test]
    fn projected_capture_backflows_only_a_concrete_sparse_requirement() {
        let unit = link("provider: 0\nconsumer: 0\n");
        let provider_owner = owner_named(&unit, "provider");
        let consumer_owner = owner_named(&unit, "consumer");
        let target = OwnerLexicalTargetRef::Declaration {
            owner: provider_owner,
            declaration: OwnerDeclarationStableKey::Public,
            capability: OwnerLexicalDeclarationCapability::Value,
        };
        let mut unifier = TypeUnifier::default();
        let provider = unifier.fresh();
        let capture = unifier.fresh();
        unifier.bind_var(
            capture,
            Type::object(ObjectShape::from_ordered_fields(
                [("selected".to_owned(), Type::Number)],
                true,
            )),
        );
        let providers = [projected_internal_capture(
            consumer_owner,
            target,
            capture,
            provider,
            &[&["selected"]],
        )];
        let states = BTreeMap::<StableCheckOwnerKey, OwnerSolveState<'_>>::new();
        let mut frames = BTreeMap::new();
        let mut surfaces = BTreeMap::new();
        propagate_lexical_capture_types(
            &states,
            &providers,
            &mut frames,
            &mut surfaces,
            true,
            &mut unifier,
        )
        .unwrap();

        let Type::Object(provider_shape) = unifier.resolve(&Type::Var(provider)) else {
            panic!("concrete sparse consumer requirement must shape the provider hole");
        };
        assert!(provider_shape.open);
        assert_eq!(provider_shape.fields.get("selected"), Some(&Type::Number));
        assert_eq!(provider_shape.fields.len(), 1);

        unifier.bind_var(
            provider,
            Type::object(ObjectShape::from_ordered_fields(
                [
                    ("selected".to_owned(), Type::Number),
                    ("omitted".to_owned(), Type::Text),
                ],
                false,
            )),
        );
        propagate_lexical_capture_types(
            &states,
            &providers,
            &mut frames,
            &mut surfaces,
            true,
            &mut unifier,
        )
        .unwrap();
        let Type::Object(capture_shape) = unifier.resolve(&Type::Var(capture)) else {
            panic!("projected capture must remain an object");
        };
        assert_eq!(capture_shape.fields.get("selected"), Some(&Type::Number));
        assert!(!capture_shape.fields.contains_key("omitted"));
    }

    #[test]
    fn closed_sparse_selector_does_not_synthesize_an_impossible_inherited_arm() {
        let mut unifier = TypeUnifier::default();
        let root = unifier.fresh();
        let read = unifier.fresh();
        let other = Type::VariantSet(vec![Variant::Tag("Other".to_owned())].into());
        unifier.bind_var(
            root,
            Type::object(ObjectShape::from_ordered_fields(
                [("choice".to_owned(), other.clone())],
                false,
            )),
        );
        let pattern = OwnerPatternConstraint::Tag {
            name: "Found".to_owned(),
            fields: Box::new([]),
        };
        let schematic = pattern_type(&pattern, &mut unifier);
        refine_owner_inherited_pattern_narrowings(
            &mut unifier,
            &[OwnerInheritedPatternNarrowing {
                root,
                frames: vec![OwnerInstantiatedPatternFrame {
                    projection: vec!["choice".to_owned()].into_boxed_slice(),
                    pattern,
                    schematic,
                }]
                .into_boxed_slice(),
                reads: vec![OwnerInstantiatedPatternRead {
                    projection: vec!["choice".to_owned()].into_boxed_slice(),
                    variable: read,
                }]
                .into_boxed_slice(),
            }],
        );

        assert_eq!(unifier.resolve(&Type::Var(read)), other);
    }

    #[test]
    fn closed_inherited_selector_replaces_a_stale_generic_branch_join() {
        let mut unifier = TypeUnifier::default();
        let root = unifier.fresh();
        let read = unifier.fresh();
        let stale = unifier.fresh();
        unifier.bind_var(
            root,
            Type::VariantSet(
                vec![Variant::Tagged {
                    tag: "WaveformOpened".to_owned(),
                    fields: boon_checked::SharedObjectShape::new(ObjectShape::from_ordered_fields(
                        [("timescale_unit".to_owned(), Type::Text)],
                        false,
                    )),
                }]
                .into(),
            ),
        );
        unifier.bind_var(
            read,
            boon_checked::canonical_union_type(vec![Type::Text, Type::Var(stale)]),
        );
        let pattern = OwnerPatternConstraint::Tag {
            name: "WaveformOpened".to_owned(),
            fields: Box::new([]),
        };
        let schematic = pattern_type(&pattern, &mut unifier);

        refine_owner_inherited_pattern_narrowings(
            &mut unifier,
            &[OwnerInheritedPatternNarrowing {
                root,
                frames: vec![OwnerInstantiatedPatternFrame {
                    projection: Box::new([]),
                    pattern,
                    schematic,
                }]
                .into_boxed_slice(),
                reads: vec![OwnerInstantiatedPatternRead {
                    projection: vec!["timescale_unit".to_owned()].into_boxed_slice(),
                    variable: read,
                }]
                .into_boxed_slice(),
            }],
        );

        assert_eq!(unifier.resolve(&Type::Var(read)), Type::Text);
    }

    #[test]
    fn closed_union_payload_authoritatively_closes_a_projected_pattern_binding() {
        let mut unifier = TypeUnifier::default();
        let selector = unifier.fresh();
        let binding = unifier.fresh();
        let pattern = OwnerPatternConstraint::Tag {
            name: "Found".to_owned(),
            fields: vec!["value".to_owned()].into_boxed_slice(),
        };
        let schematic = pattern_type(&pattern, &mut unifier);
        let binding_type = pattern_binding_type_from_pattern(&pattern, &schematic, "value")
            .expect("Found[value] must expose its payload binding");
        unifier.bind_var(binding, binding_type);
        let digest = bind_projection(&mut unifier, binding, &["digest".to_owned()]);
        let source = bind_projection(&mut unifier, binding, &["source".to_owned()]);
        let fallback = unifier.fresh();
        unifier.bind_var(fallback, Type::Text);
        let result = unifier.fresh();
        let mut flow_constraints = Vec::new();
        bind_and_record_flow_variables(
            &mut unifier,
            &mut flow_constraints,
            result,
            [digest, fallback],
        );
        let source_fallback = unifier.fresh();
        let no_file = Type::VariantSet(vec![Variant::Tag("NoFile".to_owned())].into());
        unifier.bind_var(source_fallback, no_file.clone());
        let source_result = unifier.fresh();
        bind_and_record_flow_variables(
            &mut unifier,
            &mut flow_constraints,
            source_result,
            [source, source_fallback],
        );

        let payload = |source: &str| {
            Type::object(ObjectShape::from_ordered_fields(
                [
                    ("digest".to_owned(), Type::Text),
                    (
                        "source".to_owned(),
                        Type::VariantSet(vec![Variant::Tag(source.to_owned())].into()),
                    ),
                ],
                false,
            ))
        };
        let found = |payload: Type| {
            Type::VariantSet(
                vec![Variant::Tagged {
                    tag: "Found".to_owned(),
                    fields: boon_checked::SharedObjectShape::new(ObjectShape::from_ordered_fields(
                        [("value".to_owned(), payload)],
                        false,
                    )),
                }]
                .into(),
            )
        };
        let initial_payload = payload("NoFile");
        let payload = boon_checked::canonical_union_type(vec![
            initial_payload.clone(),
            payload("FileHeader"),
        ]);
        let narrowing = OwnerPatternNarrowing {
            selector,
            pattern,
            bindings: vec![("value".to_owned(), binding)].into_boxed_slice(),
            binding_reads: vec![
                (
                    "value".to_owned(),
                    vec!["digest".to_owned()].into_boxed_slice(),
                    digest,
                ),
                (
                    "value".to_owned(),
                    vec!["source".to_owned()].into_boxed_slice(),
                    source,
                ),
            ]
            .into_boxed_slice(),
            selector_reads: Box::new([]),
        };

        unifier.publish_authoritative_provider(selector, found(initial_payload));
        refine_owner_pattern_narrowings(&mut unifier, std::slice::from_ref(&narrowing));
        assert!(replay_flow_constraints(&mut unifier, &flow_constraints));
        assert_eq!(unifier.resolve(&Type::Var(source_result)), no_file);

        unifier.publish_authoritative_provider(selector, found(payload.clone()));
        refine_owner_pattern_narrowings(&mut unifier, std::slice::from_ref(&narrowing));
        assert!(replay_flow_constraints(&mut unifier, &flow_constraints));

        assert_eq!(unifier.resolve(&Type::Var(binding)), payload);
        assert_eq!(unifier.resolve(&Type::Var(digest)), Type::Text);
        assert_eq!(unifier.resolve(&Type::Var(result)), Type::Text);
        assert_eq!(
            unifier.resolve(&Type::Var(source_result)),
            Type::VariantSet(
                vec![
                    Variant::Tag("FileHeader".to_owned()),
                    Variant::Tag("NoFile".to_owned()),
                ]
                .into(),
            )
        );
    }

    #[test]
    fn found_union_payload_projection_widens_the_public_when_result() {
        let unit = link(concat!(
            "store: [\n",
            "    rows: LIST {\n",
            "        [source: NoFile, no_file: True]\n",
            "        [source: FileHeader, file_header: True]\n",
            "    }\n",
            "    record: rows |> List/find(item, if: True)\n",
            "    result:\n",
            "        record |> WHEN {\n",
            "            Found[value] => value.source\n",
            "            NotFound => NoFile\n",
            "        }\n",
            "]\n",
        ));
        let store = owner_named(&unit, "store");
        let result = owner_named(&unit, "result");
        let seeds = unit
            .stable_check_owner_keys()
            .map(|owner| seed(&unit, &owner))
            .collect::<Vec<_>>();
        let summaries = seeds
            .iter()
            .map(|seed| resolve_owner_constraint_seed(seed, []).unwrap())
            .collect::<Vec<_>>();
        let interfaces = solve(&seeds, &summaries)
            .into_iter()
            .flat_map(|component| component.owners.into_vec())
            .map(|interface| (interface.owner.clone(), interface))
            .collect::<BTreeMap<_, _>>();

        assert_eq!(
            interfaces[&result].result.ty,
            Type::VariantSet(
                vec![
                    Variant::Tag("FileHeader".to_owned()),
                    Variant::Tag("NoFile".to_owned()),
                ]
                .into(),
            )
        );
        let Type::Object(store) = &interfaces[&store].result.ty else {
            panic!("store must publish an object: {:#?}", interfaces[&store]);
        };
        assert_eq!(
            store.fields.get("result"),
            Some(&interfaces[&result].result.ty)
        );
    }

    #[test]
    fn closed_narrowed_selector_replaces_a_sparse_consumer_projection() {
        let mut unifier = TypeUnifier::default();
        let selector = unifier.fresh();
        let read = unifier.fresh();
        let transition = Type::object(ObjectShape::from_ordered_fields(
            [
                ("time".to_owned(), Type::Number),
                ("end_time".to_owned(), Type::Number),
            ],
            false,
        ));
        let signal = Type::object(ObjectShape::from_ordered_fields(
            [
                ("signal_id".to_owned(), Type::Text),
                (
                    "transitions".to_owned(),
                    Type::List(Type::shared(transition)),
                ),
            ],
            false,
        ));
        let signals = Type::List(Type::shared(signal));
        unifier.bind_var(
            selector,
            Type::VariantSet(
                vec![Variant::Tagged {
                    tag: "SignalPage".to_owned(),
                    fields: boon_checked::SharedObjectShape::new(ObjectShape::from_ordered_fields(
                        [("signals".to_owned(), signals.clone())],
                        false,
                    )),
                }]
                .into(),
            ),
        );
        let sparse_id = unifier.fresh();
        unifier.bind_var(
            read,
            Type::List(Type::shared(Type::object(
                ObjectShape::from_ordered_fields(
                    [("signal_id".to_owned(), Type::Var(sparse_id))],
                    true,
                ),
            ))),
        );

        refine_owner_pattern_narrowings(
            &mut unifier,
            &[OwnerPatternNarrowing {
                selector,
                pattern: OwnerPatternConstraint::Tag {
                    name: "SignalPage".to_owned(),
                    fields: Box::new([]),
                },
                bindings: Box::new([]),
                binding_reads: Box::new([]),
                selector_reads: vec![(vec!["signals".to_owned()].into_boxed_slice(), read)]
                    .into_boxed_slice(),
            }],
        );

        assert_eq!(unifier.resolve(&Type::Var(sparse_id)), Type::Text);
        assert_eq!(unifier.resolve(&Type::Var(read)), signals);
    }

    #[test]
    fn projected_selector_openness_ignores_unrelated_open_siblings() {
        let mut unifier = TypeUnifier::default();
        let root = unifier.fresh();
        let read = unifier.fresh();
        let unrelated = unifier.fresh();
        let other = Type::VariantSet(vec![Variant::Tag("Other".to_owned())].into());
        unifier.bind_var(
            root,
            Type::object(ObjectShape {
                fields: [
                    ("choice".to_owned(), other.clone()),
                    ("unrelated".to_owned(), Type::Var(unrelated)),
                ]
                .into(),
                field_order: vec!["choice".to_owned(), "unrelated".to_owned()],
                open: true,
            }),
        );
        let pattern = OwnerPatternConstraint::Tag {
            name: "Found".to_owned(),
            fields: Box::new([]),
        };
        let schematic = pattern_type(&pattern, &mut unifier);
        refine_owner_inherited_pattern_narrowings(
            &mut unifier,
            &[OwnerInheritedPatternNarrowing {
                root,
                frames: vec![OwnerInstantiatedPatternFrame {
                    projection: vec!["choice".to_owned()].into_boxed_slice(),
                    pattern,
                    schematic,
                }]
                .into_boxed_slice(),
                reads: vec![OwnerInstantiatedPatternRead {
                    projection: vec!["choice".to_owned()].into_boxed_slice(),
                    variable: read,
                }]
                .into_boxed_slice(),
            }],
        );

        assert_eq!(unifier.resolve(&Type::Var(read)), other);
    }

    #[test]
    fn projected_capture_backflow_never_reaches_omitted_sibling_alphas() {
        let unit = link("provider: 0\nconsumer: 0\n");
        let provider_owner = owner_named(&unit, "provider");
        let consumer_owner = owner_named(&unit, "consumer");
        let target = OwnerLexicalTargetRef::Declaration {
            owner: provider_owner,
            declaration: OwnerDeclarationStableKey::Public,
            capability: OwnerLexicalDeclarationCapability::Value,
        };
        let mut unifier = TypeUnifier::default();
        let selected = unifier.fresh();
        let omitted = unifier.fresh();
        let provider = unifier.fresh();
        let capture = unifier.fresh();
        unifier.bind_var(
            provider,
            Type::object(ObjectShape::from_ordered_fields(
                [
                    ("selected".to_owned(), Type::Var(selected)),
                    ("omitted".to_owned(), Type::Var(omitted)),
                ],
                false,
            )),
        );
        unifier.bind_var(
            capture,
            Type::object(ObjectShape::from_ordered_fields(
                [("selected".to_owned(), Type::Number)],
                true,
            )),
        );
        let states = BTreeMap::<StableCheckOwnerKey, OwnerSolveState<'_>>::new();
        propagate_lexical_capture_types(
            &states,
            &[projected_internal_capture(
                consumer_owner,
                target,
                capture,
                provider,
                &[&["selected"]],
            )],
            &mut BTreeMap::new(),
            &mut BTreeMap::new(),
            true,
            &mut unifier,
        )
        .unwrap();

        assert_eq!(unifier.resolve(&Type::Var(selected)), Type::Number);
        assert!(matches!(unifier.resolve(&Type::Var(omitted)), Type::Var(_)));
    }

    #[test]
    fn projected_capture_backflows_a_nested_requirement_added_after_publication() {
        let unit = link("provider: 0\nconsumer: 0\n");
        let provider_owner = owner_named(&unit, "provider");
        let consumer_owner = owner_named(&unit, "consumer");
        let target = OwnerLexicalTargetRef::Declaration {
            owner: provider_owner,
            declaration: OwnerDeclarationStableKey::Public,
            capability: OwnerLexicalDeclarationCapability::Value,
        };
        let mut unifier = TypeUnifier::default();
        let selected = unifier.fresh();
        let omitted = unifier.fresh();
        let provider = unifier.fresh();
        let capture = unifier.fresh();
        unifier.bind_var(
            provider,
            Type::object(ObjectShape::from_ordered_fields(
                [
                    ("selected".to_owned(), Type::Var(selected)),
                    ("omitted".to_owned(), Type::Var(omitted)),
                ],
                false,
            )),
        );
        let states = BTreeMap::<StableCheckOwnerKey, OwnerSolveState<'_>>::new();
        let providers = [projected_internal_capture(
            consumer_owner,
            target,
            capture,
            provider,
            &[&["selected", "value"]],
        )];
        let mut frames = BTreeMap::new();
        let mut surfaces = BTreeMap::new();

        propagate_lexical_capture_types(
            &states,
            &providers,
            &mut frames,
            &mut surfaces,
            false,
            &mut unifier,
        )
        .unwrap();
        assert!(matches!(
            unifier.resolve(&Type::Var(selected)),
            Type::Var(_)
        ));
        let frame = frames.values().next().expect("projected alpha frame");
        assert!(
            frame
                .variables
                .contains_key(&unifier.root_readonly(selected))
        );
        assert!(
            !frame
                .variables
                .contains_key(&unifier.root_readonly(omitted))
        );

        let value = bind_projection(
            &mut unifier,
            capture,
            &["selected".to_owned(), "value".to_owned()],
        );
        unifier.bind_var(value, Type::Number);
        propagate_lexical_capture_types(
            &states,
            &providers,
            &mut frames,
            &mut surfaces,
            true,
            &mut unifier,
        )
        .unwrap();

        let Type::Object(selected) = unifier.resolve(&Type::Var(selected)) else {
            panic!("late nested consumer requirement must shape the selected provider alpha");
        };
        assert!(selected.open);
        assert_eq!(selected.fields.get("value"), Some(&Type::Number));
        assert!(matches!(unifier.resolve(&Type::Var(omitted)), Type::Var(_)));
    }

    #[test]
    fn capture_frames_canonicalize_provider_roots_that_coalesce_between_rounds() {
        let unit = link("provider: 0\nconsumer: 0\n");
        let provider_owner = owner_named(&unit, "provider");
        let consumer_owner = owner_named(&unit, "consumer");
        let target = OwnerLexicalTargetRef::Declaration {
            owner: provider_owner,
            declaration: OwnerDeclarationStableKey::Public,
            capability: OwnerLexicalDeclarationCapability::Value,
        };
        let mut unifier = TypeUnifier::default();
        let left = unifier.fresh();
        let right = unifier.fresh();
        let provider = unifier.fresh();
        let capture = unifier.fresh();
        unifier.bind_var(
            provider,
            Type::object(ObjectShape::from_ordered_fields(
                [
                    ("left".to_owned(), Type::Var(left)),
                    ("right".to_owned(), Type::Var(right)),
                ],
                false,
            )),
        );
        let states = BTreeMap::<StableCheckOwnerKey, OwnerSolveState<'_>>::new();
        let providers = [projected_internal_capture(
            consumer_owner,
            target,
            capture,
            provider,
            &[&["left"], &["right"]],
        )];
        let mut frames = BTreeMap::new();
        let mut surfaces = BTreeMap::new();
        propagate_lexical_capture_types(
            &states,
            &providers,
            &mut frames,
            &mut surfaces,
            false,
            &mut unifier,
        )
        .unwrap();
        assert_eq!(frames.values().next().unwrap().variables.len(), 2);

        unifier.unify(Type::Var(left), Type::Var(right));
        propagate_lexical_capture_types(
            &states,
            &providers,
            &mut frames,
            &mut surfaces,
            false,
            &mut unifier,
        )
        .unwrap();

        assert_eq!(frames.values().next().unwrap().variables.len(), 1);
        let Type::Object(capture) = unifier.resolve(&Type::Var(capture)) else {
            panic!("capture must retain both selected paths");
        };
        assert_eq!(capture.fields["left"], capture.fields["right"]);
    }

    #[test]
    fn lexical_capture_refreshes_provider_variables_closed_by_a_later_round() {
        let unit = link("provider: 0\nconsumer: 0\n");
        let provider_owner = owner_named(&unit, "provider");
        let consumer_owner = owner_named(&unit, "consumer");
        let target = OwnerLexicalTargetRef::Declaration {
            owner: provider_owner,
            declaration: OwnerDeclarationStableKey::Public,
            capability: OwnerLexicalDeclarationCapability::Value,
        };
        let mut unifier = TypeUnifier::default();
        let provider = unifier.fresh();
        let first = unifier.fresh();
        let second = unifier.fresh();
        let capture = unifier.fresh();
        unifier.bind_var(
            provider,
            Type::Union(vec![Type::Var(first), Type::Var(second)]),
        );
        let states = BTreeMap::<StableCheckOwnerKey, OwnerSolveState<'_>>::new();
        let providers = [full_internal_capture(
            consumer_owner,
            target,
            capture,
            provider,
        )];
        let mut copied_variables = BTreeMap::new();
        let mut provider_types = BTreeMap::new();

        propagate_lexical_capture_types(
            &states,
            &providers,
            &mut copied_variables,
            &mut provider_types,
            false,
            &mut unifier,
        )
        .unwrap();
        assert!(type_contains_inference_variable(
            &unifier.resolve(&Type::Var(capture))
        ));

        unifier.bind_var(first, Type::Text);
        unifier.bind_var(second, Type::Number);
        propagate_lexical_capture_types(
            &states,
            &providers,
            &mut copied_variables,
            &mut provider_types,
            false,
            &mut unifier,
        )
        .unwrap();

        let capture = unifier.resolve(&Type::Var(capture));
        assert!(!type_contains_inference_variable(&capture));
        assert_eq!(
            capture,
            boon_checked::canonical_union_type(vec![Type::Text, Type::Number])
        );
    }

    #[test]
    fn capture_backflow_binds_nested_holes_without_rewriting_concrete_siblings() {
        let unit = link("provider: 0\nconsumer: 0\n");
        let provider_owner = owner_named(&unit, "provider");
        let consumer_owner = owner_named(&unit, "consumer");
        let target = OwnerLexicalTargetRef::Declaration {
            owner: provider_owner,
            declaration: OwnerDeclarationStableKey::Public,
            capability: OwnerLexicalDeclarationCapability::Value,
        };
        let mut unifier = TypeUnifier::default();
        let provider = unifier.fresh();
        let nested_hole = unifier.fresh();
        let capture = unifier.fresh();
        unifier.bind_var(
            provider,
            Type::object(ObjectShape {
                fields: [
                    ("known".to_owned(), Type::Text),
                    (
                        "generic".to_owned(),
                        Type::List(Type::shared(Type::Var(nested_hole))),
                    ),
                ]
                .into(),
                field_order: vec!["known".to_owned(), "generic".to_owned()],
                open: false,
            }),
        );
        unifier.bind_var(
            capture,
            Type::object(ObjectShape {
                fields: [
                    ("known".to_owned(), Type::Number),
                    ("generic".to_owned(), Type::List(Type::shared(Type::Number))),
                ]
                .into(),
                field_order: vec!["known".to_owned(), "generic".to_owned()],
                open: true,
            }),
        );
        let states = BTreeMap::<StableCheckOwnerKey, OwnerSolveState<'_>>::new();
        propagate_lexical_capture_types(
            &states,
            &[full_internal_capture(
                consumer_owner,
                target,
                capture,
                provider,
            )],
            &mut BTreeMap::new(),
            &mut BTreeMap::new(),
            true,
            &mut unifier,
        )
        .unwrap();

        let Type::Object(provider) = unifier.resolve(&Type::Var(provider)) else {
            panic!("provider must retain its object shape");
        };
        assert_eq!(provider.fields.get("known"), Some(&Type::Text));
        assert_eq!(
            provider.fields.get("generic"),
            Some(&Type::List(Type::shared(Type::Number)))
        );
    }

    #[test]
    fn capture_backflow_reaches_holes_inside_tagged_payloads() {
        let mut unifier = TypeUnifier::default();
        let payload = unifier.fresh();
        let provider =
            Type::VariantSet(boon_checked::SharedVariantSet::new(vec![Variant::tagged(
                "Found".to_owned(),
                ObjectShape {
                    fields: [(
                        "payload".to_owned(),
                        Type::List(Type::shared(Type::Var(payload))),
                    )]
                    .into(),
                    field_order: vec!["payload".to_owned()],
                    open: false,
                },
            )]));
        let requirement =
            Type::VariantSet(boon_checked::SharedVariantSet::new(vec![Variant::tagged(
                "Found".to_owned(),
                ObjectShape {
                    fields: [("payload".to_owned(), Type::List(Type::shared(Type::Number)))].into(),
                    field_order: vec!["payload".to_owned()],
                    open: false,
                },
            )]));

        bind_provider_inference_holes(&mut unifier, &provider, &requirement);
        assert_eq!(unifier.resolve(&Type::Var(payload)), Type::Number);
    }

    #[test]
    fn closed_union_call_actual_specializes_formal_holes_without_provider_backflow() {
        let mut unifier = TypeUnifier::default();
        let actual = unifier.fresh();
        let formal_kind = unifier.fresh();
        let formal_label = unifier.fresh();
        let first = Type::object(ObjectShape::from_ordered_fields(
            [
                (
                    "kind".to_owned(),
                    Type::VariantSet(vec![Variant::Tag("First".to_owned())].into()),
                ),
                ("label".to_owned(), Type::Text),
            ],
            false,
        ));
        let second = Type::object(ObjectShape::from_ordered_fields(
            [
                (
                    "kind".to_owned(),
                    Type::VariantSet(vec![Variant::Tag("Second".to_owned())].into()),
                ),
                ("label".to_owned(), Type::Text),
            ],
            false,
        ));
        let actual_type = boon_checked::canonical_union_type(vec![first, second]);
        unifier.bind_var(actual, actual_type.clone());
        let formal = Type::object(ObjectShape::from_ordered_fields(
            [
                ("kind".to_owned(), Type::Var(formal_kind)),
                ("label".to_owned(), Type::Var(formal_label)),
            ],
            true,
        ));

        unifier.bind_call_input(actual, formal);

        assert_eq!(unifier.resolve(&Type::Var(actual)), actual_type);
        assert_eq!(unifier.resolve(&Type::Var(formal_label)), Type::Text);
        assert_eq!(
            unifier.resolve(&Type::Var(formal_kind)),
            crate::type_for_nested_path(&actual_type, &["kind".to_owned()]).unwrap()
        );

        let concrete_actual = unifier.fresh();
        unifier.bind_var(concrete_actual, actual_type.clone());
        unifier.bind_call_input(
            concrete_actual,
            Type::object(ObjectShape::from_ordered_fields(
                [("label".to_owned(), Type::Text)],
                true,
            )),
        );
        assert_eq!(
            unifier.resolve(&Type::Var(concrete_actual)),
            actual_type,
            "a concrete open formal must not structurally rewrite a closed union provider",
        );

        let Type::Union(records) = &actual_type else {
            panic!("fixture must retain two record alternatives");
        };
        let list_actual_type = boon_checked::canonical_union_type(
            records
                .iter()
                .cloned()
                .map(|record| Type::List(Type::shared(record)))
                .collect(),
        );
        let list_actual = unifier.fresh();
        let formal_item = unifier.fresh();
        unifier.bind_var(list_actual, list_actual_type.clone());
        unifier.bind_call_input(
            list_actual,
            Type::List(Type::shared(Type::Var(formal_item))),
        );
        assert_eq!(unifier.resolve(&Type::Var(list_actual)), list_actual_type);
        assert_eq!(unifier.resolve(&Type::Var(formal_item)), actual_type);

        let object_actual = unifier.fresh();
        let formal_signal_id = unifier.fresh();
        let object_actual_type = Type::object(ObjectShape::from_ordered_fields(
            [
                ("signal_id".to_owned(), Type::Text),
                ("unrelated".to_owned(), Type::Number),
            ],
            false,
        ));
        unifier.bind_var(object_actual, object_actual_type.clone());
        unifier.bind_call_input(
            object_actual,
            Type::object(ObjectShape::from_ordered_fields(
                [("signal_id".to_owned(), Type::Var(formal_signal_id))],
                true,
            )),
        );
        assert_eq!(
            unifier.resolve(&Type::Var(object_actual)),
            object_actual_type
        );
        assert_eq!(unifier.resolve(&Type::Var(formal_signal_id)), Type::Text);

        let absent_actual = unifier.fresh();
        let missing = unifier.fresh();
        let present = unifier.fresh();
        unifier.bind_var(
            absent_actual,
            Type::object(ObjectShape::from_ordered_fields(
                [
                    ("missing".to_owned(), Type::Absent),
                    ("present".to_owned(), Type::Text),
                ],
                false,
            )),
        );
        unifier.bind_call_input(
            absent_actual,
            Type::object(ObjectShape::from_ordered_fields(
                [
                    ("missing".to_owned(), Type::Var(missing)),
                    ("present".to_owned(), Type::Var(present)),
                ],
                true,
            )),
        );
        assert_eq!(unifier.resolve(&Type::Var(missing)), Type::Var(missing));
        assert_eq!(unifier.resolve(&Type::Var(present)), Type::Text);
    }

    #[test]
    fn union_projection_preserves_the_provider_and_projects_all_members() {
        let mut unifier = TypeUnifier::default();
        let root = unifier.fresh();
        let actual_type = boon_checked::canonical_union_type(vec![
            Type::object(ObjectShape::from_ordered_fields(
                [("value".to_owned(), Type::Number)],
                false,
            )),
            Type::object(ObjectShape::from_ordered_fields(
                [("value".to_owned(), Type::Text)],
                false,
            )),
        ]);
        unifier.bind_var(root, actual_type.clone());

        let projected = bind_projection(&mut unifier, root, &["value".to_owned()]);

        assert_eq!(unifier.resolve(&Type::Var(root)), actual_type);
        assert_eq!(
            unifier.resolve(&Type::Var(projected)),
            boon_checked::canonical_union_type(vec![Type::Number, Type::Text])
        );
    }

    #[test]
    fn partially_open_union_call_actual_projects_all_alternatives_without_provider_backflow() {
        let mut unifier = TypeUnifier::default();
        let actual = unifier.fresh();
        let actual_detail = unifier.fresh();
        let formal_label = unifier.fresh();
        let formal_detail = unifier.fresh();
        let actual_type = boon_checked::canonical_union_type(vec![
            Type::object(ObjectShape::from_ordered_fields(
                [
                    ("label".to_owned(), Type::Text),
                    ("detail".to_owned(), Type::Var(actual_detail)),
                ],
                false,
            )),
            Type::object(ObjectShape::from_ordered_fields(
                [
                    ("label".to_owned(), Type::Text),
                    ("detail".to_owned(), Type::Number),
                ],
                false,
            )),
        ]);
        unifier.bind_var(actual, actual_type);
        let formal = Type::object(ObjectShape::from_ordered_fields(
            [
                ("label".to_owned(), Type::Var(formal_label)),
                ("detail".to_owned(), Type::Var(formal_detail)),
            ],
            true,
        ));

        unifier.bind_call_input(actual, formal);

        assert_eq!(
            unifier.resolve(&Type::Var(actual_detail)),
            Type::Var(actual_detail)
        );
        assert_eq!(unifier.resolve(&Type::Var(formal_label)), Type::Text);
        assert_eq!(
            unifier.resolve(&Type::Var(formal_detail)),
            boon_checked::canonical_union_type(vec![Type::Var(actual_detail), Type::Number])
        );
        let Type::Union(actual) = unifier.resolve(&Type::Var(actual)) else {
            panic!("partially open provider must remain a union");
        };
        assert!(actual.iter().all(|member| {
            matches!(
                member,
                Type::Object(shape)
                    if shape.fields.get("label") == Some(&Type::Text)
            )
        }));
    }

    #[test]
    fn open_call_actual_remains_an_equation_with_a_concrete_formal() {
        let mut unifier = TypeUnifier::default();
        let actual = unifier.fresh();
        unifier.bind_call_input(actual, Type::Number);
        assert_eq!(unifier.resolve(&Type::Var(actual)), Type::Number);

        let open_actual = unifier.fresh();
        unifier.bind_var(
            open_actual,
            Type::object(ObjectShape::from_ordered_fields(
                [("a".to_owned(), Type::Number)],
                true,
            )),
        );
        unifier.bind_call_input(
            open_actual,
            Type::object(ObjectShape::from_ordered_fields(
                [("b".to_owned(), Type::Text)],
                true,
            )),
        );
        let Type::Object(open_actual) = unifier.resolve(&Type::Var(open_actual)) else {
            panic!("open call actual must remain an object equation");
        };
        assert_eq!(open_actual.fields.get("a"), Some(&Type::Number));
        assert_eq!(open_actual.fields.get("b"), Some(&Type::Text));
        assert!(open_actual.open);
    }

    #[test]
    fn call_placeholders_do_not_replace_generic_structural_fields() {
        for placeholder in [
            Type::Unknown,
            Type::UnresolvedShape {
                reason: "provisional call input".to_owned(),
            },
        ] {
            let mut direct = TypeUnifier::default();
            let generic = direct.fresh();
            direct.bind_call_input(generic, placeholder.clone());
            assert_eq!(
                direct.resolve(&Type::Var(generic)),
                Type::Var(generic),
                "a direct call placeholder must not close its generic input",
            );

            for reverse in [false, true] {
                let mut unifier = TypeUnifier::default();
                let actual = unifier.fresh();
                let generic = unifier.fresh();
                let actual_shape = Type::object(ObjectShape::from_ordered_fields(
                    [("value".to_owned(), placeholder.clone())],
                    true,
                ));
                let formal_shape = Type::object(ObjectShape::from_ordered_fields(
                    [("value".to_owned(), Type::Var(generic))],
                    true,
                ));
                unifier.bind_var(
                    actual,
                    if reverse {
                        formal_shape.clone()
                    } else {
                        actual_shape.clone()
                    },
                );
                unifier.bind_call_input(actual, if reverse { actual_shape } else { formal_shape });

                assert_eq!(
                    unifier.resolve(&Type::Var(generic)),
                    Type::Var(generic),
                    "placeholder {placeholder:?}, reverse={reverse}",
                );
                let Type::Object(resolved) = unifier.resolve(&Type::Var(actual)) else {
                    panic!("call input must remain an object")
                };
                assert_eq!(resolved.fields.get("value"), Some(&Type::Var(generic)));

                unifier.bind_var(generic, Type::Text);
                let Type::Object(resolved) = unifier.resolve(&Type::Var(actual)) else {
                    panic!("late exact call input must remain an object")
                };
                assert_eq!(resolved.fields.get("value"), Some(&Type::Text));
            }
        }
    }

    #[test]
    fn closed_call_provider_replaces_only_an_open_shared_formal_scaffold() {
        fn rich_item(id: Type) -> Type {
            Type::object(ObjectShape::from_ordered_fields(
                [("id".to_owned(), id), ("payload".to_owned(), Type::Text)],
                false,
            ))
        }

        let mut unifier = TypeUnifier::default();
        let actual = unifier.fresh();
        let formal_item = unifier.fresh();
        let provider = Type::List(Type::shared(rich_item(Type::Text)));
        unifier.bind_var(actual, provider.clone());
        unifier.bind_var(
            formal_item,
            Type::object(ObjectShape::from_ordered_fields(
                [("id".to_owned(), Type::Unknown)],
                true,
            )),
        );
        unifier.bind_call_input(actual, Type::List(Type::shared(Type::Var(formal_item))));
        assert_eq!(unifier.resolve(&Type::Var(actual)), provider);
        assert_eq!(
            unifier.resolve(&Type::Var(formal_item)),
            rich_item(Type::Text),
            "a closed parent provider must replace the FreshOut consumer scaffold",
        );

        let solve_conflict = |reverse: bool| {
            let mut unifier = TypeUnifier::default();
            let formal = unifier.fresh();
            unifier.bind_var(
                formal,
                Type::object(ObjectShape::from_ordered_fields(
                    [("id".to_owned(), Type::Unknown)],
                    true,
                )),
            );
            let mut providers = [rich_item(Type::Text), rich_item(Type::Number)];
            if reverse {
                providers.reverse();
            }
            for provider in providers {
                bind_call_formal_from_closed_alternatives(
                    &mut unifier,
                    &Type::Var(formal),
                    &[provider],
                );
            }
            unifier.resolve(&Type::Var(formal))
        };
        assert_eq!(
            solve_conflict(false),
            solve_conflict(true),
            "two closed providers must retain order-independent equality semantics",
        );
    }

    #[test]
    fn authoritative_call_result_replaces_only_the_provisional_consumer_scaffold() {
        let mut unifier = TypeUnifier::default();
        let occurrence = unifier.fresh();
        let provider_key = unifier.fresh();
        unifier.bind_var(
            occurrence,
            Type::object(ObjectShape::from_ordered_fields(
                [
                    ("key".to_owned(), Type::Text),
                    ("consumer_only".to_owned(), Type::Number),
                ],
                true,
            )),
        );
        unifier.publish_authoritative_provider(
            occurrence,
            Type::object(ObjectShape::from_ordered_fields(
                [
                    ("key".to_owned(), Type::Var(provider_key)),
                    ("label".to_owned(), Type::Text),
                ],
                false,
            )),
        );

        assert_eq!(unifier.resolve(&Type::Var(provider_key)), Type::Text);
        let Type::Object(result) = unifier.resolve(&Type::Var(occurrence)) else {
            panic!("authoritative occurrence must remain an object");
        };
        assert!(!result.open, "the provider's closedness is authoritative");
        assert_eq!(result.fields.get("key"), Some(&Type::Text));
        assert_eq!(result.fields.get("label"), Some(&Type::Text));
        assert!(
            !result.fields.contains_key("consumer_only"),
            "a provisional consumer field must not widen the provider result",
        );

        let list_occurrence = unifier.fresh();
        let provisional_item = unifier.fresh();
        unifier.bind_var(
            list_occurrence,
            Type::List(Type::shared(Type::Var(provisional_item))),
        );
        unifier
            .publish_authoritative_provider(list_occurrence, Type::List(Type::shared(Type::Text)));
        assert_eq!(
            unifier.resolve(&Type::Var(provisional_item)),
            Type::Text,
            "provider leaves must close holes in the provisional consumer scaffold",
        );
        assert_eq!(
            unifier.resolve(&Type::Var(list_occurrence)),
            Type::List(Type::shared(Type::Text)),
        );

        let live_occurrence = unifier.fresh();
        let provider_value = unifier.fresh();
        let consumer_value = unifier.fresh();
        let live_provider = Type::object(ObjectShape::from_ordered_fields(
            [
                ("value".to_owned(), Type::Var(provider_value)),
                ("label".to_owned(), Type::Text),
            ],
            false,
        ));
        unifier.publish_authoritative_provider(live_occurrence, live_provider.clone());
        unifier.bind_call_input(
            live_occurrence,
            Type::object(ObjectShape::from_ordered_fields(
                [("value".to_owned(), Type::Var(consumer_value))],
                true,
            )),
        );
        let Type::Object(replayed) = unifier.resolve(&Type::Var(live_occurrence)) else {
            panic!("replayed occurrence must remain an object");
        };
        assert!(
            replayed.open,
            "the ordinary input equation demonstrates the provisional reopen seam",
        );
        unifier.publish_authoritative_provider(live_occurrence, live_provider);
        unifier.bind_var(consumer_value, Type::Number);
        let Type::Object(republished) = unifier.resolve(&Type::Var(live_occurrence)) else {
            panic!("republished occurrence must remain an object");
        };
        assert!(
            !republished.open,
            "provider authority must be restored after every input replay",
        );
        assert_eq!(republished.fields.get("value"), Some(&Type::Number));
    }

    #[test]
    fn capture_backflow_ignores_placeholder_requirements_but_links_live_variables() {
        for requirement in [
            Type::Unknown,
            Type::UnresolvedShape {
                reason: "test placeholder".to_owned(),
            },
            Type::object(ObjectShape {
                fields: BTreeMap::new(),
                field_order: Vec::new(),
                open: true,
            }),
        ] {
            let mut unifier = TypeUnifier::default();
            let provider = unifier.fresh();
            bind_provider_inference_holes(&mut unifier, &Type::Var(provider), &requirement);
            assert_eq!(unifier.resolve(&Type::Var(provider)), Type::Var(provider));
        }

        let mut unifier = TypeUnifier::default();
        let provider = unifier.fresh();
        let requirement = unifier.fresh();
        bind_provider_inference_holes(&mut unifier, &Type::Var(provider), &Type::Var(requirement));
        unifier.bind_var(requirement, Type::Number);
        assert_eq!(unifier.resolve(&Type::Var(provider)), Type::Number);
    }

    #[test]
    fn identity_interface_preserves_one_alpha_normalized_type_variable() {
        let unit = link("FUNCTION identity(input) {\n    input\n}\n");
        let owner = owner_named(&unit, "identity");
        let seed = seed(&unit, &owner);
        let summary = resolve_owner_constraint_seed(&seed, []).unwrap();
        let results = solve(&[seed], &[summary]);
        assert_eq!(results[0].work.solve_rounds, 0);
        let interface = results[0].owner(&owner).unwrap();
        assert_eq!(interface.parameters.len(), 1);
        assert_eq!(interface.parameters[0].flow_type.ty, Type::Var(TypeVar(0)));
        assert_eq!(interface.result.ty, Type::Var(TypeVar(0)));
        assert_eq!(interface.type_variables.as_ref(), [TypeVar(0)]);
    }

    #[test]
    fn external_parameter_requirement_is_keyed_by_owner_and_ordinal() {
        let source = "FUNCTION identity(input) {\n    input\n}\n";
        let unit = link(source);
        let owner = owner_named(&unit, "identity");
        let seed = seed(&unit, &owner);
        let summary = resolve_owner_constraint_seed(&seed, []).unwrap();
        let mut external =
            boon_checked::ExternalTypeEnvironment::empty(boon_checked::ProgramRole::Client);
        external
            .local_function_requirements
            .entry("identity".to_owned())
            .or_default()
            .insert("input".to_owned(), Type::Number);
        let project =
            ProjectSyntaxSnapshot::from_unit_snapshots("app/RUN.bn", vec![Arc::new(unit)]).unwrap();
        let provider = crate::project_owner_abi_environment(&project, &external).unwrap();
        let results = solve_with_provider(&[seed], &[summary], &provider);
        let interface = results[0].owner(&owner).unwrap();
        assert_eq!(interface.parameters[0].flow_type.ty, Type::Number);
        assert_eq!(interface.result.ty, Type::Number);
        assert!(interface.type_variables.is_empty());

        let checked = checked_callable_interface_with_external(source, "identity", &external);
        assert_eq!(
            interface.parameters[0].flow_type,
            checked.parameters[0].flow_type
        );
        assert_eq!(interface.result, checked.result);
    }

    #[test]
    fn transparent_block_alias_preserves_parameter_result_equation() {
        let unit = link(concat!(
            "FUNCTION identity(input) {\n",
            "    BLOCK {\n",
            "        result: input\n",
            "        result\n",
            "    }\n",
            "}\n",
        ));
        let owner = owner_named(&unit, "identity");
        let seed = seed(&unit, &owner);
        let summary = resolve_owner_constraint_seed(&seed, []).unwrap();
        let results = solve(&[seed], &[summary]);
        let interface = results[0].owner(&owner).unwrap();

        assert_eq!(interface.parameters[0].flow_type.ty, Type::Var(TypeVar(0)));
        assert_eq!(interface.result.ty, Type::Var(TypeVar(0)));
        assert_eq!(interface.type_variables.as_ref(), [TypeVar(0)]);
    }

    #[test]
    fn forward_record_field_shadows_same_named_project_value_during_interface_inference() {
        let unit = link(concat!(
            "item: TEXT { outer }\n",
            "record: [copy: item, item: 1]\n",
        ));
        let owner = owner_named(&unit, "record");
        let seed = seed(&unit, &owner);
        assert!(seed.references.iter().all(|reference| {
            reference.kind != OwnerReferenceKind::Value || reference.parts.as_ref() != ["item"]
        }));
        let summary = resolve_owner_constraint_seed(&seed, []).unwrap();
        let results = solve(&[seed], &[summary]);
        let interface = results[0].owner(&owner).unwrap();
        let Type::Object(shape) = &interface.result.ty else {
            panic!("record interface must publish an object: {interface:#?}");
        };
        assert_eq!(shape.fields.get("copy"), Some(&Type::Number));
        assert_eq!(shape.fields.get("item"), Some(&Type::Number));
    }

    #[test]
    fn record_field_whole_scope_shadows_same_named_parameter() {
        let unit = link(concat!(
            "FUNCTION make(item) {\n",
            "    [copy: item, item: 1]\n",
            "}\n",
        ));
        let owner = owner_named(&unit, "make");
        let seed = seed(&unit, &owner);
        let summary = resolve_owner_constraint_seed(&seed, []).unwrap();
        let results = solve(&[seed], &[summary]);
        let interface = results[0].owner(&owner).unwrap();
        let Type::Object(shape) = &interface.result.ty else {
            panic!("record interface must publish an object: {interface:#?}");
        };
        assert_eq!(shape.fields.get("copy"), Some(&Type::Number));
        assert_eq!(shape.fields.get("item"), Some(&Type::Number));
        assert_ne!(interface.parameters[0].flow_type.ty, Type::Number);
    }

    #[test]
    fn record_self_initializer_reuses_the_outer_parameter_type() {
        let unit = link(concat!(
            "FUNCTION dimensions(width) {\n",
            "    [width: width]\n",
            "}\n",
        ));
        let owner = owner_named(&unit, "dimensions");
        let seed = seed(&unit, &owner);
        let summary = resolve_owner_constraint_seed(&seed, []).unwrap();
        let results = solve(&[seed], &[summary]);
        let interface = results[0].owner(&owner).unwrap();
        let Type::Object(shape) = &interface.result.ty else {
            panic!("dimensions must return an object: {interface:#?}");
        };
        assert_eq!(
            shape.fields.get("width"),
            Some(&interface.parameters[0].flow_type.ty),
        );
        assert!(matches!(interface.parameters[0].flow_type.ty, Type::Var(_)));
    }

    #[test]
    fn result_transfer_excludes_unrelated_child_body_changes() {
        fn identity_interface(source: &str) -> OwnerPublicInterface {
            let unit = link(source);
            let owner = owner_named(&unit, "identity");
            let seed = seed(&unit, &owner);
            let summary = resolve_owner_constraint_seed(&seed, []).unwrap();
            solve(&[seed], &[summary])
                .into_iter()
                .find_map(|result| result.owner(&owner).cloned())
                .unwrap()
        }

        let number =
            identity_interface("FUNCTION identity(input) {\n    unused: 1\n    input\n}\n");
        let text = identity_interface(
            "FUNCTION identity(input) {\n    unused: TEXT { ignored }\n    input\n}\n",
        );
        assert_eq!(number, text);
    }

    #[test]
    fn local_numeric_constraint_closes_parameter_and_result() {
        let unit = link("FUNCTION increment(input) {\n    input + 1\n}\n");
        let owner = owner_named(&unit, "increment");
        let seed = seed(&unit, &owner);
        let summary = resolve_owner_constraint_seed(&seed, []).unwrap();
        let results = solve(&[seed], &[summary]);
        let interface = results[0].owner(&owner).unwrap();
        assert_eq!(interface.parameters[0].flow_type.ty, Type::Number);
        assert_eq!(interface.result.ty, Type::Number);
        assert!(interface.type_variables.is_empty());
    }

    #[test]
    fn text_formattable_abi_placeholders_preserve_generic_projected_fields() {
        let source = concat!(
            "FUNCTION lane_identity(row) {\n",
            "    row.file\n",
            "        |> Text/concat(with: row.family, separator: row.id)\n",
            "}\n",
        );
        assert_checked_interface_parity(source, "lane_identity");
        let unit = link(source);
        let owner = owner_named(&unit, "lane_identity");
        let seed = seed(&unit, &owner);
        let summary = resolve_owner_constraint_seed(&seed, []).unwrap();
        let results = solve(&[seed], &[summary]);
        let interface = results[0].owner(&owner).unwrap();
        let Type::Object(parameter) = &interface.parameters[0].flow_type.ty else {
            panic!("lane identity parameter must retain its projected object scheme")
        };
        assert!(parameter.open);
        let fields = ["file", "family", "id"]
            .into_iter()
            .map(|name| {
                let Some(Type::Var(variable)) = parameter.fields.get(name) else {
                    panic!("lane identity {name} must remain generic: {interface:#?}")
                };
                *variable
            })
            .collect::<BTreeSet<_>>();
        assert_eq!(
            fields.len(),
            3,
            "formattable fields need independent alphas"
        );
        assert_eq!(interface.result.ty, Type::Text);
    }

    #[test]
    fn match_patterns_do_not_close_the_public_selector_contract() {
        let unit = link(concat!(
            "FUNCTION label(tone) {\n",
            "    tone |> WHEN {\n",
            "        Dark => TEXT { dark }\n",
            "        __ => TEXT { other }\n",
            "    }\n",
            "}\n",
        ));
        let owner = owner_named(&unit, "label");
        let seed = seed(&unit, &owner);
        let summary = resolve_owner_constraint_seed(&seed, []).unwrap();
        let results = solve(&[seed], &[summary]);
        let interface = results[0].owner(&owner).unwrap();

        assert_eq!(interface.parameters[0].flow_type.ty, Type::Var(TypeVar(0)));
        assert_eq!(interface.type_variables.as_ref(), [TypeVar(0)]);
        assert_eq!(interface.result.ty, Type::Text);
    }

    #[test]
    fn match_arm_field_reads_remain_branch_local() {
        let unit = link(concat!(
            "FUNCTION label(value) {\n",
            "    value |> WHEN {\n",
            "        StringValue => value.text\n",
            "        RealValue => value.value |> Number/to_text(radix: 10)\n",
            "        __ => TEXT { ? }\n",
            "    }\n",
            "}\n",
        ));
        let owner = owner_named(&unit, "label");
        let seed = seed(&unit, &owner);
        let summary = resolve_owner_constraint_seed(&seed, []).unwrap();
        let results = solve(&[seed], &[summary]);
        let interface = results[0].owner(&owner).unwrap();

        assert!(matches!(interface.parameters[0].flow_type.ty, Type::Var(_)));
    }

    #[test]
    fn pattern_bindings_shadow_same_named_parameters_without_type_leakage() {
        let unit = link(concat!(
            "FUNCTION parsed_or_length(value) {\n",
            "    value |> Text/to_number() |> WHEN {\n",
            "        Parsed[value] => value\n",
            "        InvalidNumber[reason, position] => value |> Text/length()\n",
            "    }\n",
            "}\n",
        ));
        let owner = owner_named(&unit, "parsed_or_length");
        let seed = seed(&unit, &owner);
        let summary = resolve_owner_constraint_seed(&seed, []).unwrap();
        let results = solve(&[seed], &[summary]);
        let interface = results[0].owner(&owner).unwrap();

        assert_eq!(interface.parameters[0].flow_type.ty, Type::Text);
        assert_eq!(interface.result.ty, Type::Number);
    }

    #[test]
    fn branch_result_widening_does_not_write_fields_back_into_a_parameter() {
        let unit = link(concat!(
            "FUNCTION widen(row) {\n",
            "    row.item_kind |> WHEN {\n",
            "        VariableRow => [item_kind: row.item_kind, base: row.base, generated: 1]\n",
            "        __ => row\n",
            "    }\n",
            "}\n",
        ));
        let owner = owner_named(&unit, "widen");
        let seed = seed(&unit, &owner);
        let summary = resolve_owner_constraint_seed(&seed, []).unwrap();
        let results = solve(&[seed], &[summary]);
        let interface = results[0].owner(&owner).unwrap();
        let Type::Object(parameter) = &interface.parameters[0].flow_type.ty else {
            panic!("row parameter must retain its structural read requirements");
        };

        assert!(parameter.fields.contains_key("item_kind"));
        assert!(parameter.fields.contains_key("base"));
        assert!(!parameter.fields.contains_key("generated"));
    }

    #[test]
    fn hold_owner_interface_retains_its_initialized_value_domain() {
        let unit = link(concat!(
            "state:\n",
            "    0 |> HOLD state {\n",
            "        LATEST {\n",
            "            1 |> THEN { 2 }\n",
            "        }\n",
            "    }\n",
        ));
        let owners = unit.stable_check_owner_keys().collect::<Vec<_>>();
        let owner = owner_named(&unit, "state");
        let syntax = project_owner_syntax_input(unit.owner_view_for_key(&owner).unwrap()).unwrap();
        let parent_seed = project_owner_constraint_seed(&syntax).unwrap();
        let seeds = owners
            .iter()
            .filter(|owner| matches!(owner, StableCheckOwnerKey::Item(_)))
            .map(|owner| seed(&unit, owner))
            .collect::<Vec<_>>();
        let summaries = seeds
            .iter()
            .map(|seed| resolve_owner_constraint_seed(seed, []).unwrap())
            .collect::<Vec<_>>();
        let results = solve(&seeds, &summaries);
        let interface = results
            .iter()
            .find_map(|result| result.owner(&owner))
            .unwrap();

        assert_eq!(
            interface.result.ty,
            Type::Number,
            "owners: {owners:#?}\nsyntax: {syntax:#?}\nseed: {parent_seed:#?}"
        );
    }

    #[test]
    fn hold_owner_interface_widens_over_closed_update_values() {
        let unit = link(concat!(
            "state:\n",
            "    NotStarted |> HOLD state {\n",
            "        True |> THEN { WaveformOpened[timescale_unit: TEXT { ns }] }\n",
            "    }\n",
        ));
        let owner = owner_named(&unit, "state");
        let seeds = unit
            .stable_check_owner_keys()
            .map(|owner| seed(&unit, &owner))
            .collect::<Vec<_>>();
        let summaries = seeds
            .iter()
            .map(|seed| resolve_owner_constraint_seed(seed, []).unwrap())
            .collect::<Vec<_>>();
        let results = solve(&seeds, &summaries);
        let interface = results
            .iter()
            .find_map(|result| result.owner(&owner))
            .expect("state public interface");
        let Type::VariantSet(variants) = &interface.result.ty else {
            panic!("HOLD result must retain initial and update tags: {interface:#?}");
        };
        assert!(
            variants
                .iter()
                .any(|variant| { matches!(variant, Variant::Tag(tag) if tag == "NotStarted") })
        );
        assert!(
            variants.iter().any(|variant| {
                matches!(
                    variant,
                    Variant::Tagged { tag, fields }
                        if tag == "WaveformOpened"
                            && fields.fields.get("timescale_unit") == Some(&Type::Text)
                )
            }),
            "state interface: {interface:#?}",
        );
        assert!(boon_checked::type_is_recursively_closed(
            &interface.result.ty
        ));
    }

    #[test]
    fn hold_owner_interface_drops_recursive_self_alternatives_from_its_value_domain() {
        let unit = link(concat!(
            "state:\n",
            "    Normal |> HOLD state {\n",
            "        LATEST {\n",
            "            True |> THEN { Tall }\n",
            "            False |> THEN { Compact }\n",
            "            True |> THEN {\n",
            "                state |> WHEN {\n",
            "                    Normal => Tall\n",
            "                    Tall => Compact\n",
            "                    __ => state\n",
            "                }\n",
            "            }\n",
            "        }\n",
            "    }\n",
        ));
        let owner = owner_named(&unit, "state");
        let seeds = unit
            .stable_check_owner_keys()
            .map(|owner| seed(&unit, &owner))
            .collect::<Vec<_>>();
        let summaries = seeds
            .iter()
            .map(|seed| resolve_owner_constraint_seed(seed, []).unwrap())
            .collect::<Vec<_>>();
        let results = solve(&seeds, &summaries);
        let interface = results
            .iter()
            .find_map(|result| result.owner(&owner))
            .expect("state public interface");

        assert_eq!(
            interface.result.ty,
            Type::VariantSet(
                vec![
                    Variant::Tag("Compact".to_owned()),
                    Variant::Tag("Normal".to_owned()),
                    Variant::Tag("Tall".to_owned()),
                ]
                .into(),
            ),
        );
        assert!(boon_checked::type_is_recursively_closed(
            &interface.result.ty
        ));
    }

    #[test]
    fn hold_component_quiesces_when_call_transfer_refreshes_a_generic_list_alpha() {
        let unit = link(concat!(
            "FUNCTION generic_rows(rows, selector) {\n",
            "    selector |> WHEN {\n",
            "        True => rows\n",
            "        __ => BLOCK {\n",
            "            fallback: LIST {}\n",
            "            fallback\n",
            "        }\n",
            "    }\n",
            "}\n",
            "state:\n",
            "    LIST {} |> HOLD state {\n",
            "        True |> THEN { generic_rows(rows: LIST {}, selector: True) }\n",
            "    }\n",
        ));
        let generic_rows = owner_named(&unit, "generic_rows");
        let state = owner_named(&unit, "state");
        let seeds = unit
            .stable_check_owner_keys()
            .filter(|owner| matches!(owner, StableCheckOwnerKey::Item(_)))
            .map(|owner| seed(&unit, &owner))
            .collect::<Vec<_>>();
        let summaries = seeds
            .iter()
            .map(|seed| {
                let resolutions = seed.references.iter().filter_map(|reference| {
                    (reference.kind == OwnerReferenceKind::Callable
                        && reference.parts.as_ref() == ["generic_rows"])
                    .then(|| ResolvedOwnerSymbolReference {
                        reference: reference.clone(),
                        owner: generic_rows.clone(),
                        projection: Box::new([]),
                        parameters: seeds
                            .iter()
                            .find(|seed| seed.owner == generic_rows)
                            .and_then(|seed| {
                                seed.declarations
                                    .iter()
                                    .find(|declaration| declaration.public)
                            })
                            .expect("generic_rows declaration")
                            .parameters
                            .clone(),
                    })
                });
                resolve_owner_constraint_seed(seed, resolutions).unwrap()
            })
            .collect::<Vec<_>>();

        let components = solve_components(&seeds, &summaries);
        let transfer_iterations = components
            .values()
            .map(|component| component.transfer_iterations)
            .collect::<Vec<_>>();
        let component = components
            .values()
            .find(|component| component.evaluation.result.owner(&state).is_some())
            .expect("state interface component");
        assert!(
            transfer_iterations.contains(&2),
            "one fixture component must publish and then quiesce a freshly instantiated call alpha: {transfer_iterations:?}"
        );
        let interface = component.evaluation.result.owner(&state).unwrap();
        let Type::List(item) = &interface.result.ty else {
            panic!("generic HOLD must publish a list: {interface:#?}");
        };
        assert!(
            matches!(item.as_ref(), Type::Var(_)),
            "the generic list item alpha must remain live: {interface:#?}"
        );
    }

    #[test]
    fn flush_control_is_frozen_separately_and_unionized_at_callable_boundary() {
        let source = "FUNCTION stop() {\n    FLUSH { Error }\n}\n";
        assert_checked_interface_parity(source, "stop");
        let unit = link(source);
        let owner = owner_named(&unit, "stop");
        let seed = seed(&unit, &owner);
        let summary = resolve_owner_constraint_seed(&seed, []).unwrap();
        let results = solve(&[seed], &[summary]);
        let interface = results[0].owner(&owner).unwrap();
        let error = Type::VariantSet(vec![Variant::Tag("Error".to_owned())].into());
        assert_eq!(interface.result_flush_type, Some(error.clone()));
        assert_eq!(
            interface.result,
            FlowType {
                mode: FlowMode::Continuous,
                ty: error,
            }
        );
    }

    #[test]
    fn dependency_first_value_interface_flows_without_child_body_copy() {
        let unit = link("left: 1\nright: left\n");
        let left = owner_named(&unit, "left");
        let right = owner_named(&unit, "right");
        let left_seed = seed(&unit, &left);
        let right_seed = seed(&unit, &right);
        let reference = right_seed.references[0].clone();
        let left_summary = resolve_owner_constraint_seed(&left_seed, []).unwrap();
        let right_summary = resolve_owner_constraint_seed(
            &right_seed,
            [ResolvedOwnerSymbolReference {
                reference,
                owner: left.clone(),
                projection: Box::new([]),
                parameters: Box::new([]),
            }],
        )
        .unwrap();
        let results = solve(&[left_seed, right_seed], &[left_summary, right_summary]);
        let interfaces = results
            .iter()
            .flat_map(|result| result.owners.iter())
            .map(|interface| (interface.owner.clone(), interface.result.ty.clone()))
            .collect::<BTreeMap<_, _>>();
        assert_eq!(interfaces[&left], Type::Number);
        assert_eq!(interfaces[&right], Type::Number);
    }

    #[test]
    fn dynamic_out_producer_closes_nested_user_call_before_sibling_consumption() {
        let unit = link(concat!(
            "FUNCTION copy(row) {\n",
            "    [\n",
            "        kind: row.kind\n",
            "        file: row.file\n",
            "    ]\n",
            "}\n",
            "rows:\n",
            "    LIST {\n",
            "        [kind: First, file: TEXT { a }]\n",
            "        [kind: Second, file: TEXT { b }]\n",
            "    }\n",
            "    |> List/map(item, new: copy(row: item))\n",
            "filtered:\n",
            "    rows |> List/filter(item, if: item.file == TEXT { a })\n",
        ));
        let copy = owner_named(&unit, "copy");
        let rows = owner_named(&unit, "rows");
        let filtered = owner_named(&unit, "filtered");
        let seeds = unit
            .stable_check_owner_keys()
            .filter(|owner| matches!(owner, StableCheckOwnerKey::Item(_)))
            .map(|owner| seed(&unit, &owner))
            .collect::<Vec<_>>();
        let parameters = seeds
            .iter()
            .find(|seed| seed.owner == copy)
            .expect("copy owner seed")
            .declarations
            .iter()
            .find(|declaration| declaration.public)
            .expect("copy public declaration")
            .parameters
            .clone();
        let summaries = seeds
            .iter()
            .map(|seed| {
                let resolutions = seed.references.iter().filter_map(|reference| {
                    if reference.kind == OwnerReferenceKind::Callable
                        && reference.parts.as_ref() == ["copy"]
                    {
                        Some(ResolvedOwnerSymbolReference {
                            reference: reference.clone(),
                            owner: copy.clone(),
                            projection: Box::new([]),
                            parameters: parameters.clone(),
                        })
                    } else if reference.kind == OwnerReferenceKind::Value
                        && reference.parts.as_ref() == ["rows"]
                    {
                        Some(ResolvedOwnerSymbolReference {
                            reference: reference.clone(),
                            owner: rows.clone(),
                            projection: Box::new([]),
                            parameters: Box::new([]),
                        })
                    } else {
                        None
                    }
                });
                resolve_owner_constraint_seed(seed, resolutions).unwrap()
            })
            .collect::<Vec<_>>();
        assert_eq!(
            summaries
                .iter()
                .flat_map(|summary| summary.resolved_references.iter())
                .filter(|resolved| {
                    resolved.owner == copy
                        && resolved.reference.kind == OwnerReferenceKind::Callable
                })
                .count(),
            1,
            "fixture must resolve exactly one call to copy",
        );
        assert_eq!(
            summaries
                .iter()
                .flat_map(|summary| summary.resolved_references.iter())
                .filter(|resolved| {
                    resolved.owner == rows && resolved.reference.kind == OwnerReferenceKind::Value
                })
                .count(),
            1,
            "fixture must resolve exactly one sibling read of rows",
        );

        let results = solve(&seeds, &summaries);
        let interfaces = results
            .iter()
            .flat_map(|result| result.owners.iter())
            .map(|interface| (interface.owner.clone(), interface))
            .collect::<BTreeMap<_, _>>();
        let copy_interface = interfaces[&copy];
        let Type::Object(copy_parameter) = &copy_interface.parameters[0].flow_type.ty else {
            panic!("copy parameter must be an open record scheme");
        };
        let Type::Object(copy_result) = &copy_interface.result.ty else {
            panic!("copy result must be a closed record scheme");
        };
        assert!(copy_parameter.open);
        assert!(!copy_result.open);
        let mut copied_alphas = BTreeSet::new();
        for field in ["kind", "file"] {
            let Some(Type::Var(parameter)) = copy_parameter.fields.get(field) else {
                panic!("copy parameter must expose a generic {field} field");
            };
            let Some(Type::Var(result)) = copy_result.fields.get(field) else {
                panic!("copy result must reuse a generic {field} field");
            };
            assert_eq!(parameter, result, "copy must retain the {field} alpha");
            copied_alphas.insert(*parameter);
        }
        assert_eq!(copied_alphas.len(), 2, "copy fields need distinct alphas");
        for owner in [&rows, &filtered] {
            assert!(
                boon_checked::type_is_recursively_closed(&interfaces[owner].result.ty),
                "{owner:?} must publish a closed mapped row interface: {:#?}",
                interfaces[owner].result,
            );
            let Type::List(item) = &interfaces[owner].result.ty else {
                panic!("{owner:?} must publish a list result");
            };
            assert_eq!(
                crate::type_for_nested_path(item, &["file".to_owned()]),
                Some(Type::Text),
                "{owner:?} must preserve the authored file field",
            );
            assert_eq!(
                crate::type_for_nested_path(item, &["kind".to_owned()]),
                Some(Type::VariantSet(
                    vec![
                        Variant::Tag("First".to_owned()),
                        Variant::Tag("Second".to_owned()),
                    ]
                    .into(),
                )),
                "{owner:?} must preserve the authored tag alternatives",
            );
        }
        assert_eq!(
            interfaces[&rows].result.ty, interfaces[&filtered].result.ty,
            "List/filter must preserve the exact mapped row type",
        );
    }

    #[test]
    fn filter_chain_waits_for_its_closed_sibling_list_before_field_projection() {
        let unit = link(concat!(
            "rows: LIST {\n",
            "    [\n",
            "        id: TEXT { one }\n",
            "        family: Family\n",
            "        enabled: TEXT { yes }\n",
            "        selected: True\n",
            "        hidden_when_group: Group\n",
            "    ]\n",
            "}\n",
            "visible:\n",
            "    rows\n",
            "    |> List/filter(item, if: item.family == Family)\n",
            "    |> List/filter(item, if: item.enabled == TEXT { yes })\n",
            "    |> List/filter(item, if: item.selected == True)\n",
            "    |> List/filter(item, if: item.hidden_when_group != OtherGroup)\n",
            "    |> List/filter(item, if: item.id != TEXT { removed })\n",
        ));
        let rows = owner_named(&unit, "rows");
        let visible = owner_named(&unit, "visible");
        let seeds = unit
            .stable_check_owner_keys()
            .filter(|owner| matches!(owner, StableCheckOwnerKey::Item(_)))
            .map(|owner| seed(&unit, &owner))
            .collect::<Vec<_>>();
        let owners = [("rows", &rows), ("visible", &visible)]
            .into_iter()
            .collect::<BTreeMap<_, _>>();
        let summaries = seeds
            .iter()
            .map(|seed| {
                let resolutions = seed.references.iter().filter_map(|reference| {
                    if reference.kind != OwnerReferenceKind::Value {
                        return None;
                    }
                    let [name] = reference.parts.as_ref() else {
                        return None;
                    };
                    owners
                        .get(name.as_str())
                        .map(|owner| ResolvedOwnerSymbolReference {
                            reference: reference.clone(),
                            owner: (*owner).clone(),
                            projection: Box::new([]),
                            parameters: Box::new([]),
                        })
                });
                resolve_owner_constraint_seed(seed, resolutions).unwrap()
            })
            .collect::<Vec<_>>();
        let results = solve(&seeds, &summaries);
        let interfaces = results
            .iter()
            .flat_map(|result| result.owners.iter())
            .map(|interface| (interface.owner.clone(), interface))
            .collect::<BTreeMap<_, _>>();
        assert_eq!(
            interfaces[&visible].result.ty, interfaces[&rows].result.ty,
            "List/filter must retain the exact late sibling provider type: {:#?}",
            interfaces[&visible],
        );
        assert!(
            boon_checked::type_is_recursively_closed(&interfaces[&visible].result.ty),
            "same-component filter result must be recursively closed: {:#?}",
            interfaces[&visible],
        );
        let Type::List(item) = &interfaces[&visible].result.ty else {
            panic!("visible must publish a list")
        };
        let Type::Object(item) = item.as_ref() else {
            panic!("visible must retain its authored record item")
        };
        assert!(!item.open);
        assert_eq!(item.fields.get("id"), Some(&Type::Text));
        assert!(item.fields.contains_key("family"));
        assert!(item.fields.contains_key("enabled"));
    }

    #[test]
    fn same_component_filter_result_tracks_a_late_lexical_capture_alpha() {
        let unit = link(concat!(
            "store: [\n",
            "    rows:\n",
            "        visible |> THEN {\n",
            "            LIST {\n",
            "                [\n",
            "                    id: TEXT { one }\n",
            "                    payload: TEXT { rich }\n",
            "                    family: Family\n",
            "                    enabled: TEXT { yes }\n",
            "                    selected: True\n",
            "                    hidden_when_group: Group\n",
            "                ]\n",
            "            }\n",
            "        }\n",
            "    visible:\n",
            "        rows\n",
            "        |> List/filter(item, if: item.family == Family)\n",
            "        |> List/filter(item, if: item.enabled == TEXT { yes })\n",
            "        |> List/filter(item, if: item.selected == True)\n",
            "        |> List/filter(item, if: item.hidden_when_group != OtherGroup)\n",
            "        |> List/filter(item, if: item.id != TEXT { removed })\n",
            "]\n",
        ));
        let rows = owner_named(&unit, "rows");
        let visible = owner_named(&unit, "visible");
        let seeds = unit
            .stable_check_owner_keys()
            .filter(|owner| matches!(owner, StableCheckOwnerKey::Item(_)))
            .map(|owner| seed(&unit, &owner))
            .collect::<Vec<_>>();
        let summaries = seeds
            .iter()
            .map(|seed| resolve_owner_constraint_seed(seed, []).unwrap())
            .collect::<Vec<_>>();
        let results = solve(&seeds, &summaries);
        let component = results
            .iter()
            .find(|result| result.owner(&visible).is_some())
            .expect("visible interface component");
        assert!(
            component.owner(&rows).is_some(),
            "fixture must keep provider and consumer in one interface SCC",
        );
        let interfaces = results
            .iter()
            .flat_map(|result| result.owners.iter())
            .map(|interface| (interface.owner.clone(), interface))
            .collect::<BTreeMap<_, _>>();
        let rows_interface = interfaces[&rows];
        let visible_interface = interfaces[&visible];
        let capture = visible_interface
            .lexical_captures
            .iter()
            .find(|capture| {
                matches!(
                    &capture.target,
                    OwnerLexicalTargetRef::Declaration {
                        owner,
                        declaration: OwnerDeclarationStableKey::Public,
                        capability: OwnerLexicalDeclarationCapability::Value,
                    } if owner == &rows
                )
            })
            .expect("visible must import rows through a lexical capture");
        assert_eq!(capture.demand_paths.as_ref(), [Box::<[String]>::default()]);
        assert_eq!(capture.flow_type.ty, rows_interface.result.ty);
        assert!(boon_checked::type_is_recursively_closed(
            &capture.flow_type.ty
        ));
        assert_eq!(
            visible_interface.result.ty, capture.flow_type.ty,
            "the filter occurrence must publish its closed captured provider",
        );
        let Type::List(item) = &visible_interface.result.ty else {
            panic!("visible must publish a list")
        };
        let Type::Object(item) = item.as_ref() else {
            panic!("visible must retain record items")
        };
        assert!(!item.open);
        assert_eq!(item.fields.get("id"), Some(&Type::Text));
        assert_eq!(item.fields.get("payload"), Some(&Type::Text));
    }

    #[test]
    fn post_transfer_replay_refreshes_an_enclosing_branch_flow() {
        let unit = link(concat!(
            "store: [\n",
            "    rows:\n",
            "        selected |> THEN {\n",
            "            LIST { [id: TEXT { one }, payload: TEXT { rich }] }\n",
            "        }\n",
            "    selected:\n",
            "        True |> WHEN {\n",
            "            True => rows |> List/filter(item, if: item.id == TEXT { one })\n",
            "            False => rows\n",
            "        }\n",
            "]\n",
        ));
        let rows = owner_named(&unit, "rows");
        let selected = owner_named(&unit, "selected");
        let seeds = unit
            .stable_check_owner_keys()
            .filter(|owner| matches!(owner, StableCheckOwnerKey::Item(_)))
            .map(|owner| seed(&unit, &owner))
            .collect::<Vec<_>>();
        let summaries = seeds
            .iter()
            .map(|seed| resolve_owner_constraint_seed(seed, []).unwrap())
            .collect::<Vec<_>>();
        let results = solve(&seeds, &summaries);
        let component = results
            .iter()
            .find(|result| result.owner(&selected).is_some())
            .expect("selected interface component");
        assert!(component.owner(&rows).is_some());
        let interfaces = results
            .iter()
            .flat_map(|result| result.owners.iter())
            .map(|interface| (interface.owner.clone(), interface))
            .collect::<BTreeMap<_, _>>();
        assert_eq!(interfaces[&selected].result.ty, interfaces[&rows].result.ty);
        assert!(boon_checked::type_is_recursively_closed(
            &interfaces[&selected].result.ty
        ));
        let Type::List(item) = &interfaces[&selected].result.ty else {
            panic!("selected must publish a list")
        };
        let Type::Object(item) = item.as_ref() else {
            panic!("selected must retain record items")
        };
        assert!(!item.open);
        assert_eq!(item.fields.get("id"), Some(&Type::Text));
        assert_eq!(item.fields.get("payload"), Some(&Type::Text));
    }

    #[test]
    fn public_record_declaration_interface_uses_its_record_value() {
        let unit = link("store: [count: 1, label: TEXT { ok }]\n");
        let store = owner_named(&unit, "store");
        let seeds = unit
            .stable_check_owner_keys()
            .filter(|owner| matches!(owner, StableCheckOwnerKey::Item(_)))
            .map(|owner| seed(&unit, &owner))
            .collect::<Vec<_>>();
        let summaries = seeds
            .iter()
            .map(|seed| resolve_owner_constraint_seed(seed, []).unwrap())
            .collect::<Vec<_>>();
        let interfaces = solve(&seeds, &summaries)
            .into_iter()
            .flat_map(|result| result.owners.into_vec())
            .map(|interface| (interface.owner.clone(), interface))
            .collect::<BTreeMap<_, _>>();
        let Type::Object(shape) = &interfaces[&store].result.ty else {
            panic!(
                "public store must publish its record value: {:#?}",
                interfaces[&store]
            );
        };
        assert_eq!(shape.fields.get("count"), Some(&Type::Number));
        assert_eq!(shape.fields.get("label"), Some(&Type::Text));
    }

    #[test]
    fn broad_when_republishes_late_closed_capture_into_every_branch() {
        let unit = link(concat!(
            "store: [\n",
            "    rows:\n",
            "        selected |> THEN {\n",
            "            LIST {\n",
            "                [\n",
            "                    id: TEXT { one }\n",
            "                    item_kind: VariableRow\n",
            "                    payload: TEXT { rich }\n",
            "                ]\n",
            "            }\n",
            "        }\n",
            "    visible:\n",
            "        rows\n",
            "        |> List/filter(item, if: item.item_kind == VariableRow)\n",
            "    selected:\n",
            "        True |> HOLD { False } |> WHEN {\n",
            "            True => visible |> List/filter(item, if: item.id == TEXT { one })\n",
            "            False => visible\n",
            "        }\n",
            "]\n",
        ));
        let rows = owner_named(&unit, "rows");
        let visible = owner_named(&unit, "visible");
        let selected = owner_named(&unit, "selected");
        let seeds = unit
            .stable_check_owner_keys()
            .filter(|owner| matches!(owner, StableCheckOwnerKey::Item(_)))
            .map(|owner| seed(&unit, &owner))
            .collect::<Vec<_>>();
        let summaries = seeds
            .iter()
            .map(|seed| resolve_owner_constraint_seed(seed, []).unwrap())
            .collect::<Vec<_>>();
        let results = solve(&seeds, &summaries);
        let component = results
            .iter()
            .find(|result| result.owner(&selected).is_some())
            .expect("selected interface component");
        assert!(component.owner(&rows).is_some());
        assert!(component.owner(&visible).is_some());
        let interfaces = results
            .iter()
            .flat_map(|result| result.owners.iter())
            .map(|interface| (interface.owner.clone(), interface))
            .collect::<BTreeMap<_, _>>();
        assert_eq!(
            interfaces[&selected].result.ty, interfaces[&rows].result.ty,
            "both WHEN branches must republish the exact late provider: {:#?}",
            interfaces[&selected],
        );
        assert!(boon_checked::type_is_recursively_closed(
            &interfaces[&selected].result.ty
        ));
    }

    #[test]
    fn broad_when_republishes_a_frozen_closed_dependency_into_every_branch() {
        let unit = link(concat!(
            "rows: LIST {\n",
            "    [\n",
            "        id: TEXT { one }\n",
            "        item_kind: VariableRow\n",
            "        payload: TEXT { rich }\n",
            "    ]\n",
            "}\n",
            "visible:\n",
            "    rows\n",
            "    |> List/filter(item, if: item.item_kind == VariableRow)\n",
            "flag: True |> HOLD { False }\n",
            "selected:\n",
            "    flag |> WHEN {\n",
            "        True => visible |> List/filter(item, if: item.id == TEXT { one })\n",
            "        False => visible\n",
            "    }\n",
        ));
        let rows = owner_named(&unit, "rows");
        let visible = owner_named(&unit, "visible");
        let flag = owner_named(&unit, "flag");
        let selected = owner_named(&unit, "selected");
        let seeds = unit
            .stable_check_owner_keys()
            .filter(|owner| matches!(owner, StableCheckOwnerKey::Item(_)))
            .map(|owner| seed(&unit, &owner))
            .collect::<Vec<_>>();
        let owners = [
            ("rows", &rows),
            ("visible", &visible),
            ("flag", &flag),
            ("selected", &selected),
        ]
        .into_iter()
        .collect::<BTreeMap<_, _>>();
        let summaries = seeds
            .iter()
            .map(|seed| {
                let resolutions = seed.references.iter().filter_map(|reference| {
                    if reference.kind != OwnerReferenceKind::Value {
                        return None;
                    }
                    let [name] = reference.parts.as_ref() else {
                        return None;
                    };
                    owners
                        .get(name.as_str())
                        .map(|owner| ResolvedOwnerSymbolReference {
                            reference: reference.clone(),
                            owner: (*owner).clone(),
                            projection: Box::new([]),
                            parameters: Box::new([]),
                        })
                });
                resolve_owner_constraint_seed(seed, resolutions).unwrap()
            })
            .collect::<Vec<_>>();
        let results = solve(&seeds, &summaries);
        let interfaces = results
            .iter()
            .flat_map(|result| result.owners.iter())
            .map(|interface| (interface.owner.clone(), interface))
            .collect::<BTreeMap<_, _>>();
        assert_eq!(
            interfaces[&selected].result.ty, interfaces[&rows].result.ty,
            "both WHEN branches must republish the frozen provider: {:#?}",
            interfaces[&selected],
        );
        assert!(boon_checked::type_is_recursively_closed(
            &interfaces[&selected].result.ty
        ));
    }

    #[test]
    fn dynamic_inputs_replay_inner_calls_before_enclosing_open_formals() {
        let unit = link(concat!(
            "FUNCTION identity(row) {\n",
            "    row\n",
            "}\n",
            "FUNCTION project(row) {\n",
            "    [\n",
            "        kind: row.kind\n",
            "        file: row.file\n",
            "    ]\n",
            "}\n",
            "rows:\n",
            "    LIST {\n",
            "        [kind: First, file: TEXT { a }]\n",
            "        [kind: Second, file: TEXT { b }]\n",
            "    }\n",
            "    |> List/map(\n",
            "        item\n",
            "        new: project(row: identity(row: item))\n",
            "    )\n",
        ));
        let identity = owner_named(&unit, "identity");
        let project = owner_named(&unit, "project");
        let rows = owner_named(&unit, "rows");
        let seeds = unit
            .stable_check_owner_keys()
            .filter(|owner| matches!(owner, StableCheckOwnerKey::Item(_)))
            .map(|owner| seed(&unit, &owner))
            .collect::<Vec<_>>();
        let parameters_for = |owner: &StableCheckOwnerKey| {
            seeds
                .iter()
                .find(|seed| &seed.owner == owner)
                .expect("callable owner seed")
                .declarations
                .iter()
                .find(|declaration| declaration.public)
                .expect("callable public declaration")
                .parameters
                .clone()
        };
        let identity_parameters = parameters_for(&identity);
        let project_parameters = parameters_for(&project);
        let summaries = seeds
            .iter()
            .map(|seed| {
                let resolutions = seed.references.iter().filter_map(|reference| {
                    if reference.kind == OwnerReferenceKind::Callable
                        && reference.parts.as_ref() == ["identity"]
                    {
                        Some(ResolvedOwnerSymbolReference {
                            reference: reference.clone(),
                            owner: identity.clone(),
                            projection: Box::new([]),
                            parameters: identity_parameters.clone(),
                        })
                    } else if reference.kind == OwnerReferenceKind::Callable
                        && reference.parts.as_ref() == ["project"]
                    {
                        Some(ResolvedOwnerSymbolReference {
                            reference: reference.clone(),
                            owner: project.clone(),
                            projection: Box::new([]),
                            parameters: project_parameters.clone(),
                        })
                    } else {
                        None
                    }
                });
                resolve_owner_constraint_seed(seed, resolutions).unwrap()
            })
            .collect::<Vec<_>>();

        let results = solve(&seeds, &summaries);
        let interfaces = results
            .iter()
            .flat_map(|result| result.owners.iter())
            .map(|interface| (interface.owner.clone(), interface))
            .collect::<BTreeMap<_, _>>();
        let identity_interface = interfaces[&identity];
        assert_eq!(
            identity_interface.parameters[0].flow_type.ty, identity_interface.result.ty,
            "identity must retain one live parameter/result alpha",
        );
        assert!(matches!(identity_interface.result.ty, Type::Var(_)));

        let Type::List(item) = &interfaces[&rows].result.ty else {
            panic!("nested dynamic calls must publish a list result");
        };
        assert_eq!(
            crate::type_for_nested_path(item, &["file".to_owned()]),
            Some(Type::Text),
        );
        assert_eq!(
            crate::type_for_nested_path(item, &["kind".to_owned()]),
            Some(Type::VariantSet(
                vec![
                    Variant::Tag("First".to_owned()),
                    Variant::Tag("Second".to_owned()),
                ]
                .into(),
            )),
        );
        assert!(boon_checked::type_is_recursively_closed(
            &interfaces[&rows].result.ty
        ));
    }

    #[test]
    fn nested_ordinary_call_results_publish_before_enclosing_input_matching() {
        let unit = link(concat!(
            "FUNCTION choose(kind) {\n",
            "    kind |> WHEN {\n",
            "        First => [kind: First, file: TEXT { a }]\n",
            "        __ => [kind: Second, file: TEXT { b }]\n",
            "    }\n",
            "}\n",
            "FUNCTION project(row) {\n",
            "    [kind: row.kind, file: row.file]\n",
            "}\n",
            "value: project(row: choose(kind: First))\n",
        ));
        let choose = owner_named(&unit, "choose");
        let project = owner_named(&unit, "project");
        let value = owner_named(&unit, "value");
        let seeds = unit
            .stable_check_owner_keys()
            .filter(|owner| matches!(owner, StableCheckOwnerKey::Item(_)))
            .map(|owner| seed(&unit, &owner))
            .collect::<Vec<_>>();
        let parameters_for = |owner: &StableCheckOwnerKey| {
            seeds
                .iter()
                .find(|seed| &seed.owner == owner)
                .unwrap()
                .declarations
                .iter()
                .find(|declaration| declaration.public)
                .unwrap()
                .parameters
                .clone()
        };
        let choose_parameters = parameters_for(&choose);
        let project_parameters = parameters_for(&project);
        let summaries = seeds
            .iter()
            .map(|seed| {
                let resolutions = seed.references.iter().filter_map(|reference| {
                    let (owner, parameters) = if reference.kind == OwnerReferenceKind::Callable
                        && reference.parts.as_ref() == ["choose"]
                    {
                        (choose.clone(), choose_parameters.clone())
                    } else if reference.kind == OwnerReferenceKind::Callable
                        && reference.parts.as_ref() == ["project"]
                    {
                        (project.clone(), project_parameters.clone())
                    } else {
                        return None;
                    };
                    Some(ResolvedOwnerSymbolReference {
                        reference: reference.clone(),
                        owner,
                        projection: Box::new([]),
                        parameters,
                    })
                });
                resolve_owner_constraint_seed(seed, resolutions).unwrap()
            })
            .collect::<Vec<_>>();

        let results = solve(&seeds, &summaries);
        let interface = results
            .iter()
            .find_map(|result| result.owner(&value))
            .expect("value interface");
        assert!(boon_checked::type_is_recursively_closed(
            &interface.result.ty
        ));
        assert_eq!(
            crate::type_for_nested_path(&interface.result.ty, &["file".to_owned()]),
            Some(Type::Text),
        );
        assert_eq!(
            crate::type_for_nested_path(&interface.result.ty, &["kind".to_owned()]),
            Some(Type::VariantSet(
                vec![
                    Variant::Tag("First".to_owned()),
                    Variant::Tag("Second".to_owned()),
                ]
                .into(),
            )),
        );
    }

    #[test]
    fn dependency_first_interface_results_are_independent_of_owner_traversal_order() {
        let unit = link(
            "FUNCTION alpha(input) {\n    zed(input: input)\n}\nFUNCTION zed(input) {\n    Number/to_text(value: input)\n}\n",
        );
        let alpha = owner_named(&unit, "alpha");
        let zed = owner_named(&unit, "zed");
        assert!(
            alpha < zed,
            "fixture must visit the caller before its callee"
        );

        let alpha_seed = seed(&unit, &alpha);
        let zed_seed = seed(&unit, &zed);
        let callable_reference = alpha_seed
            .references
            .iter()
            .find(|reference| reference.kind == OwnerReferenceKind::Callable)
            .cloned()
            .unwrap();
        let parameters = zed_seed
            .declarations
            .iter()
            .find(|declaration| declaration.public)
            .unwrap()
            .parameters
            .clone();
        let alpha_summary = resolve_owner_constraint_seed(
            &alpha_seed,
            [ResolvedOwnerSymbolReference {
                reference: callable_reference,
                owner: zed.clone(),
                projection: Box::new([]),
                parameters,
            }],
        )
        .unwrap();
        let zed_summary = resolve_owner_constraint_seed(&zed_seed, []).unwrap();

        let results = solve(&[alpha_seed, zed_seed], &[alpha_summary, zed_summary]);
        assert_eq!(results.len(), 2);
        let alpha_interface = results
            .iter()
            .find_map(|result| result.owner(&alpha))
            .unwrap();
        let zed_interface = results
            .iter()
            .find_map(|result| result.owner(&zed))
            .unwrap();
        assert_eq!(zed_interface.parameters[0].flow_type.ty, Type::Number);
        assert_eq!(zed_interface.result.ty, Type::Text);
        assert_eq!(alpha_interface.result.ty, Type::Text);
    }

    #[test]
    fn builtin_call_uses_authoritative_result_interface() {
        let unit = link("value: Number/to_text(value: 1)\n");
        let owner = owner_named(&unit, "value");
        let seed = seed(&unit, &owner);
        let summary = resolve_owner_constraint_seed(&seed, []).unwrap();
        let results = solve(&[seed], &[summary]);
        assert_eq!(results[0].owner(&owner).unwrap().result.ty, Type::Text);
    }

    #[test]
    fn simple_function_interfaces_match_the_independent_whole_checker_oracle() {
        for (source, name) in [
            ("FUNCTION identity(input) {\n    input\n}\n", "identity"),
            (
                "FUNCTION increment(input) {\n    input + 1\n}\n",
                "increment",
            ),
        ] {
            assert_checked_interface_parity(source, name);
        }
    }

    #[test]
    fn passed_context_interface_matches_the_independent_whole_checker_oracle() {
        assert_checked_interface_parity(
            concat!(
                "FUNCTION leaf() {\n",
                "    PASSED.store.count\n",
                "}\n",
                "value: leaf(PASS: [store: [count: 1]])\n",
            ),
            "leaf",
        );
    }

    #[test]
    fn output_scoped_parameter_interface_matches_the_independent_whole_checker_oracle() {
        assert_checked_interface_parity(
            concat!(
                "FUNCTION sorted(list, entry: OUT, key) {\n",
                "    list |> List/sort_by(item: entry, key: key, direction: Ascending)\n",
                "}\n",
                "rows: LIST { [rank: 1] }\n",
                "ordered: rows |> sorted(entry, key: entry.rank)\n",
            ),
            "sorted",
        );
    }

    #[test]
    fn inherited_context_interface_matches_the_independent_whole_checker_oracle() {
        let source = concat!(
            "FUNCTION leaf() {\n",
            "    PASSED.store.count\n",
            "}\n",
            "FUNCTION inherited() {\n",
            "    leaf()\n",
            "}\n",
            "value: inherited(PASS: [store: [count: 1]])\n",
        );
        let unit = link(source);
        let leaf = owner_named(&unit, "leaf");
        let inherited = owner_named(&unit, "inherited");
        let leaf_seed = seed(&unit, &leaf);
        let inherited_seed = seed(&unit, &inherited);
        let leaf_summary = resolve_owner_constraint_seed(&leaf_seed, []).unwrap();
        let reference = inherited_seed
            .references
            .iter()
            .find(|reference| reference.kind == OwnerReferenceKind::Callable)
            .cloned()
            .unwrap();
        let inherited_summary = resolve_owner_constraint_seed(
            &inherited_seed,
            [ResolvedOwnerSymbolReference {
                reference,
                owner: leaf.clone(),
                projection: Box::new([]),
                parameters: Box::new([]),
            }],
        )
        .unwrap();
        let results = solve(
            &[leaf_seed, inherited_seed],
            &[leaf_summary, inherited_summary],
        );
        for (owner, name) in [(leaf, "leaf"), (inherited, "inherited")] {
            let interface = results
                .iter()
                .find_map(|result| result.owner(&owner))
                .unwrap();
            assert_eq!(
                interface.context,
                checked_callable_interface(source, name).context,
                "{name} inherited context interface",
            );
        }
    }
}
