use crate::execution::{
    SemanticBlockBinding, SemanticCall, SemanticCallArgument, SemanticCallContextBinding,
    SemanticCallContextId, SemanticCallEntry, SemanticCallId, SemanticCallParameterBinding,
    SemanticCallParameterBindingKind, SemanticCallable, SemanticCallableContext,
    SemanticCallableId, SemanticCallableKind, SemanticCallableParameter,
    SemanticContextualMaterialization, SemanticContextualOperationKind, SemanticContextualOrderKey,
    SemanticExecutionGraphV1, SemanticExprId, SemanticExpression, SemanticExpressionKind,
    SemanticExpressionOrigin, SemanticFunction, SemanticFunctionParameter, SemanticLocalBindingId,
    SemanticMaterializationId, SemanticMaterializationLocalId, SemanticMaterializationResultKind,
    SemanticParameterId, SemanticPatternBinding, SemanticRecordField, SemanticRoot, SemanticScope,
    SemanticScopeId, SemanticSelectArm, SemanticSelectKind, SemanticSourceDef, SemanticSourceId,
    SemanticSourceOrigin, SemanticSourceRead, SemanticStateDef, SemanticStateId, SemanticStatement,
    SemanticStatementId, SemanticStatementKind, SemanticStatementOrigin, SemanticStaticOwner,
    SemanticTextSegment, SemanticValueId, SemanticValueMember, SemanticValueOrigin,
    SemanticValueProvenance, checked_semantic_root_specs_v1, derive_semantic_state_lifetime_v1,
};
use crate::{
    OutCallInstanceId, OutInputValue, OutNetId, ResolvedOutGraph as OutNet, ScopedCheckedExpr,
    StaticOwnerId,
};
use boon_typecheck::{
    CheckedCallEntry, CheckedCallId, CheckedCallableKind, CheckedContextBinding,
    CheckedContextualOperation, CheckedDeclarationKind, CheckedExprId, CheckedExpression,
    CheckedExpressionKind, CheckedParameterKind, CheckedParameterRequirement, CheckedPassedAccess,
    CheckedProgram, CheckedResourceBinding, CheckedTextSegment, CheckedValueUse, ContextFormalId,
    DeclId, FlowMode, FlowType, Type, apply_checked_type_substitutions, is_renderable_type,
};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

fn semantic_parameter_id(callable: SemanticCallableId, ordinal: usize) -> SemanticParameterId {
    SemanticParameterId { callable, ordinal }
}

fn provisional_semantic_source_id(id: boon_typecheck::CheckedSourceId) -> SemanticSourceId {
    SemanticSourceId(id.0 as usize)
}

fn provisional_semantic_state_id(id: boon_typecheck::CheckedStateId) -> SemanticStateId {
    SemanticStateId(id.0 as usize)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ExpansionError {
    MissingExpression(CheckedExprId),
    MissingCall(CheckedCallId),
    MissingCallInstance {
        call: CheckedCallId,
        frame: Option<OutCallInstanceId>,
    },
    MissingCallable(DeclId),
    MissingDeclaration(DeclId),
    MissingSourceDeclaration(CheckedExprId),
    MissingStateDeclaration(CheckedExprId),
    MissingStateInitializer(CheckedExprId),
    MissingFormal {
        callable: DeclId,
        formal: DeclId,
    },
    MissingFunctionResult(DeclId),
    MissingProducerOwner([u8; 32]),
    MissingPassedContext(CheckedExprId),
    MismatchedPassedFormal {
        expression: CheckedExprId,
        expected: ContextFormalId,
        found: ContextFormalId,
    },
    InvalidPassedDrainTarget(CheckedExprId),
    UnboundOutput {
        expression: CheckedExprId,
        target: DeclId,
        net: OutNetId,
    },
    PassOnNonexpandedCall(CheckedCallId),
    ExpressionCycle {
        expression: CheckedExprId,
        frame: Option<OutCallInstanceId>,
        chain: Vec<String>,
    },
    InvalidCheckedExpression {
        expression: CheckedExprId,
        tokens: Vec<String>,
    },
    MissingOperationInput {
        call: CheckedCallId,
        formal: DeclId,
    },
    InvalidMaterializationSourceType {
        call: CheckedCallId,
        function: String,
        found: Type,
    },
    MissingOwnerScope(OutNetId),
    MissingMaterialization(CheckedCallId),
    AmbiguousMaterialization(CheckedCallId),
    MissingOrderKeyMaterialization {
        call: CheckedCallId,
        call_path: Vec<CheckedCallId>,
    },
    AmbiguousOrderKeyMaterialization {
        call: CheckedCallId,
        call_path: Vec<CheckedCallId>,
    },
    UnresolvedAmbientRead {
        expression: CheckedExprId,
        path: String,
    },
    InvalidLocalBindings(String),
    DeferredExpansion {
        expression: ScopedCheckedExpr,
        inherited_owner: Option<StaticOwnerId>,
    },
}

impl fmt::Display for ExpansionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingExpression(expression) => {
                write!(formatter, "checked expression {} is missing", expression.0)
            }
            Self::MissingCall(call) => write!(formatter, "checked call {} is missing", call.0),
            Self::MissingCallInstance { call, frame } => write!(
                formatter,
                "checked call {} has no concrete instance in frame {:?}",
                call.0, frame
            ),
            Self::MissingCallable(callable) => {
                write!(formatter, "checked callable {} is missing", callable.0)
            }
            Self::MissingDeclaration(declaration) => {
                write!(
                    formatter,
                    "checked declaration {} is missing",
                    declaration.0
                )
            }
            Self::MissingSourceDeclaration(expression) => write!(
                formatter,
                "SOURCE expression {} has no checked declaration",
                expression.0
            ),
            Self::MissingStateDeclaration(expression) => write!(
                formatter,
                "anonymous line-based state is unsupported: HOLD expression {} has no checked declaration",
                expression.0
            ),
            Self::MissingStateInitializer(expression) => write!(
                formatter,
                "state expression {} has no exact initial value",
                expression.0
            ),
            Self::MissingFormal { callable, formal } => write!(
                formatter,
                "checked callable {} has no ordinary formal {}",
                callable.0, formal.0
            ),
            Self::MissingFunctionResult(callable) => write!(
                formatter,
                "contextual callable {} has no canonical result expression",
                callable.0
            ),
            Self::MissingProducerOwner(identity) => write!(
                formatter,
                "producer root {} has no static owner",
                identity
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect::<String>()
            ),
            Self::MissingPassedContext(expression) => write!(
                formatter,
                "checked PASSED expression {} has no concrete PASS context",
                expression.0
            ),
            Self::MismatchedPassedFormal {
                expression,
                expected,
                found,
            } => write!(
                formatter,
                "checked PASSED expression {} names context formal {} but its concrete call frame binds formal {}",
                expression.0, found.0, expected.0
            ),
            Self::InvalidPassedDrainTarget(expression) => write!(
                formatter,
                "checked DRAIN PASSED expression {} does not resolve to a canonical drainable binding",
                expression.0
            ),
            Self::UnboundOutput {
                expression,
                target,
                net,
            } => write!(
                formatter,
                "checked expression {} reads OUT declaration {} from net {} without a materialization local",
                expression.0, target.0, net
            ),
            Self::PassOnNonexpandedCall(call) => write!(
                formatter,
                "call {} retained PASS after contextual expansion",
                call.0
            ),
            Self::ExpressionCycle {
                expression,
                frame,
                chain,
            } => write!(
                formatter,
                "contextual expression {} recursively expands in frame {:?}: {}",
                expression.0,
                frame,
                chain.join(" -> ")
            ),
            Self::InvalidCheckedExpression { expression, tokens } => write!(
                formatter,
                "invalid checked expression {} reached contextual expansion: {}",
                expression.0,
                tokens.join(" ")
            ),
            Self::MissingOperationInput { call, formal } => write!(
                formatter,
                "contextual call {} has no concrete input for formal {}",
                call.0, formal.0
            ),
            Self::InvalidMaterializationSourceType {
                call,
                function,
                found,
            } => write!(
                formatter,
                "contextual call {} (`{function}`) requires an exactly typed list source, found {found:?}",
                call.0,
            ),
            Self::MissingOwnerScope(net) => {
                write!(formatter, "OUT net {} has no fresh owner scope", net)
            }
            Self::MissingMaterialization(call) => {
                write!(
                    formatter,
                    "contextual call {} has no erased materialization",
                    call.0
                )
            }
            Self::AmbiguousMaterialization(call) => write!(
                formatter,
                "contextual call {} resolves to more than one erased materialization",
                call.0
            ),
            Self::MissingOrderKeyMaterialization { call, call_path } => write!(
                formatter,
                "contextual order call {} cannot resolve inherited checked call path {:?}",
                call.0,
                call_path.iter().map(|call| call.0).collect::<Vec<_>>()
            ),
            Self::AmbiguousOrderKeyMaterialization { call, call_path } => write!(
                formatter,
                "contextual order call {} resolves inherited checked call path {:?} to multiple materializations",
                call.0,
                call_path.iter().map(|call| call.0).collect::<Vec<_>>()
            ),
            Self::UnresolvedAmbientRead { expression, path } => write!(
                formatter,
                "function expression {} cannot resolve ambient read `{path}` without a concrete call frame",
                expression.0
            ),
            Self::InvalidLocalBindings(message) => {
                write!(formatter, "invalid executable local bindings: {message}")
            }
            Self::DeferredExpansion { .. } => {
                formatter.write_str("internal deferred semantic expansion escaped its work stack")
            }
        }
    }
}

struct CheckedProgramLookup {
    expressions_by_id: BTreeMap<CheckedExprId, Option<usize>>,
    declarations_by_id: BTreeMap<DeclId, Option<usize>>,
    statements_by_id: BTreeMap<boon_typecheck::CheckedStatementId, Option<usize>>,
    scopes_by_id: BTreeMap<boon_typecheck::LexicalScopeId, Option<usize>>,
    calls_by_id: BTreeMap<CheckedCallId, Option<usize>>,
    callables_by_declaration: BTreeMap<DeclId, Option<usize>>,
    declarations_by_scope_and_name:
        BTreeMap<boon_typecheck::LexicalScopeId, BTreeMap<String, Option<DeclId>>>,
    pattern_bindings_by_declaration: BTreeMap<DeclId, Option<usize>>,
    statements_by_value: BTreeMap<CheckedExprId, Vec<usize>>,
    element_contexts_by_declaration: BTreeMap<DeclId, Option<(usize, usize)>>,
}

impl CheckedProgramLookup {
    fn new(program: &CheckedProgram) -> Self {
        let mut expressions_by_id = BTreeMap::new();
        for (index, expression) in program.expressions.iter().enumerate() {
            expressions_by_id
                .entry(expression.id)
                .and_modify(|entry| *entry = None)
                .or_insert(Some(index));
        }
        let mut declarations_by_id = BTreeMap::new();
        for (index, declaration) in program.declarations.iter().enumerate() {
            declarations_by_id
                .entry(declaration.id)
                .and_modify(|entry| *entry = None)
                .or_insert(Some(index));
        }
        let mut statements_by_id = BTreeMap::new();
        for (index, statement) in program.statements.iter().enumerate() {
            statements_by_id
                .entry(statement.id)
                .and_modify(|entry| *entry = None)
                .or_insert(Some(index));
        }
        let mut scopes_by_id = BTreeMap::new();
        for (index, scope) in program.scopes.iter().enumerate() {
            scopes_by_id
                .entry(scope.id)
                .and_modify(|entry| *entry = None)
                .or_insert(Some(index));
        }
        let mut calls_by_id = BTreeMap::new();
        for (index, call) in program.calls.iter().enumerate() {
            calls_by_id
                .entry(call.id)
                .and_modify(|entry| *entry = None)
                .or_insert(Some(index));
        }
        let mut callables_by_declaration = BTreeMap::new();
        for (index, callable) in program.callables.iter().enumerate() {
            callables_by_declaration
                .entry(callable.decl_id)
                .and_modify(|entry| *entry = None)
                .or_insert(Some(index));
        }
        let mut declarations_by_scope_and_name = BTreeMap::new();
        for declaration in &program.declarations {
            declarations_by_scope_and_name
                .entry(declaration.scope_id)
                .or_insert_with(BTreeMap::new)
                .entry(declaration.name.clone())
                .and_modify(|entry| *entry = None)
                .or_insert(Some(declaration.id));
        }
        let mut pattern_bindings_by_declaration = BTreeMap::new();
        for (index, binding) in program.pattern_bindings.iter().enumerate() {
            pattern_bindings_by_declaration
                .entry(binding.declaration)
                .and_modify(|entry| *entry = None)
                .or_insert(Some(index));
        }
        let mut statements_by_value = BTreeMap::<CheckedExprId, Vec<usize>>::new();
        for (index, statement) in program.statements.iter().enumerate() {
            if let Some(value) = statement.value {
                statements_by_value.entry(value).or_default().push(index);
            }
        }
        let mut element_contexts_by_declaration = BTreeMap::new();
        for (call_index, call) in program.calls.iter().enumerate() {
            for (context_index, context) in call.contexts.iter().enumerate() {
                element_contexts_by_declaration
                    .entry(context.declaration)
                    .and_modify(|entry| *entry = None)
                    .or_insert(Some((call_index, context_index)));
            }
        }
        Self {
            expressions_by_id,
            declarations_by_id,
            statements_by_id,
            scopes_by_id,
            calls_by_id,
            callables_by_declaration,
            declarations_by_scope_and_name,
            pattern_bindings_by_declaration,
            statements_by_value,
            element_contexts_by_declaration,
        }
    }

    fn expression<'a>(
        &self,
        program: &'a CheckedProgram,
        expression: CheckedExprId,
    ) -> Option<&'a CheckedExpression> {
        self.expressions_by_id
            .get(&expression)
            .copied()
            .flatten()
            .and_then(|index| program.expressions.get(index))
            .filter(|candidate| candidate.id == expression)
    }

    fn declaration<'a>(
        &self,
        program: &'a CheckedProgram,
        declaration: DeclId,
    ) -> Option<&'a boon_typecheck::CheckedDeclaration> {
        self.declarations_by_id
            .get(&declaration)
            .copied()
            .flatten()
            .and_then(|index| program.declarations.get(index))
            .filter(|candidate| candidate.id == declaration)
    }

    fn statement<'a>(
        &self,
        program: &'a CheckedProgram,
        statement: boon_typecheck::CheckedStatementId,
    ) -> Option<&'a boon_typecheck::CheckedStatement> {
        self.statements_by_id
            .get(&statement)
            .copied()
            .flatten()
            .and_then(|index| program.statements.get(index))
            .filter(|candidate| candidate.id == statement)
    }

    fn scope<'a>(
        &self,
        program: &'a CheckedProgram,
        scope: boon_typecheck::LexicalScopeId,
    ) -> Option<&'a boon_typecheck::CheckedScope> {
        self.scopes_by_id
            .get(&scope)
            .copied()
            .flatten()
            .and_then(|index| program.scopes.get(index))
            .filter(|candidate| candidate.id == scope)
    }

    fn call<'a>(
        &self,
        program: &'a CheckedProgram,
        call: CheckedCallId,
    ) -> Option<&'a boon_typecheck::CheckedCall> {
        self.calls_by_id
            .get(&call)
            .copied()
            .flatten()
            .and_then(|index| program.calls.get(index))
            .filter(|candidate| candidate.id == call)
    }

    fn callable<'a>(
        &self,
        program: &'a CheckedProgram,
        declaration: DeclId,
    ) -> Option<&'a boon_typecheck::CheckedCallableSignature> {
        self.callables_by_declaration
            .get(&declaration)
            .copied()
            .flatten()
            .and_then(|index| program.callables.get(index))
            .filter(|callable| callable.decl_id == declaration)
    }

    fn declaration_in_exact_scope(
        &self,
        scope: boon_typecheck::LexicalScopeId,
        name: &str,
    ) -> Option<DeclId> {
        self.declarations_by_scope_and_name
            .get(&scope)?
            .get(name)
            .copied()
            .flatten()
    }

    fn pattern_binding<'a>(
        &self,
        program: &'a CheckedProgram,
        declaration: DeclId,
    ) -> Option<&'a boon_typecheck::CheckedPatternBinding> {
        self.pattern_bindings_by_declaration
            .get(&declaration)
            .copied()
            .flatten()
            .and_then(|index| program.pattern_bindings.get(index))
            .filter(|binding| binding.declaration == declaration)
    }

    fn statement_indices_for_value(&self, value: CheckedExprId) -> &[usize] {
        self.statements_by_value
            .get(&value)
            .map(Vec::as_slice)
            .unwrap_or_default()
    }

    fn element_context<'a>(
        &self,
        program: &'a CheckedProgram,
        declaration: DeclId,
    ) -> Option<(
        &'a boon_typecheck::CheckedCall,
        &'a boon_typecheck::CheckedCallContext,
    )> {
        let (call, context) = self
            .element_contexts_by_declaration
            .get(&declaration)
            .copied()
            .flatten()?;
        let call = program.calls.get(call)?;
        let context = call.contexts.get(context)?;
        (context.declaration == declaration).then_some((call, context))
    }
}

#[derive(Clone)]
struct ContextualCandidate {
    call: CheckedCallId,
    instance: OutCallInstanceId,
    checked_expression: CheckedExprId,
    function: String,
    owner: StaticOwnerId,
    net: OutNetId,
    operation: SemanticContextualOperationKind,
    evaluation_owner: Option<StaticOwnerId>,
    source: ScopedCheckedExpr,
    body: ScopedCheckedExpr,
    direction: Option<ScopedCheckedExpr>,
    result_type: Type,
}

#[derive(Default)]
pub(crate) struct SemanticExpressionArena {
    expressions: Vec<SemanticExpression>,
    checked_expression_origins: Vec<SemanticExpressionOrigin>,
}

impl SemanticExpressionArena {
    fn push(
        &mut self,
        checked_expression: CheckedExprId,
        checked_scope: boon_typecheck::LexicalScopeId,
        checked_span: boon_typecheck::CheckedSpan,
        call_instance: Option<OutCallInstanceId>,
        owning_statement: Option<SemanticStatementId>,
        build: impl FnOnce(SemanticExprId, SemanticValueId) -> SemanticExpression,
    ) -> SemanticExprId {
        let id = SemanticExprId(self.expressions.len());
        let value_id = SemanticValueId(id.as_usize());
        self.expressions.push(build(id, value_id));
        self.checked_expression_origins
            .push(SemanticExpressionOrigin {
                expression: id,
                checked_expression,
                checked_scope,
                checked_span,
                owning_statement,
                call_instance,
            });
        id
    }
}

fn call_instance_matches_checked_path(
    out_net: &OutNet,
    mut instance: OutCallInstanceId,
    call_path: &[CheckedCallId],
) -> bool {
    if call_path.is_empty() {
        return false;
    }
    for (position, call) in call_path.iter().rev().enumerate() {
        let Some(current) = out_net.call_instances.get(instance.as_usize()) else {
            return false;
        };
        if current.provenance.call_id != Some(*call) {
            return false;
        }
        if position + 1 < call_path.len() {
            let Some(parent) = current.parent else {
                return false;
            };
            instance = parent;
        }
    }
    true
}

fn order_key_candidate_index(
    candidates: &[ContextualCandidate],
    out_net: &OutNet,
    call: CheckedCallId,
    call_path: &[CheckedCallId],
) -> Result<usize, ExpansionError> {
    let mut matching = candidates.iter().enumerate().filter(|candidate| {
        call_instance_matches_checked_path(out_net, candidate.1.instance, call_path)
    });
    let Some((index, _)) = matching.next() else {
        return Err(ExpansionError::MissingOrderKeyMaterialization {
            call,
            call_path: call_path.to_vec(),
        });
    };
    if matching.next().is_some() {
        return Err(ExpansionError::AmbiguousOrderKeyMaterialization {
            call,
            call_path: call_path.to_vec(),
        });
    }
    Ok(index)
}

fn push_default_order_direction(
    arena: &mut SemanticExpressionArena,
    checked_expression: &CheckedExpression,
    owner: Option<StaticOwnerId>,
    call_instance: Option<OutCallInstanceId>,
) -> SemanticExprId {
    arena.push(
        checked_expression.id,
        checked_expression.scope_id,
        checked_expression.span,
        call_instance,
        None,
        |id, value_id| SemanticExpression {
            id,
            value_id,
            checked_expr_id: checked_expression.id,
            flow_type: boon_typecheck::FlowType {
                mode: FlowMode::Continuous,
                ty: Type::VariantSet(vec![
                    boon_typecheck::Variant::Tag("Ascending".to_owned()),
                    boon_typecheck::Variant::Tag("Descending".to_owned()),
                ]),
            },
            effect: boon_typecheck::CheckedEffectSummary::default(),
            owner,
            provenance: runtime_value_provenance(),
            resource_binding_path: None,
            kind: SemanticExpressionKind::Tag("Ascending".to_owned()),
        },
    )
}

pub(crate) fn derive_contextual_materializations(
    program: &CheckedProgram,
    out_net: &OutNet,
) -> Result<
    (
        Vec<SemanticContextualMaterialization>,
        SemanticExpressionArena,
    ),
    ExpansionError,
