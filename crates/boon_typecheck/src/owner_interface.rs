use crate::owner_signature_lexical::effective_narrowed_selector_read_matches;
use crate::{
    AuthoritativeCallableSignature, AuthoritativeParameter, BuiltinSignatureRegistry,
    OwnerArgumentKind, OwnerCallableLexicalSignature, OwnerCallableScopeOwnerResult,
    OwnerCollectionKind, OwnerConstraintEdgeRole, OwnerConstraintNodeKind, OwnerConstraintSeed,
    OwnerConstraintSeedError, OwnerConstraintSummary, OwnerDeclarationKind,
    OwnerEffectiveLexicalReadPlan, OwnerEffectiveLexicalTarget, OwnerInferenceAbiEnvironment,
    OwnerInterfaceScc, OwnerInterfaceSccKey, OwnerLexicalDeclarationTarget, OwnerParameterKind,
    OwnerPatternConstraint, OwnerReferenceKind, OwnerSignatureDeclarationTarget,
    OwnerSignatureLexicalPlan, OwnerSymbolResolution, RenderContractRegistry,
    host_effect_signature, infix_requires_number_operands, infix_returns_bool,
    project_owner_signature_lexical_scope_plans, session_info_intrinsic_type,
};
use boon_checked::{
    BytesType, CheckedEffectSummary, CheckedParameterKind, CheckedParameterRequirement, FlowMode,
    FlowType, ObjectShape, OwnerDeclarationStableKey, OwnerLexicalDeclarationCapability,
    OwnerLexicalTargetRef, Type, TypeVar, Variant, widen_structural_type,
};
use boon_syntax::{StableCheckOwnerKey, StableExpressionKey};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

const OWNER_INTERFACE_SCC_RESULT_DOMAIN_V5: &[u8] = b"boon.owner-interface-scc-result.v5\0";
const OWNER_INTERFACE_SCC_KEY_DOMAIN_V1: &[u8] = b"boon.owner-interface-scc-key.v1\0";
const OWNER_INTERFACE_SCC_CURRENTNESS_DOMAIN_V6: &[u8] =
    b"boon.owner-interface-scc-currentness.v6\0";
