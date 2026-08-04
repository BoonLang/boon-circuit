use crate::{
    AuthoritativeCallableSignature, AuthoritativeParameter, BuiltinSignatureRegistry,
    OwnerArgumentKind, OwnerCallableAbiEnvironment, OwnerCollectionKind, OwnerConstraintEdgeRole,
    OwnerConstraintNodeKind, OwnerConstraintSeed, OwnerConstraintSeedError, OwnerConstraintSummary,
    OwnerDeclarationKind, OwnerInterfaceScc, OwnerInterfaceSccKey, OwnerParameterKind,
    OwnerPatternConstraint, OwnerReferenceKind, OwnerSymbolResolution, RenderContractRegistry,
    host_effect_signature, infix_returns_bool, session_info_intrinsic_type,
};
use boon_checked::{
    BytesType, CheckedEffectSummary, CheckedParameterKind, CheckedParameterRequirement, FlowMode,
    FlowType, ObjectShape, Type, TypeVar, Variant, widen_structural_type,
};
use boon_syntax::{StableCheckOwnerKey, StableExpressionKey};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

const OWNER_INTERFACE_SCC_RESULT_DOMAIN_V1: &[u8] = b"boon.owner-interface-scc-result.v1\0";

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
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OwnerResultParameterRead {
    pub parameter_ordinal: u32,
    pub projection: Box<[String]>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OwnerResultCallTarget {
    Owner {
        owner: StableCheckOwnerKey,
    },
    Abi {
        canonical_name: String,
        contract_fingerprint_v1: [u8; 32],
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
    pub context: Option<OwnerContextInterface>,
    pub effect: CheckedEffectSummary,
    pub type_variables: Box<[TypeVar]>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
pub struct OwnerInterfaceSolveWork {
    pub owners: u64,
    pub expressions: u64,
    pub local_constraints: u64,
    pub cross_owner_constraints: u64,
    pub unification_steps: u64,
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
    fingerprint_v1: [u8; 32],
}

impl OwnerInterfaceSccResult {
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

#[derive(Default)]
pub(crate) struct TypeUnifier {
    parents: Vec<u32>,
    ranks: Vec<u8>,
    bindings: Vec<Option<Type>>,
    steps: u64,
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
        let merged = slot.map_or(incoming.clone(), |current| {
            self.merge_resolved(current, incoming.clone())
        });
        self.bindings[variable.0 as usize] = Some(merged);
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
}

#[derive(Clone)]
struct OwnerSolveParameter {
    name: String,
    kind: OwnerParameterKind,
    ordinal: u32,
    variable: TypeVar,
    evaluation_scope: OwnerInterfaceEvaluationScope,
}

#[derive(Clone)]
struct OwnerSolveState<'a> {
    seed: &'a OwnerConstraintSeed,
    summary: &'a OwnerConstraintSummary,
    declaration_kind: Option<OwnerDeclarationKind>,
    names: Box<[String]>,
    parameters: Vec<OwnerSolveParameter>,
    context: TypeVar,
    result: TypeVar,
    result_flush: TypeVar,
    expressions: Vec<TypeVar>,
    expression_flushes: Vec<TypeVar>,
    expression_by_key: BTreeMap<StableExpressionKey, usize>,
    external_expressions: Vec<TypeVar>,
    external_expression_flushes: Vec<TypeVar>,
    local_roots: BTreeMap<String, Option<TypeVar>>,
    modes: Vec<Option<FlowMode>>,
    effect: CheckedEffectSummary,
}

#[derive(Clone)]
struct CrossCall {
    caller: StableCheckOwnerKey,
    expression: usize,
    target: Option<StableCheckOwnerKey>,
    function: String,
    inputs: Box<[(OwnerConstraintEdgeRole, u32)]>,
    stable_expression: StableExpressionKey,
    flush: TypeVar,
}

#[derive(Clone)]
struct InstantiatedInterfaceParameter {
    name: String,
    kind: OwnerParameterKind,
    ordinal: u32,
    ty: Type,
    requirement: CheckedParameterRequirement,
    evaluation_scope: OwnerInterfaceEvaluationScope,
}

fn interface_call_shape_is_valid(
    call: &CrossCall,
    parameters: &[InstantiatedInterfaceParameter],
    authoritative: bool,
) -> bool {
    let pipe_input = call
        .inputs
        .iter()
        .any(|(role, _)| matches!(role, OwnerConstraintEdgeRole::PipeInput));
    let piped_parameter = pipe_input
        .then(|| {
            parameters
                .iter()
                .filter(|parameter| parameter.kind == OwnerParameterKind::Value)
                .min_by_key(|parameter| parameter.ordinal)
        })
        .flatten();
    if pipe_input && piped_parameter.is_none() {
        return false;
    }
    let expected = parameters
        .iter()
        .filter(|parameter| piped_parameter.is_none_or(|piped| parameter.ordinal != piped.ordinal))
        .collect::<Vec<_>>();
    let mut expected_index = 0usize;
    for (role, _) in call.inputs.iter().filter(|(role, _)| {
        matches!(
            role,
            OwnerConstraintEdgeRole::CallArgument { .. }
                | OwnerConstraintEdgeRole::PipeArgument { .. }
        )
    }) {
        let (kind, name) = match role {
            OwnerConstraintEdgeRole::CallArgument { kind, name, .. }
            | OwnerConstraintEdgeRole::PipeArgument { kind, name, .. } => (*kind, name),
            _ => unreachable!("filtered call argument role"),
        };
        while let Some(parameter) = expected.get(expected_index).copied()
            && parameter.name != *name
            && parameter.requirement.is_optional()
        {
            expected_index += 1;
        }
        let Some(parameter) = expected.get(expected_index).copied() else {
            return false;
        };
        if parameter.name != *name
            || (parameter.kind == OwnerParameterKind::Value
                && kind == OwnerArgumentKind::BareBinding)
        {
            return false;
        }
        expected_index += 1;
    }
    if expected
        .iter()
        .skip(expected_index)
        .any(|parameter| !parameter.requirement.is_optional())
    {
        return false;
    }
    !(authoritative
        && call.inputs.iter().any(|(role, _)| {
            matches!(
                role,
                OwnerConstraintEdgeRole::CallPass { .. } | OwnerConstraintEdgeRole::PipePass { .. }
            )
        }))
}

pub(crate) fn add_local_root(
    roots: &mut BTreeMap<String, Option<TypeVar>>,
    name: String,
    variable: TypeVar,
) {
    match roots.entry(name) {
        std::collections::btree_map::Entry::Vacant(entry) => {
            entry.insert(Some(variable));
        }
        std::collections::btree_map::Entry::Occupied(mut entry) => {
            if entry.get().is_some_and(|existing| existing != variable) {
                entry.insert(None);
            }
        }
    }
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

fn direct_owner_parameter_ordinal(state: &OwnerSolveState<'_>, reference: u32) -> Option<u32> {
    let expression = state.seed.expressions.get(reference as usize)?;
    let parts = match &expression.kind {
        OwnerConstraintNodeKind::Reference { parts } | OwnerConstraintNodeKind::Drain { parts } => {
            parts
        }
        _ => return None,
    };
    let [name] = parts.as_ref() else {
        return None;
    };
    let parameter = state
        .parameters
        .iter()
        .find(|parameter| parameter.name == *name)?;
    (state.local_roots.get(name) == Some(&Some(parameter.variable))).then_some(parameter.ordinal)
}

fn referenced_owner_parameter_ordinals(
    state: &OwnerSolveState<'_>,
    reference: u32,
) -> BTreeSet<u32> {
    let mut referenced = BTreeSet::new();
    let mut pending = vec![reference];
    let mut visited = BTreeSet::new();
    while let Some(reference) = pending.pop() {
        if reference as usize >= state.seed.expressions.len() || !visited.insert(reference) {
            continue;
        }
        let expression = &state.seed.expressions[reference as usize];
        if let OwnerConstraintNodeKind::Reference { parts }
        | OwnerConstraintNodeKind::Drain { parts } = &expression.kind
            && let Some(name) = parts.first()
            && let Some(parameter) = state
                .parameters
                .iter()
                .find(|parameter| parameter.name == *name)
            && state.local_roots.get(name) == Some(&Some(parameter.variable))
        {
            referenced.insert(parameter.ordinal);
        }
        pending.extend(expression.inputs.iter().map(|input| input.expression));
    }
    referenced
}

fn call_input_for_parameter(
    call: &CrossCall,
    parameter: &InstantiatedInterfaceParameter,
    parameters: &[InstantiatedInterfaceParameter],
) -> Option<u32> {
    call.inputs
        .iter()
        .find_map(|(role, expression)| match role {
            OwnerConstraintEdgeRole::CallArgument { name, .. }
            | OwnerConstraintEdgeRole::PipeArgument { name, .. }
                if *name == parameter.name =>
            {
                Some(*expression)
            }
            OwnerConstraintEdgeRole::PipeInput
                if parameters
                    .iter()
                    .find(|candidate| candidate.kind == OwnerParameterKind::Value)
                    .is_some_and(|candidate| candidate.ordinal == parameter.ordinal) =>
            {
                Some(*expression)
            }
            _ => None,
        })
}

fn forwarded_owner_output_ordinal(
    caller: &OwnerSolveState<'_>,
    call: &CrossCall,
    parameters: &[InstantiatedInterfaceParameter],
    output_ordinal: u32,
) -> Option<u32> {
    let output = parameters.iter().find(|parameter| {
        parameter.kind == OwnerParameterKind::Out && parameter.ordinal == output_ordinal
    })?;
    let expression = call_input_for_parameter(call, output, parameters)?;
    let owner_ordinal = direct_owner_parameter_ordinal(caller, expression)?;
    caller
        .parameters
        .iter()
        .find(|parameter| parameter.ordinal == owner_ordinal)
        .filter(|parameter| parameter.kind == OwnerParameterKind::Out)
        .map(|parameter| parameter.ordinal)
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
    let parts = match &expression.kind {
        OwnerConstraintNodeKind::Reference { parts } | OwnerConstraintNodeKind::Drain { parts } => {
            parts
        }
        _ => return None,
    };
    let (name, projection) = parts.split_first()?;
    let parameter = state
        .parameters
        .iter()
        .find(|parameter| parameter.name == *name)?;
    (state.local_roots.get(name) == Some(&Some(parameter.variable))).then(|| {
        OwnerResultParameterRead {
            parameter_ordinal: parameter.ordinal,
            projection: projection.to_vec().into_boxed_slice(),
        }
    })
}

fn owner_result_call_target(
    state: &OwnerSolveState<'_>,
    abi: &OwnerCallableAbiEnvironment,
    expression: &crate::OwnerExpressionConstraint,
) -> Result<Option<OwnerResultCallTarget>, OwnerConstraintSeedError> {
    let function = match &expression.kind {
        OwnerConstraintNodeKind::Call { function }
        | OwnerConstraintNodeKind::Pipe {
            operation: function,
        } => function,
        _ => return Ok(None),
    };
    let resolution = state
        .summary
        .symbol_resolutions
        .iter()
        .find(|resolution| resolution.reference().expression == expression.expression);
    let target = match resolution {
        Some(OwnerSymbolResolution::Resolved { owner, .. }) => OwnerResultCallTarget::Owner {
            owner: owner.clone(),
        },
        Some(OwnerSymbolResolution::Authoritative { .. }) => {
            let contract = abi.callable(function).ok_or_else(|| {
                OwnerConstraintSeedError::new(format!(
                    "owner result transfer {:?} resolved `{function}` as authoritative without a frozen ABI contract",
                    state.seed.owner
                ))
            })?;
            let contract_fingerprint_v1 = boon_contract::canonical_serde_hash_v1(
                b"boon.owner-result-abi-call.v1\0",
                contract,
            )
            .map_err(|error| {
                OwnerConstraintSeedError::new(format!(
                    "cannot fingerprint owner result ABI call `{function}`: {error}"
                ))
            })?;
            OwnerResultCallTarget::Abi {
                canonical_name: function.clone(),
                contract_fingerprint_v1,
            }
        }
        Some(OwnerSymbolResolution::Ambiguous { candidates, .. }) => {
            OwnerResultCallTarget::Ambiguous {
                candidates: candidates
                    .iter()
                    .map(|candidate| candidate.owner.clone())
                    .collect::<Vec<_>>()
                    .into_boxed_slice(),
            }
        }
        Some(OwnerSymbolResolution::Unresolved { .. }) | None => OwnerResultCallTarget::Unresolved,
    };
    Ok(Some(target))
}

fn owner_result_parameter_alias(
    state: &OwnerSolveState<'_>,
    reference: u32,
) -> Option<OwnerResultParameterRead> {
    fn resolve(
        state: &OwnerSolveState<'_>,
        reference: u32,
        lexical: &BTreeMap<String, Option<OwnerResultParameterRead>>,
        active: &mut BTreeSet<u32>,
    ) -> Option<OwnerResultParameterRead> {
        let expression = state.seed.expressions.get(reference as usize)?;
        if !active.insert(reference) {
            return None;
        }
        let result = match &expression.kind {
            OwnerConstraintNodeKind::Reference { parts } => {
                let (name, projection) = parts.split_first()?;
                if let Some(read) = lexical.get(name) {
                    read.clone().map(|mut read| {
                        let mut path = read.projection.into_vec();
                        path.extend(projection.iter().cloned());
                        read.projection = path.into_boxed_slice();
                        read
                    })
                } else {
                    owner_result_parameter_read(state, expression)
                }
            }
            OwnerConstraintNodeKind::Block => {
                let mut lexical = lexical.clone();
                for input in &expression.inputs {
                    if let OwnerConstraintEdgeRole::BlockBinding { name } = &input.role {
                        let binding = resolve(state, input.expression, &lexical, active);
                        // Preserve lexical shadowing even when the binding is
                        // not a transparent alias of an owner parameter.
                        lexical.insert(name.clone(), binding);
                    }
                }
                expression
                    .inputs
                    .iter()
                    .find(|input| matches!(input.role, OwnerConstraintEdgeRole::BlockResult))
                    .and_then(|input| resolve(state, input.expression, &lexical, active))
            }
            _ => None,
        };
        active.remove(&reference);
        result
    }

    resolve(state, reference, &BTreeMap::new(), &mut BTreeSet::new())
}

fn build_owner_result_transfer(
    state: &OwnerSolveState<'_>,
    abi: &OwnerCallableAbiEnvironment,
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
            let inputs = expression
                .inputs
                .iter()
                .map(|input| {
                    Ok(OwnerResultTransferInput {
                        role: input.role.clone(),
                        expression: owner_result_expression_ref(state, input.expression)?,
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

fn solver_surface_snapshot(
    unifier: &mut TypeUnifier,
    states: &BTreeMap<StableCheckOwnerKey, OwnerSolveState<'_>>,
) -> Vec<(
    StableCheckOwnerKey,
    Vec<Type>,
    Type,
    Type,
    Type,
    CheckedEffectSummary,
    Vec<Option<FlowMode>>,
    Vec<OwnerInterfaceEvaluationScope>,
)> {
    states
        .iter()
        .map(|(owner, state)| {
            (
                owner.clone(),
                state
                    .parameters
                    .iter()
                    .map(|parameter| unifier.resolve(&Type::Var(parameter.variable)))
                    .collect(),
                unifier.resolve(&Type::Var(state.result)),
                unifier.resolve(&Type::Var(state.result_flush)),
                unifier.resolve(&Type::Var(state.context)),
                state.effect,
                state.modes.clone(),
                state
                    .parameters
                    .iter()
                    .map(|parameter| parameter.evaluation_scope)
                    .collect(),
            )
        })
        .collect()
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

/// Solve one dependency-first tagged interface SCC atomically.
pub fn solve_owner_interface_scc<'a>(
    scc: &OwnerInterfaceScc,
    abi: &OwnerCallableAbiEnvironment,
    seeds: impl IntoIterator<Item = &'a OwnerConstraintSeed>,
    summaries: impl IntoIterator<Item = &'a OwnerConstraintSummary>,
    dependency_results: impl IntoIterator<Item = &'a OwnerInterfaceSccResult>,
) -> Result<OwnerInterfaceSccResult, OwnerConstraintSeedError> {
    let seeds = seeds
        .into_iter()
        .map(|seed| (seed.owner.clone(), seed))
        .collect::<BTreeMap<_, _>>();
    let summaries = summaries
        .into_iter()
        .map(|summary| (summary.owner.clone(), summary))
        .collect::<BTreeMap<_, _>>();
    let expected = scc.key.members.iter().cloned().collect::<BTreeSet<_>>();
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

    let mut dependency_interfaces = BTreeMap::new();
    let mut dependency_keys = BTreeSet::new();
    for result in dependency_results {
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

    let mut unifier = TypeUnifier::default();
    let mut states = BTreeMap::<StableCheckOwnerKey, OwnerSolveState<'_>>::new();
    for owner in &scc.key.members {
        let seed = seeds[owner];
        let summary = summaries[owner];
        let public = seed
            .declarations
            .iter()
            .find(|declaration| declaration.public);
        let mut parameters = Vec::new();
        let mut local_roots = BTreeMap::new();
        if let Some(public) = public {
            for parameter in &public.parameters {
                let variable = unifier.fresh();
                parameters.push(OwnerSolveParameter {
                    name: parameter.name.clone(),
                    kind: parameter.kind,
                    ordinal: parameter.ordinal,
                    variable,
                    evaluation_scope: OwnerInterfaceEvaluationScope::Parent,
                });
                add_local_root(&mut local_roots, parameter.name.clone(), variable);
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
        states.insert(
            owner.clone(),
            OwnerSolveState {
                seed,
                summary,
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
                expression_by_key,
                external_expressions,
                external_expression_flushes,
                local_roots,
                modes: vec![None; seed.expressions.len()],
                effect,
            },
        );
    }

    // Publish owner-local declaration and BLOCK binding roots before resolving
    // reads. Duplicate names fail closed to an unconstrained reference rather
    // than silently selecting one declaration.
    for state in states.values_mut() {
        for declaration in &state.seed.declarations {
            let Some((_, expression)) = state
                .seed
                .statement_values
                .iter()
                .find(|(statement, _)| *statement == declaration.statement)
            else {
                continue;
            };
            let Some(variable) = expression_boundary_variable(state, *expression, &mut unifier)
            else {
                continue;
            };
            for name in &declaration.names {
                add_local_root(&mut state.local_roots, name.clone(), variable);
            }
        }
        for expression in &state.seed.expressions {
            if !matches!(expression.kind, OwnerConstraintNodeKind::Block) {
                continue;
            }
            for input in &expression.inputs {
                let OwnerConstraintEdgeRole::BlockBinding { name } = &input.role else {
                    continue;
                };
                if let Some(variable) = expression_variable(state, input.expression) {
                    add_local_root(&mut state.local_roots, name.clone(), variable);
                }
            }
        }
    }

    let mut calls = Vec::new();
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
                    mode = Some(FlowMode::PresentOrAbsent);
                    state.effect.emits_source = true;
                }
                OwnerConstraintNodeKind::Reference { parts }
                | OwnerConstraintNodeKind::Drain { parts } => {
                    if let Some(target) = resolved.get(&expression.expression) {
                        if target.reference.kind == OwnerReferenceKind::Value {
                            // Cross-owner value reads are wired after all local
                            // interfaces exist.
                        }
                    } else if let Some((root, projection)) = parts.split_first() {
                        let local = if root == "PASSED" {
                            Some(state.context)
                        } else {
                            state.local_roots.get(root).copied().flatten()
                        };
                        if let Some(local) = local {
                            let projected = bind_projection(&mut unifier, local, projection);
                            unifier.unify(Type::Var(variable), Type::Var(projected));
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
                        inputs: expression
                            .inputs
                            .iter()
                            .map(|input| (input.role.clone(), input.expression))
                            .collect(),
                        stable_expression: expression.expression.clone(),
                        flush: unifier.fresh(),
                    });
                    mode = None;
                }
                OwnerConstraintNodeKind::Draining => {
                    if let Some(input) = expression
                        .inputs
                        .first()
                        .and_then(|input| expression_variable(state, input.expression))
                    {
                        unifier.unify(Type::Var(variable), Type::Var(input));
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
                    for input in &expression.inputs {
                        if let Some(input) = expression_variable(state, input.expression) {
                            unifier.unify(Type::Var(variable), Type::Var(input));
                        }
                    }
                    state.effect.reads_state = true;
                    state.effect.writes_state = true;
                    mode = None;
                }
                OwnerConstraintNodeKind::When => {
                    for input in expression
                        .inputs
                        .iter()
                        .filter(|input| matches!(input.role, OwnerConstraintEdgeRole::WhenArm))
                    {
                        if let Some(input) = expression_variable(state, input.expression) {
                            unifier.unify(Type::Var(variable), Type::Var(input));
                        }
                    }
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
                        unifier.unify(Type::Var(variable), Type::Var(input));
                    }
                    mode = Some(FlowMode::PresentOrAbsent);
                }
                OwnerConstraintNodeKind::Infix { operation } => {
                    for input in &expression.inputs {
                        if let Some(input) = expression_variable(state, input.expression) {
                            unifier.bind_var(input, Type::Number);
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
                            unifier.unify(Type::Var(variable), Type::Var(output));
                        }
                        mode = None;
                    } else {
                        unifier.bind_var(variable, Type::Absent);
                        mode = Some(FlowMode::Absent);
                    }
                    let _ = pattern_type(pattern, &mut unifier);
                }
                OwnerConstraintNodeKind::Block => {
                    if let Some(result) = expression
                        .inputs
                        .iter()
                        .find(|input| matches!(input.role, OwnerConstraintEdgeRole::BlockResult))
                    {
                        if let Some(result) = expression_variable(state, result.expression) {
                            unifier.unify(Type::Var(variable), Type::Var(result));
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
                                unifier.unify(Type::Var(item), Type::Var(input));
                            }
                        }
                        unifier.bind_var(variable, Type::List(Type::shared(Type::Var(item))));
                    }
                    OwnerCollectionKind::Set => {
                        let item = unifier.fresh();
                        for input in &expression.inputs {
                            if let Some(input) = expression_variable(state, input.expression) {
                                unifier.unify(Type::Var(item), Type::Var(input));
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
                            unifier.unify(Type::Var(variable), Type::Var(output));
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
    for state in states.values() {
        for ((external, variable), flush_variable) in state
            .seed
            .external_expressions
            .iter()
            .zip(&state.external_expressions)
            .zip(&state.external_expression_flushes)
        {
            if let Some((result, result_flush)) = internal_results.get(&external.owner) {
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
            let Some(expression) = state
                .expression_by_key
                .get(&resolved.reference.expression)
                .and_then(|expression| state.expressions.get(*expression))
                .copied()
            else {
                continue;
            };
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
            candidates.extend(
                calls
                    .iter()
                    .filter(|call| call.caller == state.seed.owner && call.expression == index)
                    .map(|call| Type::Var(call.flush)),
            );
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
    let mut previous = solver_surface_snapshot(&mut unifier, &states);
    let maximum_rounds = states.len().saturating_add(calls.len()).saturating_add(2);
    let mut converged = calls.is_empty();
    for _round in 0..maximum_rounds {
        for (call_index, call) in calls.iter().enumerate() {
            let caller = states.get(&call.caller).ok_or_else(|| {
                OwnerConstraintSeedError::new("interface call has no caller state")
            })?;
            let variables = &mut call_variables[call_index];
            let context_variables = &mut call_context_variables[call_index];
            let (parameters, result, result_flush, result_mode, context, effect, signature_found) =
                if let Some(target) = &call.target {
                    if let Some(callee) = states.get(target) {
                        let parameters = callee
                            .parameters
                            .iter()
                            .map(|parameter| InstantiatedInterfaceParameter {
                                name: parameter.name.clone(),
                                kind: parameter.kind,
                                ordinal: parameter.ordinal,
                                ty: instantiate_type(
                                    &unifier.resolve(&Type::Var(parameter.variable)),
                                    &mut unifier,
                                    &mut *variables,
                                ),
                                requirement: CheckedParameterRequirement::Required,
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
                                resolved_expression_boundary(
                                    callee,
                                    expression,
                                    &mut unifier,
                                    raw_mode,
                                )
                                .mode
                            })
                            .unwrap_or(FlowMode::Continuous);
                        (
                            parameters,
                            result,
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
                                kind: parameter.kind,
                                ordinal: parameter.ordinal,
                                ty: instantiate_type(
                                    &parameter.flow_type.ty,
                                    &mut unifier,
                                    &mut *variables,
                                ),
                                requirement: parameter.requirement.clone(),
                                evaluation_scope: parameter.evaluation_scope,
                            })
                            .collect();
                        let result =
                            instantiate_type(&callee.result.ty, &mut unifier, &mut *variables);
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
                            result,
                            result_flush,
                            callee.result.mode,
                            context,
                            callee.effect,
                            true,
                        )
                    } else {
                        (
                            Vec::new(),
                            Type::Unknown,
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
                            kind: match parameter.kind {
                                CheckedParameterKind::Value => OwnerParameterKind::Value,
                                CheckedParameterKind::Out => OwnerParameterKind::Out,
                            },
                            ordinal: ordinal as u32,
                            ty: instantiate_type(
                                &parameter.flow_type.ty,
                                &mut unifier,
                                &mut *variables,
                            ),
                            requirement: parameter.requirement.clone(),
                            evaluation_scope: match parameter.evaluation_scope {
                                crate::OwnerAbiEvaluationScope::Parent => {
                                    OwnerInterfaceEvaluationScope::Parent
                                }
                                crate::OwnerAbiEvaluationScope::Output { parameter_ordinal } => {
                                    OwnerInterfaceEvaluationScope::Output { parameter_ordinal }
                                }
                            },
                        })
                        .collect();
                    let result =
                        instantiate_type(&signature.result.ty, &mut unifier, &mut *variables);
                    (
                        parameters,
                        result,
                        Type::Absent,
                        signature.result.mode,
                        None,
                        signature.effect,
                        true,
                    )
                } else {
                    (
                        Vec::new(),
                        Type::Unknown,
                        Type::Absent,
                        FlowMode::Continuous,
                        None,
                        CheckedEffectSummary::default(),
                        false,
                    )
                };
            let call_valid = signature_found
                && interface_call_shape_is_valid(call, &parameters, call.target.is_none());

            let mut evaluation_scope_updates = Vec::new();
            for parameter in parameters.iter().filter(|_| call_valid) {
                let OwnerInterfaceEvaluationScope::Output {
                    parameter_ordinal: output_ordinal,
                } = parameter.evaluation_scope
                else {
                    continue;
                };
                let Some(owner_output_ordinal) =
                    forwarded_owner_output_ordinal(caller, call, &parameters, output_ordinal)
                else {
                    continue;
                };
                let Some(actual) = call_input_for_parameter(call, parameter, &parameters) else {
                    continue;
                };
                for owner_parameter_ordinal in referenced_owner_parameter_ordinals(caller, actual) {
                    if caller.parameters.iter().any(|owner_parameter| {
                        owner_parameter.ordinal == owner_parameter_ordinal
                            && owner_parameter.kind == OwnerParameterKind::Value
                    }) {
                        evaluation_scope_updates.push((
                            owner_parameter_ordinal,
                            OwnerInterfaceEvaluationScope::Output {
                                parameter_ordinal: owner_output_ordinal,
                            },
                        ));
                    }
                }
            }

            let call_variable = caller.expressions[call.expression];
            let has_explicit_pass = call.inputs.iter().any(|(role, _)| {
                matches!(
                    role,
                    OwnerConstraintEdgeRole::CallPass { .. }
                        | OwnerConstraintEdgeRole::PipePass { .. }
                )
            });
            if call_valid
                && !has_explicit_pass
                && let Some(context) = &context
            {
                unifier.unify(Type::Var(caller.context), context.clone());
            }
            if call_valid && let Some(field) = call.function.strip_prefix("Field/") {
                if let Some((_, input)) = call
                    .inputs
                    .iter()
                    .find(|(role, _)| matches!(role, OwnerConstraintEdgeRole::PipeInput))
                    && let Some(input) = expression_variable(caller, *input)
                {
                    let projected = bind_projection(&mut unifier, input, &[field.to_owned()]);
                    unifier.unify(Type::Var(call_variable), Type::Var(projected));
                }
            } else {
                unifier.bind_var(call_variable, result);
            }
            for (role, input) in call.inputs.iter().filter(|_| call_valid) {
                let Some(input) = expression_variable(caller, *input) else {
                    continue;
                };
                match role {
                    OwnerConstraintEdgeRole::PipeInput => {
                        if let Some(expected) = parameters
                            .iter()
                            .find(|parameter| parameter.kind == OwnerParameterKind::Value)
                        {
                            unifier.unify(Type::Var(input), expected.ty.clone());
                        }
                    }
                    OwnerConstraintEdgeRole::CallArgument { kind, name, .. }
                    | OwnerConstraintEdgeRole::PipeArgument { kind, name, .. } => {
                        if let Some(parameter) =
                            parameters.iter().find(|parameter| parameter.name == *name)
                        {
                            match (&parameter.kind, kind) {
                                (OwnerParameterKind::Value, OwnerArgumentKind::Named)
                                | (OwnerParameterKind::Out, OwnerArgumentKind::Named) => {
                                    unifier.unify(Type::Var(input), parameter.ty.clone());
                                }
                                (OwnerParameterKind::Out, OwnerArgumentKind::BareBinding)
                                | (OwnerParameterKind::Value, OwnerArgumentKind::BareBinding) => {}
                            }
                        }
                    }
                    OwnerConstraintEdgeRole::CallPass { .. }
                    | OwnerConstraintEdgeRole::PipePass { .. } => {
                        if let Some(context) = &context {
                            unifier.unify(Type::Var(input), context.clone());
                        }
                    }
                    _ => {}
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
                if call_valid {
                    state.effect = merge_effects(state.effect, effect);
                }
                state.modes[call.expression] =
                    flow_mode_join(state.modes[call.expression], Some(result_mode));
                for (ordinal, incoming) in evaluation_scope_updates {
                    let parameter = state
                        .parameters
                        .iter_mut()
                        .find(|parameter| parameter.ordinal == ordinal)
                        .ok_or_else(|| {
                            OwnerConstraintSeedError::new(format!(
                                "owner interface {:?} has no parameter ordinal {ordinal}",
                                state.seed.owner
                            ))
                        })?;
                    match (parameter.evaluation_scope, incoming) {
                        (OwnerInterfaceEvaluationScope::Parent, incoming) => {
                            parameter.evaluation_scope = incoming;
                        }
                        (current, incoming) if current == incoming => {}
                        (current, incoming) => {
                            return Err(OwnerConstraintSeedError::new(format!(
                                "owner interface {:?} parameter `{}` requires incompatible evaluation scopes {current:?} and {incoming:?}",
                                state.seed.owner, parameter.name
                            )));
                        }
                    }
                }
            }
            let _ = call.stable_expression;
            work.cross_owner_constraints = work.cross_owner_constraints.saturating_add(1);
        }
        let current = solver_surface_snapshot(&mut unifier, &states);
        if current == previous {
            converged = true;
            break;
        }
        previous = current;
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
        let context = (!matches!(context_ty, Type::Var(_) | Type::Unknown)).then(|| {
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
        if let OwnerResultTransfer::Expression { nodes, .. } = &result_transfer {
            for node in nodes {
                collect_type_variables(&node.flow_type.ty, &mut type_variables);
            }
        }
        interfaces.push(OwnerPublicInterface {
            owner: owner.clone(),
            declaration_kind: state.declaration_kind,
            names: state.names.clone(),
            parameters: parameters.into_boxed_slice(),
            result,
            result_flush_type,
            result_transfer,
            context,
            effect: state.effect,
            type_variables: type_variables
                .into_iter()
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        });
    }
    work.unification_steps = unifier.steps;
    let fingerprint_v1 = boon_contract::canonical_serde_hash_v1(
        OWNER_INTERFACE_SCC_RESULT_DOMAIN_V1,
        &(&scc.key, &interfaces, next_alpha),
    )
    .map_err(|error| {
        OwnerConstraintSeedError::new(format!(
            "cannot fingerprint owner interface SCC result: {error}"
        ))
    })?;
    Ok(OwnerInterfaceSccResult {
        key: scc.key.clone(),
        owners: interfaces.into_boxed_slice(),
        type_variable_count: next_alpha,
        work,
        fingerprint_v1,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ResolvedOwnerSymbolReference, build_owner_interface_topology,
        project_owner_constraint_seed, project_owner_syntax_input, resolve_owner_constraint_seed,
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

    fn test_abi() -> OwnerCallableAbiEnvironment {
        let unit = link("value: 1\n");
        let project =
            ProjectSyntaxSnapshot::from_unit_snapshots("app/RUN.bn", vec![Arc::new(unit)]).unwrap();
        crate::project_owner_abi_environment(
            &project,
            &boon_checked::ExternalTypeEnvironment::empty(boon_checked::ProgramRole::Client),
        )
        .unwrap()
        .callable_environment()
        .unwrap()
    }

    fn solve(
        seeds: &[OwnerConstraintSeed],
        summaries: &[OwnerConstraintSummary],
    ) -> Vec<OwnerInterfaceSccResult> {
        let abi = test_abi();
        let topology = build_owner_interface_topology(summaries.iter()).unwrap();
        let seeds = seeds
            .iter()
            .map(|seed| (seed.owner.clone(), seed))
            .collect::<BTreeMap<_, _>>();
        let summaries = summaries
            .iter()
            .map(|summary| (summary.owner.clone(), summary))
            .collect::<BTreeMap<_, _>>();
        let mut results = BTreeMap::new();
        for scc in &topology.sccs {
            let dependencies = scc
                .dependencies
                .iter()
                .map(|dependency| results.get(dependency).unwrap())
                .collect::<Vec<_>>();
            let result = solve_owner_interface_scc(
                scc,
                &abi,
                scc.key.members.iter().map(|owner| seeds[owner]),
                scc.key.members.iter().map(|owner| summaries[owner]),
                dependencies,
            )
            .unwrap();
            results.insert(scc.key.clone(), result);
        }
        topology
            .sccs
            .iter()
            .map(|scc| results.remove(&scc.key).unwrap())
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
        let parsed = boon_parser::parse_project(
            "app/RUN.bn",
            [("app/RUN.bn".to_owned(), source.to_owned())],
        )
        .unwrap();
        let checked = crate::check_program(&parsed);
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
        let interface = results[0].owner(&owner).unwrap();
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
    fn identity_interface_preserves_one_alpha_normalized_type_variable() {
        let unit = link("FUNCTION identity(input) {\n    input\n}\n");
        let owner = owner_named(&unit, "identity");
        let seed = seed(&unit, &owner);
        let summary = resolve_owner_constraint_seed(&seed, []).unwrap();
        let results = solve(&[seed], &[summary]);
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
    fn interface_scc_fixed_point_is_independent_of_owner_traversal_order() {
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
        assert_eq!(results.len(), 1);
        let alpha_interface = results[0].owner(&alpha).unwrap();
        let zed_interface = results[0].owner(&zed).unwrap();
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