> {
    let lookup = CheckedProgramLookup::new(program);
    let mut candidates = Vec::new();
    for checked_call in &program.calls {
        let callable = lookup
            .callable(program, checked_call.callable)
            .ok_or(ExpansionError::MissingCallable(checked_call.callable))?;
        let Some(operation) = callable.contextual_operation else {
            continue;
        };
        let (operation_kind, list_formal, row_formal, body_formal, direction_formal) =
            contextual_operation_formals(operation);
        for producer in out_net.concrete_producers_for_checked_call(checked_call.id) {
            if out_net.ports[producer.port.as_usize()].formal != row_formal {
                continue;
            }
            let instance = &out_net.call_instances[producer.call.as_usize()];
            let concrete_input = |formal| {
                instance
                    .inputs
                    .iter()
                    .find(|binding| binding.formal == formal)
                    .and_then(|binding| binding.checked_value())
                    .ok_or(ExpansionError::MissingOperationInput {
                        call: checked_call.id,
                        formal,
                    })
            };
            let list_expression = concrete_input(list_formal)?;
            let body_expression = concrete_input(body_formal)?;
            let direction = direction_formal.and_then(|formal| {
                instance
                    .inputs
                    .iter()
                    .find(|binding| binding.formal == formal)
                    .and_then(|binding| binding.checked_value())
            });
            out_net
                .owner_scope_for_net(producer.net)
                .ok_or(ExpansionError::MissingOwnerScope(producer.net))?;
            candidates.push(ContextualCandidate {
                call: checked_call.id,
                instance: producer.call,
                checked_expression: checked_call.expression,
                function: checked_call.function.clone(),
                owner: producer.owner,
                net: producer.net,
                operation: operation_kind,
                evaluation_owner: out_net.owner_for_call_evaluation(producer.call),
                source: list_expression,
                body: body_expression,
                direction,
                result_type: instance.result.ty.clone(),
            });
        }
    }
    candidates.sort_by_key(|candidate| candidate.owner);
    let inherited_order_candidates = candidates
        .iter()
        .enumerate()
        .map(|(candidate_index, candidate)| {
            if !matches!(
                candidate.operation,
                SemanticContextualOperationKind::SortBy | SemanticContextualOperationKind::ThenBy
            ) {
                return Ok(Vec::new());
            }
            let chain = program
                .order_chains
                .iter()
                .filter(|entry| {
                    entry.chain.keys.last().is_some_and(|key| {
                        call_instance_matches_checked_path(
                            out_net,
                            candidate.instance,
                            &key.call_path,
                        )
                    })
                })
                .max_by_key(|entry| {
                    (
                        entry.chain.keys.len(),
                        entry.chain.keys.last().map_or(0, |key| key.call_path.len()),
                    )
                })
                .ok_or_else(|| ExpansionError::MissingOrderKeyMaterialization {
                    call: candidate.call,
                    call_path: vec![candidate.call],
                })?;
            let resolved = chain
                .chain
                .keys
                .iter()
                .map(|key| {
                    order_key_candidate_index(&candidates, out_net, candidate.call, &key.call_path)
                })
                .collect::<Result<Vec<_>, _>>()?;
            if resolved.last().copied() != Some(candidate_index) {
                return Err(ExpansionError::MissingOrderKeyMaterialization {
                    call: candidate.call,
                    call_path: chain
                        .chain
                        .keys
                        .last()
                        .map_or_else(Vec::new, |key| key.call_path.clone()),
                });
            }
            Ok(resolved[..resolved.len().saturating_sub(1)].to_vec())
        })
        .collect::<Result<Vec<_>, ExpansionError>>()?;
    let materializations_by_owner = candidates
        .iter()
        .enumerate()
        .map(|(id, candidate)| (candidate.owner, SemanticMaterializationId(id)))
        .collect::<BTreeMap<_, _>>();
    let net_by_owner = candidates
        .iter()
        .map(|candidate| (candidate.owner, candidate.net))
        .collect::<BTreeMap<_, _>>();
    let mut materialization_result_types = BTreeMap::new();
    let mut result = Vec::with_capacity(candidates.len());
    let mut arena = SemanticExpressionArena::default();
    let mut item_types_by_owner = BTreeMap::new();
    for (id, candidate) in candidates.iter().cloned().enumerate() {
        let materialization_id = SemanticMaterializationId(id);
        let inherited_candidates = inherited_order_candidates[id]
            .iter()
            .map(|candidate| &candidates[*candidate])
            .collect::<Vec<_>>();
        let mut locals = BTreeMap::new();
        let mut owner = Some(candidate.owner);
        while let Some(current) = owner {
            if let Some(net) = net_by_owner.get(&current).copied() {
                locals.insert(net, (current, SemanticMaterializationLocalId(0)));
            }
            owner = out_net
                .static_owners
                .get(current.as_usize())
                .and_then(|owner| owner.parent);
        }
        for inherited in &inherited_candidates {
            locals.insert(
                inherited.net,
                (candidate.owner, SemanticMaterializationLocalId(0)),
            );
        }
        let mut builder = SemanticExpressionBuilder::new(
            program,
            &lookup,
            out_net,
            locals,
            item_types_by_owner.clone(),
            &materializations_by_owner,
            &materialization_result_types,
        );
        let local_source =
            builder.expand_with_inherited_owner(candidate.source, candidate.evaluation_owner)?;
        let list_type = builder.expressions[local_source.as_usize()]
            .flow_type
            .ty
            .clone();
        let Type::List(item_type) = list_type else {
            return Err(ExpansionError::InvalidMaterializationSourceType {
                call: candidate.call,
                function: candidate.function,
                found: list_type,
            });
        };
        let item_type = *item_type;
        builder.set_local_type(
            candidate.owner,
            SemanticMaterializationLocalId(0),
            item_type.clone(),
        );
        let local_body =
            builder.expand_with_inherited_owner(candidate.body, Some(candidate.owner))?;
        let local_direction = candidate
            .direction
            .map(|direction| {
                builder.expand_with_inherited_owner(direction, candidate.evaluation_owner)
            })
            .transpose()?;
        let local_inherited_order = inherited_candidates
            .iter()
            .map(|inherited| {
                let body =
                    builder.expand_with_inherited_owner(inherited.body, Some(candidate.owner))?;
                let direction = inherited
                    .direction
                    .map(|direction| {
                        builder.expand_with_inherited_owner(direction, inherited.evaluation_owner)
                    })
                    .transpose()?;
                Ok((
                    inherited.operation,
                    body,
                    direction,
                    inherited.checked_expression,
                    inherited.evaluation_owner,
                    inherited.instance,
                ))
            })
            .collect::<Result<Vec<_>, ExpansionError>>()?;
        let body_type = builder.expressions[local_body.as_usize()]
            .flow_type
            .ty
            .clone();
        let result_type = match candidate.operation {
            SemanticContextualOperationKind::Map => Type::List(Box::new(body_type.clone())),
            SemanticContextualOperationKind::Filter
            | SemanticContextualOperationKind::Retain
            | SemanticContextualOperationKind::Remove
            | SemanticContextualOperationKind::SortBy
            | SemanticContextualOperationKind::ThenBy => Type::List(Box::new(item_type.clone())),
            SemanticContextualOperationKind::Every
            | SemanticContextualOperationKind::Any
            | SemanticContextualOperationKind::Find => candidate.result_type,
        };
        let result_type = erase_runtime_type_vars(&result_type);
        let mut expanded = builder.finish();
        let local_direction = match local_direction {
            Some(direction) => Some(direction),
            None if matches!(
                candidate.operation,
                SemanticContextualOperationKind::SortBy | SemanticContextualOperationKind::ThenBy
            ) =>
            {
                let checked_expression = lookup
                    .expression(program, candidate.checked_expression)
                    .ok_or(ExpansionError::MissingExpression(
                    candidate.checked_expression,
                ))?;
                Some(push_default_order_direction(
                    &mut expanded,
                    checked_expression,
                    candidate.evaluation_owner,
                    Some(candidate.instance),
                ))
            }
            None => None,
        };
        let local_inherited_order = local_inherited_order
            .into_iter()
            .map(
                |(operation, body, direction, checked_expression, owner, call_instance)| {
                    let direction = match direction {
                        Some(direction) => direction,
                        None => {
                            let checked_expression = lookup
                                .expression(program, checked_expression)
                                .ok_or(ExpansionError::MissingExpression(checked_expression))?;
                            push_default_order_direction(
                                &mut expanded,
                                checked_expression,
                                owner,
                                Some(call_instance),
                            )
                        }
                    };
                    Ok((operation, body, direction))
                },
            )
            .collect::<Result<Vec<_>, ExpansionError>>()?;
        let offset = arena.expressions.len();
        append_expression_arena_without_roots(&mut arena, expanded);
        let source = rebase_expr_id(local_source, offset);
        let body = rebase_expr_id(local_body, offset);
        let direction = local_direction.map(|direction| rebase_expr_id(direction, offset));
        let inherited_order = local_inherited_order
            .into_iter()
            .map(|(operation, body, direction)| SemanticContextualOrderKey {
                operation,
                body: rebase_expr_id(body, offset),
                direction: rebase_expr_id(direction, offset),
            })
            .collect();
        let result_kind = match &result_type {
            Type::List(item) if is_renderable_type(item) => {
                SemanticMaterializationResultKind::RenderSlot
            }
            _ => SemanticMaterializationResultKind::RuntimeValue,
        };
        result.push(SemanticContextualMaterialization {
            id: materialization_id,
            owner: candidate.owner,
            operation: candidate.operation,
            result_kind,
            source,
            source_row_predecessors: Vec::new(),
            body,
            direction,
            inherited_order,
            row_local: SemanticMaterializationLocalId(0),
            source_list_id: None,
            source_scope_id: None,
            target_list_id: None,
            target_scope_id: None,
            item_type: item_type.clone(),
            result_type: result_type.clone(),
        });
        item_types_by_owner.insert(candidate.owner, item_type);
        materialization_result_types.insert(materialization_id, result_type);
    }
    Ok((result, arena))
}

fn concrete_type_in_frame(out_net: &OutNet, ty: &Type, frame: Option<OutCallInstanceId>) -> Type {
    let ty = frame.map_or_else(
        || ty.clone(),
        |instance| {
            apply_checked_type_substitutions(
                ty,
                &out_net.call_instances[instance.as_usize()].type_substitutions,
            )
        },
    );
    erase_runtime_type_vars(&ty)
}