const OWNER_BODY_INTERFACE_IMPORT_DOMAIN_V3: &[u8] = b"boon.owner-body-interface-import.v3\0";

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

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OwnerResultExpressionRef {
    Local {
        expression: StableExpressionKey,
    },
    Child {
        owner: StableCheckOwnerKey,
        expression: StableExpressionKey,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OwnerResultTransferInput {
    pub role: OwnerConstraintEdgeRole,
    pub expression: OwnerResultExpressionRef,
    pub formal_ordinal: Option<u32>,
    pub explicit_pass: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OwnerResultParameterRead {
    pub parameter_ordinal: u32,
    pub projection: Box<[String]>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OwnerResultAbiParameterContract {
    pub name: String,
    pub kind: CheckedParameterKind,
    pub ordinal: u32,
    pub flow_type: FlowType,
}

/// Minimal authoritative contract required to specialize an imported result
/// transfer. Identity, role, intrinsic, effect, context, requirement, and
/// construction metadata remain outside this public inference boundary.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OwnerResultAbiContract {
    pub kind: boon_checked::CheckedCallableKind,
    pub parameters: Box<[OwnerResultAbiParameterContract]>,
    pub result: FlowType,
    pub result_specialization: crate::OwnerAbiResultSpecialization,
}

impl From<&crate::OwnerInferenceCallableContract> for OwnerResultAbiContract {
    fn from(contract: &crate::OwnerInferenceCallableContract) -> Self {
        Self {
            kind: contract.kind,
            parameters: contract
                .parameters
                .iter()
                .map(|parameter| OwnerResultAbiParameterContract {
                    name: parameter.name.clone(),
                    kind: parameter.kind,
                    ordinal: parameter.ordinal,
                    flow_type: parameter.flow_type.clone(),
                })
                .collect::<Vec<_>>()
                .into_boxed_slice(),
            result: contract.result.clone(),
            result_specialization: contract.result_specialization,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OwnerResultCallTarget {
    Owner {
        owner: StableCheckOwnerKey,
    },
    Abi {
        canonical_name: String,
        contract: OwnerResultAbiContract,
    },
    Unresolved,
    Ambiguous {
        candidates: Box<[StableCheckOwnerKey]>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OwnerResultTransferNode {
    pub expression: StableExpressionKey,
    pub flow_type: FlowType,
    pub static_number: Option<String>,
    pub kind: OwnerConstraintNodeKind,
    pub inputs: Box<[OwnerResultTransferInput]>,
    pub parameter_read: Option<OwnerResultParameterRead>,
    pub call_target: Option<OwnerResultCallTarget>,
}

/// Minimal stable expression slice required to instantiate one callable result.
///
/// Non-callable owners publish `Principal`: their result cannot be instantiated
/// at another call site. A callable publishes only the backwards-reachable
/// result slice, expressed with stable identities and frozen ABI/owner targets.
/// This is the public specialization boundary: callers never copy or inspect an
/// unrelated part of the callee body to determine result mode or a
/// syntax-selected result shape.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OwnerResultTransfer {
    Principal,
    Parameter {
        read: OwnerResultParameterRead,
    },
    Expression {
        root: OwnerResultExpressionRef,
        nodes: Box<[OwnerResultTransferNode]>,
    },
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
    pub flow_type: FlowType,
}

/// Alpha-normalized public currentness surface of one authored check owner.
///
/// Source positions, body-only literal payloads, dense IDs, and implementation
/// fingerprints are absent. Parameters, exact context projections, effects,
/// and the minimal result-specialization transfer are frozen against the
/// project ABI, so callers never inspect the callee body.
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
    pub result_transfer: OwnerResultTransfer,
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
            OWNER_INTERFACE_SCC_CURRENTNESS_DOMAIN_V6,
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
    if matches!(requirement, Type::Unknown | Type::UnresolvedShape { .. })
        || matches!(
            &requirement,
            Type::Object(shape) if shape.open && shape.fields.is_empty()
        )
    {
        return;
    }
    match (provider, requirement) {
        (Type::Var(variable), requirement) => unifier.bind_var(*variable, requirement),
        (Type::Object(provider), Type::Object(requirement)) => {
            for (name, provider) in &provider.fields {
                if let Some(requirement) = requirement.fields.get(name) {
                    bind_provider_inference_holes(unifier, provider, requirement);
                }
            }
        }
        (Type::List(provider), Type::List(requirement))
        | (Type::Set(provider), Type::Set(requirement)) => {
            bind_provider_inference_holes(unifier, provider, &requirement);
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
            bind_provider_inference_holes(unifier, provider_key, &requirement_key);
            bind_provider_inference_holes(unifier, provider_value, &requirement_value);
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
                bind_provider_inference_holes(unifier, provider, &requirement);
            }
            bind_provider_inference_holes(unifier, &provider_result.ty, &requirement_result.ty);
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
                        bind_provider_inference_holes(unifier, provider, requirement);
                    }
                }
            }
        }
        (Type::Union(provider), Type::Union(requirement))
            if provider.len() == requirement.len() =>
        {
            for (provider, requirement) in provider.iter().zip(requirement) {
                bind_provider_inference_holes(unifier, provider, &requirement);
            }
        }
        _ => {}
    }
}

#[derive(Default)]
pub(crate) struct TypeUnifier {
    parents: Vec<u32>,
    ranks: Vec<u8>,
    bindings: Vec<Option<Type>>,
    steps: u64,
    changes: u64,
}

impl TypeUnifier {
    pub(crate) fn fresh(&mut self) -> TypeVar {
        let id = u32::try_from(self.parents.len()).expect("interface type-variable bound");
        self.parents.push(id);
        self.ranks.push(0);
        self.bindings.push(None);
        TypeVar(id)
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

    pub(crate) fn resolve(&mut self, ty: &Type) -> Type {
        if !type_contains_inference_variable(ty) {
            return ty.clone();
        }
        self.resolve_inner(ty, &mut BTreeSet::new())
    }

    fn resolve_inner(&mut self, ty: &Type, active: &mut BTreeSet<TypeVar>) -> Type {
        match ty {
            Type::Var(variable) => {
                let root = self.root(*variable);
                if !active.insert(root) {
                    return Type::Var(root);
                }
                let binding = self.bindings[root.0 as usize].clone();
                let resolved = binding.map_or(Type::Var(root), |binding| {
                    self.resolve_inner(&binding, active)
                });
                active.remove(&root);
                resolved
            }
            Type::Object(shape) => Type::object(ObjectShape {
                fields: shape
                    .fields
                    .iter()
                    .map(|(name, ty)| (name.clone(), self.resolve_inner(ty, active)))
                    .collect(),
                field_order: shape.field_order.clone(),
                open: shape.open,
            }),
            Type::List(item) => Type::List(Type::shared(self.resolve_inner(item, active))),
            Type::Set(item) => Type::Set(Type::shared(self.resolve_inner(item, active))),
            Type::Map { key, value } => Type::Map {
                key: Box::new(self.resolve_inner(key, active)),
                value: Box::new(self.resolve_inner(value, active)),
            },
            Type::Function { args, result } => Type::Function {
                args: args
                    .iter()
                    .map(|argument| self.resolve_inner(argument, active))
                    .collect(),
                result: Box::new(FlowType {
                    mode: result.mode,
                    ty: self.resolve_inner(&result.ty, active),
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
                                        (name.clone(), self.resolve_inner(ty, active))
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
                    .map(|member| self.resolve_inner(member, active))
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
        self.changes = self.changes.saturating_add(1);
        if self.ranks[left.0 as usize] == self.ranks[right.0 as usize] {
            self.ranks[left.0 as usize] = self.ranks[left.0 as usize].saturating_add(1);
        }
        let right_binding = self.bindings[right.0 as usize].take();
        if let Some(right_binding) = right_binding {
            self.bind_var(left, right_binding);
        }
    }

    pub(crate) fn bind_var(&mut self, variable: TypeVar, incoming: Type) {
        self.steps = self.steps.saturating_add(1);
        let variable = self.root(variable);
        let incoming = self.resolve(&incoming);
        if let Type::Var(other) = incoming {
            self.union(variable, other);
            return;
        }
        if self.occurs(variable, &incoming) {
            return;
        }
        let slot = self.bindings[variable.0 as usize].take();
        let (merged, changed) = match slot {
            None => (incoming, true),
            Some(current) if current == incoming => (current, false),
            Some(current) => {
                let merged = self.merge_resolved(current.clone(), incoming);
                let changed = merged != current;
                (merged, changed)
            }
        };
        self.bindings[variable.0 as usize] = Some(merged);
        if changed {
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
            self.changes = self.changes.saturating_add(1);
        }
    }

    fn merge_resolved(&mut self, left: Type, right: Type) -> Type {
        match (left, right) {
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

    pub(crate) fn unify(&mut self, left: Type, right: Type) {
        self.steps = self.steps.saturating_add(1);
        match (left, right) {
            (Type::Var(left), Type::Var(right)) => self.union(left, right),
            (Type::Var(variable), ty) | (ty, Type::Var(variable)) => self.bind_var(variable, ty),
            (Type::Union(members), ty) | (ty, Type::Union(members)) => {
                // A flow join may retain an unresolved branch beside already
                // concrete alternatives. A later equality/ABI requirement
                // constrains every branch, so propagate it into the holes
                // instead of leaving `VALUE | True | False` generic forever.
                // Concrete mismatches remain untouched here and are reported
                // by the exact assignability validator.
                for member in members.iter() {
                    self.unify(member.clone(), ty.clone());
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

    pub(crate) const fn steps(&self) -> u64 {
        self.steps
    }

    pub(crate) const fn changes(&self) -> u64 {
        self.changes
    }

    fn append_resolved_type_snapshot(
        &self,
        ty: &Type,
        output: &mut Vec<u8>,
        active: &mut BTreeSet<TypeVar>,
    ) {
        match ty {
            Type::Var(variable) => {
                output.push(0);
                let root = self.root_readonly(*variable);
                if !active.insert(root) {
                    output.push(0);
                    output.extend_from_slice(&root.0.to_le_bytes());
                    return;
                }
                if let Some(binding) = &self.bindings[root.0 as usize] {
                    output.push(1);
                    self.append_resolved_type_snapshot(binding, output, active);
                } else {
                    output.push(2);
                    output.extend_from_slice(&root.0.to_le_bytes());
                }
                active.remove(&root);
            }
            Type::Text => output.push(1),
            Type::Number => output.push(2),
            Type::Bytes(BytesType::Dynamic) => output.extend_from_slice(&[3, 0]),
            Type::Bytes(BytesType::Fixed(size)) => {
                output.extend_from_slice(&[3, 1]);
                output.extend_from_slice(&(*size as u64).to_le_bytes());
            }
            Type::Absent => output.push(4),
            Type::VariantSet(variants) => {
                output.push(5);
                append_snapshot_length(output, variants.len());
                for variant in variants.iter() {
                    match variant {
                        Variant::Tag(tag) => {
                            output.push(0);
                            append_snapshot_length(output, tag.len());
                            output.extend_from_slice(tag.as_bytes());
                        }
                        Variant::Tagged { tag, fields } => {
                            output.push(1);
                            append_snapshot_length(output, tag.len());
                            output.extend_from_slice(tag.as_bytes());
                            append_object_shape_snapshot(self, fields, output, active);
                        }
                    }
                }
            }
            Type::Object(shape) => {
                output.push(6);
                append_object_shape_snapshot(self, shape, output, active);
            }
            Type::RenderContract => output.push(7),
            Type::List(item) => {
                output.push(8);
                self.append_resolved_type_snapshot(item, output, active);
            }
            Type::Function { args, result } => {
                output.push(9);
                append_snapshot_length(output, args.len());
                for argument in args {
                    self.append_resolved_type_snapshot(argument, output, active);
                }
                output.push(flow_mode_tag(result.mode));
                self.append_resolved_type_snapshot(&result.ty, output, active);
            }
            Type::UnresolvedShape { reason } => {
                output.push(10);
                append_snapshot_length(output, reason.len());
                output.extend_from_slice(reason.as_bytes());
            }
            Type::Unknown => output.push(11),
            Type::Union(members) => {
                output.push(12);
                append_snapshot_length(output, members.len());
                for member in members {
                    self.append_resolved_type_snapshot(member, output, active);
                }
            }
            Type::Map { key, value } => {
                output.push(13);
                self.append_resolved_type_snapshot(key, output, active);
                self.append_resolved_type_snapshot(value, output, active);
            }
            Type::Set(item) => {
                output.push(14);
                self.append_resolved_type_snapshot(item, output, active);
            }
            Type::Bits { width } => {
                output.push(15);
                output.extend_from_slice(&width.to_le_bytes());
            }
        }
    }
}

fn append_snapshot_length(output: &mut Vec<u8>, len: usize) {
    output.extend_from_slice(&(len as u64).to_le_bytes());
}

const fn flow_mode_tag(mode: FlowMode) -> u8 {
    match mode {
        FlowMode::Continuous => 0,
        FlowMode::TickPresent => 1,
        FlowMode::PresentOrAbsent => 2,
        FlowMode::Absent => 3,
    }
}

fn append_object_shape_snapshot(
    unifier: &TypeUnifier,
    shape: &ObjectShape,
    output: &mut Vec<u8>,
    active: &mut BTreeSet<TypeVar>,
) {
    append_snapshot_length(output, shape.fields.len());
    for (name, ty) in &shape.fields {
        append_snapshot_length(output, name.len());
        output.extend_from_slice(name.as_bytes());
        unifier.append_resolved_type_snapshot(ty, output, active);
    }
    append_snapshot_length(output, shape.field_order.len());
    for name in &shape.field_order {
        append_snapshot_length(output, name.len());
        output.extend_from_slice(name.as_bytes());
    }
    output.push(u8::from(shape.open));
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
    pub bindings: Box<[(String, TypeVar)]>,
    pub selector_reads: Box<[(Box<[String]>, TypeVar)]>,
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

pub(crate) fn refine_owner_pattern_narrowings(
    unifier: &mut TypeUnifier,
    narrowings: &[OwnerPatternNarrowing],
) {
    for narrowing in narrowings {
        let selector = unifier.resolve(&Type::Var(narrowing.selector));
        for (name, binding) in &narrowing.bindings {
            if let Some(ty) = matching_pattern_field(&selector, &narrowing.pattern, name) {
                match unifier.resolve(&Type::Var(*binding)) {
                    // A demand-shaped capture can leave an explicit alias to
                    // an otherwise unconstrained consumer variable. Resolve
                    // that hole instead of preserving it as a spurious union
                    // member beside the selector-derived type.
                    Type::Var(open) => unifier.bind_var(open, ty),
                    _ => unifier.bind_flow_result(*binding, ty),
                }
            }
        }
        for (projection, read) in &narrowing.selector_reads {
            if let Some(ty) =
                matching_selector_projection(&selector, &narrowing.pattern, projection)
            {
                unifier.bind_flow_result(*read, ty);
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
    let mut current = root;
    for field in fields {
        let next = unifier.fresh();
        let existing = match unifier.resolve(&Type::Var(current)) {
            Type::Object(shape) => shape.fields.get(field).cloned(),
            _ => None,
        };
        if let Some(existing) = existing {
            // Reading a known field must not merge an open projection shape
            // back into an already closed object. Preserve the object surface
            // and only connect the projected value.
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
}

pub(crate) fn bind_flow_variables(
    unifier: &mut TypeUnifier,
    output: TypeVar,
    inputs: impl IntoIterator<Item = TypeVar>,
) {
    let inputs = inputs.into_iter().map(Type::Var).collect::<Vec<_>>();
    unifier.bind_flow_result(output, boon_checked::canonical_union_type(inputs));
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

fn signature_read_preserves_base_target_for(
    seed: &OwnerConstraintSeed,
    signature_lexical_plan: &OwnerSignatureLexicalPlan,
    expression: u32,
) -> bool {
    let Some(base) = seed
        .lexical_reads()
        .get(expression as usize)
        .and_then(Option::as_ref)
    else {
        return false;
    };
    matches!(
        signature_lexical_plan
            .reads()
            .get(expression as usize)
            .and_then(Option::as_ref),
        Some(read)
            if matches!(
                &read.target,
                OwnerEffectiveLexicalTarget::Static { target } if target == &base.target
            ) && read.projection == base.projection
    )
}

fn signature_read_preserves_base_target(state: &OwnerSolveState<'_>, expression: u32) -> bool {
    signature_read_preserves_base_target_for(state.seed, &state.signature_lexical_plan, expression)
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
                    if signature_read_preserves_base_target_for(
                        seed,
                        signature_lexical_plan,
                        input.expression,
                    ) =>
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
    internal_providers: &[(StableCheckOwnerKey, OwnerLexicalTargetRef, TypeVar, TypeVar)],
    capture_type_variables: &mut BTreeMap<
        (StableCheckOwnerKey, OwnerLexicalTargetRef),
        BTreeMap<TypeVar, TypeVar>,
    >,
    allow_requirement_backflow: bool,
    unifier: &mut TypeUnifier,
) -> Result<(), OwnerConstraintSeedError> {
    // Classify providers before mutating any capture in this round. Otherwise
    // the first consumer of one generic provider can close it and make later
    // consumers look authoritative, turning inference into owner-route order.
    let open_providers = allow_requirement_backflow
        .then(|| {
            internal_providers
                .iter()
                .filter_map(|(_, _, _, provider)| {
                    let ty = unifier.resolve(&Type::Var(*provider));
                    type_contains_inference_variable(&ty).then_some((*provider, ty))
                })
                .collect::<BTreeMap<_, _>>()
        })
        .unwrap_or_default();
    for (consumer, target, capture, provider) in internal_providers {
        let provider_type = open_providers
            .get(provider)
            .cloned()
            .unwrap_or_else(|| unifier.resolve(&Type::Var(*provider)));
        let variables = capture_type_variables
            .entry((consumer.clone(), target.clone()))
            .or_default();
        let captured = instantiate_type(&provider_type, unifier, variables);
        unifier.bind_var(*capture, captured);
        if let Some(provider_type) = open_providers.get(provider) {
            // An unresolved declaration remains part of the joint interface
            // equation: requirements discovered in a child must constrain its
            // provider. Backflow the consumer requirement through the frozen
            // structural provider shape instead of unioning roots: this binds
            // exact generic holes while preserving concrete sibling fields.
            let requirement = unifier.resolve(&Type::Var(*capture));
            bind_provider_inference_holes(unifier, provider_type, &requirement);
        }
    }

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
            }
        }
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
) -> Result<OwnerResultExpressionRef, OwnerConstraintSeedError> {
    let reference = reference as usize;
    if let Some(expression) = state.seed.expressions.get(reference) {
        return Ok(OwnerResultExpressionRef::Local {
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
    Ok(OwnerResultExpressionRef::Child {
        owner: external.owner.clone(),
        expression: external.expression.clone(),
    })
}

fn owner_result_parameter_read(
    state: &OwnerSolveState<'_>,
    expression: &crate::OwnerExpressionConstraint,
) -> Option<OwnerResultParameterRead> {
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
    Some(OwnerResultParameterRead {
        parameter_ordinal: *ordinal,
        projection: read.projection.clone(),
    })
}

fn owner_result_call_target(
    state: &OwnerSolveState<'_>,
    abi: &OwnerInferenceAbiEnvironment,
    expression: &crate::OwnerExpressionConstraint,
) -> Result<Option<OwnerResultCallTarget>, OwnerConstraintSeedError> {
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
        crate::OwnerSignatureCallTarget::Owner { owner } => OwnerResultCallTarget::Owner {
            owner: owner.clone(),
        },
        crate::OwnerSignatureCallTarget::Authoritative => {
            let contract = abi.callable(function).ok_or_else(|| {
                OwnerConstraintSeedError::new(format!(
                    "owner result transfer {:?} resolved `{function}` as authoritative without a frozen ABI contract",
                    state.seed.owner
                ))
            })?;
            OwnerResultCallTarget::Abi {
                canonical_name: function.clone(),
                contract: OwnerResultAbiContract::from(contract),
            }
        }
        crate::OwnerSignatureCallTarget::Ambiguous { candidates } => {
            OwnerResultCallTarget::Ambiguous {
                candidates: candidates.clone(),
            }
        }
        crate::OwnerSignatureCallTarget::Unresolved => OwnerResultCallTarget::Unresolved,
    };
    Ok(Some(target))
}

fn owner_result_parameter_alias(
    state: &OwnerSolveState<'_>,
    reference: u32,
) -> Option<OwnerResultParameterRead> {
    fn append_projection(
        mut read: OwnerResultParameterRead,
        projection: &[String],
    ) -> OwnerResultParameterRead {
        let mut path = read.projection.into_vec();
        path.extend(projection.iter().cloned());
        read.projection = path.into_boxed_slice();
        read
    }

    fn resolve(
        state: &OwnerSolveState<'_>,
        reference: u32,
        active: &mut BTreeSet<u32>,
    ) -> Option<OwnerResultParameterRead> {
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
                } => Some(OwnerResultParameterRead {
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

fn build_owner_result_transfer(
    state: &OwnerSolveState<'_>,
    abi: &OwnerInferenceAbiEnvironment,
    unifier: &mut TypeUnifier,
    alpha_variables: &mut BTreeMap<TypeVar, TypeVar>,
    next_alpha: &mut u32,
) -> Result<OwnerResultTransfer, OwnerConstraintSeedError> {
    if state.declaration_kind != Some(OwnerDeclarationKind::Function) {
        return Ok(OwnerResultTransfer::Principal);
    }
    let Some(root) = owner_result_expression(state) else {
        return Ok(OwnerResultTransfer::Principal);
    };
    if let Some(read) = owner_result_parameter_alias(state, root) {
        return Ok(OwnerResultTransfer::Parameter { read });
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
                    Ok(OwnerResultTransferInput {
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
            Ok(OwnerResultTransferNode {
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
    Ok(OwnerResultTransfer::Expression {
        root: root_ref,
        nodes: nodes.into_boxed_slice(),
    })
}

/// Write the exact in-process convergence surface.
///
/// This is deliberately not an artifact fingerprint: callers retain and
/// compare the encoded bytes directly, so convergence cannot be hidden by a
/// digest collision and the fixed-point loop does no cryptographic work.
fn write_solver_surface_snapshot(
    unifier: &TypeUnifier,
    states: &BTreeMap<StableCheckOwnerKey, OwnerSolveState<'_>>,
    output: &mut Vec<u8>,
) {
    output.clear();
    output.extend_from_slice(b"boon.owner-interface-solver-surface.v1\0");
    append_snapshot_length(output, states.len());
    let mut active = BTreeSet::new();
    for state in states.values() {
        append_snapshot_length(output, state.parameters.len());
        for parameter in &state.parameters {
            unifier.append_resolved_type_snapshot(
                &Type::Var(parameter.variable),
                output,
                &mut active,
            );
        }
        for variable in [state.result, state.result_flush, state.context] {
            unifier.append_resolved_type_snapshot(&Type::Var(variable), output, &mut active);
        }
        for (external, variable) in state
            .seed
            .external_expressions
            .iter()
            .zip(&state.external_expressions)
            .filter(|(external, _)| external.is_exact_enclosing_capture_for(&state.seed.owner))
        {
            append_snapshot_length(output, external.expression.route_digest_v1.len());
            output.extend_from_slice(&external.expression.route_digest_v1);
            unifier.append_resolved_type_snapshot(&Type::Var(*variable), output, &mut active);
        }
    }
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
    evaluate_owner_interface_scc_impl(scc, abi, seeds, summaries, dependency_results, None)
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
    )
}

fn evaluate_owner_interface_scc_impl<'a>(
    scc: &OwnerInterfaceScc,
    abi: &OwnerInferenceAbiEnvironment,
    seeds: impl IntoIterator<Item = &'a OwnerConstraintSeed>,
    summaries: impl IntoIterator<Item = &'a OwnerConstraintSummary>,
    dependency_results: impl IntoIterator<Item = &'a OwnerInterfaceSccResult>,
    signature_scopes: Option<Vec<&OwnerCallableScopeOwnerResult>>,
) -> Result<OwnerInterfaceSccEvaluation, OwnerConstraintSeedError> {
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
        let pattern_local_expressions =
            exact_pattern_local_expressions(seed, &signature_lexical_plan);
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
            // Cross-owner lexical capture is a provider-to-consumer flow, not
            // a type equation. Keep the consumer root independent so a local
            // projection or use-site contract cannot widen the declaring
            // PatternBinding/FreshOut/public surface before it is frozen.
            internal_lexical_capture_providers.push((
                consumer.clone(),
                target.clone(),
                capture,
                provider,
            ));
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
        let mut variables = BTreeMap::new();
        let ty = instantiate_type(&interface.result.ty, &mut unifier, &mut variables);
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
    propagate_lexical_capture_types(
        &states,
        &internal_lexical_capture_providers,
        &mut lexical_capture_type_variables,
        false,
        &mut unifier,
    )?;

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
                        bind_flow_variables(&mut unifier, variable, [input]);
                    }
                    mode = None;
                }
                OwnerConstraintNodeKind::Hold { .. } => {
                    if let Some(input) = expression
                        .inputs
                        .first()
                        .and_then(|input| expression_variable(state, input.expression))
                    {
                        unifier.unify(Type::Var(variable), Type::Var(input));
                    }
                    state.effect.reads_state = true;
                    state.effect.writes_state = true;
                }
                OwnerConstraintNodeKind::Latest => {
                    let inputs = expression
                        .inputs
                        .iter()
                        .filter_map(|input| expression_variable(state, input.expression))
                        .collect::<Vec<_>>();
                    bind_flow_variables(&mut unifier, variable, inputs);
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
                    bind_flow_variables(&mut unifier, variable, inputs);
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
                        bind_flow_variables(&mut unifier, variable, [input]);
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
                            bind_flow_variables(&mut unifier, variable, [output]);
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
                            if !signature_read_preserves_base_target(state, input.expression) {
                                return None;
                            }
                            expression_variable(state, input.expression)
                                .map(|read| (name.clone(), read))
                        })
                        .collect::<Vec<_>>();
                    for (name, read) in &local_bindings {
                        if let Some(binding_ty) =
                            pattern_binding_type_from_pattern(pattern, &pattern_ty, name)
                        {
                            unifier.unify(Type::Var(*read), binding_ty);
                        }
                    }
                    bindings.extend(local_bindings);
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
                            bind_flow_variables(&mut unifier, variable, [result]);
                        }
                        mode = None;
                    } else {
                        unifier.bind_var(variable, Type::Absent);
                        mode = Some(FlowMode::Absent);
                    }
                }
                OwnerConstraintNodeKind::Collection { collection, .. } => match collection {
                    OwnerCollectionKind::List => {
                        let item = unifier.fresh();
                        for input in &expression.inputs {
                            if let Some(input) = expression_variable(state, input.expression) {
                                unifier.bind_flow_result(item, Type::Var(input));
                            }
                        }
                        unifier.bind_var(variable, Type::List(Type::shared(Type::Var(item))));
                    }
                    OwnerCollectionKind::Set => {
                        let item = unifier.fresh();
                        for input in &expression.inputs {
                            if let Some(input) = expression_variable(state, input.expression) {
                                unifier.bind_flow_result(item, Type::Var(input));
                            }
                        }
                        unifier.bind_var(variable, Type::Set(Type::shared(Type::Var(item))));
                    }
                    OwnerCollectionKind::Bytes => {
                        unifier.bind_var(variable, Type::Bytes(BytesType::Dynamic));
                    }
                    OwnerCollectionKind::Map => {
                        let key = unifier.fresh();
                        let value = unifier.fresh();
                        for input in &expression.inputs {
                            let Some(input) = expression_variable(state, input.expression) else {
                                continue;
                            };
                            unifier.bind_var(
                                input,
                                Type::object(ObjectShape::from_ordered_fields(
                                    [
                                        ("key".to_owned(), Type::Var(key)),
                                        ("value".to_owned(), Type::Var(value)),
                                    ],
                                    false,
                                )),
                            );
                        }
                        unifier.bind_var(
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
                            bind_flow_variables(&mut unifier, variable, [output]);
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

    let mut call_variables = vec![BTreeMap::new(); calls.len()];
    // Context schemes are instantiated independently from the ordinary call
    // result/parameter scheme. The checked-program oracle deliberately gives
    // an inherited PASSED requirement its own per-call variables; sharing this
    // substitution map would incorrectly couple a wrapper's result type to its
    // inherited context leaf.
    let mut call_context_variables = vec![BTreeMap::new(); calls.len()];
    let requires_fixed_point = !calls.is_empty()
        || !pattern_narrowings.is_empty()
        || !internal_lexical_capture_providers.is_empty()
        || states
            .values()
            .any(|state| !state.lexical_capture_variables.is_empty());
    let mut previous_surface = Vec::new();
    if requires_fixed_point {
        write_solver_surface_snapshot(&unifier, &states, &mut previous_surface);
    }
    let mut current_surface = Vec::with_capacity(previous_surface.len());
    let maximum_rounds = if requires_fixed_point {
        states.len().saturating_add(calls.len()).saturating_add(2)
    } else {
        0
    };
    let mut converged = !requires_fixed_point;
    for _round in 0..maximum_rounds {
        work.solve_rounds = work.solve_rounds.saturating_add(1);
        let changes_before = unifier.changes();
        let mut surface_changed = false;
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
                    let context = instantiate_type(
                        &unifier.resolve(&Type::Var(callee.context)),
                        &mut unifier,
                        &mut *context_variables,
                    );
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
                        Some(context),
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
                    })
                    .collect();
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
            if call_valid
                && !has_explicit_pass
                && let Some(context) = &context
                && let Some(caller_context) = caller_context
            {
                unifier.unify(Type::Var(caller_context), context.clone());
            }
            if call_valid && let Some(field) = call.function.strip_prefix("Field/") {
                if let Some(input) = signature_call
                    .matched_inputs
                    .iter()
                    .find(|input| {
                        input.source == crate::OwnerSignatureMatchedInputSource::PipeInput
                    })
                    .map(|input| input.expression)
                    && let Some(input) = expression_variable(caller, input)
                {
                    let projected = bind_projection(&mut unifier, input, &[field.to_owned()]);
                    unifier.unify(Type::Var(call_variable), Type::Var(projected));
                }
            } else if call_valid {
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
                    unifier.unify(Type::Var(input), parameter.ty.clone());
                }
            }
            if call_valid
                && let (Some(pass), Some(context)) = (&signature_call.explicit_pass, &context)
                && let Some(input) = expression_variable(caller, pass.expression)
            {
                unifier.unify(Type::Var(input), context.clone());
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
                        bind_signature_declaration_reads(
                            state,
                            target,
                            variable,
                            parameter.mode,
                            &mut unifier,
                        );
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
                        bind_signature_declaration_reads(
                            state,
                            &planned.target,
                            variable,
                            context.mode,
                            &mut unifier,
                        );
                    }
                }
                if call_valid {
                    let merged = merge_effects(state.effect, effect);
                    surface_changed |= merged != state.effect;
                    state.effect = merged;
                }
                if call_valid {
                    let mode = flow_mode_join(state.modes[call.expression], Some(result_mode));
                    surface_changed |= mode != state.modes[call.expression];
                    state.modes[call.expression] = mode;
                }
            }
            let _ = call.stable_expression;
            work.cross_owner_constraints = work.cross_owner_constraints.saturating_add(1);
        }
        refine_owner_pattern_narrowings(&mut unifier, &pattern_narrowings);
        propagate_lexical_capture_types(
            &states,
            &internal_lexical_capture_providers,
            &mut lexical_capture_type_variables,
            true,
            &mut unifier,
        )?;
        surface_changed |= propagate_lexical_capture_modes(&mut states)?;
        if unifier.changes() == changes_before && !surface_changed {
            converged = true;
            break;
        }
        write_solver_surface_snapshot(&unifier, &states, &mut current_surface);
        if current_surface == previous_surface && !surface_changed {
            converged = true;
            break;
        }
        std::mem::swap(&mut previous_surface, &mut current_surface);
    }
    if !converged {
        return Err(OwnerConstraintSeedError::new(format!(
            "owner interface SCC {:?} did not converge in {maximum_rounds} rounds",
            scc.key
        )));
    }

    let mut interfaces = Vec::with_capacity(states.len());
    let mut alpha_variables = BTreeMap::new();
    let mut next_alpha = 0;
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
        let resolved_result = result_expression.map_or(
            FlowType {
                mode: FlowMode::Continuous,
                ty: Type::Absent,
            },
            |expression| {
                resolved_expression_boundary(state, expression, &mut unifier, raw_result_mode)
            },
        );
        let resolved_result_flush_type = result_expression
            .and_then(|expression| resolved_expression_flush_type(state, expression, &mut unifier));
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
                projections: crate::context_scheme_projections(&flow_type.ty)
                    .into_iter()
                    .map(Vec::into_boxed_slice)
                    .collect::<Vec<_>>()
                    .into_boxed_slice(),
                flow_type,
            }
        });
        let result_transfer = build_owner_result_transfer(
            state,
            abi,
            &mut unifier,
            &mut alpha_variables,
            &mut next_alpha,
        )?;
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
                let flow_type =
                    resolved_expression_boundary(provider, expression as u32, &mut unifier, mode);
                let flush_type =
                    resolved_expression_flush_type(provider, expression as u32, &mut unifier);
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
        if let OwnerResultTransfer::Expression { nodes, .. } = &result_transfer {
            for node in nodes {
                collect_type_variables(&node.flow_type.ty, &mut type_variables);
            }
        }
        let mut interface = OwnerPublicInterface {
            owner: owner.clone(),
            declaration_kind: state.declaration_kind,
            names: state.names.clone(),
            parameters: parameters.into_boxed_slice(),
            result,
            result_flush_type,
            result_transfer,
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
            OWNER_BODY_INTERFACE_IMPORT_DOMAIN_V3,
            &interface,
        )
        .map_err(|error| {
            OwnerConstraintSeedError::new(format!(
                "cannot fingerprint owner body interface import: {error}"
            ))
        })?;
        interfaces.push(interface);
    }
    work.unification_steps = unifier.steps;
    // Every public interface already owns an exact canonical semantic
    // fingerprint. Aggregate those seals instead of serializing every full
    // interface a second time at the SCC boundary.
    let interface_fingerprints = interfaces
        .iter()
        .map(OwnerPublicInterface::fingerprint_v1)
        .collect::<Vec<_>>();
    let fingerprint_v1 = boon_contract::canonical_serde_hash_v1(
        OWNER_INTERFACE_SCC_RESULT_DOMAIN_V5,
        &(&scc.key, &interface_fingerprints, next_alpha),
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
        type_variable_count: next_alpha,
        work,
        key_fingerprint_v1,
        fingerprint_v1,
    });
    let currentness = OwnerInterfaceSccCurrentnessReceipt::from_current_evaluation(basis, &result)?;
    Ok(OwnerInterfaceSccEvaluation {
        currentness,
        result,
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
    fn resolved_type_snapshot_is_exact_reusable_and_observes_root_binding() {
        fn snapshot(unifier: &TypeUnifier, ty: &Type) -> Vec<u8> {
            let mut output = Vec::new();
            unifier.append_resolved_type_snapshot(ty, &mut output, &mut BTreeSet::new());
            output
        }

        let mut unifier = TypeUnifier::default();
        let unresolved = unifier.fresh();
        let alias = unifier.fresh();
        unifier.unify(Type::Var(unresolved), Type::Var(alias));
        let ty = Type::Function {
            args: vec![
                Type::object(ObjectShape {
                    fields: [
                        ("value".to_owned(), Type::Var(alias)),
                        (
                            "payload".to_owned(),
                            Type::VariantSet(boon_checked::SharedVariantSet::new(vec![
                                Variant::Tag("Empty".to_owned()),
                                Variant::tagged(
                                    "Found".to_owned(),
                                    ObjectShape {
                                        fields: [("item".to_owned(), Type::Text)].into(),
                                        field_order: vec!["item".to_owned()],
                                        open: false,
                                    },
                                ),
                            ])),
                        ),
                    ]
                    .into(),
                    field_order: vec!["value".to_owned(), "payload".to_owned()],
                    open: true,
                }),
                Type::Map {
                    key: Box::new(Type::Bytes(BytesType::Fixed(8))),
                    value: Box::new(Type::Set(Type::shared(Type::Bits { width: 16 }))),
                },
                Type::RenderContract,
                Type::Unknown,
                Type::UnresolvedShape {
                    reason: "test".to_owned(),
                },
            ],
            result: Box::new(FlowType {
                mode: FlowMode::PresentOrAbsent,
                ty: Type::Union(vec![
                    Type::List(Type::shared(Type::Var(unresolved))),
                    Type::Absent,
                ]),
            }),
        };

        let unbound = snapshot(&unifier, &ty);
        assert_eq!(snapshot(&unifier, &ty), unbound);
        unifier.bind_var(unresolved, Type::Number);
        let bound = snapshot(&unifier, &ty);
        assert_ne!(bound, unbound);
        assert_eq!(snapshot(&unifier, &ty), bound);
    }

    #[test]
    fn union_equality_constrains_unresolved_flow_branches() {
        let mut unifier = TypeUnifier::default();
        let unresolved = unifier.fresh();
        let boolean = Type::VariantSet(boon_checked::SharedVariantSet::new(vec![
            Variant::Tag("False".to_owned()),
            Variant::Tag("True".to_owned()),
        ]));
        let joined = boon_checked::canonical_union_type(vec![
            Type::Var(unresolved),
            Type::VariantSet(boon_checked::SharedVariantSet::new(vec![Variant::Tag(
                "False".to_owned(),
            )])),
            Type::VariantSet(boon_checked::SharedVariantSet::new(vec![Variant::Tag(
                "True".to_owned(),
            )])),
        ]);

        unifier.unify(joined, boolean.clone());

        assert_eq!(unifier.resolve(&Type::Var(unresolved)), boolean);
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
                (first_owner, target.clone(), first, provider),
                (second_owner, target, second, provider),
            ];
            if reverse {
                providers.reverse();
            }
            let states = BTreeMap::<StableCheckOwnerKey, OwnerSolveState<'_>>::new();
            propagate_lexical_capture_types(
                &states,
                &providers,
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
            &[(consumer_owner, target, capture, provider)],
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
        assert_eq!(
            interface.result_transfer,
            OwnerResultTransfer::Parameter {
                read: OwnerResultParameterRead {
                    parameter_ordinal: 0,
                    projection: Box::new([]),
                },
            }
        );
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
        assert_eq!(
            interface.result_transfer,
            OwnerResultTransfer::Parameter {
                read: OwnerResultParameterRead {
                    parameter_ordinal: 0,
                    projection: Box::new([]),
                },
            }
        );
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