pub(crate) fn erase_runtime_type_vars(ty: &Type) -> Type {
    match ty {
        Type::Var(_) => Type::Unknown,
        Type::List(item) => Type::List(Box::new(erase_runtime_type_vars(item))),
        Type::Map { key, value } => Type::Map {
            key: Box::new(erase_runtime_type_vars(key)),
            value: Box::new(erase_runtime_type_vars(value)),
        },
        Type::Set(item) => Type::Set(Box::new(erase_runtime_type_vars(item))),
        Type::Function { args, result } => Type::Function {
            args: args.iter().map(erase_runtime_type_vars).collect(),
            result: Box::new(boon_typecheck::FlowType {
                mode: result.mode,
                ty: erase_runtime_type_vars(&result.ty),
            }),
        },
        Type::Object(shape) => Type::Object(boon_typecheck::ObjectShape {
            fields: shape
                .fields
                .iter()
                .map(|(name, ty)| (name.clone(), erase_runtime_type_vars(ty)))
                .collect(),
            field_order: shape.field_order.clone(),
            open: shape.open,
        }),
        Type::VariantSet(variants) => Type::VariantSet(
            variants
                .iter()
                .map(|variant| match variant {
                    boon_typecheck::Variant::Tag(tag) => boon_typecheck::Variant::Tag(tag.clone()),
                    boon_typecheck::Variant::Tagged { tag, fields } => {
                        boon_typecheck::Variant::Tagged {
                            tag: tag.clone(),
                            fields: boon_typecheck::ObjectShape {
                                fields: fields
                                    .fields
                                    .iter()
                                    .map(|(name, ty)| (name.clone(), erase_runtime_type_vars(ty)))
                                    .collect(),
                                field_order: fields.field_order.clone(),
                                open: fields.open,
                            },
                        }
                    }
                })
                .collect(),
        ),
        Type::Union(members) => Type::Union(members.iter().map(erase_runtime_type_vars).collect()),
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

fn project_concrete_type(mut ty: Type, fields: &[String]) -> Option<Type> {
    for field in fields {
        ty = match ty {
            Type::Object(shape) => shape.fields.get(field)?.clone(),
            Type::Union(members) => {
                let projected = members
                    .iter()
                    .filter_map(|member| match member {
                        Type::Object(shape) => shape.fields.get(field).cloned(),
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                match projected.as_slice() {
                    [] => return None,
                    _ => boon_typecheck::canonical_union_type(projected),
                }
            }
            _ => return None,
        };
    }
    Some(ty)
}

fn concrete_record_type(
    expressions: &[SemanticExpression],
    fields: &[SemanticRecordField],
) -> Option<Type> {
    let mut ordered = Vec::new();
    let mut typed = BTreeMap::new();
    for field in fields {
        let value_type = expressions
            .get(field.value.as_usize())?
            .flow_type
            .ty
            .clone();
        if field.spread {
            let Type::Object(shape) = value_type else {
                return None;
            };
            for name in shape.field_order.iter().chain(shape.fields.keys()) {
                if !ordered.contains(name) {
                    ordered.push(name.clone());
                }
            }
            typed.extend(shape.fields);
        } else {
            if !ordered.contains(&field.name) {
                ordered.push(field.name.clone());
            }
            typed.insert(field.name.clone(), value_type);
        }
    }
    Some(Type::Object(boon_typecheck::ObjectShape {
        fields: typed,
        field_order: ordered,
        open: false,
    }))
}

fn concrete_structural_type(
    expressions: &[SemanticExpression],
    kind: &SemanticExpressionKind,
) -> Option<Type> {
    match kind {
        SemanticExpressionKind::Object(fields) => concrete_record_type(expressions, fields),
        SemanticExpressionKind::TaggedObject { tag, fields } => {
            let Type::Object(shape) = concrete_record_type(expressions, fields)? else {
                return None;
            };
            Some(Type::VariantSet(vec![boon_typecheck::Variant::Tagged {
                tag: tag.clone(),
                fields: shape,
            }]))
        }
        SemanticExpressionKind::List { items, .. } if !items.is_empty() => {
            let first = expressions.get(items[0].as_usize())?.flow_type.ty.clone();
            items
                .iter()
                .skip(1)
                .all(|item| {
                    expressions
                        .get(item.as_usize())
                        .is_some_and(|expression| expression.flow_type.ty == first)
                })
                .then(|| Type::List(Box::new(first)))
        }
        SemanticExpressionKind::Map { entries } if !entries.is_empty() => {
            let first = expressions.get(entries[0].as_usize())?;
            let Type::Object(first_shape) = &first.flow_type.ty else {
                return None;
            };
            let key = first_shape.fields.get("key")?.clone();
            let value = first_shape.fields.get("value")?.clone();
            entries
                .iter()
                .skip(1)
                .all(|entry| {
                    expressions.get(entry.as_usize()).is_some_and(|expression| {
                        let Type::Object(shape) = &expression.flow_type.ty else {
                            return false;
                        };
                        shape.fields.get("key") == Some(&key)
                            && shape.fields.get("value") == Some(&value)
                    })
                })
                .then(|| Type::Map {
                    key: Box::new(key),
                    value: Box::new(value),
                })
        }
        SemanticExpressionKind::Set { items } if !items.is_empty() => {
            let first = expressions.get(items[0].as_usize())?.flow_type.ty.clone();
            items
                .iter()
                .skip(1)
                .all(|item| {
                    expressions
                        .get(item.as_usize())
                        .is_some_and(|expression| expression.flow_type.ty == first)
                })
                .then(|| Type::Set(Box::new(first)))
        }
        SemanticExpressionKind::Block { result, .. } => expressions
            .get(result.as_usize())
            .map(|expression| expression.flow_type.ty.clone()),
        _ => None,
    }
}

fn semantic_callable_inventory(
    program: &CheckedProgram,
    semantic_scope_ids: &BTreeMap<boon_typecheck::LexicalScopeId, SemanticScopeId>,
) -> Result<(Vec<SemanticCallable>, BTreeMap<DeclId, SemanticCallableId>), ExpansionError> {
    let mut callable_ids = BTreeMap::new();
    let mut callables = Vec::with_capacity(program.callables.len());
    for (index, callable) in program.callables.iter().enumerate() {
        let id = SemanticCallableId(index);
        if callable_ids.insert(callable.decl_id, id).is_some() {
            return Err(ExpansionError::InvalidLocalBindings(format!(
                "checked callable {} is defined more than once",
                callable.decl_id.0
            )));
        }
        let scope = semantic_scope_ids
            .get(&callable.scope_id)
            .copied()
            .ok_or_else(|| {
                ExpansionError::InvalidLocalBindings(format!(
                    "checked callable {} references missing semantic scope {}",
                    callable.decl_id.0, callable.scope_id.0
                ))
            })?;
        callables.push(SemanticCallable {
            id,
            checked_callable: callable.decl_id,
            scope,
            kind: callable.kind,
            name: callable.name.clone(),
            external_identity: callable.external_identity,
            parameters: callable
                .parameters
                .iter()
                .map(|parameter| SemanticCallableParameter {
                    id: semantic_parameter_id(id, parameter.ordinal),
                    formal: parameter.decl_id,
                    ordinal: parameter.ordinal,
                    name: parameter.name.clone(),
                    kind: parameter.kind,
                    flow_type: parameter.flow_type.clone(),
                    requirement: parameter.requirement.clone(),
                    evaluation_scope: parameter.evaluation_scope,
                    start: parameter.start,
                    end: parameter.end,
                })
                .collect(),
            contexts: callable
                .contexts
                .iter()
                .map(|context| SemanticCallableContext {
                    name: context.name.clone(),
                    kind: context.kind,
                    provider: context.provider,
                    flow_type: context.flow_type.clone(),
                })
                .collect(),
            context_formal: callable.context_formal,
            result: callable.result.clone(),
            role: callable.role,
            effect: callable.effect,
            body: callable.body,
            result_expression: callable.result_expression,
            contextual_operation: callable.contextual_operation,
        });
    }
    Ok((callables, callable_ids))
}

fn semantic_call_inventory(
    program: &CheckedProgram,
    semantic_scope_ids: &BTreeMap<boon_typecheck::LexicalScopeId, SemanticScopeId>,
    callable_ids: &BTreeMap<DeclId, SemanticCallableId>,
) -> Result<(Vec<SemanticCall>, BTreeMap<CheckedCallId, SemanticCallId>), ExpansionError> {
    let expressions = program
        .expressions
        .iter()
        .map(|expression| (expression.id, expression))
        .collect::<BTreeMap<_, _>>();
    let callables = program
        .callables
        .iter()
        .map(|callable| (callable.decl_id, callable))
        .collect::<BTreeMap<_, _>>();
    let temporally_gated = crate::temporally_gated_checked_expressions(program);
    let mut call_ids = BTreeMap::new();
    let mut calls = Vec::with_capacity(program.calls.len());
    for (index, call) in program.calls.iter().enumerate() {
        let id = SemanticCallId(index);
        if call_ids.insert(call.id, id).is_some() {
            return Err(ExpansionError::InvalidLocalBindings(format!(
                "checked call {} is defined more than once",
                call.id.0
            )));
        }
        let callable = callables.get(&call.callable).copied().ok_or_else(|| {
            ExpansionError::InvalidLocalBindings(format!(
                "checked call {} references missing callable {}",
                call.id.0, call.callable.0
            ))
        })?;
        let semantic_callable = callable_ids.get(&call.callable).copied().ok_or_else(|| {
            ExpansionError::InvalidLocalBindings(format!(
                "checked call {} has no semantic callable {}",
                call.id.0, call.callable.0
            ))
        })?;
        let owner_callable = call
            .owner_callable
            .map(|owner| {
                callable_ids.get(&owner).copied().ok_or_else(|| {
                    ExpansionError::InvalidLocalBindings(format!(
                        "checked call {} references missing owner callable {}",
                        call.id.0, owner.0
                    ))
                })
            })
            .transpose()?;
        let mut entries = Vec::with_capacity(call.entries.len());
        for entry in &call.entries {
            let formal = match entry {
                CheckedCallEntry::Input { formal, .. }
                | CheckedCallEntry::FreshOut { formal, .. }
                | CheckedCallEntry::ForwardOut { formal, .. } => *formal,
            };
            let parameter = callable
                .parameters
                .iter()
                .find(|parameter| parameter.decl_id == formal)
                .ok_or(ExpansionError::MissingFormal {
                    callable: callable.decl_id,
                    formal,
                })?;
            entries.push(match entry {
                CheckedCallEntry::Input {
                    name,
                    value,
                    from_pipe,
                    evaluation_scope,
                    ..
                } => {
                    let value_flow_type = expressions
                        .get(value)
                        .map(|expression| expression.flow_type.clone())
                        .ok_or(ExpansionError::MissingExpression(*value))?;
                    SemanticCallEntry::Input {
                        formal,
                        ordinal: parameter.ordinal,
                        name: name.clone(),
                        checked_value: *value,
                        value_flow_type,
                        from_pipe: *from_pipe,
                        evaluation_scope: *evaluation_scope,
                        requirement: parameter.requirement.clone(),
                    }
                }
                CheckedCallEntry::FreshOut {
                    name,
                    output,
                    scope_id,
                    ..
                } => SemanticCallEntry::FreshOut {
                    formal,
                    ordinal: parameter.ordinal,
                    name: name.clone(),
                    output: *output,
                    scope: semantic_scope_ids.get(scope_id).copied().ok_or_else(|| {
                        ExpansionError::InvalidLocalBindings(format!(
                            "checked call {} OUT entry references missing scope {}",
                            call.id.0, scope_id.0
                        ))
                    })?,
                },
                CheckedCallEntry::ForwardOut {
                    name,
                    target,
                    target_name,
                    ..
                } => SemanticCallEntry::ForwardOut {
                    formal,
                    ordinal: parameter.ordinal,
                    name: name.clone(),
                    target: *target,
                    target_name: target_name.clone(),
                },
            });
        }
        let contexts = call
            .contexts
            .iter()
            .map(|context| {
                Ok(SemanticCallContextBinding {
                    declaration: context.declaration,
                    signature: context.signature,
                    scope: semantic_scope_ids
                        .get(&context.scope_id)
                        .copied()
                        .ok_or_else(|| {
                            ExpansionError::InvalidLocalBindings(format!(
                                "checked call {} context references missing scope {}",
                                call.id.0, context.scope_id.0
                            ))
                        })?,
                })
            })
            .collect::<Result<Vec<_>, ExpansionError>>()?;
        calls.push(SemanticCall {
            id,
            checked_call: call.id,
            checked_expression: call.expression,
            callable: semantic_callable,
            owner_callable,
            function: call.function.clone(),
            intrinsic: call.intrinsic,
            external_identity: callable.external_identity,
            entries,
            contexts,
            context_binding: call.context_binding,
            contextual_substitutions: call.contextual_substitutions.clone(),
            type_substitutions: call.type_substitutions.clone(),
            result: call.result.clone(),
            role: call.role,
            effect: callable.effect,
            span: call.span,
            occurrence_segment: crate::out_net::checked_call_occurrence_segment(program, call.id)
                .map_err(ExpansionError::InvalidLocalBindings)?,
            temporally_gated: temporally_gated.contains(&call.expression),
        });
    }
    Ok((calls, call_ids))
}

pub(crate) fn validate_checked_callable_and_call_inventory(
    program: &CheckedProgram,
    execution: &SemanticExecutionGraphV1,
) -> Result<(), String> {
    let semantic_scope_ids = program
        .scopes
        .iter()
        .enumerate()
        .map(|(index, scope)| (scope.id, SemanticScopeId(index)))
        .collect::<BTreeMap<_, _>>();
    let (expected_callables, callable_ids) =
        semantic_callable_inventory(program, &semantic_scope_ids)
            .map_err(|error| error.to_string())?;
    let (expected_calls, _) = semantic_call_inventory(program, &semantic_scope_ids, &callable_ids)
        .map_err(|error| error.to_string())?;
    if execution.callables != expected_callables {
        return Err(
            "semantic callable inventory does not exactly cover the checked callable schema"
                .to_owned(),
        );
    }
    if execution.calls != expected_calls {
        return Err(
            "semantic call inventory does not exactly cover the checked call schema".to_owned(),
        );
    }
    Ok(())
}

pub(crate) fn derive_semantic_execution_graph(
    program: &CheckedProgram,
    out_net: &OutNet,
    materializations: &[SemanticContextualMaterialization],
    mut arena: SemanticExpressionArena,
) -> Result<SemanticExecutionGraphV1, ExpansionError> {
    let lookup = CheckedProgramLookup::new(program);
    let semantic_scope_ids = program
        .scopes
        .iter()
        .enumerate()
        .map(|(index, scope)| (scope.id, SemanticScopeId(index)))
        .collect::<BTreeMap<_, _>>();
    let scopes = program
        .scopes
        .iter()
        .enumerate()
        .map(|(index, scope)| {
            Ok(SemanticScope {
                id: SemanticScopeId(index),
                checked_scope: scope.id,
                parent: scope
                    .parent
                    .map(|parent| {
                        semantic_scope_ids.get(&parent).copied().ok_or_else(|| {
                            ExpansionError::InvalidLocalBindings(format!(
                                "checked scope {} references missing parent {}",
                                scope.id.0, parent.0
                            ))
                        })
                    })
                    .transpose()?,
                owner: scope.owner,
                kind: scope.kind,
                span: scope.span,
            })
        })
        .collect::<Result<Vec<_>, ExpansionError>>()?;
    let (callables, callable_ids) = semantic_callable_inventory(program, &semantic_scope_ids)?;
    let (calls, _) = semantic_call_inventory(program, &semantic_scope_ids, &callable_ids)?;
    let materializations_by_owner = materializations
        .iter()
        .map(|materialization| (materialization.owner, materialization.id))
        .collect::<BTreeMap<_, _>>();
    let materialization_result_types = materializations
        .iter()
        .map(|materialization| (materialization.id, materialization.result_type.clone()))
        .collect::<BTreeMap<_, _>>();
    let mut builder = SemanticExpressionBuilder::new(
        program,
        &lookup,
        out_net,
        BTreeMap::new(),
        BTreeMap::new(),
        &materializations_by_owner,
        &materialization_result_types,
    );
    let included = program
        .statements
        .iter()
        .filter(|statement| {
            !matches!(
                statement.kind,
                boon_typecheck::CheckedStatementKind::Function { .. }
            ) && !declaration_is_function_local(program, statement.scope_id)
        })
        .map(|statement| statement.id)
        .collect::<BTreeSet<_>>();
    let semantic_statement_ids = program
        .statements
        .iter()
        .filter(|statement| included.contains(&statement.id))
        .enumerate()
        .map(|(index, statement)| (statement.id, SemanticStatementId(index)))
        .collect::<BTreeMap<_, _>>();
    let mut semantic_statement_parents = BTreeMap::new();
    for statement in program
        .statements
        .iter()
        .filter(|statement| included.contains(&statement.id))
    {
        let parent = semantic_statement_ids[&statement.id];
        for child in statement
            .children
            .iter()
            .filter(|child| included.contains(child))
        {
            let child = semantic_statement_ids[child];
            if let Some(previous) = semantic_statement_parents.insert(child, parent)
                && previous != parent
            {
                return Err(ExpansionError::InvalidLocalBindings(format!(
                    "semantic statement {child} has parents {previous} and {parent}"
                )));
            }
        }
    }
    let mut statements = Vec::with_capacity(included.len());
    for statement in program
        .statements
        .iter()
        .filter(|statement| included.contains(&statement.id))
    {
        let semantic_statement = semantic_statement_ids[&statement.id];
        builder.set_current_statement(Some(semantic_statement));
        let declaration = match &statement.kind {
            boon_typecheck::CheckedStatementKind::Function { declaration }
            | boon_typecheck::CheckedStatementKind::Field { declaration } => Some(*declaration),
            boon_typecheck::CheckedStatementKind::Source { declaration, .. }
            | boon_typecheck::CheckedStatementKind::Hold { declaration, .. }
            | boon_typecheck::CheckedStatementKind::List { declaration, .. } => *declaration,
            boon_typecheck::CheckedStatementKind::Block
            | boon_typecheck::CheckedStatementKind::Spread
            | boon_typecheck::CheckedStatementKind::Expression => None,
        };
        let value = statement
            .value
            .map(|expression| {
                let value = builder.expand_with_inherited_owner(
                    ScopedCheckedExpr {
                        expression,
                        frame: None,
                        evaluation_port: None,
                        value_frame: None,
                    },
                    None,
                )?;
                let is_host_root = !semantic_statement_parents.contains_key(&semantic_statement);
                if declaration.is_some() || is_host_root {
                    let flow_type = declaration.and_then(|declaration| {
                        lookup
                            .declaration(program, declaration)
                            .map(|declaration| declaration.flow_type.clone())
                    });
                    let boundary_expression =
                        builder.flush_boundary_origin_for_value(expression, value);
                    builder.wrap_flush_boundary(boundary_expression, value, None, flow_type)
                } else {
                    Ok(value)
                }
            })
            .transpose()?;
        let declaration_parts = |declaration: Option<DeclId>| {
            declaration
                .and_then(|declaration| {
                    let checked = lookup.declaration(program, declaration)?;
                    Some((
                        checked.name.clone(),
                        canonical_declaration_path(program, &lookup, declaration)?,
                    ))
                })
                .unzip()
        };
        let kind = match &statement.kind {
            boon_typecheck::CheckedStatementKind::Function { .. } => unreachable!(),
            boon_typecheck::CheckedStatementKind::Field { declaration } => {
                let (name, path) = declaration_parts(Some(*declaration));
                SemanticStatementKind::Field {
                    name: name.ok_or(ExpansionError::MissingDeclaration(*declaration))?,
                    path: path.ok_or(ExpansionError::MissingDeclaration(*declaration))?,
                }
            }
            boon_typecheck::CheckedStatementKind::Source { declaration, event } => {
                let (name, path) = declaration_parts(*declaration);
                SemanticStatementKind::Source {
                    name,
                    path,
                    event: event.clone(),
                }
            }
            boon_typecheck::CheckedStatementKind::Hold {
                declaration,
                name: hold_name,
            } => {
                let (name, path) = declaration_parts(*declaration);
                SemanticStatementKind::Hold {
                    name,
                    path,
                    hold_name: hold_name.clone(),
                }
            }
            boon_typecheck::CheckedStatementKind::List {
                declaration,
                capacity,
            } => {
                let (name, path) = declaration_parts(*declaration);
                SemanticStatementKind::List {
                    name,
                    path,
                    capacity: *capacity,
                }
            }
            boon_typecheck::CheckedStatementKind::Block => SemanticStatementKind::Block,
            boon_typecheck::CheckedStatementKind::Spread => SemanticStatementKind::Spread,
            boon_typecheck::CheckedStatementKind::Expression => SemanticStatementKind::Expression,
        };
        statements.push(SemanticStatement {
            id: semantic_statement,
            origin: SemanticStatementOrigin::Checked {
                statement: statement.id,
            },
            scope: semantic_scope_ids
                .get(&statement.scope_id)
                .copied()
                .ok_or_else(|| {
                    ExpansionError::InvalidLocalBindings(format!(
                        "checked statement {} references missing scope {}",
                        statement.id.0, statement.scope_id.0
                    ))
                })?,
            parent: semantic_statement_parents.get(&semantic_statement).copied(),
            call_instance: None,
            span: statement.span,
            checked_resources: statement.resources.clone(),
            declaration,
            flow_type: declaration
                .and_then(|declaration| lookup.declaration(program, declaration))
                .map(|declaration| declaration.flow_type.clone()),
            kind,
            value,
            value_use: match statement.value_use {
                CheckedValueUse::RuntimeValue => SemanticMaterializationResultKind::RuntimeValue,
                CheckedValueUse::RenderSlot => SemanticMaterializationResultKind::RenderSlot,
            },
            children: statement
                .children
                .iter()
                .filter(|child| included.contains(child))
                .map(|child| semantic_statement_ids[child])
                .collect(),
        });
    }
    builder.set_current_statement(None);
    let mut producer_bodies = Vec::with_capacity(out_net.producer_roots().len());
    let producer_statement_offset = statements.len();
    for (producer_index, producer) in out_net.producer_roots().iter().enumerate() {
        let semantic_statement = SemanticStatementId(producer_statement_offset + producer_index);
        builder.set_current_statement(Some(semantic_statement));
        let callable = lookup
            .callable(program, producer.spec.callable)
            .ok_or(ExpansionError::MissingCallable(producer.spec.callable))?;
        let result = callable
            .result_expression
            .ok_or(ExpansionError::MissingFunctionResult(
                producer.spec.callable,
            ))?;
        let owner = out_net
            .owner_for_call(producer.call)
            .ok_or(ExpansionError::MissingProducerOwner(producer.spec.identity))?;
        let body = builder.expand_with_inherited_owner(
            ScopedCheckedExpr {
                expression: result,
                frame: Some(producer.call),
                evaluation_port: None,
                value_frame: None,
            },
            Some(owner),
        )?;
        producer_bodies.push((producer, result, owner, body, semantic_statement));
    }
    builder.set_current_statement(None);

    statements.sort_by_key(|statement| statement.id);
    let offset = arena.expressions.len();
    let local_expressions = builder.finish();
    for statement in &mut statements {
        if let Some(value) = &mut statement.value {
            *value = rebase_expr_id(*value, offset);
        }
    }
    append_expression_arena_without_roots(&mut arena, local_expressions);
    let mut functions = Vec::with_capacity(producer_bodies.len());
    let mut producer_sources = Vec::new();
    for (producer, checked_result, owner, body, semantic_statement) in producer_bodies {
        let body = rebase_expr_id(body, offset);
        let callable = lookup
            .callable(program, producer.spec.callable)
            .ok_or(ExpansionError::MissingCallable(producer.spec.callable))?;
        let checked_statement = callable
            .body
            .and_then(|statement| lookup.statement(program, statement))
            .ok_or(ExpansionError::MissingFunctionResult(
                producer.spec.callable,
            ))?;
        let semantic_scope = semantic_scope_ids
            .get(&checked_statement.scope_id)
            .copied()
            .ok_or_else(|| {
                ExpansionError::InvalidLocalBindings(format!(
                    "producer {} checked statement {} references missing scope {}",
                    producer.spec.function_name,
                    checked_statement.id.0,
                    checked_statement.scope_id.0
                ))
            })?;
        let checked_result_definition = lookup
            .expression(program, checked_result)
            .ok_or(ExpansionError::MissingExpression(checked_result))?;
        let invocation_source =
            if producer.spec.mode == crate::ProducerMaterializationMode::Invocation {
                let identity = producer
                    .spec
                    .identity
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect::<String>();
                let binding_path = format!("_producer_{identity}_invoke");
                let source = arena.push(
                    checked_result,
                    checked_result_definition.scope_id,
                    checked_result_definition.span,
                    Some(producer.call),
                    Some(semantic_statement),
                    |source, value_id| SemanticExpression {
                        id: source,
                        value_id,
                        checked_expr_id: checked_result,
                        flow_type: boon_typecheck::FlowType {
                            mode: FlowMode::PresentOrAbsent,
                            ty: Type::Absent,
                        },
                        effect: boon_typecheck::CheckedEffectSummary {
                            emits_source: true,
                            ..boon_typecheck::CheckedEffectSummary::default()
                        },
                        owner: Some(owner),
                        provenance: SemanticValueProvenance {
                            members: vec![SemanticValueMember {
                                path: Vec::new(),
                                origin: SemanticValueOrigin::ProducerSource {
                                    function: callable_ids[&producer.spec.callable],
                                    producer: producer.spec.function,
                                    identity: producer.spec.identity,
                                    owner,
                                },
                            }],
                        },
                        resource_binding_path: Some(binding_path.clone()),
                        kind: SemanticExpressionKind::Source {
                            binding_path: binding_path.clone(),
                        },
                    },
                );
                producer_sources.push((
                    callable_ids[&producer.spec.callable],
                    producer.spec.function,
                    producer.spec.identity,
                    producer.spec.result_declaration,
                    source,
                    binding_path,
                    owner,
                ));
                Some(source)
            } else {
                None
            };
        let root = if let Some(source) = invocation_source {
            let provenance = arena.expressions[body.as_usize()].provenance.clone();
            arena.push(
                checked_result,
                checked_result_definition.scope_id,
                checked_result_definition.span,
                Some(producer.call),
                Some(semantic_statement),
                |root, value_id| SemanticExpression {
                    id: root,
                    value_id,
                    checked_expr_id: checked_result,
                    flow_type: producer.spec.result_type.clone(),
                    effect: boon_typecheck::CheckedEffectSummary {
                        emits_source: true,
                        ..lookup
                            .callable(program, producer.spec.callable)
                            .map(|callable| callable.effect)
                            .unwrap_or_default()
                    },
                    owner: Some(owner),
                    provenance,
                    resource_binding_path: None,
                    kind: SemanticExpressionKind::Then {
                        input: source,
                        output: Some(body),
                    },
                },
            )
        } else {
            body
        };
        statements.push(SemanticStatement {
            id: semantic_statement,
            origin: SemanticStatementOrigin::ProducerResult {
                identity: producer.spec.identity,
                function: producer.spec.function,
                callable: producer.spec.callable,
                root_call: producer.call,
                result_statement: producer.spec.result_statement,
                checked_statement: checked_statement.id,
                checked_result_expression: checked_result,
            },
            scope: semantic_scope,
            parent: None,
            call_instance: Some(producer.call),
            span: checked_statement.span,
            checked_resources: checked_statement.resources.clone(),
            declaration: Some(producer.spec.result_declaration),
            flow_type: Some(producer.spec.result_type.clone()),
            kind: SemanticStatementKind::Field {
                name: "result".to_owned(),
                path: producer.spec.result_path.clone(),
            },
            value: Some(root),
            value_use: SemanticMaterializationResultKind::RuntimeValue,
            children: Vec::new(),
        });
        functions.push(SemanticFunction {
            producer: producer.spec.function,
            callable: callable_ids[&producer.spec.callable],
            identity: producer.spec.identity,
            name: producer.spec.function_name.clone(),
            parameters: producer
                .spec
                .parameters
                .iter()
                .map(|parameter| {
                    let checked_parameter = callable
                        .parameters
                        .iter()
                        .find(|candidate| candidate.decl_id == parameter.formal)
                        .ok_or(ExpansionError::MissingFormal {
                            callable: callable.decl_id,
                            formal: parameter.formal,
                        })?;
                    let semantic_parameter = semantic_parameter_id(
                        callable_ids[&producer.spec.callable],
                        checked_parameter.ordinal,
                    );
                    let input_expressions = arena
                        .expressions
                        .iter()
                        .filter_map(|expression| match expression.kind {
                            SemanticExpressionKind::FunctionParameter {
                                parameter: candidate,
                                ..
                            } if candidate == semantic_parameter
                                && expression.owner == Some(owner) =>
                            {
                                Some(expression.id)
                            }
                            _ => None,
                        })
                        .collect();
                    Ok(SemanticFunctionParameter {
                        id: semantic_parameter,
                        formal: parameter.formal,
                        name: parameter.name.clone(),
                        flow_type: parameter.flow_type.clone(),
                        requirement: checked_parameter.requirement.clone(),
                        input_expressions,
                    })
                })
                .collect::<Result<Vec<_>, ExpansionError>>()?,
            result_type: producer.spec.result_type.clone(),
            root,
            invocation_source,
        });
    }
    statements.sort_by_key(|statement| statement.id);
    synthesize_statement_owned_states(program, &lookup, &mut arena, &mut statements)?;
    resolve_executable_local_provenance(&mut arena.expressions, &statements)?;
    let root_specs =
        checked_semantic_root_specs_v1(program).map_err(ExpansionError::InvalidLocalBindings)?;
    for root in &root_specs {
        let statement = semantic_statement_ids
            .get(&root.checked_statement)
            .copied()
            .ok_or_else(|| {
                ExpansionError::InvalidLocalBindings(format!(
                    "checked output-root statement {} has no exact semantic statement",
                    root.checked_statement.0
                ))
            })?;
        let statement_definition = statements
            .get(statement.as_usize())
            .filter(|definition| definition.id == statement)
            .ok_or_else(|| {
                ExpansionError::InvalidLocalBindings(format!(
                    "checked output-root statement {} references missing semantic statement {statement}",
                    root.checked_statement.0
                ))
            })?;
        if statement_definition.origin
            != (SemanticStatementOrigin::Checked {
                statement: root.checked_statement,
            })
            || statement_definition.declaration != Some(root.declaration)
            || statement_definition.call_instance.is_some()
        {
            return Err(ExpansionError::InvalidLocalBindings(format!(
                "checked output-root statement {} does not match semantic statement {statement}",
                root.checked_statement.0
            )));
        }
        let input = statement_definition.value.ok_or_else(|| {
            ExpansionError::InvalidLocalBindings(format!(
                "semantic output-root statement {statement} has no value"
            ))
        })?;
        let input_definition = arena
            .expressions
            .get(input.as_usize())
            .filter(|definition| definition.id == input)
            .cloned()
            .ok_or_else(|| {
                ExpansionError::InvalidLocalBindings(format!(
                    "semantic output-root statement {statement} references missing expression {input}"
                ))
            })?;
        let input_origin = arena
            .checked_expression_origins
            .get(input.as_usize())
            .filter(|origin| origin.expression == input)
            .cloned()
            .ok_or_else(|| {
                ExpansionError::InvalidLocalBindings(format!(
                    "semantic output-root expression {input} has no exact checked origin"
                ))
            })?;
        if input_origin.checked_expression != input_definition.checked_expr_id {
            return Err(ExpansionError::InvalidLocalBindings(format!(
                "semantic output-root expression {input} has mismatched checked provenance"
            )));
        }
        if input_origin.owning_statement != Some(statement) {
            let normalized = arena.push(
                input_definition.checked_expr_id,
                input_origin.checked_scope,
                input_origin.checked_span,
                input_origin.call_instance,
                Some(statement),
                |id, value_id| SemanticExpression {
                    id,
                    value_id,
                    checked_expr_id: input_definition.checked_expr_id,
                    flow_type: input_definition.flow_type.clone(),
                    effect: boon_typecheck::CheckedEffectSummary::default(),
                    owner: input_definition.owner,
                    provenance: input_definition.provenance.clone(),
                    resource_binding_path: None,
                    kind: SemanticExpressionKind::Project {
                        input,
                        fields: Vec::new(),
                    },
                },
            );
            statements[statement.as_usize()].value = Some(normalized);
        }
    }
    let roots = root_specs
        .into_iter()
        .enumerate()
        .map(|(ordinal, root)| {
            let statement = semantic_statement_ids
                .get(&root.checked_statement)
                .copied()
                .ok_or_else(|| {
                    ExpansionError::InvalidLocalBindings(format!(
                        "checked output-root statement {} has no exact semantic statement",
                        root.checked_statement.0
                    ))
                })?;
            let statement_definition = statements
                .get(statement.as_usize())
                .filter(|definition| definition.id == statement)
                .ok_or_else(|| {
                    ExpansionError::InvalidLocalBindings(format!(
                        "checked output-root statement {} references missing semantic statement {statement}",
                        root.checked_statement.0
                    ))
                })?;
            if statement_definition.origin
                != (SemanticStatementOrigin::Checked {
                    statement: root.checked_statement,
                })
                || statement_definition.declaration != Some(root.declaration)
                || statement_definition.call_instance.is_some()
            {
                return Err(ExpansionError::InvalidLocalBindings(format!(
                    "checked output-root statement {} does not match semantic statement {statement}",
                    root.checked_statement.0
                )));
            }
            let expression = statement_definition.value.ok_or_else(|| {
                ExpansionError::InvalidLocalBindings(format!(
                    "semantic output-root statement {statement} has no value"
                ))
            })?;
            let expression_definition = arena
                .expressions
                .get(expression.as_usize())
                .filter(|definition| definition.id == expression)
                .ok_or_else(|| {
                    ExpansionError::InvalidLocalBindings(format!(
                        "semantic output-root statement {statement} references missing expression {expression}"
                    ))
                })?;
            let origin = arena
                .checked_expression_origins
                .get(expression.as_usize())
                .filter(|origin| origin.expression == expression)
                .ok_or_else(|| {
                    ExpansionError::InvalidLocalBindings(format!(
                        "semantic output-root expression {expression} has no exact checked origin"
                    ))
                })?;
            if origin.checked_expression != expression_definition.checked_expr_id
                || origin.owning_statement != Some(statement)
            {
                return Err(ExpansionError::InvalidLocalBindings(format!(
                    "semantic output-root expression {expression} has checked expression {} and statement {:?}, expected expression provenance {} and statement {statement}",
                    origin.checked_expression.0,
                    origin.owning_statement,
                    expression_definition.checked_expr_id.0
                )));
            }
            Ok(SemanticRoot {
                ordinal,
                kind: root.kind,
                declaration: root.declaration,
                checked_statement: root.checked_statement,
                statement,
                checked_expr_id: expression_definition.checked_expr_id,
                expression,
                value: expression_definition.value_id,
            })
        })
        .collect::<Result<Vec<_>, ExpansionError>>()?;
    let mut sources = Vec::new();
    let mut semantic_source_by_checked_instance = BTreeMap::new();
    for checked_source in &program.sources {
        let fallback_path = program.semantic_path(&checked_source.path).ok_or(
            ExpansionError::MissingSourceDeclaration(checked_source.expression),
        )?;
        let mut candidates_by_instance = BTreeMap::new();
        for expression in arena.expressions.iter().filter(|expression| {
            expression.checked_expr_id == checked_source.expression
                && (matches!(expression.kind, SemanticExpressionKind::Source { .. })
                    || expression.effect.emits_source)
        }) {
            let _origin = arena
                .checked_expression_origins
                .get(expression.id.as_usize())
                .filter(|origin| origin.expression == expression.id)
                .ok_or(ExpansionError::MissingExpression(
                    expression.checked_expr_id,
                ))?;
            // A checked resource plus its structural owner is the executable
            // identity. Transparent wrapper frames are debug provenance only
            // and must not split one owned SOURCE into multiple resources.
            let key = (checked_source.id, expression.owner, None);
            candidates_by_instance
                .entry(key)
                .or_insert_with(Vec::new)
                .push(expression.id);
        }
        for (key, candidates) in candidates_by_instance {
            let (expression, statement) = exact_resource_instance_expression(
                ExactResourceInstanceContext {
                    expressions: &arena.expressions,
                    origins: &arena.checked_expression_origins,
                    statements: &statements,
                    materializations,
                    out_net,
                    checked_statement: checked_source.statement,
                    declaration: checked_source.declaration,
                    checked_binding: CheckedResourceBinding::Source {
                        source: checked_source.id,
                    },
                    resource_kind: "source",
                    checked_id: checked_source.id.0,
                },
                &candidates,
            )?;
            let (expression, statement) = ensure_resource_definition_statement(
                program,
                &lookup,
                &semantic_scope_ids,
                &mut arena,
                &mut statements,
                expression,
                statement,
                checked_source.statement,
                checked_source.declaration,
                CheckedResourceBinding::Source {
                    source: checked_source.id,
                },
            )?;
            let expression_definition = &arena.expressions[expression.as_usize()];
            let origin = &arena.checked_expression_origins[expression.as_usize()];
            let id = SemanticSourceId(sources.len());
            if semantic_source_by_checked_instance
                .insert(key, id)
                .is_some()
            {
                return Err(ExpansionError::InvalidLocalBindings(format!(
                    "checked source {} concrete instance was enumerated twice",
                    checked_source.id.0
                )));
            }
            sources.push(SemanticSourceDef {
                id,
                origin: SemanticSourceOrigin::Checked {
                    source: checked_source.id,
                },
                declaration: checked_source.declaration,
                statement,
                checked_statement: Some(checked_source.statement),
                expression,
                call_instance: origin.call_instance,
                binding_path: fallback_path.clone(),
                owner: expression_definition.owner,
            });
        }
    }
    for (function, producer, identity, declaration, expression, binding_path, owner) in
        producer_sources
    {
        let origin = arena
            .checked_expression_origins
            .get(expression.as_usize())
            .filter(|origin| origin.expression == expression)
            .ok_or_else(|| {
                ExpansionError::InvalidLocalBindings(format!(
                    "producer source expression {expression} has no exact semantic origin"
                ))
            })?;
        let statement = origin.owning_statement.ok_or_else(|| {
            ExpansionError::InvalidLocalBindings(format!(
                "producer source expression {expression} has no exact semantic statement"
            ))
        })?;
        sources.push(SemanticSourceDef {
            id: SemanticSourceId(sources.len()),
            origin: SemanticSourceOrigin::ProducerInvocation {
                function,
                producer,
                identity,
            },
            declaration,
            statement,
            checked_statement: None,
            expression,
            call_instance: origin.call_instance,
            binding_path,
            owner: Some(owner),
        });
    }
    let mut states = Vec::new();
    let mut semantic_state_by_checked_instance = BTreeMap::new();
    for checked_state in &program.states {
        let fallback_path = program.semantic_path(&checked_state.path).ok_or(
            ExpansionError::MissingStateDeclaration(checked_state.expression),
        )?;
        let mut candidates_by_instance = BTreeMap::new();
        for expression in arena.expressions.iter().filter(|expression| {
            expression.checked_expr_id == checked_state.expression
                && (matches!(
                    expression.kind,
                    SemanticExpressionKind::Hold { .. } | SemanticExpressionKind::Latest { .. }
                ) || expression.effect.writes_state)
        }) {
            let _origin = arena
                .checked_expression_origins
                .get(expression.id.as_usize())
                .filter(|origin| origin.expression == expression.id)
                .ok_or(ExpansionError::MissingExpression(
                    expression.checked_expr_id,
                ))?;
            // Stateful wrapper depth does not participate in runtime identity;
            // distinct call sites already have distinct structural owners.
            let key = (checked_state.id, expression.owner, None);
            candidates_by_instance
                .entry(key)
                .or_insert_with(Vec::new)
                .push(expression.id);
        }
        for (key, candidates) in candidates_by_instance {
            let (expression, statement) = exact_resource_instance_expression(
                ExactResourceInstanceContext {
                    expressions: &arena.expressions,
                    origins: &arena.checked_expression_origins,
                    statements: &statements,
                    materializations,
                    out_net,
                    checked_statement: checked_state.statement,
                    declaration: checked_state.declaration,
                    checked_binding: CheckedResourceBinding::State {
                        state: checked_state.id,
                    },
                    resource_kind: "state",
                    checked_id: checked_state.id.0,
                },
                &candidates,
            )?;
            let (expression, statement) = ensure_resource_definition_statement(
                program,
                &lookup,
                &semantic_scope_ids,
                &mut arena,
                &mut statements,
                expression,
                statement,
                checked_state.statement,
                checked_state.declaration,
                CheckedResourceBinding::State {
                    state: checked_state.id,
                },
            )?;
            let expression_definition = &arena.expressions[expression.as_usize()];
            let origin = &arena.checked_expression_origins[expression.as_usize()];
            let initial = concrete_state_initial_expression(&arena.expressions, expression).ok_or(
                ExpansionError::MissingStateInitializer(checked_state.expression),
            )?;
            let lifetime = derive_semantic_state_lifetime_v1(&arena.expressions, expression)
                .map_err(ExpansionError::InvalidLocalBindings)?;
            let id = SemanticStateId(states.len());
            if semantic_state_by_checked_instance.insert(key, id).is_some() {
                return Err(ExpansionError::InvalidLocalBindings(format!(
                    "checked state {} concrete instance was enumerated twice",
                    checked_state.id.0
                )));
            }
            states.push(SemanticStateDef {
                id,
                checked_state: checked_state.id,
                declaration: checked_state.declaration,
                statement,
                checked_statement: checked_state.statement,
                expression,
                initial,
                call_instance: origin.call_instance,
                binding_path: fallback_path.clone(),
                owner: expression_definition.owner,
                lifetime,
            });
        }
    }
    remap_checked_resource_ids(
        program,
        &mut arena.expressions,
        &arena.checked_expression_origins,
        &semantic_source_by_checked_instance,
        &semantic_state_by_checked_instance,
    )?;
    Ok(SemanticExecutionGraphV1 {
        materializations: materializations.to_vec(),
        expressions: arena.expressions,
        statements,
        scopes,
        callables,
        calls,
        sources,
        states,
        roots,
        functions,
        static_owners: out_net
            .static_owners
            .iter()
            .map(SemanticStaticOwner::from)
            .collect(),
        checked_expression_origins: arena.checked_expression_origins,
    })
}

struct ExactResourceInstanceContext<'a> {
    expressions: &'a [SemanticExpression],
    origins: &'a [SemanticExpressionOrigin],
    statements: &'a [SemanticStatement],
    materializations: &'a [SemanticContextualMaterialization],
    out_net: &'a OutNet,
    checked_statement: boon_typecheck::CheckedStatementId,
    declaration: DeclId,
    checked_binding: CheckedResourceBinding,
    resource_kind: &'static str,
    checked_id: u32,
}

fn exact_resource_instance_expression(
    context: ExactResourceInstanceContext<'_>,
    candidates: &[SemanticExprId],
) -> Result<(SemanticExprId, Option<SemanticStatementId>), ExpansionError> {
    let ExactResourceInstanceContext {
        expressions,
        origins,
        statements,
        materializations,
        out_net,
        checked_statement,
        declaration,
        checked_binding,
        resource_kind,
        checked_id,
    } = context;
    let candidate = |id: SemanticExprId| {
        let expression = expressions
            .get(id.as_usize())
            .filter(|expression| expression.id == id)
            .ok_or(ExpansionError::InvalidLocalBindings(format!(
                "checked {resource_kind} {checked_id} references missing semantic candidate {id}"
            )))?;
        let origin = origins
            .get(id.as_usize())
            .filter(|origin| {
                origin.expression == id && origin.checked_expression == expression.checked_expr_id
            })
            .ok_or(ExpansionError::InvalidLocalBindings(format!(
                "checked {resource_kind} {checked_id} candidate {id} has no exact origin"
            )))?;
        let statement = origin
            .owning_statement
            .map(|statement_id| {
                statements
                    .get(statement_id.as_usize())
                    .filter(|statement| statement.id == statement_id)
                    .ok_or(ExpansionError::InvalidLocalBindings(format!(
                        "checked {resource_kind} {checked_id} candidate {id} references missing semantic statement {statement_id}"
                    )))
            })
            .transpose()?;
        Ok((id, statement))
    };

    let mut candidates = candidates
        .iter()
        .copied()
        .map(candidate)
        .collect::<Result<Vec<_>, ExpansionError>>()?;
    let exact_resource_candidates = candidates
        .iter()
        .copied()
        .filter(|(candidate, _)| {
            expressions
                .get(candidate.as_usize())
                .filter(|expression| expression.id == *candidate)
                .is_some_and(|expression| match &checked_binding {
                    CheckedResourceBinding::Source { .. } => {
                        matches!(expression.kind, SemanticExpressionKind::Source { .. })
                    }
                    CheckedResourceBinding::State { .. } => matches!(
                        expression.kind,
                        SemanticExpressionKind::Hold { .. } | SemanticExpressionKind::Latest { .. }
                    ),
                    CheckedResourceBinding::ListAuthority { .. }
                    | CheckedResourceBinding::ListAlias { .. } => false,
                })
        })
        .collect::<Vec<_>>();
    if !exact_resource_candidates.is_empty() {
        candidates = exact_resource_candidates;
    }
    let declaration_statements = statements
        .iter()
        .filter(|statement| {
            matches!(
                statement.origin,
                SemanticStatementOrigin::Checked { statement }
                    if statement == checked_statement
            ) && statement.declaration == Some(declaration)
                && statement.checked_resources.contains(&checked_binding)
        })
        .map(|statement| statement.id)
        .collect::<Vec<_>>();
    if declaration_statements.len() > 1 {
        return Err(ExpansionError::InvalidLocalBindings(format!(
            "checked {resource_kind} {checked_id} has {} exact semantic declaration statements",
            declaration_statements.len()
        )));
    }
    let mut occurrence_statements = declaration_statements.clone();
    if occurrence_statements.is_empty()
        && let Some((candidate, _)) = candidates.first()
    {
        let expression = expressions
            .get(candidate.as_usize())
            .filter(|expression| expression.id == *candidate)
            .ok_or(ExpansionError::MissingExpression(CheckedExprId(
                candidate.as_usize() as u32,
            )))?;
        let mut occurrence_materializations = BTreeSet::new();
        let mut owner = expression.owner;
        while let Some(current) = owner {
            let at_owner = materializations
                .iter()
                .filter(|materialization| materialization.owner == current)
                .map(|materialization| materialization.id)
                .collect::<BTreeSet<_>>();
            if !at_owner.is_empty() {
                occurrence_materializations = at_owner;
                break;
            }
            owner = out_net
                .static_owners
                .get(current.as_usize())
                .filter(|owner| owner.id == current)
                .and_then(|owner| owner.parent);
        }
        occurrence_statements.extend(expressions.iter().filter_map(|expression| {
            let SemanticExpressionKind::Materialize { materialization } = expression.kind else {
                return None;
            };
            occurrence_materializations
                .contains(&materialization)
                .then(|| {
                    origins
                        .get(expression.id.as_usize())
                        .filter(|origin| origin.expression == expression.id)
                        .and_then(|origin| origin.owning_statement)
                        .filter(|statement| {
                            statements
                                .get(statement.as_usize())
                                .filter(|candidate| candidate.id == *statement)
                                .is_some()
                        })
                })
                .flatten()
        }));
    }
    occurrence_statements.sort();
    occurrence_statements.dedup();
    if occurrence_statements.len() > 1 {
        let declarations = occurrence_statements
            .iter()
            .copied()
            .filter(|statement| {
                statements
                    .get(statement.as_usize())
                    .filter(|candidate| candidate.id == *statement)
                    .is_some_and(|statement| statement.declaration.is_some())
            })
            .collect::<Vec<_>>();
        if !declarations.is_empty() {
            occurrence_statements = declarations;
        }
    }
    let owned_statement_ids = candidates
        .iter()
        .filter_map(|(_, statement)| statement.map(|statement| statement.id))
        .collect::<BTreeSet<_>>();
    let anchor = match occurrence_statements.as_slice() {
        [statement] => Some(*statement),
        [] if owned_statement_ids.len() == 1 => owned_statement_ids.iter().next().copied(),
        [] => None,
        _ => {
            let details = occurrence_statements
                .iter()
                .filter_map(|id| {
                    statements
                        .get(id.as_usize())
                        .filter(|statement| statement.id == *id)
                        .map(|statement| {
                            (
                                statement.id,
                                statement.origin.clone(),
                                statement.declaration,
                                statement.value,
                            )
                        })
                })
                .collect::<Vec<_>>();
            return Err(ExpansionError::InvalidLocalBindings(format!(
                "checked {resource_kind} {checked_id} concrete occurrence maps to {} semantic statements {details:?}",
                occurrence_statements.len(),
            )));
        }
    };
    let Some(anchor) = anchor else {
        if let [candidate] = candidates.as_slice() {
            // Function-local resources inside contextual materializations do
            // not have a pre-existing root semantic statement. Preserve the
            // exact occurrence and let the caller create its declaration
            // statement from the checked statement identity.
            return Ok((candidate.0, None));
        }
        let occurrence = candidates
            .iter()
            .filter_map(|(candidate, statement)| {
                expressions
                    .get(candidate.as_usize())
                    .zip(origins.get(candidate.as_usize()))
                    .map(|(expression, origin)| {
                        (
                            *candidate,
                            expression.owner,
                            origin.call_instance,
                            origin.owning_statement,
                            statement.map(|statement| statement.id),
                        )
                    })
            })
            .collect::<Vec<_>>();
        let materialization_owners = materializations
            .iter()
            .map(|materialization| (materialization.id, materialization.owner))
            .collect::<Vec<_>>();
        let materialize_expressions = expressions
            .iter()
            .filter_map(|expression| {
                let SemanticExpressionKind::Materialize { materialization } = expression.kind
                else {
                    return None;
                };
                Some((
                    expression.id,
                    materialization,
                    expression.owner,
                    origins
                        .get(expression.id.as_usize())
                        .and_then(|origin| origin.owning_statement),
                ))
            })
            .collect::<Vec<_>>();
        return Err(ExpansionError::InvalidLocalBindings(format!(
            "checked {resource_kind} {checked_id} occurrences {occurrence:?} have {} semantic use copies and no exact semantic declaration statement; materialization owners: {materialization_owners:?}; materialize expressions: {materialize_expressions:?}",
            candidates.len(),
        )));
    };
    let definition_sites = candidates
        .iter()
        .filter(|(_, statement)| statement.is_some_and(|statement| statement.id == anchor))
        .map(|(expression, _)| *expression)
        .collect::<Vec<_>>();
    if let [definition_site] = definition_sites.as_slice() {
        return Ok((*definition_site, Some(anchor)));
    }
    if definition_sites.is_empty()
        && let [candidate] = candidates.as_slice()
    {
        return Ok((candidate.0, Some(anchor)));
    }
    let [definition_site] = definition_sites.as_slice() else {
        return Err(ExpansionError::InvalidLocalBindings(format!(
            "checked {resource_kind} {checked_id} has {} semantic use copies but {} definition-owned expressions",
            candidates.len(),
            definition_sites.len()
        )));
    };
    Ok((*definition_site, Some(anchor)))
}

fn ensure_statement_owned_expression(
    arena: &mut SemanticExpressionArena,
    expression: SemanticExprId,
    statement: SemanticStatementId,
) -> Result<SemanticExprId, ExpansionError> {
    let definition = arena
        .expressions
        .get(expression.as_usize())
        .filter(|candidate| candidate.id == expression)
        .cloned()
        .ok_or(ExpansionError::MissingExpression(CheckedExprId(
            expression.as_usize() as u32,
        )))?;
    let origin = arena
        .checked_expression_origins
        .get(expression.as_usize())
        .filter(|origin| origin.expression == expression)
        .cloned()
        .ok_or(ExpansionError::MissingExpression(
            definition.checked_expr_id,
        ))?;
    if origin.owning_statement == Some(statement) {
        return Ok(expression);
    }
    Ok(arena.push(
        definition.checked_expr_id,
        origin.checked_scope,
        origin.checked_span,
        origin.call_instance,
        Some(statement),
        |id, value_id| SemanticExpression {
            id,
            value_id,
            ..definition
        },
    ))
}

fn ensure_resource_statement_owned_expression(
    arena: &mut SemanticExpressionArena,
    statements: &[SemanticStatement],
    expression: SemanticExprId,
    statement: SemanticStatementId,
) -> Result<SemanticExprId, ExpansionError> {
    let origin = arena
        .checked_expression_origins
        .get(expression.as_usize())
        .filter(|origin| origin.expression == expression)
        .cloned()
        .ok_or(ExpansionError::MissingExpression(CheckedExprId(
            expression.as_usize() as u32,
        )))?;
    if origin.owning_statement == Some(statement) {
        return Ok(expression);
    }
    let owns_previous_statement_root = origin.owning_statement.is_some_and(|owner| {
        statements
            .get(owner.as_usize())
            .filter(|candidate| candidate.id == owner)
            .is_some_and(|statement| statement.value == Some(expression))
    });
    if !owns_previous_statement_root {
        let origin = arena
            .checked_expression_origins
            .get_mut(expression.as_usize())
            .filter(|origin| origin.expression == expression)
            .ok_or(ExpansionError::MissingExpression(CheckedExprId(
                expression.as_usize() as u32,
            )))?;
        origin.owning_statement = Some(statement);
        return Ok(expression);
    }
    ensure_statement_owned_expression(arena, expression, statement)
}

fn arena_expression_reaches(
    arena: &SemanticExpressionArena,
    root: SemanticExprId,
    target: SemanticExprId,
) -> Result<bool, ExpansionError> {
    let mut pending = vec![root];
    let mut visited = BTreeSet::new();
    while let Some(id) = pending.pop() {
        if !visited.insert(id) {
            continue;
        }
        if id == target {
            return Ok(true);
        }
        let expression = arena
            .expressions
            .get(id.as_usize())
            .filter(|candidate| candidate.id == id)
            .ok_or(ExpansionError::MissingExpression(CheckedExprId(
                id.as_usize() as u32,
            )))?;
        pending.extend(arena_expression_children(&expression.kind));
    }
    Ok(false)
}

fn arena_expression_children(kind: &SemanticExpressionKind) -> Vec<SemanticExprId> {
    match kind {
        SemanticExpressionKind::CanonicalRead { .. }
        | SemanticExpressionKind::LocalRead { .. }
        | SemanticExpressionKind::ExternalRead { .. }
        | SemanticExpressionKind::ElementState { .. }
        | SemanticExpressionKind::Drain { .. }
        | SemanticExpressionKind::Text(_)
        | SemanticExpressionKind::Number(_)
        | SemanticExpressionKind::Bits(_)
        | SemanticExpressionKind::BytesByte(_)
        | SemanticExpressionKind::Absent
        | SemanticExpressionKind::Tag(_)
        | SemanticExpressionKind::Source { .. }
        | SemanticExpressionKind::Materialize { .. }
        | SemanticExpressionKind::Delimiter
        | SemanticExpressionKind::MaterializationLocal { .. }
        | SemanticExpressionKind::FunctionParameter { .. } => Vec::new(),
        SemanticExpressionKind::TextTemplate { segments } => segments
            .iter()
            .filter_map(|segment| match segment {
                SemanticTextSegment::Static { .. } => None,
                SemanticTextSegment::Dynamic { value } => Some(*value),
            })
            .collect(),
        SemanticExpressionKind::TaggedObject { fields, .. }
        | SemanticExpressionKind::Object(fields) => {
            fields.iter().map(|field| field.value).collect()
        }
        SemanticExpressionKind::Call { arguments, .. } => {
            arguments.iter().map(|argument| argument.value).collect()
        }
        SemanticExpressionKind::Flush { payload: input }
        | SemanticExpressionKind::FlushBoundary { input }
        | SemanticExpressionKind::Draining { input }
        | SemanticExpressionKind::Project { input, .. } => vec![*input],
        SemanticExpressionKind::Hold {
            initial, updates, ..
        } => std::iter::once(*initial)
            .chain(updates.iter().copied())
            .collect(),
        SemanticExpressionKind::Latest { branches } => branches.clone(),
        SemanticExpressionKind::When { input, arms, .. } => std::iter::once(*input)
            .chain(arms.iter().map(|arm| arm.output))
            .collect(),
        SemanticExpressionKind::Then { input, output } => {
            std::iter::once(*input).chain(*output).collect()
        }
        SemanticExpressionKind::Infix { left, right, .. } => vec![*left, *right],
        SemanticExpressionKind::MapEntry { key, value } => vec![*key, *value],
        SemanticExpressionKind::MatchArm { output, .. } => output.iter().copied().collect(),
        SemanticExpressionKind::Block { bindings, result } => bindings
            .iter()
            .map(|binding| binding.value)
            .chain(std::iter::once(*result))
            .collect(),
        SemanticExpressionKind::List { items, .. }
        | SemanticExpressionKind::Bytes { items, .. }
        | SemanticExpressionKind::Map { entries: items }
        | SemanticExpressionKind::Set { items } => items.clone(),
    }
}

#[allow(clippy::too_many_arguments)]
fn ensure_resource_definition_statement(
    program: &CheckedProgram,
    lookup: &CheckedProgramLookup,
    semantic_scope_ids: &BTreeMap<boon_typecheck::LexicalScopeId, SemanticScopeId>,
    arena: &mut SemanticExpressionArena,
    statements: &mut Vec<SemanticStatement>,
    expression: SemanticExprId,
    suggested_statement: Option<SemanticStatementId>,
    checked_statement: boon_typecheck::CheckedStatementId,
    declaration: DeclId,
    checked_binding: CheckedResourceBinding,
) -> Result<(SemanticExprId, SemanticStatementId), ExpansionError> {
    let origin = arena
        .checked_expression_origins
        .get(expression.as_usize())
        .filter(|origin| origin.expression == expression)
        .cloned()
        .ok_or_else(|| {
            ExpansionError::InvalidLocalBindings(format!(
                "resource expression {expression} has no exact semantic origin"
            ))
        })?;
    let suggested_definition = suggested_statement.and_then(|suggested_statement| {
        statements
            .get(suggested_statement.as_usize())
            .filter(|statement| statement.id == suggested_statement)
    });
    let resource_needs_distinct_producer_statement =
        matches!(&checked_binding, CheckedResourceBinding::State { .. })
            && suggested_definition.is_some_and(|statement| {
                matches!(
                    statement.origin,
                    SemanticStatementOrigin::ProducerResult { .. }
                )
            });
    let suggested_reaches_expression = suggested_definition
        .and_then(|statement| statement.value)
        .map(|root| arena_expression_reaches(arena, root, expression))
        .transpose()?
        .unwrap_or(true);
    if let Some(suggested_statement) = suggested_definition.and_then(|statement| {
        (statement.declaration == Some(declaration)
            && !resource_needs_distinct_producer_statement
            && suggested_reaches_expression)
            .then_some(statement.id)
    }) {
        let checked_expression = arena
            .expressions
            .get(expression.as_usize())
            .filter(|candidate| candidate.id == expression)
            .map(|candidate| candidate.checked_expr_id)
            .ok_or(ExpansionError::MissingExpression(CheckedExprId(
                expression.as_usize() as u32,
            )))?;
        let selected_owner = arena.expressions[expression.as_usize()].owner;
        let owned_candidates = arena
            .expressions
            .iter()
            .zip(&arena.checked_expression_origins)
            .filter(|(candidate, candidate_origin)| {
                candidate.checked_expr_id == checked_expression
                    && candidate.owner == selected_owner
                    && candidate_origin.expression == candidate.id
                    && candidate_origin.owning_statement == Some(suggested_statement)
                    && candidate_origin.call_instance == origin.call_instance
                    && match &checked_binding {
                        CheckedResourceBinding::Source { .. } => {
                            matches!(candidate.kind, SemanticExpressionKind::Source { .. })
                        }
                        CheckedResourceBinding::State { .. } => {
                            matches!(candidate.kind, SemanticExpressionKind::Hold { .. })
                        }
                        CheckedResourceBinding::ListAuthority { .. }
                        | CheckedResourceBinding::ListAlias { .. } => false,
                    }
            })
            .map(|(candidate, _)| candidate.id)
            .collect::<Vec<_>>();
        let statement_value_candidate = statements
            .get(suggested_statement.as_usize())
            .and_then(|statement| statement.value)
            .and_then(|value| {
                arena
                    .expressions
                    .get(value.as_usize())
                    .filter(|candidate| {
                        candidate.id == value
                            && candidate.checked_expr_id == checked_expression
                            && candidate.owner == selected_owner
                            && arena
                                .checked_expression_origins
                                .get(candidate.id.as_usize())
                                .filter(|candidate_origin| {
                                    candidate_origin.expression == candidate.id
                                })
                                .is_some_and(|candidate_origin| {
                                    candidate_origin.call_instance == origin.call_instance
                                })
                            && match &checked_binding {
                                CheckedResourceBinding::Source { .. } => {
                                    matches!(candidate.kind, SemanticExpressionKind::Source { .. })
                                }
                                CheckedResourceBinding::State { .. } => {
                                    matches!(candidate.kind, SemanticExpressionKind::Hold { .. })
                                }
                                CheckedResourceBinding::ListAuthority { .. }
                                | CheckedResourceBinding::ListAlias { .. } => false,
                            }
                    })
                    .map(|candidate| candidate.id)
            });
        let selected_is_reachable = if let Some(root) = statements
            .get(suggested_statement.as_usize())
            .and_then(|statement| statement.value)
        {
            arena_expression_reaches(arena, root, expression)?
        } else {
            false
        };
        let expression = if selected_is_reachable {
            expression
        } else if let Some(value) = statement_value_candidate {
            value
        } else {
            match owned_candidates.as_slice() {
                [owned] => *owned,
                [] => ensure_statement_owned_expression(arena, expression, suggested_statement)?,
                _ => {
                    return Err(ExpansionError::InvalidLocalBindings(format!(
                        "resource declaration {} checked expression {} has {} statement-owned semantic definitions",
                        declaration.0,
                        checked_expression.0,
                        owned_candidates.len()
                    )));
                }
            }
        };
        let statement = statements
            .get_mut(suggested_statement.as_usize())
            .filter(|statement| statement.id == suggested_statement)
            .ok_or_else(|| {
                ExpansionError::InvalidLocalBindings(format!(
                    "resource declaration {} references missing semantic statement {suggested_statement}",
                    declaration.0
                ))
            })?;
        // The declaration value can intentionally wrap the durable resource
        // expression (for example `HOLD ... |> DRAINING`). Keep that complete
        // value as the reactive binding producer while giving the exact
        // source/state expression statement ownership of its own.
        if statement.value.is_none() {
            statement.value = Some(expression);
        }
        if !statement.checked_resources.contains(&checked_binding) {
            statement.checked_resources.push(checked_binding);
        }
        return Ok((expression, suggested_statement));
    }

    let checked = program
        .statements
        .iter()
        .find(|statement| statement.id == checked_statement)
        .ok_or_else(|| {
            ExpansionError::InvalidLocalBindings(format!(
                "resource declaration {} references missing checked statement {}",
                declaration.0, checked_statement.0
            ))
        })?;
    let declaration_parts = |declaration: Option<DeclId>| {
        declaration
            .and_then(|declaration| {
                let checked = lookup.declaration(program, declaration)?;
                Some((
                    checked.name.clone(),
                    canonical_declaration_path(program, lookup, declaration)?,
                ))
            })
            .unzip()
    };
    let kind = match &checked.kind {
        boon_typecheck::CheckedStatementKind::Function { .. } => {
            let (name, path) = declaration_parts(Some(declaration));
            match &checked_binding {
                CheckedResourceBinding::Source { .. } => SemanticStatementKind::Source {
                    name,
                    path,
                    event: None,
                },
                CheckedResourceBinding::State { .. } => {
                    let hold_name = arena
                        .expressions
                        .get(expression.as_usize())
                        .filter(|candidate| candidate.id == expression)
                        .and_then(|expression| match &expression.kind {
                            SemanticExpressionKind::Hold { name, .. } => Some(name.clone()),
                            _ => None,
                        });
                    SemanticStatementKind::Hold {
                        name,
                        path,
                        hold_name,
                    }
                }
                CheckedResourceBinding::ListAuthority { .. }
                | CheckedResourceBinding::ListAlias { .. } => {
                    return Err(ExpansionError::InvalidLocalBindings(format!(
                        "list resource declaration {} resolves to function statement {}",
                        declaration.0, checked_statement.0
                    )));
                }
            }
        }
        boon_typecheck::CheckedStatementKind::Field { declaration } => {
            let (name, path) = declaration_parts(Some(*declaration));
            SemanticStatementKind::Field {
                name: name.ok_or(ExpansionError::MissingDeclaration(*declaration))?,
                path: path.ok_or(ExpansionError::MissingDeclaration(*declaration))?,
            }
        }
        boon_typecheck::CheckedStatementKind::Source { declaration, event } => {
            let (name, path) = declaration_parts(*declaration);
            SemanticStatementKind::Source {
                name,
                path,
                event: event.clone(),
            }
        }
        boon_typecheck::CheckedStatementKind::Hold {
            declaration,
            name: hold_name,
        } => {
            let (name, path) = declaration_parts(*declaration);
            SemanticStatementKind::Hold {
                name,
                path,
                hold_name: hold_name.clone(),
            }
        }
        boon_typecheck::CheckedStatementKind::List {
            declaration,
            capacity,
        } => {
            let (name, path) = declaration_parts(*declaration);
            SemanticStatementKind::List {
                name,
                path,
                capacity: *capacity,
            }
        }
        boon_typecheck::CheckedStatementKind::Block => SemanticStatementKind::Block,
        boon_typecheck::CheckedStatementKind::Spread => SemanticStatementKind::Spread,
        boon_typecheck::CheckedStatementKind::Expression => SemanticStatementKind::Expression,
    };
    let statement = SemanticStatementId(statements.len());
    let expression =
        ensure_resource_statement_owned_expression(arena, statements, expression, statement)?;
    let flow_type = arena
        .expressions
        .get(expression.as_usize())
        .filter(|candidate| candidate.id == expression)
        .map(|expression| expression.flow_type.clone())
        .ok_or(ExpansionError::MissingExpression(CheckedExprId(
            expression.as_usize() as u32,
        )))?;
    statements.push(SemanticStatement {
        id: statement,
        origin: SemanticStatementOrigin::Checked {
            statement: checked_statement,
        },
        scope: semantic_scope_ids
            .get(&checked.scope_id)
            .copied()
            .ok_or_else(|| {
                ExpansionError::InvalidLocalBindings(format!(
                    "resource checked statement {} references missing semantic scope {}",
                    checked_statement.0, checked.scope_id.0
                ))
            })?,
        parent: None,
        call_instance: origin.call_instance,
        span: checked.span,
        checked_resources: vec![checked_binding],
        declaration: Some(declaration),
        flow_type: Some(flow_type),
        kind,
        value: Some(expression),
        value_use: match checked.value_use {
            CheckedValueUse::RuntimeValue => SemanticMaterializationResultKind::RuntimeValue,
            CheckedValueUse::RenderSlot => SemanticMaterializationResultKind::RenderSlot,
        },
        children: Vec::new(),
    });
    Ok((expression, statement))
}

fn remap_checked_resource_ids(
    program: &CheckedProgram,
    expressions: &mut [SemanticExpression],
    origins: &[SemanticExpressionOrigin],
    semantic_sources: &BTreeMap<
        (
            boon_typecheck::CheckedSourceId,
            Option<StaticOwnerId>,
            Option<OutCallInstanceId>,
        ),
        SemanticSourceId,
    >,
    semantic_states: &BTreeMap<
        (
            boon_typecheck::CheckedStateId,
            Option<StaticOwnerId>,
            Option<OutCallInstanceId>,
        ),
        SemanticStateId,
    >,
) -> Result<(), ExpansionError> {
    let checked_sources = program
        .sources
        .iter()
        .map(|source| (provisional_semantic_source_id(source.id), source.id))
        .collect::<BTreeMap<_, _>>();
    let checked_states = program
        .states
        .iter()
        .map(|state| (provisional_semantic_state_id(state.id), state.id))
        .collect::<BTreeMap<_, _>>();
    let source_instance = |checked, owner| {
        semantic_sources
            .get(&(checked, owner, None))
            .copied()
            .map(|source| (source, owner))
    };
    let state_instance = |checked, owner| {
        semantic_states
            .get(&(checked, owner, None))
            .copied()
            .map(|state| (state, owner))
    };

    if origins.len() != expressions.len() {
        return Err(ExpansionError::InvalidLocalBindings(format!(
            "semantic resource remap has {} expressions but {} exact origins",
            expressions.len(),
            origins.len()
        )));
    }
    for (index, expression) in expressions.iter_mut().enumerate() {
        let frame = origins
            .get(index)
            .filter(|origin| origin.expression == expression.id)
            .ok_or_else(|| {
                ExpansionError::InvalidLocalBindings(format!(
                    "semantic expression {} has no dense exact origin during resource remap",
                    expression.id
                ))
            })?
            .call_instance;
        for member in &mut expression.provenance.members {
            match &mut member.origin {
                SemanticValueOrigin::Source { source, owner } => {
                    let checked = checked_sources.get(source).copied().ok_or_else(|| {
                        ExpansionError::InvalidLocalBindings(format!(
                            "expression {} has unknown provisional source {}",
                            expression.id, source
                        ))
                    })?;
                    let (resolved, resolved_owner) =
                        source_instance(checked, *owner).ok_or_else(|| {
                            let available = semantic_sources
                                .iter()
                                .filter_map(
                                    |((candidate, candidate_owner, candidate_frame), source)| {
                                        (*candidate == checked).then_some((
                                            *candidate_owner,
                                            *candidate_frame,
                                            *source,
                                        ))
                                    },
                                )
                                .collect::<Vec<_>>();
                            ExpansionError::InvalidLocalBindings(format!(
                                "expression {} has no semantic source instance for checked source {:?}, owner {:?}, frame {:?}; available instances: {available:?}",
                                expression.id, checked, owner, frame
                            ))
                        })?;
                    *source = resolved;
                    *owner = resolved_owner;
                }
                SemanticValueOrigin::State { state, owner } => {
                    let checked = checked_states.get(state).copied().ok_or_else(|| {
                        ExpansionError::InvalidLocalBindings(format!(
                            "expression {} has unknown provisional state {}",
                            expression.id, state
                        ))
                    })?;
                    let (resolved, resolved_owner) =
                        state_instance(checked, *owner).ok_or_else(|| {
                            let available = semantic_states
                                .iter()
                                .filter_map(
                                    |((candidate, candidate_owner, candidate_frame), state)| {
                                        (*candidate == checked).then_some((
                                            *candidate_owner,
                                            *candidate_frame,
                                            *state,
                                        ))
                                    },
                                )
                                .collect::<Vec<_>>();
                            ExpansionError::InvalidLocalBindings(format!(
                                "expression {} has no semantic state instance for checked state {:?}, owner {:?}, frame {:?}; available instances: {available:?}",
                                expression.id, checked, owner, frame
                            ))
                        })?;
                    *state = resolved;
                    *owner = resolved_owner;
                }
                SemanticValueOrigin::Runtime
                | SemanticValueOrigin::ProducerSource { .. }
                | SemanticValueOrigin::MaterializationLocal { .. } => {}
            }
        }
        if let SemanticExpressionKind::CanonicalRead {
            source: Some(source),
            ..
        } = &mut expression.kind
        {
            let checked = checked_sources
                .get(&source.source)
                .copied()
                .ok_or_else(|| {
                    ExpansionError::InvalidLocalBindings(format!(
                        "expression {} has unknown provisional source read {}",
                        expression.id, source.source
                    ))
                })?;
            source.source = [expression.owner, None]
                .into_iter()
                .find_map(|owner| source_instance(checked, owner).map(|(source, _)| source))
                .ok_or_else(|| {
                    let available = semantic_sources
                        .iter()
                        .filter_map(
                            |((candidate, owner, frame), source)| {
                                (*candidate == checked).then_some((*owner, *frame, *source))
                            },
                        )
                        .collect::<Vec<_>>();
                    ExpansionError::InvalidLocalBindings(format!(
                        "expression {} has no semantic source read instance for checked source {:?}, owner {:?}, frame {:?}; available instances: {available:?}",
                        expression.id, checked, expression.owner, frame
                    ))
                })?;
        }
    }
    Ok(())
}

fn concrete_state_initial_expression(
    expressions: &[SemanticExpression],
    root: SemanticExprId,
) -> Option<SemanticExprId> {
    let mut current = root;
    let mut visited = BTreeSet::new();
    while visited.insert(current) {
        let expression = expressions
            .get(current.as_usize())
            .filter(|candidate| candidate.id == current)?;
        current = match &expression.kind {
            SemanticExpressionKind::Hold { initial, .. } => *initial,
            SemanticExpressionKind::Latest { branches } => *branches.first()?,
            SemanticExpressionKind::Call { arguments, .. } if expression.effect.writes_state => {
                arguments
                    .iter()
                    .min_by_key(|argument| argument.ordinal)
                    .map(|argument| argument.value)?
            }
            _ => return Some(current),
        };
    }
    None
}

fn executable_latest_has_initial(
    expressions: &[SemanticExpression],
    branches: &[SemanticExprId],
) -> bool {
    branches
        .first()
        .and_then(|branch| expressions.get(branch.as_usize()))
        .is_some_and(|branch| branch.flow_type.mode == FlowMode::Continuous)
}

fn synthesize_statement_owned_states(
    program: &CheckedProgram,
    lookup: &CheckedProgramLookup,
    arena: &mut SemanticExpressionArena,
    statements: &mut [SemanticStatement],
) -> Result<(), ExpansionError> {
    let statement_values = statements
        .iter()
        .map(|statement| (statement.id, (statement.value, statement.children.clone())))
        .collect::<BTreeMap<_, _>>();
    for statement in statements {
        let SemanticStatementKind::Hold {
            path,
            hold_name,
            name,
        } = &statement.kind
        else {
            continue;
        };
        let Some(initial) = statement.value else {
            continue;
        };
        let initial_expression = arena
            .expressions
            .get(initial.as_usize())
            .filter(|expression| expression.id == initial)
            .cloned()
            .ok_or(ExpansionError::MissingExpression(CheckedExprId(
                initial.0 as u32,
            )))?;
        let Some(declaration) = statement.declaration else {
            continue;
        };
        let owner = initial_expression.owner;
        if matches!(
            initial_expression.kind,
            SemanticExpressionKind::Hold { .. } | SemanticExpressionKind::Latest { .. }
        ) || matches!(
            initial_expression.kind,
            SemanticExpressionKind::Call {
                callable_kind: SemanticCallableKind::Builtin,
                ..
            } if initial_expression.effect.writes_state
        ) {
            continue;
        }
        if let Some(existing) = arena.expressions.iter().find(|expression| {
            expression.owner == owner
                && resource_declaration(program, lookup, expression.checked_expr_id)
                    == Some(declaration)
                && (matches!(expression.kind, SemanticExpressionKind::Hold { .. })
                    || matches!(
                        &expression.kind,
                        SemanticExpressionKind::Latest { branches }
                            if executable_latest_has_initial(&arena.expressions, branches)
                    )
                    || matches!(
                        expression.kind,
                        SemanticExpressionKind::Call {
                            callable_kind: SemanticCallableKind::Builtin,
                            ..
                        } if expression.effect.writes_state
                    ))
        }) {
            statement.value = Some(existing.id);
            continue;
        }
        let mut pending = statement.children.iter().rev().copied().collect::<Vec<_>>();
        let mut visited = BTreeSet::new();
        let mut updates = Vec::new();
        while let Some(child) = pending.pop() {
            if !visited.insert(child) {
                continue;
            }
            let Some((value, children)) = statement_values.get(&child) else {
                continue;
            };
            match value {
                Some(value) if *value != initial => updates.push(*value),
                _ => pending.extend(children.iter().rev().copied()),
            }
        }
        updates.dedup();
        let binding_path = path
            .clone()
            .or_else(|| name.clone())
            .or_else(|| hold_name.clone())
            .unwrap_or_default();
        let state_name = hold_name
            .clone()
            .or_else(|| name.clone())
            .unwrap_or_else(|| binding_path.clone());
        let provenance = program
            .states
            .iter()
            .find(|state| {
                state.declaration == declaration
                    && state.expression == initial_expression.checked_expr_id
            })
            .map_or_else(runtime_value_provenance, |state| SemanticValueProvenance {
                members: vec![SemanticValueMember {
                    path: Vec::new(),
                    origin: SemanticValueOrigin::State {
                        state: provisional_semantic_state_id(state.id),
                        owner: initial_expression.owner,
                    },
                }],
            });
        let initial_origin = arena
            .checked_expression_origins
            .get(initial.as_usize())
            .cloned()
            .ok_or(ExpansionError::MissingExpression(
                initial_expression.checked_expr_id,
            ))?;
        let call_instance = initial_origin.call_instance;
        let checked_expression = initial_expression.checked_expr_id;
        let flow_type = statement
            .flow_type
            .clone()
            .unwrap_or_else(|| initial_expression.flow_type.clone());
        let expression_owner = initial_expression.owner;
        let id = arena.push(
            checked_expression,
            initial_origin.checked_scope,
            initial_origin.checked_span,
            call_instance,
            Some(statement.id),
            |id, value_id| SemanticExpression {
                id,
                value_id,
                checked_expr_id: initial_expression.checked_expr_id,
                flow_type,
                effect: boon_typecheck::CheckedEffectSummary {
                    reads_state: true,
                    writes_state: true,
                    ..boon_typecheck::CheckedEffectSummary::default()
                },
                owner: expression_owner,
                provenance,
                resource_binding_path: Some(binding_path.clone()),
                kind: SemanticExpressionKind::Hold {
                    initial,
                    name: state_name,
                    binding_path,
                    updates,
                },
            },
        );
        statement.value = Some(id);
    }
    Ok(())
}

fn append_expression_arena_without_roots(
    target: &mut SemanticExpressionArena,
    mut source: SemanticExpressionArena,
) {
    let offset = target.expressions.len();
    let local_binding_offset = target
        .expressions
        .iter()
        .flat_map(|expression| match &expression.kind {
            SemanticExpressionKind::Block { bindings, .. } => {
                bindings.iter().map(|binding| binding.id).collect()
            }
            _ => Vec::new(),
        })
        .map(SemanticLocalBindingId::as_usize)
        .max()
        .map_or(0, |maximum| maximum + 1);
    for expression in &mut source.expressions {
        expression.id = rebase_expr_id(expression.id, offset);
        expression.value_id = SemanticValueId(expression.id.as_usize());
        rebase_expression_kind(&mut expression.kind, offset, local_binding_offset);
    }
    for origin in &mut source.checked_expression_origins {
        origin.expression = rebase_expr_id(origin.expression, offset);
    }
    target.expressions.extend(source.expressions);
    target
        .checked_expression_origins
        .extend(source.checked_expression_origins);
}

fn rebase_expr_id(expression: SemanticExprId, offset: usize) -> SemanticExprId {
    SemanticExprId(expression.as_usize() + offset)
}

fn rebase_expression_kind(
    kind: &mut SemanticExpressionKind,
    offset: usize,
    local_binding_offset: usize,
) {
    let rebase = |expression: &mut SemanticExprId| {
        *expression = rebase_expr_id(*expression, offset);
    };
    match kind {
        SemanticExpressionKind::CanonicalRead { .. }
        | SemanticExpressionKind::ExternalRead { .. }
        | SemanticExpressionKind::ElementState { .. }
        | SemanticExpressionKind::Drain { .. }
        | SemanticExpressionKind::Text(_)
        | SemanticExpressionKind::Number(_)
        | SemanticExpressionKind::Bits(_)
        | SemanticExpressionKind::BytesByte(_)
        | SemanticExpressionKind::Absent
        | SemanticExpressionKind::Tag(_)
        | SemanticExpressionKind::Source { .. }
        | SemanticExpressionKind::Materialize { .. }
        | SemanticExpressionKind::Delimiter
        | SemanticExpressionKind::MaterializationLocal { .. }
        | SemanticExpressionKind::FunctionParameter { .. } => {}
        SemanticExpressionKind::LocalRead { binding, .. } => {
            *binding = SemanticLocalBindingId(binding.as_usize() + local_binding_offset);
        }
        SemanticExpressionKind::TextTemplate { segments } => {
            for value in segments.iter_mut().filter_map(|segment| match segment {
                SemanticTextSegment::Static { .. } => None,
                SemanticTextSegment::Dynamic { value } => Some(value),
            }) {
                rebase(value);
            }
        }
        SemanticExpressionKind::TaggedObject { fields, .. }
        | SemanticExpressionKind::Object(fields) => {
            for field in fields {
                rebase(&mut field.value);
            }
        }
        SemanticExpressionKind::Block { bindings, result } => {
            for binding in bindings {
                binding.id = SemanticLocalBindingId(binding.id.as_usize() + local_binding_offset);
                rebase(&mut binding.value);
            }
            rebase(result);
        }
        SemanticExpressionKind::Call {
            arguments,
            parameter_bindings,
            ..
        } => {
            for argument in arguments {
                rebase(&mut argument.value);
            }
            for binding in parameter_bindings {
                if let SemanticCallParameterBindingKind::Explicit { value, .. } = &mut binding.kind
                {
                    rebase(value);
                }
            }
        }
        SemanticExpressionKind::Flush { payload: input }
        | SemanticExpressionKind::FlushBoundary { input }
        | SemanticExpressionKind::Draining { input }
        | SemanticExpressionKind::Project { input, .. } => rebase(input),
        SemanticExpressionKind::Hold {
            initial, updates, ..
        } => {
            rebase(initial);
            for update in updates {
                rebase(update);
            }
        }
        SemanticExpressionKind::Latest { branches } => {
            for branch in branches {
                rebase(branch);
            }
        }
        SemanticExpressionKind::When { input, arms, .. } => {
            rebase(input);
            for arm in arms {
                rebase(&mut arm.output);
            }
        }
        SemanticExpressionKind::Then { input, output } => {
            rebase(input);
            if let Some(output) = output {
                rebase(output);
            }
        }
        SemanticExpressionKind::Infix { left, right, .. } => {
            rebase(left);
            rebase(right);
        }
        SemanticExpressionKind::MapEntry { key, value } => {
            rebase(key);
            rebase(value);
        }
        SemanticExpressionKind::MatchArm { output, .. } => {
            if let Some(output) = output {
                rebase(output);
            }
        }
        SemanticExpressionKind::List { items, .. }
        | SemanticExpressionKind::Bytes { items, .. }
        | SemanticExpressionKind::Map { entries: items }
        | SemanticExpressionKind::Set { items } => {
            for item in items {
                rebase(item);
            }
        }
    }
}

fn contextual_operation_formals(
    operation: CheckedContextualOperation,
) -> (
    SemanticContextualOperationKind,
    DeclId,
    DeclId,
    DeclId,
    Option<DeclId>,
) {
    match operation {
        CheckedContextualOperation::Map { list, row, body } => {
            (SemanticContextualOperationKind::Map, list, row, body, None)
        }
        CheckedContextualOperation::Filter {
            list,
            row,
            predicate,
        } => (
            SemanticContextualOperationKind::Filter,
            list,
            row,
            predicate,
            None,
        ),
        CheckedContextualOperation::Retain {
            list,
            row,
            predicate,
        } => (
            SemanticContextualOperationKind::Retain,
            list,
            row,
            predicate,
            None,
        ),
        CheckedContextualOperation::Remove {
            list,
            row,
            predicate,
        } => (
            SemanticContextualOperationKind::Remove,
            list,
            row,
            predicate,
            None,
        ),
        CheckedContextualOperation::Every {
            list,
            row,
            predicate,
        } => (
            SemanticContextualOperationKind::Every,
            list,
            row,
            predicate,
            None,
        ),
        CheckedContextualOperation::Any {
            list,
            row,
            predicate,
        } => (
            SemanticContextualOperationKind::Any,
            list,
            row,
            predicate,
            None,
        ),
        CheckedContextualOperation::Find {
            list,
            row,
            predicate,
        } => (
            SemanticContextualOperationKind::Find,
            list,
            row,
            predicate,
            None,
        ),
        CheckedContextualOperation::SortBy {
            list,
            row,
            key,
            direction,
        } => (
            SemanticContextualOperationKind::SortBy,
            list,
            row,
            key,
            Some(direction),
        ),
        CheckedContextualOperation::ThenBy {
            list,
            row,
            key,
            direction,
        } => (
            SemanticContextualOperationKind::ThenBy,
            list,
            row,
            key,
            Some(direction),
        ),
    }
}

fn checked_scope(
    program: &CheckedProgram,
    scope: boon_typecheck::LexicalScopeId,
) -> Option<&boon_typecheck::CheckedScope> {
    program
        .scopes
        .iter()
        .find(|candidate| candidate.id == scope)
}

fn declaration_in_exact_scope(
    lookup: &CheckedProgramLookup,
    scope: boon_typecheck::LexicalScopeId,
    name: &str,
) -> Option<DeclId> {
    lookup.declaration_in_exact_scope(scope, name)
}

fn declaration_in_lexical_scope(
    program: &CheckedProgram,
    lookup: &CheckedProgramLookup,
    mut scope: boon_typecheck::LexicalScopeId,
    name: &str,
) -> Option<DeclId> {
    let mut visited = BTreeSet::new();
    while visited.insert(scope) {
        if let Some(declaration) = declaration_in_exact_scope(lookup, scope, name) {
            return Some(declaration);
        }
        scope = checked_scope(program, scope).and_then(|scope| scope.parent)?;
    }
    None
}

fn declaration_is_function_local(
    program: &CheckedProgram,
    mut scope: boon_typecheck::LexicalScopeId,
) -> bool {
    let mut visited = BTreeSet::new();
    while visited.insert(scope) {
        let Some(current) = checked_scope(program, scope) else {
            return false;
        };
        if current.kind == boon_typecheck::CheckedScopeKind::Function {
            return true;
        }
        let Some(parent) = current.parent else {
            return false;
        };
        scope = parent;
    }
    false
}

fn canonical_declaration_path(
    program: &CheckedProgram,
    lookup: &CheckedProgramLookup,
    target: DeclId,
) -> Option<String> {
    let declaration = lookup.declaration(program, target)?;
    let mut segments = vec![declaration.name.clone()];
    let mut scope = declaration.scope_id;
    let mut visited = BTreeSet::new();
    while scope != program.root_scope && visited.insert(scope) {
        let current = lookup.scope(program, scope)?;
        if current.kind == boon_typecheck::CheckedScopeKind::Function {
            break;
        }
        if let Some(owner) = current.owner
            && let Some(owner) = lookup.declaration(program, owner)
            && matches!(
                owner.kind,
                boon_typecheck::CheckedDeclarationKind::Field
                    | boon_typecheck::CheckedDeclarationKind::Source
                    | boon_typecheck::CheckedDeclarationKind::Hold
                    | boon_typecheck::CheckedDeclarationKind::List
            )
        {
            segments.push(owner.name.clone());
        }
        scope = current.parent?;
    }
    segments.reverse();
    Some(segments.join("."))
}

fn resource_declaration(
    program: &CheckedProgram,
    lookup: &CheckedProgramLookup,
    expression: CheckedExprId,
) -> Option<DeclId> {
    lookup.expression(program, expression)?.declaration
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct ExpansionKey {
    expression: CheckedExprId,
    frame: Option<OutCallInstanceId>,
    value_frame: Option<usize>,
    evaluation_owner: Option<StaticOwnerId>,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum SemanticValueFrameKey {
    Block {
        expression: CheckedExprId,
        frame: Option<OutCallInstanceId>,
        parent: Option<usize>,
        owner: Option<StaticOwnerId>,
    },
    SelectArm {
        arm: CheckedExprId,
        frame: Option<OutCallInstanceId>,
        parent: Option<usize>,
        owner: Option<StaticOwnerId>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum SemanticValueBinding {
    Local(SemanticLocalBindingId),
    Projection {
        input: ScopedCheckedExpr,
        fields: Vec<String>,
    },
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct SemanticValueFrame {
    bindings: BTreeMap<DeclId, SemanticValueBinding>,
    local_ids: BTreeMap<DeclId, SemanticLocalBindingId>,
}

fn runtime_value_provenance() -> SemanticValueProvenance {
    SemanticValueProvenance {
        members: vec![SemanticValueMember {
            path: Vec::new(),
            origin: SemanticValueOrigin::Runtime,
        }],
    }
}

fn normalize_value_provenance(mut provenance: SemanticValueProvenance) -> SemanticValueProvenance {
    provenance.members.sort();
    provenance.members.dedup();
    provenance
}

fn record_value_provenance(
    expressions: &[SemanticExpression],
    fields: &[SemanticRecordField],
) -> SemanticValueProvenance {
    if fields.is_empty() {
        return runtime_value_provenance();
    }
    let mut members = Vec::new();
    for field in fields {
        let Some(value) = expressions.get(field.value.as_usize()) else {
            continue;
        };
        for mut member in value.provenance.members.clone() {
            if !field.spread {
                member.path.insert(0, field.name.clone());
            }
            members.push(member);
        }
    }
    normalize_value_provenance(SemanticValueProvenance { members })
}

struct LocalProvenanceResolver<'a> {
    expressions: &'a [SemanticExpression],
    bindings: BTreeMap<SemanticLocalBindingId, (DeclId, SemanticExprId)>,
    declarations: BTreeMap<(DeclId, Option<StaticOwnerId>), SemanticExprId>,
    cache: BTreeMap<SemanticExprId, SemanticValueProvenance>,
}

impl<'a> LocalProvenanceResolver<'a> {
    fn resolve(
        &mut self,
        expression_id: SemanticExprId,
    ) -> Result<SemanticValueProvenance, ExpansionError> {
        if let Some(cached) = self.cache.get(&expression_id) {
            return Ok(cached.clone());
        }
        #[derive(Clone, Copy)]
        enum Work {
            Enter(SemanticExprId),
            Finish(SemanticExprId),
        }

        let mut work = vec![Work::Enter(expression_id)];
        let mut visiting = BTreeSet::new();
        while let Some(next) = work.pop() {
            match next {
                Work::Enter(id) => {
                    if self.cache.contains_key(&id) {
                        continue;
                    }
                    if !visiting.insert(id) {
                        return Err(ExpansionError::InvalidLocalBindings(format!(
                            "provenance cycle reaches expression {id}"
                        )));
                    }
                    let expression = self
                        .expressions
                        .get(id.as_usize())
                        .filter(|candidate| candidate.id == id)
                        .ok_or_else(|| {
                            ExpansionError::InvalidLocalBindings(format!(
                                "provenance references missing expression {id}"
                            ))
                        })?;
                    let owner = expression.owner;
                    let mut dependencies = match &expression.kind {
                        SemanticExpressionKind::LocalRead {
                            binding,
                            declaration,
                            ..
                        } => {
                            let (bound_declaration, value) =
                                self.bindings.get(binding).copied().ok_or_else(|| {
                                    ExpansionError::InvalidLocalBindings(format!(
                                        "expression {id} references missing local binding {binding}"
                                    ))
                                })?;
                            if bound_declaration != *declaration {
                                return Err(ExpansionError::InvalidLocalBindings(format!(
                                    "expression {id} declaration {} differs from binding {binding} declaration {}",
                                    declaration.0, bound_declaration.0
                                )));
                            }
                            vec![value]
                        }
                        SemanticExpressionKind::Object(fields)
                        | SemanticExpressionKind::TaggedObject { fields, .. } => {
                            fields.iter().map(|field| field.value).collect()
                        }
                        SemanticExpressionKind::Block { result, .. }
                        | SemanticExpressionKind::Draining { input: result }
                        | SemanticExpressionKind::Project { input: result, .. } => vec![*result],
                        SemanticExpressionKind::CanonicalRead { target, .. }
                        | SemanticExpressionKind::Drain { target, .. } => self
                            .declarations
                            .get(&(*target, owner))
                            .or_else(|| self.declarations.get(&(*target, None)))
                            .copied()
                            .filter(|value| *value != id)
                            .into_iter()
                            .collect(),
                        SemanticExpressionKind::Latest { branches } => branches.clone(),
                        SemanticExpressionKind::When { arms, .. } => {
                            arms.iter().map(|arm| arm.output).collect()
                        }
                        SemanticExpressionKind::Then {
                            output: Some(output),
                            ..
                        }
                        | SemanticExpressionKind::MatchArm {
                            output: Some(output),
                            ..
                        } => vec![*output],
                        _ => Vec::new(),
                    };
                    work.push(Work::Finish(id));
                    dependencies.reverse();
                    work.extend(dependencies.into_iter().map(Work::Enter));
                }
                Work::Finish(id) => {
                    let expression = self
                        .expressions
                        .get(id.as_usize())
                        .filter(|candidate| candidate.id == id)
                        .cloned()
                        .ok_or_else(|| {
                            ExpansionError::InvalidLocalBindings(format!(
                                "provenance references missing expression {id}"
                            ))
                        })?;
                    let cached = |dependency: SemanticExprId| {
                        self.cache.get(&dependency).cloned().ok_or_else(|| {
                            ExpansionError::InvalidLocalBindings(format!(
                                "expression {id} has unresolved provenance dependency {dependency}"
                            ))
                        })
                    };
                    let owner = expression.owner;
                    let resolved = match expression.kind {
                        SemanticExpressionKind::LocalRead {
                            binding,
                            projection,
                            ..
                        } => {
                            let (_, value) =
                                self.bindings.get(&binding).copied().ok_or_else(|| {
                                    ExpansionError::InvalidLocalBindings(format!(
                                        "expression {id} references missing local binding {binding}"
                                    ))
                                })?;
                            cached(value)?.projected(&projection)
                        }
                        SemanticExpressionKind::Object(fields)
                        | SemanticExpressionKind::TaggedObject { fields, .. } => {
                            if fields.is_empty() {
                                runtime_value_provenance()
                            } else {
                                let mut members = Vec::new();
                                for field in fields {
                                    for mut member in cached(field.value)?.members {
                                        if !field.spread {
                                            member.path.insert(0, field.name.clone());
                                        }
                                        members.push(member);
                                    }
                                }
                                normalize_value_provenance(SemanticValueProvenance { members })
                            }
                        }
                        SemanticExpressionKind::Block { result, .. }
                        | SemanticExpressionKind::Draining { input: result } => cached(result)?,
                        SemanticExpressionKind::Project { input, fields } => {
                            cached(input)?.projected(&fields)
                        }
                        SemanticExpressionKind::CanonicalRead {
                            target, projection, ..
                        }
                        | SemanticExpressionKind::Drain {
                            target, projection, ..
                        } => match self
                            .declarations
                            .get(&(target, owner))
                            .or_else(|| self.declarations.get(&(target, None)))
                            .copied()
                        {
                            Some(value) if value != id => cached(value)?.projected(&projection),
                            _ => expression.provenance,
                        },
                        SemanticExpressionKind::Latest { branches } => {
                            let mut members = Vec::new();
                            for branch in branches {
                                members.extend(cached(branch)?.members);
                            }
                            normalize_value_provenance(SemanticValueProvenance { members })
                        }
                        SemanticExpressionKind::When { arms, .. } => {
                            let mut members = Vec::new();
                            for arm in arms {
                                members.extend(cached(arm.output)?.members);
                            }
                            if members.is_empty() {
                                runtime_value_provenance()
                            } else {
                                normalize_value_provenance(SemanticValueProvenance { members })
                            }
                        }
                        SemanticExpressionKind::Then {
                            output: Some(output),
                            ..
                        }
                        | SemanticExpressionKind::MatchArm {
                            output: Some(output),
                            ..
                        } => cached(output)?,
                        _ => expression.provenance,
                    };
                    visiting.remove(&id);
                    self.cache.insert(id, resolved);
                }
            }
        }
        self.cache.get(&expression_id).cloned().ok_or_else(|| {
            ExpansionError::InvalidLocalBindings(format!(
                "provenance did not resolve expression {expression_id}"
            ))
        })
    }
}

fn resolve_executable_local_provenance(
    expressions: &mut [SemanticExpression],
    statements: &[SemanticStatement],
) -> Result<(), ExpansionError> {
    let mut bindings = BTreeMap::new();
    for binding in expressions
        .iter()
        .filter_map(|expression| match &expression.kind {
            SemanticExpressionKind::Block { bindings, .. } => Some(bindings.as_slice()),
            _ => None,
        })
        .flatten()
    {
        let value = (binding.declaration, binding.value);
        if let Some(previous) = bindings.insert(binding.id, value)
            && previous != value
        {
            return Err(ExpansionError::InvalidLocalBindings(format!(
                "binding {} has conflicting values {previous:?} and {value:?}",
                binding.id
            )));
        }
    }
    for (index, binding) in bindings.keys().copied().enumerate() {
        if binding != SemanticLocalBindingId(index) {
            return Err(ExpansionError::InvalidLocalBindings(format!(
                "binding at index {index} has non-dense ID {binding}"
            )));
        }
    }
    let mut declarations = BTreeMap::new();
    for statement in statements {
        let (Some(declaration), Some(value)) = (statement.declaration, statement.value) else {
            continue;
        };
        let owner = expressions
            .get(value.as_usize())
            .filter(|candidate| candidate.id == value)
            .and_then(|expression| expression.owner);
        let key = (declaration, owner);
        if let Some(previous) = declarations.insert(key, value)
            && previous != value
        {
            return Err(ExpansionError::InvalidLocalBindings(format!(
                "declaration {} owner {owner:?} has conflicting provenance values {previous} and {value}",
                declaration.0
            )));
        }
    }
    let snapshot = expressions.to_vec();
    let mut resolver = LocalProvenanceResolver {
        expressions: &snapshot,
        bindings,
        declarations,
        cache: BTreeMap::new(),
    };
    let provenances = snapshot
        .iter()
        .map(|expression| resolver.resolve(expression.id))
        .collect::<Result<Vec<_>, _>>()?;
    for (expression, provenance) in expressions.iter_mut().zip(provenances) {
        expression.provenance = provenance;
    }
    Ok(())
}

pub(crate) struct SemanticExpressionBuilder<'a> {
    program: &'a CheckedProgram,
    lookup: &'a CheckedProgramLookup,
    out_net: &'a OutNet,
    locals: BTreeMap<OutNetId, (StaticOwnerId, SemanticMaterializationLocalId)>,
    local_types: BTreeMap<(StaticOwnerId, SemanticMaterializationLocalId), Type>,
    materializations_by_owner: &'a BTreeMap<StaticOwnerId, SemanticMaterializationId>,
    materialization_result_types: &'a BTreeMap<SemanticMaterializationId, Type>,
    callable_ids: BTreeMap<DeclId, SemanticCallableId>,
    call_ids: BTreeMap<CheckedCallId, SemanticCallId>,
    producer_callable_ids: BTreeMap<crate::ProducerFunctionId, SemanticCallableId>,
    expressions: Vec<SemanticExpression>,
    checked_expression_origins: Vec<SemanticExpressionOrigin>,
    memo: BTreeMap<ExpansionKey, SemanticExprId>,
    owner_stack: Vec<Option<StaticOwnerId>>,
    frame_stack: Vec<Option<OutCallInstanceId>>,
    current_statement: Option<SemanticStatementId>,
    value_frames: Vec<SemanticValueFrame>,
    value_frame_by_key: BTreeMap<SemanticValueFrameKey, usize>,
    next_local_binding: usize,
    defer_nested_expansion: bool,
}

impl<'a> SemanticExpressionBuilder<'a> {
    fn new(
        program: &'a CheckedProgram,
        lookup: &'a CheckedProgramLookup,
        out_net: &'a OutNet,
        locals: BTreeMap<OutNetId, (StaticOwnerId, SemanticMaterializationLocalId)>,
        local_types: BTreeMap<StaticOwnerId, Type>,
        materializations_by_owner: &'a BTreeMap<StaticOwnerId, SemanticMaterializationId>,
        materialization_result_types: &'a BTreeMap<SemanticMaterializationId, Type>,
    ) -> Self {
        let callable_ids = program
            .callables
            .iter()
            .enumerate()
            .map(|(index, callable)| (callable.decl_id, SemanticCallableId(index)))
            .collect::<BTreeMap<_, _>>();
        let call_ids = program
            .calls
            .iter()
            .enumerate()
            .map(|(index, call)| (call.id, SemanticCallId(index)))
            .collect::<BTreeMap<_, _>>();
        let producer_callable_ids = out_net
            .producer_roots()
            .iter()
            .filter_map(|producer| {
                callable_ids
                    .get(&producer.spec.callable)
                    .copied()
                    .map(|callable| (producer.spec.function, callable))
            })
            .collect::<BTreeMap<_, _>>();
        Self {
            program,
            lookup,
            out_net,
            locals,
            local_types: local_types
                .into_iter()
                .map(|(owner, ty)| ((owner, SemanticMaterializationLocalId(0)), ty))
                .collect(),
            materializations_by_owner,
            materialization_result_types,
            callable_ids,
            call_ids,
            producer_callable_ids,
            expressions: Vec::new(),
            checked_expression_origins: Vec::new(),
            memo: BTreeMap::new(),
            owner_stack: Vec::new(),
            frame_stack: Vec::new(),
            current_statement: None,
            value_frames: Vec::new(),
            value_frame_by_key: BTreeMap::new(),
            next_local_binding: 0,
            defer_nested_expansion: false,
        }
    }

    fn set_local_type(
        &mut self,
        owner: StaticOwnerId,
        local: SemanticMaterializationLocalId,
        ty: Type,
    ) {
        self.local_types.insert((owner, local), ty);
    }

    fn set_current_statement(&mut self, statement: Option<SemanticStatementId>) {
        self.current_statement = statement;
    }

    fn parent_value_bindings(
        &self,
        parent: Option<usize>,
    ) -> BTreeMap<DeclId, SemanticValueBinding> {
        parent
            .and_then(|frame| self.value_frames.get(frame))
            .map(|frame| frame.bindings.clone())
            .unwrap_or_default()
    }

    fn intern_block_value_frame(
        &mut self,
        scoped: ScopedCheckedExpr,
        owner: Option<StaticOwnerId>,
        declarations: &[DeclId],
    ) -> (usize, BTreeMap<DeclId, SemanticLocalBindingId>) {
        let key = SemanticValueFrameKey::Block {
            expression: scoped.expression,
            frame: scoped.frame,
            parent: scoped.value_frame,
            owner,
        };
        if let Some(frame) = self.value_frame_by_key.get(&key).copied() {
            return (frame, self.value_frames[frame].local_ids.clone());
        }

        let mut bindings = self.parent_value_bindings(scoped.value_frame);
        let local_ids = declarations
            .iter()
            .copied()
            .map(|declaration| {
                let id = SemanticLocalBindingId(self.next_local_binding);
                self.next_local_binding += 1;
                bindings.insert(declaration, SemanticValueBinding::Local(id));
                (declaration, id)
            })
            .collect::<BTreeMap<_, _>>();
        let frame = self.value_frames.len();
        self.value_frames.push(SemanticValueFrame {
            bindings,
            local_ids: local_ids.clone(),
        });
        self.value_frame_by_key.insert(key, frame);
        (frame, local_ids)
    }

    fn intern_select_value_frame(
        &mut self,
        scoped: ScopedCheckedExpr,
        owner: Option<StaticOwnerId>,
        arm: CheckedExprId,
        input: CheckedExprId,
        bindings: &[(DeclId, Vec<String>)],
    ) -> usize {
        let key = SemanticValueFrameKey::SelectArm {
            arm,
            frame: scoped.frame,
            parent: scoped.value_frame,
            owner,
        };
        if let Some(frame) = self.value_frame_by_key.get(&key).copied() {
            return frame;
        }

        let mut frame_bindings = self.parent_value_bindings(scoped.value_frame);
        frame_bindings.extend(bindings.iter().cloned().map(|(declaration, fields)| {
            (
                declaration,
                SemanticValueBinding::Projection {
                    input: ScopedCheckedExpr {
                        expression: input,
                        frame: scoped.frame,
                        evaluation_port: None,
                        value_frame: scoped.value_frame,
                    },
                    fields,
                },
            )
        }));
        let frame = self.value_frames.len();
        self.value_frames.push(SemanticValueFrame {
            bindings: frame_bindings,
            local_ids: BTreeMap::new(),
        });
        self.value_frame_by_key.insert(key, frame);
        frame
    }

    fn direct_resource_provenance(
        &self,
        expression: CheckedExprId,
        owner: Option<StaticOwnerId>,
    ) -> Option<SemanticValueProvenance> {
        if let Some(source) = self
            .program
            .sources
            .iter()
            .find(|source| source.expression == expression)
        {
            return Some(SemanticValueProvenance {
                members: vec![SemanticValueMember {
                    path: Vec::new(),
                    origin: SemanticValueOrigin::Source {
                        source: provisional_semantic_source_id(source.id),
                        owner,
                    },
                }],
            });
        }
        self.program
            .states
            .iter()
            .find(|state| state.expression == expression)
            .map(|state| SemanticValueProvenance {
                members: vec![SemanticValueMember {
                    path: Vec::new(),
                    origin: SemanticValueOrigin::State {
                        state: provisional_semantic_state_id(state.id),
                        owner,
                    },
                }],
            })
    }

    fn value_provenance(
        &self,
        expression: &CheckedExpression,
        owner: Option<StaticOwnerId>,
        kind: &SemanticExpressionKind,
    ) -> SemanticValueProvenance {
        if let Some(provenance) = self.direct_resource_provenance(expression.id, owner) {
            return provenance;
        }
        let child = |id: SemanticExprId| {
            self.expressions
                .get(id.as_usize())
                .map(|expression| expression.provenance.clone())
                .unwrap_or_else(runtime_value_provenance)
        };
        match kind {
            SemanticExpressionKind::Object(fields)
            | SemanticExpressionKind::TaggedObject { fields, .. } => {
                record_value_provenance(&self.expressions, fields)
            }
            SemanticExpressionKind::Block { result, .. } => child(*result),
            SemanticExpressionKind::Project { input, fields } => child(*input).projected(fields),
            SemanticExpressionKind::MaterializationLocal {
                owner,
                local,
                projection,
            } => SemanticValueProvenance {
                members: vec![SemanticValueMember {
                    path: Vec::new(),
                    origin: SemanticValueOrigin::MaterializationLocal {
                        owner: *owner,
                        local: *local,
                        projection: projection.clone(),
                    },
                }],
            },
            SemanticExpressionKind::Flush { payload: input }
            | SemanticExpressionKind::FlushBoundary { input }
            | SemanticExpressionKind::Draining { input } => child(*input),
            SemanticExpressionKind::Latest { branches } => {
                let members = branches
                    .iter()
                    .flat_map(|branch| child(*branch).members)
                    .collect();
                normalize_value_provenance(SemanticValueProvenance { members })
            }
            SemanticExpressionKind::When { arms, .. } => {
                let members = arms
                    .iter()
                    .flat_map(|arm| child(arm.output).members)
                    .collect::<Vec<_>>();
                if members.is_empty() {
                    runtime_value_provenance()
                } else {
                    normalize_value_provenance(SemanticValueProvenance { members })
                }
            }
            SemanticExpressionKind::Then {
                output: Some(output),
                ..
            }
            | SemanticExpressionKind::MatchArm {
                output: Some(output),
                ..
            } => child(*output),
            SemanticExpressionKind::Source { .. } => {
                debug_assert!(
                    false,
                    "checked SOURCE expression has no CheckedSource provenance"
                );
                SemanticValueProvenance::default()
            }
            SemanticExpressionKind::CanonicalRead {
                source: Some(source),
                ..
            } if source.payload_projection.is_empty() => SemanticValueProvenance {
                members: vec![SemanticValueMember {
                    path: Vec::new(),
                    origin: SemanticValueOrigin::Source {
                        source: source.source,
                        owner,
                    },
                }],
            },
            SemanticExpressionKind::CanonicalRead { .. }
            | SemanticExpressionKind::LocalRead { .. }
            | SemanticExpressionKind::ExternalRead { .. }
            | SemanticExpressionKind::ElementState { .. }
            | SemanticExpressionKind::Drain { .. }
            | SemanticExpressionKind::Text(_)
            | SemanticExpressionKind::TextTemplate { .. }
            | SemanticExpressionKind::Number(_)
            | SemanticExpressionKind::Bits(_)
            | SemanticExpressionKind::BytesByte(_)
            | SemanticExpressionKind::Absent
            | SemanticExpressionKind::Tag(_)
            | SemanticExpressionKind::Call { .. }
            | SemanticExpressionKind::Materialize { .. }
            | SemanticExpressionKind::Hold { .. }
            | SemanticExpressionKind::Then { output: None, .. }
            | SemanticExpressionKind::Infix { .. }
            | SemanticExpressionKind::MatchArm { output: None, .. }
            | SemanticExpressionKind::List { .. }
            | SemanticExpressionKind::MapEntry { .. }
            | SemanticExpressionKind::Map { .. }
            | SemanticExpressionKind::Set { .. }
            | SemanticExpressionKind::Bytes { .. }
            | SemanticExpressionKind::Delimiter
            | SemanticExpressionKind::FunctionParameter { .. } => runtime_value_provenance(),
        }
    }

    pub(crate) fn expand(
        &mut self,
        expression: ScopedCheckedExpr,
    ) -> Result<SemanticExprId, ExpansionError> {
        let inherited_owner = self.owner_stack.last().copied().flatten();
        let key = self.expansion_key(expression, inherited_owner);
        if let Some(existing) = self.memo.get(&key).copied() {
            return Ok(existing);
        }
        if self.defer_nested_expansion {
            return Err(ExpansionError::DeferredExpansion {
                expression,
                inherited_owner,
            });
        }

        #[derive(Clone)]
        struct Work {
            expression: ScopedCheckedExpr,
            inherited_owner: Option<StaticOwnerId>,
            ancestry: Vec<ExpansionKey>,
        }

        let root_key = key;
        let mut work = vec![Work {
            expression,
            inherited_owner,
            ancestry: Vec::new(),
        }];
        while let Some(current) = work.pop() {
            let current_key = self.expansion_key(current.expression, current.inherited_owner);
            if self.memo.contains_key(&current_key) {
                continue;
            }
            let expression_len = self.expressions.len();
            let origin_len = self.checked_expression_origins.len();
            self.defer_nested_expansion = true;
            let result = self.expand_once(current.expression, current.inherited_owner);
            self.defer_nested_expansion = false;
            match result {
                Ok(_) => {}
                Err(ExpansionError::DeferredExpansion {
                    expression,
                    inherited_owner,
                }) => {
                    self.expressions.truncate(expression_len);
                    self.checked_expression_origins.truncate(origin_len);
                    let dependency_key = self.expansion_key(expression, inherited_owner);
                    if dependency_key == current_key || current.ancestry.contains(&dependency_key) {
                        let cycle_start = current
                            .ancestry
                            .iter()
                            .position(|candidate| *candidate == dependency_key)
                            .unwrap_or(current.ancestry.len());
                        let chain = current.ancestry[cycle_start..]
                            .iter()
                            .copied()
                            .chain(std::iter::once(current_key))
                            .chain(std::iter::once(dependency_key))
                            .map(|key| {
                                format!(
                                    "{}@{:?}/owner={:?}",
                                    key.expression.0, key.frame, key.evaluation_owner
                                )
                            })
                            .collect();
                        return Err(ExpansionError::ExpressionCycle {
                            expression: dependency_key.expression,
                            frame: dependency_key.frame,
                            chain,
                        });
                    }
                    let mut ancestry = current.ancestry.clone();
                    ancestry.push(current_key);
                    work.push(current);
                    work.push(Work {
                        expression,
                        inherited_owner,
                        ancestry,
                    });
                }
                Err(error) => {
                    self.expressions.truncate(expression_len);
                    self.checked_expression_origins.truncate(origin_len);
                    return Err(error);
                }
            }
        }
        self.memo.get(&root_key).copied().ok_or_else(|| {
            ExpansionError::InvalidLocalBindings(format!(
                "semantic expression work stack did not produce checked expression {}",
                expression.expression.0
            ))
        })
    }

    fn expansion_key(
        &self,
        expression: ScopedCheckedExpr,
        inherited_owner: Option<StaticOwnerId>,
    ) -> ExpansionKey {
        let evaluation_owner = self.evaluation_owner(expression).or(inherited_owner);
        ExpansionKey {
            expression: expression.expression,
            frame: expression.frame,
            value_frame: expression.value_frame,
            evaluation_owner,
        }
    }

    fn expand_once(
        &mut self,
        expression: ScopedCheckedExpr,
        inherited_owner: Option<StaticOwnerId>,
    ) -> Result<SemanticExprId, ExpansionError> {
        let key = self.expansion_key(expression, inherited_owner);
        if let Some(existing) = self.memo.get(&key).copied() {
            return Ok(existing);
        }
        self.owner_stack.push(key.evaluation_owner);
        self.frame_stack.push(expression.frame);
        let result = self.expand_uncached(expression, key.evaluation_owner);
        let popped_frame = self
            .frame_stack
            .pop()
            .expect("contextual expansion frame stack contains active expression");
        debug_assert_eq!(popped_frame, expression.frame);
        let popped_owner = self
            .owner_stack
            .pop()
            .expect("contextual expansion owner stack contains active expression");
        debug_assert_eq!(popped_owner, key.evaluation_owner);
        let result = result?;
        self.memo.insert(key, result);
        Ok(result)
    }

    fn finish(self) -> SemanticExpressionArena {
        SemanticExpressionArena {
            expressions: self.expressions,
            checked_expression_origins: self.checked_expression_origins,
        }
    }

    fn expand_with_inherited_owner(
        &mut self,
        expression: ScopedCheckedExpr,
        owner: Option<StaticOwnerId>,
    ) -> Result<SemanticExprId, ExpansionError> {
        self.owner_stack.push(owner);
        let result = self.expand(expression);
        let popped = self
            .owner_stack
            .pop()
            .expect("contextual expansion owner seed is balanced");
        debug_assert_eq!(popped, owner);
        result
    }

    fn resolve_ambient_read(
        &self,
        mut frame: Option<OutCallInstanceId>,
        scope: boon_typecheck::LexicalScopeId,
        path: &str,
    ) -> Option<(DeclId, Vec<String>)> {
        let parts = path
            .split('.')
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>();
        let first = *parts.first()?;
        let mut scopes = Vec::new();
        scopes.push(scope);
        while let Some(frame_id) = frame {
            let instance = self.out_net.call_instances.get(frame_id.as_usize())?;
            let scope = self
                .lookup
                .expression(self.program, instance.provenance.expression)
                .map(|expression| expression.scope_id)?;
            scopes.push(scope);
            frame = instance.parent;
        }
        for scope in scopes {
            if let Some(target) =
                declaration_in_lexical_scope(self.program, self.lookup, scope, first)
            {
                let mut target = target;
                for (index, field) in parts.iter().enumerate().skip(1) {
                    let Some(body_scope) = self
                        .lookup
                        .declaration(self.program, target)
                        .and_then(|declaration| declaration.body_scope)
                    else {
                        return Some((
                            target,
                            parts[index..]
                                .iter()
                                .map(|part| (*part).to_owned())
                                .collect(),
                        ));
                    };
                    let Some(child) = declaration_in_exact_scope(self.lookup, body_scope, field)
                    else {
                        return Some((
                            target,
                            parts[index..]
                                .iter()
                                .map(|part| (*part).to_owned())
                                .collect(),
                        ));
                    };
                    target = child;
                }
                return Some((target, Vec::new()));
            }
        }
        None
    }

    fn expand_uncached(
        &mut self,
        scoped: ScopedCheckedExpr,
        owner: Option<StaticOwnerId>,
    ) -> Result<SemanticExprId, ExpansionError> {
        let expression = self
            .lookup
            .expression(self.program, scoped.expression)
            .cloned()
            .ok_or(ExpansionError::MissingExpression(scoped.expression))?;
        let kind = match expression.kind.clone() {
            CheckedExpressionKind::Read {
                target,
                projection,
                source,
            } => {
                if self
                    .lookup
                    .declaration(self.program, target)
                    .is_some_and(|declaration| {
                        declaration.kind == boon_typecheck::CheckedDeclarationKind::ElementState
                    })
                {
                    let (call, context) = self
                        .lookup
                        .element_context(self.program, target)
                        .ok_or(ExpansionError::MissingDeclaration(target))?;
                    let instance = self
                        .out_net
                        .call_instance_for_checked_call(call.id, scoped.frame)
                        .ok_or(ExpansionError::MissingCallInstance {
                            call: call.id,
                            frame: scoped.frame,
                        })?;
                    return Ok(self.push(
                        &expression,
                        owner,
                        SemanticExpressionKind::ElementState {
                            context: SemanticCallContextId {
                                call_instance: instance,
                                ordinal: context.signature,
                            },
                            projection,
                        },
                    ));
                }
                if let Some(binding) = scoped
                    .value_frame
                    .and_then(|frame| self.value_frames.get(frame))
                    .and_then(|frame| frame.bindings.get(&target))
                    .cloned()
                {
                    return match binding {
                        SemanticValueBinding::Local(binding) => Ok(self.push(
                            &expression,
                            owner,
                            SemanticExpressionKind::LocalRead {
                                binding,
                                declaration: target,
                                projection,
                            },
                        )),
                        SemanticValueBinding::Projection {
                            input,
                            fields: mut binding_fields,
                        } => {
                            let input = self.expand(input)?;
                            binding_fields.extend(projection);
                            self.project(&expression, owner, input, binding_fields)
                        }
                    };
                }
                if let Some(net) = self.out_net.output_net_in_frame(scoped.frame, target) {
                    let (local_owner, local) =
                        self.locals
                            .get(&net)
                            .copied()
                            .ok_or(ExpansionError::UnboundOutput {
                                expression: scoped.expression,
                                target,
                                net,
                            })?;
                    let local_type = self
                        .local_types
                        .get(&(local_owner, local))
                        .cloned()
                        .and_then(|ty| project_concrete_type(ty, &projection));
                    let local_expression = self.push(
                        &expression,
                        owner,
                        SemanticExpressionKind::MaterializationLocal {
                            owner: local_owner,
                            local,
                            projection,
                        },
                    );
                    if let Some(ty) = local_type {
                        self.expressions[local_expression.as_usize()].flow_type.ty =
                            erase_runtime_type_vars(&ty);
                    }
                    return Ok(local_expression);
                }
                if let Some((actual, argument_owner)) = scoped.frame.and_then(|frame| {
                    self.out_net.call_instances[frame.as_usize()]
                        .inputs
                        .iter()
                        .find(|binding| binding.formal == target)
                        .map(|binding| {
                            (
                                binding.value.clone(),
                                self.out_net.owner_for_call_evaluation(frame),
                            )
                        })
                }) {
                    match actual {
                        OutInputValue::Checked(actual) => {
                            let actual = ScopedCheckedExpr {
                                value_frame: actual.value_frame.or(scoped.value_frame),
                                ..actual
                            };
                            let expanded =
                                self.expand_with_inherited_owner(actual, argument_owner)?;
                            return self.project(&expression, owner, expanded, projection);
                        }
                        OutInputValue::ProducerParameter {
                            parameter,
                            mut flow_type,
                        } => {
                            flow_type.ty = project_concrete_type(flow_type.ty, &projection)
                                .ok_or_else(|| {
                                    ExpansionError::InvalidLocalBindings(format!(
                                        "producer parameter {:?} cannot project checked fields {:?}",
                                        parameter, projection
                                    ))
                                })?;
                            let callable = self
                                .producer_callable_ids
                                .get(&parameter.function)
                                .copied()
                                .ok_or_else(|| {
                                    ExpansionError::InvalidLocalBindings(format!(
                                        "producer parameter {:?} has no semantic callable",
                                        parameter
                                    ))
                                })?;
                            let parameter = semantic_parameter_id(callable, parameter.ordinal);
                            let parameter = self.push(
                                &expression,
                                owner,
                                SemanticExpressionKind::FunctionParameter {
                                    parameter,
                                    projection,
                                },
                            );
                            self.expressions[parameter.as_usize()].flow_type = flow_type;
                            return Ok(parameter);
                        }
                    }
                }
                let declaration = self
                    .lookup
                    .declaration(self.program, target)
                    .ok_or(ExpansionError::MissingDeclaration(target))?;
                if declaration.kind == boon_typecheck::CheckedDeclarationKind::PatternBinding {
                    let binding = self
                        .lookup
                        .pattern_binding(self.program, target)
                        .ok_or(ExpansionError::MissingDeclaration(target))?;
                    let input =
                        self.expand_in_frame(binding.selector, scoped.frame, scoped.value_frame)?;
                    let mut fields = binding.projection.clone();
                    fields.extend(projection);
                    return self.project(&expression, owner, input, fields);
                }
                if declaration.kind == boon_typecheck::CheckedDeclarationKind::Field
                    && declaration_is_function_local(self.program, declaration.scope_id)
                    && declaration.value.is_some_and(|value| {
                        self.lookup
                            .expression(self.program, value)
                            .is_some_and(|value| {
                                !value.effect.writes_state
                                    && !value.effect.emits_source
                                    && !value.effect.invokes_host
                                    && !matches!(
                                        value.kind,
                                        CheckedExpressionKind::Hold { .. }
                                            | CheckedExpressionKind::Latest { .. }
                                            | CheckedExpressionKind::Source
                                            | CheckedExpressionKind::Draining { .. }
                                    )
                            })
                    })
                {
                    let expanded = self.expand_in_frame(
                        declaration.value.expect("checked local value exists"),
                        scoped.frame,
                        scoped.value_frame,
                    )?;
                    return self.project(&expression, owner, expanded, projection);
                }
                SemanticExpressionKind::CanonicalRead {
                    target,
                    path: canonical_declaration_path(self.program, self.lookup, target)
                        .ok_or(ExpansionError::MissingDeclaration(target))?,
                    projection,
                    source: source.map(|source| SemanticSourceRead {
                        source: provisional_semantic_source_id(source.source),
                        payload_projection: source.payload_projection,
                    }),
                }
            }
            CheckedExpressionKind::Passed {
                formal,
                projection,
                access,
            } => {
                let passed = scoped
                    .frame
                    .and_then(|frame| self.out_net.call_instances[frame.as_usize()].passed)
                    .ok_or(ExpansionError::MissingPassedContext(scoped.expression))?;
                if passed.formal != formal {
                    return Err(ExpansionError::MismatchedPassedFormal {
                        expression: scoped.expression,
                        expected: passed.formal,
                        found: formal,
                    });
                }
                let argument_owner = self
                    .out_net
                    .owner_for_call_evaluation(passed.evaluation_call);
                let expanded = self.expand_with_inherited_owner(passed.value, argument_owner)?;
                let projected = self.project(&expression, owner, expanded, projection)?;
                if access == CheckedPassedAccess::Read {
                    return Ok(projected);
                }
                let SemanticExpressionKind::CanonicalRead {
                    target,
                    path,
                    projection,
                    ..
                } = self.expressions[projected.as_usize()].kind.clone()
                else {
                    return Err(ExpansionError::InvalidPassedDrainTarget(scoped.expression));
                };
                return Ok(self.push(
                    &expression,
                    owner,
                    SemanticExpressionKind::Drain {
                        target,
                        path,
                        projection,
                    },
                ));
            }
            CheckedExpressionKind::ExternalRead {
                canonical_path,
                external_identity,
            } => {
                if let Some((target, projection)) =
                    self.resolve_ambient_read(scoped.frame, expression.scope_id, &canonical_path)
                {
                    SemanticExpressionKind::CanonicalRead {
                        target,
                        path: canonical_declaration_path(self.program, self.lookup, target)
                            .ok_or(ExpansionError::MissingDeclaration(target))?,
                        projection,
                        source: None,
                    }
                } else if canonical_path.contains('/') {
                    SemanticExpressionKind::ExternalRead {
                        canonical_path,
                        external_identity,
                    }
                } else {
                    return Err(ExpansionError::UnresolvedAmbientRead {
                        expression: scoped.expression,
                        path: canonical_path,
                    });
                }
            }
            CheckedExpressionKind::Drain { target, projection } => SemanticExpressionKind::Drain {
                target,
                path: canonical_declaration_path(self.program, self.lookup, target)
                    .ok_or(ExpansionError::MissingDeclaration(target))?,
                projection,
            },
            CheckedExpressionKind::Text { value } => SemanticExpressionKind::Text(value),
            CheckedExpressionKind::TextTemplate { segments } => {
                SemanticExpressionKind::TextTemplate {
                    segments: segments
                        .into_iter()
                        .map(|segment| match segment {
                            CheckedTextSegment::Static { value } => {
                                Ok(SemanticTextSegment::Static { value })
                            }
                            CheckedTextSegment::Dynamic { value } => self
                                .expand_in_frame(value, scoped.frame, scoped.value_frame)
                                .map(|value| SemanticTextSegment::Dynamic { value }),
                        })
                        .collect::<Result<Vec<_>, ExpansionError>>()?,
                }
            }
            CheckedExpressionKind::Number { value } => SemanticExpressionKind::Number(value),
            CheckedExpressionKind::Bits { value } => SemanticExpressionKind::Bits(value),
            CheckedExpressionKind::BytesByte { value } => SemanticExpressionKind::BytesByte(value),
            CheckedExpressionKind::Absent => SemanticExpressionKind::Absent,
            CheckedExpressionKind::Flush { payload } => SemanticExpressionKind::Flush {
                payload: self.expand_in_frame(payload, scoped.frame, scoped.value_frame)?,
            },
            CheckedExpressionKind::Tag { name } => SemanticExpressionKind::Tag(name),
            CheckedExpressionKind::TaggedObject { tag, fields } => {
                SemanticExpressionKind::TaggedObject {
                    tag,
                    fields: self.expand_fields(scoped.frame, scoped.value_frame, fields)?,
                }
            }
            CheckedExpressionKind::Source => SemanticExpressionKind::Source {
                binding_path: self
                    .concrete_resource_binding_path(scoped.expression, scoped.frame, owner)
                    .ok_or(ExpansionError::MissingSourceDeclaration(scoped.expression))?,
            },
            CheckedExpressionKind::Call { call } => {
                return self.expand_call(&expression, scoped, owner, call);
            }
            CheckedExpressionKind::Draining { input } => SemanticExpressionKind::Draining {
                input: self.expand_in_frame(input, scoped.frame, scoped.value_frame)?,
            },
            CheckedExpressionKind::Hold { initial, name } => SemanticExpressionKind::Hold {
                initial: self.expand_in_frame(initial, scoped.frame, scoped.value_frame)?,
                binding_path: self
                    .concrete_resource_binding_path(scoped.expression, scoped.frame, owner)
                    .unwrap_or_else(|| name.clone()),
                name,
                updates: self.expand_statement_child_values(scoped)?,
            },
            CheckedExpressionKind::Latest { branches } => SemanticExpressionKind::Latest {
                branches: self.expand_many(scoped.frame, scoped.value_frame, branches)?,
            },
            CheckedExpressionKind::When { input, arms } => {
                let checked_input = input;
                let input =
                    self.expand_in_frame(checked_input, scoped.frame, scoped.value_frame)?;
                SemanticExpressionKind::When {
                    select_kind: SemanticSelectKind::When,
                    input,
                    arms: self.expand_select_arms(scoped, owner, checked_input, &arms)?,
                }
            }
            CheckedExpressionKind::While { input, arms } => {
                let checked_input = input;
                let input =
                    self.expand_in_frame(checked_input, scoped.frame, scoped.value_frame)?;
                SemanticExpressionKind::When {
                    select_kind: SemanticSelectKind::While,
                    input,
                    arms: self.expand_select_arms(scoped, owner, checked_input, &arms)?,
                }
            }
            CheckedExpressionKind::Then { input, output } => SemanticExpressionKind::Then {
                input: self.expand_in_frame(input, scoped.frame, scoped.value_frame)?,
                output: output
                    .map(|output| self.expand_in_frame(output, scoped.frame, scoped.value_frame))
                    .transpose()?,
            },
            CheckedExpressionKind::Infix { left, op, right } => SemanticExpressionKind::Infix {
                left: self.expand_in_frame(left, scoped.frame, scoped.value_frame)?,
                op,
                right: self.expand_in_frame(right, scoped.frame, scoped.value_frame)?,
            },
            CheckedExpressionKind::MatchArm {
                pattern, output, ..
            } => SemanticExpressionKind::MatchArm {
                pattern,
                output: output
                    .map(|output| self.expand_in_frame(output, scoped.frame, scoped.value_frame))
                    .transpose()?,
            },
            CheckedExpressionKind::Block { bindings, result } => {
                let declarations = bindings
                    .iter()
                    .map(|binding| binding.declaration)
                    .collect::<Vec<_>>();
                let (frame, binding_ids) =
                    self.intern_block_value_frame(scoped, owner, &declarations);
                let result = result.ok_or(ExpansionError::MissingExpression(scoped.expression))?;
                let bindings = bindings
                    .into_iter()
                    .map(|binding| {
                        let value = self.expand(ScopedCheckedExpr {
                            expression: binding.value,
                            frame: scoped.frame,
                            evaluation_port: None,
                            value_frame: Some(frame),
                        })?;
                        let boundary_expression =
                            self.flush_boundary_origin_for_value(binding.value, value);
                        let value = self.wrap_flush_boundary(
                            boundary_expression,
                            value,
                            owner,
                            self.lookup
                                .declaration(self.program, binding.declaration)
                                .map(|declaration| declaration.flow_type.clone()),
                        )?;
                        Ok(SemanticBlockBinding {
                            id: binding_ids[&binding.declaration],
                            declaration: binding.declaration,
                            value,
                        })
                    })
                    .collect::<Result<Vec<_>, ExpansionError>>()?;
                let result_expression = result;
                let result = self.expand(ScopedCheckedExpr {
                    expression: result,
                    frame: scoped.frame,
                    evaluation_port: None,
                    value_frame: Some(frame),
                })?;
                let boundary_expression =
                    self.flush_boundary_origin_for_value(result_expression, result);
                let result = self.wrap_flush_boundary(
                    boundary_expression,
                    result,
                    owner,
                    Some(expression.flow_type.clone()),
                )?;
                SemanticExpressionKind::Block { bindings, result }
            }
            CheckedExpressionKind::Object { fields } => SemanticExpressionKind::Object(
                self.expand_fields(scoped.frame, scoped.value_frame, fields)?,
            ),
            CheckedExpressionKind::List { capacity, items } => SemanticExpressionKind::List {
                capacity,
                items: self.expand_many(scoped.frame, scoped.value_frame, items)?,
            },
            CheckedExpressionKind::MapEntry { key, value } => SemanticExpressionKind::MapEntry {
                key: self.expand(ScopedCheckedExpr {
                    expression: key,
                    frame: scoped.frame,
                    evaluation_port: None,
                    value_frame: scoped.value_frame,
                })?,
                value: self.expand(ScopedCheckedExpr {
                    expression: value,
                    frame: scoped.frame,
                    evaluation_port: None,
                    value_frame: scoped.value_frame,
                })?,
            },
            CheckedExpressionKind::Map { entries } => SemanticExpressionKind::Map {
                entries: self.expand_many(scoped.frame, scoped.value_frame, entries)?,
            },
            CheckedExpressionKind::Set { items } => SemanticExpressionKind::Set {
                items: self.expand_many(scoped.frame, scoped.value_frame, items)?,
            },
            CheckedExpressionKind::Bytes { fixed_size, items } => SemanticExpressionKind::Bytes {
                fixed_size,
                items: self.expand_many(scoped.frame, scoped.value_frame, items)?,
            },
            CheckedExpressionKind::Delimiter => SemanticExpressionKind::Delimiter,
            CheckedExpressionKind::Invalid { tokens } => {
                return Err(ExpansionError::InvalidCheckedExpression {
                    expression: scoped.expression,
                    tokens,
                });
            }
        };
        Ok(self.push(&expression, owner, kind))
    }

    fn expand_call(
        &mut self,
        expression: &CheckedExpression,
        scoped: ScopedCheckedExpr,
        owner: Option<StaticOwnerId>,
        call_id: CheckedCallId,
    ) -> Result<SemanticExprId, ExpansionError> {
        let checked_call = self
            .lookup
            .call(self.program, call_id)
            .cloned()
            .ok_or(ExpansionError::MissingCall(call_id))?;
        let callable = self
            .lookup
            .callable(self.program, checked_call.callable)
            .cloned()
            .ok_or(ExpansionError::MissingCallable(checked_call.callable))?;
        let instance = self
            .out_net
            .call_instance_for_checked_call(call_id, scoped.frame)
            .ok_or(ExpansionError::MissingCallInstance {
                call: call_id,
                frame: scoped.frame,
            })?;
        let has_out = checked_call
            .entries
            .iter()
            .any(|entry| !matches!(entry, CheckedCallEntry::Input { .. }));
        if has_out {
            let mut materializations = self.out_net.call_instances[instance.as_usize()]
                .ports
                .iter()
                .filter_map(|port| {
                    let net = self.out_net.net_for_port(*port);
                    let owner = self.out_net.owner_for_net(net)?;
                    self.materializations_by_owner.get(&owner).copied()
                })
                .collect::<BTreeSet<_>>();
            let materialization = materializations
                .pop_first()
                .ok_or(ExpansionError::MissingMaterialization(call_id))?;
            if !materializations.is_empty() {
                return Err(ExpansionError::AmbiguousMaterialization(call_id));
            }
            let expression_id = self.push(
                expression,
                owner,
                SemanticExpressionKind::Materialize { materialization },
            );
            if let Some(result_type) = self.materialization_result_types.get(&materialization) {
                self.expressions[expression_id.as_usize()].flow_type.ty =
                    erase_runtime_type_vars(result_type);
            }
            return Ok(expression_id);
        }
        if callable.kind == CheckedCallableKind::User {
            let result = callable
                .result_expression
                .ok_or(ExpansionError::MissingFunctionResult(callable.decl_id))?;
            let call_owner = self.out_net.owner_for_call(instance).or(owner);
            let concrete_result = self.out_net.call_instances[instance.as_usize()]
                .result
                .clone();
            let result = self.expand_with_inherited_owner(
                ScopedCheckedExpr {
                    expression: result,
                    frame: Some(instance),
                    evaluation_port: None,
                    value_frame: scoped.value_frame,
                },
                call_owner,
            )?;
            // User calls erase their wrapper expression, so the expanded body
            // is the call occurrence. Give that occurrence the checked,
            // call-local result instead of leaving the callable's open
            // structural scheme on the shared body syntax.
            self.expressions[result.as_usize()].flow_type = concrete_result.clone();
            let result_expression = callable
                .result_expression
                .ok_or(ExpansionError::MissingFunctionResult(callable.decl_id))?;
            let boundary_expression =
                self.flush_boundary_origin_for_value(result_expression, result);
            return self.wrap_flush_boundary(
                boundary_expression,
                result,
                call_owner,
                Some(concrete_result),
            );
        }
        if !matches!(checked_call.context_binding, CheckedContextBinding::None) {
            return Err(ExpansionError::PassOnNonexpandedCall(call_id));
        }
        let inputs = self.out_net.call_instances[instance.as_usize()]
            .inputs
            .clone();
        let argument_owner = self.out_net.owner_for_call_evaluation(instance);
        let mut arguments = Vec::with_capacity(inputs.len());
        for input in inputs {
            let parameter = callable
                .parameters
                .iter()
                .find(|parameter| parameter.decl_id == input.formal)
                .ok_or(ExpansionError::MissingFormal {
                    callable: callable.decl_id,
                    formal: input.formal,
                })?;
            let checked_value = input.checked_value().ok_or(ExpansionError::MissingFormal {
                callable: callable.decl_id,
                formal: input.formal,
            })?;
            // OUT topology is derived before semantic BLOCK frames exist, so
            // a builtin argument may not yet carry the lexical value frame in
            // which its call occurs. Inherit the call-site frame exactly as
            // user-call parameter substitution does above.
            let checked_value = ScopedCheckedExpr {
                value_frame: checked_value.value_frame.or(scoped.value_frame),
                ..checked_value
            };
            arguments.push(SemanticCallArgument {
                formal: input.formal,
                ordinal: parameter.ordinal,
                name: parameter.name.clone(),
                checked_value: checked_value.expression,
                value: self.expand_with_inherited_owner(checked_value, argument_owner)?,
                from_pipe: checked_call.entries.iter().any(|entry| {
                    matches!(
                        entry,
                        CheckedCallEntry::Input {
                            formal,
                            from_pipe: true,
                            ..
                        } if *formal == input.formal
                    )
                }),
            });
        }
        arguments.sort_by_key(|argument| argument.ordinal);
        let mut parameter_bindings = Vec::new();
        for parameter in callable
            .parameters
            .iter()
            .filter(|parameter| parameter.kind == CheckedParameterKind::Value)
        {
            let kind = if let Some(argument) = arguments
                .iter()
                .find(|argument| argument.formal == parameter.decl_id)
            {
                SemanticCallParameterBindingKind::Explicit {
                    checked_value: argument.checked_value,
                    value: argument.value,
                    from_pipe: argument.from_pipe,
                }
            } else {
                if matches!(parameter.requirement, CheckedParameterRequirement::Required) {
                    return Err(ExpansionError::MissingFormal {
                        callable: callable.decl_id,
                        formal: parameter.decl_id,
                    });
                }
                SemanticCallParameterBindingKind::Omitted
            };
            parameter_bindings.push(SemanticCallParameterBinding {
                formal: parameter.decl_id,
                ordinal: parameter.ordinal,
                name: parameter.name.clone(),
                requirement: parameter.requirement.clone(),
                kind,
            });
        }
        parameter_bindings.sort_by_key(|binding| binding.ordinal);
        let kind = match callable.kind {
            CheckedCallableKind::Builtin => SemanticCallableKind::Builtin,
            CheckedCallableKind::External => SemanticCallableKind::External,
            CheckedCallableKind::User => unreachable!("user calls are expanded above"),
        };
        let contexts = checked_call
            .contexts
            .iter()
            .map(|context| SemanticCallContextId {
                call_instance: instance,
                ordinal: context.signature,
            })
            .collect();
        let semantic_call = self.call_ids.get(&call_id).copied().ok_or_else(|| {
            ExpansionError::InvalidLocalBindings(format!(
                "checked call {} has no semantic call identity",
                call_id.0
            ))
        })?;
        let semantic_callable = self
            .callable_ids
            .get(&checked_call.callable)
            .copied()
            .ok_or_else(|| {
                ExpansionError::InvalidLocalBindings(format!(
                    "checked call {} callable {} has no semantic identity",
                    call_id.0, checked_call.callable.0
                ))
            })?;
        Ok(self.push(
            expression,
            owner,
            SemanticExpressionKind::Call {
                call: semantic_call,
                callable: semantic_callable,
                callable_kind: kind,
                name: callable.name.clone(),
                function: checked_call.function.clone(),
                intrinsic: checked_call.intrinsic,
                role: checked_call.role,
                effect: callable.effect,
                result: checked_call.result.clone(),
                instance,
                arguments,
                parameter_bindings,
                contexts,
            },
        ))
    }

    fn expand_in_frame(
        &mut self,
        expression: CheckedExprId,
        frame: Option<OutCallInstanceId>,
        value_frame: Option<usize>,
    ) -> Result<SemanticExprId, ExpansionError> {
        self.expand(ScopedCheckedExpr {
            expression,
            frame,
            evaluation_port: None,
            value_frame,
        })
    }

    fn expand_many(
        &mut self,
        frame: Option<OutCallInstanceId>,
        value_frame: Option<usize>,
        expressions: Vec<CheckedExprId>,
    ) -> Result<Vec<SemanticExprId>, ExpansionError> {
        expressions
            .into_iter()
            .map(|expression| self.expand_in_frame(expression, frame, value_frame))
            .collect()
    }

    fn expand_fields(
        &mut self,
        frame: Option<OutCallInstanceId>,
        value_frame: Option<usize>,
        fields: Vec<boon_typecheck::CheckedRecordField>,
    ) -> Result<Vec<SemanticRecordField>, ExpansionError> {
        fields
            .into_iter()
            .map(|field| {
                let value = self.expand_in_frame(field.value, frame, value_frame)?;
                let boundary_expression = field
                    .declaration
                    .and_then(|declaration| {
                        self.lookup
                            .declaration(self.program, declaration)
                            .and_then(|declaration| declaration.value)
                    })
                    .filter(|expression| {
                        self.lookup
                            .expression(self.program, *expression)
                            .is_some_and(|expression| expression.flush_type.is_some())
                    })
                    .unwrap_or_else(|| self.flush_boundary_origin_for_value(field.value, value));
                let value = self.wrap_flush_boundary(
                    boundary_expression,
                    value,
                    self.owner_stack.last().copied().flatten(),
                    field.declaration.and_then(|declaration| {
                        self.lookup
                            .declaration(self.program, declaration)
                            .map(|declaration| declaration.flow_type.clone())
                    }),
                )?;
                Ok(SemanticRecordField {
                    declaration: field.declaration,
                    name: field.name,
                    value,
                    spread: field.spread,
                })
            })
            .collect()
    }

    fn flush_boundary_origin_for_value(
        &self,
        checked_expression: CheckedExprId,
        semantic_expression: SemanticExprId,
    ) -> CheckedExprId {
        if self
            .lookup
            .expression(self.program, checked_expression)
            .is_some_and(|expression| expression.flush_type.is_some())
        {
            return checked_expression;
        }
        let Some(SemanticExpressionKind::CanonicalRead { target, .. }) = self
            .expressions
            .get(semantic_expression.as_usize())
            .map(|expression| &expression.kind)
        else {
            return checked_expression;
        };
        self.lookup
            .declaration(self.program, *target)
            .and_then(|declaration| declaration.value)
            .filter(|expression| {
                self.lookup
                    .expression(self.program, *expression)
                    .is_some_and(|expression| expression.flush_type.is_some())
            })
            .unwrap_or(checked_expression)
    }

    fn wrap_flush_boundary(
        &mut self,
        checked_expression: CheckedExprId,
        input: SemanticExprId,
        owner: Option<StaticOwnerId>,
        flow_type: Option<FlowType>,
    ) -> Result<SemanticExprId, ExpansionError> {
        let origin = self
            .lookup
            .expression(self.program, checked_expression)
            .cloned()
            .ok_or(ExpansionError::MissingExpression(checked_expression))?;
        let Some(flush_type) = origin.flush_type.clone() else {
            return Ok(input);
        };
        let mut flow_type = flow_type.unwrap_or_else(|| origin.flow_type.clone());
        flow_type.ty = boon_typecheck::canonical_union_type(vec![flow_type.ty, flush_type]);
        if flow_type.mode == boon_typecheck::FlowMode::Absent {
            flow_type.mode = boon_typecheck::FlowMode::Continuous;
        }
        let boundary = self.push(
            &origin,
            owner,
            SemanticExpressionKind::FlushBoundary { input },
        );
        self.expressions[boundary.as_usize()].flow_type = flow_type;
        Ok(boundary)
    }

    fn expand_statement_child_values(
        &mut self,
        scoped: ScopedCheckedExpr,
    ) -> Result<Vec<SemanticExprId>, ExpansionError> {
        self.semantic_statement_child_values(scoped.expression)
            .into_iter()
            .map(|expression| self.expand_in_frame(expression, scoped.frame, scoped.value_frame))
            .collect()
    }

    fn expand_select_arms(
        &mut self,
        scoped: ScopedCheckedExpr,
        owner: Option<StaticOwnerId>,
        input: CheckedExprId,
        arm_ids: &[CheckedExprId],
    ) -> Result<Vec<SemanticSelectArm>, ExpansionError> {
        let mut arms = Vec::new();
        for child in arm_ids {
            let Some(expression) = self.lookup.expression(self.program, *child).cloned() else {
                continue;
            };
            let CheckedExpressionKind::MatchArm {
                pattern,
                bindings,
                output: Some(output),
            } = &expression.kind
            else {
                continue;
            };
            let value_frame = if bindings.is_empty() {
                scoped.value_frame
            } else {
                let frame_bindings = bindings
                    .iter()
                    .map(|binding| {
                        let projection = self
                            .lookup
                            .pattern_binding(self.program, *binding)
                            .ok_or(ExpansionError::MissingDeclaration(*binding))?
                            .projection
                            .clone();
                        Ok((*binding, projection))
                    })
                    .collect::<Result<Vec<_>, ExpansionError>>()?;
                Some(self.intern_select_value_frame(scoped, owner, *child, input, &frame_bindings))
            };
            arms.push(SemanticSelectArm {
                pattern: pattern.clone(),
                bindings: bindings
                    .iter()
                    .map(|binding| {
                        let declaration = self
                            .lookup
                            .declaration(self.program, *binding)
                            .ok_or(ExpansionError::MissingDeclaration(*binding))?;
                        let projection = self
                            .lookup
                            .pattern_binding(self.program, *binding)
                            .ok_or(ExpansionError::MissingDeclaration(*binding))?
                            .projection
                            .clone();
                        Ok(SemanticPatternBinding {
                            name: declaration.name.clone(),
                            projection,
                        })
                    })
                    .collect::<Result<Vec<_>, ExpansionError>>()?,
                output: self.expand_select_arm_output(
                    *child,
                    *output,
                    scoped.frame,
                    value_frame,
                    owner,
                )?,
            });
        }
        Ok(arms)
    }

    fn expand_select_arm_output(
        &mut self,
        arm: CheckedExprId,
        output: CheckedExprId,
        frame: Option<OutCallInstanceId>,
        value_frame: Option<usize>,
        owner: Option<StaticOwnerId>,
    ) -> Result<SemanticExprId, ExpansionError> {
        let output_expression = self
            .lookup
            .expression(self.program, output)
            .cloned()
            .ok_or(ExpansionError::MissingExpression(output))?;
        if !matches!(output_expression.kind, CheckedExpressionKind::Delimiter) {
            return self.expand_in_frame(output, frame, value_frame);
        }

        let Some(statement) = self
            .lookup
            .statement_indices_for_value(arm)
            .iter()
            .filter_map(|index| self.program.statements.get(*index))
            .find(|statement| statement.value == Some(arm))
        else {
            return self.expand_in_frame(output, frame, value_frame);
        };
        let children = statement.children.clone();
        let mut structural_fields = Vec::with_capacity(children.len());
        for child_id in children {
            let child = self
                .lookup
                .statement(self.program, child_id)
                .ok_or(ExpansionError::MissingExpression(output))?;
            let (declaration, name, spread) = match child.kind {
                boon_typecheck::CheckedStatementKind::Field { declaration } => (
                    Some(declaration),
                    self.lookup
                        .declaration(self.program, declaration)
                        .ok_or(ExpansionError::MissingDeclaration(declaration))?
                        .name
                        .clone(),
                    false,
                ),
                boon_typecheck::CheckedStatementKind::Spread => (None, String::new(), true),
                _ => return self.expand_in_frame(output, frame, value_frame),
            };
            let value = child
                .value
                .ok_or(ExpansionError::MissingExpression(output))?;
            structural_fields.push((declaration, name, value, spread));
        }
        if structural_fields.is_empty() {
            return self.expand_in_frame(output, frame, value_frame);
        }

        let fields = structural_fields
            .into_iter()
            .map(|(declaration, name, value, spread)| {
                Ok(SemanticRecordField {
                    declaration,
                    name,
                    value: self.expand_in_frame(value, frame, value_frame)?,
                    spread,
                })
            })
            .collect::<Result<Vec<_>, ExpansionError>>()?;
        Ok(self.push(
            &output_expression,
            owner,
            SemanticExpressionKind::Object(fields),
        ))
    }

    fn semantic_statement_child_values(
        &self,
        parent_expression: CheckedExprId,
    ) -> Vec<CheckedExprId> {
        let Some(statement) = self
            .lookup
            .statement_indices_for_value(parent_expression)
            .iter()
            .filter_map(|index| self.program.statements.get(*index))
            .find(|statement| {
                statement.value == Some(parent_expression) && !statement.children.is_empty()
            })
        else {
            return Vec::new();
        };
        let mut pending = statement.children.iter().rev().copied().collect::<Vec<_>>();
        let mut values = Vec::new();
        let mut visited = BTreeSet::new();
        while let Some(child) = pending.pop() {
            if !visited.insert(child) {
                continue;
            }
            let Some(statement) = self.lookup.statement(self.program, child) else {
                continue;
            };
            match statement.value {
                // Pipeline continuation statements share the enclosing statement's
                // canonical final expression. They carry structure, not another
                // reactive branch, so walk through them instead of expanding the
                // same expression recursively.
                Some(value) if value == parent_expression => {
                    pending.extend(statement.children.iter().rev().copied());
                }
                Some(value) => values.push(value),
                None => pending.extend(statement.children.iter().rev().copied()),
            }
        }
        values.dedup();
        let mut updates = Vec::with_capacity(values.len());
        for value in values {
            let continues_previous = updates
                .last()
                .copied()
                .is_some_and(|previous| self.checked_pipeline_input(value) == Some(previous));
            if continues_previous {
                updates.pop();
            }
            updates.push(value);
        }
        updates
    }

    fn checked_pipeline_input(&self, expression: CheckedExprId) -> Option<CheckedExprId> {
        match &self.lookup.expression(self.program, expression)?.kind {
            CheckedExpressionKind::Call { call } => self
                .lookup
                .call(self.program, *call)?
                .entries
                .iter()
                .find_map(|entry| match entry {
                    boon_typecheck::CheckedCallEntry::Input {
                        value,
                        from_pipe: true,
                        ..
                    } => Some(*value),
                    boon_typecheck::CheckedCallEntry::Input { .. }
                    | boon_typecheck::CheckedCallEntry::FreshOut { .. }
                    | boon_typecheck::CheckedCallEntry::ForwardOut { .. } => None,
                }),
            CheckedExpressionKind::Draining { input }
            | CheckedExpressionKind::Hold { initial: input, .. }
            | CheckedExpressionKind::When { input, .. }
            | CheckedExpressionKind::While { input, .. }
            | CheckedExpressionKind::Then { input, .. } => Some(*input),
            CheckedExpressionKind::Read { .. }
            | CheckedExpressionKind::Passed { .. }
            | CheckedExpressionKind::ExternalRead { .. }
            | CheckedExpressionKind::Drain { .. }
            | CheckedExpressionKind::Text { .. }
            | CheckedExpressionKind::TextTemplate { .. }
            | CheckedExpressionKind::Number { .. }
            | CheckedExpressionKind::BytesByte { .. }
            | CheckedExpressionKind::Absent
            | CheckedExpressionKind::Flush { .. }
            | CheckedExpressionKind::Tag { .. }
            | CheckedExpressionKind::TaggedObject { .. }
            | CheckedExpressionKind::Source
            | CheckedExpressionKind::Latest { .. }
            | CheckedExpressionKind::Infix { .. }
            | CheckedExpressionKind::MatchArm { .. }
            | CheckedExpressionKind::Block { .. }
            | CheckedExpressionKind::Object { .. }
            | CheckedExpressionKind::List { .. }
            | CheckedExpressionKind::Bytes { .. }
            | CheckedExpressionKind::Delimiter
            | CheckedExpressionKind::Invalid { .. }
            | CheckedExpressionKind::MapEntry { .. }
            | CheckedExpressionKind::Map { .. }
            | CheckedExpressionKind::Set { .. }
            | CheckedExpressionKind::Bits { .. } => None,
        }
    }

    fn project(
        &mut self,
        expression: &CheckedExpression,
        owner: Option<StaticOwnerId>,
        mut input: SemanticExprId,
        mut fields: Vec<String>,
    ) -> Result<SemanticExprId, ExpansionError> {
        loop {
            let Some(first) = fields.first() else {
                return Ok(input);
            };
            let direct_field = match &self.expressions[input.as_usize()].kind {
                SemanticExpressionKind::Object(record_fields)
                    if record_fields.iter().all(|field| !field.spread) =>
                {
                    let matches = record_fields
                        .iter()
                        .filter(|field| field.name == *first)
                        .map(|field| field.value)
                        .collect::<Vec<_>>();
                    matches
                        .as_slice()
                        .first()
                        .copied()
                        .filter(|_| matches.len() == 1)
                }
                SemanticExpressionKind::TaggedObject {
                    fields: record_fields,
                    ..
                } if record_fields.iter().all(|field| !field.spread) => {
                    let matches = record_fields
                        .iter()
                        .filter(|field| field.name == *first)
                        .map(|field| field.value)
                        .collect::<Vec<_>>();
                    matches
                        .as_slice()
                        .first()
                        .copied()
                        .filter(|_| matches.len() == 1)
                }
                _ => None,
            };
            let Some(direct_field) = direct_field else {
                break;
            };
            input = direct_field;
            fields.remove(0);
        }
        let projected_type = project_concrete_type(
            self.expressions[input.as_usize()].flow_type.ty.clone(),
            &fields,
        );
        let kind = match &self.expressions[input.as_usize()].kind {
            SemanticExpressionKind::CanonicalRead {
                target,
                path,
                projection,
                source,
            } => {
                let mut projection = projection.clone();
                projection.extend(fields.iter().cloned());
                let mut source = source.clone();
                if let Some(source) = &mut source {
                    source.payload_projection.extend(fields.iter().cloned());
                }
                SemanticExpressionKind::CanonicalRead {
                    target: *target,
                    path: path.clone(),
                    projection,
                    source,
                }
            }
            SemanticExpressionKind::MaterializationLocal {
                owner: local_owner,
                local,
                projection,
            } => {
                let mut projection = projection.clone();
                projection.extend(fields);
                SemanticExpressionKind::MaterializationLocal {
                    owner: *local_owner,
                    local: *local,
                    projection,
                }
            }
            _ => SemanticExpressionKind::Project { input, fields },
        };
        let projected = self.push(expression, owner, kind);
        if let Some(ty) = projected_type {
            self.expressions[projected.as_usize()].flow_type.ty = erase_runtime_type_vars(&ty);
        }
        Ok(projected)
    }

    fn evaluation_owner(&self, scoped: ScopedCheckedExpr) -> Option<StaticOwnerId> {
        if let Some(port) = scoped.evaluation_port {
            return self.out_net.owner_for_net(self.out_net.net_for_port(port));
        }
        let expression = self.lookup.expression(self.program, scoped.expression)?;
        let mut scope = Some(expression.scope_id);
        while let Some(scope_id) = scope {
            let checked_scope = self.lookup.scope(self.program, scope_id)?;
            if checked_scope.kind == boon_typecheck::CheckedScopeKind::RepeatedOutput {
                let output = checked_scope.owner?;
                let net = self.out_net.output_net_in_frame(scoped.frame, output)?;
                return self.out_net.owner_for_net(net);
            }
            scope = checked_scope.parent;
        }
        None
    }

    fn concrete_call_result_path(&self, frame: OutCallInstanceId) -> Option<String> {
        let mut ancestry = Vec::new();
        let mut next = Some(frame);
        let mut remaining = self.out_net.call_instances.len().saturating_add(1);
        while let Some(instance) = next {
            if remaining == 0 {
                return None;
            }
            remaining -= 1;
            let instance = self.out_net.call_instances.get(instance.as_usize())?;
            ancestry.push(instance.id);
            next = instance.parent;
        }
        ancestry.reverse();

        let mut result: Option<String> = None;
        for instance_id in ancestry {
            let instance = self.out_net.call_instances.get(instance_id.as_usize())?;
            let local = if let Some(call) = instance.provenance.call_id {
                let checked_path = self.program.result_path_for_call(call)?;
                self.program.semantic_path(checked_path).or_else(|| {
                    self.lookup
                        .declaration(self.program, checked_path.anchor)
                        .is_some_and(|declaration| {
                            declaration.kind == CheckedDeclarationKind::Function
                                && checked_path.projection.is_empty()
                        })
                        .then(String::new)
                })?
            } else {
                self.out_net
                    .producer_root_result_path(instance_id)?
                    .to_owned()
            };
            if local.is_empty() {
                continue;
            }
            result = Some(match result {
                None => local,
                Some(prefix) if local == prefix || local.starts_with(&(prefix.clone() + ".")) => {
                    local
                }
                Some(mut prefix) => {
                    prefix.push('.');
                    prefix.push_str(&local);
                    prefix
                }
            });
        }
        result
    }

    fn concrete_resource_binding_path(
        &self,
        expression: CheckedExprId,
        frame: Option<OutCallInstanceId>,
        _owner: Option<StaticOwnerId>,
    ) -> Option<String> {
        let local = self
            .program
            .sources
            .iter()
            .find(|source| source.expression == expression)
            .and_then(|source| self.program.semantic_path(&source.path))
            .or_else(|| {
                self.program
                    .states
                    .iter()
                    .find(|state| state.expression == expression)
                    .and_then(|state| self.program.semantic_path(&state.path))
            });
        let prefix = frame.and_then(|frame| self.concrete_call_result_path(frame));
        match (prefix, local) {
            (Some(prefix), Some(local))
                if local == prefix || local.starts_with(&(prefix.clone() + ".")) =>
            {
                Some(local)
            }
            (Some(mut prefix), Some(local)) => {
                prefix.push('.');
                prefix.push_str(&local);
                Some(prefix)
            }
            (Some(prefix), None) => Some(prefix),
            (None, local) => local,
        }
    }

    fn push(
        &mut self,
        expression: &CheckedExpression,
        owner: Option<StaticOwnerId>,
        kind: SemanticExpressionKind,
    ) -> SemanticExprId {
        let frame = self.frame_stack.last().copied().flatten();
        let mut flow_type = match &expression.kind {
            CheckedExpressionKind::Call { call } => self
                .out_net
                .call_instance_for_checked_call(*call, frame)
                .map(|instance| {
                    self.out_net.call_instances[instance.as_usize()]
                        .result
                        .clone()
                })
                .unwrap_or_else(|| expression.flow_type.clone()),
            _ => expression.flow_type.clone(),
        };
        if !matches!(&expression.kind, CheckedExpressionKind::Call { .. }) {
            flow_type.ty = concrete_type_in_frame(self.out_net, &flow_type.ty, frame);
        }
        if let Some(ty) = concrete_structural_type(&self.expressions, &kind) {
            flow_type.ty = ty;
        }
        flow_type.ty = erase_runtime_type_vars(&flow_type.ty);
        let resource_binding_path = match expression.kind {
            CheckedExpressionKind::Source
            | CheckedExpressionKind::Hold { .. }
            | CheckedExpressionKind::Latest { .. } => {
                self.concrete_resource_binding_path(expression.id, frame, owner)
            }
            CheckedExpressionKind::Call { .. }
                if (expression.effect.writes_state
                    || expression.effect.emits_source
                    || expression.effect.invokes_host)
                    && (self
                        .program
                        .sources
                        .iter()
                        .any(|source| source.expression == expression.id)
                        || self
                            .program
                            .states
                            .iter()
                            .any(|state| state.expression == expression.id)) =>
            {
                self.concrete_resource_binding_path(expression.id, frame, owner)
            }
            _ => None,
        };
        let provenance = self.value_provenance(expression, owner, &kind);
        let id = SemanticExprId(self.expressions.len());
        self.expressions.push(SemanticExpression {
            id,
            value_id: SemanticValueId(id.as_usize()),
            checked_expr_id: expression.id,
            flow_type,
            effect: expression.effect,
            owner,
            provenance,
            resource_binding_path,
            kind,
        });
        self.checked_expression_origins
            .push(SemanticExpressionOrigin {
                expression: id,
                checked_expression: expression.id,
                checked_scope: expression.scope_id,
                checked_span: expression.span,
                owning_statement: self.current_statement,
                call_instance: frame,
            });
        id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Debug, Eq, PartialEq)]
    enum BodyShape {
        Number(boon_data::ExactNumber),
        Infix(Box<BodyShape>, String, Box<BodyShape>),
        Local(SemanticMaterializationLocalId, Vec<String>),
        Project(Box<BodyShape>, Vec<String>),
    }

    fn semantic_graph(source: &str) -> SemanticExecutionGraphV1 {
        let parsed = boon_parser::parse_source("semantic-contextual-expansion.bn", source).unwrap();
        let checked = boon_typecheck::check_program(&parsed);
        assert!(
            !checked.report.has_errors(),
            "diagnostics: {:#?}",
            checked.report.diagnostics
        );
        crate::elaborate(
            checked
                .program
                .expect("valid fixture has a checked program"),
            &[],
        )
        .expect("valid fixture has a semantic execution graph")
        .execution_graph()
        .clone()
    }

    fn body_shape(graph: &SemanticExecutionGraphV1, expression: SemanticExprId) -> BodyShape {
        match &graph.expressions[expression.as_usize()].kind {
            SemanticExpressionKind::Number(value) => BodyShape::Number(value.clone()),
            SemanticExpressionKind::Infix { left, op, right } => BodyShape::Infix(
                Box::new(body_shape(graph, *left)),
                op.clone(),
                Box::new(body_shape(graph, *right)),
            ),
            SemanticExpressionKind::MaterializationLocal {
                local, projection, ..
            } => BodyShape::Local(*local, projection.clone()),
            SemanticExpressionKind::Project { input, fields } => {
                BodyShape::Project(Box::new(body_shape(graph, *input)), fields.clone())
            }
            other => panic!("unexpected semantic body node: {other:?}"),
        }
    }

    fn only_materialization(
        graph: &SemanticExecutionGraphV1,
    ) -> &SemanticContextualMaterialization {
        let [materialization] = graph.materializations.as_slice() else {
            panic!(
                "expected one semantic materialization, found {}",
                graph.materializations.len()
            );
        };
        materialization
    }

    #[test]
    fn structural_named_field_flow_tracks_exposed_flush_boundary() {
        let source = r#"
store: [
    fail: SOURCE
    value:
        Ready |> HOLD value {
            fail |> THEN {
                FLUSH { InvalidUpdate[position: 1] }
            }
        }
    exposed: value
]
"#;
        let graph = semantic_graph(source);
        let store = graph
            .statements
            .iter()
            .find(|statement| {
                matches!(
                    &statement.kind,
                    SemanticStatementKind::Field { path, .. } if path == "store"
                )
            })
            .expect("store statement");
        let value = store.value.expect("store value");
        let expression = &graph.expressions[value.as_usize()];

        assert_eq!(
            store.flow_type.as_ref(),
            Some(&expression.flow_type),
            "structural statement and expression disagree: {expression:#?}"
        );
        let SemanticExpressionKind::Object(fields) = &expression.kind else {
            panic!("store is not a semantic object: {expression:#?}");
        };
        let exposed = fields
            .iter()
            .find(|field| field.name == "exposed")
            .expect("exposed field");
        assert!(
            matches!(
                &graph.expressions[exposed.value.as_usize()].flow_type.ty,
                Type::VariantSet(variants)
                    if variants.iter().any(|variant| matches!(
                        variant,
                        boon_typecheck::Variant::Tagged { tag, .. } if tag == "InvalidUpdate"
                    ))
            ),
            "exposed FLUSH boundary expression: {:#?}",
            graph.expressions[exposed.value.as_usize()]
        );
    }

    #[test]
    fn direct_and_transparent_wrappers_normalize_to_the_same_contextual_body() {
        let direct = semantic_graph(
            r#"
rows: LIST { [value: 1] }
result:
    rows
    |> List/map(
        item
        new: (item.value + 1) * 2
    )
"#,
        );
        let wrapped = semantic_graph(
            r#"
FUNCTION doubled(list, entry: OUT, new) {
    list
    |> List/map(
        item: entry
        new: new * 2
    )
}

rows: LIST { [value: 1] }
result:
    rows
    |> doubled(
        entry
        new: entry.value + 1
    )
"#,
        );
        let multiply_wrapped = semantic_graph(
            r#"
FUNCTION doubled(list, entry: OUT, new) {
    list
    |> List/map(
        item: entry
        new: new * 2
    )
}

FUNCTION outer(list, row: OUT, new) {
    list
    |> doubled(
        entry: row
        new: new
    )
}

rows: LIST { [value: 1] }
result:
    rows
    |> outer(
        row
        new: row.value + 1
    )
"#,
        );

        let direct_materialization = only_materialization(&direct);
        let wrapped_materialization = only_materialization(&wrapped);
        let multiply_wrapped_materialization = only_materialization(&multiply_wrapped);
        let expected = body_shape(&direct, direct_materialization.body);
        assert_eq!(body_shape(&wrapped, wrapped_materialization.body), expected);
        assert_eq!(
            body_shape(&multiply_wrapped, multiply_wrapped_materialization.body),
            expected
        );
        assert_eq!(
            direct_materialization.operation,
            SemanticContextualOperationKind::Map
        );
        assert_eq!(
            wrapped_materialization.operation,
            direct_materialization.operation
        );
        assert_eq!(
            multiply_wrapped_materialization.operation,
            direct_materialization.operation
        );
        assert_eq!(
            direct.expressions[direct_materialization.body.as_usize()].owner,
            Some(direct_materialization.owner)
        );
        assert_eq!(
            wrapped.expressions[wrapped_materialization.body.as_usize()].owner,
            Some(wrapped_materialization.owner)
        );
        assert_eq!(
            multiply_wrapped.expressions[multiply_wrapped_materialization.body.as_usize()].owner,
            Some(multiply_wrapped_materialization.owner)
        );
    }

    #[test]
    fn deeply_nested_transparent_calls_expand_on_the_default_stack() {
        const WRAPPER_COUNT: usize = 128;
        let mut source = String::new();
        source.push_str(
            r#"
FUNCTION wrapper_0(list, row: OUT, new) {
    list
    |> List/map(
        item: row
        new: new
    )
}
"#,
        );
        for index in 1..WRAPPER_COUNT {
            source.push_str(&format!(
                r#"
FUNCTION wrapper_{index}(list, row: OUT, new) {{
    list
    |> wrapper_{}(
        row: row
        new: new
    )
}}
"#,
                index - 1
            ));
        }
        source.push_str(&format!(
            r#"
rows: LIST {{ [value: 1] }}
result:
    rows
    |> wrapper_{}(
        row
        new: row.value + 1
    )
"#,
            WRAPPER_COUNT - 1
        ));

        let graph = semantic_graph(&source);
        let materialization = only_materialization(&graph);
        assert_eq!(
            materialization.operation,
            SemanticContextualOperationKind::Map
        );
        assert_eq!(
            graph.expressions[materialization.body.as_usize()].owner,
            Some(materialization.owner)
        );
    }
}
